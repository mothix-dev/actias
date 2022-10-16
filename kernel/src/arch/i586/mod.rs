pub mod acpi;
pub mod apic;
pub mod gdt;
pub mod ints;
pub mod paging;

use crate::{
    task::{
        cpu::{ThreadID, CPU},
        set_cpus,
    },
};
use alloc::{collections::BTreeMap, string::String, vec::Vec};
use core::arch::asm;
use log::{debug, error, info, trace, warn};
use raw_cpuid::{CpuId, CpuIdResult, TopologyType};
use x86::{bits32::eflags::EFlags, segmentation::SegmentSelector, Ring, cpuid};

// various useful constants
pub const MEM_TOP: usize = 0xffffffff;

pub const PAGE_SIZE: usize = 0x1000;
pub const INV_PAGE_SIZE: usize = !(PAGE_SIZE - 1);

pub const MAX_STACK_FRAMES: usize = 1024;

// reasonable stack size
pub const STACK_SIZE: usize = 0x1000 * 4;

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
#[derive(Clone, Debug, Default)]
pub enum APICToCPU {
    /// APIC ID directly maps to core ID
    #[default]
    OneToOne,
    /// APIC ID to thread ID translation can be performed with simple bitwise math
    Bitwise {
        /// how many bits to shift the APIC ID right to get the core ID
        core_shift_right: usize,

        /// value to perform a bitwise AND of with the APIC ID to get the thread ID
        smt_bitwise_and: usize,
    },
    /// APIC ID to thread ID translation is arbitrary or unknown
    Arbitrary { translation: Vec<ThreadID> },
}

