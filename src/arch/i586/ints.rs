//! i586 low level interrupt/exception handling

use core::arch::asm;
use core::fmt;
use aligned::{Aligned, A16};
use x86::dtables::{DescriptorTablePointer, lidt};
use bitmask_enum::bitmask;
use super::halt;
use crate::console::{get_console, PANIC_COLOR};
use crate::tasks::get_current_task_mut;
use super::paging::{PAGE_DIR, PageTableFlags};
use crate::arch::{MEM_TOP, PAGE_SIZE};

#[cfg(test)]
use crate::platform::debug::exit_failure;

/// IDT flags
#[bitmask(u8)]
pub enum IDTFlags {
    X16Interrupt    = Self(0x06),
    X16Trap         = Self(0x07),
    X32Task         = Self(0x05),
    X32Interrupt    = Self(0x0e),
    X32Trap         = Self(0x0f),
    Ring1           = Self(0x40),
    Ring2           = Self(0x20),
    Ring3           = Self(0x60),
    Present         = Self(0x80),

    Exception       = Self(Self::X32Interrupt.0 | Self::Present.0), // exception
    External        = Self(Self::X32Interrupt.0 | Self::Present.0), // external interrupt
    Call            = Self(Self::X32Interrupt.0 | Self::Present.0 | Self::Ring3.0), // system call
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
pub const IDT_ENTRIES: usize = 256;

/// the IDT itself (aligned to 16 bits for performance)
pub static mut IDT: Aligned<A16, [IDTEntry; IDT_ENTRIES]> = Aligned([IDTEntry::new_empty(); IDT_ENTRIES]);

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
    if let Some(console) = get_console() {
        console.set_color(PANIC_COLOR);
    }

    log!("PANIC: double fault @ {:#x}", frame.instruction_pointer);
    log!("{:#?}", frame);

    #[cfg(test)]
    exit_failure();

    halt();
}

extern "C" {
    fn copy_page_physical(src: u32, dest: u32);
}

