//! paging abstraction layer

use crate::util::{array::BitSet, debug::FormatHex};
//use common::types::errno::Errno;
use alloc::{
    alloc::{alloc, dealloc, Layout},
    vec::Vec,
};
use core::fmt;
use log::{debug, error, trace};
use spin::{Mutex, MutexGuard};

/// an error that can be returned from paging operations
pub enum PagingError {
    NoAvailableFrames,
    FrameUnused,
    FrameInUse,
    AllocError,
    BadFrame,
    BadAddress,
}

impl fmt::Display for PagingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
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

impl fmt::Debug for PagingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PagingError: \"{self}\"")
    }
}

/// hardware agnostic form of a page frame
#[derive(Copy, Clone)]
pub struct PageFrame {
    /// physical address of this page frame
    ///
    /// this determines where in physical memory this page will map to
    pub addr: u64,

    /// whether this page is present in memory and can be accessed
    ///
    /// can be used to swap pages out of memory and reload them when accessed
    pub present: bool,

    /// whether this frame can be accessed in user mode (ring 3)
    pub user_mode: bool,

    /// whether this frame can be written to
    pub writable: bool,

    /// whether this page should be copied upon attempting to write to it (requires writable flag to be disabled)
    pub copy_on_write: bool,
}

impl fmt::Debug for PageFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageFrame")
            .field("addr", &FormatHex(self.addr))
            .field("present", &self.present)
            .field("user_mode", &self.user_mode)
            .field("writable", &self.writable)
            .field("copy_on_write", &self.copy_on_write)
            .finish()
    }
}

/// safe abstraction layer for page directories. allows a consistent interface to page directories of multiple architectures
pub trait PageDirectory {
    const PAGE_SIZE: usize;

    /* -= Required functions -= */

    /// given a virtual address, get the page that contains it from this directory in a hardware agnostic form
    fn get_page(&self, addr: usize) -> Option<PageFrame>;

    /// insert a page frame into the directory
    ///
    /// # Arguments
    ///
    /// * `addr` - the virtual address to insert the page frame at
    /// * `page` - the page frame to insert
    fn set_page(&mut self, addr: usize, page: Option<PageFrame>) -> Result<(), PagingError>;

    /// switch the mmu to this page directory
    ///
    /// # Safety
    ///
    /// this function is unsafe since whatever code is being run currently could be different or nonexistent when switching pages, thus causing undefined behavior
    unsafe fn switch_to(&self);

    /* -= Non required functions =- */

    /// given an address, checks whether the page that contains it is unused and can be freely remapped
    fn is_unused(&self, addr: usize) -> bool {
        self.get_page(addr).is_none()
    }

    /// copy a certain amount of pages from the given page directory to this one
    ///
    /// # Arguments
    ///
    /// * `dir` - the PageDirectory to copy pages from
    /// * `from` - the starting index in the page directory to be copied from (index here means an address divided by the system's page size, i.e. 4 KiB on x86)
    /// * `to` - the starting index in the page directory we're copying to
    /// * `num` - the number of pages to copy
    fn copy_from(&mut self, dir: &mut impl PageDirectory, from: usize, to: usize, num: usize) -> Result<(), PagingError> {
        let page_size = Self::PAGE_SIZE;

        // just iterate over all pages in the range provided and copy them
        for i in (0..(num * page_size)).step_by(page_size) {
            self.set_page(to + i, dir.get_page(from + i))?;
        }

        Ok(())
    }

    /// copy a certain amount of pages from the given page directory to this one and set them as copy-on-write
    ///
    /// # Arguments
    ///
    /// * `dir` - the PageDirectory to copy pages from
    /// * `from` - the starting index in the page directory to be copied from (index here means an address divided by the system's page size, i.e. 4 KiB on x86)
    /// * `to` - the starting index in the page directory we're copying to
    /// * `num` - the number of pages to copy
    fn copy_on_write_from(&mut self, dir: &mut impl PageDirectory, from: usize, to: usize, num: usize) -> Result<(), PagingError> {
        let page_size = Self::PAGE_SIZE;

        for i in (0..(num * page_size)).step_by(page_size) {
            let mut page = dir.get_page(from + i);

            // does this page exist?
            if let Some(page) = page.as_mut() {
                // if this page is writable, set it as non-writable and set it to copy on write
                //
                // pages have to be set as non writable in order for copy on write to work since attempting to write to a non writable page causes a page fault exception,
                // which we can then use to copy the page and resume execution as normal
                if page.writable {
                    page.writable = false;
                    page.copy_on_write = true;
                }
            }

            self.set_page(to + i, page)?;
        }

        Ok(())
    }

    /// transforms the provided virtual address in this page directory into a physical address, if possible
    fn virt_to_phys(&self, virt: usize) -> Option<u64> {
        let page_size = Self::PAGE_SIZE - 1;
        let page_addr = virt & !page_size;
        let offset = virt & page_size;

        self.get_page(page_addr).map(|page| page.addr | offset as u64)
    }

