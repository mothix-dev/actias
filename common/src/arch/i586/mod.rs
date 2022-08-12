pub mod paging;

use core::arch::asm;
use x86::bits32::eflags::EFlags;

// various useful constants
pub const MEM_TOP: usize = 0xffffffff;
pub const LINKED_BASE: usize = 0xc0000000;
pub const KHEAP_START: usize = LINKED_BASE + 0x10000000;

pub const PAGE_SIZE: usize = 0x1000;
pub const INV_PAGE_SIZE: usize = !(PAGE_SIZE - 1);

pub const MAX_STACK_FRAMES: usize = 1024;

pub static mut MEM_SIZE: u64 = 0; // filled in later by BIOS or something similar

/// gets the value of the eflags register in the cpu as an easy to use struct
pub fn get_eflags() -> EFlags {
    unsafe {
        let mut flags: u32;

        asm!(
            "pushfd",
            "pop {}",
            out(reg) flags,
        );

        EFlags::from_bits(flags).unwrap()
    }
}

/// sets the value of the eflags register in the cpu
pub fn set_eflags(flags: EFlags) {
    unsafe {
        let flags: u32 = flags.bits();

        asm!(
            "push {}",
            "popfd",
            in(reg) flags,
        );
    }
}

/// completely halts the cpu
pub unsafe fn halt() -> ! {
    // exit qemu
    x86::io::outb(0x501, 0x31);

    // exit bochs
    x86::io::outw(0x8a00, 0x8a00);
    x86::io::outw(0x8a00, 0x8ae0);

    // halt cpu
    loop {
        asm!("cli; hlt");
    }
}
