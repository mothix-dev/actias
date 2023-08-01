use crate::{arch::bsp::RegisterContext, timer::Timer};
use alloc::{boxed::Box, sync::Arc, vec};
use const_soft_float::soft_f32::SoftF32;
use core::pin::Pin;
use crossbeam::queue::SegQueue;
use spin::Mutex;

type Registers = <crate::arch::InterruptManager as crate::arch::bsp::InterruptManager>::Registers;

const WAIT_STACK_SIZE: usize = 0x1000;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ExecMode {
    Running,
    Blocked,
    Exited,
}

/// a schedulable task, which can be a process, a thread, or something else entirely
pub struct Task {
    /// whether this task is valid or not.
    /// used when a task is queued for execution but needs to be removed from the queue before executing
    pub is_valid: bool,

    /// the register context of this task
    pub registers: Registers,

    /// whether this task is running, blocked, etc.
    pub exec_mode: ExecMode,

    /// the niceness value of this task, -20..=20
    pub niceness: i8,

    /// an offset added to the niceness value depending on how much CPU time this task has used recently
    pub niceness_adj: i8,
}

/// scheduler for a single CPU
pub struct Scheduler {
    /// the queue of tasks to run in the future
    pub run_queue: SegQueue<Arc<Mutex<crate::sched::Task>>>,

    /// the task that's currently running
    pub current_task: Mutex<Option<Arc<Mutex<Task>>>>,

    /// the timer used for scheduling
    pub timer: Arc<Timer>,

    /// the stack used when waiting around for a task to be queued
    pub wait_around_stack: Mutex<Pin<Box<[u8]>>>,
}

impl Scheduler {
    pub fn new(timer: Arc<Timer>) -> Self {
        Self {
            run_queue: SegQueue::new(),
            current_task: Mutex::new(None),
            timer,
            wait_around_stack: Mutex::new(Box::into_pin(vec![0_u8; WAIT_STACK_SIZE].into_boxed_slice())),
        }
    }

    /// performs a context switch,
    pub fn context_switch(&self, registers: &mut Registers, arc_self: Arc<Self>) {
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
                }

                if exec_mode == ExecMode::Running {
                    self.run_queue.push(task);
                }
            }
        }

        let mut has_task = false;

        // load state of new task from the queue, or just wait around if there are no tasks
        while let Some(task) = self.run_queue.pop() {
            #[allow(clippy::clone_on_copy)]
            {
                let task = task.lock();

                if !task.is_valid {
                    continue;
                }

                *registers = task.registers.clone();

                // calculate effective niceness value and clamp it
                let niceness = (task.niceness + task.niceness_adj).max(-20).min(20);
                // calculate time slice length
                let time_slice = (TIME_SLICE_LOOKUP[(niceness + 20) as usize] * self.timer.hz()) >> 30;

                self.timer.timeout_in(time_slice, move |registers| arc_self.context_switch(registers, arc_self.clone()));
            }

            *self.current_task.lock() = Some(task.clone());

            has_task = true;
            break;
        }

        if !has_task {
            // technically not safe or correct because the lock isn't held while waiting, but also i don't care
            let stack = {
                let mut stack = self.wait_around_stack.lock();
                let i = stack.len() - 1;
                &mut stack[i] as *mut _
            };
            *registers = Registers::from_fn(wait_around as *const _, stack);
        }
    }
}

/// niceness to time slice lookup table
const TIME_SLICE_LOOKUP: [u64; 41] = {
    let mut out = [0; 41];

    let mut i = 0;
    while i < 41 {
        out[i] = SoftF32(536.0/500.0).powi(20 - i as i32).add(SoftF32(2.0)).mul(SoftF32(1048576.0)).to_f32() as u64;
        i += 1;
    }

    out
};

pub extern "C" fn wait_around() -> ! {
    loop {
        (crate::arch::PROPERTIES.wait_for_interrupt)();
    }
}

// y = (536/500)^-x + 2
