#![no_std]

use common::types::{Errno, MmapArguments, MmapFlags, MmapProtection, ProcessID, Result, Syscalls, UnmapArguments};
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

pub fn is_computer_on() -> bool {
    if let Ok(res) = unsafe { syscall_0_args(Syscalls::IsComputerOn) } {
        res > 0
    } else {
        false
    }
}

#[allow(clippy::empty_loop)]
pub fn exit() -> ! {
    unsafe {
        syscall_0_args(Syscalls::ExitProcess).unwrap();
    }

    loop {}
}

#[allow(clippy::empty_loop)]
pub fn exit_thread() -> ! {
    unsafe {
        syscall_0_args(Syscalls::ExitThread).unwrap();
    }

    loop {}
}

pub fn fork() -> Result<u32> {
    unsafe { syscall_0_args(Syscalls::Fork) }
}

pub fn mmap(id: u32, addr_hint: *mut u8, length: usize, protection: MmapProtection, flags: MmapFlags) -> Result<*mut u8> {
    let mut args = MmapArguments {
        id,
        address: addr_hint as u64,
        length: length as u64,
        protection,
        flags,
    };

    let addr = &mut args as *mut _ as usize;

    #[cfg(target_pointer_width = "64")]
    unsafe {
        syscall_2_args(Syscalls::Mmap, (addr >> 32) as u32, addr as u32 & u32::MAX)?;
    }

    #[cfg(target_pointer_width = "32")]
    unsafe {
        syscall_1_args(Syscalls::Mmap, addr as u32)?;
    }

    let res: usize = unsafe { core::ptr::read_volatile(&args.address) }.try_into().map_err(|_| Errno::ValueOverflow)?;
    Ok(res as *mut u8)
}

pub fn unmap(addr_hint: *mut u8, length: usize) -> Result<()> {
    let args = UnmapArguments {
        address: addr_hint as u64,
        length: length as u64,
    };

    let addr = &args as *const _ as usize;

    #[cfg(target_pointer_width = "64")]
    unsafe {
        syscall_2_args(Syscalls::Unmap, (addr >> 32) as u32, addr as u32 & u32::MAX)?;
    }

    #[cfg(target_pointer_width = "32")]
    unsafe {
        syscall_1_args(Syscalls::Unmap, addr as u32)?;
    }

    Ok(())
}

pub fn get_process_id() -> Result<ProcessID> {
    let mut pid: ProcessID = ProcessID { process: 0, thread: 0 };

    let addr = &mut pid as *mut _ as usize;

    #[cfg(target_pointer_width = "64")]
    unsafe {
        syscall_2_args(Syscalls::GetProcessID, (addr >> 32) as u32, addr as u32 & u32::MAX)?;
    }

    #[cfg(target_pointer_width = "32")]
    unsafe {
        syscall_1_args(Syscalls::GetProcessID, addr as u32)?;
    }

    Ok(unsafe { core::ptr::read_volatile(&pid) })
}
