use core::{alloc::Layout, ptr::NonNull};

use crate::{arch::PhysicalAddress, array::BitSet};
use alloc::{
    alloc::{alloc, dealloc},
    collections::BTreeMap,
    sync::{Arc, Weak},
    vec,
    vec::Vec,
};
use common::Errno;
use log::{debug, error, trace};
use spin::Mutex;

/// an error that can be returned from paging operations
pub enum PagingError {
    NoAvailableFrames,
    AllocError,
    BadFrame,
    BadAddress,
    Invalid,
    IOError,
}

impl core::fmt::Debug for PagingError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}", match self {
            Self::NoAvailableFrames => "no available frames (out of memory)",
            Self::AllocError => "error allocating memory",
            Self::BadFrame => "bad frame",
            Self::BadAddress => "address not mapped",
            Self::Invalid => "invalid request",
            Self::IOError => "input/output error",
        })
    }
}

impl From<PagingError> for Errno {
    fn from(value: PagingError) -> Self {
        match value {
            PagingError::NoAvailableFrames | PagingError::AllocError => Errno::OutOfMemory,
            PagingError::Invalid | PagingError::BadFrame => Errno::InvalidArgument,
            PagingError::BadAddress => Errno::BadAddress,
            PagingError::IOError => Errno::IOError,
        }
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
    page_size: usize,

    /// how many pages in this page manager are reserved and inaccessible
    pub num_reserved: usize,

    /// stores references to page frames to allow for them to be shared and mapped out
    frame_references: BTreeMap<PhysicalAddress, Vec<FrameReference>>,
}

impl PageManager {
    /// creates a new page manager with the provided bitset for available frames
    ///
    /// # Arguments
    /// * `frame_set` - a BitSet that stores which pages are available and which aren't.
    /// should be created based on the system's memory map, and should only extend to the limit of writable memory
    pub fn new(frame_set: BitSet, page_size: usize) -> Self {
        Self {
            frame_set,
            page_size,
            num_reserved: 0,
            frame_references: BTreeMap::new(),
        }
    }

    /// allocates a frame in memory, returning its physical address without assigning it to any page directories
    ///
    /// # Arguments
    /// * `reference` - an optional reference to assign to the page upon allocation
    pub fn alloc_frame(&mut self, reference: Option<FrameReference>) -> Result<PhysicalAddress, PagingError> {
        if let Some(idx) = self.frame_set.first_unset() {
            self.frame_set.set(idx);

            let addr = idx as PhysicalAddress * self.page_size as PhysicalAddress;
            if let Some(reference) = reference {
                self.add_reference(addr, reference);
            }

            Ok(addr)
        } else {
            Err(PagingError::NoAvailableFrames)
        }
    }

    /// gets the first frame available for allocation
    pub fn first_available_frame(&self) -> Option<PhysicalAddress> {
        self.frame_set.first_unset().map(|i| (i as PhysicalAddress) * (self.page_size as PhysicalAddress))
    }

    /// sets a frame in the list of frames as used, preventing it from being allocated elsewhere
    ///
    /// # Arguments
    /// * `addr` - the physical address of the frame
    pub fn set_frame_used(&mut self, addr: PhysicalAddress) {
        assert!(addr % self.page_size as PhysicalAddress == 0, "frame address is not page aligned");

        let idx = (addr / self.page_size as PhysicalAddress).try_into().unwrap();
        trace!("setting {idx:#x} as used");
        self.frame_set.set(idx);

        trace!("first_unset is now {:?}", self.frame_set.first_unset());
    }

