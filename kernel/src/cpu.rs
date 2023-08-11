use crate::{sched::Scheduler, timer::Timer};
use alloc::sync::Arc;
use log::debug;
use spin::Mutex;

pub struct CPU {
    pub timer: Arc<Timer>,
    pub stack_manager: crate::arch::StackManager,
    pub interrupt_manager: Arc<Mutex<crate::arch::InterruptManager>>,
    pub scheduler: Arc<Scheduler>,
}

impl CPU {
    pub fn start_context_switching(&self) -> ! {
        debug!("starting context switching");
        self.scheduler.force_next_context_switch();

        (crate::arch::PROPERTIES.enable_interrupts)();
        crate::sched::wait_around();
    }
}
