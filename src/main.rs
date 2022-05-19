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

// we need this to effectively use our heap
extern crate alloc;

pub const NAME: &str = env!("CARGO_PKG_NAME");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
use platform::debug::exit_success;

extern "C" {
    fn loop_test() -> !;
    fn enter_user_mode(ptr: u32) -> !;
}

// kernel entrypoint (called by arch/<foo>/boot.S)
#[no_mangle]
pub extern fn kmain() -> ! {
    // initialize kernel
    arch::init(); // platform specific initialization

    mm::init(); // init memory management/heap/etc

    console::init(); // init console

    log!("{} v{}", NAME, VERSION);

    #[cfg(test)]
    {
        test_main();
        exit_success();
    }

    #[cfg(not(test))]
    {
        log!("UwU");

        unsafe {
            //*(0xdeadbeef as *mut u32) = 3621; // page fault lmao
            let ptr = (&(user_mode_test as fn()) as *const _) as usize;
            //let ptr = (&loop_test as *const _) as u32;
            //let ptr = 0x62100000;
            log!("fn @ {:#x}", ptr);
            enter_user_mode_2(0xc0108494);
        }
    }

    arch::halt();
}

use core::arch::asm;

unsafe fn enter_user_mode_2(fn_ptr: u32) -> ! {
    asm!(
        "mov ecx, 0xc010c260",
        "jmp enter_user_mode",

        //in(reg) fn_ptr,
    );
    loop {}
}

/*unsafe fn enter_user_mode(fn_ptr: u32) {
    /*asm!(
        "mov ax, 0x23",
        "mov ds, ax",
        "mov es, ax",
        "mov fs, ax",
        "mov gs, ax",

        /*"mov eax, esp",
        "push 0x23",
        "push eax",
        "pushf",
        "push 0x1b",
        "push ebx",
        "iret",*/

        "xor edx, edx",
        "mov eax, 0x100008",
        "mov ecx, 0x174",
        "wrmsr",

        "mov edx, ebx",
        "mov ecx, esp",
        "sysexit",

        in("ebx") fn_ptr,
        out("eax") _,
        //out("ecx") _,
        //out("edx") _,
    );*/
    /*
    // Set up a stack structure for switching to user mode.
    asm volatile("  \
        cli; \
        mov $0x23, %ax; \
        mov %ax, %ds; \
        mov %ax, %es; \
        mov %ax, %fs; \
        mov %ax, %gs; \
                    \
        mov %esp, %eax; \
        pushl $0x23; \
        pushl %eax; \
        pushf; \
        pushl $0x1B; \
        push $1f; \
        iret; \
    1: \
        ");
    */
    asm!(
        "cli",
        "mov ax, 0x23",
        "mov ds, ax",
        "mov es, ax",
        "mov fs, ax",
        "mov gs, ax",

        "mov eax, esp",
        "push 0x23",
        "push eax",
        "pushf",
        /*"pop eax",
        "or eax, 0x200",
        "push eax",*/
        "push 0x1b",
        "push {0}",
        "iret",

        in(reg) fn_ptr,
        out("eax") _,
    );

}*/

fn user_mode_test() {
    /*unsafe {
        asm!("cli");
    }*/
    loop {}
}