    /// when run on the current page directory, this function maps the range `addr..addr + len` from the page table given in `from`
    /// to a region on the heap, then calls `op` with a reference to a slice of the mapped region. the region on the heap is then deallocated.
    /// this function does not allocate new pages in the given page directory, and attempting to run it on a region which is not fully allocated
    /// will return an error
    ///
    /// # Arguments
    ///
    /// * `from` - the page directory to map memory from. must be the same type as the one that this function is being called on
    /// * `addr` - the starting address to map memory from
    /// * `len` - how much memory to map, in bytes
    /// * `op` - function to be called while memory is mapped
    ///
    /// # Safety
    ///
    /// this function is unsafe because it (at least in its default implementation) cannot guarantee that it's being called on the current
    /// page directory, and things can and will break if it's called on any other page directory
    unsafe fn map_memory_from<O, R>(&mut self, from: &mut impl PageDirectory, addr: usize, len: usize, op: O) -> Result<R, PagingError>
    where O: FnOnce(&mut [u8]) -> R {
        let page_size = Self::PAGE_SIZE;

        // get starting and ending addresses
        let mut start = addr;
        let mut end = addr + len;

        assert!(end > start);

        debug!("mapping partial page directory in");
        debug!("start @ {start:#x}, end @ {end:#x}");

        // offset into memory we've paged in
        let mut offset = 0;

        // align start and end addresses to page boundaries
        if start % page_size != 0 {
            start &= !(page_size - 1);
            offset = addr - start;
        }

        if end % page_size != 0 {
            end = (end & !(page_size - 1)) + (page_size - 1);
        }

        debug!("start now @ {start:#x}, end now @ {end:#x}");
        debug!("offset is {offset:#x}, len is {len:#x}");

        let mut addresses: Vec<u64> = Vec::new();

        // attempt to safely reserve memory
        if let Err(err) = addresses.try_reserve_exact((end - start) / page_size) {
            error!("error reserving memory in map_memory_from(): {err:?}");

            return Err(PagingError::AllocError);
        }

        // get physical addresses of this region
        for i in (start..=end).step_by(page_size) {
            let phys_addr = match from.virt_to_phys(i) {
                Some(a) => a,
                None => {
                    debug!("couldn't get phys addr for virt {i:#x}");

                    return Err(PagingError::BadAddress);
                }
            };

            addresses.push(phys_addr);
        }

        debug!("addresses: {addresses:x?}");

        // map the memory
        self.map_memory(&addresses, |s| op(&mut s[offset..offset + len]))
    }

    /// maps the given physical addresses in order into a region of memory allocated on the heap, then calls `op` with a slice over all the mapped memory
    ///
    /// # Arguments
    ///
    /// * `addresses` - a list of physical addresses to map into memory in order
    /// * `op` - function to be called while memory is mapped
    ///
    /// # Safety
    ///
    /// this function is unsafe because it (at least in its default implementation) cannot guarantee that it's being called on the current
    /// page directory, and things can and will break if it's called on any other page directory
    unsafe fn map_memory<O, R>(&mut self, addresses: &[u64], op: O) -> Result<R, PagingError>
    where O: FnOnce(&mut [u8]) -> R {
        let page_size = Self::PAGE_SIZE;

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
        let mut existing_phys: Vec<u64> = Vec::new();

        // attempt to safely reserve memory for our mapping
        if let Err(err) = existing_phys.try_reserve_exact(addresses.len()) {
            error!("error reserving memory in map_memory(): {err:?}");
            dealloc(ptr, layout);

            return Err(PagingError::AllocError);
        }

        for i in (ptr as usize..ptr as usize + buf_len).step_by(page_size) {
            // virt to phys calculation from current page directory
            let addr = match self.virt_to_phys(i) {
                Some(a) => a,
                None => {
                    // something bad happened, revert back to original state and return an error
                    debug!("aborting map (before remap), dealloc()ing");
                    dealloc(ptr, layout);

                    return Err(PagingError::BadAddress);
                }
            };
            debug!("existing: {i:#x} -> {addr:#x}");
            existing_phys.push(addr);
        }

        debug!("existing_phys: {existing_phys:x?}");

        // remap all pages in region
        for (i, phys_addr) in addresses.iter().enumerate() {
            let virt = ptr as usize + i * page_size;

            debug!("{virt:x} now @ phys addr: {phys_addr:x}");

            // todo: maybe change this to debug_assert at some point? its prolly hella slow
            assert!(!existing_phys.contains(phys_addr), "trampling on other page directory's memory");

            // remap memory
            self.set_page(
                virt,
                Some(PageFrame {
                    addr: *phys_addr,
                    present: true,
                    user_mode: false,
                    writable: true,
                    copy_on_write: false,
                }),
            )
            .expect("couldn't remap page");
        }

        debug!("slice @ {ptr:?}, len {buf_len:#x}");

        // call function
        let res = op(core::slice::from_raw_parts_mut(ptr as *mut u8, buf_len));

        // map pages back to their original addresses
        debug!("cleaning up mapping");
        for (idx, addr) in (ptr as usize..ptr as usize + buf_len).step_by(page_size).enumerate() {
            let phys_addr = existing_phys[idx];
            debug!("virt @ {addr:x}, phys @ {phys_addr:x}");
            self.set_page(
                addr,
                Some(PageFrame {
                    addr: phys_addr,
                    present: true,
                    user_mode: false,
                    writable: true,
                    copy_on_write: false,
                }),
            )
            .expect("couldn't remap page");
        }

        // deallocate the buffer
        dealloc(ptr, layout);

        Ok(res)
    }

