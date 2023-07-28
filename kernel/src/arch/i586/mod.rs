pub mod paging;

use super::{ArchProperties, ContiguousRegion};

const SPLIT_ADDR: usize = 0xe0000000;

const PAGE_SIZE: usize = 0x1000;

pub const PROPERTIES: ArchProperties = ArchProperties {
    page_size: PAGE_SIZE,
    userspace_region: ContiguousRegion::new(0, SPLIT_ADDR),
    kernel_region: ContiguousRegion::new(SPLIT_ADDR, usize::MAX),
};

/// the physical address size for this architecture
///
/// since PAE is optional and for i686 and up, there's no point in using a full 64 bit pointer when the top 32 bits are irrelevant
pub type PhysicalAddress = u32;
