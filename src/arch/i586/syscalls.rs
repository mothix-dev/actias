//! i586 syscall handlers

use core::ffi::CStr;
use super::ints::SyscallRegisters;
use crate::tasks::{IN_TASK, CURRENT_TASK, get_current_task, get_current_task_mut};
use crate::arch::tasks::{exit_current_task, fork_task};

/// amount of syscalls we have
pub const NUM_SYSCALLS: usize = 5;

/// list of function pointers for all available syscalls
pub static SYSCALL_LIST: [fn(&mut SyscallRegisters) -> (); NUM_SYSCALLS] = [
    is_computer_on,
    test_log,
    fork,
    exit,
    get_pid,
];

/// is computer on?
/// sets ebx to 1 (true) if computer is on
/// if computer is off, behavior is undefined
pub fn is_computer_on(regs: &mut SyscallRegisters) {
    regs.ebx = 1;
}

/// test syscall- logs a string
pub fn test_log(regs: &mut SyscallRegisters) {
    unsafe { IN_TASK = false; }

    let string = unsafe { CStr::from_ptr(regs.ebx as *const _).to_string_lossy().into_owned() };
    log!("{}", string);

    unsafe { IN_TASK = true; }
}

/// forks task
/// sets ebx to 0 in parent task, 1 in child task
pub fn fork(regs: &mut SyscallRegisters) {
    unsafe { IN_TASK = false; }

    // save state of current task
    get_current_task_mut().unwrap().state.save(regs);

    let new_task =
        match fork_task(unsafe { CURRENT_TASK }) {
            Ok(task) => task,
            Err(msg) => panic!("could not fork task: {}", msg),
        };

    // identify parent and child tasks
    regs.ebx = 0;
    new_task.state.registers.ebx = 1;

    unsafe { IN_TASK = true; }
}

/// exits task
pub fn exit(_regs: &mut SyscallRegisters) {
    exit_current_task();
}

/// gets id of current task
/// sets ebx to id
pub fn get_pid(regs: &mut SyscallRegisters) {
    unsafe { IN_TASK = false; }

    regs.ebx = get_current_task().expect("no current task").id.try_into().unwrap();

    unsafe { IN_TASK = true; }
}

/// platform-specific syscall handler
#[no_mangle]
pub unsafe extern "C" fn syscall_handler(mut regs: SyscallRegisters) {
    let syscall_num = regs.eax as usize;

    if syscall_num < NUM_SYSCALLS {
        SYSCALL_LIST[syscall_num](&mut regs);
    }
}
