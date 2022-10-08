pub mod acpi;
pub mod gdt;
pub mod ints;
pub mod paging;

use crate::task::{cpu::CPU, set_cpus};
use alloc::{collections::BTreeMap, format, string::ToString};
use core::arch::asm;
use log::{info, warn};
use raw_cpuid::{CpuId, CpuIdResult};
use x86::bits32::eflags::EFlags;

// various useful constants
pub const MEM_TOP: usize = 0xffffffff;

pub const PAGE_SIZE: usize = 0x1000;
pub const INV_PAGE_SIZE: usize = !(PAGE_SIZE - 1);

pub const MAX_STACK_FRAMES: usize = 1024;

/// gets the value of the eflags register in the cpu as an easy to use struct
pub fn get_eflags() -> EFlags {
    unsafe {
        let mut flags: u32;

        asm!(
            "pushfd",
            "pop {}",
            out(reg) flags,
        );

        EFlags::from_bits(flags).unwrap()
    }
}

/// sets the value of the eflags register in the cpu
pub fn set_eflags(flags: EFlags) {
    unsafe {
        let flags: u32 = flags.bits();

        asm!(
            "push {}",
            "popfd",
            in(reg) flags,
        );
    }
}

// ==== standard architecture exports ====

pub use ints::register_irq;

/// registers passed to interrupt handlers
pub type Registers = ints::InterruptRegisters;

/// page directory type
pub type PageDirectory<'a> = paging::PageDir<'a>;

/// exits emulators if applicable then completely halts the CPU
///
/// # Safety
///
/// yeah
pub unsafe fn halt() -> ! {
    warn!("halting CPU");

    // exit qemu
    x86::io::outb(0x501, 0x31);

    // exit bochs
    x86::io::outw(0x8a00, 0x8a00);
    x86::io::outw(0x8a00, 0x8ae0);

    // halt cpu
    loop {
        asm!("cli; hlt");
    }
}

/// halts the CPU until an interrupt occurs
pub fn halt_until_interrupt() {
    unsafe {
        asm!("sti; hlt");
    }
}

pub fn init(args: Option<BTreeMap<&str, &str>>) {
    unsafe {
        ints::init_irqs();
    }

    let mut cpus = CPU::new();

    if args.and_then(|a| a.get("acpi").cloned()).unwrap_or("yes") == "no" {
        warn!("ACPI disabled, ignoring APIC and other CPUs");

        // populate CPU geometry - 1 core, 1 thread
        cpus.add_core(1);
    } else {
        fn cpuid_reader(leaf: u32, subleaf: u32) -> CpuIdResult {
            let eax: u32;
            let ebx: u32;
            let ecx: u32;
            let edx: u32;

            unsafe {
                asm!(
                    "cpuid",

                    inout("eax") leaf => eax,
                    out("ebx") ebx,
                    inout("ecx") subleaf => ecx,
                    out("edx") edx,
                );
            }

            CpuIdResult { eax, ebx, ecx, edx }
        }

        let cpuid = CpuId::with_cpuid_fn(cpuid_reader);

        info!("{:?}", cpuid);

        let model = if let Some(brand) = cpuid.get_processor_brand_string() {
            brand.as_str().to_string()
        } else if let Some(feature_info) = cpuid.get_feature_info() {
            let vendor = cpuid.get_vendor_info().map(|v| v.to_string()).unwrap_or_else(|| "unknown".to_string());
            let family = feature_info.family_id();
            let model = feature_info.model_id();
            let stepping = feature_info.stepping_id();
            format!("{vendor} family {family} model {model} stepping {stepping}")
        } else {
            "unknown".to_string()
        };

        info!("cpu model is {model:?}");

        let has_apic = match cpuid.get_feature_info() {
            Some(feature_info) => feature_info.has_apic(),
            None => false,
        };

        if has_apic {
            // check for multiple logical processors

            let has_htt = cpuid.get_feature_info().unwrap().has_htt();

            info!("has_htt: {has_htt}");
        } else {
            info!("no APIC detected, assuming 1 core 1 thread");

            cpus.add_core(1);
        }
    }

    set_cpus(cpus);
}
