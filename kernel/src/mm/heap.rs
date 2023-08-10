//! heap and heap accessories

use super::ReservedMemory;
use crate::{
    arch::PROPERTIES,
    mm::{PageDirectory, PagingError},
};
use core::{alloc::Layout, ptr::NonNull};
use linked_list_allocator::Heap;
use log::{debug, error, trace};

pub struct HeapAllocError;

type Reserved = <crate::arch::PageDirectory as super::paging::PageDirectory>::Reserved;

/// contains the global state of our custom allocator
pub struct HeapAllocator {
    /// the heap we're using to allocate and deallocate
    heap: Heap,

    /// area of memory that's reserved on the heap
    reserved_memory: Option<Reserved>,

    /// the maximum size that this heap is allowed to grow to
    max_size: usize,
}

impl HeapAllocator {
    /// creates a new HeapAllocator, waiting for initialization
    ///
    /// # Safety
    ///
    /// the provided base and length must point to a valid contiguous region in memory, and must be valid for the 'static lifetime
    pub unsafe fn new(base: *mut u8, size: usize, max_size: usize) -> Self {
        let mut heap = Heap::new(base, size);
        let reserved_memory = Some(Reserved::allocate(|layout| heap.allocate_first_fit(layout).map_err(|_| HeapAllocError)).unwrap());

        Self { heap, reserved_memory, max_size }
    }

    /// allocates memory from the heap
    pub fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, HeapAllocError> {
        match self.heap.allocate_first_fit(layout) {
            Ok(allocation) => Ok(allocation),
            Err(_) => {
                trace!("ran out of heap space, expanding");

                let reserved_layout = Reserved::layout();

                fn align(unaligned: usize, alignment: usize) -> usize {
                    (unaligned / alignment) * alignment + alignment
                }

                // calculate where to expand the heap to
                let current_top = self.heap.top() as *const _ as usize;
                let new_top = align(current_top, reserved_layout.align()) + reserved_layout.size(); // add reserved layout
                let new_top = (align(new_top, layout.align()) + layout.size()).max(self.max_size); // add alloc layout
                let new_top = align(new_top, PROPERTIES.page_size); // align up to page size
                let growth = new_top - current_top;

                trace!("new_top is {new_top:#x} (growth {growth:#x})");

                fn alloc_pages(current_top: usize, new_top: usize, reserved_memory: &mut Option<Reserved>) -> Result<(), HeapAllocError> {
                    let global_state = crate::get_global_state();
                    let mut page_dir = global_state.page_directory.lock();
                    let mut page_manager = global_state.page_manager.lock();

                    // allocate and map in new pages for the heap
                    for i in (current_top..new_top).step_by(PROPERTIES.page_size) {
                        let page = Some(super::PageFrame {
                            addr: page_manager.alloc_frame(None).map_err(|_| HeapAllocError)?,
                            present: true,
                            writable: true,
                            ..Default::default()
                        });

                        match page_dir.set_page_no_alloc(None::<&crate::arch::PageDirectory>, i, page, None) {
                            Ok(_) => (),
                            Err(PagingError::AllocError) => {
                                page_dir
                                    .set_page_no_alloc(None::<&crate::arch::PageDirectory>, i, page, reserved_memory.take())
                                    .map_err(|_| HeapAllocError)?;
                            }
                            Err(_) => return Err(HeapAllocError),
                        }
                    }

                    // synchronize the current page directory and TLB
                    // TODO: synchronize this with other CPUs
                    global_state.cpus.read()[0].scheduler.sync_page_directory();
                    for i in (current_top..new_top).step_by(PROPERTIES.page_size) {
                        crate::arch::PageDirectory::flush_page(i);
                    }

                    Ok(())
                }

                match alloc_pages(current_top, new_top, &mut self.reserved_memory) {
                    Ok(_) => (),
                    Err(err) => {
                        error!("heap expansion failed, attempting cleanup");

                        let global_state = crate::get_global_state();
                        let mut page_dir = global_state.page_directory.lock();
                        let mut page_manager = global_state.page_manager.lock();

                        // free and unmap any pages for the heap that were allocated before failing
                        for i in (current_top..new_top).step_by(PROPERTIES.page_size) {
                            if let Some(page) = page_dir.get_page(i) {
                                page_manager.free_frame(page.addr, None);

                                if page_dir.set_page_no_alloc(None::<&crate::arch::PageDirectory>, i, None, None).is_err() {
                                    error!("couldn't remove page when cleaning up failed heap expansion");
                                }
                            }
                        }

                        global_state.cpus.read()[0].scheduler.sync_page_directory();
                        for i in (current_top..new_top).step_by(PROPERTIES.page_size) {
                            crate::arch::PageDirectory::flush_page(i);
                        }

                        return Err(err);
                    }
                }

                unsafe {
                    self.heap.extend(growth);
                }

                trace!("heap is now {:?} - {:?}", self.heap.bottom(), self.heap.top());

                if self.reserved_memory.is_none() {
                    match Reserved::allocate(|layout| self.heap.allocate_first_fit(layout).map_err(|_| HeapAllocError)) {
                        Ok(reserved) => self.reserved_memory = Some(reserved),
                        Err(err) => error!("failed to allocate reserved memory: {err:?}"),
                    }
                }

                // TODO: synchronize page table of currently running process on this CPU

                // try allocating again
                self.heap.allocate_first_fit(layout).map_err(|_| HeapAllocError)
            }
        }
    }

    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        if ptr < self.heap.bottom() || ptr >= self.heap.top() {
            debug!("can't free pointer allocated outside of heap ({layout:?} @ {ptr:?})");
        } else {
            unsafe {
                self.heap.deallocate(NonNull::new_unchecked(ptr), layout);
            }
        }
    }
}