    /// sets a frame in the list of frames as free if it has no more references, allowing it to be allocated elsewhere
    ///
    /// # Arguments
    /// * `addr` - the physical address of the frame
    /// * `map` - a reference to which memory map references this frame, if any
    pub fn free_frame(&mut self, addr: PhysicalAddress, map: Option<&Mutex<super::ProcessMap>>) {
        assert!(addr % self.page_size as PhysicalAddress == 0, "frame address is not page aligned");
        let mut should_free = true;

        if let Some(list) = self.frame_references.get_mut(&addr) {
            if let Some(map) = map {
                // remove any references that match the given page directory, and clean up any dangling references at the same time
                list.retain(|r| !r.map.upgrade().map(|a| Arc::as_ptr(&a) == map as *const _).unwrap_or_default());
            }
            should_free = list.is_empty();
        }

        if should_free {
            self.frame_set.clear((addr / self.page_size as PhysicalAddress).try_into().unwrap());
        }
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

    /// adds the given reference to the reference list for the given page
    ///
    /// # Arguments
    /// * `addr` - the physical address of the frame
    /// * `reference` - information about what memory map holds the reference to the frame and where it's mapped in that memory map
    pub fn add_reference(&mut self, addr: PhysicalAddress, reference: FrameReference) {
        if let Some(list) = self.frame_references.get_mut(&addr) {
            list.push(reference);
        } else {
            self.frame_references.insert(addr, vec![reference]);
        }
    }
}

/// stores information for a reference to a page frame
pub struct FrameReference {
    /// the process map referencing this page frame
    pub map: Weak<Mutex<super::ProcessMap>>,

    /// the virtual address this page frame is mapped at
    pub addr: usize,
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
            .finish()
    }
}

/// callback used when allocating reserved memory, wrapper around internal allocator state
pub trait AllocCallback = FnMut(core::alloc::Layout) -> Result<NonNull<u8>, super::HeapAllocError>;

/// stores allocations necessary for inserting all necessary new page table levels for a worst case (i.e. requires the most possible new table level allocations)
/// page insertion in kernel space. this type is *not* allocated itself, instead it must store allocations and free them when dropped.
///
/// this allocated memory is then used in the case when the kernel heap must be expanded, but inserting the newly allocated pages requires allocating more memory for
/// new page table levels than there is space in the heap.
/// to solve this problem, enough memory is reserved to handle inserting a new page without requiring any heap allocations (since that would cause a deadlock!).
///
/// if any allocations are made in types implementing this trait, they must be freed when it is dropped in order to prevent memory leaks.
pub trait ReservedMemory {
    /// creates a new instance of this type and allocates memory for it
    fn allocate<F: AllocCallback>(alloc: F) -> Result<Self, PagingError>
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

    /// a type that's used to store a raw representation of the kernel's area in a page directory
    type RawKernelArea: ?Sized;

    /* -= Required functions -= */

    /// creates a new instance of this page directory, allocating any necessary memory for it in the process
    ///
    /// # Arguments
    /// * `current_dir` - the currently active page directory, used for virtual to physical address translations
    fn new(current_dir: &impl PageDirectory) -> Result<Self, PagingError>
    where Self: Sized;

    /// given a virtual address, gets the page that contains it from this directory in a hardware agnostic form
    fn get_page(&self, addr: usize) -> Option<PageFrame>;

    /// inserts a page frame into the directory. allocation failures when inserting frames must be handled properly and returned, as things will break otherwise.
    ///
    /// # Arguments
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
    /// * `current_dir` - the currently active page directory, used for virtual to physical address translations.
    /// if this is None then address translations must be done in the page directory this is called on
    /// * `addr` - the virtual address to insert the page frame at
    /// * `page` - the page frame to insert
    /// * `reserved_memory` - the region of memory reserved for storing any allocations that may need to happen while inserting a page
    fn set_page_no_alloc(&mut self, current_dir: Option<&impl PageDirectory>, addr: usize, page: Option<PageFrame>, reserved_memory: Option<Self::Reserved>) -> Result<(), PagingError>;

    /// switch the mmu to this page directory
    ///
    /// # Safety
    /// this function is unsafe since whatever code is being run currently could be different or nonexistent when switching pages, thus causing undefined behavior.
    /// also, care must be taken to ensure that this page directory isn't dropped while active to prevent use-after-free.
    unsafe fn switch_to(&self);

    /// when ran on the current page directory, flushes the entry for the given page in the TLB (or equivalent).
    /// when ran on any page directory other than the current one, the behavior is undefined
    fn flush_page(addr: usize);

