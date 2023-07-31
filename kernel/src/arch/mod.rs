pub mod i586;

#[cfg(target_arch = "i586")]
pub use i586::*;

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
}

pub trait InterruptManager {
    type Registers;
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
}
