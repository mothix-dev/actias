pub mod bootloader;
pub mod logger;

use core::ptr::addr_of_mut;

use crate::{
    arch::{PhysicalAddress, PROPERTIES},
    array::BitSet,
    mm::PageManager,
};

/// the address the kernel is linked at
pub const LINKED_BASE: usize = 0xe0000000;

#[allow(unused)]
extern "C" {
    /// start of the kernel's code/data/etc.
    static mut kernel_start: u8;
    /// located at end of loader, used for more efficient memory mappings
    static mut kernel_end: u8;
    /// base of the stack, used to map out the page below to catch stack overflow
    static stack_base: u8;
    /// top of the stack
    static stack_end: u8;
}

/// ran during paging init by boot.S to initialize the page directory that the kernel will be mapped into
#[no_mangle]
pub extern "C" fn x86_prep_page_table(buf: &mut [u32; 1024]) {
    // identity map the first 4 MB (minus the first 128k?) of RAM
    for i in 0u32..1024 {
        buf[i as usize] = (i * PROPERTIES.page_size as u32) | 3; // 3 (0b111) is r/w iirc
    }

    // unmap pages below the stack to try and catch stack overflow
    buf[((unsafe { (&stack_base as *const _) as usize } - LINKED_BASE) / PROPERTIES.page_size) - 1] = 0;
}

/// ran by boot.S when paging has been successfully initialized
#[no_mangle]
pub fn kmain() {
    logger::init().unwrap();
    crate::init_message();

    unsafe {
        if bootloader::mboot_sig != 0x2badb002 {
            panic!("invalid multiboot signature!");
        }
    }

    let mboot_ptr = unsafe { bootloader::mboot_ptr.byte_add(LINKED_BASE) };

    // create initial memory map based on where the kernel is loaded into memory
    let memory_map = unsafe {
        let start_ptr = addr_of_mut!(kernel_start);
        let end_ptr = addr_of_mut!(kernel_end);
        let map_end = (LINKED_BASE + 1024 * PROPERTIES.page_size) as *const u8;

        // sanity checks
        if mboot_ptr as *const _ >= map_end {
            panic!("multiboot structure outside of initially mapped memory");
        } else if mboot_ptr as *const _ >= start_ptr {
            panic!("multiboot structure overlaps with allocated memory");
        }

        let kernel_area = core::slice::from_raw_parts_mut(start_ptr, end_ptr.offset_from(start_ptr).try_into().unwrap());
        let bump_alloc_area = core::slice::from_raw_parts_mut(end_ptr, map_end.offset_from(end_ptr).try_into().unwrap());

        crate::mm::InitMemoryMap {
            kernel_area,
            kernel_phys: start_ptr as PhysicalAddress - LINKED_BASE as PhysicalAddress,
            bump_alloc_area,
            bump_alloc_phys: end_ptr as PhysicalAddress - LINKED_BASE as PhysicalAddress,
        }
    };

    use log::debug;
    debug!("kernel {}k, alloc {}k", memory_map.kernel_area.len() / 1024, memory_map.bump_alloc_area.len() / 1024);

    // create proper memory map from multiboot info
    let mmap_buf = unsafe {
        debug!("multiboot info @ {:?}", mboot_ptr);

        let info = &*mboot_ptr;

        let mmap_addr = info.mmap_addr as usize + LINKED_BASE;
        debug!("{}b of memory mappings @ {mmap_addr:#x}", info.mmap_length);

        core::slice::from_raw_parts(mmap_addr as *const u8, info.mmap_length as usize)
    };

    let entries = core::iter::from_generator(|| {
        let mut offset = 0;
        while offset + core::mem::size_of::<bootloader::MemMapEntry>() <= mmap_buf.len() {
            let entry = unsafe { &*(&mmap_buf[offset] as *const _ as *const bootloader::MemMapEntry) };
            if entry.size == 0 {
                break;
            }

            yield entry;

            offset += entry.size as usize + 4; // the size field isn't counted towards size for some reason?? common gnu L
        }
    });

    // refactor starting here into generic code for all platforms
    let mut bump_alloc = crate::mm::BumpAllocator::new(memory_map.bump_alloc_area);
    let slice = bump_alloc.collect_iter(entries.map(crate::mm::MemoryRegion::from)).unwrap();

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

    debug!("{} pages ({}k) reserved", set.bits_used, set.bits_used * PROPERTIES.page_size / 1024);

    // mark the kernel and bump alloc areas as used
    set_used(&mut set, page_size, memory_map.kernel_phys, memory_map.kernel_area.len().try_into().unwrap());
    set_used(&mut set, page_size, memory_map.bump_alloc_phys, bump_alloc.area().len().try_into().unwrap());

    let mut manager = PageManager::new(set, PROPERTIES.page_size);

    manager.print_free();

    // do things with the bump allocator here...

    bump_alloc.print_free();

    // free any extra memory used by the bump allocator
    let freed_area = bump_alloc.shrink();
    let offset = unsafe { freed_area.as_ptr().offset_from(bump_alloc.area().as_ptr()) };
    let freed_phys = memory_map.bump_alloc_phys + offset as PhysicalAddress;

    set_free(&mut manager.frame_set, page_size, freed_phys, freed_area.len().try_into().unwrap());

    manager.print_free();

    unsafe {
        use core::arch::asm;
        asm!("cli; hlt");
    }
}
