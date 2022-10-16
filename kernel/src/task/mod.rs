//! smooth brain scheduler

pub mod cpu;
pub mod exec;
pub mod queue;

use crate::{
    arch::{get_thread_id, Registers},
    mm::{paging::PageDirectory, sync::PageDirSync},
    util::array::VecBitSet,
};
use alloc::vec::Vec;
use core::fmt;
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
    pub threads: Vec<Thread>,
}

impl Process {
    pub fn set_page_directory(&mut self, page_dir: crate::arch::PageDirectory<'static>) {
        self.page_directory.task = page_dir;
        self.page_directory.force_sync();
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

#[derive(Debug)]
pub struct CreateProcessError;

pub struct ProcessList {
    pub processes: Vec<Option<Process>>,
    pub process_bitset: VecBitSet,
}

impl ProcessList {
    pub const fn new() -> Self {
        Self {
            processes: Vec::new(),
            process_bitset: VecBitSet::new(),
        }
    }

    pub fn get_process(&self, id: usize) -> Option<&Process> {
        self.processes.get(id)?.as_ref()
    }

    pub fn get_process_mut(&mut self, id: usize) -> Option<&mut Process> {
        self.processes.get_mut(id)?.as_mut()
    }

    pub fn get_thread(&self, id: ProcessID) -> Option<&Thread> {
        self.processes.get(id.process)?.as_ref()?.threads.get(id.thread)
    }

    pub fn get_thread_mut(&mut self, id: ProcessID) -> Option<&mut Thread> {
        self.processes.get_mut(id.process)?.as_mut()?.threads.get_mut(id.thread)
    }

    pub fn create_process(&mut self, page_dir: crate::arch::PageDirectory<'static>) -> Result<usize, CreateProcessError> {
        let id = self.process_bitset.first_unset();

        if id >= self.processes.len() {
            self.processes.try_reserve(id - self.processes.len() + 1).map_err(|_| CreateProcessError)?;

            while self.processes.len() <= id + 1 {
                self.processes.push(None);
            }
        }

        let mut page_directory = PageDirSync {
            kernel: crate::mm::paging::get_page_dir().0,
            task: page_dir,
            process_id: id,
            kernel_space_updates: 0,
        };

        page_directory.force_sync();

        self.processes[id] = Some(Process { page_directory, threads: Vec::new() });
        self.process_bitset.set(id);

        Ok(id)
    }
}

impl Default for ProcessList {
    fn default() -> Self {
        Self::new()
    }
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

static PROCESSES: Mutex<ProcessList> = Mutex::new(ProcessList::new());

pub fn get_process_list() -> spin::MutexGuard<'static, ProcessList> {
    PROCESSES.lock()
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

    let in_kernel = if !manual {
        thread.enter_kernel()
    } else {
        true
    };

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

        // do we have an active task?
        let last_id = if let Some(current) = queue.current() {
            // yes, save task state
            let mut processes = PROCESSES.lock();

            if let Some(thread) = processes.get_thread_mut(current.id()) {
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
                            // TODO: this
                            thread.is_blocked = true;
                            None
                        }
                    }
                }
            } else {
                error!("couldn't get thread {:?} for saving in context switch", current.id());

                None
            }
        } else {
            None
        };

        // do we have another task to load?
        let mut has_task = false;

        while let Some(next) = queue.consume() {
            // yes, save task state
            let mut processes = PROCESSES.lock();

            if let Some(process) = processes.get_process_mut(next.id().process) {
                if let Some(thread) = process.threads.get(next.id().thread) {
                    if thread.is_blocked {
                        continue;
                    }

                    thread.registers.task_sanity_check().expect("thread registers failed sanity check");

                    regs.transfer(&thread.registers);

                    // todo: loading of other registers (x87, MMX, SSE, etc.)

                    trace!("syncing");
                    process.page_directory.sync();

                    if let Some((id, _)) = last_id.as_ref() {
                        // is the process different? (i.e. not the same thread)
                        if id.process != next.id().process {
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
                    error!("couldn't get thread {:?} for saving in context switch", next.id());
                }
            } else {
                error!("couldn't get process {:?} for loading in context switch", next.id().process);
            }
        }

        if !has_task {
            // this'll set the registers into a safe state so the cpu will return from the interrupt handler and just wait for an interrupt there,
            // since for whatever reason just waiting here really messes things up
            crate::arch::safely_halt_cpu(regs);
        }

        // put previous task back into queue if necessary
        if mode == ContextSwitchMode::Normal {
            trace!("re-inserting thread");

            if let Some((id, priority)) = last_id {
                queue.insert(TaskQueueEntry::new(id, priority));
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

    thread.leave_kernel();
}

/// timer callback run every time we want to perform a context switch
pub fn context_switch_timer(timer_num: usize, cpu: Option<cpu::ThreadID>, regs: &mut Registers) {
    _context_switch(timer_num, cpu, regs, false, ContextSwitchMode::Normal);
}

/// manually performs a context switch
pub fn manual_context_switch(timer_num: usize, cpu: Option<cpu::ThreadID>, regs: &mut Registers, mode: ContextSwitchMode) {
    _context_switch(timer_num, cpu, regs, true, mode);
}

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
