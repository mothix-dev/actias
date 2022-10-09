//! i586 low level interrupt/exception handling

use super::halt;
/*use crate::{
    arch::{tasks::exit_current_task, PAGE_SIZE},
    platform::debug::exit_failure,
};
use crate::tasks::{get_current_task, get_current_task_mut, remove_page_reference, IN_TASK};*/
use crate::util::debug::FormatHex;
use aligned::{Aligned, A16};
use bitmask_enum::bitmask;
use core::{arch::asm, fmt};
use interrupt_macro::*;
use log::{debug, error, info};
use x86::{
    dtables::{lidt, DescriptorTablePointer},
    io::outb,
};

/// IDT flags
#[bitmask(u8)]
pub enum IDTFlags {
    X16Interrupt = Self(0x06),
    X16Trap = Self(0x07),
    X32Task = Self(0x05),
    X32Interrupt = Self(0x0e),
    X32Trap = Self(0x0f),
    Ring1 = Self(0x40),
    Ring2 = Self(0x20),
    Ring3 = Self(0x60),
    Present = Self(0x80),

    Exception = Self(Self::X32Interrupt.0 | Self::Present.0),            // exception
    External = Self(Self::X32Interrupt.0 | Self::Present.0),             // external interrupt
    Call = Self(Self::X32Interrupt.0 | Self::Present.0 | Self::Ring3.0), // system call
}

/// entry in IDT
/// this describes an interrupt handler (i.e. where it is, how it works, etc)
#[repr(C, packed(16))]
#[derive(Copy, Clone)]
pub struct IDTEntry {
    /// low 16 bits of handler pointer
    isr_low: u16,

    /// GDT segment selector to be loaded before calling handler
    kernel_cs: u16,

    /// unused
    reserved: u8,

    /// type and attributes
    attributes: u8,

    /// high 16 bits of handler pointer
    isr_high: u16,
}

impl IDTEntry {
    /// creates a new IDT entry
    pub fn new(isr: *const (), flags: IDTFlags) -> Self {
        Self {
            // not sure if casting to u16 will only return lower 2 bytes?
            isr_low: ((isr as u32) & 0xffff) as u16, // gets address of function pointer, then chops off the top 2 bytes
            isr_high: ((isr as u32) >> 16) as u16,   // upper 2 bytes
            kernel_cs: 0x08,                         // offset of kernel code selector in GDT (see boot.S)
            attributes: flags.0,
            reserved: 0,
        }
    }

    /// creates an empty IDT entry
    const fn new_empty() -> Self {
        Self {
            // empty entry
            isr_low: 0,
            kernel_cs: 0,
            reserved: 0,
            attributes: 0,
            isr_high: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.isr_low == 0 && self.isr_high == 0
    }
}

/// list of exceptions
pub enum Exceptions {
    /// divide-by-zero error
    DivideByZero = 0,

    /// debug
    Debug = 1,

    /// non-maskable interrupt
    NonMaskableInterrupt = 2,

    /// breakpoint
    Breakpoint = 3,

    /// overflow
    Overflow = 4,

    /// bound range exceeded
    BoundRangeExceeded = 5,

    /// invalid opcode
    InvalidOpcode = 6,

    /// device not available
    DeviceNotAvailable = 7,

    /// double fault
    DoubleFault = 8,

    /// coprocessor segment overrun
    CoprocessorSegmentOverrun = 9,

    /// invalid TSS
    InvalidTSS = 10,

    /// segment not present
    SegmentNotPresent = 11,

    /// stack segment fault
    StackSegmentFault = 12,

    /// general protection fault
    GeneralProtectionFault = 13,

    /// page fault
    PageFault = 14,

    /// x87 floating point exception
    FloatingPoint = 16,

    /// alignment check
    AlignmentCheck = 17,

    /// machine check
    MachineCheck = 18,

    /// SIMD floating point exception
    SIMDFloatingPoint = 19,

    /// virtualization exception
    Virtualization = 20,

    /// control protection exception
    ControlProtection = 21,

    /// hypervisor injection exception
    HypervisorInjection = 28,

    /// vmm communication exception
    VMMCommunication = 29,

    /// security exception
    Security = 30,
}

/// page fault error code wrapper
#[repr(transparent)]
pub struct PageFaultErrorCode(u32);

impl fmt::Display for PageFaultErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PageFaultErrorCode {{")?;

        if self.0 & (1 << 0) > 0 {
            write!(f, " present,")?;
        }

