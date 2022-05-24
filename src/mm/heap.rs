//! heap functions, malloc, maybe global allocator?

use crate::util::array::OrderedArray;
use core::mem::size_of;
use core::cmp::{Ordering, PartialOrd};
use crate::arch::paging::{alloc_page, free_page};
use crate::arch::{KHEAP_START, PAGE_SIZE, INV_PAGE_SIZE};
use alloc::alloc::{GlobalAlloc, Layout};
use core::sync::atomic;
use crate::arch::halt;

// useful constants
pub const KHEAP_INITIAL_SIZE: usize = 0x100000;
pub const KHEAP_MAX_SIZE: usize = 0xffff000;
pub const HEAP_INDEX_SIZE: usize = 0x20000;
pub const HEAP_MIN_SIZE: usize = 0x70000;

// based on http://www.jamesmolloy.co.uk/tutorial_html/7.-The%20Heap.html

#[derive(Debug)]
#[repr(C)]
pub struct Header {
    pub magic: u32,
    pub is_hole: bool,
    pub size: usize,
}

/// wrapper around raw pointer to header to allow for comparing by size
#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct HeaderPtr(pub *mut Header);

impl PartialEq for HeaderPtr {
    fn eq(&self, other: &Self) -> bool {
        unsafe { (*self.0).size == (*other.0).size }
    }
}

impl PartialOrd for HeaderPtr {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        unsafe { (*self.0).size.partial_cmp(&(*other.0).size) }
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct Footer {
    pub magic: u32,
    pub header: *mut Header,
}

const MAGIC_NUMBER: u32 = 0xdeadbeef; // TODO: more interesting magic number lmao

#[derive(Debug)]
pub struct Heap {
    pub index: OrderedArray<HeaderPtr>,
    pub start_address: usize,
    pub end_address: usize,
    pub max_address: usize,
    pub supervisor: bool,
    pub readonly: bool,
}

impl Heap {
    /// create a new heap
    pub fn new(mut start: usize, end: usize, max: usize, supervisor: bool, readonly: bool) -> Self {
        assert!(start % PAGE_SIZE == 0, "start address needs to be page aligned!");
        assert!(end % PAGE_SIZE == 0, "end address needs to be page aligned!");

        // create ordered array for index
        let mut index = OrderedArray::place_at(start as *mut _, HEAP_INDEX_SIZE);

        // increment start by array size and page align it
        start += HEAP_INDEX_SIZE * size_of::<HeaderPtr>();
        if start & INV_PAGE_SIZE != 0 {
            start &= INV_PAGE_SIZE;
            start += PAGE_SIZE;
        }

        // create a new hole spanning the entire heap and add it to index
        let hole = unsafe { &mut *(start as *mut Header) };
        hole.size = end - start;
        hole.magic = MAGIC_NUMBER;
        hole.is_hole = true;
        index.insert(HeaderPtr(hole));

        Self {
            index,
            start_address: start,
            end_address: end,
            max_address: max,
            supervisor, readonly,
        }
    }

