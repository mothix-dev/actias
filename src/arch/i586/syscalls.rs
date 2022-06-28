//! i586 syscall handlers

use core::ffi::CStr;

use crate::{
    tasks::{IN_TASK, get_current_task, get_current_task_mut},
    arch::tasks::{exit_current_task, fork_task},
};
use super::ints::SyscallRegisters;

/// amount of syscalls we have
pub const NUM_SYSCALLS: usize = 6;

/// list of function pointers for all available syscalls
pub static SYSCALL_LIST: [fn(&mut SyscallRegisters) -> (); NUM_SYSCALLS] = [
    is_computer_on,
    test_log,
    fork,
    exit,
    get_pid,
    exec,
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

    //log!("string @ {:#x}", regs.ebx);

    let string = unsafe { CStr::from_ptr(regs.ebx as *const _).to_string_lossy().into_owned() };
    log!("{}", string);

    unsafe { IN_TASK = true; }
}

/// forks task
/// sets ebx to the child pid in parent task, 0 in child task
pub fn fork(regs: &mut SyscallRegisters) {
    unsafe { IN_TASK = false; }

    // save state of current task
    get_current_task_mut().unwrap().state.save(regs);

    let new_task =
        match fork_task(get_current_task().unwrap().id) {
            Ok(task) => task,
            Err(msg) => panic!("could not fork task: {}", msg), // do we really want to bring the whole system down if we can't fork a process?
        };

    // identify parent and child tasks
    regs.ebx = new_task.id.try_into().unwrap();
    new_task.state.registers.ebx = 0;

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

/// replaces this process's address space with that of a new process at the path provided
pub fn exec(regs: &mut SyscallRegisters) {
    unsafe { IN_TASK = false; }

    let path = unsafe { CStr::from_ptr(regs.ebx as *const _).to_string_lossy().into_owned() };

    debug!("exec()ing {} as pid {}", path, get_current_task().unwrap().id);

    let mut new_task = get_current_task().unwrap().recreate();

    if let Err(err) = crate::exec::exec_as(&mut new_task, &path) {
        log!("error exec()ing: {}", err);

        new_task.state.free_pages();
    } else {
        let mut task = get_current_task_mut().unwrap();

        // free pages of old task
        task.state.free_pages();

        // exec_as only touches the state, so we can just replace the state and be fine
        task.state = new_task.state;

        // page table has been replaced, so switch to it
        task.state.pages.switch_to();

        // registers have been changed, update them
        *regs = task.state.registers;
    }

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
