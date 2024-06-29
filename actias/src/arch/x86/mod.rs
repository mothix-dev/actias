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

    /// base of the stack
    static stack_base: u8;

    /// top of the stack
    static stack_end: u8;
}

/// ran by boot.S when paging has been successfully initialized
#[no_mangle]
extern "C" fn kmain() {
    logger::init().unwrap();
    info!("HellOwOrld! :3");

    loop {}
}