    pub fn alloc<T>(&mut self, size: usize, alignment: usize) -> *mut T {
        // make sure we aren't allocating too little memory for the requested type
        assert!(size >= size_of::<T>(), "size of type is larger than requested size");

        // account for header and footer size
        let mut new_size = size + size_of::<Header>() + size_of::<Footer>();
        
        // check if we have a large enough hole
        if let Some(hole_index) = self.find_smallest_hole(new_size, alignment) {
            let orig_hole_header_ptr = self.index.get(hole_index).0;

            let mut orig_hole_pos = orig_hole_header_ptr as usize;
            let orig_hole_header = unsafe { &mut *orig_hole_header_ptr };
            let mut orig_hole_size = orig_hole_header.size;

            // if the hole is bigger than our requested size but too small to split in two, increase its size so it isn't split later
            if orig_hole_size - new_size < size_of::<Header>() + size_of::<Footer>() {
                //size += orig_hole_size - new_size;
                new_size = orig_hole_size;
            }

            // if we want aligned data and aren't page aligned already
            if alignment > 1 && ((orig_hole_pos + size_of::<Header>()) % alignment) > 0 {
                let offset: usize = 
                    if (orig_hole_pos + size_of::<Header>()) % alignment != 0 {
                        alignment - ((orig_hole_pos + size_of::<Header>()) % alignment)
                    } else {
                        0
                    };
                
                // make sure hole is big enough
                assert!(offset + new_size <= orig_hole_size, "hole is too small");
                
                // do we have enough room to split?
                if offset >= size_of::<Header>() + size_of::<Footer>() {
                    // yes, split hole in two to free up space before our allocated region
                    let new_location = orig_hole_pos + offset;

                    // modify the original hole header to make a new hole that takes up the space in between the original hole position and the nearest page boundary
                    // we can just modify the original header since we'd just delete it otherwise
                    orig_hole_header.size = offset;
                    orig_hole_header.magic = MAGIC_NUMBER;
                    orig_hole_header.is_hole = true;

                    let hole_footer = unsafe { &mut *((new_location - size_of::<Footer>()) as *mut Footer) };
                    hole_footer.magic = MAGIC_NUMBER;
                    hole_footer.header = orig_hole_header_ptr;

                    // change our position and size to point to the proper page aligned location and size
                    orig_hole_pos = new_location;
                    orig_hole_size -= orig_hole_header.size;
                } else {
                    // no, remove the hole from our index as normal
                    self.index.remove(hole_index);
                }
            } else {
                // otherwise just remove the hole from our index, it's not needed anymore
                self.index.remove(hole_index);
            }

            // overwrite original header or create it if we want it somewhere else
            let block_header = unsafe { &mut *(orig_hole_pos as *mut Header) };
            block_header.magic = MAGIC_NUMBER;
            block_header.is_hole = false;
            block_header.size = new_size;

            // and footer
            let block_footer = unsafe { &mut *((orig_hole_pos + size_of::<Header>() + size) as *mut Footer) };
            block_footer.magic = MAGIC_NUMBER;
            block_footer.header = block_header;

            // is the allocated block big enough to put another hole after it?
            if orig_hole_size - new_size > 0 {
                // create a new hole after our allocated space
                let hole_header = unsafe { &mut *((orig_hole_pos + new_size) as *mut Header) };
                hole_header.magic = MAGIC_NUMBER;
                hole_header.is_hole = true;
                hole_header.size = orig_hole_size - new_size;

                // create a new footer if applicable
                let hole_footer_ptr = (orig_hole_pos + orig_hole_size - size_of::<Footer>()) as *mut Footer;

                if (hole_footer_ptr as usize) < self.end_address {
                    let hole_footer = unsafe { &mut *hole_footer_ptr };
                    hole_footer.magic = MAGIC_NUMBER;
                    hole_footer.header = hole_header;
                }

                // add our new hole to the index
                self.index.insert(HeaderPtr(hole_header));
            }

            // return a reference to our newly allocated memory
            (orig_hole_pos + size_of::<Header>()) as *mut T
        } else { // we don't have a large enough hole
            // save some data
            let old_length = self.end_address - self.start_address;
            let old_end_address = self.end_address;

            // allocate more space for the heap
            self.expand(old_length + new_size);
            let new_length = self.end_address - self.start_address;

            // find last header (in location)
            let mut value: *mut Header = core::ptr::null_mut();
            let mut idx: Option<usize> = None;

            for i in 0..self.index.size {
                let tmp = self.index.get(i).0;
                if tmp > (value as *mut _) {
                    value = tmp;
                    idx = Some(i);
                }
            }

            // did we find a header?
            if let Some(idx) = idx {
                // adjust last header to take up new allocated space
                let header_ptr = self.index.get(idx).0;
                let header = unsafe { &mut *header_ptr };
                //header.size += new_length - old_length;
                header.size = self.end_address - (header_ptr as usize);

                // create new footer at end of allocated space
                let footer_ptr = (header_ptr as usize + header.size - size_of::<Footer>()) as *mut Footer;
                let footer = unsafe { &mut *footer_ptr };
                footer.magic = MAGIC_NUMBER;
                footer.header = header;
            } else { // we didn't find a header
                // create a new header
                let header = unsafe { &mut *(old_end_address as *mut Header) };
                header.magic = MAGIC_NUMBER;
                header.size = new_length - old_length;
                header.is_hole = true;

                // and footer
                let footer = unsafe { &mut *((old_end_address + header.size - size_of::<Footer>()) as *mut Footer) };
                footer.magic = MAGIC_NUMBER;
                footer.header = header;

                // insert the new header into index
                self.index.insert(HeaderPtr(header));
            }

            // we now have enough space, so we can recurse and try again
            self.alloc(size, alignment)
        }
    }

