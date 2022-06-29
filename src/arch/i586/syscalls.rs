//! i586 syscall handlers

use alloc::{
    vec, vec::Vec,
    string::String,
};
use core::{
    ffi::CStr,
    mem::size_of,
};
use crate::{
    tasks::{IN_TASK, get_current_task, get_current_task_mut},
    arch::tasks::{exit_current_task, fork_task},
    types::Errno,
};
use super::ints::SyscallRegisters;

/// function prototype for individual syscall handlers
type SyscallHandler = fn(&mut SyscallRegisters) -> Result<(), Errno>;

/// amount of syscalls we have
pub const NUM_SYSCALLS: usize = 6;

/// list of function pointers for all available syscalls
pub static SYSCALL_LIST: [SyscallHandler; NUM_SYSCALLS] = [
    is_computer_on,
    test_log,
    fork,
    exit,
    get_pid,
    exec,
];

/// makes sure a pointer is valid
/// 
/// wrapper around TaskState.check_ptr to make usage easier in syscall handlers
fn check_ptr(ptr: *const u8) -> Result<(), Errno> {
    if get_current_task_mut().unwrap().state.check_ptr(ptr) {
        Ok(())
    } else {
        Err(Errno::BadAddress)
    }
}

/// is computer on?
/// 
/// sets ebx to 1 (true) if computer is on
/// 
/// if computer is off, behavior is undefined
pub fn is_computer_on(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    regs.ebx = 1;

    Ok(())
}

/// test syscall- logs a string
pub fn test_log(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    check_ptr(regs.ebx as *const _)?;
    let string = unsafe { CStr::from_ptr(regs.ebx as *const _).to_string_lossy() };

    unsafe { IN_TASK = false; }

    log!("{}", string);

    unsafe { IN_TASK = true; }

    Ok(())
}

/// forks task
/// 
/// sets ebx to the child pid in parent task, 0 in child task
pub fn fork(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    unsafe { IN_TASK = false; }

    // save state of current task
    get_current_task_mut().unwrap().state.save(regs);

    let pid = get_current_task().unwrap().id;

    match fork_task(pid) {
        Ok(new_task) => {
            // identify parent and child tasks
            regs.ebx = new_task.id.try_into().unwrap();
            new_task.state.registers.ebx = 0;
        },
        Err(msg) => log!("could not fork pid {}: {}", pid, msg),
    };

    unsafe { IN_TASK = true; }

    Ok(())
}

/// exits task
pub fn exit(_regs: &mut SyscallRegisters) -> Result<(), Errno> {
    exit_current_task();
}

/// gets id of current task
/// 
/// sets ebx to id
pub fn get_pid(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    unsafe { IN_TASK = false; }

    regs.ebx = get_current_task().expect("no current task").id.try_into().unwrap();

    unsafe { IN_TASK = true; }

    Ok(())
}

/// parses a null-terminated array of pointers into a vec of strings, watching for invalid pointers
fn parse_ptr_array(ptr: *const *const u8) -> Result<Vec<String>, Errno> {
    let mut items: Vec<String> = vec![];
    let mut ptr = ptr as usize;

    // TODO: maybe specify a limit for number of arguments and argument length?

    unsafe {
        loop {
            check_ptr(ptr as *const _)?;
            if *(ptr as *const usize) == 0 {
                break;
            }
            check_ptr(*(ptr as *const *const _))?;
            items.push(CStr::from_ptr(*(ptr as *const *const _)).to_string_lossy().into_owned());
            ptr += size_of::<usize>();
        }
    }

    Ok(items)
}

/// replaces this process's address space with that of a new process at the path provided
pub fn exec(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    check_ptr(regs.ebx as *const _)?;
    let path = unsafe { CStr::from_ptr(regs.ebx as *const _).to_string_lossy() };

    // parse arguments and env
    let args = parse_ptr_array(regs.ecx as *const *const _)?;

    let env = parse_ptr_array(regs.edx as *const *const _)?;

    // leave task mode, anything that breaks now will really fuck things up
    unsafe { IN_TASK = false; }

    debug!("exec()ing {} as pid {}", path, get_current_task().unwrap().id);

    let mut new_task = get_current_task().unwrap().recreate();

    match crate::exec::exec_as(&mut new_task, &path, &args, &env) {
        Ok(_) => {
            let mut task = get_current_task_mut().unwrap();

            // free pages of old task
            task.state.free_pages();
    
            // exec_as only touches the state, so we can just replace the state and be fine
            task.state = new_task.state;
    
            // page table has been replaced, so switch to it
            task.state.pages.switch_to();
    
            // registers have been changed, update them
            *regs = task.state.registers;

            debug!("done exec()ing");
        
            unsafe { IN_TASK = true; }
        
            Ok(())
        },
        Err(err) => {
            new_task.state.free_pages();
            Err(err)
        }
    }
}

/// platform-specific syscall handler
#[no_mangle]
pub unsafe extern "C" fn syscall_handler(mut regs: SyscallRegisters) {
    let syscall_num = regs.eax as usize;

    if syscall_num < NUM_SYSCALLS {
        match SYSCALL_LIST[syscall_num](&mut regs) {
            Ok(()) => (),
            Err(err) => {
                log!("syscall error: {}", err);
                exit_current_task(); // TODO: signals
            }
        }
    } else {
        log!("bad syscall {}", syscall_num);
        exit_current_task();
    }
}
