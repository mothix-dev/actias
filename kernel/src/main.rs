#![no_std]
#![no_main]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]
#![feature(core_c_str)]
#![feature(cstr_from_bytes_until_nul)]

extern crate alloc;

use alloc::alloc::Layout;
use core::arch::asm;
use common::mm::heap::CustomAlloc;
use log::{debug, error, info, trace, warn};

pub const NAME: &str = env!("CARGO_PKG_NAME");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[global_allocator]
static ALLOCATOR: CustomAlloc = CustomAlloc;

#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    panic!("allocation error with layout {:?}", layout);
}

#[panic_handler]
pub fn panic_implementation(info: &core::panic::PanicInfo) -> ! {
    let (file, line) = match info.location() {
        Some(loc) => (loc.file(), loc.line()),
        None => ("", 0),
    };

    if let Some(m) = info.message() {
        error!("PANIC: file='{}', line={} :: {}", file, line, m);
    } else if let Some(m) = info.payload().downcast_ref::<&str>() {
        error!("PANIC: file='{}', line={} :: {}", file, line, m);
    } else {
        error!("PANIC: file='{}', line={} :: ?", file, line);
    }

    loop {
        unsafe {
            asm!("cli; hlt");
        }
    }
}

#[no_mangle]
pub extern "cdecl" fn _start() -> ! {
    // initialize our logger
    common::logger::init().unwrap();

    //info!("{} v{}", NAME, VERSION);
    info!("Hellorld!");

    loop {
        unsafe {
            asm!("hlt");
        }
    }
}
