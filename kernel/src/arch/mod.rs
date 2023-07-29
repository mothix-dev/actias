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
