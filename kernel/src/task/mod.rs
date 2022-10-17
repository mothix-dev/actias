//! smooth brain scheduler

pub mod cpu;
pub mod exec;
pub mod queue;

use crate::{
    arch::{get_thread_id, Registers},
    mm::{paging::PageDirectory, sync::PageDirSync},
    util::array::ConsistentIndexArray,
};
use core::{fmt, sync::atomic::{AtomicBool, Ordering}};
use log::{debug, error, trace, warn};
use spin::Mutex;

use queue::PageUpdateEntry;
use queue::TaskQueueEntry;

/// how much time each process gets before it's forcefully preempted
pub const CPU_TIME_SLICE: u64 = 200; // 5 ms quantum

pub struct Process {
    /// the page directory of this process
    pub page_directory: PageDirSync<'static, crate::arch::PageDirectory<'static>>,

    /// all the threads of this process
    pub threads: ConsistentIndexArray<Thread>,
}

impl Process {
    pub fn set_page_directory(&mut self, page_dir: crate::arch::PageDirectory<'static>) {
        crate::mm::paging::free_page_dir(&mut self.page_directory.task);
        self.page_directory.task = page_dir;
        self.page_directory.force_sync();
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        crate::mm::paging::free_page_dir(&mut self.page_directory.task);
    }
}

pub struct Thread {
    /// this thread's registers
    pub registers: Registers,

    /// this thread's priority
    pub priority: i8,

    /// the CPU this thread was last on
    pub cpu: Option<cpu::ThreadID>,

    /// whether this thread is blocked or not
    pub is_blocked: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ProcessID {
    pub process: usize,
    pub thread: usize,
}

impl fmt::Display for ProcessID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.process, self.thread)
    }
}

// this is all very jank but it seems to work? wonder whether the overhead of locking individual processes is at all worth it
static PROCESSES_LOCK: AtomicBool = AtomicBool::new(false);
static mut PROCESSES: ConsistentIndexArray<Mutex<Process>> = ConsistentIndexArray::new();

fn take_processes_lock() {
    trace!("taking process list lock");
    if PROCESSES_LOCK.swap(true, Ordering::Acquire) {
        debug!("processes list is locked, spinning");
        while PROCESSES_LOCK.swap(true, Ordering::Acquire) {}
    }
}

fn release_processes_lock() {
    trace!("releasing process list lock");
    PROCESSES_LOCK.store(false, Ordering::Release);
}

pub fn get_process(id: usize) -> Option<spin::MutexGuard<'static, Process>> {
    take_processes_lock();

    let res = unsafe { PROCESSES.get(id).map(|p| p.lock()) };

    release_processes_lock();

    res
}

#[derive(Debug)]
pub struct CreateProcessError;

pub fn create_process(page_dir: crate::arch::PageDirectory<'static>) -> Result<usize, CreateProcessError> {
    take_processes_lock();

    let index = match unsafe { PROCESSES.add(Mutex::new(Process {
        page_directory: PageDirSync {
            kernel: crate::mm::paging::get_page_dir().0,
            task: page_dir,
            process_id: 0,
            kernel_space_updates: 0,
        },
        threads: ConsistentIndexArray::new(),
    })) } {
        Ok(index) => index,
        Err(_) => {
            release_processes_lock();
            return Err(CreateProcessError);
        }
    };

    let mut process = unsafe { PROCESSES.get(index).unwrap().lock() };

    process.page_directory.process_id = index;
    process.page_directory.force_sync();

    drop(process);

    release_processes_lock();

    Ok(index)
}

pub fn remove_process(id: usize) {
    take_processes_lock();

    unsafe {
        PROCESSES.remove(id);
    }

    release_processes_lock();
}

pub fn remove_thread(id: ProcessID) {
    take_processes_lock();

    if let Some(mut process) = unsafe { PROCESSES.get(id.process).map(|p| p.lock()) } {
        process.threads.remove(id.thread);
    }

    release_processes_lock();
}

pub fn num_processes() -> usize {
    take_processes_lock();

    let res = unsafe { PROCESSES.num_entries() };

    release_processes_lock();

    res
}

