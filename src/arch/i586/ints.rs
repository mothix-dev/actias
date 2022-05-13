//! i586 low level interrupt/exception handling

use core::arch::asm;
use core::fmt;
use aligned::{Aligned, A16};
use x86::dtables::{DescriptorTablePointer, lidt};
use bitmask_enum::bitmask;
use crate::console::{TextConsole, SimpleConsole, PANIC_COLOR};
use super::vga::create_console;
use super::halt;

/// IDT flags
#[bitmask(u8)]
enum IDTFlags {
    X16Interrupt    = Self(0x06),
    X16Trap         = Self(0x07),
    X32Task         = Self(0x05),
    X32Interrupt    = Self(0x0e),
    X32Trap         = Self(0x0f),
    Ring1           = Self(0x40),
    Ring2           = Self(0x20),
    Ring3           = Self(0x60),
    Present         = Self(0x80),

    Exception       = Self(Self::X32Interrupt.0 | Self::Present.0), // exception?
    External        = Self(Self::X32Interrupt.0 | Self::Present.0), // external interrupt?
    Call            = Self(Self::X32Interrupt.0 | Self::Present.0 | Self::Ring3.0), // system call?
}

/// entry in IDT
/// this describes an interrupt handler (i.e. where it is, how it works, etc)
#[repr(C, packed(16))]
#[derive(Copy, Clone)]
struct IDTEntry {
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
    fn new(isr: *const (), flags: IDTFlags) -> Self {
        Self {
            isr_low: ((isr as u32) & 0xffff) as u16, // gets address of function pointer, then chops off the top 2 bytes
                                                     // not sure if casting to u16 will only return lower 2 bytes?
            isr_high: ((isr as u32) >> 16) as u16, // upper 2 bytes
            kernel_cs: 0x08, // offset of kernel code selector in GDT (see boot.S)
            attributes: flags.0,
            reserved: 0,
        }
    }

    /// creates an empty IDT entry
    const fn new_empty() -> Self {
        Self { // empty entry
            isr_low: 0,
            kernel_cs: 0,
            reserved: 0,
            attributes: 0,
            isr_high: 0,
        }
    }
}

/// how many entries do we want in our IDT
const IDT_ENTRIES: usize = 256;

/// the IDT itself (aligned to 16 bits for performance)
static mut IDT: Aligned<A16, [IDTEntry; IDT_ENTRIES]> = Aligned([IDTEntry::new_empty(); IDT_ENTRIES]);

/// stores state of cpu prior to running exception handler
/// this should be the proper stack frame format? it isn't provided by the x86 crate to my knowledge
#[repr(C)]
#[derive(Debug)]
pub struct ExceptionStackFrame {
    pub instruction_pointer: u32,
    pub code_segment: u32,
    pub cpu_flags: u32,
    pub stack_pointer: u32,
    pub stack_segment: u32,
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

/// exception handler for breakpoint
unsafe extern "x86-interrupt" fn breakpoint_handler(frame: ExceptionStackFrame) {
    log!("breakpoint @ {:#x}", frame.instruction_pointer);
}

/// exception handler for double fault
unsafe extern "x86-interrupt" fn double_fault_handler(frame: ExceptionStackFrame, _error_code: u32) {
    log!("PANIC: double fault @ {:#x}", frame.instruction_pointer);
    log!("{:#?}", frame);

    // write same message to screen
    let mut raw = create_console();
    let mut console = SimpleConsole::new(&mut raw, 80, 25);

    console.color = PANIC_COLOR;

    console.clear();
    fmt::write(&mut console, format_args!("PANIC: double fault @ {:#x}\n", frame.instruction_pointer)).expect("lol. lmao");
    fmt::write(&mut console, format_args!("{:#?}\n", frame)).expect("lol. lmao");
    //console.puts("owo nowo! ur compuwuter did a fucky wucky uwu");

    halt();
}

/// exception handler for page fault
unsafe extern "x86-interrupt" fn page_fault_handler(frame: ExceptionStackFrame, error_code: PageFaultErrorCode) {
    let mut address: u32;
    asm!("mov {0}, cr2", out(reg) address);

    log!("PANIC: page fault @ {:#x}, error code {}", frame.instruction_pointer, error_code);
    log!("accessed address {:#x}", address);
    log!("{:#?}", frame);

    let mut raw = create_console();
    let mut console = SimpleConsole::new(&mut raw, 80, 25);
    
    console.color = PANIC_COLOR;

    console.clear();
    fmt::write(&mut console, format_args!("PANIC: page fault @ {:#x}, error code {}\n", frame.instruction_pointer, error_code)).expect("lol. lmao");
    fmt::write(&mut console, format_args!("accessed address {:#x}\n", address)).expect("lol. lmao");
    fmt::write(&mut console, format_args!("{:#?}\n", frame)).expect("lol. lmao");

    halt();
}

// todo: more handlers

/// set up idt(r) and enable interrupts
pub unsafe fn init() {
    // set up exception handlers
    IDT[Exceptions::Breakpoint as usize] = IDTEntry::new(breakpoint_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::DoubleFault as usize] = IDTEntry::new(double_fault_handler as *const (), IDTFlags::Exception);
    //IDT[Exceptions::PageFault as usize] = IDTEntry::new(page_fault_handler as *const (), IDTFlags::Exception);
    
    // load interrupt handler table
    let idt_desc = DescriptorTablePointer::new(&IDT);
    lidt(&idt_desc);

    //asm!("sti"); // just in case lidt() doesn't enable interrupts
}
