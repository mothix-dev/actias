#![no_std]
#![feature(panic_info_message)]
#![feature(abi_x86_interrupt)]
#![feature(core_c_str)]

extern crate alloc;

// architecture specific code
#[cfg(target_arch = "i586")]
#[path = "arch/i586/mod.rs"]
pub mod arch;

pub mod mm;
pub mod types;
pub mod util;
