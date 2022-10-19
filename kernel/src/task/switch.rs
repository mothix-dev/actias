use super::{cpu::ThreadID, get_cpus, get_process, queue::TaskQueueEntry, remove_thread};
use crate::{
    arch::{get_thread_id, Registers},
    mm::paging::PageDirectory,
};
use log::{error, trace};

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
            let mut can_load_task = true;

            // save state of task if we're in one
            let mut find_last_id = || {
                let current = queue.current()?;

                // make sure we'll be able to reinsert the process back into the queue if we need to
                if mode == ContextSwitchMode::Normal && queue.try_reserve(1).is_err() {
                    can_load_task = false;
                    return None;
                }

                let id = current.id();

                let mut process = get_process(id.process)?;
                let thread = process.threads.get_mut(id.thread as usize)?;
                regs.task_sanity_check().expect("registers failed sanity check");

                thread.registers.transfer(regs);

                // todo: saving of other registers (x87, MMX, SSE, etc.)

                if !thread.is_blocked {
                    match mode {
                        ContextSwitchMode::Normal => return Some((current.id(), thread.priority)),
                        ContextSwitchMode::Block => thread.is_blocked = true,
                        ContextSwitchMode::Remove => remove_id = Some(current.id()),
                    }
                }

                None
            };
            let last_id = find_last_id();

            if can_load_task {
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
                            queue.insert(TaskQueueEntry::new(id, priority)).unwrap();
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
            } else {
                error!("something bad happened, skipping context switch");
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
