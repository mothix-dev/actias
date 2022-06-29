#[path="src/types/syscalls.rs"]
pub mod syscalls;

use core::arch::asm;
use syscalls::Syscall;

#[inline(always)]
pub fn is_computer_on() -> bool {
    let result: u32;
    unsafe {
        asm!("int 0x80", in("eax") Syscall::IsComputerOn as u32, out("ebx") result);
    }

    result > 0
}

#[inline(always)]
pub fn test_log(string: &[u8]) {
    unsafe {
        asm!("int 0x80", in("eax") Syscall::TestLog as u32, in("ebx") &string[0] as *const _);
    }
}

#[inline(always)]
pub fn test_log_ptr(string: *const u8) {
    unsafe {
        asm!("int 0x80", in("eax") Syscall::TestLog as u32, in("ebx") string);
    }
}

#[inline(always)]
pub fn fork() -> u32 {
    let result: u32;
    unsafe {
        asm!("int 0x80", in("eax") Syscall::Fork as u32, out("ebx") result);
    }

    result
}

#[inline(always)]
#[allow(clippy::empty_loop)]
pub fn exit() -> ! {
    unsafe {
        asm!("int 0x80", in("eax") Syscall::Exit as u32);
    }
    loop {}
}

#[inline(always)]
pub fn get_pid() -> u32 {
    let result: u32;
    unsafe {
        asm!("int 0x80", in("eax") Syscall::GetPID as u32, out("ebx") result);
    }

    result
}

#[inline(always)]
pub fn exec(string: &[u8], args: &[*const u8], env: &[*const u8]) {
    unsafe {
        asm!("int 0x80", in("eax") Syscall::Exec as u32,
             in("ebx") &string[0] as *const _, in("ecx") &args[0] as *const _, in("edx") &env[0] as *const _);
    }
}
