#![no_std]
#![no_main]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]

extern crate alloc;

// low level boot code for ibmpc
#[cfg(target_platform = "ibmpc")]
#[path = "boot/ibmpc/mod.rs"]
pub mod boot;

use alloc::alloc::Layout;
use common::{
    arch::{
        paging::{PageDir, PageDirEntry, PageTable, TableRef},
        LINKED_BASE, PAGE_SIZE,
    },
    mm::{
        heap::CustomAlloc,
        paging::{PageDirectory, PageError, PageManager},
    },
    util::array::BitSet,
};
use core::{arch::asm, mem::size_of};
use log::{debug, error, info, trace, warn};

pub const NAME: &str = env!("CARGO_PKG_NAME");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[panic_handler]
pub fn panic_implementation(info: &core::panic::PanicInfo) -> ! {
    let (file, line) = match info.location() {
        Some(loc) => (loc.file(), loc.line()),
        None => ("", 0),
    };

    if let Some(m) = info.message() {
        error!("PANIC: file='{}', line={} :: {}", file, line, m);
    } else if let Some(m) = info.payload().downcast_ref::<&str>() {
        error!("PANIC: file='{}', line={} :: {}", file, line, m);
    } else {
        error!("PANIC: file='{}', line={} :: ?", file, line);
    }

    loop {
        unsafe {
            asm!("cli; hlt");
        }
    }
}

#[global_allocator]
static ALLOCATOR: CustomAlloc = CustomAlloc;

#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    panic!("allocation error with layout {:?}", layout);
}

pub const KHEAP_START: usize = LINKED_BASE + 0x10000000;
pub const KHEAP_INITIAL_SIZE: usize = 0x100000;
pub const KHEAP_MAX_SIZE: usize = 0xffff000;
pub const HEAP_MIN_SIZE: usize = 0x70000;

extern "C" {
    /// located at end of kernel, used for calculating placement address
    static kernel_end: u32;
}

const BUMP_ALLOC_SIZE: usize = 0x100000; // 1mb

static mut PLACEMENT_ADDR_INITIAL: usize = 0; // initial placement addr
static mut PLACEMENT_ADDR: usize = 0; // to be filled in with end of kernel on init
static mut PLACEMENT_AREA: [u8; BUMP_ALLOC_SIZE] = [0; BUMP_ALLOC_SIZE]; // hopefully this will just be located in bss? we can't just allocate memory for it since we need it to allocate memory

/// result of kmalloc calls
pub struct MallocResult<T> {
    pub pointer: *mut T,
    pub phys_addr: usize,
}

/// simple bump allocator, used to allocate memory required for initializing things
pub unsafe fn bump_alloc<T>(size: usize, align: bool) -> MallocResult<T> {
    if align && PLACEMENT_ADDR % PAGE_SIZE != 0 {
        // if alignment is requested and we aren't already aligned
        PLACEMENT_ADDR &= !(PAGE_SIZE - 1); // round down to nearest 4k block
        PLACEMENT_ADDR += PAGE_SIZE; // increment by 4k- we don't want to overwrite things
    }

    // increment address to make room for area of provided size, return pointer to start of area
    let tmp = PLACEMENT_ADDR;
    PLACEMENT_ADDR += size;

    if PLACEMENT_ADDR >= PLACEMENT_ADDR_INITIAL + BUMP_ALLOC_SIZE {
        // prolly won't happen but might as well
        panic!("out of memory (bump_alloc)");
    }

    trace!("bump allocated virt {:#x}, phys {:#x}, size {:#x}", tmp + LINKED_BASE, tmp, size);

    MallocResult {
        pointer: (tmp + LINKED_BASE) as *mut T,
        phys_addr: tmp,
    }
}

/// initialize the bump allocator
///
/// # Safety
///
/// this function is unsafe because if it's called more than once, the bump allocator will reset and potentially critical data can be overwritten
pub unsafe fn init_bump_alloc() {
    // calculate end of kernel in memory
    let kernel_end_pos = (&kernel_end as *const _) as usize;

    // calculate placement addr for initial kmalloc calls
    PLACEMENT_ADDR_INITIAL = (&PLACEMENT_AREA as *const _) as usize - LINKED_BASE;
    PLACEMENT_ADDR = PLACEMENT_ADDR_INITIAL;

    debug!("kernel end @ {:#x}, linked @ {:#x}", kernel_end_pos, LINKED_BASE);
    debug!(
        "placement @ {:#x} - {:#x} (virt @ {:#x})",
        PLACEMENT_ADDR,
        PLACEMENT_ADDR + BUMP_ALLOC_SIZE,
        PLACEMENT_ADDR + LINKED_BASE
    );
}

