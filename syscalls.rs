#[path="src/syscalls.rs"]
pub mod syscalls;

use core::arch::asm;
use syscalls::Syscalls;

#[inline(always)]
pub fn is_computer_on() -> bool {
    let result: u32;
    unsafe {
        asm!("int 0x80", in("eax") Syscalls::IsComputerOn as u32, out("ebx") result);
    }

    result > 0
}

#[inline(always)]
pub fn test_log(string: &[u8]) {
    unsafe {
        asm!("int 0x80", in("eax") Syscalls::TestLog as u32, in("ebx") &string[0] as *const _);
    }
}

#[inline(always)]
pub fn fork() -> u32 {
    let result: u32;
    unsafe {
        asm!("int 0x80", in("eax") Syscalls::Fork as u32, out("ebx") result);
    }

    result
}

#[inline(always)]
#[allow(clippy::empty_loop)]
pub fn exit() -> ! {
    unsafe {
        asm!("int 0x80", in("eax") Syscalls::Exit as u32);
    }
    loop {}
}

#[inline(always)]
pub fn get_pid() -> u32 {
    let result: u32;
    unsafe {
        asm!("int 0x80", in("eax") Syscalls::GetPID as u32, out("ebx") result);
    }

    result
}

#[inline(always)]
pub fn exec(string: &[u8]) {
    unsafe {
        asm!("int 0x80", in("eax") Syscalls::Exec as u32, in("ebx") &string[0] as *const _);
    }
}
