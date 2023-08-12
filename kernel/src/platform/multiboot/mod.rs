pub mod bootloader;
pub mod logger;

use crate::{
    arch::{
        bsp::{InterruptManager, RegisterContext},
        interrupts::InterruptRegisters,
        PhysicalAddress, PROPERTIES,
    },
    mm::MemoryRegion,
    platform::bootloader::ModuleEntry,
};
use alloc::{boxed::Box, string::ToString, sync::Arc, vec};
use core::{arch::asm, mem::size_of, ptr::addr_of_mut};
use log::{debug, error, info};
use spin::Mutex;

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
    {
        logger::init().unwrap();
        crate::init_message();
        crate::arch::interrupts::init_pic();

        unsafe {
            if bootloader::mboot_sig != 0x2badb002 {
                panic!("invalid multiboot signature!");
            }
        }

        // create initial memory map based on where the kernel is loaded into memory
        let mut init_memory_map = unsafe {
            let start_ptr = addr_of_mut!(kernel_start);
            let end_ptr = addr_of_mut!(kernel_end);
            let map_end = (LINKED_BASE + 1024 * PROPERTIES.page_size) as *const u8;

            let kernel_area = core::slice::from_raw_parts_mut(start_ptr, end_ptr.offset_from(start_ptr).try_into().unwrap());
            let bump_alloc_area = core::slice::from_raw_parts_mut(end_ptr, map_end.offset_from(end_ptr).try_into().unwrap());

            debug!("kernel {}k (@ {start_ptr:?}), alloc {}k (@ {end_ptr:?})", kernel_area.len() / 1024, bump_alloc_area.len() / 1024);

            crate::mm::InitMemoryMap {
                kernel_area,
                kernel_phys: start_ptr as PhysicalAddress - LINKED_BASE as PhysicalAddress,
                bump_alloc_area,
                bump_alloc_phys: end_ptr as PhysicalAddress - LINKED_BASE as PhysicalAddress,
            }
        };

        let mboot_ptr = unsafe { bootloader::mboot_ptr.byte_add(LINKED_BASE) };

        /// checks whether a block of data is in the kernel or bump alloc area and adjusts the bump alloc area if there's overlap
        fn check_addr(addr: usize, length: usize, init_memory_map: &mut crate::mm::InitMemoryMap, allow_non_mapped: bool) {
            let map_end = LINKED_BASE + 1024 * PROPERTIES.page_size;
            let kernel_start_addr = unsafe { addr_of_mut!(kernel_start) } as usize;
            let kernel_end_addr = unsafe { addr_of_mut!(kernel_end) } as usize;

            // sanity checks
            if (addr < LINKED_BASE || addr >= map_end || addr + length < LINKED_BASE || addr + length >= map_end) && !allow_non_mapped {
                panic!("multiboot structure outside of initially mapped memory");
            } else if (addr >= kernel_start_addr && addr < kernel_end_addr) || (addr + length >= kernel_start_addr && addr + length < kernel_end_addr) {
                panic!("multiboot structure overlaps with kernel memory");
            }

            let bump_alloc_start = init_memory_map.bump_alloc_area.as_ptr() as usize;
            let bump_alloc_len = init_memory_map.bump_alloc_area.len();
            let region = crate::mm::ContiguousRegion { base: addr, length }.align_covering(PROPERTIES.page_size);
            let addr = region.base;
            let length = region.length;

            if addr >= bump_alloc_start && addr - bump_alloc_start < bump_alloc_len {
                let before_data = addr - bump_alloc_start;
                //debug!("{before_data:#x} before");

                let after_data = if addr - bump_alloc_start + length <= bump_alloc_len {
                    bump_alloc_len - (before_data + length)
                } else {
                    0
                };
                //debug!("{after_data:#x} after");

                if after_data >= before_data {
                    let offset = before_data + length;
                    init_memory_map.bump_alloc_phys += offset as u32;
                    let slice = &mut init_memory_map.bump_alloc_area;
                    init_memory_map.bump_alloc_area = unsafe { core::slice::from_raw_parts_mut(slice.as_mut_ptr().add(offset), slice.len() - offset) };
                } else {
                    init_memory_map.bump_alloc_area = unsafe { core::slice::from_raw_parts_mut(init_memory_map.bump_alloc_area.as_mut_ptr(), before_data) };
                }
            } else if addr < bump_alloc_start && addr + length > bump_alloc_start {
                // untested
                let offset = bump_alloc_start - (addr + length);
                let slice = &mut init_memory_map.bump_alloc_area;
                init_memory_map.bump_alloc_area = unsafe { core::slice::from_raw_parts_mut(slice.as_mut_ptr().add(offset), slice.len() - offset) };
            }
        }

        debug!("multiboot info @ {mboot_ptr:?}");
        check_addr(mboot_ptr as usize, size_of::<bootloader::MultibootInfo>(), &mut init_memory_map, false);
        let info = unsafe { &*mboot_ptr };

        // create proper memory map from multiboot info
        let mmap_buf = unsafe {
            let mmap_addr = info.mmap_addr as usize + LINKED_BASE;
            debug!("{}b of memory mappings @ {mmap_addr:#x}", info.mmap_length);
            check_addr(mmap_addr, info.mmap_length as usize, &mut init_memory_map, false);

            core::slice::from_raw_parts(mmap_addr as *const u8, info.mmap_length as usize)
        };

        debug!("cmdline @ {:#x}", info.cmdline as usize + LINKED_BASE);
        check_addr(info.cmdline as usize + LINKED_BASE, 1, &mut init_memory_map, false);

        let cmdline = unsafe {
            core::ffi::CStr::from_ptr((info.cmdline as usize + LINKED_BASE) as *const i8)
                .to_str()
                .expect("kernel command line isn't valid utf8")
        };
        check_addr(info.cmdline as usize + LINKED_BASE, cmdline.len(), &mut init_memory_map, false);

        debug!("cmdline is {cmdline:?}");

        let mods_addr = info.mods_addr as usize + LINKED_BASE;
        debug!("{} module(s) @ {mods_addr:#x}", info.mods_count);

        let initrd_region = if info.mods_count > 0 {
            check_addr(mods_addr, size_of::<ModuleEntry>(), &mut init_memory_map, false);
            let module = unsafe { &*(mods_addr as *const ModuleEntry) };

            let region = crate::mm::ContiguousRegion {
                base: module.mod_start,
                length: module.mod_end - module.mod_start,
            };
            check_addr(region.base as usize + LINKED_BASE, region.length as usize, &mut init_memory_map, true);

            Some(region)
        } else {
            None
        };

        debug!("initrd region is {initrd_region:?}");

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

        debug!("alloc now {}k (@ {:?})", init_memory_map.bump_alloc_area.len() / 1024, init_memory_map.bump_alloc_area.as_ptr());
        let initrd_region = crate::mm::init_memory_manager(init_memory_map, memory_map_entries, cmdline, initrd_region);

        match crate::get_global_state().cmdline.read().parsed.get("log_level").map(|s| s.as_str()) {
            Some("error") => log::set_max_level(log::LevelFilter::Error),
            Some("warn") => log::set_max_level(log::LevelFilter::Warn),
            Some("info") => log::set_max_level(log::LevelFilter::Info),
            Some("debug") => log::set_max_level(log::LevelFilter::Debug),
            Some("trace") => log::set_max_level(log::LevelFilter::Trace),
            _ => (),
        }

        let stack_manager = crate::arch::gdt::init(0x1000 * 8);
        let timer = alloc::sync::Arc::new(crate::timer::Timer::new(10000));
        let interrupt_manager = Arc::new(Mutex::new(crate::arch::InterruptManager::new()));
        let scheduler = crate::sched::Scheduler::new(crate::get_global_state().page_directory.clone(), &timer);
        crate::get_global_state().cpus.write().push(crate::cpu::CPU {
            timer: timer.clone(),
            stack_manager,
            interrupt_manager: interrupt_manager.clone(),
            scheduler: scheduler.clone(),
        });

        {
            let mut manager = interrupt_manager.lock();

            manager.register_aborts(|regs, info| {
                unsafe {
                    asm!("cli");
                }
                error!("unrecoverable exception: {info}");
                info!("register dump: {regs:#?}");
                panic!("unrecoverable exception");
            });
            manager.register_faults(|regs, info| {
                let global_state = crate::get_global_state();
                let scheduler = global_state.cpus.read()[0].scheduler.clone();

                if scheduler.is_running_task(regs) {
                    if let Some(task) = scheduler.get_current_task() {
                        let mut task = task.lock();
                        debug!("exception in process {}: {info}", task.pid.unwrap_or_default());
                        task.exec_mode = crate::sched::ExecMode::Exited;
                    }

                    unsafe {
                        asm!("sti");
                    }
                    scheduler.context_switch(regs);
                } else {
                    error!("exception in kernel mode: {info}");
                    info!("register dump: {regs:#?}");
                    panic!("exception in kernel mode");
                }
            });
            manager.register(crate::arch::interrupts::Exceptions::PageFault as usize, |regs| {
                let fault_addr = unsafe { x86::controlregs::cr2() };
                let error_code = crate::arch::interrupts::PageFaultErrorCode::from(regs.error_code);

                let global_state = crate::get_global_state();
                let scheduler = global_state.cpus.read()[0].scheduler.clone();

                if scheduler.is_running_task(regs) {
                    if let Some(task) = scheduler.get_current_task() {
                        let mut task = task.lock();
                        unsafe {
                            asm!("sti");
                        }
                        if task.memory_map.lock().page_fault(&task.memory_map, fault_addr, error_code.into()) {
                            return;
                        } else {
                            debug!("page fault in process {}", task.pid.unwrap_or_default());
                            task.exec_mode = crate::sched::ExecMode::Exited;
                        }
                    }

                    scheduler.context_switch(regs);
                } else {
                    error!("page fault @ {fault_addr:#x} in kernel mode: {error_code}");
                    info!("register dump: {regs:#?}");
                    panic!("exception in kernel mode");
                }
            });

            // init PIT
            let divisor = 1193182 / timer.hz();

            let l = (divisor & 0xff) as u8;
            let h = ((divisor >> 8) & 0xff) as u8;

            unsafe {
                use x86::io::outb;
                outb(0x43, 0x36);
                outb(0x40, l);
                outb(0x40, h);
            }

            manager.register(0x20, move |regs| timer.tick(regs));

            manager.register(0x80, move |regs| {
                crate::syscalls::syscall_handler(regs, regs.eax, regs.ebx as usize, regs.ecx as usize, regs.edx as usize, regs.edi as usize)
            });

            manager.load_handlers();
        }

        fn every_second() {
            let global_state = crate::get_global_state();

            let total_load_avg: u64 = global_state.cpus.read().iter().map(|cpu| cpu.scheduler.calc_load_avg()).sum();
            info!("load_avg is {}", crate::sched::FixedPoint(total_load_avg, 2));

            for (_pid, process) in global_state.process_table.read().iter() {
                for task in process.threads.write().iter_mut() {
                    task.lock().calc_cpu_time(total_load_avg.try_into().unwrap());
                }
            }
        }

        let timer = &crate::get_global_state().cpus.read()[0].timer;
        let hz = timer.hz();
        timer
            .add_timeout(move |_, jiffies| -> Option<u64> {
                every_second();
                Some(jiffies + hz)
            })
            .expires_at
            .store(0, core::sync::atomic::Ordering::Release);

        let environment = Arc::new(crate::fs::FsEnvironment::new());
        environment
            .namespace
            .write()
            .insert("sysfs".to_string(), Arc::new(crate::fs::KernelFs::new(Box::new(crate::fs::sys::SysFsRoot))));
        environment
            .namespace
            .write()
            .insert("procfs".to_string(), Arc::new(crate::fs::KernelFs::new(Box::new(crate::fs::proc::ProcRoot))));

        if let Some(region) = initrd_region {
            let filesystem = crate::fs::tar::parse_tar(region);
            environment.namespace.write().insert("initrd".to_string(), Arc::new(crate::fs::KernelFs::new(Box::new(filesystem))));

            crate::fs::FsEnvironment::open(
                environment.clone(),
                0,
                "/../initrd".to_string(),
                common::OpenFlags::Read | common::OpenFlags::AtCWD,
                Box::new(move |res, _| assert!(res == Ok(0))),
            );
            environment.chroot(0).unwrap();
            environment.chdir(0).unwrap();
            environment.close(0).unwrap();
        }
        let stack_ptr = (PROPERTIES.kernel_region.base - 1) as *mut u8;

        let global_state = crate::get_global_state();

        crate::fs::FsEnvironment::open(
            environment.clone(),
            0,
            "/init".to_string(),
            common::OpenFlags::Read | common::OpenFlags::AtCWD,
            Box::new(move |res, _| assert!(res == Ok(0))),
        );
        crate::exec::exec(environment.get_open_file(0).unwrap(), Box::new(|res, blocked| todo!()));

        /*let stack_size = 0x1000 * 4;
        let split_addr = crate::arch::PROPERTIES.kernel_region.base;

        {
            let mut map = arc_map.lock();
            map.add_mapping(
                &arc_map,
                crate::mm::Mapping::new(
                    crate::mm::MappingKind::Anonymous,
                    crate::mm::ContiguousRegion::new(split_addr - stack_size, stack_size),
                    crate::mm::MemoryProtection::Read | crate::mm::MemoryProtection::Write,
                ),
                false,
                true,
            )
            .unwrap();
        }

        let task_a = Arc::new(Mutex::new(crate::sched::Task {
            registers: InterruptRegisters::from_fn(entry as *const _, stack_ptr, true),
            niceness: 0,
            exec_mode: crate::sched::ExecMode::Running,
            cpu_time: 0,
            memory_map: arc_map.clone(),
            pid: None,
        }));
        let pid_a = global_state
            .process_table
            .write()
            .insert(crate::process::Process {
                threads: spin::RwLock::new(vec![task_a.clone()]),
                memory_map: arc_map,
                environment,
            })
            .unwrap();
        task_a.lock().pid = Some(pid_a);
        scheduler.push_task(task_a);*/

        //crate::fs::print_tree(&environment.get_fs_list());
    }

    crate::get_global_state().cpus.read()[0].start_context_switching();
}

pub fn get_stack_ptr() -> *mut u8 {
    unsafe { &stack_end as *const _ as usize as *mut u8 }
}