    /// finds available area in this page directory's memory of given size. this area is guaranteed to be unused, unallocated, and aligned to a page boundary
    ///
    /// # Arguments
    ///
    /// * `start` - the lowest address this hole can be located at. useful to keep null pointers null. must be page aligned
    /// * `end` - the highest address this hole can be located at. must be page aligned
    /// * `size` - the size of the hole (automatically rounded up to the nearest multiple of the page size of this page directory)
    fn find_hole(&self, start: usize, end: usize, size: usize) -> Option<usize> {
        let page_size = Self::PAGE_SIZE;

        assert!(start % page_size == 0, "start address is not page aligned");
        assert!(end % page_size == 0, "end address is not page aligned");

        let size = (size / page_size) * page_size + page_size;

        let mut hole_start: Option<usize> = None;

        for addr in (start..end).step_by(page_size) {
            if self.is_unused(addr) {
                if let Some(start) = hole_start {
                    if addr - start >= size {
                        return hole_start;
                    }
                /*} else if size <= page_size && addr >= start {
                return Some(addr);*/
                } else if hole_start.is_none() && addr >= start {
                    hole_start = Some(addr);
                }
            } else {
                hole_start = None;
            }
        }

        None
    }
}

/// struct to make allocating physical memory for page directories easier
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
    /// * `frame_set` - a BitSet that stores which pages are available and which arent. should be created based on the system's memory map
    pub fn new(frame_set: BitSet, page_size: usize) -> Self {
        Self { frame_set, page_size }
    }

    /// allocates a frame in the provided page directory
    ///
    /// the physical address of the newly allocated frame will be returned if successful
    ///
    /// # Arguments
    ///
    /// * `dir` - the page directory to allocate the frame in
    /// * `addr` - the virtual address to allocate the frame at. must be page aligned
    /// * `user_mode` - whether the allocated page will be accessible in user mode
    /// * `writable` - whether the allocated page will be able to be written to
    pub fn alloc_frame<T: PageDirectory>(&mut self, dir: &mut T, addr: usize, user_mode: bool, writable: bool) -> Result<u64, PagingError> {
        assert!(T::PAGE_SIZE == self.page_size);

        assert!(addr % self.page_size == 0, "frame address is not page aligned");

        if dir.is_unused(addr) {
            if let Some(idx) = self.frame_set.first_unset() {
                let phys_addr = idx as u64 * self.page_size as u64;

                let frame = PageFrame {
                    addr: phys_addr,
                    present: true,
                    user_mode,
                    writable,
                    copy_on_write: false,
                };

                trace!("allocating frame {:?} @ virt {:#x}", frame, addr);

                self.frame_set.set(idx);
                dir.set_page(addr, Some(frame))?;

                Ok(phys_addr)
            } else {
                Err(PagingError::NoAvailableFrames)
            }
        } else {
            Err(PagingError::FrameInUse)
        }
    }

    pub fn first_available_frame(&self) -> Option<u64> {
        self.frame_set.first_unset().map(|i| (i as u64) * (self.page_size as u64))
    }

    /// allocates a frame in the provided page directory at the given physical address, if available
    ///
    /// # Arguments
    ///
    /// * `dir` - the page directory to allocate the frame in
    /// * `addr` - the virtual address to allocate the frame at. must be page aligned
    /// * `phys` - the physical address to map the frame to. must also be page aligned
    /// * `user_mode` - whether the allocated page will be accessible in user mode
    /// * `writable` - whether the allocated page will be able to be written to
    pub fn alloc_frame_at<T: PageDirectory>(&mut self, dir: &mut T, addr: usize, phys: u64, user_mode: bool, writable: bool) -> Result<(), PagingError> {
        assert!(T::PAGE_SIZE == self.page_size);

        assert!(addr % self.page_size == 0, "frame address is not page aligned");
        assert!(phys % self.page_size as u64 == 0, "physical address is not page aligned");

        if dir.is_unused(addr) {
            let idx = phys / self.page_size as u64;

            let frame = PageFrame {
                addr: phys,
                present: true,
                user_mode,
                writable,
                copy_on_write: false,
            };

            trace!("allocating frame {:?} @ {:#x}", frame, addr);

            self.frame_set.set(idx as usize);
            dir.set_page(addr, Some(frame))?;

            Ok(())
        } else {
            Err(PagingError::FrameInUse)
        }
    }

    /// sets a frame in our list of frames as used, preventing it from being allocated elsewhere
    ///
    /// # Arguments
    ///
    /// * `dir` - a page table, used to get page size
    /// * `addr` - the address of the frame
    pub fn set_frame_used(&mut self, addr: u64) {
        assert!(addr % self.page_size as u64 == 0, "frame address is not page aligned");

        let idx = (addr / self.page_size as u64).try_into().unwrap();
        debug!("setting {idx:#x} as used");
        self.frame_set.set(idx);

        debug!("first_unset is now {:?}", self.frame_set.first_unset());
    }

    /// sets a frame in our list of frames as free, allowing it to be allocated elsewhere
    ///
    /// # Arguments
    ///
    /// * `dir` - a page table, used to get page size
    /// * `addr` - the address of the frame
    pub fn set_frame_free(&mut self, addr: u64) {
        assert!(addr % self.page_size as u64 == 0, "frame address is not page aligned");

        self.frame_set.clear((addr / self.page_size as u64).try_into().unwrap());
    }

    /// frees a frame in the provided page directory, allowing that region of memory to be used by other things
    ///
    /// returns the frame's physical address if successful
    ///
    /// # Arguments
    ///
    /// * `dir` - the page directory to free the frame in
    /// * `addr` - the virtual address to free the frame at. must be page aligned
    pub fn free_frame<T: PageDirectory>(&mut self, dir: &mut T, addr: usize) -> Result<u64, PagingError> {
        assert!(T::PAGE_SIZE == self.page_size);

        assert!(addr % self.page_size == 0, "frame address is not page aligned");

        if let Some(page) = dir.get_page(addr) {
            trace!("freeing phys {:#x}", page.addr);

            self.frame_set.clear((page.addr / self.page_size as u64) as usize);
            dir.set_page(addr, None)?;

            Ok(page.addr)
        } else {
            Err(PagingError::FrameUnused)
        }
    }

    /// prints out information about this page directory
    pub fn print_free(&self) {
        let bits_used = self.frame_set.bits_used;
        let size = self.frame_set.size;
        debug!("{}/{} mapped ({}% usage)", bits_used, size, (bits_used * 100) / size);
    }

    /// sets all the pages mapped in the given page directory to used in this PageManager, so that no future allocations use the same memory
    ///
    /// note: this is slow! very slow! this should be done as infrequently as possible
    pub fn sync_from_dir<T: PageDirectory>(&mut self, dir: &T) {
        assert!(T::PAGE_SIZE == self.page_size);

        // iterate over all virtual addresses
        for i in (0..=usize::MAX).step_by(self.page_size) {
            if dir.get_page(i).is_some() {
                //info!("got page @ {:#x}", i);
                self.frame_set.set(i / self.page_size);
            }
        }
    }
}

