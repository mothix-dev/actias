pub mod i586;

#[cfg(target_arch = "i586")]
pub use i586::*;

/// properties describing a CPU architecture
#[derive(Debug)]
pub struct ArchProperties {
    /// the MMU's page size, in bytes
    pub page_size: usize,
    /// the region in memory where userspace code will reside
    pub userspace_region: ContiguousRegion,
    /// the region in memory where the kernel resides
    pub kernel_region: ContiguousRegion,
}

/// a contiguous region in memory
#[derive(Debug)]
pub struct ContiguousRegion {
    start: usize,
    end: usize,
}

impl ContiguousRegion {
    /// creates a new ContinuousRegion from a given start and end address
    ///
    /// # Panics
    ///
    /// this function will panic if the address of `end` is not greater than the address of `start`
    pub const fn new(start: usize, end: usize) -> Self {
        assert!(end > start, "end is not greater than start");

        Self { start, end }
    }

    /// gets the start address of this region
    pub fn start(&self) -> usize {
        self.start
    }

    /// gets the end address of this region
    pub fn end(&self) -> usize {
        self.end
    }
}
