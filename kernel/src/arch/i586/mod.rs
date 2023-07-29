pub mod paging;

use super::ArchProperties;
use crate::mm::ContiguousRegion;

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
};

/// the physical address size for this architecture
///
/// since PAE is optional and for i686 and up, there's no point in using a full 64 bit pointer when the top 32 bits are irrelevant
pub type PhysicalAddress = u32;

/// the page directory type for this architecture
pub type PageDirectory = paging::PageDir;
