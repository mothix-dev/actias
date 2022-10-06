#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(cstr_from_bytes_until_nul)]
#![feature(panic_info_message)]
#![feature(abi_x86_interrupt)]
#![feature(naked_functions)]

extern crate alloc;

// architecture specific code
#[cfg(target_arch = "i586")]
#[path = "arch/i586/mod.rs"]
pub mod arch;

// platform specific code
#[cfg(target_platform = "ibmpc")]
#[path = "platform/ibmpc/mod.rs"]
pub mod platform;

pub mod mm;
pub mod util;
pub mod task;
pub mod timer;

use log::error;

#[panic_handler]
pub fn panic_implementation(info: &core::panic::PanicInfo) -> ! {
    let (file, line) = match info.location() {
        Some(loc) => (loc.file(), loc.line()),
        None => ("", 0),
    };

    if let Some(m) = info.message() {
        error!("PANIC: {m} @ {file}:{line}");
    } else if let Some(m) = info.payload().downcast_ref::<&str>() {
        error!("PANIC: {m} @ {file}:{line}");
    } else {
        error!("PANIC @ {file}:{line}");
    }

    unsafe {
        arch::halt();
    }
}