        if self.0 & (1 << 1) > 0 {
            write!(f, " write")?;
        } else {
            write!(f, " read")?;
        }

        if self.0 & (1 << 2) > 0 {
            write!(f, ", user mode")?;
        } else {
            write!(f, ", supervisor mode")?;
        }

        if self.0 & (1 << 3) > 0 {
            write!(f, ", reserved")?;
        }

        if self.0 & (1 << 4) > 0 {
            write!(f, ", instruction fetch")?;
        } else {
            write!(f, ", data access")?;
        }

        if self.0 & (1 << 5) > 0 {
            write!(f, ", protection-key")?;
        }

        if self.0 & (1 << 6) > 0 {
            write!(f, ", shadow")?;
        }

        if self.0 & (1 << 15) > 0 {
            write!(f, ", sgx")?;
        }

        write!(f, " }}")
    }
}

/// registers passed to interrupt handlers
#[repr(C, packed(32))]
#[derive(Default, Copy, Clone)]
pub struct InterruptRegisters {
    pub ds: u32,
    pub edi: u32,
    pub esi: u32,
    pub ebp: u32,
    pub esp: u32,
    pub ebx: u32,
    pub edx: u32,
    pub ecx: u32,
    pub eax: u32,
    pub error_code: u32,
    pub eip: u32,
    pub cs: u32,
    pub eflags: u32,
    pub useresp: u32,
    pub ss: u32,
}

impl fmt::Debug for InterruptRegisters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InterruptRegisters")
            .field("ds", &FormatHex(self.ds))
            .field("edi", &FormatHex(self.edi))
            .field("esi", &FormatHex(self.esi))
            .field("ebp", &FormatHex(self.ebp))
            .field("esp", &FormatHex(self.esp))
            .field("ebx", &FormatHex(self.ebx))
            .field("edx", &FormatHex(self.edx))
            .field("ecx", &FormatHex(self.ecx))
            .field("eax", &FormatHex(self.eax))
            .field("error_code", &FormatHex(self.error_code))
            .field("eip", &FormatHex(self.eip))
            .field("cs", &FormatHex(self.cs))
            .field("eflags", &FormatHex(self.eflags))
            .field("useresp", &FormatHex(self.useresp))
            .field("ss", &FormatHex(self.ss))
            .finish()
    }
}

unsafe fn generic_exception(name: &str, regs: &InterruptRegisters) {
    if regs.error_code == 0 {
        error!("PANIC: {} @ {:#x}, no error code", name, regs.eip);
    } else {
        error!("PANIC: {} @ {:#x}, error code {:#x}", name, regs.eip, regs.error_code);
    }

    info!("{:#?}", regs);

    halt();
}

/// exception handler for divide by zero
#[interrupt(x86)]
unsafe fn divide_by_zero_handler(regs: &InterruptRegisters) {
    generic_exception("divide by zero", regs);
}

/// exception handler for breakpoint
#[interrupt(x86)]
unsafe fn breakpoint_handler(regs: &InterruptRegisters) {
    info!("breakpoint @ {:#x}", regs.eip);
}

/// exception handler for overflow
#[interrupt(x86)]
unsafe fn overflow_handler(regs: &InterruptRegisters) {
    info!("overflow @ {:#x}", regs.eip);
}

/// exception handler for bound range exceeded
#[interrupt(x86)]
unsafe fn bound_range_handler(regs: &InterruptRegisters) {
    generic_exception("bound range exceeded", regs);
}

/// exception handler for invalid opcode
#[interrupt(x86)]
unsafe fn invalid_opcode_handler(regs: &InterruptRegisters) {
    generic_exception("invalid opcode", regs);
}

/// exception handler for device not available
#[interrupt(x86)]
unsafe fn device_not_available_handler(regs: &InterruptRegisters) {
    generic_exception("device not available", regs);
}

/// exception handler for double fault
#[interrupt(x86_error_code)]
unsafe fn double_fault_handler(regs: &InterruptRegisters) {
    generic_exception("double fault", regs);
}

/// exception handler for invalid tss
#[interrupt(x86_error_code)]
unsafe fn invalid_tss_handler(regs: &InterruptRegisters) {
    generic_exception("invalid TSS", regs);
}

/// exception handler for segment not present
#[interrupt(x86_error_code)]
unsafe fn segment_not_present_handler(regs: &InterruptRegisters) {
    generic_exception("segment not present", regs);
}

