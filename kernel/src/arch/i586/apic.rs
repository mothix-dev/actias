use super::PAGE_SIZE;
use crate::{
    mm::paging::{get_kernel_page_dir, get_page_dir, get_page_manager, PageDirectory, PageFrame},
    task::cpu::ThreadID,
};
use alloc::{
    alloc::{alloc, Layout},
    vec::Vec,
};
use core::fmt;
use log::{debug, info, trace};
use volatile::{
    access::{ReadOnly, ReadWrite, WriteOnly},
    Volatile, // fluid
};
use x86::apic::{DeliveryMode, DestinationMode, DestinationShorthand, Level};

#[repr(usize)]
pub enum APICRegisters {
    LocalAPICID = 0x8,
    LocalAPICVersion = 0xc,
    TaskPriority = 0x20,
    ArbitrationPriority = 0x24,
    ProcessorPriority = 0x28,
    EOI = 0x2c,
    RemoteRead = 0x30,
    LogicalDestination = 0x34,
    DestinationFormat = 0x38,
    SpuriousInterruptVector = 0x3c,
    InService0 = 0x40,
    InService1 = 0x44,
    InService2 = 0x48,
    InService3 = 0x4c,
    InService4 = 0x50,
    InService5 = 0x54,
    InService6 = 0x58,
    InService7 = 0x5c,
    TriggerMode0 = 0x60,
    TriggerMode1 = 0x64,
    TriggerMode2 = 0x68,
    TriggerMode3 = 0x6c,
    TriggerMode4 = 0x70,
    TriggerMode5 = 0x74,
    TriggerMode6 = 0x78,
    TriggerMode7 = 0x7c,
    InterruptRequest0 = 0x80,
    InterruptRequest1 = 0x84,
    InterruptRequest2 = 0x88,
    InterruptRequest3 = 0x8c,
    InterruptRequest4 = 0x90,
    InterruptRequest5 = 0x94,
    InterruptRequest6 = 0x98,
    InterruptRequest7 = 0x9c,
    ErrorStatus = 0xa0,
    CorrectedMachineCheckInterrupt = 0xbc,
    InterruptCommand0 = 0xc0,
    InterruptCommand1 = 0xc4,
    LVTTimer = 0xc8,
    LVTThermalSensor = 0xcc,
    LVTPerfMonitoringCounters = 0xd0,
    LVTLINT0 = 0xd4,
    LVTLINT1 = 0xd8,
    LVTError = 0xdc,
    TimerInitialCount = 0xe0,
    TimerCurrentCount = 0xe4,
    TimerDivideConfiguration = 0xf8,
}

