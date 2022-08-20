pub mod bootloader;
pub mod ints;
pub mod logger;

use crate::{
    bump_alloc, init_bump_alloc,
    tar::{EntryKind, TarIterator},
    ALLOCATOR, NAME, VERSION,
};
use alloc::{
    alloc::{alloc, Layout},
    boxed::Box,
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use common::{
    arch::{
        paging::{PageDir, PageDirEntry, PageTable, TableRef},
        LINKED_BASE, PAGE_SIZE,
    },
    mm::paging::{PageDirectory, PageError, PageFrame, PageManager},
    util::{array::BitSet, DebugArray},
    BootModule, MemoryKind, MemoryRegion,
};
use compression::prelude::*;
use core::mem::size_of;
use goblin::elf::{program_header::PT_LOAD, Elf};
use log::{debug, error, info, trace, warn};

pub const KHEAP_START: usize = LINKED_BASE + 0x10000000;
pub const KHEAP_INITIAL_SIZE: usize = 0x100000;
pub const KHEAP_MAX_SIZE: usize = 0xffff000;
pub const HEAP_MIN_SIZE: usize = 0x70000;

extern "C" {
    /// located at end of loader, used for more efficient memory mappings
    static kernel_end: u32;
}

/// initialize paging, just cleanly map our kernel to 3gb
#[no_mangle]
pub extern "C" fn x86_prep_page_table(buf: &mut [u32; 1024]) {
    for i in 0u32..1024 {
        buf[i as usize] = i * 0x1000 + 3;
    }
}

static mut PAGE_MANAGER: Option<PageManager<PageDir>> = None;
static mut LOADER_DIR: Option<PageDir> = None;

#[no_mangle]
pub fn kmain() {
    // initialize our logger
    logger::init().unwrap();
    unsafe {
        logger::init_vga(core::slice::from_raw_parts_mut((LINKED_BASE + 0xb8000) as *mut u16, 80 * 25), 80, 25);
    }

    // initialize interrupts so we can catch exceptions
    unsafe {
        ints::init();
    }

    info!("{} v{}", NAME, VERSION);

    let kernel_end_pos = unsafe { (&kernel_end as *const _) as usize };

    // === multiboot pre-init ===

    let mem_size = bootloader::init();
    let mem_size_pages: usize = (mem_size / PAGE_SIZE as u64).try_into().unwrap();

    // === paging init ===

    // initialize the bump allocator so we can allocate initial memory for paging
    unsafe {
        init_bump_alloc(LINKED_BASE);
    }

    // create a pagemanager to manage our page allocations
    let mut manager: PageManager<PageDir> = PageManager::new({
        let layout = Layout::new::<u32>();
        let ptr = unsafe { bump_alloc::<u32>(Layout::from_size_align(mem_size_pages / 32 * layout.size(), layout.align()).unwrap()).pointer };
        let mut bitset = BitSet::place_at(ptr, mem_size_pages);
        bitset.clear_all();
        bootloader::reserve_pages(&mut bitset);
        bitset
    });

    // page directory for loader
    let mut loader_dir = {
        let layout = Layout::from_size_align(size_of::<[Option<TableRef<'static>>; 1024]>(), PAGE_SIZE).unwrap();
        let tables = unsafe { &mut *bump_alloc::<[Option<TableRef<'static>>; 1024]>(layout).pointer };
        for table_ref in tables.iter_mut() {
            *table_ref = None;
        }

        let ptr = unsafe { bump_alloc::<[PageDirEntry; 1024]>(Layout::from_size_align(size_of::<[PageDirEntry; 1024]>(), PAGE_SIZE).unwrap()) };

        PageDir::from_allocated(tables, unsafe { &mut *ptr.pointer }, ptr.phys_addr.try_into().unwrap())
    };

    let heap_reserved = PAGE_SIZE * 2;

    // allocate pages
    debug!("mapping loader ({:#x} - {:#x})", LINKED_BASE, kernel_end_pos);

    for addr in (LINKED_BASE..kernel_end_pos).step_by(PAGE_SIZE) {
        if !loader_dir.has_page_table(addr.try_into().unwrap()) {
            debug!("allocating new page table");
            let ptr = unsafe { bump_alloc::<PageTable>(Layout::from_size_align(size_of::<PageTable>(), PAGE_SIZE).unwrap()) };
            loader_dir.add_page_table(addr.try_into().unwrap(), unsafe { &mut *ptr.pointer }, ptr.phys_addr.try_into().unwrap(), false);
        }

        manager.alloc_frame_at(&mut loader_dir, addr, (addr - LINKED_BASE) as u64, false, true).unwrap();
    }

    debug!("mapping heap ({:#x} - {:#x})", KHEAP_START, KHEAP_START + heap_reserved);

    for addr in (KHEAP_START..KHEAP_START + heap_reserved).step_by(PAGE_SIZE) {
        if !loader_dir.has_page_table(addr.try_into().unwrap()) {
            debug!("allocating new page table");
            let ptr = unsafe { bump_alloc::<PageTable>(Layout::from_size_align(size_of::<PageTable>(), PAGE_SIZE).unwrap()) };
            loader_dir.add_page_table(addr.try_into().unwrap(), unsafe { &mut *ptr.pointer }, ptr.phys_addr.try_into().unwrap(), false);
        }

        manager.alloc_frame(&mut loader_dir, addr, false, true).unwrap();
    }

    // switch to our new page directory so all the pages we've just mapped will be accessible
    unsafe {
        // if we don't set this as global state something breaks, haven't bothered figuring out what yet
        LOADER_DIR = Some(loader_dir);

        LOADER_DIR.as_ref().unwrap().switch_to();

        PAGE_MANAGER = Some(manager);
    }

    // === heap init ===

    // set up allocator with minimum size
    ALLOCATOR.init(KHEAP_START, heap_reserved);

    ALLOCATOR.reserve_memory(Some(Layout::from_size_align(heap_reserved, PAGE_SIZE).unwrap()));

    fn expand(old_top: usize, new_top: usize, alloc: &dyn Fn(Layout) -> Result<*mut u8, ()>, _free: &dyn Fn(*mut u8, Layout)) -> Result<usize, ()> {
        debug!("expand (old_top: {:#x}, new_top: {:#x})", old_top, new_top);
        if new_top <= KHEAP_START + KHEAP_MAX_SIZE {
            let new_top = (new_top / PAGE_SIZE) * PAGE_SIZE + PAGE_SIZE;
            debug!("new_top now @ {:#x}", new_top);

            let old_top = (old_top / PAGE_SIZE) * PAGE_SIZE;
            debug!("old_top now @ {:#x}", old_top);

            let dir = unsafe { LOADER_DIR.as_mut().unwrap() };

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

                unsafe {
                    PAGE_MANAGER.as_mut().unwrap().alloc_frame(dir, addr, false, true).map_err(|err| {
                        error!("error allocating page for heap: {:?}", err);
                    })?;
                }
            }

            Ok(new_top)
        } else {
            Err(())
        }
    }

    ALLOCATOR.set_expand_callback(&expand);

    unsafe {
        PAGE_MANAGER.as_mut().unwrap().print_free();
    }

    // === multiboot init after heap init ===

    unsafe {
        bootloader::init_after_heap(PAGE_MANAGER.as_mut().unwrap(), LOADER_DIR.as_mut().unwrap());
    }

    let info = bootloader::get_multiboot_info();

    debug!("{:?}", info);

    // === module discovery ===

    if info.mods.is_none() || info.mods.as_ref().unwrap().is_empty() {
        panic!("no modules have been passed to loader, cannot continue booting");
    }

    let bootloader_modules = info.mods.as_ref().unwrap();

    let mut modules: BTreeMap<String, &'static [u8]> = BTreeMap::new();

    fn discover_module(modules: &mut BTreeMap<String, &'static [u8]>, name: String, data: &'static [u8]) {
        debug!("found module {:?}: {:?}", name, DebugArray(data));

        match name.split('.').last() {
            Some("tar") => {
                info!("discovering all files in {:?} as modules", name);

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

                info!("decompressing {:?} as {:?}", name, new_name);

                match data.iter().cloned().decode(&mut BZip2Decoder::new()).collect::<Result<Vec<_>, _>>() {
                    // Box::leak() prevents the decompressed data from being dropped, giving it the 'static lifetime since it doesn't
                    // contain any references to anything else
                    Ok(decompressed) => discover_module(modules, new_name, Box::leak(decompressed.into_boxed_slice())),
                    Err(err) => error!("error decompressing {}: {:?}", name, err),
                }
            }
            Some("gz") => {
                let new_name = {
                    let mut split: Vec<&str> = name.split('.').collect();
                    split.pop();
                    split.join(".")
                };

                info!("decompressing {:?} as {:?}", name, new_name);

                match data.iter().cloned().decode(&mut GZipDecoder::new()).collect::<Result<Vec<_>, _>>() {
                    Ok(decompressed) => discover_module(modules, new_name, Box::leak(decompressed.into_boxed_slice())),
                    Err(err) => error!("error decompressing {}: {:?}", name, err),
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
        info!("{} modules:", num_modules);
    }

    for (name, data) in modules.iter() {
        let size = if data.len() > 1024 * 1024 * 10 {
            format!("{} MB", data.len() / 1024 / 1024)
        } else if data.len() > 1024 * 10 {
            format!("{} KB", data.len() / 1024)
        } else {
            format!("{} B", data.len())
        };
        info!("\t{:width$} : {}", name, size, width = max_len);
    }

    unsafe {
        PAGE_MANAGER.as_mut().unwrap().print_free();
    }

    // === load kernel from elf ===

    let default_kernel_name = "kernel";

    // find an argument matching "kernel=..." and use that if available, else default to the default kernel name
    let kernel_name = {
        let kernel_name = cmdline.and_then(|map| map.get("kernel").copied()).unwrap_or(default_kernel_name);

        // check if we have a module with this name
        if modules.contains_key(kernel_name) {
            kernel_name
        } else {
            warn!("couldn't find module {:?}, trying {:?}", kernel_name, default_kernel_name);

            default_kernel_name
        }
    };

    info!("loading module {:?} as kernel", kernel_name);

    let kernel_data = modules.get(kernel_name).unwrap_or_else(|| panic!("couldn't find module with name {}", kernel_name));

    let elf = Elf::parse(kernel_data).expect("failed to parse kernel header");

    if elf.is_64 && size_of::<usize>() != 64 / 8 {
        panic!("cannot load 64 bit executable on non 64 bit system");
    } else if elf.dynamic.is_some() {
        panic!("cannot load dynamically linked binary as kernel");
    } else if elf.interpreter.is_some() {
        panic!("cannot load interpreted binary as kernel");
    }

    // === assemble program in memory ===

    let mut kernel_dir = PageDir::new();

    let mut lowest_addr = usize::MAX;

    for ph in elf.program_headers {
        debug!("{:?}", ph);

        match ph.p_type {
            PT_LOAD => {
                let file_start: usize = ph.p_offset.try_into().unwrap();
                let file_end: usize = (ph.p_offset + ph.p_filesz).try_into().unwrap();

                let filesz: usize = ph.p_filesz.try_into().unwrap();
                let memsz: usize = ph.p_memsz.try_into().unwrap();

                let vaddr: usize = ph.p_vaddr.try_into().unwrap();

                if vaddr < lowest_addr {
                    lowest_addr = vaddr;
                }

                let data: Vec<u8> = if filesz > 0 {
                    let mut data = vec![0; filesz];

                    data.clone_from_slice(&kernel_data[file_start..file_end]);

                    // bit inefficient but it works
                    for _i in filesz..memsz {
                        data.push(0);
                    }

                    assert!(data.len() == memsz);

                    data
                } else {
                    vec![0; memsz]
                };

                debug!("data @ {:#x} - {:#x}", vaddr, vaddr + memsz);

                unsafe {
                    let vaddr_align = vaddr / PAGE_SIZE * PAGE_SIZE;

                    for addr in (vaddr_align..=vaddr_align + (memsz / PAGE_SIZE) * PAGE_SIZE + PAGE_SIZE).step_by(PAGE_SIZE) {
                        match PAGE_MANAGER.as_mut().unwrap().alloc_frame(&mut kernel_dir, addr, false, ph.is_write()) {
                            Ok(_) => (),
                            Err(PageError::FrameInUse) => {
                                // if this region is writable, and the page already allocated here is not, fix that
                                // it's better to have regions that are writable when they shouldn't be than have regions that are the other way around
                                if ph.is_write() {
                                    let mut page = kernel_dir.get_page(addr).unwrap();
                                    if !page.writable {
                                        debug!("fixing page @ {:#x} to be writable", addr);
                                        page.writable = true;
                                        kernel_dir.set_page(addr, Some(page)).unwrap();
                                    }
                                }
                            }
                            Err(err) => panic!("failed to allocate memory for kernel: {:?}", err),
                        }
                    }

                    LOADER_DIR
                        .as_mut()
                        .unwrap()
                        .map_memory_from(&mut kernel_dir, vaddr, memsz, |s| s.clone_from_slice(&data))
                        .expect("failed to populate kernel's memory");
                }
            }
            _ => debug!("unknown program header {:?}", ph.p_type),
        }
    }

    // === load assembly shim to jump to and start kernel ===

    // small assembly shim to switch page directories and call the kernel
    let exec_kernel = include_bytes!("../../../../target/exec_kernel.bin");

    // round up to page size
    let exec_kernel_size = (exec_kernel.len() / PAGE_SIZE + 1) * PAGE_SIZE;

    // address we're loading the shim at
    let exec_kernel_addr = usize::MAX - exec_kernel_size + 1;

    // function pointer to the shim
    let exec_kernel_ptr: unsafe extern "cdecl" fn(u32, u32, u32) -> ! = unsafe { core::mem::transmute(exec_kernel_addr) };

    debug!("exec_kernel @ {:#x}, size {:#x}", exec_kernel_addr, exec_kernel_size);

    // allocate memory for shim
    debug!("allocating memory for exec_kernel");
    for addr in (exec_kernel_addr..exec_kernel_addr + exec_kernel.len()).step_by(PAGE_SIZE) {
        unsafe {
            PAGE_MANAGER
                .as_mut()
                .unwrap()
                .alloc_frame(LOADER_DIR.as_mut().unwrap(), addr, false, true)
                .expect("failed to allocate memory for exec_kernel");
            PAGE_MANAGER
                .as_mut()
                .unwrap()
                .alloc_frame(&mut kernel_dir, addr, false, false)
                .expect("failed to allocate memory for exec_kernel");
        }
    }

    // insert the shim into kernel memory
    debug!("copying exec_kernel in kernel");
    unsafe {
        LOADER_DIR
            .as_mut()
            .unwrap()
            .map_memory_from(&mut kernel_dir, exec_kernel_addr, exec_kernel.len(), |s| s.clone_from_slice(exec_kernel))
            .expect("failed to populate kernel's memory");
    }

    // insert the shim into loader memory
    debug!("copying exec_kernel in loader");
    unsafe {
        core::slice::from_raw_parts_mut(exec_kernel_addr as *mut u8, exec_kernel.len()).clone_from_slice(exec_kernel);
    }

    // === allocate kernel's stack ===

    debug!("allocating stack");

    // the top address of the stack
    let stack_top = exec_kernel_addr - 1;
    let stack_size = PAGE_SIZE * 16;
    let stack_bottom = exec_kernel_addr - stack_size;

    // allocate memory for kernel stack
    for addr in (stack_bottom..stack_top).step_by(PAGE_SIZE) {
        unsafe {
            PAGE_MANAGER
                .as_mut()
                .unwrap()
                .alloc_frame(&mut kernel_dir, addr, false, true)
                .expect("failed to allocate memory for kernel stack");
        }
    }

    // === prepare modules for passing to the kernel ===

    debug!("preparing modules for kernel");

    let (modules_addr, new_modules) = if modules.len() > 1 {
        // list of start indices and lengths into our big table of strings
        let mut indices: Vec<(usize, usize)> = Vec::new();

        // the name of every module in order
        let string_bank: Vec<&str> = modules.iter().filter(|(name, _data)| name != &kernel_name).map(|(name, _data)| name.as_ref()).collect();

        // populate indices with the positions of the strings in our big string list
        let mut index = 0;
        for string in &string_bank {
            indices.push((index, string.len()));
            index += string.len();
        }

        let module_names = string_bank.join("");

        debug!("{:?}, {:?}", module_names, indices);

        // copy module names into kernel's memory
        let module_names_len = module_names.as_bytes().len();
        let module_names_hole = kernel_dir
            .find_hole(lowest_addr, stack_bottom, module_names_len)
            .expect("couldn't find space in kernel's page directory");

        for addr in (module_names_hole..module_names_hole + module_names_len).step_by(PAGE_SIZE) {
            unsafe {
                PAGE_MANAGER
                    .as_mut()
                    .unwrap()
                    .alloc_frame(&mut kernel_dir, addr, false, true)
                    .expect("failed to allocate memory for module names");
            }
        }

        unsafe {
            LOADER_DIR
                .as_mut()
                .unwrap()
                .map_memory_from(&mut kernel_dir, module_names_hole, module_names_len, |s| s.clone_from_slice(module_names.as_bytes()))
                .expect("failed to populate kernel's memory");
        }

        let new_modules = modules
            .iter()
            .filter(|(name, _data)| name != &kernel_name)
            .enumerate()
            .map(|(i, (_name, data))| {
                let start = &data[0] as *const u8 as usize;
                let end = &data[data.len() - 1] as *const u8 as usize;

                let start_phys = unsafe { LOADER_DIR.as_ref().unwrap().virt_to_phys(start).expect("couldn't get physical address for module") };
                let end_phys = unsafe { LOADER_DIR.as_ref().unwrap().virt_to_phys(end).expect("couldn't get physical address for module") };

                let (name_start, name_len) = indices[i];

                BootModule {
                    start: start_phys,
                    end: end_phys,
                    string: unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts((module_names_hole + name_start) as *const u8, name_len)) },
                }
            })
            .collect::<Vec<_>>();

        //debug!("new_modules: {:?}", new_modules);

        let new_modules_len = new_modules.len() * size_of::<BootModule>();
        let new_modules_hole = kernel_dir
            .find_hole(lowest_addr, stack_bottom, new_modules_len)
            .expect("couldn't find space in kernel's page directory");

        debug!("new_modules @ {:#x}", new_modules_hole);

        for addr in (new_modules_hole..new_modules_hole + new_modules_len).step_by(PAGE_SIZE) {
            unsafe {
                PAGE_MANAGER
                    .as_mut()
                    .unwrap()
                    .alloc_frame(&mut kernel_dir, addr, false, true)
                    .expect("failed to allocate memory for module names");
            }
        }

        unsafe {
            LOADER_DIR
                .as_mut()
                .unwrap()
                .map_memory_from(&mut kernel_dir, new_modules_hole, new_modules_len, |s| {
                    s.clone_from_slice(core::slice::from_raw_parts(new_modules.as_slice() as *const _ as *const u8, new_modules_len))
                })
                .expect("failed to populate kernel's memory");
        }

        (new_modules_hole, new_modules)
    } else {
        (0, vec![])
    };

    // === create list of memory regions ===

    // get reserved regions from bootloader
    let mut regions = match &bootloader::get_multiboot_info().memory_map {
        Some(regions) => regions.iter().map(|&r| r.into()).collect::<Vec<MemoryRegion>>(),
        None => vec![],
    };

    // reserve memory of modules
    regions.append(
        &mut new_modules
            .iter()
            .map(|m| MemoryRegion {
                start: m.start,
                end: m.end,
                kind: MemoryKind::ReservedReclaimable,
            })
            .collect::<Vec<_>>(),
    );

    // sort and slice modules so none of them overlap
    let regions = common::sort_regions(regions, PAGE_SIZE as u64);

    debug!("memory regions: {:?}", regions);

    // === map memory regions into kernel ===

    debug!("mapping memory regions");

    let regions_size = regions.len() * size_of::<MemoryRegion>();
    let regions_hole = kernel_dir.find_hole(lowest_addr, stack_bottom, regions_size).expect("couldn't find space in kernel's page directory");

    for addr in (regions_hole..regions_hole + regions_size).step_by(PAGE_SIZE) {
        unsafe {
            PAGE_MANAGER
                .as_mut()
                .unwrap()
                .alloc_frame(&mut kernel_dir, addr, false, true)
                .expect("failed to allocate memory for module names");
        }
    }

    unsafe {
        LOADER_DIR
            .as_mut()
            .unwrap()
            .map_memory_from(&mut kernel_dir, regions_hole, regions_size, |s| {
                s.clone_from_slice(core::slice::from_raw_parts(regions.as_slice() as *const _ as *const u8, regions_size))
            })
            .expect("failed to populate kernel's memory");
    }

    // === map the kernel's page directory into itself ===

    // we can create a new tables array by mapping its tables_physical entries into its address space, then populate the tables array
    // with the new virtual addresses

    debug!("mapping page directory");

    // create and map page table list
    let tables_new_ptr = unsafe { alloc(Layout::new::<[Option<TableRef<'static>>; 1024]>()) };
    let tables_new: &mut [Option<TableRef<'static>>; 1024] = unsafe { &mut *(tables_new_ptr as *mut [Option<TableRef<'static>>; 1024]) };
    for table in tables_new.iter_mut() {
        *table = None;
    }

    let tables_size = size_of::<[Option<TableRef<'static>>; 1024]>();
    let tables_hole = kernel_dir.find_hole(lowest_addr, stack_bottom, tables_size).expect("couldn't find space in kernel's page directory");

    debug!("mapping {:#x} - {:#x}", tables_hole, tables_hole + tables_size);

    kernel_dir
        .set_page(
            tables_hole,
            Some(PageFrame {
                addr: unsafe { LOADER_DIR.as_ref().unwrap().virt_to_phys(tables_new_ptr as usize).unwrap() },
                present: true,
                user_mode: false,
                writable: true,
                copy_on_write: false,
            }),
        )
        .expect("couldn't write to kernel's page directory");

    // map physical page table list
    let tables_physical_size = size_of::<[PageDirEntry; 1024]>();
    let tables_physical_hole = kernel_dir
        .find_hole(lowest_addr, stack_bottom, tables_physical_size)
        .expect("couldn't find space in kernel's page directory");

    kernel_dir
        .set_page(
            tables_physical_hole,
            Some(PageFrame {
                addr: kernel_dir.tables_physical_addr as u64,
                present: true,
                user_mode: false,
                writable: true,
                copy_on_write: false,
            }),
        )
        .expect("couldn't write to kernel's page directory");

    // fill out page table list
    let tables_physical: &mut [PageDirEntry; 1024] = unsafe { &mut *(kernel_dir.tables_physical as *mut _) };

    loop {
        // count number of used page tables
        let num_old = tables_physical.iter().filter(|e| !e.is_unused()).count();

        for (idx, entry) in tables_physical.iter().enumerate() {
            if !entry.is_unused() && tables_new[idx].is_none() {
                let hole = kernel_dir
                    .find_hole(lowest_addr, stack_bottom, size_of::<PageTable>())
                    .expect("couldn't find space in kernel's page directory");
                debug!("mapping page table @ {:#x} into kernel @ {:#x}", entry.get_address(), hole);
                kernel_dir
                    .set_page(
                        hole,
                        Some(PageFrame {
                            addr: entry.get_address() as u64,
                            present: true,
                            user_mode: false,
                            writable: true,
                            copy_on_write: false,
                        }),
                    )
                    .expect("couldn't write to kernel's page directory");
                // dereferencing this pointer is fine because we won't be using it, it'll just be passed along to the kernel where it will be valid
                tables_new[idx] = Some(TableRef {
                    table: unsafe { &mut *(hole as *mut PageTable) },
                    can_free: false,
                });
            }
        }

        // repeat if the number of used page tables has changed, so any newly allocated page tables from this process will be mapped
        let num_new = tables_physical.iter().filter(|e| !e.is_unused()).count();

        if num_old == num_new {
            break;
        }
    }

    // create new pagedir
    let kernel_dir_internal = unsafe {
        PageDir::from_allocated(
            &mut *(tables_hole as *mut [Option<TableRef<'static>>; 1024]),
            &mut *(tables_physical_hole as *mut [PageDirEntry; 1024]),
            kernel_dir.tables_physical_addr,
        )
    };

    // === prepare kernel stack ===

    debug!("preparing stack");

    // i have no idea what the hell is going on or why this all works
    let mut stack: Vec<u32> = vec![
        // whatever you put here seems to not matter at all
        0,
        // arguments go here in the order they show up in the function declaration
    ];

    // push the page directory
    unsafe {
        stack.append(&mut core::slice::from_raw_parts(&kernel_dir_internal as *const _ as *const u32, size_of::<PageDir>() / size_of::<u32>()).to_vec());
    }

    // push the address and number of modules
    stack.push(modules_addr.try_into().unwrap());
    stack.push(new_modules.len().try_into().unwrap());

    // push the address and number of memory map regions
    stack.push(regions_hole.try_into().unwrap());
    stack.push(regions.len().try_into().unwrap());

    // assemble the stack
    let data_bytes = unsafe { core::slice::from_raw_parts(stack.as_slice().as_ptr() as *const _ as *const u8, stack.len() * size_of::<u32>()).to_vec() };

    let stack_addr = (stack_top - data_bytes.len()) & !(16 - 1); // align to 16 byte boundary

    debug!("writing stack mem @ {:#x} - {:#x}", stack_addr, stack_addr + data_bytes.len());

    unsafe {
        LOADER_DIR
            .as_mut()
            .unwrap()
            .map_memory_from(&mut kernel_dir, stack_addr, data_bytes.len(), |s| s.clone_from_slice(&data_bytes))
            .expect("failed to populate kernel's stack");
    }

    debug!("calling shim");

    // === jump to kernel ===

    // call the shim, which will then switch to the kernel's page table, switch to its stack pointer, and jump to its entry point
    unsafe {
        debug!("tables_physical_addr is {:#x}", kernel_dir.tables_physical_addr);
        debug!("stack top is {:#x}", stack_top);
        debug!("stack pointer is {:#x}", stack_addr);
        debug!("elf entry is {:#x}", elf.entry);
        (exec_kernel_ptr)(kernel_dir.tables_physical_addr, stack_addr.try_into().unwrap(), elf.entry.try_into().unwrap());
    }
}
