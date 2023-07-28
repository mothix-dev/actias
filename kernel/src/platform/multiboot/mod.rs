pub mod bootloader;
pub mod logger;

use core::ptr::addr_of_mut;

use crate::{
    arch::{PhysicalAddress, PROPERTIES},
    mm::MemoryRegion,
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
    let init_memory_map = unsafe {
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
    debug!("kernel {}k, alloc {}k", init_memory_map.kernel_area.len() / 1024, init_memory_map.bump_alloc_area.len() / 1024);

    // create proper memory map from multiboot info
    let mmap_buf = unsafe {
        debug!("multiboot info @ {:?}", mboot_ptr);

        let info = &*mboot_ptr;

        let mmap_addr = info.mmap_addr as usize + LINKED_BASE;
        debug!("{}b of memory mappings @ {mmap_addr:#x}", info.mmap_length);

        core::slice::from_raw_parts(mmap_addr as *const u8, info.mmap_length as usize)
    };

    let memory_map_entries = core::iter::from_generator(|| {
        let mut offset = 0;
        while offset + core::mem::size_of::<bootloader::MemMapEntry>() <= mmap_buf.len() {
            let entry = unsafe { &*(&mmap_buf[offset] as *const _ as *const bootloader::MemMapEntry) };
            if entry.size == 0 {
                break;
            }

            yield MemoryRegion::from(entry);

            offset += entry.size as usize + 4; // the size field isn't counted towards size for some reason?? common gnu L
        }
    });

    crate::mm::init_memory_manager(init_memory_map, memory_map_entries);

    unsafe {
        use core::arch::asm;
        asm!("cli; hlt");
    }
}
