//! heap and heap accessories

use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
    sync::atomic,
};
use linked_list_allocator::Heap;
use log::{debug, error, trace};

/// type of callback that's run to allocate memory for expanding the heap when needed
///
/// ```rust
/// fn(old_top: usize, new_top: usize, alloc: &dyn Fn(Layout) -> Result<*mut u8, ()>, dealloc: &dyn Fn(*mut u8, Layout)) -> Result<usize, ()>) -> Result<usize, ()>
/// ```
///
/// # Arguments
///
/// * `old_top` - the top address of the heap before expansion
/// * `new_top` - minimum bound for the top of the heap after expansion. the heap can be expanded as much as we want as long as the top address is higher than this
/// * `alloc` - function pointer to an implementation of alloc (same as allocating memory manually). this is very unsafe and should be handled with care
/// since it bypasses all locks in place over the global AllocState, since this occurs in the middle of an allocation
/// * `dealloc` - function pointer to an implementation of dealloc. same deal with this
///
/// # Returns
///
/// all functions of this type return the actual new top address of the heap after allocation, and the unit type on failure
///
/// if the heap is unable to be expanded all the way (but is expanded at least a little bit), the function should return Ok with
/// the highest top address that it's managed to allocate. the heap will then be expanded (to allow more page tables to be allocated, for example)
/// and this function will be called again as many times as it needs to finish the allocation
pub type AllocExpandCallback = dyn Fn(usize, usize, &dyn Fn(Layout) -> Result<*mut u8, ()>, &dyn Fn(*mut u8, Layout)) -> Result<usize, ()>;

/// contains the global state of our custom allocator
struct AllocState<'a> {
    /// controls access to the state
    lock: atomic::AtomicBool,

    /// the heap we're using to allocate and deallocate
    heap: Heap,

    /// the layout of a portion of memory to keep unused for heap expansion, if applicable
    reserved_layout: Option<Layout>,

    /// a slice representation of our reserved memory, if present
    reserved_area: Option<&'static [u8]>,

    /// callback that's run to expand the heap when needed
    expand_callback: &'a AllocExpandCallback,

    /// whether we can wait for this state to be unlocked instead of panicing. only useful with multiple cpus
    can_spin: bool,
}

impl<'a> AllocState<'a> {
    /// creates a new AllocState, waiting for initialization
    pub const fn new() -> Self {
        // our initial expand callback, just returns Err
        fn initial_expand_callback(_old_top: usize, _new_top: usize, _alloc: &dyn Fn(Layout) -> Result<*mut u8, ()>, _dealloc: &dyn Fn(*mut u8, Layout)) -> Result<usize, ()> {
            Err(())
        }

        Self {
            lock: atomic::AtomicBool::new(false),
            heap: Heap::empty(),
            reserved_layout: None,
            reserved_area: None,
            expand_callback: &initial_expand_callback,
            can_spin: false,
        }
    }

    /// waits for the lock to be released if spinlocks are enabled, otherwise panics
    #[allow(clippy::while_immutable_condition)]
    fn wait_for_unlock(&self) {
        // should we use a spinlock?
        if self.can_spin {
            // just try this over and over until it works lmao
            while self.lock.swap(true, atomic::Ordering::Acquire) {}
        } else {
            panic!("allocator state locked");
        }
    }

    /// initializes the heap in this AllocState
    pub fn init(&mut self, start: usize, size: usize) {
        // check if we have the lock
        if !self.lock.swap(true, atomic::Ordering::Acquire) {
            debug!("initializing allocator @ {:#x}, size {:#x}", start, size);

            // init heap
            unsafe {
                self.heap.init(start, size);
            }

            // allocate reserved memory if it hasn't been already for whatever reason
            if self.reserved_layout.is_some() && self.reserved_area.is_none() {
                unsafe {
                    self.alloc_reserved();
                }
            }

            // release lock
            self.lock.store(false, atomic::Ordering::Release);
        } else {
            // we do not
            self.wait_for_unlock();

            self.init(start, size);
        }
    }

    /// sets whether we can use spinlocking or not
    pub fn set_can_spin(&mut self, can_spin: bool) {
        // check if we have the lock
        if !self.lock.swap(true, atomic::Ordering::Acquire) {
            self.can_spin = can_spin;

            // release lock
            self.lock.store(false, atomic::Ordering::Release);
        } else {
            // we do not
            self.wait_for_unlock();

            self.set_can_spin(can_spin);
        }
    }

    /// sets the expand callback in this AllocState, which is run every time we run out of memory in order to allocate more pages for expanding the heap
    pub fn set_expand_callback(&mut self, callback: &'a AllocExpandCallback) {
        // check if we have the lock
        if !self.lock.swap(true, atomic::Ordering::Acquire) {
            self.expand_callback = callback;

            // release lock
            self.lock.store(false, atomic::Ordering::Release);
        } else {
            // we do not
            self.wait_for_unlock();

            self.set_expand_callback(callback);
        }
    }

    /// allocate reserved memory
    unsafe fn alloc_reserved(&mut self) {
        trace!("allocating reserved memory");
        self.reserved_area = Some(core::slice::from_raw_parts(
            self.heap.allocate_first_fit(self.reserved_layout.unwrap()).expect("couldn't allocate reserved memory").as_ptr(),
            self.reserved_layout.unwrap().size(),
        ));
    }

