use core::arch::asm;
use crate::types::{
    syscalls::Syscall,
    errno::Errno,
    file::{OpenFlags, SeekKind, FileStatus},
};
use super::FileDescriptor;

#[inline(always)]
pub fn is_computer_on() -> Result<bool, Errno> {
    let result: u32;
    let errno: u32;

    unsafe {
        asm!("int 0x80", in("eax") Syscall::IsComputerOn as u32, out("ebx") result, lateout("eax") errno);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
        Ok(result > 0)
    }
}

#[inline(always)]
pub fn test_log(string: &[u8]) -> Result<(), Errno> {
    let errno: u32;

    unsafe {
        asm!("int 0x80", in("eax") Syscall::TestLog as u32, in("ebx") string.as_ptr(), lateout("eax") errno);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
        Ok(())
    }
}

#[inline(always)]
pub fn test_log_ptr(string: *const u8) -> Result<(), Errno> {
    let errno: u32;

    unsafe {
        asm!("int 0x80", in("eax") Syscall::TestLog as u32, in("ebx") string, lateout("eax") errno);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
        Ok(())
    }
}

#[inline(always)]
pub fn fork() -> Result<u32, Errno> {
    let result: u32;
    let errno: u32;

    unsafe {
        asm!("int 0x80", in("eax") Syscall::Fork as u32, out("ebx") result, lateout("eax") errno);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
        Ok(result)
    }
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
pub fn get_pid() -> Result<u32, Errno> {
    let result: u32;
    let errno: u32;

    unsafe {
        asm!("int 0x80", in("eax") Syscall::GetPID as u32, out("ebx") result, lateout("eax") errno);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
        Ok(result)
    }
}

#[inline(always)]
pub fn exec(string: &[u8], args: &[*const u8], env: &[*const u8]) -> Result<(), Errno> {
    let errno: u32;

    unsafe {
        asm!("int 0x80", in("eax") Syscall::Exec as u32,
             in("ebx") &string[0] as *const _, in("ecx") &args[0] as *const _, in("edx") &env[0] as *const _, lateout("eax") errno);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
        Ok(())
    }
}

#[inline(always)]
pub fn open(path: &[u8], flags: OpenFlags) -> Result<FileDescriptor, Errno> {
    let result: u32;
    let errno: u32;
    let flags: u8 = flags.into();

    unsafe {
        asm!("int 0x80", in("eax") Syscall::Open as u32, in("ebx") &path[0] as *const _, in("ecx") flags as usize, lateout("ebx") result, lateout("eax") errno);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
        Ok(FileDescriptor(result.try_into().map_err(|_| Errno::ValueOverflow)?))
    }
}

#[inline(always)]
pub fn close(desc: &FileDescriptor) -> Result<(), Errno> {
    let errno: u32;

    unsafe {
        asm!("int 0x80", in("eax") Syscall::Close as u32, in("ebx") desc.0, lateout("eax") errno);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
        Ok(())
    }
}

#[inline(always)]
pub fn write(desc: &FileDescriptor, slice: &[u8]) -> Result<u32, Errno> {
    let result: u32;
    let errno: u32;

    unsafe {
        asm!("int 0x80", in("eax") Syscall::Write as u32, in("ebx") desc.0, in("ecx") slice.as_ptr(), in("edx") slice.len(), lateout("eax") errno, lateout("ebx") result);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
        Ok(result)
    }
}

#[inline(always)]
pub fn read(desc: &FileDescriptor, slice: &mut [u8]) -> Result<u32, Errno> {
    let result: u32;
    let errno: u32;

    unsafe {
        asm!("int 0x80", in("eax") Syscall::Read as u32, in("ebx") desc.0, in("ecx") slice.as_mut_ptr(), in("edx") slice.len(), lateout("eax") errno, lateout("ebx") result);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
        Ok(result)
    }
}

#[inline(always)]
pub fn seek(desc: &FileDescriptor, offset: isize, kind: SeekKind) -> Result<u64, Errno> {
    let result_low: u32;
    let result_high: u32;
    let errno: u32;

    unsafe {
        asm!("int 0x80", in("eax") Syscall::Seek as u32, in("ebx") desc.0, in("ecx") offset, in("edx") kind as usize, lateout("eax") errno, lateout("ebx") result_low, lateout("ecx") result_high);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
        let result = result_low as u64 | ((result_high as u64) << 32);

        Ok(result)
    }
}

#[inline(always)]
pub fn get_seek(desc: &FileDescriptor) -> Result<u64, Errno> {
    let result_low: u32;
    let result_high: u32;
    let errno: u32;

    unsafe {
        asm!("int 0x80", in("eax") Syscall::GetSeek as u32, in("ebx") desc.0, lateout("eax") errno, out("ecx") result_low, out("edx") result_high);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
        let result = result_low as u64 | ((result_high as u64) << 32);

        Ok(result)
    }
}

#[inline(always)]
pub fn truncate(desc: &FileDescriptor, size: usize) -> Result<(), Errno> {
    let errno: u32;

    unsafe {
        asm!("int 0x80", in("eax") Syscall::Truncate as u32, in("ebx") desc.0, in("ecx") size, lateout("eax") errno);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
        Ok(())
    }
}

#[inline(always)]
pub fn stat(desc: &FileDescriptor, stat_struct: &mut FileStatus) -> Result<(), Errno> {
    let errno: u32;

    unsafe {
        asm!("int 0x80", in("eax") Syscall::Stat as u32, in("ebx") desc.0, in("ecx") stat_struct as *mut FileStatus, lateout("eax") errno);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
        Ok(())
    }
}
