use crate::{sched::Scheduler, timer::Timer};
use alloc::sync::Arc;
use log::debug;

pub struct CPU {
    pub timer: Arc<Timer>,
    pub stack_manager: crate::arch::StackManager,
    pub scheduler: Arc<Scheduler>,
}

impl CPU {
    pub fn start_context_switching(&self) -> ! {
        debug!("starting context switching");
        let scheduler = self.scheduler.clone();
        self.scheduler.force_next_context_switch();
        self.timer.timeout_at(0, move |registers| scheduler.context_switch(registers, scheduler.clone(), true));

        crate::sched::wait_around();
    }
}
