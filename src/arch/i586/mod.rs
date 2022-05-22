pub mod ints;
pub mod gdt;
pub mod paging;
pub mod syscalls;
pub mod tasks;

use core::arch::asm;

// various useful constants
pub const MEM_TOP: usize = 0xffffffff;
pub const LINKED_BASE: usize = 0xc0000000;
pub const KHEAP_START: usize = LINKED_BASE + 0x10000000;

pub const PAGE_SIZE: usize = 0x1000;
pub const INV_PAGE_SIZE: usize = !(PAGE_SIZE - 1);

pub const MAX_STACK_FRAMES: usize = 1024;

pub static mut MEM_SIZE: usize = 128 * 1024 * 1024; // TODO: get actual RAM size from BIOS

/// initialize paging, just cleanly map our kernel to 3gb
#[no_mangle]
pub extern fn x86_prep_page_table(buf: &mut [u32; 1024]) {
    for i in 0u32 .. 1024 {
        buf[i as usize] = i * 0x1000 + 3;
    }
}

/// halt system
pub fn halt() -> ! {
    log!("halting");

    unsafe {
        loop {
            asm!("cli; hlt"); // clear interrupts, halt
        }
    }
}

/// initialize sub-modules
pub fn init() {
    log!("initializing GDT");
    unsafe { gdt::init(); }
    log!("initializing interrupts");
    unsafe { ints::init(); }
    log!("initializing paging");
    unsafe { paging::init(); }
}