/// our kernel-wide page manager instance
static mut PAGE_MANAGER: Option<Mutex<PageManager>> = None;

/// gets the global page manager, locked with a spinlock
pub fn get_page_manager() -> MutexGuard<'static, PageManager> {
    unsafe {
        let manager = PAGE_MANAGER.as_ref().expect("page manager not initialized");

        if manager.is_locked() {
            debug!("warning: page manager is locked");
        }

        manager.lock()
    }
}

/// sets the global page manager. can only be called once
pub fn set_page_manager(manager: PageManager) {
    unsafe {
        if PAGE_MANAGER.is_some() {
            panic!("can't initialize pagemanager twice");
        } else {
            PAGE_MANAGER = Some(Mutex::new(manager));
        }
    }
}

static mut KERNEL_PAGE_DIR: Option<Mutex<crate::arch::PageDirectory<'static>>> = None;

pub fn get_page_dir() -> MutexGuard<'static, crate::arch::PageDirectory<'static>> {
    unsafe {
        let dir = KERNEL_PAGE_DIR.as_ref().expect("kernel page directory not set");

        if dir.is_locked() {
            debug!("warning: kernel page directory is locked");
        }

        dir.lock()
    }
}

pub fn set_page_dir(dir: crate::arch::PageDirectory<'static>) {
    unsafe {
        if KERNEL_PAGE_DIR.is_some() {
            panic!("can't set kernel page directory twice");
        } else {
            KERNEL_PAGE_DIR = Some(Mutex::new(dir));
        }
    }
}
