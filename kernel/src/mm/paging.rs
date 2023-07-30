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
    /// bitset to speed up allocation of page frames, where every bit in this set represents an individual page in the directory.
    ///
    /// the size of this bitset can be calculated by dividing the address of the top of available memory by the system's page size.
    pub frame_set: BitSet,

    /// the page size of this page manager
    pub page_size: usize,

    /// how many pages in this page manager are reserved and inaccessible
    pub num_reserved: usize,
}

impl PageManager {
    /// creates a new page manager with the provided bitset for available frames
    ///
    /// # Arguments
    ///
    /// * `frame_set` - a BitSet that stores which pages are available and which aren't.
    /// should be created based on the system's memory map, and should only extend to the limit of writable memory
    pub fn new(frame_set: BitSet, page_size: usize) -> Self {
        Self {
            frame_set,
            page_size,
            num_reserved: 0,
        }
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
        let bits_used = self.frame_set.bits_used - self.num_reserved;
        let size = self.frame_set.size - self.num_reserved;
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

/// hardware agnostic form of a page frame
#[derive(Default, Copy, Clone)]
pub struct PageFrame {
    /// physical address of this page frame
    ///
    /// this determines where in physical memory this page will map to
    pub addr: PhysicalAddress,

    /// whether this page is present in memory and can be accessed
    ///
    /// can be used to swap pages out of memory and reload them when accessed
    pub present: bool,

    /// whether this frame can be accessed in user mode (ring 3)
    pub user_mode: bool,

    /// whether this frame can be written to
    pub writable: bool,

    /// whether code can be executed from this page. not supported on all platforms
    pub executable: bool,

    /// whether this page should be copied upon attempting to write to it (requires writable flag to be disabled)
    pub copy_on_write: bool,

    /// whether this page has more than one reference and its freeing should be handled by the reference counter
    pub referenced: bool,
}

impl core::fmt::Debug for PageFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PageFrame")
            .field("addr", &crate::FormatHex(self.addr))
            .field("present", &self.present)
            .field("user_mode", &self.user_mode)
            .field("writable", &self.writable)
            .field("executable", &self.executable)
            .field("copy_on_write", &self.copy_on_write)
            .field("referenced", &self.referenced)
            .finish()
    }
}

/// stores allocations necessary for inserting all necessary new page table levels for a worst case (i.e. requires the most possible new table level allocations)
/// page insertion in kernel space. this type is *not* allocated itself, instead it must store allocations and free them when dropped
///
/// this allocated memory is then used in the case when the kernel heap must be expanded, but inserting the newly allocated pages requires allocating more memory for
/// new page table levels than there is space in the heap.
/// to solve this problem, enough memory is reserved to handle inserting a new page without requiring any heap allocations (since that would cause a deadlock!).
///
/// if any allocations are made in types implementing this trait, they must be freed when it is dropped in order to prevent memory leaks.
pub trait ReservedMemory {
    /// creates a new instance of this type and allocates memory for it
    fn allocate() -> Result<Self, PagingError>
    where Self: Sized;
    /// gets a layout which will encompass all allocations made by allocate()
    fn layout() -> core::alloc::Layout;
}

/// safe abstraction layer for page directories. allows a consistent interface to page directories of multiple architectures
pub trait PageDirectory {
    /// the size of each individual page in this page directory in bytes
    const PAGE_SIZE: usize;

    /// a type storing references to any memory allocations that could be made while inserting a page into this page directory.
    /// see the `ReservedMemory` trait for more details.
    type Reserved: ReservedMemory;

    /* -= Required functions -= */

    /// creates a new instance of this page directory, allocating any necessary memory for it in the process
    ///
    /// # Arguments
    ///
    /// * `current_dir` - the currently active page directory, used for virtual to physical address translations
    fn new(current_dir: &impl PageDirectory) -> Result<Self, PagingError>
    where Self: Sized;

    /// given a virtual address, gets the page that contains it from this directory in a hardware agnostic form
    fn get_page(&self, addr: usize) -> Option<PageFrame>;

    /// inserts a page frame into the directory. allocation failures when inserting frames must be handled properly and returned, as things will break otherwise.
    ///
    /// # Arguments
    ///
    /// * `current_dir` - the currently active page directory, used for virtual to physical address translations.
    /// if this is None then address translations must be done in the page directory this is called on
    /// * `addr` - the virtual address to insert the page frame at
    /// * `page` - the page frame to insert
    fn set_page(&mut self, current_dir: Option<&impl PageDirectory>, addr: usize, page: Option<PageFrame>) -> Result<(), PagingError>;

    /// inserts a page frame into this page directory **without allocating memory**, using the reserved memory type given as a substitute for any allocations that may be
    /// made.
    ///
    /// if reserved_memory is `None` and allocations are required to insert a page, `Err(PagingError::AllocError)`
    /// must be returned as allocating could cause a deadlock.
    ///
    /// # Arguments
    ///
    /// * `current_dir` - the currently active page directory, used for virtual to physical address translations.
    /// if this is None then address translations must be done in the page directory this is called on
    /// * `addr` - the virtual address to insert the page frame at
    /// * `page` - the page frame to insert
    /// * `reserved_memory` - the region of memory reserved for storing any allocations that may need to happen while inserting a page
    fn set_page_no_alloc(&mut self, current_dir: Option<&impl PageDirectory>, addr: usize, page: Option<PageFrame>, reserved_memory: Option<Self::Reserved>) -> Result<(), PagingError>;

    /// switch the mmu to this page directory
    ///
    /// # Safety
    ///
    /// this function is unsafe since whatever code is being run currently could be different or nonexistent when switching pages, thus causing undefined behavior.
    /// also, care must be taken to ensure that this page directory isn't dropped while active to prevent use-after-free.
    unsafe fn switch_to(&self);

    /// when ran on the current page directory, flushes the entry for the given page in the TLB (or equivalent).
    /// when ran on any page directory other than the current one, the behavior is undefined
    fn flush_page(addr: usize);

    /* -= Non required functions =- */

    /// given an address, checks whether the page that contains it is unused and can be freely remapped
    fn is_unused(&self, addr: usize) -> bool {
        self.get_page(addr).is_none()
    }

    /// transforms the provided virtual address in this page directory into a physical address, if possible
    fn virt_to_phys(&self, virt: usize) -> Option<PhysicalAddress> {
        let page_size = Self::PAGE_SIZE - 1;
        let page_addr = virt & !page_size;
        let offset = virt & page_size;

        self.get_page(page_addr).map(|page| page.addr | offset as PhysicalAddress)
    }
}
