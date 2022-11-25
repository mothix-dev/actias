#![no_std]

use common::types::{Errno, MmapFlags, MmapAccess, ProcessID, Result, Syscalls};
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

pub fn mmap(id: u32, addr_hint: *mut u8, length: usize, protection: MmapAccess, flags: MmapFlags) -> Result<*mut u8> {
    unsafe {
        syscall_4_args(
            Syscalls::Mmap,
            id,
            (addr_hint as usize).try_into().map_err(|_| Errno::ValueOverflow)?,
            length.try_into().map_err(|_| Errno::ValueOverflow)?,
            ((u8::from(protection) as u32) << 8) | (u8::from(flags) as u32),
        )
        .map(|addr| addr as *mut u8)
    }
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
        syscall_2_args(
            Syscalls::ShareMemory,
            (addr as usize).try_into().map_err(|_| Errno::ValueOverflow)?,
            length.try_into().map_err(|_| Errno::ValueOverflow)?,
        )
    }
}

pub fn send_message(target: u32, message: u32, data: Option<&[u8]>) -> Result<()> {
    let addr;
    let len;

    if let Some(slice) = data {
        addr = (slice.as_ptr() as usize).try_into().map_err(|_| Errno::ValueOverflow)?;
        len = slice.len().try_into().map_err(|_| Errno::ValueOverflow)?;
    } else {
        addr = 0;
        len = 0;
    }

    unsafe {
        syscall_4_args(Syscalls::SendMessage, target, message, addr, len)?;
    }

    Ok(())
}

pub fn set_message_handler(message: u32, priority: u8, handler: extern "fastcall" fn(u32, u32)) -> Result<()> {
    unsafe {
        syscall_3_args(Syscalls::MessageHandler, message, priority as u32, handler as u32)?;
    }

    Ok(())
}

pub fn exit_message_handler() -> Result<()> {
    unsafe {
        syscall_0_args(Syscalls::ExitMessageHandler)?;
    }

    Ok(())
}
