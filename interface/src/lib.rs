#![no_std]

#[path="../../src/types/mod.rs"]
pub mod types;

pub mod syscalls;

use core::fmt;

#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
pub struct FileDescriptor(pub usize);

impl fmt::Write for FileDescriptor {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        syscalls::write(self, s.as_bytes()).map_err(|_| fmt::Error)?;
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ( $($arg:tt)* ) => ({
        use core::fmt::Write;
        let _ = write!(&mut interface::FileDescriptor(1), $($arg)*);
    })
}

#[macro_export]
macro_rules! println {
    ( $($arg:tt)* ) => ({
        use core::fmt::Write;
        let _ = write!(&mut interface::FileDescriptor(1), $($arg)*);
        let _ = write!(&mut interface::FileDescriptor(1), "\n");
    })
}

#[macro_export]
macro_rules! eprint {
    ( $($arg:tt)* ) => ({
        use core::fmt::Write;
        let _ = write!(&mut interface::FileDescriptor(2), $($arg)*);
    })
}

#[macro_export]
macro_rules! eprintln {
    ( $($arg:tt)* ) => ({
        use core::fmt::Write;
        let _ = write!(&mut interface::FileDescriptor(2), $($arg)*);
        let _ = write!(&mut interface::FileDescriptor(2), "\n");
    })
}
