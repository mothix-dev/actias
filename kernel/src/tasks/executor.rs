use crate::arch::bsp::RegisterContext;
use alloc::sync::Arc;
use core::{task::{Context, Poll, Waker}, sync::atomic::{AtomicBool, Ordering}};
use crossbeam::queue::SegQueue;
use futures::{
    future::BoxFuture,
    task::{waker_ref, ArcWake},
    Future, FutureExt,
};
use spin::Mutex;

type Registers = <crate::arch::InterruptManager as crate::arch::bsp::InterruptManager>::Registers;

#[derive(Default)]
pub struct RegisterHandling {
    saved: Option<Registers>,
    waiting: Option<Arc<Mutex<WaitForRegistersState>>>,
}

impl RegisterHandling {
    /// check whether there's something waiting for saved registers and wake it if there is
    #[allow(clippy::clone_on_copy)]
    fn check_waiting(&mut self) {
        if let Some(registers) = self.saved.as_ref() {
            if let Some(state) = self.waiting.take() {
                let mut state = state.lock();
    
                state.registers = Some(registers.clone());
                if let Some(waker) = state.waker.take() {
                    waker.wake();
                }
            }
        }
    }

    fn add_waiting(&mut self, state: Arc<Mutex<WaitForRegistersState>>) {
        if self.waiting.is_some() {
            panic!("can't have more than one future waiting for registers");
        }

        self.waiting = Some(state);
    }
}

pub struct Executor {
    queue: Arc<SegQueue<Arc<Task>>>,
    register_handling: Mutex<RegisterHandling>,
    is_running: AtomicBool,
}

impl Executor {
    /// creates a new Executor
    pub fn new() -> Self {
        Self {
            queue: Arc::new(SegQueue::new()),
            register_handling: Mutex::new(Default::default()),
            is_running: AtomicBool::new(false),
        }
    }

    /// spawns a new task in this Executor
    pub fn spawn(&self, future: impl Future<Output = ()> + 'static + Send) {
        let future = future.boxed();

        let task = Arc::new(Task {
            future: Mutex::new(Some(future)),
            queue: self.queue.clone(),
        });

        self.queue.push(task);
    }

    /// runs all the queued tasks in this Executor, then either halts and waits for interrupts or returns to the previous task
    pub fn run(&self) -> ! {
        self.is_running.store(true, Ordering::SeqCst);

        // run all the currently queued tasks
        while let Some(task) = self.queue.pop() {
            let mut future_slot = task.future.lock();

            if let Some(mut future) = future_slot.take() {
                let waker = waker_ref(&task);
                let context = &mut Context::from_waker(&waker);

                if future.as_mut().poll(context).is_pending() {
                    *future_slot = Some(future);
                }
            }
        }

        self.is_running.store(false, Ordering::SeqCst);
        if let Some(registers) = self.register_handling.lock().saved.take() {
            registers.context_switch_to();
        } else {
            loop {
                (crate::arch::PROPERTIES.wait_for_interrupt)();
            }
        }
    }

    /// whether or not this Executor should be ran
    pub fn should_run(&self) -> bool {
        !self.is_running.load(Ordering::SeqCst)
    }

    /// saves registers before running this Executor
    #[allow(clippy::clone_on_copy)]
    pub fn save_registers(&self, registers: &Registers) {
        let mut handling = self.register_handling.lock();
        handling.saved = Some(registers.clone());
        handling.check_waiting();
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

struct Task {
    future: Mutex<Option<BoxFuture<'static, ()>>>,
    queue: Arc<SegQueue<Arc<Task>>>,
}

impl ArcWake for Task {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.queue.push(arc_self.clone());
    }
}

#[derive(Default)]
struct WaitForRegistersState {
    registers: Option<Registers>,
    waker: Option<Waker>,
}

pub struct WaitForRegisters {
    state: Arc<Mutex<WaitForRegistersState>>,
}

impl WaitForRegisters {
    pub fn new(executor: &Executor) -> Self {
        let state = Arc::new(Mutex::new(Default::default()));
        executor.register_handling.lock().add_waiting(state.clone());
        Self { state }
    }
}

impl Future for WaitForRegisters {
    type Output = Registers;

    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.lock();

        if let Some(registers) = state.registers.take() {
            Poll::Ready(registers)
        } else {
            state.waker = Some(cx.waker().clone());

            Poll::Pending
        }
    }
}

pub fn run_executor(registers: &mut Registers) {
    let executor = super::get_cpu_executor();

    if !executor.should_run() {
        return;
    }

    executor.save_registers(registers);
    // TODO: sane stack pointer acquisition
    *registers = Registers::from_fn(run_executor_internal as *const _, crate::platform::get_stack_ptr());
}

extern "C" fn run_executor_internal() -> ! {
    super::get_cpu_executor().run();
}
