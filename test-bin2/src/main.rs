#![no_std]
#![no_main]

#[path="../../syscalls.rs"]
pub mod syscalls;

use core::arch::asm;
use core::panic::PanicInfo;

#[panic_handler]
fn panic_handler(_info: &PanicInfo) -> ! {
    syscalls::test_log(b"panic :(\0");
    syscalls::exit();
}

#[no_mangle]
fn _start() {
    loop {
        syscalls::test_log(b"completely independent process!\0");

        for _i in 0..1024 * 1024 * 2 { // slow things down
            unsafe {
                asm!("nop");
            }
        }
    }
}