/// exception handler for stack-segment fault
#[interrupt(x86_error_code)]
unsafe fn stack_segment_handler(regs: &InterruptRegisters) {
    generic_exception("stack-segment fault", regs);
}

/// exception handler for general protection fault
#[interrupt(x86_error_code)]
unsafe fn general_protection_fault_handler(regs: &InterruptRegisters) {
    generic_exception("general protection fault", regs);
}

/// exception handler for page fault
#[interrupt(x86_error_code)]
unsafe extern "x86-interrupt" fn page_fault_handler(regs: &InterruptRegisters) {
    /*let mut address: u32;
    asm!("mov {0}, cr2", out(reg) address);*/

    /*// no longer in task, indicate as such
    let was_in_task = IN_TASK;
    IN_TASK = false;

    // switch to kernel's page directory if initialized
    if let Some(dir) = PAGE_DIR.as_mut() {
        dir.switch_to();
    }

    // rust moment
    if !was_in_task ||
        // is there a current task?
        if let Some(current) = get_current_task_mut() {
            // get reference to kernel's page directory
            let dir = PAGE_DIR.as_mut().unwrap();

            // get current task's page entry for given address
            if let Some(page) = current.state.pages.get_page(address, false) {
                let page = &mut *page;

                // get flags
                let flags: PageTableFlags = page.get_flags().into();

                // is read/write flag unset and copy on write flag set?
                if flags & PageTableFlags::ReadWrite == 0 && flags & PageTableFlags::CopyOnWrite != 0 {
                    // this is a terrible copy on write implementation but at least it works

                    trace!("copy on write, accessed @ {:#x}", address);

                    let old_addr = page.get_address();
                    let page_addr = address as u64 & !(PAGE_SIZE as u64 - 1);
                    let page_mem = current.state.read_mem(page_addr, PAGE_SIZE, true).unwrap();

                    page.set_unused();

                    match dir.alloc_frame(page, false, true) {
                        Ok(_) => {
                            current.state.write_mem(page_addr, &page_mem, true).unwrap();

                            remove_page_reference(old_addr as u64);

                            current.state.pages.switch_to();

                            false
                        },
                        Err(msg) => {
                            error!("couldn't allocate frame for copy on write: {}", msg);

                            true // panic
                        }
                    }
                } else {
                    true // panic
                }
            } else {
                true // panic
            }
        } else {
            true // panic
        }
    {*/
    //IN_TASK = was_in_task; // make sure generic handler knows whether we were in a task or not
    let mut address: u32;
    asm!("mov {0}, cr2", out(reg) address);

    error!("PANIC: page fault @ {:#x} (accessed {:#x}), error code {:#x}", regs.eip, address, regs.error_code);
    info!("{:#?}", regs);
    halt();
    /*}

    IN_TASK = was_in_task;*/
}

/// exception handler for x87 floating point exception
#[interrupt(x86)]
unsafe fn x87_fpu_exception_handler(regs: &InterruptRegisters) {
    generic_exception("x87 FPU exception", regs);
}

/// exception handler for alignment check
#[interrupt(x86_error_code)]
unsafe fn alignment_check_handler(regs: &InterruptRegisters) {
    generic_exception("alignment check", regs);
}

/// exception handler for SIMD floating point exception
#[interrupt(x86)]
unsafe fn simd_fpu_exception_handler(regs: &InterruptRegisters) {
    generic_exception("SIMD FPU exception", regs);
}

/// exception handler for virtualization exception
#[interrupt(x86)]
unsafe fn virtualization_exception_handler(regs: &InterruptRegisters) {
    generic_exception("virtualization exception", regs);
}

/// exception handler for control protection exception
#[interrupt(x86_error_code)]
unsafe fn control_protection_handler(regs: &InterruptRegisters) {
    generic_exception("control protection exception", regs);
}

/// exception handler for hypervisor injection exception
#[interrupt(x86)]
unsafe fn hypervisor_injection_handler(regs: &InterruptRegisters) {
    generic_exception("hypervisor injection exception", regs);
}

/// exception handler for VMM communication exception
#[interrupt(x86_error_code)]
unsafe fn vmm_exception_handler(regs: &InterruptRegisters) {
    generic_exception("VMM commuication exception", regs);
}

/// exception handler for security exception
#[interrupt(x86_error_code)]
unsafe fn security_exception_handler(regs: &InterruptRegisters) {
    generic_exception("security exception", regs);
}

pub type InterruptHandler = fn(&mut InterruptRegisters);

