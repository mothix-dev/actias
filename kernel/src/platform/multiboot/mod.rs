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
    crate::arch::interrupts::init_pic();

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

    use crate::arch::bsp::InterruptManager;
    let mut manager = crate::arch::InterruptManager::new();

    use log::{error, info};
    manager.register_aborts(|regs, info| {
        error!("unrecoverable exception: {info}");
        info!("register dump: {regs:#?}");
        panic!("unrecoverable exception");
    });
    manager.register_faults(|regs, info| {
        error!("exception in kernel mode: {info}");
        info!("register dump: {regs:#?}");
        panic!("exception in kernel mode");
    });

    let hz = 1000;

    // init PIT
    let divisor = 1193180 / hz;

    let l = (divisor & 0xff) as u8;
    let h = ((divisor >> 8) & 0xff) as u8;

    unsafe {
        use x86::io::outb;
        outb(0x43, 0x36);
        outb(0x40, l);
        outb(0x40, h);
    }

    let timer = alloc::sync::Arc::new(crate::tasks::Timer::new(hz));

    {
        let timer = timer.clone();
        manager.register(0x20, move |_| timer.tick());
    }

    manager.load_handlers();

    /*manager.register(crate::arch::interrupts::Exceptions::Breakpoint as usize, move |_| info!("breakpoint :333"));

    unsafe {
        debug!("TEST: making huge allocation");
        let uwu = alloc::alloc::alloc(alloc::alloc::Layout::from_size_align(0x1000 * 1024, 1).unwrap());
        debug!("got {uwu:?}");
    }

    unsafe {
        use core::arch::asm;
        debug!("TEST: breakpoint exception");
        asm!("int3");
    }

    unsafe {
        debug!("TEST: page fault");
        *(1 as *mut u8) = 0;
    }*/

    unsafe {
        crate::tasks::init();
    }

    crate::tasks::get_cpu_executor().spawn(async move {
        info!("UwU");
        timer.timeout_in(timer.hz()).unwrap().await;
        info!("OwO");
    });

    //crate::tasks::get_cpu_executor().run();

    loop {
        (crate::arch::PROPERTIES.wait_for_interrupt)();
    }
}

pub fn get_stack_ptr() -> *mut u8 {
    unsafe { &stack_end as *const _ as usize as *mut u8 }
}
