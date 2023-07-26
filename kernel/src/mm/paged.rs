use crate::{arch::PhysicalAddress, array::BitSet};
use log::{debug, trace};

/// an error that can be returned from paging operations
pub enum PagingError {
    NoAvailableFrames,
    FrameUnused,
    FrameInUse,
    AllocError,
    BadFrame,
    BadAddress,
}

impl core::fmt::Debug for PagingError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}", match self {
            Self::NoAvailableFrames => "no available frames (out of memory)",
            Self::FrameUnused => "frame is unused",
            Self::FrameInUse => "frame already in use",
            Self::AllocError => "error allocating memory",
            Self::BadFrame => "bad frame",
            Self::BadAddress => "address not mapped",
        })
    }
}

/// struct to keep track of which pages in memory are used and which are available for use
#[repr(C)]
pub struct PageManager {
    /// bitset to speed up allocation of page frames
    ///
    /// every bit in this set represents an individual page in the directory
    ///
    /// the size of this bitset can be calculated by dividing the address of the top of available memory by the system's page size
    pub frame_set: BitSet,

    /// the page size of this page manager
    pub page_size: usize,
}

impl PageManager {
    /// creates a new page manager with the provided bitset for available frames
    ///
    /// # Arguments
    ///
    /// * `frame_set` - a BitSet that stores which pages are available and which aren't.
    /// should be created based on the system's memory map, and should only extend to the limit of writable memory
    pub fn new(frame_set: BitSet, page_size: usize) -> Self {
        Self { frame_set, page_size }
    }

    /// allocates a frame in memory, returning its physical address without assigning it to any page directories
    pub fn alloc_frame(&mut self) -> Result<PhysicalAddress, PagingError> {
        if let Some(idx) = self.frame_set.first_unset() {
            self.frame_set.set(idx);

            Ok(idx as PhysicalAddress * self.page_size as PhysicalAddress)
        } else {
            Err(PagingError::NoAvailableFrames)
        }
    }

    /// gets the first frame available for allocation
    pub fn first_available_frame(&self) -> Option<PhysicalAddress> {
        self.frame_set.first_unset().map(|i| (i as PhysicalAddress) * (self.page_size as PhysicalAddress))
    }

    /// sets a frame in our list of frames as used, preventing it from being allocated elsewhere
    ///
    /// # Arguments
    ///
    /// * `dir` - a page table, used to get page size
    /// * `addr` - the address of the frame
    pub fn set_frame_used(&mut self, addr: PhysicalAddress) {
        assert!(addr % self.page_size as PhysicalAddress == 0, "frame address is not page aligned");

        let idx = (addr / self.page_size as PhysicalAddress).try_into().unwrap();
        trace!("setting {idx:#x} as used");
        self.frame_set.set(idx);

        trace!("first_unset is now {:?}", self.frame_set.first_unset());
    }

    /// sets a frame in our list of frames as free, allowing it to be allocated elsewhere
    ///
    /// # Arguments
    ///
    /// * `dir` - a page table, used to get page size
    /// * `addr` - the address of the frame
    pub fn free_frame(&mut self, addr: PhysicalAddress) {
        assert!(addr % self.page_size as PhysicalAddress == 0, "frame address is not page aligned");

        self.frame_set.clear((addr / self.page_size as PhysicalAddress).try_into().unwrap());
    }

    /// prints out information about this page directory
    pub fn print_free(&self) {
        let bits_used = self.frame_set.bits_used;
        let size = self.frame_set.size;
        debug!(
            "{}/{} mapped ({}k/{}k, {}% usage)",
            bits_used,
            size,
            bits_used * self.page_size / 1024,
            size * self.page_size / 1024,
            (bits_used * 100) / size
        );
    }
}