    pub fn free<T>(&mut self, raw_ptr: *mut T) {
        let raw_ptr_loc = raw_ptr as usize;

        // if for some reason we get a null pointer, exit gracefully
        if raw_ptr.is_null() {
            return;
        }

        // get references and pointers to header and footer
        let mut header_ptr = (raw_ptr_loc - size_of::<Header>()) as *mut Header;
        let mut header = unsafe { &mut *header_ptr };

        let mut footer_ptr = (header_ptr as usize + header.size - size_of::<Footer>()) as *mut Footer;
        let mut footer = unsafe { &mut *footer_ptr };

        // make sure header and footer aren't corrupted
        assert!(header.magic == MAGIC_NUMBER, "header magic number doesn't match");
        assert!(footer.magic == MAGIC_NUMBER, "footer magic number doesn't match");

        // convert to a hole
        header.is_hole = true;

        // do we want to add this header into the holes index?
        let mut add_to_index = true;

        // === unify left

        // check if there's a footer immediately before our header
        let test_footer_ptr = (header_ptr as usize - size_of::<Footer>()) as *mut Footer;
        let test_footer = unsafe { &mut *test_footer_ptr };

        if test_footer.magic == MAGIC_NUMBER && unsafe { &mut *test_footer.header }.is_hole {
            // found a hole, switch our header with it and increase its size 
            let cache_size = header.size;

            header_ptr = test_footer.header;
            header = unsafe { &mut *header_ptr };
            footer.header = header_ptr;
            header.size += cache_size;

            add_to_index = false;
        }

        // === unify right

        // check if there's a header immediately after our footer
        let test_header_ptr = (footer_ptr as usize + size_of::<Footer>()) as *mut Header;
        let test_header = unsafe { &mut *test_header_ptr };

        if test_header.magic == MAGIC_NUMBER && test_header.is_hole {
            // found a hole
            header.size += test_header.size;

            footer_ptr = (test_header_ptr as usize + test_header.size - size_of::<Footer>()) as *mut Footer;
            //footer = unsafe { &mut *footer_ptr };

            let mut removed = false;
            for i in 0..self.index.size { // FIXME: use iterator for this lmao
                if self.index.get(i).0 == test_header_ptr {
                    self.index.remove(i);
                    removed = true;
                    break;
                }
            }

            if !removed {
                panic!("header doesn't exist in index");
            }
        }

        // ===

        // if the footer location is the end address of this heap, we can contract it
        if footer_ptr as usize + size_of::<Footer>() == self.end_address {
            let old_length = self.end_address - self.start_address;
            let new_length = self.contract(header_ptr as usize - self.start_address);

            let header_size_signed: isize = header.size.try_into().unwrap();
            let old_length_signed: isize = old_length.try_into().unwrap();
            let new_length_signed: isize = new_length.try_into().unwrap();

            // will we still exist after resizing?
            if header_size_signed - old_length_signed - new_length_signed > 0 { // yes, resize
                header.size -= old_length - new_length;

                footer_ptr = (header_ptr as usize + header.size - size_of::<Footer>()) as *mut Footer;
                footer = unsafe { &mut *footer_ptr };

                footer.magic = MAGIC_NUMBER;
                footer.header = header_ptr;
            } else { // no, remove from index
                for i in 0..self.index.size {
                    if self.index.get(i).0 == test_header_ptr {
                        self.index.remove(i);
                        break;
                    }
                }
            }
        }

        // add the header to the index if needed
        if add_to_index {
            self.index.insert(HeaderPtr(header_ptr));
        }
    }

    /// find smallest hole in heap
    fn find_smallest_hole(&self, size: usize, alignment: usize) -> Option<usize> {
        // loop through all headers
        let mut iterator = 0;
        while iterator < self.index.size {
            let header_ptr = self.index.get(iterator).0;
            let location = header_ptr as usize;
            let header = unsafe { &*header_ptr };

            if alignment > 1 { // do we want aligning?
                // find nearest alignment
                let offset: isize = 
                    if (location + size_of::<Header>()) % alignment != 0 {
                        alignment as isize - ((location + size_of::<Header>()) % alignment) as isize
                    } else {
                        0
                    };

                // check if the hole is big enough to fit the amount of data we want when aligned
                let hole_size = header.size as isize - offset;

                if hole_size >= size.try_into().unwrap() {
                    break;
                }
            } else if header.size >= size { // check if header is big enough
                break;
            }

            iterator += 1;
        }

        if iterator == self.index.size { // we didn't find a header
            None
        } else { // we found a header
            Some(iterator)
        }
    }

