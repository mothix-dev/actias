use common::types::Syscalls;

pub fn syscall_handler(regs: &mut crate::arch::Registers, num: u32, arg0: u32, arg1: u32, arg2: u32) -> u32 {
    match Syscalls::try_from(num) {
        Ok(Syscalls::IsComputerOn) => 1,
        Err(_) => {
            // invalid syscall :(  

            let thread_id = crate::arch::get_thread_id();
            let thread = crate::task::get_cpus().expect("CPUs not initialized").get_thread(thread_id).expect("couldn't get CPU thread object");
            crate::task::manual_context_switch(thread.timer, Some(thread_id), regs, crate::task::ContextSwitchMode::Remove);

            0
        },
    }
}
