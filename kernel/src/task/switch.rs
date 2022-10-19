use super::{
    cpu::{CPUThread, ThreadID},
    get_cpus, get_process,
    queue::TaskQueueEntry,
    remove_process, remove_thread,
};
use crate::{
    arch::{get_thread_id, Registers},
    mm::paging::PageDirectory,
};
use log::{debug, error, trace};

/// how much time each process gets before it's forcefully preempted
pub const CPU_TIME_SLICE: u64 = 200; // 5 ms quantum

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ContextSwitchMode {
    /// normal context switch, places the current task back onto the queue
    Normal,

    /// blocks the current task and doesn't place it back on the queue
    Block,

    /// removes the current task and obviously doesn't place it back on the queue
    Remove,
}

/// performs a context switch
fn _context_switch(timer_num: usize, cpu: Option<ThreadID>, regs: &mut Registers, manual: bool, mode: ContextSwitchMode) {
    let cpu = cpu.unwrap_or_else(get_thread_id);

    let thread = get_cpus().expect("CPUs not initialized").get_thread(cpu).expect("couldn't get CPU thread object");

    let in_kernel = if !manual { thread.enter_kernel() } else { true };

    // get the task queue for this CPU
    let mut queue = thread.task_queue.lock();

    if in_kernel {
        if manual {
            // remove the pending timer if there is one
            if let Some(expires) = queue.timer {
                let timer_state = crate::timer::get_timer(timer_num).expect("unable to get timer for next context switch");
                timer_state.remove_timer(expires);

                queue.timer = None;
            }
        }

        if queue.len() > 0 || mode != ContextSwitchMode::Normal {
            let mut remove_id = None;

            // do we have an active task?
            let last_id = if let Some(current) = queue.current() {
                // yes, save task state

                let id = current.id();

                if let Some(mut process) = get_process(id.process) {
                    if let Some(thread) = process.threads.get_mut(id.thread as usize) {
                        regs.task_sanity_check().expect("registers failed sanity check");

                        thread.registers.transfer(regs);

                        // todo: saving of other registers (x87, MMX, SSE, etc.)

                        if thread.is_blocked {
                            None
                        } else {
                            match mode {
                                ContextSwitchMode::Normal => Some((current.id(), thread.priority)),
                                ContextSwitchMode::Block => {
                                    thread.is_blocked = true;
                                    None
                                }
                                ContextSwitchMode::Remove => {
                                    remove_id = Some(current.id());
                                    None
                                }
                            }
                        }
                    } else {
                        error!("couldn't get process {:?} for saving in context switch", current.id());

                        None
                    }
                } else {
                    error!("couldn't get process {:?} for saving in context switch", current.id());

                    None
                }
            } else {
                None
            };

            // do we have another task to load?
            let mut has_task = false;

            while let Some(next) = queue.consume() {
                // yes, save task state

                let id = next.id();

                if let Some(mut process) = get_process(id.process) {
                    if let Some(thread) = process.threads.get_mut(id.thread as usize) {
                        if thread.is_blocked {
                            continue;
                        }

                        thread.registers.task_sanity_check().expect("thread registers failed sanity check");

                        regs.transfer(&thread.registers);

                        // todo: loading of other registers (x87, MMX, SSE, etc.)

                        process.page_directory.sync();

                        if let Some((last_process_id, _)) = last_id.as_ref() {
                            // is the process different? (i.e. not the same thread)
                            if last_process_id.process != id.process {
                                // yes, switch the page directory
                                unsafe {
                                    process.page_directory.switch_to();
                                }
                            }
                        } else {
                            // switch the page directory no matter what since we weren't in a task before
                            unsafe {
                                process.page_directory.switch_to();
                            }
                        }

                        has_task = true;
                        break;
                    } else {
                        error!("couldn't get thread {id:?} for loading in context switch");
                    }
                } else {
                    error!("couldn't get process {:?} for loading in context switch", id.process);
                }
            }

            if !has_task {
                // this'll set the registers into a safe state so the cpu will return from the interrupt handler and just wait for an interrupt there,
                // since for whatever reason just waiting here really messes things up
                crate::arch::safely_halt_cpu(regs);
            }

            // put previous task back into queue if necessary
            match mode {
                ContextSwitchMode::Normal => {
                    if let Some((id, priority)) = last_id {
                        queue.insert(TaskQueueEntry::new(id, priority));
                    }
                }
                ContextSwitchMode::Block => (),
                ContextSwitchMode::Remove => {
                    if let Some(id) = remove_id {
                        remove_thread(id);
                    } else {
                        trace!("no thread to remove");
                    }
                }
            }
        }
    }

    // requeue timer
    let timer = crate::timer::get_timer(timer_num).expect("unable to get timer for next context switch");
    let expires = timer
        .add_timer_in(timer.hz() / CPU_TIME_SLICE, context_switch_timer)
        .expect("unable to add timer callback for next context switch");
    queue.timer = Some(expires);

    // release lock on queue
    drop(queue);

    if !manual {
        thread.leave_kernel();
    }
}

