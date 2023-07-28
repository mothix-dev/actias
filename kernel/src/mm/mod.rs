mod init;
mod paging;
mod regions;

pub use init::*;
pub use paging::*;
pub use regions::*;

use alloc::alloc::{GlobalAlloc, Layout};

pub struct CustomAlloc;

unsafe impl GlobalAlloc for CustomAlloc {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        panic!("can't allocate");
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        panic!("can't free");
    }
}

/// our global allocator
#[global_allocator]
pub static ALLOCATOR: CustomAlloc = CustomAlloc;

/// run if the allocator encounters an error. not much we can do other than panic
#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    panic!("allocation error with layout {:?}", layout);
}
