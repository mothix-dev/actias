pub mod errno;
pub mod syscalls;

pub use errno::Errno;
pub use syscalls::Syscalls;

pub type Result<T> = core::result::Result<T, Errno>;
