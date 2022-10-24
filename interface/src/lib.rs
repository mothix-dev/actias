#![no_std]

use common::types::{Errno, MmapArguments, MmapFlags, MmapProtection, ProcessID, Result, Syscalls};
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

    unsafe {
        syscall_1_args(Syscalls::Mmap, addr.try_into().map_err(|_| Errno::ValueOverflow)?)?;
    }

    let res: usize = unsafe { core::ptr::read_volatile(&args.address) }.try_into().map_err(|_| Errno::ValueOverflow)?;
    Ok(res as *mut u8)
}

pub fn unmap(addr_hint: *mut u8, length: usize) -> Result<()> {
    unsafe {
        syscall_2_args(
            Syscalls::Unmap,
            (addr_hint as usize).try_into().map_err(|_| Errno::ValueOverflow)?,
            length.try_into().map_err(|_| Errno::ValueOverflow)?,
        )?;
    }

    Ok(())
}

pub fn get_process_id() -> Result<ProcessID> {
    let mut pid: ProcessID = ProcessID { process: 0, thread: 0 };

    let addr = &mut pid as *mut _ as usize;

    unsafe {
        syscall_1_args(Syscalls::GetProcessID, addr.try_into().map_err(|_| Errno::ValueOverflow)?)?;
    }

    Ok(unsafe { core::ptr::read_volatile(&pid) })
}

pub fn share_memory(addr: *const u8, length: usize) -> Result<u32> {
    unsafe {
        syscall_2_args(Syscalls::ShareMemory, (addr as usize).try_into().map_err(|_| Errno::ValueOverflow)?, length.try_into().map_err(|_| Errno::ValueOverflow)?)
    }
}