const IRQ_HANDLER_INIT: Option<InterruptHandler> = None;
static mut IRQ_HANDLERS: [Option<InterruptHandler>; 16] = [IRQ_HANDLER_INIT; 16];
static mut PIT_TIMER_NUM: usize = 0;

// this is a terrible and inefficient way of doing things but i don't really care
#[interrupt(x86)]
unsafe fn irq0_handler(regs: &mut InterruptRegisters) {
    /*if let Some(h) = IRQ_HANDLERS[0].as_ref() {
        (h)(regs);
    }*/
    // irq0 is always timer
    if let Some(timer) = crate::timer::get_timer(PIT_TIMER_NUM) {
        timer.try_tick(regs);
    }

    outb(0x20, 0x20); // reset primary interrupt controller
}

#[interrupt(x86)]
unsafe fn irq1_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[1].as_ref() {
        (h)(regs);
    }

    outb(0x20, 0x20);
}

/*#[interrupt(x86)]
unsafe fn irq2_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[2].as_ref() {
        (h)(regs);
    }

    outb(0x20, 0x20);
}*/

#[interrupt(x86)]
unsafe fn irq3_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[3].as_ref() {
        (h)(regs);
    }

    outb(0x20, 0x20);
}

#[interrupt(x86)]
unsafe fn irq4_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[4].as_ref() {
        (h)(regs);
    }

    outb(0x20, 0x20);
}

#[interrupt(x86)]
unsafe fn irq5_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[5].as_ref() {
        (h)(regs);
    }

    outb(0x20, 0x20);
}

#[interrupt(x86)]
unsafe fn irq6_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[6].as_ref() {
        (h)(regs);
    }

    outb(0x20, 0x20);
}

#[interrupt(x86)]
unsafe fn irq7_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[7].as_ref() {
        (h)(regs);
    }

    outb(0x20, 0x20);
}

#[interrupt(x86)]
unsafe fn irq8_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[8].as_ref() {
        (h)(regs);
    }

    outb(0xa0, 0x20); // reset secondary interrupt controller
    outb(0x20, 0x20);
}

#[interrupt(x86)]
unsafe fn irq9_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[9].as_ref() {
        (h)(regs);
    }

    outb(0xa0, 0x20);
    outb(0x20, 0x20);
}

#[interrupt(x86)]
unsafe fn irq10_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[10].as_ref() {
        (h)(regs);
    }

    outb(0xa0, 0x20);
    outb(0x20, 0x20);
}

#[interrupt(x86)]
unsafe fn irq11_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[11].as_ref() {
        (h)(regs);
    }

    outb(0xa0, 0x20);
    outb(0x20, 0x20);
}

#[interrupt(x86)]
unsafe fn irq12_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[12].as_ref() {
        (h)(regs);
    }

    outb(0xa0, 0x20);
    outb(0x20, 0x20);
}

#[interrupt(x86)]
unsafe fn irq13_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[13].as_ref() {
        (h)(regs);
    }

    outb(0xa0, 0x20);
    outb(0x20, 0x20);
}

#[interrupt(x86)]
unsafe fn irq14_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[14].as_ref() {
        (h)(regs);
    }

    outb(0xa0, 0x20);
    outb(0x20, 0x20);
}

#[interrupt(x86)]
unsafe fn irq15_handler(regs: &mut InterruptRegisters) {
    if let Some(h) = IRQ_HANDLERS[15].as_ref() {
        (h)(regs);
    }

    outb(0xa0, 0x20);
    outb(0x20, 0x20);
}

/// how many entries do we want in our IDT
pub const IDT_ENTRIES: usize = 256;

/// the IDT itself (aligned to 16 bits for performance)
pub static mut IDT: Aligned<A16, [IDTEntry; IDT_ENTRIES]> = Aligned([IDTEntry::new_empty(); IDT_ENTRIES]);

