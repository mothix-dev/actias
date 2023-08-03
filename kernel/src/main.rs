#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(allocator_api)]
#![feature(asm_const)]
#![feature(generators)]
#![feature(iter_from_generator)]
#![feature(naked_functions)]
#![feature(new_uninit)]
#![feature(panic_info_message)]
#![feature(pointer_byte_offsets)]
#![feature(trait_alias)]

extern crate alloc;

pub mod arch;
pub mod array;
pub mod cpu;
pub mod mm;
pub mod platform;
pub mod process;
pub mod sched;
pub mod timer;

use core::{fmt, fmt::LowerHex};
use log::{error, info};

pub struct FormatHex<T: LowerHex>(pub T);

impl<T: LowerHex> fmt::Debug for FormatHex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

#[panic_handler]
pub fn panic_implementation(info: &core::panic::PanicInfo) -> ! {
    let (file, line) = match info.location() {
        Some(loc) => (loc.file(), loc.line()),
        None => ("", 0),
    };

    if let Some(m) = info.message() {
        error!("PANIC: \"{m}\" @ {file}:{line}");
    } else if let Some(m) = info.payload().downcast_ref::<&str>() {
        error!("PANIC: \"{m}\" @ {file}:{line}");
    } else {
        error!("PANIC @ {file}:{line}");
    }

    (crate::arch::PROPERTIES.halt)();
}

pub fn init_message() {
    info!(
        "ockernel {} (built at {} with rustc {}, LLVM {} on {})",
        env!("VERGEN_BUILD_SEMVER"),
        env!("VERGEN_BUILD_TIMESTAMP"),
        env!("VERGEN_RUSTC_SEMVER"),
        env!("VERGEN_RUSTC_LLVM_VERSION"),
        env!("VERGEN_RUSTC_HOST_TRIPLE")
    );
}