static mut PAGE_MANAGER: Option<PageManager<PageDir>> = None;
static mut LOADER_DIR: Option<PageDir> = None;

#[no_mangle]
pub fn kmain() {
    // initialize our logger
    common::logger::init().unwrap();

    info!("{} v{}", NAME, VERSION);

    // our memory size, just a total guess for now
    let mem_size = 128 * 1024 * 1024;
    let mem_size_pages = mem_size / PAGE_SIZE;

    let kernel_end_pos = unsafe { (&kernel_end as *const _) as usize };

    // initialize the bump allocator so we can allocate initial memory for paging
    unsafe {
        init_bump_alloc();
    }

    // create a pagemanager to manage our page allocations
    let mut manager: PageManager<PageDir> = PageManager::new({
        let alloc_size = mem_size_pages / 32 * size_of::<u32>();
        let ptr = unsafe { bump_alloc::<u32>(alloc_size, false).pointer };
        let mut bitset = BitSet::place_at(ptr, mem_size_pages);
        bitset.clear_all();
        bitset
    });

    // page directory for loader
    let mut loader_dir = {
        let tables = unsafe { &mut *bump_alloc::<[Option<TableRef<'static>>; 1024]>(size_of::<[Option<TableRef<'static>>; 1024]>(), true).pointer };
        for table_ref in tables.iter_mut() {
            *table_ref = None;
        }

        let ptr = unsafe { bump_alloc::<[PageDirEntry; 1024]>(size_of::<[PageDirEntry; 1024]>(), true) };

        PageDir::from_allocated(tables, unsafe { &mut *ptr.pointer }, ptr.phys_addr.try_into().unwrap())
    };

    // allocate a range of addresses. this can't be done outside this because rust gets confused with lifetimes
    let mut map = |kind: &str, start: usize, end: usize| {
        debug!("mapping {} ({:#x} - {:#x})", kind, start, end);

        for addr in (start..end).step_by(PAGE_SIZE) {
            if !loader_dir.has_page_table(addr.try_into().unwrap()) {
                trace!("allocating new page table");
                let alloc_size = size_of::<PageTable>();
                let ptr = unsafe { bump_alloc::<PageTable>(alloc_size, true) };
                loader_dir.add_page_table(addr.try_into().unwrap(), unsafe { &mut *ptr.pointer }, ptr.phys_addr.try_into().unwrap());
            }

            manager.alloc_frame(&mut loader_dir, addr, false, true).unwrap();
        }
    };

    let heap_reserved = PAGE_SIZE * 2;

    // allocate pages
    map("loader", LINKED_BASE, kernel_end_pos);
    map("heap", KHEAP_START, KHEAP_START + heap_reserved);

    // switch to our new page directory so all the pages we've just mapped will be accessible
    unsafe {
        // if we don't set this as global state something breaks, haven't bothered figuring out what yet
        LOADER_DIR = Some(loader_dir);

        LOADER_DIR.as_ref().unwrap().switch_to();

        PAGE_MANAGER = Some(manager);
    }

    // set up allocator with minimum size
    ALLOCATOR.init(KHEAP_START, heap_reserved);

    ALLOCATOR.reserve_memory(Some(Layout::from_size_align(heap_reserved, PAGE_SIZE).unwrap()));

    fn expand(old_top: usize, new_top: usize, alloc: &dyn Fn(Layout) -> Result<*mut u8, ()>, free: &dyn Fn(*mut u8, Layout)) -> Result<usize, ()> {
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

                    dir.add_page_table(addr.try_into().unwrap(), unsafe { &mut *(virt as *mut PageTable) }, phys.try_into().unwrap());
                }

                unsafe {
                    PAGE_MANAGER.as_mut().unwrap().alloc_frame(dir, addr, false, true).map_err(|err| {
                        error!("error allocating page for heap: {:?}", err);
                        ()
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

    {
        use alloc::vec::Vec;
        let mut vec: Vec<u32> = Vec::with_capacity(1);
        vec.push(3);
        vec.push(5);
        vec.push(9);
        vec.push(15);

        debug!("{:?}", vec);

        assert!(vec.len() == 4);
    }

    unsafe {
        PAGE_MANAGER.as_mut().unwrap().print_free();
    }

    info!("done?");
}