/// timer callback run every time we want to perform a context switch
pub fn context_switch_timer(timer_num: usize, cpu: Option<ThreadID>, regs: &mut Registers) {
    _context_switch(timer_num, cpu, regs, false, ContextSwitchMode::Normal);
}

/// manually performs a context switch
pub fn manual_context_switch(timer_num: usize, cpu: Option<ThreadID>, regs: &mut Registers, mode: ContextSwitchMode) {
    _context_switch(timer_num, cpu, regs, true, mode);
}

/// starts the context switch timer and blocks the thread waiting for the next context switch
pub fn wait_for_context_switch(timer_num: usize, cpu: ThreadID) {
    let thread = get_cpus().expect("CPUs not initialized").get_thread(cpu).expect("couldn't get CPU thread object");

    // get the task queue for this CPU
    let mut queue = thread.task_queue.lock();

    // queue timer
    let timer = crate::timer::get_timer(timer_num).expect("unable to get timer for next context switch");
    let expires = timer
        .add_timer_in(timer.hz() / CPU_TIME_SLICE, context_switch_timer)
        .expect("unable to add timer callback for next context switch");
    queue.timer = Some(expires);

    // release lock on queue
    drop(queue);

    thread.leave_kernel();

    loop {
        crate::arch::halt_until_interrupt();
    }
}

/// cancels the next context switch timer
pub fn cancel_context_switch_timer(cpu: Option<ThreadID>) {
    let cpu = cpu.unwrap_or_else(get_thread_id);

    let thread = get_cpus().expect("CPUs not initialized").get_thread(cpu).expect("couldn't get CPU thread object");

    // get the task queue for this CPU
    let mut queue = thread.task_queue.lock();

    // remove the pending timer if there is one
    if let Some(expires) = queue.timer {
        let timer_state = crate::timer::get_timer(thread.timer).expect("unable to get timer for next context switch");
        timer_state.remove_timer(expires);

        queue.timer = None;
    }
}

/// exits the current process, cleans up memory, and performs a context switch to the next process if applicable
pub fn exit_current_process(thread_id: ThreadID, thread: &super::cpu::CPUThread, regs: &mut crate::arch::Registers) {
    let cpus = get_cpus().expect("CPUs not initialized");

    // make sure we're not on the process' page directory
    unsafe {
        crate::mm::paging::get_kernel_page_dir().switch_to();
    }

    let id = thread.task_queue.lock().current().unwrap().id();
    let num_threads = get_process(id.process).unwrap().threads.num_entries();

    debug!("exiting process {}", id.process);

    // perform context switch so we're not on this thread anymore
    manual_context_switch(thread.timer, Some(thread_id), regs, ContextSwitchMode::Remove);

    // remove any more threads of the process
    thread.task_queue.lock().remove_process(id.process);

    if num_threads > 1 && cpus.cores.len() > 1 {
        // tell all other CPUs to kill this process
        for (core_num, core) in cpus.cores.iter().enumerate() {
            for (thread_num, thread) in core.threads.iter().enumerate() {
                if (thread_id.core != core_num || thread_id.thread != thread_num) && thread.has_started() {
                    thread.kill_queue.lock().push_back(super::cpu::KillQueueEntry::Process(id.process));

                    let id = ThreadID { core: core_num, thread: thread_num };

                    assert!(crate::arch::send_interrupt_to_cpu(id, crate::arch::KILL_PROCESS_INT), "failed to send interrupt");

                    // inefficient and slow :(
                    trace!("waiting for {id}");
                    while !thread.kill_queue.lock().is_empty() {
                        crate::arch::spin();
                    }
                }
            }
        }
    }

    remove_process(id.process);
}

/// exits current thread, calls exit_current_process if it's the last remaining thread
pub fn exit_current_thread(thread_id: ThreadID, thread: &CPUThread, regs: &mut crate::arch::Registers) {
    debug!("exiting current thread");
    let id = thread.task_queue.lock().current().unwrap().id();
    let num_threads = get_process(id.process).unwrap().threads.num_entries();

    if num_threads > 1 {
        manual_context_switch(thread.timer, Some(thread_id), regs, ContextSwitchMode::Remove);
    } else {
        exit_current_process(thread_id, thread, regs);
    }
}
