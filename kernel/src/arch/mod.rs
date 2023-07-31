pub mod bsp;
pub mod i586;

#[cfg(target_arch = "i586")]
pub use i586::*;
