pub mod gdt;
pub mod ints;
pub mod paging;
pub mod syscalls;
pub mod tasks;

use crate::platform::bootloader;
use core::arch::asm;
use log::{error, warn, info, debug, trace};

// various useful constants
pub const MEM_TOP: usize = 0xffffffff;
pub const LINKED_BASE: usize = 0xc0000000;
pub const KHEAP_START: usize = LINKED_BASE + 0x10000000;

pub const PAGE_SIZE: usize = 0x1000;
pub const INV_PAGE_SIZE: usize = !(PAGE_SIZE - 1);

pub const MAX_STACK_FRAMES: usize = 1024;

pub static mut MEM_SIZE: u64 = 0; // filled in later by BIOS or something similar

/// halt system
pub fn halt() -> ! {
    info!("halting");

    unsafe {
        loop {
            asm!("cli; hlt"); // clear interrupts, halt
        }
    }
}

/// initialize sub-modules
pub fn init() {
    info!("bootloader pre init");
    unsafe {
        bootloader::pre_init();
    }

    info!("initializing GDT");
    unsafe {
        gdt::init();
    }
    info!("initializing interrupts");
    unsafe {
        ints::init();
    }

    info!("bootloader init");
    unsafe {
        bootloader::init();
    }

    info!("initializing paging");
    unsafe {
        paging::init();
    }
}

pub fn init_after_heap() {
    info!("bootloader init after heap");
    bootloader::init_after_heap();
}
