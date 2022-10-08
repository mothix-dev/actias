//! smooth brain scheduler

/*
each logical CPU has its own task queue
if a CPU's task queue is empty, it will search thru other CPU task queues (in order of distance, SMT threads first, then other cores, then other CPUs?) to find a task to run (todo: do we want to steal tasks from other physical processors? will that fuck up cache?)
if no task can be found, the CPU will enter a power-saving state until the next task switch
hopefully this behavior can be done independently, with CPU task queues being locked with a spinlock (or maybe a more advanced mutex could be possible? halt the CPU, then resume it with an interrupt when the lock is released)

context switch would go something like
 - save state of process
 - lock task queue
    - get next task
    - insert process back into queue if it's been forcefully preempted (i.e. didn't voluntarily block)
 - release lock
 - load state of process
*/

pub mod cpu;
pub mod queue;

use crate::{arch::Registers, mm::paging::PageDirectory};
use alloc::vec::Vec;
use log::{debug, error, info};
use spin::Mutex;

use self::queue::TaskQueueEntry;

/// how much time each process gets before it's forcefully preempted
pub const CPU_TIME_SLICE: u64 = 200; // 5 ms quantum

pub struct Process {
    /// the page directory of this process
    pub page_directory: crate::arch::PageDirectory<'static>,

    /// all the threads of this process
    pub threads: Vec<Thread>,
}

pub struct Thread {
    /// this thread's registers
    pub registers: Registers,

    /// this thread's priority
    pub priority: i8,

    /// the CPU this thread was last on
    pub cpu: cpu::ThreadID,

    /// whether this thread is blocked or not
    pub is_blocked: bool,
}

pub struct ProcessList {
    pub processes: Vec<Option<Process>>,
}

impl ProcessList {
    pub const fn new() -> Self {
        Self { processes: Vec::new() }
    }

    pub fn get_process(&self, id: ProcessID) -> Option<&Process> {
        self.processes.get(id.process)?.as_ref()
    }

    pub fn get_process_mut(&mut self, id: ProcessID) -> Option<&mut Process> {
        self.processes.get_mut(id.process)?.as_mut()
    }

    pub fn get_thread(&self, id: ProcessID) -> Option<&Thread> {
        self.processes.get(id.process)?.as_ref()?.threads.get(id.thread)
    }

    pub fn get_thread_mut(&mut self, id: ProcessID) -> Option<&mut Thread> {
        self.processes.get_mut(id.process)?.as_mut()?.threads.get_mut(id.thread)
    }
}

impl Default for ProcessList {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ProcessID {
    pub process: usize,
    pub thread: usize,
}

static PROCESSES: Mutex<ProcessList> = Mutex::new(ProcessList::new());

static mut CPUS: Option<cpu::CPU> = None;

pub fn get_cpus() -> &'static cpu::CPU {
    unsafe { CPUS.as_mut().expect("CPUs not initialized") }
}

pub fn set_cpus(cpus: cpu::CPU) {
    unsafe {
        if CPUS.is_some() {
            panic!("can't set CPUs twice");
        }

        info!("setting CPUs: {:#?}", cpus);

        CPUS = Some(cpus);
    }
}

/// performs a context switch
fn _context_switch(timer_num: usize, cpu: Option<cpu::ThreadID>, regs: &mut Registers, manual: bool, block_thread: bool) {
    let cpu = cpu.unwrap_or(cpu::ThreadID { core: 0, thread: 0 });

    // get the task queue for this CPU
    let mut queue = get_cpus().get_thread(cpu).expect("couldn't get CPU thread object").queue.lock();

    if manual {
        // remove the pending timer if there is one
        if let Some(expires) = queue.timer {
            let timer_state = crate::timer::get_timer(timer_num).expect("unable to get timer for next context switch");
            timer_state.remove_timer(expires);
        }
    }

    // do we have an active task?
    let last_id = if let Some(current) = queue.current() {
        // yes, save task state
        let mut processes = PROCESSES.lock();

        if let Some(thread) = processes.get_thread_mut(current.id()) {
            thread.registers = *regs;

            // todo: saving of other registers (x87, MMX, SSE, etc.)

            if block_thread {
                thread.is_blocked = true;

                None
            } else if thread.is_blocked {
                None
            } else {
                Some((current.id(), thread.priority))
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
        let processes = PROCESSES.lock();

        if let Some(process) = processes.get_process(next.id()) {
            if let Some(thread) = process.threads.get(next.id().thread) {
                if thread.is_blocked {
                    continue;
                }

                *regs = thread.registers;

                // todo: loading of other registers (x87, MMX, SSE, etc.)

                // todo: keeping page directory up to date
                unsafe {
                    process.page_directory.switch_to();
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

    // put previous task back into queue if necessary
    if !block_thread {
        if let Some((id, priority)) = last_id {
            queue.insert(TaskQueueEntry::new(id, priority));
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

    if !has_task {
        crate::arch::halt_until_interrupt();
    }
}

/// timer callback run every time we want to perform a context switch
pub fn context_switch_timer(timer_num: usize, cpu: Option<cpu::ThreadID>, regs: &mut Registers) {
    _context_switch(timer_num, cpu, regs, false, false);
}

/// blocks the current thread and switches to the next thread
pub fn block_thread_and_context_switch(timer_num: usize, cpu: Option<cpu::ThreadID>, regs: &mut Registers) {
    _context_switch(timer_num, cpu, regs, true, true);
}

/// manually switches to the next thread
pub fn context_switch(timer_num: usize, cpu: Option<cpu::ThreadID>, regs: &mut Registers) {
    _context_switch(timer_num, cpu, regs, true, false);
}
