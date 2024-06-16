#[cfg(not(target_arch = "x86"))]
compile_error!("multiboot platform only supports the x86 architecture");

mod logger;

use log::info;

/// the address the kernel is linked at
pub const LINKED_BASE: usize = 0xe000_0000;

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
        buf[i as usize] = (i * 4096) | 3; // 3 (0b111) is r/w iirc
    }

    // unmap pages below the stack to try and catch stack overflow
    buf[((unsafe { (&stack_base as *const _) as usize } - LINKED_BASE) / 4096) - 1] = 0;
}

/// ran by boot.S when paging has been successfully initialized
#[no_mangle]
pub extern "C" fn kmain() {
    logger::init().unwrap();
    info!("HellOwOrld! :3");

    loop {}
}
