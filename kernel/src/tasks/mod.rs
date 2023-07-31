//! kernel-level cooperative multitasking

mod executor;
mod timer;

pub use executor::*;
pub use timer::*;

// TODO: make this per-cpu and sane
static mut EXECUTOR: Option<Executor> = None;

/// gets the executor for the CPU this is running on
pub fn get_cpu_executor() -> &'static Executor {
    unsafe { EXECUTOR.as_ref().unwrap() }
}

/// initializes the executor
///
/// # Safety
///
/// mutable statics. you'd have to be crazy (and in a rubber room with rubber rats) to use this more than once and/or in a multi CPU context.
/// it will not work
pub unsafe fn init() {
    EXECUTOR = Some(Executor::new());
}
