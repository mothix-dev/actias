//! misc types

pub mod errno;
pub mod keysym;
pub mod signals;
pub mod syscalls;

// re-export these types to save on typing
pub use errno::Errno;
pub use keysym::KeySym;
pub use signals::Signal;
pub use syscalls::Syscall;
