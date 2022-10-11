pub mod bootloader;
pub mod logger;

use crate::{
    arch::{
        paging::{PageDir, PageTable},
        PAGE_SIZE,
    },
    mm::{
        bump_alloc::{bump_alloc, init_bump_alloc},
        heap::{ExpandAllocCallback, ExpandFreeCallback, ALLOCATOR},
        paging::{get_page_manager, set_page_manager, PageDirectory, PageManager},
    },
    util::{
        array::BitSet,
        debug::DebugArray,
        tar::{EntryKind, TarIterator},
    },
};
use alloc::{
    alloc::Layout,
    boxed::Box,
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};
use compression::prelude::*;
use core::{arch::asm, mem::size_of};
use log::{debug, error, info, trace};

pub const LINKED_BASE: usize = 0xe0000000;
pub const KHEAP_START: usize = LINKED_BASE + 0x01000000;
pub const KHEAP_INITIAL_SIZE: usize = 0x100000;
pub const KHEAP_MAX_SIZE: usize = 0xffff000;
pub const HEAP_MIN_SIZE: usize = 0x70000;

//static mut PAGE_MANAGER: Option<PageManager<PageDir>> = None;
static mut PAGE_DIR: Option<PageDir> = None;

static mut BOOTSTRAP_ADDR: u64 = 0;

extern "C" {
    /// located at end of loader, used for more efficient memory mappings
    static kernel_end: u8;

    /// base of the stack, used to map out the page below to catch stack overflow
    static stack_base: u8;

    /// top of the stack
    static stack_end: u8;

    /// base of the interrupt handler stack
    static int_stack_base: u8;

    /// top of the interrupt handler stack
    static int_stack_end: u8;
}

/// initialize paging, just cleanly map our kernel to 3.5gb
#[no_mangle]
pub extern "C" fn x86_prep_page_table(buf: &mut [u32; 1024]) {
    for i in 0u32..1024 {
        buf[i as usize] = i * PAGE_SIZE as u32 + 3;
    }

    buf[((unsafe { (&int_stack_base as *const _) as usize } - LINKED_BASE) / PAGE_SIZE) - 1] = 0;
    buf[((unsafe { (&stack_base as *const _) as usize } - LINKED_BASE) / PAGE_SIZE) - 1] = 0;
}

/// gets the physical address for bootstrap code for other cpus
pub fn get_cpu_bootstrap_addr() -> u64 {
    unsafe { BOOTSTRAP_ADDR }
}

