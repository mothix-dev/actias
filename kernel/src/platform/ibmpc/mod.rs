pub mod bootloader;
pub mod debug;
pub mod irq;
pub mod keyboard;
pub mod vga;

use crate::{
    arch::{
        paging::{alloc_pages_at, free_pages},
        PAGE_SIZE,
    },
    console::{SimpleConsole, TextConsole},
};
use alloc::{
    alloc::{alloc, Layout},
    boxed::Box,
};
use bootloader::{get_multiboot_info, FramebufferKind};
use log::{error, warn, info, debug, trace};

/// creates a system console
pub fn create_console() -> SimpleConsole {
    let mut phys_addr = 0xb8000;
    let mut width = 80;
    let mut height = 25;

    let info = get_multiboot_info();

    if let Some(buf) = info.framebuffer.as_ref() {
        if buf.kind == FramebufferKind::EGAText {
            phys_addr = buf.addr;
            width = buf.width as usize;
            height = buf.height as usize;
            info!("got ega text console from bootloader @ {:#x}, {}x{}", phys_addr, width, height);
        } else {
            warn!("unsupported framebuffer, using defaults");
        }
    } else {
        info!("no framebuffer info supplied, using defaults");
    }

    info!("mapping video memory");

    // how big is screen memory?
    let buf_size = width * height * core::mem::size_of::<u16>();

    // align the buffer size to a page boundary
    let num_pages = if buf_size % PAGE_SIZE != 0 {
        (buf_size + PAGE_SIZE) / PAGE_SIZE
    } else {
        buf_size / PAGE_SIZE
    };

    let buf_size_aligned = num_pages * PAGE_SIZE;

    debug!("buf size {:#x}, aligned to {:#x}",buf_size, buf_size_aligned);

    // allocate heap memory that we can then remap to screen memory. we need it to be aligned to a page boundary so we can swap out the pages and replace them with our own
    let layout = Layout::from_size_align(buf_size_aligned, PAGE_SIZE).unwrap();
    let ptr = unsafe { alloc(layout) };

    assert!(ptr as usize % PAGE_SIZE == 0); // make absolutely sure pointer is page aligned

    // free pages we're going to remap
    free_pages(ptr as usize, num_pages);

    // remap memory
    alloc_pages_at(ptr as usize, num_pages, phys_addr, true, true, true);

    // create console
    let raw = Box::new(vga::VGAConsole {
        buffer: unsafe { core::slice::from_raw_parts_mut(ptr as *mut u16, width * height) },
        width,
        height,
    });

    let mut console =
        SimpleConsole::new(raw, width.try_into().unwrap(), height.try_into().unwrap());

    console.clear();

    console
}

/// gets a reference to initrd data if one exists
pub fn get_initrd() -> Option<&'static [u8]> {
    let info = crate::platform::bootloader::get_multiboot_info();

    info.mods.as_ref().map(|m| m[0].data())
}