    /// expand heap
    fn expand(&mut self, mut new_size: usize) {
        // make sure we're actually expanding
        assert!(new_size > self.end_address - self.start_address, "new size is smaller than current size");

        // align new_size to page boundary
        if new_size & INV_PAGE_SIZE != 0 {
            new_size &= INV_PAGE_SIZE;
            new_size += PAGE_SIZE;
        }

        // make sure we're not expanding too much
        assert!(self.start_address + new_size <= self.max_address);

        // allocate new pages for heap
        let old_size = self.end_address - self.start_address;
        
        for i in (old_size..new_size).step_by(PAGE_SIZE) {
            alloc_page(self.start_address + i, self.supervisor, !self.readonly);
        }

        self.end_address = self.start_address + new_size;
    }

    /// contract heap
    fn contract(&mut self, mut new_size: usize) -> usize {
        // make sure we're actually contracting
        assert!(new_size < self.end_address - self.start_address, "new size is greater than current size");

        // align new_size to page boundary
        if new_size & INV_PAGE_SIZE != 0 {
            new_size &= INV_PAGE_SIZE;
            new_size += PAGE_SIZE;
        }

        // don't contract below minimum size
        if new_size < HEAP_MIN_SIZE {
            new_size = HEAP_MIN_SIZE;
        }

        // free unneeded pages
        let old_size = self.end_address - self.start_address;

        for i in (old_size - PAGE_SIZE..new_size).step_by(PAGE_SIZE).rev() {
            free_page(self.start_address + i);
        }

        self.end_address = self.start_address + new_size;

        new_size
    }

    pub fn print_holes(&self) {
        log!("{} holes", self.index.size);
        for i in 0..self.index.size {
            let header_ptr = self.index.get(i).0;
            let header = unsafe { &*header_ptr };
            log!("{:#x}: {:?}", header_ptr as usize, header);
        }
        log!(" ===");
    }
}

pub static mut KERNEL_HEAP: Option<Heap> = None;

/// initialize heap
pub fn init() {
    unsafe {
        KERNEL_HEAP = Some(Heap::new(KHEAP_START, KHEAP_START + KHEAP_INITIAL_SIZE, KHEAP_START + KHEAP_MAX_SIZE, false, false));
    }
}

/// lock for heap, prevents access if interrupted during an operation
static HEAP_LOCK: atomic::AtomicBool = atomic::AtomicBool::new(false);

/// wrapper to safely access kernel heap for allocating memory
pub fn alloc<T>(size: usize) -> *mut T {
    alloc_aligned(size, 0)
}

/// wrapper to safely access kernel heap for allocating page-aligned memory
pub fn alloc_aligned<T>(size: usize, alignment: usize) -> *mut T {
    if let Some(heap) = unsafe { KERNEL_HEAP.as_mut() } {
        // check if we have the lock
        if !HEAP_LOCK.swap(true, atomic::Ordering::Acquire) { // we do
            // allocate memory
            let ptr = heap.alloc(size, alignment);

            // release lock
            HEAP_LOCK.store(false, atomic::Ordering::Release);

            // return pointer
            ptr
        } else { // we do not
            log!("!!! WARNING: heap locked, using bump alloc !!!");

            // use simple bump allocator to allocate memory, since we want panic messages to be able to be displayed
            unsafe { crate::arch::paging::bump_alloc(size, alignment) }
        }
    } else {
        panic!("can't alloc before heap init");
    }
}

/// wrapper to safely access kernel heap for freeing memory
pub fn free<T>(p: *mut T) {
    if let Some(heap) = unsafe { KERNEL_HEAP.as_mut() } {
        // check if we have the lock
        if !HEAP_LOCK.swap(true, atomic::Ordering::Acquire) { // we do
            // free memory
            heap.free(p);

            // release lock
            HEAP_LOCK.store(false, atomic::Ordering::Release);
        } else { // we do not
            log!("!!! WARNING: heap locked, cannot free !!!");
        }
    } else {
        panic!("can't alloc before heap init");
    }
}

/// our custom allocator, allows rust to use our heap
pub struct CustomAlloc;

#[global_allocator]
static ALLOCATOR: CustomAlloc = CustomAlloc;

unsafe impl GlobalAlloc for CustomAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        debug!("global alloc: {:?}", layout);
        alloc_aligned::<u8>(layout.size(), layout.align())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        debug!("global dealloc: {:#x}, {:?}", ptr as usize, _layout);
        free::<u8>(ptr)
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    log!("PANIC: allocation error: {:?}", layout);
    halt();
}
