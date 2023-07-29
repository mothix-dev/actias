//! heap and heap accessories

use super::ReservedMemory;
use core::{alloc::Layout, ptr::NonNull};
use linked_list_allocator::Heap;
use log::debug;

pub struct HeapAllocError;

/// contains the global state of our custom allocator
pub struct HeapAllocator<R: ReservedMemory> {
    /// the heap we're using to allocate and deallocate
    heap: Heap,

    /// area of memory that's reserved on the heap
    reserved_memory: Option<R>,

    /// the maximum size that this heap is allowed to grow to
    max_size: usize,
}

impl<R: ReservedMemory> HeapAllocator<R> {
    /// creates a new HeapAllocator, waiting for initialization
    ///
    /// # Safety
    ///
    /// the provided base and length must point to a valid contiguous region in memory, and must be valid for the 'static lifetime
    pub unsafe fn new(base: *mut u8, size: usize, max_size: usize) -> Self {
        Self {
            heap: Heap::new(base, size),
            reserved_memory: Some(R::allocate().unwrap()),
            max_size,
        }
    }

    /// allocates memory from the heap
    pub fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, HeapAllocError> {
        /*match self.heap.allocate_first_fit(layout) {
            Ok(allocation) => Ok(allocation.as_ptr()),
            Err(_) => {
                trace!("ran out of heap space, expanding");

                // calculate lower bound for heap expansion
                let new_top = {
                    let align = align_of::<R>();
                    (self.heap.top() as *const _ as usize / align) * align + align // heap top aligned to reserved layout align
                };
                let new_top = {
                    let align = layout.align();
                    (new_top / align) * align + align + layout.size() // heap top aligned to reserved layout align and alloc align plus alloc size
                };

                loop {
                    // allocate memory to expand the heap
                    //let new_top_2 = (self.expand_callback)(self.heap.top(), new_top, &Self::external_alloc, &Self::external_dealloc)?;
                    todo!();

                    // sanity check
                    if new_top_2 <= self.heap.top() {
                        error!("heap didn't expand");
                        Err(())?
                    }

                    // expand the heap
                    unsafe {
                        self.heap.extend(new_top_2 - self.heap.top());
                    }

                    // if the target top address hasn't been reached but we've at least been able to expand a little bit, just try again
                    // the heap has been expanded so the callback will be able to have more memory to work with
                    // we can do this as many times as we want (tho we probably shouldn't)
                    // TODO: maybe figure out some way to limit this?
                    if new_top_2 < new_top {
                        debug!("heap didn't expand enough (need top {:#x}, got {:#x}), trying again", new_top, new_top_2);
                    } else {
                        // break out of the loop, expand callback has finished
                        break;
                    }
                }

                trace!("heap is now {:?} - {:?}", self.heap.bottom(), self.heap.top());

                // try allocating again
                let allocation = self.heap.allocate_first_fit(layout).map(|allocation| allocation.as_ptr());

                allocation
            }
        }*/

        self.heap.allocate_first_fit(layout).map_err(|_| HeapAllocError)
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
