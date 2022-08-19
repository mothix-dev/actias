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

// idk where to put this so its going here

/// describes a module as passed to the kernel from the loader
#[derive(Debug)]
pub struct BootModule {
    /// the start of this module, in physical memory (not virtual! this will not be mapped into the kernel's memory by default)
    pub start: u64,

    /// the end of this module, in physical memory
    pub end: u64,

    /// a reference to a string identifying this module, typically its filename
    pub string: &'static str,
}
