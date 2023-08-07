#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(allocator_api)]
#![feature(asm_const)]
#![feature(generators)]
#![feature(iter_from_generator)]
#![feature(let_chains)]
#![feature(naked_functions)]
#![feature(new_uninit)]
#![feature(panic_info_message)]
#![feature(pointer_byte_offsets)]
#![feature(trait_alias)]

extern crate alloc;

pub mod arch;
pub mod array;
pub mod cpu;
pub mod mm;
pub mod platform;
pub mod process;
pub mod sched;
pub mod syscalls;
pub mod tar;
pub mod timer;
pub mod vfs;

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::{fmt, fmt::LowerHex};
use log::{error, info};
use spin::{Mutex, RwLock};

pub struct FormatHex<T: LowerHex>(pub T);

impl<T: LowerHex> fmt::Debug for FormatHex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#x}", self.0)
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

    (crate::arch::PROPERTIES.halt)();
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

/// the global state that is stored by all CPUs
pub struct GlobalState {
    pub cpus: RwLock<Vec<cpu::CPU>>,
    pub page_directory: Arc<Mutex<mm::PageDirTracker<arch::PageDirectory>>>,
    pub page_manager: Arc<Mutex<mm::PageManager>>,
    pub process_table: RwLock<process::ProcessTable>,
    pub cmdline: RwLock<CommandLine>,
}

pub struct CommandLine {
    pub parsed: BTreeMap<String, String>,
    pub unparsed: String,
}

impl CommandLine {
    pub fn parse(unparsed: String) -> Self {
        let mut parsed = BTreeMap::new();

        for arg in unparsed.split(' ') {
            if !arg.is_empty() {
                let arg = arg.split('=').collect::<Vec<_>>();
                parsed.insert(arg[0].to_string(), arg.get(1).unwrap_or(&"").to_string());
            }
        }

        Self { parsed, unparsed }
    }
}

static mut GLOBAL_STATE: Option<GlobalState> = None;

/// gets the global shared state
pub fn get_global_state() -> &'static GlobalState {
    unsafe { GLOBAL_STATE.as_ref().unwrap() }
}

/// initializes the global shared state. must be ran only once, before interrupts are enabled and other CPUs are brought up
///
/// # Safety
///
/// this is unsafe because it changes the state of a global static containing a non thread safe value (the `Option`, not the `GlobalState`)
pub unsafe fn init_global_state(state: GlobalState) {
    if GLOBAL_STATE.is_some() {
        panic!("can't init global state more than once");
    }

    GLOBAL_STATE = Some(state);
}
