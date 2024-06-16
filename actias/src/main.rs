#![no_std]
#![no_main]
#![feature(panic_info_message)]
#![feature(let_chains)]
#![warn(clippy::too_many_lines)]
#![warn(clippy::if_not_else)]
#![warn(clippy::match_same_arms)]
#![warn(clippy::unreadable_literal)]
#![warn(clippy::cast_lossless)]
#![warn(clippy::implicit_clone)]
#![warn(clippy::manual_is_variant_and)]
#![warn(clippy::explicit_iter_loop)]
#![warn(clippy::semicolon_if_nothing_returned)]
#![warn(clippy::default_trait_access)]
#![warn(clippy::ptr_as_ptr)]
#![warn(clippy::wildcard_imports)]
#![warn(clippy::trivially_copy_pass_by_ref)]
#![warn(clippy::redundant_closure_for_method_calls)]

pub mod platform;

use log::error;

#[panic_handler]
pub fn panic_implementation(info: &core::panic::PanicInfo) -> ! {
    let (file, line) = match info.location() {
        Some(loc) => (loc.file(), loc.line()),
        None => ("", 0),
    };

    if let Some(m) = info.message() {
        error!("PANIC: {m} at {file}:{line}");
    } else if let Some(m) = info.payload().downcast_ref::<&str>() {
        error!("PANIC: {m} at {file}:{line}");
    } else {
        error!("PANIC (no message) at {file}:{line}");
    }

    loop {}
}
