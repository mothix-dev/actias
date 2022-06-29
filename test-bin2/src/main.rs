#![no_std]
#![no_main]

#[path="../../syscalls.rs"]
pub mod syscalls;

use core::{
    arch::asm,
    mem::size_of,
    panic::PanicInfo,
};

#[panic_handler]
fn panic_handler(_info: &PanicInfo) -> ! {
    syscalls::test_log(b"panic :(\0");
    syscalls::exit();
}

#[no_mangle]
pub extern "cdecl" fn _start(argc: usize, argv: *const *const u8, envp: *const *const u8) {
    syscalls::test_log(b"args:\0");
    let mut i = 0;
    loop {
        let ptr = unsafe { *((argv as usize + i * size_of::<usize>()) as *const *const u8) };
        if ptr.is_null() {
            break;
        } else {
            syscalls::test_log_ptr(ptr);
        }
        i += 1;
    }

    syscalls::test_log(b"env:\0");
    let mut i = 0;
    loop {
        let ptr = unsafe { *((envp as usize + i * size_of::<usize>()) as *const *const u8) };
        if ptr.is_null() {
            break;
        } else {
            syscalls::test_log_ptr(ptr);
        }
        i += 1;
    }

    syscalls::test_log_ptr(0xb0000000 as *const _);

    loop {
        syscalls::test_log(b"completely independent process!\0");

        for _i in 0..1024 * 1024 * 2 { // slow things down
            unsafe {
                asm!("nop");
            }
        }
    }
}
