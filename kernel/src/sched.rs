//! simple multi-level feedback queue scheduler based on the 4.4BSD scheduler as described in https://www.scs.stanford.edu/23wi-cs212/pintos/pintos_7.html
//! because it seems to work and i don't care enough to reinvent the wheel here

use crate::{
    arch::{bsp::RegisterContext, PROPERTIES},
    mm::{PageDirTracker, PageDirectory},
    timer::{Timer, Timeout},
};
use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use core::{
    fmt::Display,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use crossbeam::queue::SegQueue;
use log::trace;
use spin::Mutex;

type Registers = <crate::arch::InterruptManager as crate::arch::bsp::InterruptManager>::Registers;

const WAIT_STACK_SIZE: usize = 0x1000;
const TIME_SLICE: u64 = 6;
const MAX_PRIORITY: usize = 63;

/// formats a fixed point number properly with the given number of decimal places
pub struct FixedPoint<T>(pub T, pub usize);

impl<T: Display + Copy + TryFrom<usize> + core::ops::Shr<T, Output = T> + core::ops::BitAnd<T, Output = T> + core::ops::Mul<T, Output = T> + core::ops::Div<T, Output = T>> core::fmt::Display
    for FixedPoint<T>
where <T as TryFrom<usize>>::Error: core::fmt::Debug
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.1 == 0 {
            write!(f, "{}", self.0 >> 14_usize.try_into().unwrap())
        } else {
            write!(
                f,
                "{}.{:0width$}",
                self.0 >> 14_usize.try_into().unwrap(),
                ((self.0 & ((1_usize << 14) - 1).try_into().unwrap()) * 10_usize.pow(self.1.try_into().unwrap()).try_into().unwrap()) / (1_usize << 14).try_into().unwrap(),
                width = self.1
            )
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ExecMode {
    Running,
    Blocked,
    Exited,
}

/// a schedulable task, which can be a process, a thread, or something else entirely
pub struct Task {
    /// the register context of this task
    pub registers: Registers,

    /// whether this task is running, blocked, etc.
    pub exec_mode: ExecMode,

    /// the niceness value of this task, -20..=20
    pub niceness: i64,

    /// estimate of how much CPU time this task has used recently in 17.14 fixed point
    pub cpu_time: i64,

    /// the memory map associated with this task
    pub memory_map: Arc<Mutex<crate::mm::ProcessMap>>,

    /// the PID associated with this task
    pub pid: Option<usize>,
}

impl Task {
    pub fn calc_cpu_time(&mut self, load_avg: i64) {
        // cpu_time = (load_avg * 2) / (load_avg * 2 + 1) * cpu_time + niceness
        self.cpu_time = ((load_avg * 2 * (1 << 14)) / (load_avg * 2 + (1 << 14)) * self.cpu_time) / (1 << 14) + (self.niceness * (1 << 14));
    }
}

/// scheduler for a single CPU
pub struct Scheduler {
    /// the queues of tasks to run in the future
    run_queues: [SegQueue<Arc<Mutex<Task>>>; MAX_PRIORITY + 1],

    /// the task that's currently running
    current_task: Mutex<Option<Arc<Mutex<Task>>>>,

    /// the timeout used for scheduling
    timeout: Arc<Timeout>,

    /// the stack used when waiting around for a task to be queued
    wait_around_stack: Mutex<Pin<Box<[u8]>>>,

    /// the page directory of the kernel, to be switched to when there aren't any tasks to run
    kernel_page_directory: Arc<Mutex<PageDirTracker<crate::arch::PageDirectory>>>,

    /// how many tasks are ready for execution
    ready_tasks: AtomicUsize,

    /// average of how many tasks have been ready over the past minute
    load_avg: AtomicUsize,

    /// whether or not this scheduler has been dropped
    is_dropped: Arc<AtomicBool>,

    /// whether to force a context switch to happen regardless of whether or not we're in kernel mode
    force_context_switch: AtomicBool,
}

impl Scheduler {
    pub fn new(kernel_page_directory: Arc<Mutex<PageDirTracker<crate::arch::PageDirectory>>>, timer: &Timer) -> Arc<Self> {
        let new = Arc::new(Self {
            run_queues: {
                let mut v = Vec::with_capacity(MAX_PRIORITY + 1);
                for _i in 0..=MAX_PRIORITY {
                    v.push(SegQueue::new());
                }
                v.try_into().unwrap()
            },
            current_task: Mutex::new(None),
            timeout: timer.add_timeout(|_, _| None),
            wait_around_stack: Mutex::new(Box::into_pin(vec![0_u8; WAIT_STACK_SIZE].into_boxed_slice())),
            kernel_page_directory,
            ready_tasks: AtomicUsize::new(0),
            load_avg: AtomicUsize::new(0),
            is_dropped: Arc::new(AtomicBool::new(false)),
            force_context_switch: AtomicBool::new(false),
        });

        // register the timeout
        {
            let arc_self = new.clone();
            *new.timeout.callback.lock() = Box::new(move |registers, jiffies| arc_self.context_switch_timeout(registers, jiffies));
        }

        new
    }

    pub fn force_next_context_switch(&self) {
        self.force_context_switch.store(true, Ordering::SeqCst);
        self.timeout.expires_at.store(0, Ordering::Release);
    }

    /// calculates the load average of the scheduler. should only be called once per second
    pub fn calc_load_avg(&self) -> u64 {
        let cur_load_avg = self.load_avg.load(Ordering::SeqCst) as u64;
        let cur_ready_tasks = self.ready_tasks.load(Ordering::SeqCst) as u64;

        // new_load_avg = (59.0 / 60.0) * cur_load_avg + (1.0 / 60.0) * cur_ready_tasks
        let new_load_avg = ((((59 << 14) / 60) * cur_load_avg) >> 14) + ((1 << 14) / 60) * cur_ready_tasks;

        self.load_avg.store(new_load_avg.try_into().unwrap(), Ordering::SeqCst);
        new_load_avg
    }

    /// pushes a task onto the proper runqueue
    pub fn push_task(&self, task: Arc<Mutex<Task>>) {
        let priority = {
            let task = task.lock();

            // MAX_PRIORITY - (cpu_time / 4) + (niceness * 2)
            // niceness was originally subtracted as originally described, however upon testing it has the exact opposite effect as intended
            let raw_prio = MAX_PRIORITY as i64 - (((task.cpu_time / 4) + (task.niceness * 2 * (1 << 14))) >> 14);

            // clamp priority to 0..=MAX_PRIORITY
            raw_prio.max(0).min(MAX_PRIORITY as i64) as usize
        };

        self.run_queues[priority].push(task);
        self.ready_tasks.fetch_add(1, Ordering::SeqCst);
    }

    /// iterates thru all the runqueues from highest to lowest priority to find an available task
    fn pop_task(&self) -> Option<Arc<Mutex<Task>>> {
        for i in (0..=MAX_PRIORITY).rev() {
            if let Some(task) = self.run_queues[i].pop() {
                self.ready_tasks.fetch_sub(1, Ordering::SeqCst);

                if task.lock().exec_mode != ExecMode::Running {
                    continue;
                }

                return Some(task);
            }
        }

        None
    }

    /// performs a manual context switch
    pub fn context_switch(&self, registers: &mut Registers) {
        // maybe not the best idea since it'll immediately fire if the timeout was disabled earlier, but it's probably fine
        let mut jiffies = self.timeout.expires_at.load(Ordering::Acquire);
        if jiffies == u64::MAX {
            jiffies = 0;
        }

        let new = self.context_switch_timeout(registers, jiffies).unwrap_or(u64::MAX);
        let _ = self.timeout.expires_at.compare_exchange(jiffies, new, Ordering::Release, Ordering::Relaxed);
    }

    /// performs a context switch
    fn context_switch_timeout(&self, registers: &mut Registers, jiffies: u64) -> Option<u64> {
        if self.is_dropped.load(Ordering::SeqCst) {
            return None;
        }

        // skip context switching if the kernel is busy doing something
        if !self.is_running_task(registers) && !self.force_context_switch.load(Ordering::SeqCst) {
            return Some(0);
        }

        self.force_context_switch.store(false, Ordering::SeqCst);

        // used to keep the previous task's page directory from being dropped until it's been switched out
        let mut _page_directory = None;

        // save state of current task and re-queue it if necessary
        {
            let mut current_task = self.current_task.lock();

            if let Some(task) = current_task.take() {
                let exec_mode;

                #[allow(clippy::clone_on_copy)]
                {
                    let mut task = task.lock();

                    task.registers = registers.clone();
                    exec_mode = task.exec_mode;
                    _page_directory = Some(task.memory_map.clone());
                }

                if exec_mode == ExecMode::Running {
                    self.push_task(task);
                }
            }
        }

        // load state of new task from the queue, or just wait around if there are no tasks
        if let Some(task) = self.pop_task() {
            #[allow(clippy::clone_on_copy)]
            {
                let mut task = task.lock();

                *registers = task.registers.clone();
                task.cpu_time += TIME_SLICE as i64 * (1 << 14);

                unsafe {
                    let mut map = task.memory_map.lock();
                    map.page_directory.check_synchronize();
                    map.page_directory.switch_to();
                }
            }

            *self.current_task.lock() = Some(task);

            Some(jiffies + TIME_SLICE)
        } else {
            // technically not safe or correct because the lock isn't held while waiting, but also i don't care
            let stack = {
                let mut stack = self.wait_around_stack.lock();
                let i = stack.len() - 1;
                &mut stack[i] as *mut _
            };
            *registers = Registers::from_fn(wait_around as *const _, stack, false);

            unsafe {
                self.kernel_page_directory.lock().switch_to();
            }

            trace!("no more tasks, waiting...");

            None
        }
    }

    /// synchronizes the page directory of the running task with that of the kernel
    pub fn sync_page_directory(&self) {
        let current_task = self.current_task.lock();

        if let Some(task) = &*current_task {
            task.lock().memory_map.lock().page_directory.check_synchronize();
        }
    }

    /// gets the currently running task on this scheduler
    pub fn get_current_task(&self) -> Option<Arc<Mutex<Task>>> {
        self.current_task.lock().clone()
    }

    /// figures out whether or not a task is currently running based on registers
    pub fn is_running_task(&self, registers: &Registers) -> bool {
        !PROPERTIES.kernel_region.contains(registers.instruction_pointer() as usize)
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.is_dropped.store(true, Ordering::SeqCst);
    }
}

pub extern "C" fn wait_around() -> ! {
    loop {
        (crate::arch::PROPERTIES.wait_for_interrupt)();
    }
}
