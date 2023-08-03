use crate::mm::ContiguousRegion;

/// properties describing a CPU architecture
#[derive(Debug)]
pub struct ArchProperties {
    /// the MMU's page size, in bytes
    pub page_size: usize,

    /// the region in memory where userspace code will reside
    pub userspace_region: ContiguousRegion<usize>,

    /// the region in memory where the kernel resides
    pub kernel_region: ContiguousRegion<usize>,

    /// the region in memory where the heap resides
    pub heap_region: ContiguousRegion<usize>,

    /// the initial size of the heap when it's first initialized
    pub heap_init_size: usize,

    /// function that'll halt the CPU until an interrupt occurs
    pub wait_for_interrupt: fn(),

    /// function that'll halt execution of the current CPU
    pub halt: fn() -> !,
}

pub trait RegisterContext: Clone {
    /// creates a set of registers which, when switched to, will start running the provided function with the stack set to the provided stack pointer
    fn from_fn(func: *const extern "C" fn(), stack: *mut u8) -> Self;

    /// gets the value of the instruction pointer stored in this context
    fn instruction_pointer(&self) -> *mut u8;

    /// gets the value of the stack pointer stored in this context
    fn stack_pointer(&self) -> *mut u8;
}

pub trait InterruptManager {
    type Registers: RegisterContext;
    type ExceptionInfo: core::fmt::Display;

    /// creates a new InterruptManager
    fn new() -> Self
    where Self: Sized;

    /// registers an interrupt handler, replacing the previous handler registered to this interrupt (if any)
    fn register<F: FnMut(&mut Self::Registers) + 'static>(&mut self, interrupt_num: usize, handler: F);

    /// deregisters an interrupt handler so that the handler will no longer be called any time this interrupt is triggered
    fn deregister(&mut self, interrupt_num: usize);

    /// registers an interrupt handler for all aborting (i.e. unrecoverable) exceptions
    fn register_aborts<F: Fn(&mut Self::Registers, Self::ExceptionInfo) + Clone + 'static>(&mut self, handler: F);

    /// registers an interrupt handler for all faulting (i.e. recoverable) exceptions
    fn register_faults<F: Fn(&mut Self::Registers, Self::ExceptionInfo) + Clone + 'static>(&mut self, handler: F);

    /// loads all the interrupt handlers registered in this InterruptManager into the CPU
    fn load_handlers(&self);
}