#[derive(Debug)]
pub struct LocalAPIC {
    pub local_apic_id: Volatile<&'static mut u32, ReadOnly>,
    pub local_apic_version: Volatile<&'static mut u32, ReadOnly>,
    pub task_priority: Volatile<&'static mut u32, ReadWrite>,
    pub arbitration_priority: Volatile<&'static mut u32, ReadOnly>,
    pub processor_priority: Volatile<&'static mut u32, ReadOnly>,
    pub eoi: Volatile<&'static mut u32, WriteOnly>,
    pub remote_read: Volatile<&'static mut u32, ReadOnly>,
    pub destination_mode: Volatile<&'static mut u32, ReadWrite>,
    pub destination_format: Volatile<&'static mut u32, ReadWrite>,
    pub spurious_interrupt_vector: Volatile<&'static mut u32, ReadWrite>,
    pub in_service: [Volatile<&'static mut u32, ReadOnly>; 8],
    pub trigger_mode: [Volatile<&'static mut u32, ReadOnly>; 8],
    pub interrupt_request: [Volatile<&'static mut u32, ReadOnly>; 8],
    pub error_status: Volatile<&'static mut u32, ReadOnly>,
    pub corrected_machine_check_interrupt: Volatile<&'static mut u32, ReadWrite>,
    pub interrupt_command: [Volatile<&'static mut u32, ReadWrite>; 2],
    pub lvt_timer: Volatile<&'static mut u32, ReadWrite>,
    pub lvt_thermal_sensor: Volatile<&'static mut u32, ReadWrite>,
    pub lvt_perf_monitoring_counters: Volatile<&'static mut u32, ReadWrite>,
    pub lvt_lint: [Volatile<&'static mut u32, ReadWrite>; 2],
    pub lvt_error: Volatile<&'static mut u32, ReadWrite>,
    pub timer_initial_count: Volatile<&'static mut u32, ReadWrite>,
    pub timer_current_count: Volatile<&'static mut u32, ReadOnly>,
    pub timer_divide_configuration: Volatile<&'static mut u32, ReadWrite>,
}

impl LocalAPIC {
    pub unsafe fn from_raw_pointer(mapped: *mut u32) -> Self {
        Self {
            local_apic_id: Volatile::new_read_only(&mut *(mapped.add(APICRegisters::LocalAPICID as usize))),
            local_apic_version: Volatile::new_read_only(&mut *(mapped.add(APICRegisters::LocalAPICVersion as usize))),
            task_priority: Volatile::new(&mut *(mapped.add(APICRegisters::TaskPriority as usize))),
            arbitration_priority: Volatile::new_read_only(&mut *(mapped.add(APICRegisters::ArbitrationPriority as usize))),
            processor_priority: Volatile::new_read_only(&mut *(mapped.add(APICRegisters::ProcessorPriority as usize))),
            eoi: Volatile::new_write_only(&mut *(mapped.add(APICRegisters::EOI as usize))),
            remote_read: Volatile::new_read_only(&mut *(mapped.add(APICRegisters::RemoteRead as usize))),
            destination_mode: Volatile::new(&mut *(mapped.add(APICRegisters::LogicalDestination as usize))),
            destination_format: Volatile::new(&mut *(mapped.add(APICRegisters::DestinationFormat as usize))),
            spurious_interrupt_vector: Volatile::new(&mut *(mapped.add(APICRegisters::SpuriousInterruptVector as usize))),
            in_service: [
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InService0 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InService1 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InService2 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InService3 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InService4 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InService5 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InService6 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InService7 as usize))),
            ],
            trigger_mode: [
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::TriggerMode0 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::TriggerMode1 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::TriggerMode2 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::TriggerMode3 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::TriggerMode4 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::TriggerMode5 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::TriggerMode6 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::TriggerMode7 as usize))),
            ],
            interrupt_request: [
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InterruptRequest0 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InterruptRequest1 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InterruptRequest2 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InterruptRequest3 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InterruptRequest4 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InterruptRequest5 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InterruptRequest6 as usize))),
                Volatile::new_read_only(&mut *(mapped.add(APICRegisters::InterruptRequest7 as usize))),
            ],
            error_status: Volatile::new_read_only(&mut *(mapped.add(APICRegisters::ErrorStatus as usize))),
            corrected_machine_check_interrupt: Volatile::new(&mut *(mapped.add(APICRegisters::CorrectedMachineCheckInterrupt as usize))),
            interrupt_command: [
                Volatile::new(&mut *(mapped.add(APICRegisters::InterruptCommand0 as usize))),
                Volatile::new(&mut *(mapped.add(APICRegisters::InterruptCommand1 as usize))),
            ],
            lvt_timer: Volatile::new(&mut *(mapped.add(APICRegisters::LVTTimer as usize))),
            lvt_thermal_sensor: Volatile::new(&mut *(mapped.add(APICRegisters::LVTThermalSensor as usize))),
            lvt_perf_monitoring_counters: Volatile::new(&mut *(mapped.add(APICRegisters::LVTPerfMonitoringCounters as usize))),
            lvt_lint: [
                Volatile::new(&mut *(mapped.add(APICRegisters::LVTLINT0 as usize))),
                Volatile::new(&mut *(mapped.add(APICRegisters::LVTLINT1 as usize))),
            ],
            lvt_error: Volatile::new(&mut *(mapped.add(APICRegisters::LVTError as usize))),
            timer_initial_count: Volatile::new(&mut *(mapped.add(APICRegisters::TimerInitialCount as usize))),
            timer_current_count: Volatile::new_read_only(&mut *(mapped.add(APICRegisters::TimerCurrentCount as usize))),
            timer_divide_configuration: Volatile::new(&mut *(mapped.add(APICRegisters::TimerDivideConfiguration as usize))),
        }
    }

    /// writes an interrupt command to the interrupt command registers
    pub fn write_interrupt_command(&mut self, command: InterruptCommand) {
        trace!("writing interrupt command {command:?}");

        let flags = super::get_flags();
        super::cli();

        self.interrupt_command[1].write(command.1);
        self.interrupt_command[0].write(command.0);

        super::set_flags(flags);
    }

    /// checks whether the last interrupt command sent was accepted
    pub fn was_interrupt_accepted(&self) -> bool {
        (self.interrupt_command[0].read() & (1 << 12)) > 0
    }

    /// waits for the last interrupt command sent to be accepted
    pub fn wait_for_interrupt_accepted(&self) {
        while !self.was_interrupt_accepted() {
            super::spin();
        }
    }

    /// sends a SIPI interrupt to the given CPU, telling it to start executing code at the provided page
    pub fn send_sipi(&mut self, timer_id: usize, id: u8, starting_page: u8) {
        debug!("sending SIPI to APIC {id} @ page {starting_page:#x}");

        let timer = crate::timer::get_timer(timer_id).unwrap();

        // assert INIT
        self.write_interrupt_command(InterruptCommandBuilder::new().delivery_mode(DeliveryMode::Init).level(Level::Assert).physical_destination(id).finish());

        // not sure if this is required or not, works on QEMU but doesn't on bochs
        //self.wait_for_interrupt_accepted();

        // de-assert INIT
        self.write_interrupt_command(
            InterruptCommandBuilder::new()
                .delivery_mode(DeliveryMode::Init)
                .level(Level::Deassert)
                .physical_destination(id)
                .finish(),
        );

        //self.wait_for_interrupt_accepted();

        // wait for 10 ms
        timer.wait(timer.millis() * 10);

        for _i in 0..2 {
            self.write_interrupt_command(
                InterruptCommandBuilder::new()
                    .delivery_mode(DeliveryMode::StartUp)
                    .starting_page_number(starting_page)
                    .physical_destination(id)
                    .finish(),
            );

            // wait as little time as we can
            timer.wait(1);

            //self.wait_for_interrupt_accepted();
        }

        // need to wait for a bit here to ensure things don't break when firing off multiple consecutive SIPIs
        timer.wait(timer.millis() * 10);
    }

    /// gets the ID of this APIC
    pub fn id(&self) -> u8 {
        (self.local_apic_id.read() >> 24) as u8
    }

    /// gets the version of this APIC
    pub fn version(&self) -> u8 {
        (self.local_apic_version.read() & 0xff) as u8
    }

    /// gets how many local vector table entries this APIC supports
    pub fn max_lvt_entries(&self) -> u8 {
        ((self.local_apic_version.read() >> 16) & 0xff) as u8
    }

    pub fn set_cmci_interrupt(&mut self, entry: LVTEntry) {
        self.corrected_machine_check_interrupt.write(entry.0);
    }

    pub fn set_timer_interrupt(&mut self, entry: LVTEntry) {
        self.lvt_timer.write(entry.0);
    }

    pub fn set_thermal_interrupt(&mut self, entry: LVTEntry) {
        self.lvt_thermal_sensor.write(entry.0);
    }

    pub fn set_perf_count_interrupt(&mut self, entry: LVTEntry) {
        self.lvt_perf_monitoring_counters.write(entry.0);
    }

    pub fn set_lint0_interrupt(&mut self, entry: LVTEntry) {
        self.lvt_lint[0].write(entry.0);
    }

    pub fn set_lint1_interrupt(&mut self, entry: LVTEntry) {
        self.lvt_lint[1].write(entry.0);
    }

    pub fn set_error_interrupt(&mut self, entry: LVTEntry) {
        self.lvt_error.write(entry.0);
    }

    pub fn set_spurious_interrupt(&mut self, vector: SpuriousInterruptVector) {
        self.spurious_interrupt_vector.write(vector.0);
    }

    pub fn check_error(&mut self) -> Result<(), APICError> {
        let code = self.error_status.read();

        if code > 0 {
            Err(APICError(code))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug)]
