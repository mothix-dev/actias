// i586 low level interrupt/exception handling

use core::arch::asm;
use aligned::{Aligned, A16};
use x86::dtables::{DescriptorTablePointer, lidt};
use bitmask_enum::bitmask;

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

/// exception handler for interrupt 3 (breakpoint)
unsafe extern "x86-interrupt" fn breakpoint_handler(frame: ExceptionStackFrame) {
    log!("cpu exception 3 (breakpoint) @ {:#x}", frame.instruction_pointer);
}

/// exception handler for interrupt 8 (double fault)
unsafe extern "x86-interrupt" fn double_fault_handler(frame: ExceptionStackFrame, error_code: u32) {
    log!("PANIC: cpu exception 8 (double fault) @ {:#x}, error code {:#x}", frame.instruction_pointer, error_code);
    log!("{:#?}", frame);
    log!("halting");
    asm!("cli; hlt"); // clear interrupts, halt
}

/// set up idt(r) and enable interrupts
pub unsafe fn init() {
    // set up exception handlers
    IDT[3] = IDTEntry::new(breakpoint_handler as *const (), IDTFlags::Exception);
    IDT[8] = IDTEntry::new(double_fault_handler as *const (), IDTFlags::Exception);
    
    // load interrupt handler table
    let idt_desc = DescriptorTablePointer::new(&IDT);
    lidt(&idt_desc);

    asm!("sti"); // just in case lidt() doesn't enable interrupts
}