static mut CPUS: Option<cpu::CPU> = None;

pub fn get_cpus() -> Option<&'static cpu::CPU> {
    unsafe { CPUS.as_ref() }
}

pub fn set_cpus(cpus: cpu::CPU) {
    unsafe {
        if CPUS.is_some() {
            panic!("can't set CPUs twice");
        }

        debug!("setting CPUs: {:#?}", cpus);

        CPUS = Some(cpus);
    }
}

pub fn process_page_updates() {
    let thread_id = crate::arch::get_thread_id();

    get_cpus().expect("CPUs not initialized").get_thread(thread_id).unwrap().process_page_updates();
}

pub fn update_page(entry: PageUpdateEntry) {
    trace!("updating page {entry:?}");

    let thread_id = crate::arch::get_thread_id();

    if let Some(cpus) = get_cpus() {
        match entry {
            // send to all cpus, wait for all cpus
            PageUpdateEntry::Kernel { addr: _ } => {
                for (core_num, core) in cpus.cores.iter().enumerate() {
                    for (thread_num, thread) in core.threads.iter().enumerate() {
                        if (thread_id.core != core_num || thread_id.thread != thread_num) && thread.has_started() {
                            thread.page_update_queue.lock().insert(entry);

                            let id = cpu::ThreadID { core: core_num, thread: thread_num };

                            assert!(crate::arch::send_interrupt_to_cpu(id, crate::arch::PAGE_REFRESH_INT), "failed to send interrupt");

                            // inefficient and slow :(
                            trace!("waiting for {id}");
                            while !thread.page_update_queue.lock().is_empty() {
                                crate::arch::spin();
                            }
                        }
                    }
                }
            }

            // only send to and wait for cpus with the same process
            PageUpdateEntry::Task { process_id, addr: _ } => {
                for (core_num, core) in cpus.cores.iter().enumerate() {
                    for (thread_num, thread) in core.threads.iter().enumerate() {
                        if (thread_id.core != core_num || thread_id.thread != thread_num) && thread.has_started()
                            && let Some(current_id) = thread.task_queue.lock().current().map(|c| c.id()) && current_id.process == process_id {
                            thread.page_update_queue.lock().insert(entry);

                            let id = cpu::ThreadID { core: core_num, thread: thread_num };

                            assert!(crate::arch::send_interrupt_to_cpu(id, crate::arch::PAGE_REFRESH_INT), "failed to send interrupt");

                            // inefficient and slow :(
                            trace!("waiting for {id}");
                            while !thread.page_update_queue.lock().is_empty() {
                                crate::arch::spin();
                            }
                        }
                    }
                }
            }
        }
    }
}

