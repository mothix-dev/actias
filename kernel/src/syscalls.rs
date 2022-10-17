use common::types::Syscalls;

pub fn syscall_handler(regs: &mut crate::arch::Registers, num: u32, _arg0: u32, _arg1: u32, _arg2: u32) -> Option<u32> {
    let thread_id = crate::arch::get_thread_id();
    let cpus = crate::task::get_cpus().expect("CPUs not initialized");
    let thread = cpus.get_thread(thread_id).expect("couldn't get CPU thread");

    thread.check_enter_kernel();

    let res = match Syscalls::try_from(num) {
        Ok(Syscalls::IsComputerOn) => Some(1),
        Ok(Syscalls::ExitProcess) => {
            crate::task::exit_current_process(thread_id, thread, regs);
            None
        }
        Ok(Syscalls::ExitThread) => {
            crate::task::exit_current_thread(thread_id, thread, regs);
            None
        }
        Err(_) => {
            // invalid syscall, yoink the thread
            crate::task::manual_context_switch(thread.timer, Some(thread_id), regs, crate::task::ContextSwitchMode::Remove);
            None
        }
    };

    thread.leave_kernel();

    res
}