    /// gets the raw kernel area from this page directory
    fn get_raw_kernel_area(&self) -> &Self::RawKernelArea;

    /// sets the raw kernel area of this page directory to the one given.
    ///
    /// # Safety
    /// once the raw kernel area is modified in a page directory, the behavior of any `get_page()` or `set_page()` calls in the kernel area of that page directory are undefined
    unsafe fn set_raw_kernel_area(&mut self, area: &Self::RawKernelArea);

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

/// maps the given physical addresses in order into a region of memory allocated on the heap, then calls `op` with a slice over all the mapped memory
///
/// # Arguments
/// * `addresses` - a list of physical addresses to map into memory in order
/// * `op` - function to be called while memory is mapped
///
/// # Safety
/// this function is unsafe because it (at least in its default implementation) cannot guarantee that it's being called on the current
/// page directory, and things can and will break if it's called on any other page directory
pub unsafe fn map_memory<D: PageDirectory, O, R>(map_into: &mut D, addresses: &[PhysicalAddress], op: O) -> Result<R, PagingError>
where O: FnOnce(&mut [u8]) -> R {
    let page_size = D::PAGE_SIZE;

    let buf_len = addresses.len() * page_size;

    // allocate memory for us to remap
    let layout = Layout::from_size_align(buf_len, page_size).unwrap();
    let ptr = alloc(layout);

    if ptr.is_null() {
        error!("error allocating buffer in map_memory()");
        return Err(PagingError::AllocError);
    }

    assert!(ptr as usize % page_size == 0); // make absolutely sure pointer is page aligned

    debug!("mapping {} pages to {:#x} (kernel mem)", addresses.len(), ptr as usize);

    // get addresses of pages we're gonna remap so we can map them back later
    let mut existing_phys = Vec::new();

    // attempt to safely reserve memory for our mapping
    if let Err(err) = existing_phys.try_reserve_exact(addresses.len()) {
        error!("error reserving memory in map_memory(): {err:?}");
        dealloc(ptr, layout);

        return Err(PagingError::AllocError);
    }

    for i in (ptr as usize..ptr as usize + buf_len).step_by(page_size) {
        // virt to phys calculation from current page directory
        let addr = match map_into.virt_to_phys(i) {
            Some(a) => a,
            None => {
                // something bad happened, revert back to original state and return an error
                debug!("aborting map (before remap), dealloc()ing");
                dealloc(ptr, layout);

                return Err(PagingError::BadAddress);
            }
        };
        trace!("existing: {i:#x} -> {addr:#x}");
        existing_phys.push(addr);
    }

    trace!("existing_phys: {existing_phys:x?}");

    // remap all pages in region
    for (i, phys_addr) in addresses.iter().enumerate() {
        let virt = ptr as usize + i * page_size;

        trace!("{virt:x} now @ phys addr: {phys_addr:x}");

        // todo: maybe change this to debug_assert at some point? its prolly hella slow
        assert!(!existing_phys.contains(phys_addr), "trampling on other page directory's memory");

        // remap memory
        map_into
            .set_page(
                None::<&D>,
                virt,
                Some(PageFrame {
                    addr: *phys_addr,
                    present: true,
                    writable: true,
                    ..Default::default()
                }),
            )
            .expect("couldn't remap page");

        D::flush_page(virt);
    }

    trace!("slice @ {ptr:?}, len {buf_len:#x}");

    // call function
    let res = op(core::slice::from_raw_parts_mut(ptr, buf_len));

    // map pages back to their original addresses
    trace!("cleaning up mapping");
    for (idx, addr) in (ptr as usize..ptr as usize + buf_len).step_by(page_size).enumerate() {
        let phys_addr = existing_phys[idx];
        trace!("virt @ {addr:x}, phys @ {phys_addr:x}");

        map_into
            .set_page(
                None::<&D>,
                addr,
                Some(PageFrame {
                    addr: phys_addr,
                    present: true,
                    writable: true,
                    ..Default::default()
                }),
            )
            .expect("couldn't remap page");
        D::flush_page(addr);
    }

    // deallocate the buffer
    dealloc(ptr, layout);

    Ok(res)
}
