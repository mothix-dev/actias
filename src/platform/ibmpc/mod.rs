pub mod debug;
pub mod io;
pub mod vga;
pub mod irq;
pub mod bootloader;

use crate::{
    console::{TextConsole, SimpleConsole},
    arch::{
        PAGE_SIZE,
        paging::{alloc_pages_at, free_pages},
    },
};
use alloc::{
    boxed::Box,
    alloc::{Layout, alloc},
};
use bootloader::{FramebufferKind, get_multiboot_info};

pub fn create_console() -> SimpleConsole {
    let mut phys_addr = 0xb8000;
    let mut width = 80;
    let mut height = 25;

    // TODO: detect text console from multiboot info

    /*let info = get_multiboot_info();

    if let Some(buf) = info.get_framebuffer() {
        if buf.kind == FramebufferKind::EGAText {
            phys_addr = buf.addr;
            width = buf.width as usize;
            height = buf.height as usize;
            debug!("got ega text console from bootloader @ {:#x}, {}x{}", phys_addr, width, height);
        }
    }*/

    debug!("mapping video memory");

    // how big is screen memory?
    let buf_size = width * height * core::mem::size_of::<u16>();

    let num_pages =
        if buf_size % PAGE_SIZE != 0 {
            (buf_size + PAGE_SIZE) / PAGE_SIZE
        } else {
            buf_size / PAGE_SIZE
        };
    
    let buf_size_aligned = num_pages * PAGE_SIZE;
    
    debug!("buf size {:#x}, aligned to {:#x}", buf_size, buf_size_aligned);

    let layout = Layout::from_size_align(buf_size_aligned, PAGE_SIZE).unwrap();
    let ptr = unsafe { alloc(layout) };

    assert!(ptr as usize % PAGE_SIZE == 0); // make absolutely sure pointer is page aligned

    // free memory we're going to remap
    free_pages(ptr as usize, num_pages);

    // remap memory
    alloc_pages_at(ptr as usize, num_pages, phys_addr, true, true, true);

    let raw = Box::new(vga::VGAConsole {
        buffer: unsafe { core::slice::from_raw_parts_mut(ptr as *mut u16, width * height) },
        width, height,
    });

    let mut console = SimpleConsole::new(raw, width.try_into().unwrap(), height.try_into().unwrap());

    console.clear();

    console
}
