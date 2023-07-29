mod heap;
mod init;
mod paging;
mod regions;

pub use heap::*;
pub use init::*;
pub use paging::*;
pub use regions::*;

use alloc::alloc::{GlobalAlloc, Layout};
use core::ops::DerefMut;
use log::error;
use spin::Mutex;

pub enum AllocState {
    None,
    BumpAlloc(BumpAllocator),
    Heap(HeapAllocator<<crate::arch::PageDirectory as paging::PageDirectory>::Reserved>),
}

pub struct CustomAlloc(pub Mutex<AllocState>);

unsafe impl GlobalAlloc for CustomAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut state = self.0.lock();
        match state.deref_mut() {
            AllocState::None => panic!("can't allocate before allocator init"),
            AllocState::BumpAlloc(allocator) => match allocator.alloc(layout) {
                Ok(ptr) => ptr.as_ptr(),
                Err(_) => core::ptr::null_mut(),
            },
            AllocState::Heap(allocator) => match allocator.alloc(layout) {
                Ok(ptr) => ptr.as_ptr(),
                Err(_) => core::ptr::null_mut(),
            },
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut state = self.0.lock();
        match state.deref_mut() {
            AllocState::Heap(allocator) => allocator.dealloc(ptr, layout),
            _ => error!("can't free ({layout:?} @ {ptr:?})"),
        }
        
    }
}

/// our global allocator
#[global_allocator]
pub static ALLOCATOR: CustomAlloc = CustomAlloc(Mutex::new(AllocState::None));

/// run if the allocator encounters an error. not much we can do other than panic
#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    panic!("allocation error with layout {:?}", layout);
}
