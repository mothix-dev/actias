pub mod bootloader;
pub mod ints;
pub mod logger;

/// initialize paging, just cleanly map our kernel to 3gb
#[no_mangle]
pub extern "C" fn x86_prep_page_table(buf: &mut [u32; 1024]) {
    for i in 0u32..1024 {
        buf[i as usize] = i * 0x1000 + 3;
    }
}