#[no_mangle]
pub fn kmain() {
    logger::init().unwrap();

    unsafe {
        bootloader::pre_init();
    }

    unsafe {
        crate::arch::ints::init();
        crate::arch::gdt::init((&int_stack_end as *const _) as u32);
    }

    let kernel_end_pos = unsafe { (&kernel_end as *const _) as usize };
    let stack_base_pos = unsafe { (&stack_base as *const _) as usize };
    let stack_end_pos = unsafe { (&stack_end as *const _) as usize };
    let int_stack_base_pos = unsafe { (&int_stack_base as *const _) as usize };
    let int_stack_end_pos = unsafe { (&int_stack_end as *const _) as usize };

    // === multiboot pre-init ===

    let mem_size = bootloader::init();
    let mem_size_pages: usize = (mem_size / PAGE_SIZE as u64).try_into().unwrap();

    // === paging init ===

    // initialize the bump allocator so we can allocate initial memory for paging
    unsafe {
        init_bump_alloc(LINKED_BASE);
    }

    // initialize the pagemanager to manage our page allocations
    set_page_manager(PageManager::new({
        let layout = Layout::new::<u32>();
        let ptr = unsafe {
            bump_alloc::<u32>(Layout::from_size_align(mem_size_pages / 32 * layout.size(), layout.align()).unwrap())
                .unwrap()
                .pointer
        };
        let mut bitset = BitSet::place_at(ptr, mem_size_pages);
        bitset.clear_all();
        bootloader::reserve_pages(&mut bitset);
        bitset
    }));

    // grab a reference to the page manager so we don't have to continuously lock and unlock it while we're doing initial memory allocations
    let mut manager = get_page_manager();

    // page directory for kernel
    let mut page_dir = PageDir::bump_allocate();

    let heap_reserved = PAGE_SIZE * 2;

    let kernel_start = LINKED_BASE + 0x100000;

    // allocate pages
    debug!("mapping kernel ({kernel_start:#x} - {kernel_end_pos:#x})");

    for addr in (kernel_start..kernel_end_pos).step_by(PAGE_SIZE) {
        if !page_dir.has_page_table(addr.try_into().unwrap()) {
            debug!("allocating new page table");
            let ptr = unsafe { bump_alloc::<PageTable>(Layout::from_size_align(size_of::<PageTable>(), PAGE_SIZE).unwrap()).unwrap() };
            page_dir.add_page_table(addr.try_into().unwrap(), unsafe { &mut *ptr.pointer }, ptr.phys_addr.try_into().unwrap(), false);
        }

        manager.alloc_frame_at(&mut page_dir, addr, (addr - LINKED_BASE) as u64, false, true).unwrap();
    }

    // free the page below the stack, to catch stack overflow
    debug!("stack @ {stack_base_pos:#x} - {stack_end_pos:#x}");
    manager.free_frame(&mut page_dir, stack_base_pos - PAGE_SIZE).unwrap();

    debug!("interrupt stack @ {int_stack_base_pos:#x} - {int_stack_end_pos:#x}");
    manager.free_frame(&mut page_dir, int_stack_base_pos - PAGE_SIZE).unwrap();

    // set aside some memory for bootstrapping other CPUs
    //let bootstrap_addr = manager.first_available_frame(&page_dir).unwrap();
    let bootstrap_addr = 0x1000;
    manager.set_frame_used(&page_dir, bootstrap_addr);

    debug!("bootstrap code @ {bootstrap_addr:#x}");

    unsafe {
        BOOTSTRAP_ADDR = bootstrap_addr;
    }

    let heap_init_end = KHEAP_START + HEAP_MIN_SIZE;
    debug!("mapping heap ({KHEAP_START:#x} - {heap_init_end:#x})");

    for addr in (KHEAP_START..heap_init_end).step_by(PAGE_SIZE) {
        if !page_dir.has_page_table(addr.try_into().unwrap()) {
            debug!("allocating new page table");
            let ptr = unsafe { bump_alloc::<PageTable>(Layout::from_size_align(size_of::<PageTable>(), PAGE_SIZE).unwrap()).unwrap() };
            page_dir.add_page_table(addr.try_into().unwrap(), unsafe { &mut *ptr.pointer }, ptr.phys_addr.try_into().unwrap(), false);
        }

        manager.alloc_frame(&mut page_dir, addr, false, true).unwrap();
    }

    // let go of our lock on the global page manager, since it would likely cause problems with the allocator
    drop(manager);

    // switch to our new page directory so all the pages we've just mapped will be accessible
    unsafe {
        // if we don't set this as global state something breaks, haven't bothered figuring out what yet
        PAGE_DIR = Some(page_dir);

        PAGE_DIR.as_ref().unwrap().switch_to();
    }

    // === heap init ===

    // set up allocator with minimum size
    ALLOCATOR.init(KHEAP_START, HEAP_MIN_SIZE);

    ALLOCATOR.reserve_memory(Some(Layout::from_size_align(heap_reserved, PAGE_SIZE).unwrap()));

    fn expand(old_top: usize, new_top: usize, alloc: &ExpandAllocCallback, _free: &ExpandFreeCallback) -> Result<usize, ()> {
        debug!("expand (old_top: {old_top:#x}, new_top: {new_top:#x})");
        if new_top <= KHEAP_START + KHEAP_MAX_SIZE {
            let new_top = (new_top / PAGE_SIZE) * PAGE_SIZE + PAGE_SIZE;
            debug!("new_top now @ {new_top:#x}");

            let old_top = (old_top / PAGE_SIZE) * PAGE_SIZE;
            debug!("old_top now @ {old_top:#x}");

            let dir = unsafe { PAGE_DIR.as_mut().unwrap() };

            for addr in (old_top..new_top).step_by(PAGE_SIZE) {
                if !dir.has_page_table(addr.try_into().unwrap()) {
                    trace!("allocating new page table");

                    let virt = match alloc(Layout::from_size_align(size_of::<PageTable>(), PAGE_SIZE).unwrap()) {
                        Ok(ptr) => ptr,
                        Err(()) => return Ok(addr), // fail gracefully if we can't allocate
                    };
                    let phys = dir.virt_to_phys(virt as usize).ok_or(())?;

                    dir.add_page_table(addr.try_into().unwrap(), unsafe { &mut *(virt as *mut PageTable) }, phys.try_into().unwrap(), true);
                }

                get_page_manager().alloc_frame(dir, addr, false, true).map_err(|err| {
                    error!("error allocating page for heap: {err:?}");
                })?;
            }

            Ok(new_top)
        } else {
            Err(())
        }
    }

    ALLOCATOR.set_expand_callback(&expand);

    unsafe {
        crate::mm::bump_alloc::free_unused_bump_alloc(&mut get_page_manager(), PAGE_DIR.as_mut().unwrap());
    }

    get_page_manager().print_free();

    // === enable interrupts ===

    unsafe {
        asm!("sti");
    }

    // === multiboot init after heap init ===

    unsafe {
        bootloader::init_after_heap(&mut get_page_manager(), PAGE_DIR.as_mut().unwrap());
    }

    let info = bootloader::get_multiboot_info();

    debug!("{info:?}");

    // === discover modules ===

    if info.mods.is_none() || info.mods.as_ref().unwrap().is_empty() {
        panic!("no modules found, cannot continue booting");
    }

    let bootloader_modules = info.mods.as_ref().unwrap();

    let mut modules: BTreeMap<String, &'static [u8]> = BTreeMap::new();

    fn discover_module(modules: &mut BTreeMap<String, &'static [u8]>, name: String, data: &'static [u8]) {
        debug!("found module {name:?}: {:?}", DebugArray(data));

        match name.split('.').last() {
            Some("tar") => {
                info!("discovering all files in {name:?} as modules");

                for entry in TarIterator::new(data) {
                    if entry.header.kind() == EntryKind::NormalFile {
                        discover_module(modules, entry.header.name().to_string(), entry.contents);
                    }
                }
            }
            Some("bz2") => {
                // remove the extension from the name of the compressed file
                let new_name = {
                    let mut split: Vec<&str> = name.split('.').collect();
                    split.pop();
                    split.join(".")
                };

                info!("decompressing {name:?} as {new_name:?}");

                match data.iter().cloned().decode(&mut BZip2Decoder::new()).collect::<Result<Vec<_>, _>>() {
                    // Box::leak() prevents the decompressed data from being dropped, giving it the 'static lifetime since it doesn't
                    // contain any references to anything else
                    Ok(decompressed) => discover_module(modules, new_name, Box::leak(decompressed.into_boxed_slice())),
                    Err(err) => error!("error decompressing {name}: {err:?}"),
                }
            }
            Some("gz") => {
                let new_name = {
                    let mut split: Vec<&str> = name.split('.').collect();
                    split.pop();
                    split.join(".")
                };

                info!("decompressing {name:?} as {new_name:?}");

                match data.iter().cloned().decode(&mut GZipDecoder::new()).collect::<Result<Vec<_>, _>>() {
                    Ok(decompressed) => discover_module(modules, new_name, Box::leak(decompressed.into_boxed_slice())),
                    Err(err) => error!("error decompressing {name}: {err:?}"),
                }
            }
            // no special handling for this file, assume it's a module
            _ => {
                modules.insert(name, data);
            }
        }
    }

    for module in bootloader_modules.iter() {
        discover_module(&mut modules, module.string().to_string(), module.data());
    }

    // === add special modules ===

    // add cmdline module and parse cmdline at the same time
    let cmdline = bootloader::get_multiboot_info().cmdline.filter(|s| !s.is_empty()).map(|cmdline| {
        modules.insert("*cmdline".to_string(), cmdline.as_bytes());

        info!("kernel command line: {:?}", cmdline);

        let mut map = BTreeMap::new();

        for arg in cmdline.split(' ') {
            if !arg.is_empty() {
                let arg = arg.split('=').collect::<Vec<_>>();
                map.insert(arg[0], arg.get(1).copied().unwrap_or(""));
            }
        }

        map
    });

    debug!("{:?}", cmdline);

    // === print module info ===

    let mut num_modules = 0;
    let mut max_len = 0;
    for (name, _) in modules.iter() {
        num_modules += 1;
        if name.len() > max_len {
            max_len = name.len();
        }
    }

    if num_modules == 1 {
        info!("1 module:");
    } else {
        info!("{num_modules} modules:");
    }

    for (name, data) in modules.iter() {
        let size = if data.len() > 1024 * 1024 * 10 {
            format!("{} MB", data.len() / 1024 / 1024)
        } else if data.len() > 1024 * 10 {
            format!("{} KB", data.len() / 1024)
        } else {
            format!("{} B", data.len())
        };
        info!("\t{name:max_len$} : {size}");
    }

    get_page_manager().print_free();

    crate::arch::init(unsafe { PAGE_DIR.as_mut().unwrap() }, cmdline);

    /*let timer = crate::timer::get_timer(0).unwrap();
    timer.add_timer_in(timer.hz(), test_timer_callback).unwrap();

    loop {
        crate::arch::halt_until_interrupt();
    }*/
}

/*fn test_timer_callback(_num: usize, _cpu: Option<crate::task::cpu::ThreadID>, _regs: &mut crate::arch::Registers) {
    info!("timed out!");

    let timer = crate::timer::get_timer(0).unwrap();
    timer.add_timer_in(timer.hz(), test_timer_callback).unwrap();
}*/