    /// free reserved memory
    unsafe fn free_reserved(&mut self) {
        trace!("freeing reserved memory");
        self.heap.deallocate((&self.reserved_area.unwrap()[0]).into(), self.reserved_layout.unwrap());
    }

    /// set the layout of reserved memory
    pub fn reserve_memory(&mut self, layout: Option<Layout>) {
        // check if we have the lock
        if !self.lock.swap(true, atomic::Ordering::Acquire) {
            // check whether we need to allocate or free reserved regions
            if layout.is_none() && self.reserved_layout.is_some() && self.reserved_area.is_some() {
                // we had memory allocated, free it
                unsafe {
                    self.free_reserved();
                }

                self.reserved_layout = layout;
            } else if layout.is_some() && self.reserved_layout.is_none() && self.reserved_area.is_none() && self.heap.bottom() != 0 {
                self.reserved_layout = layout;

                // we don't have memory allocated and the heap has been initialized, allocate it
                unsafe {
                    self.alloc_reserved();
                }
            } else {
                self.reserved_layout = layout;
            }

            // release lock
            self.lock.store(false, atomic::Ordering::Release);
        } else {
            // we do not
            self.wait_for_unlock();

            self.reserve_memory(layout);
        }
    }

    fn external_alloc(layout: Layout) -> Result<*mut u8, ()> {
        unsafe { ALLOC_STATE._alloc(layout) }
    }

    fn external_dealloc(ptr: *mut u8, layout: Layout) {
        unsafe {
            ALLOC_STATE._dealloc(ptr, layout);
        }
    }

    fn _alloc(&mut self, layout: Layout) -> Result<*mut u8, ()> {
        match self.heap.allocate_first_fit(layout) {
            Ok(allocation) => Ok(allocation.as_ptr()),
            Err(_) => {
                // make sure we have reserved memory
                self.reserved_area.ok_or(())?;
                self.reserved_layout.ok_or(())?;

                trace!("ran out of heap space, expanding");

                // free our reserved memory to allow for allocations in the callback
                unsafe {
                    self.free_reserved();
                }

                // calculate lower bound for heap expansion
                let new_top = {
                    let align = self.reserved_layout.unwrap().align();
                    (self.heap.top() / align) * align + align // heap top aligned to reserved layout align
                };
                let new_top = {
                    let align = layout.align();
                    (new_top / align) * align + align + layout.size() // heap top aligned to reserved layout align and alloc align plus alloc size
                };

                loop {
                    // allocate memory to expand the heap
                    let new_top_2 = (self.expand_callback)(self.heap.top(), new_top, &Self::external_alloc, &Self::external_dealloc)?;

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

                trace!("heap is now {:#x} - {:#x}", self.heap.bottom(), self.heap.top());

                // allocate new reserved memory
                unsafe {
                    self.alloc_reserved();
                }

                trace!("trying allocation again");

                // try allocating again
                let allocation = self.heap.allocate_first_fit(layout).map(|allocation| allocation.as_ptr());

                trace!("allocation: {:?}", allocation);

                allocation
            }
        }
    }

    fn _dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        unsafe {
            self.heap.deallocate(NonNull::new_unchecked(ptr), layout);
        }
    }

    /// allocates memory
    pub unsafe fn alloc(&mut self, layout: Layout) -> *mut u8 {
        // check if we have the lock
        if !self.lock.swap(true, atomic::Ordering::Acquire) {
            // allocate memory
            let ptr = match self._alloc(layout) {
                Ok(ptr) => ptr,
                Err(_) => {
                    error!("couldn't allocate memory");
                    core::ptr::null_mut()
                }
            };

            // release lock
            self.lock.store(false, atomic::Ordering::Release);

            // return pointer
            ptr
        } else {
            // we do not
            self.wait_for_unlock();

            self.alloc(layout)
        }
    }

    /// frees memory
    pub unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        // check if we have the lock
        if !self.lock.swap(true, atomic::Ordering::Acquire) {
            // free memory
            self._dealloc(ptr, layout);

            // release lock
            self.lock.store(false, atomic::Ordering::Release);
        } else {
            // we do not
            self.wait_for_unlock();

            self.dealloc(ptr, layout);
        }
    }
}

/// the kernel heap itself
static mut ALLOC_STATE: AllocState = AllocState::new();

/// wrapper around AllocState to provide a GlobalAllocator interface
pub struct CustomAlloc;

impl CustomAlloc {
    pub fn init(&self, start: usize, size: usize) {
        unsafe {
            ALLOC_STATE.init(start, size);
        }
    }

    pub fn set_can_spin(&self, can_spin: bool) {
        unsafe {
            ALLOC_STATE.set_can_spin(can_spin);
        }
    }

    pub fn set_expand_callback(&self, callback: &'static AllocExpandCallback) {
        unsafe {
            ALLOC_STATE.set_expand_callback(callback);
        }
    }

    pub fn reserve_memory(&self, layout: Option<Layout>) {
        unsafe {
            ALLOC_STATE.reserve_memory(layout);
        }
    }
}

unsafe impl GlobalAlloc for CustomAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_STATE.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        ALLOC_STATE.dealloc(ptr, layout);
    }
}