pub struct LVTEntry(u32);

#[repr(u32)]
pub enum IntDeliveryMode {
    Fixed = 0,
    SMI = 2,
    NMI = 4,
    INIT = 5,
    External = 7,
}

#[repr(u32)]
pub enum InputPinPolarity {
    ActiveHigh = 0,
    ActiveLow = 1,
}

#[repr(u32)]
pub enum TriggerMode {
    Edge = 0,
    Level = 1,
}

#[repr(u32)]
pub enum TimerMode {
    OneShot = 0,
    Periodic = 1,
    TSCDeadline = 2,
}

pub struct LVTEntryBuilder {
    vector: u8,
    delivery_mode: IntDeliveryMode,
    input_pin_polarity: InputPinPolarity,
    trigger_mode: TriggerMode,
    is_masked: bool,
    timer_mode: TimerMode,
}

impl LVTEntryBuilder {
    pub fn new() -> Self {
        Self {
            vector: 0,
            delivery_mode: IntDeliveryMode::Fixed,
            input_pin_polarity: InputPinPolarity::ActiveHigh,
            trigger_mode: TriggerMode::Edge,
            is_masked: false,
            timer_mode: TimerMode::OneShot,
        }
    }

    pub fn vector(mut self, vector: u8) -> Self {
        self.vector = vector;
        self
    }

