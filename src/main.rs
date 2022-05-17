#![feature(panic_info_message)] //< Panic handling
#![feature(abi_x86_interrupt)]
//#![feature(llvm_asm)] //< As a kernel, we need inline assembly
#![no_std]  //< Kernels can't use std
#![no_main]
#![crate_name="ockernel"]
#![allow(clippy::missing_safety_doc)] // dont really want to write safety docs yet

#![feature(custom_test_frameworks)]
#![test_runner(crate::test::test_runner)]
#![reexport_test_harness_main = "test_main"]

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

/// Exception handling (panic)
pub mod unwind;

/// Logging code
mod logging;

/// text mode console
mod console;

/// memory management
pub mod mm;

/// various utility things
pub mod util;

/// tests
#[cfg(test)]
pub mod test;

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");

use platform::vga::*;
use console::*;

#[cfg(test)]
use platform::debug::exit_success;

// kernel entrypoint (called by arch/<foo>/boot.S)
#[no_mangle]
pub extern fn kmain() -> ! {
    log!("booting {} v{}", NAME, VERSION);

    // initialize kernel
    arch::init(); // platform specific initialization
    mm::init();

    #[cfg(test)]
    {
        test_main();
        exit_success();
    }

    #[cfg(not(test))]
    {
        log!("initializing console");
        let mut raw = create_console();
        let mut console = SimpleConsole::new(&mut raw, 80, 25);

        console.clear();
        console.puts(NAME);
        console.puts(" v");
        console.puts(VERSION);
        console.puts("\n\n");

        console.puts("UwU\n");


    }

    arch::halt();
}
