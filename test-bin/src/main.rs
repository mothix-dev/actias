#![no_std]
#![no_main]

#![feature(start)]

#[path="../../src/syscalls.rs"]
pub mod syscalls;

use core::arch::asm;
use core::panic::PanicInfo;
use syscalls::Syscalls;

#[inline(always)]
fn syscall_is_computer_on() -> bool {
    let result: u32;
    unsafe {
        asm!("int 0x80", in("eax") Syscalls::IsComputerOn as u32, out("ebx") result);
    }

    result > 0
}

#[inline(always)]
fn syscall_test_log(string: &[u8]) {
    unsafe {
        asm!("int 0x80", in("eax") Syscalls::TestLog as u32, in("ebx") &string[0] as *const _);
    }
}

#[inline(always)]
fn syscall_fork() -> u32 {
    let result: u32;
    unsafe {
        asm!("int 0x80", in("eax") Syscalls::Fork as u32, out("ebx") result);
    }

    result
}

#[inline(always)]
#[allow(clippy::empty_loop)]
fn syscall_exit() -> ! {
    unsafe {
        asm!("int 0x80", in("eax") Syscalls::Exit as u32);
    }
    loop {}
}

#[inline(always)]
fn syscall_get_pid() -> u32 {
    let result: u32;
    unsafe {
        asm!("int 0x80", in("eax") Syscalls::GetPID as u32, out("ebx") result);
    }

    result
}

#[panic_handler]
fn panic_handler(_info: &PanicInfo) -> ! {
    syscall_test_log(b"panic :(\0");
    syscall_exit();
}

static mut TEST_STATIC: usize = 0;

#[no_mangle]
fn _start() {
    if syscall_is_computer_on() {
        syscall_test_log(b"computer is on\0");
    } else {
        syscall_test_log(b"computer is not on\0");
    }

    unsafe { TEST_STATIC = 621; }

    if unsafe { TEST_STATIC == 621 } {
        syscall_test_log(b"TEST_STATIC is set\0");
    } else {
        syscall_test_log(b"TEST_STATIC is not set\0");
    }

    if syscall_fork() != 0 {
        syscall_test_log(b"parent\0");

        if unsafe { TEST_STATIC == 621 } {
            syscall_test_log(b"parent: preserved\0");
        }
    } else {
        syscall_test_log(b"child\0");

        if unsafe { TEST_STATIC == 621 } {
            syscall_test_log(b"child: preserved\0");
        }

        syscall_exit();
    }

    let proc = syscall_fork();

    if proc != 0 {
        for _i in 0..8 {
            for _i in 0..1024 * 1024 { // slow things down
                unsafe {
                    asm!("nop");
                }
            }

            syscall_test_log(b"OwO\0");

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
            syscall_test_log(b"UwU\0");

            for _i in 0..1024 * 1024 * 2 { // slow things down
                unsafe {
                    asm!("nop");
                }
            }
        }
    }

    panic!("OwO");
}