    pub fn delivery_mode(mut self, delivery_mode: IntDeliveryMode) -> Self {
        self.delivery_mode = delivery_mode;
        self
    }

    pub fn input_pin_polarity(mut self, input_pin_polarity: InputPinPolarity) -> Self {
        self.input_pin_polarity = input_pin_polarity;
        self
    }

    pub fn trigger_mode(mut self, trigger_mode: TriggerMode) -> Self {
        self.trigger_mode = trigger_mode;
        self
    }

    pub fn mask(mut self) -> Self {
        self.is_masked = true;
        self
    }

    pub fn timer_mode(mut self, timer_mode: TimerMode) -> Self {
        self.timer_mode = timer_mode;
        self
    }

    pub fn finish(self) -> LVTEntry {
        let mut res = 0;

        res |= self.vector as u32;
        res |= (self.delivery_mode as u32) << 8;
        res |= (self.input_pin_polarity as u32) << 13;
        res |= (self.trigger_mode as u32) << 15;
        if self.is_masked {
            res |= 1 << 16;
        }
        res |= (self.timer_mode as u32) << 17;

        LVTEntry(res)
    }
}

impl Default for LVTEntryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct InterruptCommand(pub u32, pub u32);

pub struct InterruptCommandBuilder {
    vector_number: u8,
    delivery_mode: DeliveryMode,
    destination_mode: DestinationMode,
    level: Level,
    dest_kind: DestinationShorthand,
    physical_destination: u8,
}

impl InterruptCommandBuilder {
    /// creates a new interrupt command builder
    pub fn new() -> Self {
        Self {
            vector_number: 0,
            delivery_mode: DeliveryMode::Fixed,
            destination_mode: DestinationMode::Physical,
            level: Level::Assert,
            dest_kind: DestinationShorthand::NoShorthand,
            physical_destination: 0,
        }
    }

    /// sets the vector number of this interrupt command
    pub fn vector_number(mut self, vector_number: u8) -> Self {
        self.vector_number = vector_number;
        self
    }

    /// if this interrupt is of mode SIPI, set the page that the CPU should start executing from
    pub fn starting_page_number(mut self, starting_page_number: u8) -> Self {
        self.vector_number = starting_page_number;
        self
    }

    /// sets the destination mode of this interrupt command
    pub fn delivery_mode(mut self, delivery_mode: DeliveryMode) -> Self {
        self.delivery_mode = delivery_mode;
        self
    }

    /// sets whether this interrupt command should have a logical destination
    pub fn destination_mode(mut self, destination_mode: DestinationMode) -> Self {
        self.destination_mode = destination_mode;
        self
    }

    /// sets the IPI level of this interrupt command
    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// sets the destination type of this interrupt command
    pub fn destination_shorthand(mut self, dest_kind: DestinationShorthand) -> Self {
        self.dest_kind = dest_kind;
        self
    }

    /// sets the physical destination of this interrupt command
    pub fn physical_destination(mut self, destination: u8) -> Self {
        self.physical_destination = destination;
        self
    }

