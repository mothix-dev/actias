#![feature(panic_info_message)] //< Panic handling
#![feature(abi_x86_interrupt)]
//#![feature(llvm_asm)] //< As a kernel, we need inline assembly
#![no_std]  //< Kernels can't use std
#![no_main]
#![crate_name="ockernel"]
#![allow(clippy::missing_safety_doc)] // dont really want to write safety docs yet

/// Macros, need to be loaded before everything else due to how rust parses
#[macro_use]
mod macros;

// Architecture-specific modules
#[path="arch/i586/mod.rs"]
pub mod arch;

/// Exception handling (panic)
pub mod unwind;

/// Logging code
mod logging;

use core::arch::asm;

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");

static TEXT: &[u8] = b"UwU";

// kernel entrypoint (called by arch/<foo>/boot.S)
#[no_mangle]
#[allow(clippy::empty_loop)]
pub extern fn kmain() -> ! {
    log!("booting {} v{}", NAME, VERSION);

    unsafe {
        log!("initializing GDT");
        arch::gdt::init();
        log!("initializing interrupts");
        arch::ints::init();
    }

    /*let mut asdf = 123;
    let mut ghjk = 123;
    asdf = 0;
    if asdf == 0 {
        ghjk /= asdf;
    }
    log!("UwU {}", ghjk);
    log!("OwO");*/

    /*log!("breakpoint test");

    unsafe {
        asm!("int3");
    }

    log!("no crash lfg");*/

    /*log!("page fault test");

    // trigger a page fault
    unsafe {
        *(0xdeadbeef as *mut u32) = 42;
    };*/

    /*log!("stack overflow test");

    #[allow(unconditional_recursion)]
    fn stack_overflow() {
        stack_overflow(); // for each recursion, the return address is pushed
        stack_overflow(); // we need this one to actually fuck it up
    }

    // trigger a stack overflow
    stack_overflow();*/

    log!("vga test");

    let vga_buffer = 0xb8000 as *mut u8;

    for (i, &byte) in TEXT.iter().enumerate() {
        unsafe {
            *vga_buffer.offset(i as isize * 2) = byte;
            *vga_buffer.offset(i as isize * 2 + 1) = 0xb;
        }
    }

    log!("no crash?");

    unsafe {
        asm!("hlt");
    }

    loop {}
}
