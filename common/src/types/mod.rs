pub mod errno;
pub mod syscalls;

pub use errno::*;
pub use syscalls::*;

pub type Result<T> = core::result::Result<T, Errno>;

use core::fmt;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct ProcessID {
    pub process: u32,
    pub thread: u32,
}

impl fmt::Display for ProcessID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.process, self.thread)
    }
}
