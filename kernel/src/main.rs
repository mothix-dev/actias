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
pub mod task;
pub mod timer;
pub mod util;

use log::error;

#[panic_handler]
pub fn panic_implementation(info: &core::panic::PanicInfo) -> ! {
    let thread_id = arch::get_thread_id();

    let (file, line) = match info.location() {
        Some(loc) => (loc.file(), loc.line()),
        None => ("", 0),
    };

    if let Some(m) = info.message() {
        error!("PANIC (CPU {thread_id}): {m} @ {file}:{line}");
    } else if let Some(m) = info.payload().downcast_ref::<&str>() {
        error!("PANIC (CPU {thread_id}): {m} @ {file}:{line}");
    } else {
        error!("PANIC (CPU {thread_id}) @ {file}:{line}");
    }

    unsafe {
        arch::halt();
    }
}
