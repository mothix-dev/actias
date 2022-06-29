#![no_std]
#![no_main]

#[path="../../syscalls.rs"]
pub mod syscalls;

use core::{
    arch::asm,
    panic::PanicInfo,
};

#[panic_handler]
fn panic_handler(_info: &PanicInfo) -> ! {
    syscalls::test_log(b"panic :(\0");
    syscalls::exit();
}

static mut TEST_STATIC: usize = 0;

#[no_mangle]
pub extern "cdecl" fn _start(argc: usize, argv: *const *const u8, _envp: *const *const u8) {
    if syscalls::is_computer_on() {
        syscalls::test_log(b"computer is on\0");
    } else {
        syscalls::test_log(b"computer is not on\0");
    }

    unsafe { TEST_STATIC = 621; }

    if unsafe { TEST_STATIC == 621 } {
        syscalls::test_log(b"TEST_STATIC is set\0");
    } else {
        syscalls::test_log(b"TEST_STATIC is not set\0");
    }

    if syscalls::fork() != 0 {
        syscalls::test_log(b"parent\0");

        if unsafe { TEST_STATIC == 621 } {
            syscalls::test_log(b"parent: preserved\0");
        }

        let file = b"/fs/initrd/test-bin2\0";

        let args: [*const u8; 4] = [
            file as *const u8,
            b"test arg 1\0" as *const u8,
            b"test arg 2\0" as *const u8,
            0 as *const u8,
        ];

        let env: [*const u8; 3] = [
            //0xb0000000 as *const u8,
            b"env test 1\0" as *const u8,
            b"env test 2\0" as *const u8,
            0 as *const u8,
        ];

        syscalls::exec(file, &args, &env);
    } else {
        syscalls::test_log(b"child\0");

        if unsafe { TEST_STATIC == 621 } {
            syscalls::test_log(b"child: preserved\0");
        }
    }

    let proc = syscalls::fork();

    if proc != 0 {
        for _i in 0..8 {
            for _i in 0..1024 * 1024 { // slow things down
                unsafe {
                    asm!("nop");
                }
            }

            syscalls::test_log(b"OwO\0");

            for _i in 0..1024 * 1024 {
                unsafe {
                    asm!("nop");
                }
            }
        }

        unsafe {
            asm!("int3"); // effectively crash this process
        }

        loop {}
    } else {
        for _i in 0..32 {
            syscalls::test_log(b"UwU\0");

            for _i in 0..1024 * 1024 * 2 { // slow things down
                unsafe {
                    asm!("nop");
                }
            }
        }
    }

    panic!("OwO");
}
