//! heap and heap accessories

use crate::arch::{
    KHEAP_START, PAGE_SIZE, INV_PAGE_SIZE, halt,
    paging::alloc_pages,
};
use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
    sync::atomic,
};
use linked_list_allocator::Heap;

// useful constants
pub const KHEAP_INITIAL_SIZE: usize = 0x100000;
pub const KHEAP_MAX_SIZE: usize = 0xffff000;
pub const HEAP_MIN_SIZE: usize = 0x70000;

pub fn init() {
    debug!("initializing heap");

    unsafe {
        let mut heap = Heap::empty();
        heap.init(KHEAP_START, KHEAP_INITIAL_SIZE);

        KERNEL_HEAP = Some(heap);
    }
}

/// the kernel heap itself
pub static mut KERNEL_HEAP: Option<Heap> = None;

/// global allocator that locks the heap with atomics, uses a bump allocator if it's locked (we want to allocate memory when panicing!),
/// and automatically grows and shrinks the heap to save memory
pub struct CustomAlloc(atomic::AtomicBool);

#[global_allocator]
static ALLOCATOR: CustomAlloc = CustomAlloc(atomic::AtomicBool::new(false));

unsafe impl GlobalAlloc for CustomAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if let Some(heap) = KERNEL_HEAP.as_mut() {
            // check if we have the lock
            if !self.0.swap(true, atomic::Ordering::Acquire) { // we do
                // allocate memory
                let ptr = match heap.allocate_first_fit(layout) {
                    Ok(allocation) => allocation.as_ptr(),
                    Err(_) => {
                        // something's gone wrong- likely not enough space
                        // expand the heap and try again

                        // give plenty of room to grow, we don't want to do this again
                        let mut new_size = heap.size() + layout.size() + layout.align() + PAGE_SIZE;

                        // align new_size to page boundary
                        if new_size & INV_PAGE_SIZE != 0 {
                            new_size &= INV_PAGE_SIZE;
                            new_size += PAGE_SIZE;
                        }

                        // make sure we're not expanding too much
                        assert!(heap.bottom() + new_size <= KHEAP_START + KHEAP_MAX_SIZE, "kernel heap out of memory");

                        debug!("growing kernel heap by {}", new_size - heap.size());

                        // allocate new pages for heap
                        let old_size = heap.size();

                        alloc_pages(heap.bottom() + heap.size(), (new_size - old_size) / PAGE_SIZE, true, true); // supervisor, read/write

                        // expand heap
                        heap.extend(new_size - heap.size());

                        // release lock
                        self.0.store(false, atomic::Ordering::Release);

                        // try again
                        self.alloc(layout)
                    },
                };
    
                // release lock
                self.0.store(false, atomic::Ordering::Release);
    
                // return pointer
                ptr
            } else { // we do not
                log!("!!! WARNING: heap locked, using bump alloc !!!");
    
                // use simple bump allocator to allocate memory, since we want panic messages to be able to be displayed
                crate::arch::paging::bump_alloc(layout.size(), layout.align())
            }
        } else {
            panic!("can't alloc before heap init");
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Some(heap) = KERNEL_HEAP.as_mut() {
            // check if we have the lock
            if ((ptr as usize) < KHEAP_START) || ((ptr as usize) >= KHEAP_START + KHEAP_MAX_SIZE) {
                log!("!!! WARNING: attempted dealloc outside of heap !!!");
            } else if !self.0.swap(true, atomic::Ordering::Acquire) { // we do
                // free memory
                heap.deallocate(NonNull::new_unchecked(ptr), layout);
    
                // release lock
                self.0.store(false, atomic::Ordering::Release);
            } else { // we do not
                log!("!!! WARNING: heap locked, cannot free !!!");
            }
        } else {
            panic!("can't alloc before heap init");
        }
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    log!("PANIC: allocation error: {:?}", layout);
    halt();
}