/// sends a non-maskable interrupt to all known CPUs
pub fn nmi_all_other_cpus() {
    warn!("sending NMI to all other CPUs");

    let local_id = crate::arch::get_thread_id();

    if let Some(cpus) = unsafe { CPUS.as_ref() } {
        for (core_id, core) in cpus.cores.iter().enumerate() {
            for thread_id in 0..core.threads.len() {
                let remote_id = cpu::ThreadID { core: core_id, thread: thread_id };

                if remote_id != local_id {
                    crate::arch::send_nmi_to_cpu(remote_id);
                }
            }
        }
    }
}

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
fn _context_switch(timer_num: usize, cpu: Option<cpu::ThreadID>, regs: &mut Registers, manual: bool, mode: ContextSwitchMode) {
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

        let mut remove_id = None;

        // do we have an active task?
        let last_id = if let Some(current) = queue.current() {
            // yes, save task state

            let id = current.id();

            if let Some(mut process) = get_process(id.process) {
                if let Some(thread) = process.threads.get_mut(id.thread) {
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
                if let Some(thread) = process.threads.get_mut(id.thread) {
                    if thread.is_blocked {
                        continue;
                    }

                    thread.registers.task_sanity_check().expect("thread registers failed sanity check");

                    regs.transfer(&thread.registers);

                    // todo: loading of other registers (x87, MMX, SSE, etc.)

                    trace!("syncing");
                    process.page_directory.sync();

                    if let Some((last_process_id, _)) = last_id.as_ref() {
                        // is the process different? (i.e. not the same thread)
                        if last_process_id.process != id.process {
                            // yes, switch the page directory
                            trace!("switching page directory");
                            unsafe {
                                process.page_directory.switch_to();
                            }
                        }
                    } else {
                        // switch the page directory no matter what since we weren't in a task before
                        trace!("switching page directory");
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
                trace!("re-inserting thread");

                if let Some((id, priority)) = last_id {
                    queue.insert(TaskQueueEntry::new(id, priority));
                }
            }
            ContextSwitchMode::Block => (),
            ContextSwitchMode::Remove => {
                trace!("removing thread");

                if let Some(id) = remove_id {
                    remove_thread(id);
                } else {
                    trace!("no thread to remove");
                }
            }
        }
    } else {
        trace!("skipped context switch");
    }

    // requeue timer
    trace!("re-queueing timer");
    let timer = crate::timer::get_timer(timer_num).expect("unable to get timer for next context switch");
    let expires = timer
        .add_timer_in(timer.hz() / CPU_TIME_SLICE, context_switch_timer)
        .expect("unable to add timer callback for next context switch");
    queue.timer = Some(expires);

    // release lock on queue
    drop(queue);

    trace!("finished context switch");

    if !manual {
        thread.leave_kernel();
    }
}

/// timer callback run every time we want to perform a context switch
pub fn context_switch_timer(timer_num: usize, cpu: Option<cpu::ThreadID>, regs: &mut Registers) {
    _context_switch(timer_num, cpu, regs, false, ContextSwitchMode::Normal);
}

/// manually performs a context switch
pub fn manual_context_switch(timer_num: usize, cpu: Option<cpu::ThreadID>, regs: &mut Registers, mode: ContextSwitchMode) {
    _context_switch(timer_num, cpu, regs, true, mode);
}

/// starts the context switch timer and blocks the thread waiting for the next context switch
pub fn wait_for_context_switch(timer_num: usize, cpu: cpu::ThreadID) {
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
pub fn cancel_context_switch_timer(cpu: Option<cpu::ThreadID>) {
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
pub fn exit_current_process(thread_id: cpu::ThreadID, thread: &cpu::CPUThread, regs: &mut crate::arch::Registers) {
    let cpus = get_cpus().expect("CPUs not initialized");

    // make sure we're not on the process' page directory
    unsafe {
        crate::mm::paging::get_page_dir().switch_to();
    }

    let id = thread.task_queue.lock().current().unwrap().id();
    let num_threads = get_process(id.process).unwrap().threads.num_entries();

    // perform context switch so we're not on this thread anymore
    manual_context_switch(thread.timer, Some(thread_id), regs, ContextSwitchMode::Remove);

    // remove any more threads of the process
    thread.task_queue.lock().remove_process(id.process);

    if num_threads > 1 && cpus.cores.len() > 1 {
        // tell all other CPUs to kill this process
        for (core_num, core) in cpus.cores.iter().enumerate() {
            for (thread_num, thread) in core.threads.iter().enumerate() {
                if (thread_id.core != core_num || thread_id.thread != thread_num) && thread.has_started() {
                    thread.kill_queue.lock().push_back(cpu::KillQueueEntry::Process(id.process));

                    let id = cpu::ThreadID { core: core_num, thread: thread_num };

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
pub fn exit_current_thread(thread_id: cpu::ThreadID, thread: &cpu::CPUThread, regs: &mut crate::arch::Registers) {
    let id = thread.task_queue.lock().current().unwrap().id();
    let num_threads = get_process(id.process).unwrap().threads.num_entries();

    if num_threads > 1 {
        crate::task::manual_context_switch(thread.timer, Some(thread_id), regs, crate::task::ContextSwitchMode::Remove);
    } else {
        debug!("exiting current process");
        exit_current_process(thread_id, thread, regs);
    }
}
