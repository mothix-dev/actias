pub mod gdt;
pub mod interrupts;
pub mod paging;

use super::bsp::ArchProperties;
use crate::mm::ContiguousRegion;
use core::arch::asm;

const SPLIT_ADDR: usize = 0xe0000000;
const HEAP_ADDR: usize = SPLIT_ADDR + 0x01000000;

const PAGE_SIZE: usize = 0x1000;

pub const PROPERTIES: ArchProperties = ArchProperties {
    page_size: PAGE_SIZE,
    userspace_region: ContiguousRegion { base: 0, length: SPLIT_ADDR },
    kernel_region: ContiguousRegion {
        base: SPLIT_ADDR,
        length: usize::MAX - SPLIT_ADDR + 1,
    },
    heap_region: ContiguousRegion { base: HEAP_ADDR, length: 0xffff000 },
    heap_init_size: 0x100000,
    wait_for_interrupt,
    halt,
    enable_interrupts,
    disable_interrupts,
};

/// the physical address size for this architecture
///
/// since PAE is optional and for i686 and up, there's no point in using a full 64 bit pointer when the top 32 bits are irrelevant
pub type PhysicalAddress = u32;

/// the page directory type for this architecture
pub type PageDirectory = paging::PageDir;

/// the interrupt manager for this architecture
pub type InterruptManager = interrupts::IntManager;

pub type StackManager = gdt::GDTState;

fn wait_for_interrupt() {
    unsafe {
        asm!("sti; hlt");
    }
}

fn halt() -> ! {
    loop {
        unsafe {
            asm!("cli; hlt");
        }
    }
}

fn enable_interrupts() {
    unsafe {
        asm!("sti");
    }
}

fn disable_interrupts() {
    unsafe {
        asm!("cli");
    }
}
