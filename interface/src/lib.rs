#![no_std]

use common::types::Syscalls;
use core::arch::asm;

#[cfg(target_arch = "x86")]
pub unsafe fn syscall_0_args(num: Syscalls) -> u32 {
    let res: u32;
    let num = num as u32;

    asm!(
        "int 0x80",
        inlateout("eax") num => res,
    );

    res
}

#[cfg(target_arch = "x86")]
pub unsafe fn syscall_1_args(num: Syscalls, arg0: u32) -> u32 {
    let res: u32;
    let num = num as u32;

    asm!(
        "int 0x80",
        inlateout("eax") num => res,
        in("ebx") arg0,
    );

    res
}

#[cfg(target_arch = "x86")]
pub unsafe fn syscall_2_args(num: Syscalls, arg0: u32, arg1: u32) -> u32 {
    let res: u32;
    let num = num as u32;

    asm!(
        "int 0x80",
        inlateout("eax") num => res,
        in("ebx") arg0,
        in("ecx") arg1,
    );

    res
}

#[cfg(target_arch = "x86")]
pub unsafe fn syscall_3_args(num: Syscalls, arg0: u32, arg1: u32, arg2: u32) -> u32 {
    let res: u32;
    let num = num as u32;

    asm!(
        "int 0x80",
        inlateout("eax") num => res,
        in("ebx") arg0,
        in("ecx") arg1,
        in("edx") arg2,
    );

    res
}

pub fn is_computer_on() -> bool {
    unsafe {
        syscall_0_args(Syscalls::IsComputerOn) > 0
    }
}
