use core::{
    arch::asm,
    fmt,
};
use bitmask_enum::bitmask;
use crate::types::{Syscall, Errno};

#[bitmask(u8)]
pub enum OpenFlags {
    #[num_enum(default)]
    None        = Self(0),
    Read        = Self(1 << 0),
    Write       = Self(1 << 1),
    Append      = Self(1 << 2),
    Create      = Self(1 << 3),
    Truncate    = Self(1 << 4),
    NonBlocking = Self(1 << 5),
}

#[repr(u8)]
pub enum SeekKind {
    /// set file writing offset to provided offset
    Set = 0,

    /// add the provided offset to the current file offset
    Current,

    /// set the file offset to the end of the file plus the provided offset
    End,
}

#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
pub struct FileDescriptor(pub usize);

impl fmt::Write for FileDescriptor {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        write(self, s.as_bytes()).map_err(|_| fmt::Error)?;
        Ok(())
    }
}

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

    unsafe {
        asm!("int 0x80", in("eax") Syscall::Open as u32, in("ebx") &path[0] as *const _, in("ecx") flags.0 as usize, lateout("ebx") result, lateout("eax") errno);
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
pub fn seek(desc: &FileDescriptor, offset: isize, kind: SeekKind) -> Result<u32, Errno> {
    let result: u32;
    let errno: u32;

    unsafe {
        asm!("int 0x80", in("eax") Syscall::Seek as u32, in("ebx") desc.0, in("ecx") offset, in("edx") kind as usize, lateout("eax") errno, lateout("ebx") result);
    }

    if errno != 0 {
        Err(Errno::from(errno))
    } else {
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
