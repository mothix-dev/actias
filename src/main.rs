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

pub mod vfs;

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
    fn enter_user_mode(ptr: u32, stack: u32) -> !;
    //fn enter_user_mode(ptr: u32) -> !;
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
    switch_to_user_mode(user_mode_test as *const _);
}

use core::arch::asm;
use tasks::{IN_TASK, Task, add_task, get_current_task_mut};
use syscalls::Syscalls;
use arch::{LINKED_BASE, PAGE_SIZE};

/// set up stack, multitasking, switch to user mode
pub fn switch_to_user_mode(ptr: *const u32) -> ! {
    debug!("creating task");

    let mut task = Task::new();

    // map page at top of user memory (right below kernel memory) for stack
    debug!("allocating stack");

    task.state.alloc_page((LINKED_BASE - PAGE_SIZE) as u32, false, true, false);

    debug!("adding task");
    
    add_task(task);

    debug!("switching page tables");

    get_current_task_mut().expect("no tasks?").state.pages.switch_to();

    log!("entering user mode @ {:#x}", ptr as u32);

    unsafe {
        IN_TASK = true;
        
        enter_user_mode(ptr as u32, (LINKED_BASE - 1) as u32); // this also enables interrupts, effectively enabling task switching
    }
}

#[inline(always)]
unsafe fn syscall_is_computer_on() -> bool {
    let result: u32;
    asm!("int 0x80", in("eax") Syscalls::IsComputerOn as u32, out("ebx") result);

    result > 0
}

#[inline(always)]
unsafe fn syscall_test_log(string: &[u8]) {
    asm!("int 0x80", in("eax") Syscalls::TestLog as u32, in("ebx") &string[0] as *const _);
}

#[inline(always)]
unsafe fn syscall_fork() -> u32 {
    let result: u32;
    asm!("int 0x80", in("eax") Syscalls::Fork as u32, out("ebx") result);

    result
}

#[inline(always)]
#[allow(clippy::empty_loop)]
unsafe fn syscall_exit() {
    asm!("int 0x80", in("eax") Syscalls::Exit as u32);
    loop {}
}

#[inline(always)]
unsafe fn syscall_get_pid() -> u32 {
    let result: u32;
    asm!("int 0x80", in("eax") Syscalls::GetPID as u32, out("ebx") result);

    result
}

unsafe extern fn user_mode_test() -> ! {
    if syscall_is_computer_on() {
        syscall_test_log(b"computer is on\0");
    } else {
        syscall_test_log(b"computer is not on\0");
    }

    let ptr = (LINKED_BASE - PAGE_SIZE) as *mut u16;

    *ptr = 621;

    if syscall_fork() != 0 {
        syscall_test_log(b"parent\0");

        if *ptr == 621 {
            syscall_test_log(b"parent: preserved\0");
        }
    } else {
        syscall_test_log(b"child\0");

        if *ptr == 621 {
            syscall_test_log(b"child: preserved\0");
        }

        syscall_exit();
    }

    let proc = syscall_fork();

    if proc != 0 {
        for _i in 0..8 {
            syscall_test_log(b"OwO\0");

            for _i in 0..1024 * 1024 * 128 { // slow things down
                asm!("nop");
            }
        }

        asm!("int3"); // effectively crash this process

        loop {}
    } else {
        loop {
            syscall_test_log(b"UwU\0");

            for _i in 0..1024 * 1024 * 128 { // slow things down
                asm!("nop");
            }
        }
    }

    //loop {}
}