    pub fn finish(self) -> InterruptCommand {
        let mut command = InterruptCommand(0, 0);

        command.0 |= self.vector_number as u32;
        command.0 |= (self.delivery_mode as u32) << 8;
        command.0 |= (self.destination_mode as u32) << 11;
        command.0 |= 1 << 12; // set delivery status to 1, will be cleared when the interrupt is accepted
        if self.level == Level::Assert {
            command.0 |= 1 << 15;
        } else {
            command.0 |= 1 << 14;
        }
        command.0 |= (self.dest_kind as u32) << 18;

        command.1 |= ((self.physical_destination & 0xf) as u32) << 24;

        command
    }
}

impl Default for InterruptCommandBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SpuriousInterruptVector(u32);

pub struct SpuriousIntVectorBuilder {
    eoi_broadcast_suppression: bool,
    focus_processor_checking: bool,
    apic_disable: bool,
    vector: u8,
}

impl SpuriousIntVectorBuilder {
    pub fn new() -> Self {
        Self {
            eoi_broadcast_suppression: false,
            focus_processor_checking: true,
            apic_disable: false,
            vector: 0,
        }
    }

    pub fn vector(mut self, vector: u8) -> Self {
        self.vector = vector;
        self
    }

    pub fn disable_apic(mut self) -> Self {
        self.apic_disable = true;
        self
    }

    pub fn disable_focus_processor_checking(mut self) -> Self {
        self.focus_processor_checking = false;
        self
    }

    pub fn eoi_broadcast_suppression(mut self) -> Self {
        self.eoi_broadcast_suppression = true;
        self
    }

    pub fn finish(self) -> SpuriousInterruptVector {
        let mut res = 0;

        res |= self.vector as u32;
        if !self.apic_disable {
            res |= 1 << 8;
        }
        if self.focus_processor_checking {
            res |= 1 << 9;
        }
        if self.eoi_broadcast_suppression {
            res |= 1 << 12;
        }

        SpuriousInterruptVector(res)
    }
}

impl Default for SpuriousIntVectorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub struct APICError(u32);

impl fmt::Debug for APICError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut reasons: Vec<&str> = Vec::new();

        if (self.0 & (1 << 0)) > 0 {
            reasons.push("Send Checksum Error");
        }
        if (self.0 & (1 << 1)) > 0 {
            reasons.push("Receive Checksum Error");
        }
        if (self.0 & (1 << 2)) > 0 {
            reasons.push("Send Accept Error");
        }
        if (self.0 & (1 << 3)) > 0 {
            reasons.push("Receive Accept Error");
        }
        if (self.0 & (1 << 4)) > 0 {
            reasons.push("Redirectable IPI");
        }
        if (self.0 & (1 << 5)) > 0 {
            reasons.push("Sent Illegal Vector");
        }
        if (self.0 & (1 << 6)) > 0 {
            reasons.push("Received Illegal Vector");
        }
        if (self.0 & (1 << 7)) > 0 {
            reasons.push("Illegal Register Address");
        }

        write!(f, "APICError({})", reasons.join(", "))
    }
}

pub fn map_local_apic(addr: u64) {
    let layout = Layout::from_size_align(PAGE_SIZE, PAGE_SIZE).unwrap();

    let buf = unsafe { alloc(layout) };

    assert!(!buf.is_null(), "failed to allocate memory for mapping local APIC");

    get_page_manager().free_frame(&mut get_page_dir(None), buf as usize).expect("couldn't free page");

    get_page_dir(None)
        .set_page(
            buf as usize,
            Some(PageFrame {
                addr,
                present: true,
                user_mode: false,
                writable: true,
                copy_on_write: false,
                executable: false,
            }),
        )
        .expect("couldn't remap page");

    debug!("local APIC: {addr:#x} @ {buf:?}");

    unsafe {
        LOCAL_APIC = Some(LocalAPIC::from_raw_pointer(buf as *mut u32));
    }

    debug!("local APIC version {}", get_local_apic().unwrap().version());
}

static mut LOCAL_APIC: Option<LocalAPIC> = None;

pub fn get_local_apic() -> Option<&'static mut LocalAPIC> {
    // distributing mutable references is fine here since it's MMIO local to the current processor
    unsafe { LOCAL_APIC.as_mut() }
}

