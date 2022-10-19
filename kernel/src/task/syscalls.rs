use super::switch::{exit_current_process, exit_current_thread};
use common::types::Syscalls;
use log::error;

pub fn syscall_handler(regs: &mut crate::arch::Registers, num: u32, _arg0: u32, _arg1: u32, _arg2: u32) {
    let thread_id = crate::arch::get_thread_id();
    let cpus = crate::task::get_cpus().expect("CPUs not initialized");
    let thread = cpus.get_thread(thread_id).expect("couldn't get CPU thread");

    thread.check_enter_kernel();

    match regs.task_sanity_check() {
        Ok(_) => (),
        Err(err) => {
            let pid = thread.task_queue.lock().current().unwrap().id();
            error!("process {pid} failed sanity check: {err:?}");
            exit_current_thread(thread_id, thread, regs);
        }
    }

    match Syscalls::try_from(num) {
        Ok(Syscalls::IsComputerOn) => regs.syscall_return(Ok(1)),
        Ok(Syscalls::ExitProcess) => exit_current_process(thread_id, thread, regs),
        Ok(Syscalls::ExitThread) => exit_current_thread(thread_id, thread, regs),
        Ok(Syscalls::Fork) => {
            // whatever we put here will end up in the newly forked process, since we're gonna be overwriting these values in the original process
            //
            // the fork syscall should return 0 in the child process and the PID of the child process in the parent
            regs.syscall_return(Ok(0));

            let res = super::fork_current_process(thread, regs);
            regs.syscall_return(res);
        }
        Err(err) => {
            // invalid syscall, yoink the thread
            let pid = thread.task_queue.lock().current().unwrap().id();
            error!("invalid syscall {num} in process {pid} ({err})");
            exit_current_thread(thread_id, thread, regs);
        }
    }

    thread.leave_kernel();
}
