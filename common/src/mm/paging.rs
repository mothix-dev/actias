//! paging abstraction layer

use crate::{
    types::errno::Errno,
    util::{array::BitSet, FormatHex},
};
use alloc::{
    alloc::{alloc, dealloc, Layout},
    vec::Vec,
};
use core::{fmt, marker::PhantomData};
use log::{debug, error, trace};

/// an error that can be returned from paging operations
pub enum PageError {
    NoAvailableFrames,
    FrameUnused,
    FrameInUse,
    AllocError,
    BadFrame,
}

impl fmt::Display for PageError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", match self {
            Self::NoAvailableFrames => "no available frames",
            Self::FrameUnused => "frame is unused",
            Self::FrameInUse => "frame already in use",
            Self::AllocError => "error allocating memory for table",
            Self::BadFrame => "bad frame",
        })
    }
}

impl fmt::Debug for PageError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PageError: \"{}\"", self)
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
    /* -= Required functions -= */

    /// given a virtual address, get the page that contains it from this directory in a hardware agnostic form
    fn get_page(&self, addr: usize) -> Option<PageFrame>;

    /// insert a page frame into the directory
    ///
    /// # Arguments
    ///
    /// * `addr` - the virtual address to insert the page frame at
    /// * `page` - the page frame to insert
    fn set_page(&mut self, addr: usize, page: Option<PageFrame>) -> Result<(), PageError>;

    /// switch the mmu to this page directory
    ///
    /// # Safety
    ///
    /// this function is unsafe since whatever code is being run currently could be different or nonexistent when switching pages, thus causing undefined behavior
    unsafe fn switch_to(&self);

    /// gets the page size that this page directory uses in bytes
    fn page_size(&self) -> usize;

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
    fn copy_from(&mut self, dir: &mut Self, from: usize, to: usize, num: usize) -> Result<(), PageError> {
        let page_size = self.page_size();

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
    fn copy_on_write_from(&mut self, dir: &mut Self, from: usize, to: usize, num: usize) -> Result<(), PageError> {
        let page_size = self.page_size();

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
        let page_size = self.page_size() - 1;
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
    unsafe fn map_memory_from<O, R>(&mut self, from: &mut Self, addr: usize, len: usize, op: O) -> Result<R, Errno>
    where O: FnOnce(&mut [u8]) -> R {
        let page_size = self.page_size();

        // get starting and ending addresses
        let mut start = addr;
        let mut end = addr + len;

        assert!(end > start);

        debug!("mapping partial page directory in");
        debug!("start @ {:#x}, end @ {:#x}", start, end);

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

        let buf_len = end - start;

        debug!("start now @ {:#x}, end now @ {:#x}", start, end);

        debug!("buf size {:#x}, aligned to {:#x}, offset {:#x}", len, buf_len, offset);

        let layout = Layout::from_size_align(buf_len, page_size).unwrap();
        let ptr = alloc(layout);

        if ptr.is_null() {
            error!("error allocating buffer in map_memory()");
            Err(Errno::OutOfMemory)?
        }

        assert!(ptr as usize % page_size == 0); // make absolutely sure pointer is page aligned

        debug!("mapping {} pages from {:#x} (task mem) to {:#x} (kernel mem)", (end - start) / page_size, start, ptr as usize);

        // get addresses of pages we're gonna remap so we can map them back later
        let mut existing_phys: Vec<u64> = Vec::new();

        // attempt to safely reserve memory for our mapping
        if let Err(err) = existing_phys.try_reserve_exact((end - start) / page_size) {
            error!("error reserving memory in map_memory(): {:?}", err);
            dealloc(ptr, layout);

            Err(Errno::OutOfMemory)?
        }

        for i in (ptr as usize..ptr as usize + buf_len).step_by(page_size) {
            // virt to phys calculation from current page directory
            let addr = match self.virt_to_phys(i) {
                Some(a) => a,
                None => {
                    // something bad happened, revert back to original state and return an error
                    debug!("aborting map (before remap), dealloc()ing");
                    dealloc(ptr, layout);

                    Err(Errno::BadAddress)?
                }
            };
            debug!("existing: {:#x} -> {:#x}", i, addr);
            existing_phys.push(addr);
        }

        debug!("existing_phys: {:x?}", existing_phys);

        // loop over pages, get physical address of each page and map it in or create new page and alloc mem
        for i in (start..end).step_by(page_size) {
            // get the physical address of the page at the given address, or allocate a new one if there isn't one mapped
            let phys_addr = match from.virt_to_phys(i) {
                Some(a) => a,
                None => {
                    debug!("couldn't get phys addr for virt {:#x}", i);

                    // something bad happened, revert back to original state and return an error
                    debug!("aborting map (during remap), dealloc()ing and fixing mapping");
                    for (idx, addr) in (ptr as usize..ptr as usize + buf_len).step_by(page_size).enumerate() {
                        debug!("virt @ {:x}, phys @ {:x}", addr, existing_phys[idx]);
                        self.set_page(
                            addr,
                            Some(PageFrame {
                                addr: existing_phys[idx],
                                present: true,
                                user_mode: false,
                                writable: true,
                                copy_on_write: false,
                            }),
                        )
                        .expect("couldn't remap page");
                    }
                    dealloc(ptr, layout);

                    Err(Errno::BadAddress)?
                }
            };

            debug!("{:x} @ phys addr: {:x}", i, phys_addr);

            // todo: maybe change this to debug_assert at some point? its prolly hella slow
            assert!(!existing_phys.contains(&phys_addr), "trampling on other page directory's memory");

            let virt = ptr as usize + (i - start) as usize;

            // remap memory
            self.set_page(
                virt,
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

        debug!("slice @ {:#x} + {:#x}: {:#x}, len {:#x}", ptr as usize, offset as usize, ptr as usize + offset as usize, len);

        // call function
        let res = op(core::slice::from_raw_parts_mut((ptr as usize + offset as usize) as *mut u8, len));

        // map pages back to their original addresses
        debug!("cleaning up mapping");
        for (idx, addr) in (ptr as usize..ptr as usize + buf_len).step_by(page_size).enumerate() {
            debug!("virt @ {:x}, phys @ {:x}", addr, existing_phys[idx]);
            self.set_page(
                addr,
                Some(PageFrame {
                    addr: existing_phys[idx],
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
        let page_size = self.page_size();

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
pub struct PageManager<T: PageDirectory> {
    /// bitset to speed up allocation of page frames
    ///
    /// every bit in this set represents an individual page in the directory
    ///
    /// the size of this bitset can be calculated by dividing the address of the top of available memory by the system's page size
    pub frame_set: BitSet,

    /// phantom data used to restrict page managers to specific page directory types, otherwise rust complains about vtables
    phantom: PhantomData<T>,
}

impl<T: PageDirectory> PageManager<T> {
    /// creates a new page manager with the provided bitset for available frames
    ///
    /// # Arguments
    ///
    /// * `frame_set` - a BitSet that stores which pages are available and which arent. should be created based on the system's memory map
    pub fn new(frame_set: BitSet) -> Self {
        Self { frame_set, phantom: PhantomData::<T> }
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
    pub fn alloc_frame(&mut self, dir: &mut T, addr: usize, user_mode: bool, writable: bool) -> Result<u64, PageError> {
        let page_size = dir.page_size();

        assert!(addr % page_size == 0, "frame address is not page aligned");

        if dir.is_unused(addr) {
            if let Some(idx) = self.frame_set.first_unset() {
                let phys_addr = idx as u64 * page_size as u64;

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
                Err(PageError::NoAvailableFrames)
            }
        } else {
            Err(PageError::FrameInUse)
        }
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
    pub fn alloc_frame_at(&mut self, dir: &mut T, addr: usize, phys: u64, user_mode: bool, writable: bool) -> Result<(), PageError> {
        let page_size = dir.page_size();

        assert!(addr % page_size == 0, "frame address is not page aligned");
        assert!(phys % page_size as u64 == 0, "physical address is not page aligned");

        if dir.is_unused(addr) {
            let idx = phys / page_size as u64;

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
            Err(PageError::FrameInUse)
        }
    }

    /// sets a frame in our list of frames as used, preventing it from being allocated elsewhere
    ///
    /// # Arguments
    ///
    /// * `dir` - a page table, used to get page size
    /// * `addr` - the address of the frame
    pub fn set_frame_used(&mut self, dir: &mut T, addr: usize) {
        let page_size = dir.page_size();

        assert!(addr % page_size == 0, "frame address is not page aligned");

        self.frame_set.set(addr / page_size);
    }

    /// sets a frame in our list of frames as free, allowing it to be allocated elsewhere
    ///
    /// # Arguments
    ///
    /// * `dir` - a page table, used to get page size
    /// * `addr` - the address of the frame
    pub fn set_frame_free(&mut self, dir: &mut T, addr: usize) {
        let page_size = dir.page_size();

        assert!(addr % page_size == 0, "frame address is not page aligned");

        self.frame_set.clear(addr / page_size);
    }

    /// frees a frame in the provided page directory, allowing that region of memory to be used by other things
    ///
    /// returns the frame's physical address if successful
    ///
    /// # Arguments
    ///
    /// * `dir` - the page directory to free the frame in
    /// * `addr` - the virtual address to free the frame at. must be page aligned
    pub fn free_frame(&mut self, dir: &mut T, addr: usize) -> Result<u64, PageError> {
        let page_size = dir.page_size();

        assert!(addr % page_size == 0, "frame address is not page aligned");

        if let Some(page) = dir.get_page(addr) {
            trace!("freeing phys {:#x}", page.addr);

            self.frame_set.clear((page.addr / page_size as u64) as usize);
            dir.set_page(addr, None)?;

            Ok(page.addr)
        } else {
            Err(PageError::FrameUnused)
        }
    }

    /// prints out information about this page directory
    pub fn print_free(&self) {
        let bits_used = self.frame_set.bits_used;
        let size = self.frame_set.size;
        debug!("{}/{} mapped ({}% usage)", bits_used, size, (bits_used * 100) / size);
    }
}