pub fn bring_up_cpus(apic_ids: &[u8]) {
    let thread_id = super::get_thread_id();
    let thread = crate::task::get_cpus().expect("CPUs not initialized").get_thread(thread_id).unwrap();

    let timer_num = thread.timer;

    // populate cpu bootstrap memory region
    let bootstrap_bytes = include_bytes!("../../../../target/cpu_bootstrap.bin");
    let bootstrap_addr = crate::platform::get_cpu_bootstrap_addr();

    // map bootstrap area into memory temporarily
    get_page_dir(None)
        .set_page(
            bootstrap_addr.try_into().unwrap(),
            Some(crate::mm::paging::PageFrame {
                addr: bootstrap_addr,
                present: true,
                user_mode: false,
                writable: true,
                copy_on_write: false,
                executable: true,
            }),
        )
        .unwrap();

    let bootstrap_area = unsafe { &mut *(bootstrap_addr as *mut [u8; PAGE_SIZE]) };

    bootstrap_area[0..bootstrap_bytes.len()].copy_from_slice(bootstrap_bytes);

    // specify protected mode entry point
    unsafe {
        *(&mut bootstrap_area[bootstrap_area.len() - 8] as *mut u8 as *mut u32) = cpu_entry_point as *const u8 as u32;
    }

    // specify page table physical address
    unsafe {
        *(&mut bootstrap_area[bootstrap_area.len() - 12] as *mut u8 as *mut u32) = get_kernel_page_dir().lock().inner().tables_physical_addr;
    }

    let local_apic = get_local_apic().expect("local APIC not mapped");
    let local_apic_id = local_apic.id();

    // start up other CPUs
    for &id in apic_ids {
        if id != local_apic_id {
            // allocate stack
            let stack_layout = Layout::from_size_align(super::STACK_SIZE + super::PAGE_SIZE, super::PAGE_SIZE).unwrap();
            let stack = unsafe { alloc(stack_layout) };

            // free bottom page of stack to prevent the stack from corrupting other memory without anyone knowing
            get_page_manager().free_frame(&mut get_page_dir(None), stack as usize).unwrap();

            let stack_end = stack as usize + stack_layout.size() - 1;

            // specify stack pointer
            unsafe {
                *(&mut bootstrap_area[bootstrap_area.len() - 4] as *mut u8 as *mut u32) = stack_end.try_into().unwrap();
            }

            local_apic.send_sipi(timer_num, id, (crate::platform::get_cpu_bootstrap_addr() / 0x1000).try_into().unwrap());
        }
    }

    // unmap the bootstrap area from memory
    get_page_dir(None).set_page(bootstrap_addr.try_into().unwrap(), None).unwrap();
}

unsafe extern "C" fn cpu_entry_point() {
    super::ints::load();

    // make sure we're on the latest page directory
    get_page_dir(None).switch_to();

    super::gdt::init_other_cpu(super::STACK_SIZE);

    let local_apic = get_local_apic().expect("local APIC not mapped");

    local_apic.set_spurious_interrupt(SpuriousIntVectorBuilder::new().vector(0xf0).finish());
    local_apic.eoi.write(0);

    local_apic.check_error().unwrap();

    let cpus = crate::task::get_cpus().expect("CPUs not initialized");
    let thread_id = super::get_thread_id();

    info!("CPU {thread_id} is alive!");

    super::sti(); // i spent HOURS debugging to find that i forgot this

    // signal that this CPU has started
    cpus.get_thread(thread_id).unwrap().start();

    get_page_dir(Some(thread_id)).switch_to();

    // FIXME: should probably store BSP's ID somewhere
    calibrate_apic_timer_from(cpus.get_thread(ThreadID { core: 0, thread: 0 }).unwrap().timer);

    local_apic.check_error().unwrap();

    super::start_context_switching();
}

const CALIBRATING_HZ_DIVIDE: usize = 8; // 1 second / 8 = 125 ms
const INIT_TIMER_DIV: usize = 16384; // 8192 is spot-on with bochs but is twice as fast on qemu, not sure if it's a problem or not
const TARGET_NS_PER_TICK: u64 = 100_000; // 10 kHz

