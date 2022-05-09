pub mod debug;
pub mod io;
pub mod ints;
pub mod gdt;
pub mod paging;
pub mod vga;

use core::arch::asm;

/// initialize paging, just cleanly map our kernel to 3gb
#[no_mangle]
pub extern fn x86_prep_page_table(buf: &mut [u32; 1024]) {
    for i in 0u32 .. 1024 {
        buf[i as usize] = i * 0x1000 + 3;
    }
}

pub fn halt() {
    log!("halting");

    unsafe {
        asm!("cli; hlt"); // clear interrupts, halt
    }
}
