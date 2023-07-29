mod heap;
mod init;
mod paging;

pub use heap::*;
pub use init::*;
use num_traits::Num;
pub use paging::*;

use crate::arch::PhysicalAddress;
use alloc::alloc::{GlobalAlloc, Layout};
use core::{fmt, ops::DerefMut};
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

/// describes a region of physical memory and its use
#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct MemoryRegion {
    /// the base address of this region
    pub base: PhysicalAddress,

    /// the length of this region
    pub length: PhysicalAddress,

    /// how this region should be treated
    pub kind: MemoryKind,
}

impl fmt::Debug for MemoryRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryRegion")
            .field("base", &crate::FormatHex(self.base))
            .field("length", &crate::FormatHex(self.length))
            .field("kind", &self.kind)
            .finish()
    }
}

/// describes what a region of memory is to be used for
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u32)]
pub enum MemoryKind {
    Bad = 0,
    Reserved,
    Available,
}

/// a contiguous region in memory
#[derive(Debug, Copy, Clone)]
pub struct ContiguousRegion<T: Num + Copy> {
    pub base: T,
    pub length: T,
}

impl<T: Num + Copy> ContiguousRegion<T> {
    /// aligns this region to the specified page size so that the resulting region completely covers the original region
    pub fn align_covering(&self, page_size: T) -> Self {
        let base = (self.base / page_size) * page_size;
        let offset = self.base - base;
        let length = ((self.length + offset + page_size - T::one()) / page_size) * page_size;

        Self { base, length }
    }

    /// aligns this region to the specified page size so that the resulting region doesn't exceed the bounds of the original region
    pub fn align_inside(&self, page_size: T) -> Self {
        let base = ((self.base + page_size - T::one()) / page_size) * page_size;
        let offset = base - self.base;
        let length = ((self.length - offset) / page_size) * page_size;

        Self { base, length }
    }
}

impl From<MemoryRegion> for ContiguousRegion<PhysicalAddress> {
    fn from(region: MemoryRegion) -> Self {
        Self {
            base: region.base,
            length: region.length,
        }
    }
}

impl<T> From<&[T]> for ContiguousRegion<usize> {
    fn from(slice: &[T]) -> Self {
        Self {
            base: slice.as_ptr() as *const _ as usize,
            length: slice.len(),
        }
    }
}

impl<T> From<&mut [T]> for ContiguousRegion<usize> {
    fn from(slice: &mut [T]) -> Self {
        Self {
            base: slice.as_ptr() as *const _ as usize,
            length: slice.len(),
        }
    }
}