fn calibrate_timer(calibrate_from: usize, calibrating: usize) -> u64 {
    let calibrate_from = crate::timer::get_timer(calibrate_from).unwrap();
    let calibrating = crate::timer::get_timer(calibrating).unwrap();

    let expire_time = calibrate_from.jiffies() + (calibrate_from.hz() / CALIBRATING_HZ_DIVIDE as u64);
    let starting_jiffies = calibrating.jiffies();

    while calibrate_from.jiffies() < expire_time {
        super::spin();
    }

    let ending_jiffies = calibrating.jiffies();

    (ending_jiffies - starting_jiffies) * CALIBRATING_HZ_DIVIDE as u64
}

/// very roughly calibrates the local APIC timer to 100k ns/t (10 kHz) from the provided timer source
pub fn calibrate_apic_timer_from(timer_num: usize) {
    let thread_id = super::get_thread_id();
    let thread = crate::task::get_cpus().expect("CPUs not initialized").get_thread(thread_id).unwrap();

    let calibrating = thread.timer;

    let local_apic = get_local_apic().expect("local APIC not mapped");

    debug!("calibrating APIC {} (CPU {thread_id})", local_apic.id());

    local_apic.set_timer_interrupt(LVTEntryBuilder::new().vector(0x30).timer_mode(TimerMode::Periodic).finish());
    local_apic.timer_divide_configuration.write(0b1011); // divide by 1

    // first calibration pass (roughly detect the timer's ticks per second value)
    local_apic.timer_initial_count.write(INIT_TIMER_DIV.try_into().unwrap());
    let hz = calibrate_timer(timer_num, calibrating);

    assert!(hz != 0, "timer didn't tick");

    let count_down = (INIT_TIMER_DIV as u64 * TARGET_NS_PER_TICK) / (1_000_000_000 / hz);

    // second calibration pass (timer may not be 10 kHz, so double check its frequency)
    local_apic.timer_initial_count.write(count_down.try_into().unwrap());
    let hz = calibrate_timer(timer_num, calibrating);

    crate::timer::get_timer(calibrating).unwrap().set_hz(hz);

    let ns_per_tick = 1_000_000_000 / hz;

    info!("calibrated APIC timer for CPU {thread_id} to {}.{} kHz ({ns_per_tick} ns/t)", hz / 1000, (hz % 1000) / 100);
}

pub fn init_bsp_apic() {
    // set spurious timer interrupt
    get_local_apic()
        .expect("local APIC not mapped")
        .set_spurious_interrupt(SpuriousIntVectorBuilder::new().vector(0xf0).finish());

    // calibrate BSP's APIC timer
    calibrate_apic_timer_from(super::ints::pit_timer_num());

    // disable PIT timer
    super::ints::disable_pit();
}

/// sends a non-maskable interrupt to the given CPU
pub fn send_nmi_to_cpu(id: ThreadID) -> bool {
    trace!("sending NMI to CPU {id}");

    if let Some(thread) = crate::task::get_cpus().and_then(|cpus| cpus.get_thread(id)) {
        if let Some(apic_id) = thread.info.apic_id.and_then(|i| i.try_into().ok()) {
            let local_apic = get_local_apic().expect("local APIC not mapped");
            local_apic.write_interrupt_command(InterruptCommandBuilder::new().delivery_mode(DeliveryMode::NMI).physical_destination(apic_id).finish());
            local_apic.check_error().unwrap();
            return true;
        }
    }

    false
}

/// sends a normal interrupt to the given CPU
pub fn send_interrupt_to_cpu(id: ThreadID, int: usize) -> bool {
    trace!("sending interrupt {int} to CPU {id}");

    if let Some(thread) = crate::task::get_cpus().and_then(|cpus| cpus.get_thread(id)) {
        if let Some(apic_id) = thread.info.apic_id.and_then(|i| i.try_into().ok()) {
            if let Ok(int) = int.try_into() {
                let local_apic = get_local_apic().expect("local APIC not mapped");
                local_apic.write_interrupt_command(
                    InterruptCommandBuilder::new()
                        .delivery_mode(DeliveryMode::Fixed)
                        .vector_number(int)
                        .physical_destination(apic_id)
                        .finish(),
                );
                local_apic.check_error().unwrap();
                return true;
            }
        }
    }

    false
}
