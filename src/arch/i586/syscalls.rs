//! i586 syscall handlers

use core::ffi::CStr;
use super::ints::SyscallRegisters;
use crate::tasks::{Task, get_current_task_mut, add_task, TASKS};
use crate::arch::tasks::TaskState;
use crate::arch::paging::{PAGE_DIR, PageDirectory};
use crate::arch::LINKED_BASE;

/// is computer on?
/// sets ebx to 1 (true) if computer is on
/// if computer is off, behavior is undefined
pub fn is_computer_on(regs: &mut SyscallRegisters) {
    regs.ebx = 1;
}

/// test syscall- logs a string
pub fn test_log(regs: &mut SyscallRegisters) {
    let string = unsafe { CStr::from_ptr(regs.ebx as *const _).to_string_lossy().into_owned() };
    log!("{}", string);
}

/// forks task
/// sets ebx to 0 in parent task, 1 in child task
pub fn fork(regs: &mut SyscallRegisters) {
    let current = get_current_task_mut().expect("no tasks?");

    // save state of current task
    current.state.save(regs);

    // create new task state
    let mut state = TaskState {
        registers: current.state.registers,
        pages: PageDirectory::new(),
        page_updates: current.state.page_updates,
    };

    // copy kernel pages, copy parent task's pages as copy on write
    let kernel_start = LINKED_BASE >> 22;
    let dir = unsafe { PAGE_DIR.as_mut().unwrap() };
    state.copy_on_write_from(&mut current.state.pages, 0, kernel_start);
    state.copy_pages_from(dir, kernel_start, 1024);

    // set ebx of child
    state.registers.ebx = 1;

    add_task(Task {
        id: unsafe { TASKS.len() },
        state,
    });

    // set ebx contents of parent
    regs.ebx = 0;
}

/// amount of syscalls we have
pub const NUM_SYSCALLS: usize = 3;

/// list of function pointers for all available syscalls
pub static SYSCALL_LIST: [fn(&mut SyscallRegisters) -> (); NUM_SYSCALLS] = [
    is_computer_on,
    test_log,
    fork,
];

/// platform-specific syscall handler
#[no_mangle]
pub unsafe extern "C" fn syscall_handler(mut regs: SyscallRegisters) {
    let syscall_num = regs.eax as usize;

    if syscall_num < NUM_SYSCALLS {
        SYSCALL_LIST[syscall_num](&mut regs);
    }
}