/// set up and load IDT
pub unsafe fn init() {
    // set up PIT, basically just some magic incantation
    outb(0x20, 0x11);
    outb(0xa0, 0x11);
    outb(0x21, 0x20);
    outb(0xa1, 0x28);
    outb(0x21, 0x04);
    outb(0xa1, 0x02);
    outb(0x21, 0x01);
    outb(0xa1, 0x01);
    outb(0x21, 0x0);
    outb(0xa1, 0x0);

    debug!("idt @ {:#x}", &IDT as *const _ as usize);

    // set up exception handlers
    IDT[Exceptions::DivideByZero as usize] = IDTEntry::new(divide_by_zero_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::Breakpoint as usize] = IDTEntry::new(breakpoint_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::Overflow as usize] = IDTEntry::new(overflow_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::BoundRangeExceeded as usize] = IDTEntry::new(bound_range_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::InvalidOpcode as usize] = IDTEntry::new(invalid_opcode_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::DeviceNotAvailable as usize] = IDTEntry::new(device_not_available_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::DoubleFault as usize] = IDTEntry::new(double_fault_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::InvalidTSS as usize] = IDTEntry::new(invalid_tss_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::SegmentNotPresent as usize] = IDTEntry::new(segment_not_present_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::StackSegmentFault as usize] = IDTEntry::new(stack_segment_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::GeneralProtectionFault as usize] = IDTEntry::new(general_protection_fault_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::PageFault as usize] = IDTEntry::new(page_fault_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::FloatingPoint as usize] = IDTEntry::new(x87_fpu_exception_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::AlignmentCheck as usize] = IDTEntry::new(alignment_check_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::SIMDFloatingPoint as usize] = IDTEntry::new(simd_fpu_exception_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::Virtualization as usize] = IDTEntry::new(virtualization_exception_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::ControlProtection as usize] = IDTEntry::new(control_protection_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::HypervisorInjection as usize] = IDTEntry::new(hypervisor_injection_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::VMMCommunication as usize] = IDTEntry::new(vmm_exception_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::Security as usize] = IDTEntry::new(security_exception_handler as *const (), IDTFlags::Exception);

    // PIT IRQs
    IDT[0x20] = IDTEntry::new(irq0_handler as *const (), IDTFlags::External);
    IDT[0x21] = IDTEntry::new(irq1_handler as *const (), IDTFlags::External);
    //IDT[0x22] = IDTEntry::new(irq2_handler as *const (), IDTFlags::External);
    IDT[0x23] = IDTEntry::new(irq3_handler as *const (), IDTFlags::External);
    IDT[0x24] = IDTEntry::new(irq4_handler as *const (), IDTFlags::External);
    IDT[0x25] = IDTEntry::new(irq5_handler as *const (), IDTFlags::External);
    IDT[0x26] = IDTEntry::new(irq6_handler as *const (), IDTFlags::External);
    IDT[0x27] = IDTEntry::new(irq7_handler as *const (), IDTFlags::External);
    IDT[0x28] = IDTEntry::new(irq8_handler as *const (), IDTFlags::External);
    IDT[0x29] = IDTEntry::new(irq9_handler as *const (), IDTFlags::External);
    IDT[0x2a] = IDTEntry::new(irq10_handler as *const (), IDTFlags::External);
    IDT[0x2b] = IDTEntry::new(irq11_handler as *const (), IDTFlags::External);
    IDT[0x2c] = IDTEntry::new(irq12_handler as *const (), IDTFlags::External);
    IDT[0x2d] = IDTEntry::new(irq13_handler as *const (), IDTFlags::External);
    IDT[0x2e] = IDTEntry::new(irq14_handler as *const (), IDTFlags::External);
    IDT[0x2f] = IDTEntry::new(irq15_handler as *const (), IDTFlags::External);

    //IDT[0x80] = IDTEntry::new(syscall_handler_wrapper as *const (), IDTFlags::Call);

    // load interrupt handler table
    lidt(&DescriptorTablePointer::new(&IDT));
}

pub fn init_pit(hz: usize) {
    // init PIT
    let divisor = 1193180 / hz;

    let l = (divisor & 0xff) as u8;
    let h = ((divisor >> 8) & 0xff) as u8;

    unsafe {
        outb(0x43, 0x36);
        outb(0x40, l);
        outb(0x40, h);
    }

    // register timer
    unsafe {
        PIT_TIMER_NUM = crate::timer::register_timer(Some(crate::task::cpu::ThreadID { core: 0, thread: 0 }), hz as u64).expect("couldn't register PIT timer");
    }
}

/// set up IRQ handler list, separate from init() since this requires the heap to be initialized
pub unsafe fn init_irqs() {
    init_pit(1000);
}

#[derive(Debug)]
pub struct InterruptRegisterError;

pub fn register_irq(num: usize, handler: InterruptHandler) -> Result<(), InterruptRegisterError> {
    unsafe {
        // irq 0 is always the timer, which is handled separately
        if num != 0 && IRQ_HANDLERS[num].is_none() {
            IRQ_HANDLERS[num] = Some(handler);
            Ok(())
        } else {
            Err(InterruptRegisterError)
        }
    }
}
