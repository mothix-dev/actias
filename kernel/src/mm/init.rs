//! code to handle memory management initialization

use crate::{
    arch::{PhysicalAddress, PROPERTIES},
    array::BitSet,
    mm::PageManager,
};
use core::{alloc::Layout, ptr::NonNull};
use log::debug;

/// describes the memory map set up by the bootloader and/or platform-specific bringup code
pub struct InitMemoryMap {
    /// the region of memory containing kernel code and data. must be contiguous in both virtual and physical memory
    pub kernel_area: &'static mut [u8],
    /// the physical address of the start of the kernel_area slice
    pub kernel_phys: PhysicalAddress,
    /// the region of memory that bump allocations can happen in. must be contiguous in both virtual and physical memory
    pub bump_alloc_area: &'static mut [u8],
    /// the physical address of the start of the bump_alloc slice
    pub bump_alloc_phys: PhysicalAddress,
}

/// simple bump allocator, used for allocating memory necessary for initializing paging and the kernel heap
pub struct BumpAllocator {
    area: &'static mut [u8],
    position: usize,
}

#[derive(Debug)]
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

        if start >= slice_end || end > slice_end {
            Err(BumpAllocError)
        } else {
            self.position += offset + layout.size();

            Ok(NonNull::new_unchecked(start as usize as *mut u8))
        }
    }

    /// collects the results from an iterator into a slice stored in the bump allocator's allocation area
    pub fn collect_iter<T, I: Iterator<Item = T>>(&mut self, iterator: I) -> Result<&'static [T], BumpAllocError> {
        let size = core::mem::size_of::<T>();
        let align = core::mem::align_of::<T>();

        unsafe {
            // array alignment for a type is the same as a single instance of the type so just align normally
            let start = self.area.as_ptr().add(self.position);
            let offset = start.align_offset(align);
            let start = start.add(offset);

            let slice_end = self.area.as_ptr().add(self.area.len());

            if start >= slice_end {
                return Err(BumpAllocError);
            }

            // dump all the resulting items from the iterator into the allocation area
            let start = start as *mut T;
            let mut len = 0;
            for item in iterator {
                let ptr = start.add(len);

                if ptr as usize >= slice_end as usize || ptr.add(1) as usize > slice_end as usize {
                    return Err(BumpAllocError);
                }

                *ptr = item;
                len += 1;
            }

            self.position += offset + size * len;

            Ok(core::slice::from_raw_parts(start, len))
        }
    }

    pub fn area(&self) -> &[u8] {
        self.area
    }

    /// shrinks the allocation area to only cover what's been allocated so far, returning a slice over the rest of the area
    pub fn shrink(&mut self) -> &'static mut [u8] {
        // this code is Very Bad, however since everything uses static lifetimes (as it basically has to) it's probably fine
        let ptr = self.area.as_mut_ptr();
        let len = self.area.len();

        unsafe {
            self.area = core::slice::from_raw_parts_mut(ptr, self.position);
            core::slice::from_raw_parts_mut(ptr.add(self.position), len - self.position)
        }
    }

    pub fn print_free(&self) {
        debug!(
            "bump allocator: {}k/{}k used, {}% usage",
            self.position / 1024,
            self.area.len() / 1024,
            (self.position * 100) / self.area.len()
        );
    }
}

/// initializes memory management given the initial memory map of the kernel and a way to get the full memory map
pub fn init_memory_manager<I: Iterator<Item = super::MemoryRegion>>(init_memory_map: InitMemoryMap, memory_map_entries: I) {
    let mut bump_alloc = crate::mm::BumpAllocator::new(init_memory_map.bump_alloc_area);
    let slice = bump_alloc.collect_iter(memory_map_entries).unwrap();

    debug!("got {} memory map entries:", slice.len());

    // find highest available available address
    let mut highest_available = 0;
    for region in slice.iter() {
        debug!("    {region:?}");
        if region.kind == crate::mm::MemoryKind::Available {
            highest_available = region.base.saturating_add(region.length);
        }
    }

    // round up to nearest page boundary
    let highest_page = (highest_available as usize + PROPERTIES.page_size - 1) / PROPERTIES.page_size;
    debug!("highest available @ {highest_available:#x} / page {highest_page:#x}");

    let mut set = BitSet::bump_allocate(&mut bump_alloc, highest_page).unwrap();

    // fill the set with true values
    for num in set.array.to_slice_mut().iter_mut() {
        *num = 0xffffffff;
    }
    set.bits_used = set.size;

    let page_size = PROPERTIES.page_size as PhysicalAddress;

    fn set_used(set: &mut BitSet, page_size: PhysicalAddress, base: PhysicalAddress, length: PhysicalAddress) {
        // align the base address down to the nearest page boundary so this entire region is covered
        let base_page = base / page_size;
        let offset = base - (base_page * page_size);
        let len_pages = (length + offset + page_size - 1) / page_size;

        for i in 0..len_pages {
            set.set((base_page + i).try_into().unwrap());
        }
    }

    fn set_free(set: &mut BitSet, page_size: PhysicalAddress, base: PhysicalAddress, length: PhysicalAddress) {
        // align the base address up to the nearest page boundary to avoid overlapping with any unavailable regions
        let base_page = (base + page_size - 1) / page_size;
        let offset = (base_page * page_size) - base;
        let len_pages = (length - offset) / page_size;

        for i in 0..len_pages {
            set.clear((base_page + i).try_into().unwrap());
        }
    }

    // mark all available memory regions from the memory map
    for region in slice.iter() {
        if region.kind == crate::mm::MemoryKind::Available {
            set_free(&mut set, page_size, region.base, region.length);
        }
    }

    // mark the kernel and bump alloc areas as used
    set_used(&mut set, page_size, init_memory_map.kernel_phys, init_memory_map.kernel_area.len().try_into().unwrap());

    let num_reserved = set.bits_used;
    debug!("{num_reserved} pages ({}k) reserved", num_reserved * PROPERTIES.page_size / 1024);

    set_used(&mut set, page_size, init_memory_map.bump_alloc_phys, bump_alloc.area().len().try_into().unwrap());

    let mut manager = PageManager::new(set, PROPERTIES.page_size);
    manager.num_reserved = num_reserved;

    manager.print_free();

    // do things with the bump allocator here...

    bump_alloc.print_free();

    // free any extra memory used by the bump allocator
    let freed_area = bump_alloc.shrink();
    let offset = unsafe { freed_area.as_ptr().offset_from(bump_alloc.area().as_ptr()) };
    let freed_phys = init_memory_map.bump_alloc_phys + offset as PhysicalAddress;

    set_free(&mut manager.frame_set, page_size, freed_phys, freed_area.len().try_into().unwrap());

    manager.print_free();
}
