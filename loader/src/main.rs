#![no_std]
#![no_main]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]
#![feature(core_c_str)]
#![feature(cstr_from_bytes_until_nul)]
#![feature(abi_x86_interrupt)]

extern crate alloc;

/// low level boot code for ibmpc
#[cfg(target_platform = "ibmpc")]
#[path = "boot/ibmpc/mod.rs"]
pub mod boot;

pub mod tar;

use alloc::alloc::Layout;
use common::mm::heap::CustomAlloc;
use log::{debug, error, trace};

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

    unsafe {
        common::arch::halt();
    }
}

#[global_allocator]
static ALLOCATOR: CustomAlloc = CustomAlloc;

#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    panic!("allocation error with layout {:?}", layout);
}

const BUMP_ALLOC_SIZE: usize = 0x100000; // 1mb

static mut PLACEMENT_ADDR_INITIAL: usize = 0; // initial placement addr
static mut PLACEMENT_ADDR: usize = 0; // to be filled in with end of kernel on init
static mut PLACEMENT_AREA: [u8; BUMP_ALLOC_SIZE] = [0; BUMP_ALLOC_SIZE]; // hopefully this will just be located in bss? we can't just allocate memory for it since we need it to allocate memory
static mut PLACEMENT_OFFSET: usize = 0;

/// result of kmalloc calls
pub struct MallocResult<T> {
    pub pointer: *mut T,
    pub phys_addr: usize,
}

/// simple bump allocator, used to allocate memory required for initializing things
pub unsafe fn bump_alloc<T>(layout: Layout) -> MallocResult<T> {
    // check if alignment is requested and we aren't already aligned
    let align_inv = !(layout.align() - 1); // alignment is guaranteed to be a power of two
    if layout.align() > 1 && PLACEMENT_ADDR & align_inv > 0 {
        PLACEMENT_ADDR &= align_inv;
        PLACEMENT_ADDR += layout.align();
    }

    // increment address to make room for area of provided size, return pointer to start of area
    let tmp = PLACEMENT_ADDR;
    PLACEMENT_ADDR += layout.size();

    if PLACEMENT_ADDR >= PLACEMENT_ADDR_INITIAL + BUMP_ALLOC_SIZE {
        // prolly won't happen but might as well
        panic!("out of memory (bump_alloc)");
    }

    trace!("bump allocated virt {:#x}, phys {:#x}, size {:#x}", tmp + PLACEMENT_OFFSET, tmp, layout.size());

    MallocResult {
        pointer: (tmp + PLACEMENT_OFFSET) as *mut T,
        phys_addr: tmp,
    }
}

/// initialize the bump allocator
///
/// # Safety
///
/// this function is unsafe because if it's called more than once, the bump allocator will reset and potentially critical data can be overwritten
pub unsafe fn init_bump_alloc(offset: usize) {
    // calculate placement addr for initial kmalloc calls
    PLACEMENT_OFFSET = offset;
    PLACEMENT_ADDR_INITIAL = (&PLACEMENT_AREA as *const _) as usize - PLACEMENT_OFFSET;
    PLACEMENT_ADDR = PLACEMENT_ADDR_INITIAL;

    debug!(
        "placement @ {:#x} - {:#x} (virt @ {:#x})",
        PLACEMENT_ADDR,
        PLACEMENT_ADDR + BUMP_ALLOC_SIZE,
        PLACEMENT_ADDR + PLACEMENT_OFFSET
    );
}

// the entry point isn't contained in this file!! try looking in boot/*/mod.rs
