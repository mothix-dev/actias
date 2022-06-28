#![crate_name="ockernel"]

#![no_std]
#![no_main]

#![feature(panic_info_message)]

#![feature(abi_x86_interrupt)]

#![feature(custom_test_frameworks)]
#![test_runner(crate::test::test_runner)]
#![reexport_test_harness_main = "test_main"]

#![feature(alloc_error_handler)]

#![allow(clippy::missing_safety_doc)] // dont really want to write safety docs yet

#![feature(core_c_str)]
#![feature(cstr_from_bytes_until_nul)]

#![feature(arbitrary_enum_discriminant)] // we want errno to be numbered properly yet have a custom string field

/// Macros, need to be loaded before everything else due to how rust parses
#[macro_use]
mod macros;

// architecture specific modules
#[path="arch/i586/mod.rs"]
#[cfg(target_arch = "i586")]
pub mod arch;

// platform specific modules
#[path="platform/ibmpc/mod.rs"]
#[cfg(target_platform = "ibmpc")]
pub mod platform;

pub mod unwind;

mod logging;

pub mod console;

pub mod mm;

pub mod util;

pub mod tasks;
pub mod syscalls;

pub mod fs;

pub mod errno;

pub mod tar;

pub mod exec;

pub mod keysym;

/// tests
#[cfg(test)]
pub mod test;

// we need this to effectively use our heap
extern crate alloc;

pub const NAME: &str = env!("CARGO_PKG_NAME");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
use platform::debug::exit_success;

use crate::arch::tasks::idle_until_switch;

// kernel entrypoint (called by arch/<foo>/boot.S)
#[no_mangle]
pub extern fn kmain() -> ! {
    // initialize kernel
    arch::init(); // platform specific initialization

    mm::init(); // init memory management/heap/etc

    console::init(); // init console

    fs::init(); // init filesystems

    log!("{} v{}", NAME, VERSION);

    #[cfg(test)]
    {
        test_main();
        exit_success();
    }

    #[cfg(not(test))]
    {
        exec::exec("/fs/initrd/test-bin").unwrap();

        idle_until_switch(); // this also enables multitasking
    }
}
