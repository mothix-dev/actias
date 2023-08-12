#![no_std]
#![no_main]

use common::{Errno, Result, Syscalls};
use core::arch::asm;

#[inline]
#[cfg(target_arch = "x86")]
unsafe fn syscall_0_args(num: Syscalls) -> Result<u32> {
    let res_ok: u32;
    let res_err: u32;
    let num = num as u32;

    asm!(
        "int 0x80",
        inlateout("eax") num => res_ok,
        out("ebx") res_err,
    );

    if res_err == 0 {
        Ok(res_ok)
    } else {
        Err(Errno::from(res_err))
    }
}

#[inline]
#[cfg(target_arch = "x86")]
unsafe fn syscall_1_args(num: Syscalls, arg0: u32) -> Result<u32> {
    let res_ok: u32;
    let res_err: u32;
    let num = num as u32;

    asm!(
        "int 0x80",
        inlateout("eax") num => res_ok,
        inlateout("ebx") arg0 => res_err,
    );

    if res_err == 0 {
        Ok(res_ok)
    } else {
        Err(Errno::from(res_err))
    }
}

#[inline]
#[cfg(target_arch = "x86")]
unsafe fn syscall_2_args(num: Syscalls, arg0: u32, arg1: u32) -> Result<u32> {
    let res_ok: u32;
    let res_err: u32;
    let num = num as u32;

    asm!(
        "int 0x80",
        inlateout("eax") num => res_ok,
        inlateout("ebx") arg0 => res_err,
        in("ecx") arg1,
    );

    if res_err == 0 {
        Ok(res_ok)
    } else {
        Err(Errno::from(res_err))
    }
}

#[inline]
#[cfg(target_arch = "x86")]
unsafe fn syscall_3_args(num: Syscalls, arg0: u32, arg1: u32, arg2: u32) -> Result<u32> {
    let res_ok: u32;
    let res_err: u32;
    let num = num as u32;

    asm!(
        "int 0x80",
        inlateout("eax") num => res_ok,
        inlateout("ebx") arg0 => res_err,
        in("ecx") arg1,
        in("edx") arg2,
    );

    if res_err == 0 {
        Ok(res_ok)
    } else {
        Err(Errno::from(res_err))
    }
}

#[inline]
#[cfg(target_arch = "x86")]
unsafe fn syscall_4_args(num: Syscalls, arg0: u32, arg1: u32, arg2: u32, arg3: u32) -> Result<u32> {
    let res_ok: u32;
    let res_err: u32;
    let num = num as u32;

    asm!(
        "int 0x80",
        inlateout("eax") num => res_ok,
        inlateout("ebx") arg0 => res_err,
        in("ecx") arg1,
        in("edx") arg2,
        in("edi") arg3,
    );

    if res_err == 0 {
        Ok(res_ok)
    } else {
        Err(Errno::from(res_err))
    }
}

fn write_message(message: &str) {
    unsafe {
        syscall_3_args(common::Syscalls::Write, 1, message.as_bytes().as_ptr() as u32, message.as_bytes().len() as u32).unwrap();
    }
}

#[no_mangle]
pub extern "C" fn _start() {
    unsafe {
        *(0xdffffffc as *mut u32) = 0xe621;
    }

    if unsafe { syscall_0_args(Syscalls::IsComputerOn).unwrap() } == 1 {
        write_message("computer is on!");
    }

    let uwu = unsafe { *(0xdffffffd as *mut u8) };
    if uwu != 0xe6 {
        write_message(":(");
    }

    let child_pid = unsafe { syscall_0_args(Syscalls::Fork).unwrap() };

    if child_pid == 0 {
        write_message("child process");

        unsafe {
            *(0xdffffffc as *mut u32) = 0xe926;
        }
    } else {
        write_message("parent process");

        for _i in 0..1048576 {
            unsafe {
                asm!("pause");
            }
        }

        let uwu = unsafe { *(0xdffffffd as *mut u8) };
        if uwu == 0xe9 {
            write_message(":(");
        } else if uwu == 0xe6 {
            write_message(":)");
        }
    }

    unsafe {
        syscall_0_args(Syscalls::Exit).unwrap();
    }

    write_message("not supposed to be here");

    #[allow(clippy::empty_loop)]
    loop {}
}

#[panic_handler]
pub fn panic_implementation(_info: &core::panic::PanicInfo) -> ! {
    write_message("panic!");

    unsafe {
        syscall_0_args(Syscalls::Exit).unwrap();
    }

    #[allow(clippy::empty_loop)]
    loop {}
}