impl APICToCPU {
    pub fn apic_to_cpu(&self, apic_id: usize) -> ThreadID {
        match self {
            Self::OneToOne => ThreadID { core: apic_id, thread: 0 },
            Self::Bitwise { core_shift_right, smt_bitwise_and } => ThreadID {
                core: apic_id >> core_shift_right,
                thread: apic_id & smt_bitwise_and,
            },
            Self::Arbitrary { translation } => *translation.get(apic_id).expect("unable to get arbitrary APIC ID to thread ID translation entry"),
        }
    }
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
fn get_cpu_topology(cpuid: &CpuId) -> Option<(CPUTopology, APICToCPU)> {
    // TODO: figure out more ways to detect CPU topology with CPUID, this current method is a bit lacking

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

            let num_cores = level3_cores / level2_cores;
            let threads_per_core = level2_cores;
            let logical_processors = num_cores * threads_per_core;

            Some((
                CPUTopology {
                    num_cores,
                    threads_per_core,
                    logical_processors,
                },
                APICToCPU::Bitwise {
                    core_shift_right: bits_required_for(threads_per_core + 1),
                    smt_bitwise_and: find_nearest_power_of_2(threads_per_core.try_into().unwrap()) as usize - 1,
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
                    }
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
                APICToCPU::Bitwise {
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

pub const KERNEL_PAGE_DIR_SPLIT: usize = 0xe0000000;

#[derive(Copy, Clone, Debug)]
pub struct ThreadInfo {
    /// the ID of this thread's APIC
    pub apic_id: Option<usize>,

    pub processor_id: usize,

    /// this thread's stack
    pub stack: Option<usize>,
}

/// exits emulators if applicable then completely halts the CPU
///
/// # Safety
///
/// yeah
pub unsafe fn halt() -> ! {
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
#[inline(always)]
pub fn halt_until_interrupt() {
    unsafe {
        asm!("sti; hlt");
    }
}

/// busy waits for a cycle
#[inline(always)]
pub fn spin() {
    unsafe {
        asm!("pause");
    }
}

#[repr(transparent)]
pub struct CPUFlags(pub u32);

impl From<EFlags> for CPUFlags {
    fn from(flags: EFlags) -> Self {
        CPUFlags(flags.bits())
    }
}

impl From<CPUFlags> for EFlags {
    fn from(flags: CPUFlags) -> Self {
        Self::from_bits(flags.0).unwrap()
    }
}

/// gets the current CPU flags
#[inline(always)]
pub fn get_flags() -> CPUFlags {
    unsafe {
        let mut flags: u32;

        asm!(
            "pushfd",
            "pop {}",
            out(reg) flags,
        );

        CPUFlags(flags)
    }
}

/// sets the current CPU flags to the provided flags
#[inline(always)]
pub fn set_flags(flags: CPUFlags) {
    unsafe {
        asm!(
            "push {}",
            "popfd",
            in(reg) flags.0,
        );
    }
}

/// disables all interrupts for the current processor
#[inline(always)]
pub fn cli() {
    unsafe {
        asm!("cli");
    }
}

/// enables all interrupts for the current processor
#[inline(always)]
pub fn sti() {
    unsafe {
        asm!("sti");
    }
}

static mut APIC_TO_CPU: APICToCPU = APICToCPU::OneToOne;

pub fn get_thread_id() -> ThreadID {
    unsafe { apic::get_local_apic().map(|apic| APIC_TO_CPU.apic_to_cpu(apic.id().into())).unwrap_or_default() }
}

pub use apic::{send_nmi_to_cpu, send_interrupt_to_cpu};

/// refreshes the page at the provided address in the TLB
pub fn refresh_page(addr: usize) {
    trace!("flushing {:#x} in tlb", addr);
    unsafe {
        x86::tlb::flush(addr);
    }
}

pub const PAGE_REFRESH_INT: usize = 0x31;
pub const SYSCALL_INT: usize = 0x80;

pub fn safely_halt_cpu(regs: &mut Registers) {
    // sets the instruction pointer to set_interrupts_and_halt so the stack is properly cleaned up
    // failing to do this would cause the system to triple fault presumably due to stack overflow
    regs.cs = SegmentSelector::new(1, Ring::Ring0).bits().into();
    regs.ds = SegmentSelector::new(2, Ring::Ring0).bits().into();
    regs.ss = SegmentSelector::new(2, Ring::Ring0).bits().into();
    regs.eip = set_interrups_and_halt as *const u8 as u32;
}

// basically just halts and waits for an interrupt
#[naked]
unsafe extern "C" fn set_interrups_and_halt() {
    asm!(
        "2:", // 0 and 1 don't work lmao
        "sti",
        "hlt",
        "jmp 2b", // jump backwards to nearest label 2
        options(noreturn),
    );
}

fn init_single_core(timer: usize) {
    // set up CPUs
    let mut cpus = CPU::new();

    // 1 core, 1 thread
    // using PIT timer
    cpus.add_core();
    cpus.cores.get_mut(0).unwrap().add_thread(
        ThreadInfo {
            apic_id: None,
            processor_id: 0,
            stack: None,
        },
        timer,
    );

    // set global cpu topology, queues, etc
    set_cpus(cpus);
}

pub fn init(args: Option<BTreeMap<&str, &str>>, modules: BTreeMap<String, &'static [u8]>) {
    unsafe {
        ints::init_irqs();
    }

    let cpuid = read_cpuid();

    // try to get cpu topology from cpuid
    let res = get_cpu_topology(&cpuid);
    let topology = res.as_ref().map(|(t, _)| t);
    let mapping = res.as_ref().map(|(_, m)| m);

    if let Some(t) = topology.as_ref() {
        info!("detected {} CPUs ({} cores, {} threads per core)", t.logical_processors, t.num_cores, t.threads_per_core);
    }

    let can_use_acpi = args.as_ref().and_then(|a| a.get("acpi").cloned()).unwrap_or("yes") == "yes";

    if can_use_acpi && let Some((final_mapping, cpus)) = acpi::detect_cpus(topology, mapping) {
        // we have ACPI, so we for sure have an APIC

        unsafe {
            APIC_TO_CPU = final_mapping;
        }

        if topology.is_none() {
            info!("detected {} CPUs, unknown topology", cpus.cores.len());
        }

        // set global cpu topology, queues, etc
        crate::task::set_cpus(cpus);

        // init BSP's APIC, calibrate timer, disable PIT
        apic::init_bsp_apic();

        // get APIC IDs from CPU list
        let mut apic_ids: Vec<u8> = Vec::new();

        for core in crate::task::get_cpus().expect("CPUs not initialized").cores.iter() {
            for thread in core.threads.iter() {
                if let Some(id) = thread.info.apic_id.and_then(|i| i.try_into().ok()) {
                    apic_ids.push(id);
                }
            }
        }

        // bring up CPUs
        if apic_ids.len() > 1 {
            apic::bring_up_cpus(&apic_ids);
        }
    } else if cpuid.get_feature_info().map(|i| i.has_apic()).unwrap_or(false) {
        // we don't have ACPI but CPUID reports an APIC

        match crate::timer::register_timer(Some(ThreadID { core: 0, thread: 0 }), 0) {
            Ok(timer) => {
                apic::map_local_apic(0xfee00000);

                init_single_core(timer);

                apic::init_bsp_apic();
            }
            Err(err) => {
                error!("error registering timer: {err:?}");

                init_single_core(ints::pit_timer_num());
            }
        }
    } else {
        // no APIC?
        info!("no APIC detected, falling back on PIT for timer");

        init_single_core(ints::pit_timer_num());
    }

    // TODO: maybe intel MP support?

    const DEFAULT_INIT: &str = "init";
    let init_name = args.as_ref().and_then(|a| a.get("init").cloned()).unwrap_or(DEFAULT_INIT);
    let init_data = modules.get(init_name).unwrap_or_else(|| modules.get(DEFAULT_INIT).expect("couldn't find init in modules"));

    let mut process_list = crate::task::get_process_list();

    let process = process_list.create_process(paging::PageDir::new()).unwrap();
    crate::task::exec::exec_as::<paging::PageDir>(None, process_list.get_process_mut(process).unwrap(), init_data).expect("failed to exec init");

    let cpus = crate::task::get_cpus().expect("CPUs not initialized");

    let thread_id = cpus.find_thread_to_add_to().unwrap();

    cpus.get_thread(thread_id).unwrap().task_queue.lock().insert(crate::task::queue::TaskQueueEntry::new(crate::task::ProcessID { process, thread: 0 }, 0));

    drop(process_list);

    start_context_switching();
}

/*unsafe extern "C" fn test_thread_1() {
    loop {
        info!("UwU");

        for _i in 0..16384 {
            spin();
        }
    }
}

unsafe extern "C" fn test_thread_2() {
    loop {
        /*for _i in 0..8192 {
            spin();
        }*/

        info!("OwO");

        for _i in 0..8192 {
            spin();
        }
    }
}

fn test_create_thread(entry_point: unsafe extern "C" fn()) {
    let mut process_list = crate::task::get_process_list();

    use crate::mm::paging::PageDirectory;

    let mut page_dir = paging::PageDir::new();

    for addr in (KERNEL_PAGE_DIR_SPLIT - STACK_SIZE..KERNEL_PAGE_DIR_SPLIT).step_by(PAGE_SIZE) {
        // make sure there's actually something here so we don't deadlock if we need to allocate something and the page manager is busy
        page_dir.set_page(addr, None).unwrap();
    
        get_page_manager().alloc_frame(&mut page_dir, addr, true, true).unwrap();
    }

    let stack_end = KERNEL_PAGE_DIR_SPLIT - 4096;

    let process = process_list.create_process(page_dir).unwrap();
    process_list.get_process_mut(process).unwrap().threads.push(crate::task::Thread {
        registers: Registers {
            cs: SegmentSelector::new(1, Ring::Ring0).bits().into(),
            ds: SegmentSelector::new(2, Ring::Ring0).bits().into(),
            ss: SegmentSelector::new(2, Ring::Ring0).bits().into(),
            /*cs: SegmentSelector::new(3, Ring::Ring3).bits().into(),
            ds: SegmentSelector::new(4, Ring::Ring3).bits().into(),
            ss: SegmentSelector::new(4, Ring::Ring3).bits().into(),*/

            edi: 0,
            esi: 0,
            ebp: stack_end as u32,
            esp: stack_end as u32,
            ebx: 0,
            edx: 0,
            ecx: 0,
            eax: 0,
            error_code: 0, // lol, lmao
            eip: entry_point as *const u8 as u32,
            eflags: get_flags().0,
            useresp: stack_end as u32,
        },
        priority: 0,
        cpu: None,
        is_blocked: false,
    });

    let thread_id = crate::task::get_cpus().find_thread_to_add_to().unwrap();

    crate::task::get_cpus().get_thread(thread_id).unwrap().task_queue.lock().insert(crate::task::queue::TaskQueueEntry::new(crate::task::ProcessID { process, thread: 0 }, 0));

    drop(process_list);
}*/

fn start_context_switching() {
    // set timer and wait for it to expire to kick off context switching
    let thread_id = get_thread_id();
    let thread = crate::task::get_cpus().expect("CPUs not initialized").get_thread(thread_id).unwrap();

    info!("CPU {thread_id} starting context switching");

    crate::task::wait_for_context_switch(thread.timer, thread_id);
}
