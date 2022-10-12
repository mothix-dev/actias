use super::{paging::PageDir, PAGE_SIZE};
use crate::mm::paging::{get_page_manager, PageDirectory, PageFrame};
use alloc::alloc::{alloc, Layout};
use log::debug;
use volatile::{
    access::{ReadOnly, ReadWrite, WriteOnly},
    Volatile, // fluid
};

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
    pub local_apic_id: Volatile<&'static mut u32, ReadWrite>,
    pub local_apic_version: Volatile<&'static mut u32, ReadOnly>,
    pub task_priority: Volatile<&'static mut u32, ReadWrite>,
    pub arbitration_priority: Volatile<&'static mut u32, ReadOnly>,
    pub processor_priority: Volatile<&'static mut u32, ReadOnly>,
    pub eoi: Volatile<&'static mut u32, WriteOnly>,
    pub remote_read: Volatile<&'static mut u32, ReadOnly>,
    pub logical_destination: Volatile<&'static mut u32, ReadWrite>,
    pub destination_format: Volatile<&'static mut u32, ReadWrite>,
    pub spurious_interrupt_vector: Volatile<&'static mut u32, ReadWrite>,
    pub in_service: [Volatile<&'static mut u32, ReadOnly>; 8],
    pub trigger_mode: [Volatile<&'static mut u32, ReadOnly>; 8],
    pub interrupt_request: [Volatile<&'static mut u32, ReadOnly>; 8],
    pub error_status: Volatile<&'static mut u32, ReadWrite>,
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
            local_apic_id: Volatile::new(&mut *(mapped.add(APICRegisters::LocalAPICID as usize))),
            local_apic_version: Volatile::new_read_only(&mut *(mapped.add(APICRegisters::LocalAPICVersion as usize))),
            task_priority: Volatile::new(&mut *(mapped.add(APICRegisters::TaskPriority as usize))),
            arbitration_priority: Volatile::new_read_only(&mut *(mapped.add(APICRegisters::ArbitrationPriority as usize))),
            processor_priority: Volatile::new_read_only(&mut *(mapped.add(APICRegisters::ProcessorPriority as usize))),
            eoi: Volatile::new_write_only(&mut *(mapped.add(APICRegisters::EOI as usize))),
            remote_read: Volatile::new_read_only(&mut *(mapped.add(APICRegisters::RemoteRead as usize))),
            logical_destination: Volatile::new(&mut *(mapped.add(APICRegisters::LogicalDestination as usize))),
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
            error_status: Volatile::new(&mut *(mapped.add(APICRegisters::ErrorStatus as usize))),
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
        self.write_interrupt_command(InterruptCommandBuilder::new().dest_mode(InterruptDestMode::INIT).physical_destination(id).finish());

        // not sure if this is required or not, works on QEMU but doesn't on bochs
        //self.wait_for_interrupt_accepted();

        // de-assert INIT
        self.write_interrupt_command(
            InterruptCommandBuilder::new()
                .dest_mode(InterruptDestMode::INIT)
                .init_level_deassert()
                .physical_destination(id)
                .finish(),
        );

        //self.wait_for_interrupt_accepted();

        // wait for 10 ms
        timer.wait(timer.millis() * 10);

        for _i in 0..2 {
            self.write_interrupt_command(
                InterruptCommandBuilder::new()
                    .dest_mode(InterruptDestMode::SIPI)
                    .starting_page_number(starting_page)
                    .physical_destination(id)
                    .finish(),
            );

            // wait 1 ms
            timer.wait(timer.millis());

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
    pub fn version(&self) -> u16 {
        (self.local_apic_version.read() >> 16) as u16
    }
}

pub struct InterruptCommand(pub u32, pub u32);

#[repr(u32)]
pub enum InterruptDestMode {
    Normal = 0,
    LowestPriority = 1,
    SMI = 2,
    NMI = 4,
    INIT = 5,
    SIPI = 6,
}

#[repr(u32)]
pub enum InterruptDestKind {
    Normal = 0,
    Local = 1,
    AllCPUs = 2,
    AllCPUsButCurrent = 3,
}

pub struct InterruptCommandBuilder {
    vector_number: u8,
    dest_mode: InterruptDestMode,
    logical_destination: bool,
    init_level_deassert: bool,
    dest_kind: InterruptDestKind,
    physical_destination: u8,
}

impl InterruptCommandBuilder {
    /// creates a new interrupt command builder
    pub fn new() -> Self {
        Self {
            vector_number: 0,
            dest_mode: InterruptDestMode::Normal,
            logical_destination: false,
            init_level_deassert: false,
            dest_kind: InterruptDestKind::Normal,
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
    pub fn dest_mode(mut self, dest_mode: InterruptDestMode) -> Self {
        self.dest_mode = dest_mode;
        self
    }

    /// sets whether this interrupt command should have a logical destination
    pub fn logical_destination(mut self) -> Self {
        self.logical_destination = true;
        self
    }

    /// set if
    pub fn init_level_deassert(mut self) -> Self {
        self.init_level_deassert = true;
        self
    }

    /// sets the destination type of this interrupt command
    pub fn dest_kind(mut self, dest_kind: InterruptDestKind) -> Self {
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
        command.0 |= (self.dest_mode as u32) << 8;
        if self.logical_destination {
            command.0 |= 1 << 11;
        }
        command.0 |= 1 << 12; // set delivery status to 1, will be cleared when the interrupt is accepted
        if self.init_level_deassert {
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

pub fn map_local_apic(page_dir: &mut PageDir<'static>, addr: u64) {
    let layout = Layout::from_size_align(PAGE_SIZE, PAGE_SIZE).unwrap();

    let buf = unsafe { alloc(layout) };

    assert!(!buf.is_null(), "failed to allocate memory for mapping local APIC");

    get_page_manager().free_frame(page_dir, buf as usize).expect("couldn't free page");

    page_dir
        .set_page(
            buf as usize,
            Some(PageFrame {
                addr,
                present: true,
                user_mode: false,
                writable: true,
                copy_on_write: false,
            }),
        )
        .expect("couldn't remap page");

    debug!("local APIC: {addr:#x} @ {buf:?}");

    unsafe {
        LOCAL_APIC = Some(LocalAPIC::from_raw_pointer(buf as *mut u32));
    }

    debug!("local APIC version {}", get_local_apic().version());
}

static mut LOCAL_APIC: Option<LocalAPIC> = None;

pub fn get_local_apic() -> &'static mut LocalAPIC {
    // distributing mutable references is fine here since it's MMIO local to the current processor
    unsafe { LOCAL_APIC.as_mut().expect("local APIC not mapped yet") }
}

pub fn bring_up_cpus(page_dir: &mut super::paging::PageDir<'static>, apic_ids: &[u8]) {
    // populate cpu bootstrap memory region
    let bootstrap_bytes = include_bytes!("../../../../target/cpu_bootstrap.bin");
    let bootstrap_addr = crate::platform::get_cpu_bootstrap_addr();

    // map bootstrap area into memory temporarily
    page_dir
        .set_page(
            bootstrap_addr.try_into().unwrap(),
            Some(crate::mm::paging::PageFrame {
                addr: bootstrap_addr,
                present: true,
                user_mode: false,
                writable: true,
                copy_on_write: false,
            }),
        )
        .unwrap();

    let bootstrap_area = unsafe { &mut *(bootstrap_addr as *mut [u8; PAGE_SIZE]) };

    bootstrap_area[0..bootstrap_bytes.len()].copy_from_slice(bootstrap_bytes);

    // specify stack pointer
    unsafe {
        *(&mut bootstrap_area[bootstrap_area.len() - 4] as *mut u8 as *mut u32) = 0x2000 - 12;
    }

    // specify protected mode entry point
    unsafe {
        *(&mut bootstrap_area[bootstrap_area.len() - 8] as *mut u8 as *mut u32) = cpu_entry_point as *const u8 as u32;
    }

    debug!("page dir @ {:#x}", page_dir.tables_physical_addr);

    // specify page table physical address
    unsafe {
        *(&mut bootstrap_area[bootstrap_area.len() - 12] as *mut u8 as *mut u32) = page_dir.tables_physical_addr;
    }

    let local_apic = get_local_apic();
    let local_apic_id = local_apic.id();

    // start up other CPUs
    for &id in apic_ids {
        if id != local_apic_id {
            local_apic.send_sipi(super::ints::pit_timer_num(), id, (crate::platform::get_cpu_bootstrap_addr() / 0x1000).try_into().unwrap());
        }
    }

    // unmap the bootstrap area from memory
    page_dir.set_page(bootstrap_addr.try_into().unwrap(), None).unwrap();
}

unsafe extern "C" fn cpu_entry_point() {
    use log::info;
    use core::arch::asm;

    super::ints::load();
    super::gdt::init_other_cpu(0x1000 * 4);

    info!("CPU {} is alive!", super::get_thread_id());

    loop {
        asm!("cli; hlt");
    }
}
