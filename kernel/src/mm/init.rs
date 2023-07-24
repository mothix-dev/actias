//! code to handle memory management initialization

use core::{alloc::Layout, ptr::NonNull};

/// describes the memory map set up by the bootloader and/or platform-specific bringup code
pub struct InitMemoryMap {
    /// the region of memory containing kernel code and data
    pub kernel_area: &'static mut [u8],
    /// the region of memory that bump allocations can happen in
    pub bump_alloc_area: &'static mut [u8],
}

/// simple bump allocator, used for allocating memory necessary for initializing paging and the kernel heap
pub struct BumpAllocator {
    area: &'static mut [u8],
    position: usize,
}

pub struct BumpAllocError;

impl BumpAllocator {
    /// creates a new bump allocator with the given allocation area
    pub fn new(area: &'static mut [u8]) -> Self {
        Self { area, position: 0 }
    }

    /// allocates memory with this bump allocator.
    ///
    /// allocations made with bump allocators cannot be freed, so care must be taken to ensure that
    /// no unnecessary allocations are made
    /// 
    /// # Safety
    /// care has to be taken that memory outside the allocated area isn't accessed, as that results in undefined behavior
    pub unsafe fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, BumpAllocError> {
        let start = self.area.as_ptr().add(self.position);
        let offset = start.align_offset(layout.align());
        let start = start.add(offset);
        let end = start.add(layout.size());

        let slice_end = self.area.as_ptr().add(self.area.len());

        if start >= slice_end || end >= slice_end {
            Err(BumpAllocError)
        } else {
            self.position += offset + layout.size();

            Ok(NonNull::new_unchecked(start as usize as *mut u8))
        }
    }
}