/// exception handler for page fault
unsafe extern "x86-interrupt" fn page_fault_handler(frame: ExceptionStackFrame, error_code: PageFaultErrorCode) {
    let mut address: u32;
    asm!("mov {0}, cr2", out(reg) address);

    let dir = unsafe { PAGE_DIR.as_mut().unwrap() };

    // switch to kernel's page directory
    dir.switch_to();

    // rust moment
    if 
        // is there a current task?
        if let Some(current) = get_current_task_mut() {
            // get current task's page entry for given address
            if let Some(page) = current.state.pages.get_page(address, false) {
                let page = &mut *page;

                // get flags
                let flags: PageTableFlags = page.get_flags().into();

                // is read/write flag unset and copy on write flag set?
                if u16::from(flags & PageTableFlags::ReadWrite) == 0 && u16::from(flags & PageTableFlags::CopyOnWrite) > 0 {
                    #[cfg(debug_messages)]
                    log!("copy on write");

                    // get physical address of page
                    let old_addr = page.get_address();

                    // set page as unused so we can get a new frame
                    page.set_unused();

                    if let Some(addr) = dir.alloc_frame(page, false, true) {
                        #[cfg(debug_messages)]
                        log!("copying");

                        // temporarily map the page we want to copy from and the page we want to copy to into memory
                        let from_virt = MEM_TOP - PAGE_SIZE * 2 + 1;
                        let to_virt = MEM_TOP - PAGE_SIZE + 1;

                        let page_from = &mut *dir.get_page(from_virt as u32, true).expect("can't get page");
                        page_from.set_flags(PageTableFlags::Present | PageTableFlags::ReadWrite);
                        page_from.set_address(old_addr);
                        asm!("invlpg [{0}]", in(reg) old_addr);

                        let page_to = &mut *dir.get_page(to_virt as u32, true).expect("can't get page");
                        page_to.set_flags(PageTableFlags::Present | PageTableFlags::ReadWrite);
                        page_to.set_address(addr);
                        asm!("invlpg [{0}]", in(reg) addr);

                        // pointer shenanigans to get buffers we can copy
                        let from_buf = &mut *(from_virt as *mut [u32; 256]);
                        let to_buf = &mut *(to_virt as *mut [u32; 256]);

                        // do the copy
                        to_buf.copy_from_slice(from_buf);

                        // set our temporary pages as unused so the data can't be accessed elsewhere
                        page_from.set_unused();
                        page_to.set_unused();

                        // switch back to task's page directory
                        current.state.pages.switch_to();

                        #[cfg(debug_messages)]
                        log!("copied {:#x} -> {:#x}", old_addr, addr);
                        
                        false // don't panic
                    } else {
                        true // panic
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
    {
        if let Some(console) = get_console() {
            console.set_color(PANIC_COLOR);
        }

        log!("PANIC: page fault @ {:#x}, error code {}", frame.instruction_pointer, error_code);
        log!("accessed address {:#x}", address);
        log!("{:#?}", frame);
        
        #[cfg(test)]
        exit_failure();

        halt();
    }
}

/// exception handler for general protection fault
unsafe extern "x86-interrupt" fn general_protection_fault_handler(frame: ExceptionStackFrame, error_code: u32) {
    if let Some(console) = get_console() {
        console.set_color(PANIC_COLOR);
    }

    log!("PANIC: general protection fault @ {:#x}, error code {:#x}", frame.instruction_pointer, error_code);
    log!("{:#?}", frame);

    let mut address: u32;
    asm!("mov {0}, esp", out(reg) address);

    log!("esp: {:#x} ({})", address, address);
    
    #[cfg(test)]
    exit_failure();

    halt();
}

unsafe extern "x86-interrupt" fn test_handler(_frame: ExceptionStackFrame) {
    log!("UwU");
}

/// structure of registers saved in the syscall handler
#[repr(C, packed(32))]
#[derive(Debug, Copy, Clone)]
pub struct SyscallRegisters {
    pub ds: u32,
    pub edi: u32,
    pub esi: u32,
    pub ebp: u32,
    pub esp: u32,
    pub ebx: u32,
    pub edx: u32,
    pub ecx: u32,
    pub eax: u32,
    pub eip: u32,
    pub cs: u32,
    pub eflags: u32,
    pub useresp: u32,
    pub ss: u32,
}

impl Default for SyscallRegisters {
    fn default() -> Self {
        Self {
            ds: 0,
            edi: 0,
            esi: 0,
            ebp: 0,
            esp: 0,
            ebx: 0,
            edx: 0,
            ecx: 0,
            eax: 0,
            eip: 0,
            cs: 0,
            eflags: 0,
            useresp: 0,
            ss: 0,
        }
    }
}

extern "C" {
    /// wrapper around syscall_handler to save and restore state
    fn syscall_handler_wrapper() -> !;
}

// todo: more handlers

/// set up idt(r) and enable interrupts
pub unsafe fn init() {
    // set up exception handlers
    IDT[Exceptions::Breakpoint as usize] = IDTEntry::new(breakpoint_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::DoubleFault as usize] = IDTEntry::new(double_fault_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::PageFault as usize] = IDTEntry::new(page_fault_handler as *const (), IDTFlags::Exception);
    IDT[Exceptions::GeneralProtectionFault as usize] = IDTEntry::new(general_protection_fault_handler as *const (), IDTFlags::Exception);

    IDT[0x80] = IDTEntry::new(syscall_handler_wrapper as *const (), IDTFlags::Call);

    // init irq handlers
    crate::platform::irq::init();
    
    // load interrupt handler table
    let idt_desc = DescriptorTablePointer::new(&IDT);
    lidt(&idt_desc);

    //asm!("sti"); // just in case lidt() doesn't enable interrupts
}
