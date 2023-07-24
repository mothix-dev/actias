pub mod logger;

use core::ptr::addr_of_mut;

use crate::arch::PROPERTIES;

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

    let memory_map = unsafe {
        let start_ptr = addr_of_mut!(kernel_start);
        let end_ptr = addr_of_mut!(kernel_end);

        let kernel_area = core::slice::from_raw_parts_mut(start_ptr, end_ptr.offset_from(start_ptr).try_into().unwrap());
        let bump_alloc_area = core::slice::from_raw_parts_mut(end_ptr, ((LINKED_BASE + 1024 * PROPERTIES.page_size) as *const u8).offset_from(end_ptr).try_into().unwrap());

        crate::mm::InitMemoryMap { kernel_area, bump_alloc_area }
    };

    use log::debug;
    debug!("kernel {}k, alloc {}k", memory_map.kernel_area.len() / 1024, memory_map.bump_alloc_area.len() / 1024);

    unsafe {
        use core::arch::asm;
        asm!("cli; hlt");
    }
}
