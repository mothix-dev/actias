#![no_std]
#![no_main]
#![feature(panic_info_message)]

pub mod arch;
pub mod platform;
pub mod mm;

use log::{error, info};

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

#[panic_handler]
pub fn panic_implementation(info: &core::panic::PanicInfo) -> ! {
    let (file, line) = match info.location() {
        Some(loc) => (loc.file(), loc.line()),
        None => ("", 0),
    };

    if let Some(m) = info.message() {
        error!("PANIC: \"{m}\" @ {file}:{line}");
    } else if let Some(m) = info.payload().downcast_ref::<&str>() {
        error!("PANIC: \"{m}\" @ {file}:{line}");
    } else {
        error!("PANIC @ {file}:{line}");
    }

    loop {}
}

pub fn init_message() {
    info!(
        "ockernel {} (built at {} with rustc {}, LLVM {} on {})",
        env!("VERGEN_BUILD_SEMVER"),
        env!("VERGEN_BUILD_TIMESTAMP"),
        env!("VERGEN_RUSTC_SEMVER"),
        env!("VERGEN_RUSTC_LLVM_VERSION"),
        env!("VERGEN_RUSTC_HOST_TRIPLE")
    );
}
