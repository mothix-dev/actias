pub mod acpi;
pub mod gdt;
pub mod ints;
pub mod paging;

use crate::task::{cpu::CPU, set_cpus};
use alloc::{collections::BTreeMap, format, string::ToString};
use core::arch::asm;
use log::{debug, error, info, warn};
use raw_cpuid::{CpuId, CpuIdResult, TopologyType};
use x86::{bits32::eflags::EFlags, cpuid};

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

pub fn read_cpuid() -> CpuId {
    CpuId::with_cpuid_fn(cpuid_reader)
}

/// describes the topology of the CPU
#[derive(Copy, Clone, Debug)]
pub struct CPUTopology {
    /// how many cores are in the CPU
    pub num_cores: usize,

    /// how many threads are there per core
    pub threads_per_core: usize,

    /// how many logical processors are there total
    pub logical_processors: usize,
}

impl CPUTopology {
    pub fn to_cpu(&self) -> CPU {
        let mut cpu = CPU::new();

        for _i in 0..self.num_cores {
            cpu.add_core(self.threads_per_core);
        }

        cpu
    }
}

impl Default for CPUTopology {
    fn default() -> Self {
        Self {
            num_cores: 1,
            threads_per_core: 1,
            logical_processors: 1,
        }
    }
}

/// describes how APIC IDs map to CPU IDs
#[derive(Copy, Clone, Debug, Default)]
pub struct APICToCPU {
    /// how many bits to shift the APIC ID right to get the core ID
    pub core_shift_right: usize,

    /// value to perform a bitwise AND of with the APIC ID to get the thread ID
    pub smt_bitwise_and: usize,
}

/// given a number, finds the next highest power of 2 for it
/// 
/// https://graphics.stanford.edu/~seander/bithacks.html#RoundUpPowerOf2
fn find_nearest_power_of_2(mut num: u32) -> u32 {
    num -= 1;
    num |= num >> 1;
    num |= num >> 2;
    num |= num >> 4;
    num |= num >> 8;
    num |= num >> 16;
    num += 1;

    num
}

/// calculates how many bits are required to store a number (i think)
fn bits_required_for(num: usize) -> usize {
    let mut shift = 0;

    while (num >> shift) != 0 {
        shift += 1;
    }

    shift - 1
}

/// attempts to find the CPU topology based on CPUID calls
fn get_cpu_topology() -> Option<(CPUTopology, APICToCPU)> {
    // TODO: figure out more ways to detect CPU topology with CPUID, this current method is a bit lacking
    
    let cpuid = read_cpuid();

    let has_htt = match cpuid.get_feature_info() {
        Some(feature_info) => feature_info.has_htt(),
        None => false,
    };

    if has_htt {
        if let Some(parameters) = cpuid.get_cache_parameters() {
            debug!("using cache_parameters for CPU topology");

            debug!("got cache parameters: {parameters:?}");

            // this is probably a terrible way to do this lmao
            let mut level2_cores = 0;
            let mut level3_cores = 0;

            for parameter in parameters {
                debug!("{parameter:?}");
                if parameter.cache_type() == cpuid::CacheType::Unified {
                    match parameter.level() {
                        2 => level2_cores = parameter.max_cores_for_cache(),
                        3 => level3_cores = parameter.max_cores_for_cache(),
                        _ => (),
                    }
                }
            }

            if level2_cores == 0 || level3_cores == 0 {
                // don't want to use garbage data as the CPU topology
                debug!("couldn't use cache_parameters for CPU topology");

                return None;
            }

            Some((
                CPUTopology {
                    num_cores: level3_cores / level2_cores,
                    threads_per_core: level2_cores,
                    logical_processors: level3_cores,
                },
                APICToCPU {
                    core_shift_right: bits_required_for(level2_cores),
                    smt_bitwise_and: find_nearest_power_of_2(level2_cores.try_into().unwrap()) as usize - 1,
                },
            ))
        } else if let Some(info) = cpuid.get_extended_topology_info() {
            debug!("using extended_topology_info for CPU topology");

            let mut core_shift = 0;
            let mut smt_count = 0;
            let mut core_count = 0;

            for level in info {
                debug!("{level:?}");
                match level.level_type() {
                    TopologyType::SMT => {
                        core_shift = level.shift_right_for_next_apic_id();
                        smt_count = level.processors();
                    },
                    TopologyType::Core => core_count = level.processors(),
                    _ => (),
                }
            }

            Some((
                CPUTopology {
                    num_cores: (core_count / smt_count) as usize,
                    threads_per_core: smt_count as usize,
                    logical_processors: core_count as usize,
                },
                APICToCPU {
                    core_shift_right: core_shift as usize,
                    smt_bitwise_and: if core_shift == 0 { 0 } else { u32::pow(2, core_shift) as usize - 1 },
                },
            ))
        } else {
            warn!("FIXME: find more ways to find CPU topology");

            None
        }
    } else {
        None
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

pub fn init(page_dir: &mut paging::PageDir, args: Option<BTreeMap<&str, &str>>) {
    unsafe {
        ints::init_irqs();
    }

    let mut cpus: CPU;

    if args.and_then(|a| a.get("acpi").cloned()).unwrap_or("yes") == "no" {
        warn!("ACPI disabled, ignoring APIC and other CPUs");

        // populate CPU geometry - 1 core, 1 thread
        cpus = CPU::new();
        cpus.add_core(1);
    } else {
        let cpuid = read_cpuid();

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

        debug!("cpu model is {model:?}");

        // try to get cpu topology from cpuid
        let res = get_cpu_topology();

        let mut topology = res.map(|(t, _)| t);

        // we can just assume everything's a core if get_cpu_topology failed
        let mapping = res.map(|(_, m)| m).unwrap_or_else(|| Default::default());

        debug!("cpu topology: {topology:#?}");
        debug!("apic id to cpu id mapping: {mapping:#?}");

        let has_apic = match cpuid.get_feature_info() {
            Some(feature_info) => feature_info.has_apic(),
            None => false,
        };

        if has_apic {
            // `mapping` only really matters here

            /*if let Some(detected_cpus) = acpi::detect_cpu_geometry(page_dir) {
                cpus = detected_cpus;
            } else {
                error!("error detecting CPUs, assuming 1 core 1 thread");

                cpus = CPU::new();
                cpus.add_core(1);
            }*/

            topology = Some(Default::default());
        } else {
            info!("no APIC detected, assuming 1 core 1 thread");

            topology = Some(Default::default());
        }

        cpus = topology.unwrap().to_cpu();
    }

    set_cpus(cpus);
}
