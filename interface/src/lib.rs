#![no_std]

#[path="../../src/types/mod.rs"]
pub mod types;

pub mod syscalls;

#[macro_export]
macro_rules! print {
    ( $($arg:tt)* ) => ({
        use core::fmt::Write;
        let _ = write!(&mut interface::syscalls::FileDescriptor(1), $($arg)*);
    })
}

#[macro_export]
macro_rules! println {
    ( $($arg:tt)* ) => ({
        use core::fmt::Write;
        let _ = write!(&mut interface::syscalls::FileDescriptor(1), $($arg)*);
        let _ = write!(&mut interface::syscalls::FileDescriptor(1), "\n");
    })
}

#[macro_export]
macro_rules! eprint {
    ( $($arg:tt)* ) => ({
        use core::fmt::Write;
        let _ = write!(&mut interface::syscalls::FileDescriptor(2), $($arg)*);
    })
}

#[macro_export]
macro_rules! eprintln {
    ( $($arg:tt)* ) => ({
        use core::fmt::Write;
        let _ = write!(&mut interface::syscalls::FileDescriptor(2), $($arg)*);
        let _ = write!(&mut interface::syscalls::FileDescriptor(2), "\n");
    })
}
