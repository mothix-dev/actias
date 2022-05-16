#![feature(panic_info_message)] //< Panic handling
#![feature(abi_x86_interrupt)]
//#![feature(llvm_asm)] //< As a kernel, we need inline assembly
#![no_std]  //< Kernels can't use std
#![no_main]
#![crate_name="ockernel"]
#![allow(clippy::missing_safety_doc)] // dont really want to write safety docs yet

#![feature(custom_test_frameworks)]
#![test_runner(crate::test_runner)]
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

#[allow(unused_imports)]
use core::arch::asm;

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
fn test_runner(tests: &[&dyn Fn()]) {
    log!("Running {} tests", tests.len());
    for test in tests {
        test();
    }
}

use platform::vga::*;
use console::*;
use mm::heap::{KERNEL_HEAP, alloc, free};

// kernel entrypoint (called by arch/<foo>/boot.S)
#[no_mangle]
pub extern fn kmain() -> ! {
    log!("booting {} v{}", NAME, VERSION);

    // initialize kernel
    arch::init(); // platform specific initialization
    mm::init();

    #[cfg(test)]
    test_main();

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

        unsafe {
            log!("{:?}", KERNEL_HEAP);
        }

        let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };

        heap.print_holes();

        let a = alloc::<u32>(8);
        let b = alloc::<u32>(8);
        log!("a (8): {:#x}", a as usize);
        log!("b (8): {:#x}", b as usize);

        heap.print_holes();

        log!("free a");
        free(a);

        heap.print_holes();

        log!("free b");
        free(b);

        heap.print_holes();

        assert!(heap.index.size == 1);

        let c = alloc::<u32>(12);
        log!("c (12): {:#x}", c as usize);

        assert!(c == a);

        let d = alloc::<u32>(1024);
        log!("d (1024): {:#x}", d as usize);

        let e = alloc::<u32>(16);
        log!("e (16): {:#x}", e as usize);

        heap.print_holes();

        log!("free c");
        free(c);

        heap.print_holes();

        let f = alloc::<u32>(12);
        log!("f (12): {:#x}", f as usize);

        heap.print_holes();

        assert!(f == c);

        log!("free e");
        free(e);

        log!("free d");
        free(d);

        log!("free f");
        free(f);

        assert!(heap.index.size == 1);

        heap.print_holes();

        let g = alloc::<u32>(8);
        log!("g (8): {:#x}", g as usize);

        heap.print_holes();

        assert!(g == a);

        let h = alloc::<u32>(2048);
        log!("h (2048): {:#x}", h as usize);

        heap.print_holes();

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

        log!("no crash lfg");

        log!("page fault test");

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

        //log!("vga test");

        /*let vga_buffer = 0xc00b8000 as *mut u8; // first 4 mb is mapped to upper 1 gb, including video ram lmao

        for (i, &byte) in TEXT.iter().enumerate() {
            unsafe {
                *vga_buffer.offset(i as isize * 2) = byte;
                *vga_buffer.offset(i as isize * 2 + 1) = 0xb;
            }
        }*/

        //log!("no crash?");

        /*let mut writer = Writer {
            column_position: 0,
            color_code: ColorCode::new(Color::LightGray, Color::Black),
            buffer: unsafe { &mut *(0xc00b8000 as *mut Buffer) },
        };

        writer.write_string("UwU OwO");*/

        /*let mut console = create_console();
        let color = ColorCode {
            foreground: Color::LightGray,
            background: Color::Black,
        };

        console.clear(0, 0, 80, 25);
        console.write_char(0, 0, color, b'U');
        console.write_char(1, 0, color, b'w');
        console.write_char(2, 0, color, b'U');
        console.write_string(4, 0, color, "OwO");*/

        /*loop {
            for bg in 0..16 {
                for fg in 0..16 {
                    console.color = ColorCode {
                        foreground: fg.into(),
                        background: bg.into()
                    };
                    console.puts("OwO ");
                }
            }
        }*/
        /*console.puts("binawy uwu\n");
        console.puts("UwU UwU UwU UwU\n");
        console.puts("UwU UwU UwU OwO\n");
        console.puts("UwU UwU OwO UwU\n");
        console.puts("UwU UwU OwO OwO\n");
        console.puts("UwU OwO UwU UwU\n");
        console.puts("UwU OwO UwU OwO\n");
        console.puts("UwU OwO OwO UwU\n");
        console.puts("UwU OwO OwO OwO\n");
        console.puts("OwO UwU UwU UwU\n");
        console.puts("OwO UwU UwU OwO\n");
        console.puts("OwO UwU OwO UwU\n");
        console.puts("OwO UwU OwO OwO\n");
        console.puts("OwO OwO UwU UwU\n");
        console.puts("OwO OwO UwU OwO\n");
        console.puts("OwO OwO OwO UwU\n");
        console.puts("OwO OwO OwO OwO\n");

        //console.raw.copy(0, 3, console.height - 3);

        console.puts("UwU UwU UwU UwU\n");
        console.puts("UwU UwU UwU OwO\n");
        console.puts("UwU UwU OwO UwU\n");
        console.puts("UwU UwU OwO OwO\n");
        console.puts("UwU OwO UwU UwU\n");
        console.puts("UwU OwO UwU OwO\n");
        console.puts("UwU OwO OwO UwU\n");
        console.puts("UwU OwO OwO OwO\n");
        console.puts("OwO UwU UwU UwU\n");
        console.puts("OwO UwU UwU OwO\n");
        console.puts("OwO UwU OwO UwU\n");
        console.puts("OwO UwU OwO OwO\n");
        console.puts("OwO OwO UwU UwU\n");
        console.puts("OwO OwO UwU OwO\n");
        console.puts("OwO OwO OwO UwU\n");
        console.puts("OwO OwO OwO OwO\n");
        console.puts("binawy owo\n");*/

        /*for i in 0..16 {
            for bg in 0..16 {
                for fg in 0..16 {
                    console.color = ColorCode {
                        foreground: fg.into(),
                        background: bg.into()
                    };
                    console.puts("UwU ");
                }
            }
        }
        console.color = ColorCode::default();
        console.puts("\nOwO\n");*/
    }

    arch::halt();
}
