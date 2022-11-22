//! loading executable formats

use crate::{
    arch::{KERNEL_PAGE_DIR_SPLIT, STACK_SIZE},
    mm::paging::{free_page_dir, get_page_dir, get_page_manager, map_memory, map_memory_from, FreeablePageDir, PageDirectory, PageFrame},
};
use common::types::{Errno, Result};
use core::mem::size_of;
use goblin::elf::{
    program_header::{PT_INTERP, PT_LOAD},
    Elf,
};
use log::{debug, info};

/*
/// spawn a process from the given path
pub fn exec(path: &str, args: &[String], env: &[String]) -> Result<usize, Errno> {
    let mut task = Task::new(0, 0);

    match exec_as(&mut task, path, args, env) {
        Ok(_) => {
            debug!("adding task");
            Ok(add_task(task))
        },
        Err(err) => {
            task.state.free_pages();
            Err(err)
        },
    }
}

/// maximum total size for all command line arguments passed to a program
const ARG_MAX: usize = 0x200000;

/// poke a slice of strings into memory of a task. useful for inserting arguments and environment variables into a task
fn poke_str_slice_into_mem(task: &mut Task, slice: &[String]) -> Result<usize, Errno> {
    // convert slice contents from strings to Vec<u8>s, for easy poking into memory
    let mut bytes = Vec::new();

    #[cfg(target_pointer_width = "64")]
    let mut offsets: Vec<u64> = Vec::new(); // used to calculate pointers to individual arguments in list

    #[cfg(target_pointer_width = "32")]
    let mut offsets: Vec<u32> = Vec::new();

    for str in slice {
        let mut str_bytes = str.as_bytes().to_vec();
        str_bytes.push(0); // make sure it's null terminated, otherwise everything breaks
        if bytes.len() + str_bytes.len() > ARG_MAX {
            return Err(Errno::TooBig);
        }
        offsets.push(bytes.len().try_into().map_err(|_| Errno::ValueOverflow)?);
        bytes.append(&mut str_bytes);
    }

    // prepare args for poking into process memory
    let hole = task.state.find_hole(PAGE_SIZE, bytes.len() + (slice.len() + 1) * size_of::<usize>()).ok_or(Errno::TooBig)?;

    let ptr = hole + bytes.len();

    #[cfg(target_pointer_width = "64")]
    for offset in offsets.iter_mut() {
        *offset += hole as u64;
    }

    #[cfg(target_pointer_width = "32")]
    for offset in offsets.iter_mut() {
        *offset += hole as u32;
    }

    offsets.push(0);

    debug!("offsets: {:?}", offsets);

    let mut temp: Vec<u8> = vec![0; offsets.len() * size_of::<usize>()];

    #[cfg(target_pointer_width = "64")]
    NativeEndian::write_u64_into(&offsets, &mut temp);

    #[cfg(target_pointer_width = "32")]
    NativeEndian::write_u32_into(&offsets, &mut temp);

    bytes.append(&mut temp);

    task.state.write_mem(hole as u64, &bytes, true)?;

    Ok(ptr)
}
*/

#[allow(clippy::vec_init_then_push)]
pub fn exec_as<D: PageDirectory>(mut kernel_page_dir: Option<&mut D>, process: &mut super::Process, data: &[u8]) -> Result<()> {
    let elf = Elf::parse(data).map_err(|_| Errno::ExecutableFormatErr)?;

    if (elf.is_64 && size_of::<usize>() != 64 / 8) || (!elf.is_64 && size_of::<usize>() != 32 / 8) {
        Err(Errno::ExecutableFormatErr)
    } else {
        let mut process_page_dir = FreeablePageDir::new(crate::arch::PageDirectory::new());

        let thread_id = crate::arch::get_thread_id();

        // assemble program in memory
        for ph in elf.program_headers {
            debug!("{:?}", ph);

            match ph.p_type {
                PT_LOAD => {
                    let file_start: usize = ph.p_offset.try_into().map_err(|_| Errno::ValueOverflow)?;
                    let file_end: usize = (ph.p_offset + ph.p_filesz).try_into().map_err(|_| Errno::ValueOverflow)?;

                    let vaddr: usize = ph.p_vaddr.try_into().map_err(|_| Errno::ValueOverflow)?;
                    let filesz: usize = ph.p_filesz.try_into().map_err(|_| Errno::ValueOverflow)?;
                    let memsz: usize = ph.p_memsz.try_into().map_err(|_| Errno::ValueOverflow)?;

                    if vaddr >= KERNEL_PAGE_DIR_SPLIT {
                        return Err(Errno::ValueOverflow);
                    }

                    debug!("data @ {:#x} - {:#x} (filesz {:#x})", ph.p_vaddr, ph.p_vaddr + memsz as u64, filesz);

                    let addr_start = (vaddr / D::PAGE_SIZE) * D::PAGE_SIZE;
                    let addr_end = ((vaddr + memsz) / D::PAGE_SIZE) * D::PAGE_SIZE + (D::PAGE_SIZE - 1);

                    for addr in (addr_start..=addr_end).step_by(D::PAGE_SIZE) {
                        if process_page_dir.get_page(addr).is_none() {
                            let phys = get_page_manager().alloc_frame().map_err(|_| Errno::OutOfMemory)?;

                            process_page_dir
                                .set_page(
                                    addr,
                                    Some(PageFrame {
                                        addr: phys,
                                        user_mode: true,
                                        writable: true,
                                        executable: ph.is_executable(),
                                        present: true,
                                        ..Default::default()
                                    }),
                                )
                                .map_err(|_| {
                                    get_page_manager().set_frame_free(phys);
                                    Errno::OutOfMemory
                                })?;

                            // clear page so we don't leak any information
                            unsafe {
                                let op = |s: &mut [u8]| {
                                    for i in s.iter_mut() {
                                        *i = 0;
                                    }
                                };

                                if let Some(dir) = kernel_page_dir.as_mut() {
                                    map_memory(*dir, &[phys], op).map_err(|_| Errno::OutOfMemory)?;
                                } else {
                                    map_memory(&mut get_page_dir(Some(thread_id)), &[phys], op).map_err(|_| Errno::OutOfMemory)?;
                                }
                            }
                        }
                    }

                    // write data
                    if filesz > 0 {
                        unsafe {
                            #[allow(clippy::needless_range_loop)]
                            let op = |s: &mut [u8]| s.clone_from_slice(&data[file_start..file_end]);

                            if let Some(dir) = kernel_page_dir.as_mut() {
                                map_memory_from(*dir, &mut process_page_dir, vaddr, filesz, op).map_err(|_| Errno::OutOfMemory)?;
                            } else {
                                map_memory_from(&mut get_page_dir(Some(thread_id)), &mut process_page_dir, vaddr, filesz, op).map_err(|_| Errno::OutOfMemory)?;
                            }
                        }
                    }

                    if !ph.is_write() {
                        for addr in (((vaddr + D::PAGE_SIZE - 1) / D::PAGE_SIZE) * D::PAGE_SIZE..=((vaddr + memsz) / D::PAGE_SIZE) * D::PAGE_SIZE).step_by(D::PAGE_SIZE) {
                            let mut page = process_page_dir.get_page(addr).ok_or(Errno::OutOfMemory)?;
                            page.writable = false;
                            process_page_dir.set_page(addr, Some(page)).unwrap();
                        }
                    }
                }
                PT_INTERP => {
                    // TODO: use data given by this header to load interpreter for dynamic linking
                    info!("dynamic linking not supported");
                    return Err(Errno::ExecutableFormatErr);
                }
                _ => debug!("unknown program header {:?}", ph.p_type),
            }
        }

        for addr in (KERNEL_PAGE_DIR_SPLIT - STACK_SIZE..KERNEL_PAGE_DIR_SPLIT).step_by(D::PAGE_SIZE) {
            let phys = get_page_manager().alloc_frame().map_err(|_| Errno::OutOfMemory)?;

            process_page_dir
                .set_page(
                    addr,
                    Some(PageFrame {
                        addr: phys,
                        user_mode: true,
                        writable: true,
                        present: true,
                        ..Default::default()
                    }),
                )
                .map_err(|_| {
                    get_page_manager().set_frame_free(phys);
                    Errno::OutOfMemory
                })?;
        }

        let entry_point = elf.entry.try_into().map_err(|_| Errno::ValueOverflow)?;

        /*debug!("lowest @ {:#x}", lowest_addr);

        debug!("allocating stack");

        // alloc stack for task
        task.state.alloc_page((LINKED_BASE - PAGE_SIZE) as u32, false, true, false);

        /*debug!("preparing environment");
        let args_ptr = poke_str_slice_into_mem(task, args)?;
        let env_ptr = poke_str_slice_into_mem(task, env)?;*/

        debug!("preparing stack");

        // this seems to work for setting up a valid cdecl call frame? honestly idk
        #[cfg(target_pointer_width = "64")]
        let mut stack: Vec<u64> = Vec::new();

        #[cfg(target_pointer_width = "32")]
        let mut stack: Vec<u32> = Vec::new();

        stack.push(0);

        stack.push(args.len().try_into().map_err(|_| Errno::ValueOverflow)?); // argc
        stack.push(args_ptr.try_into().map_err(|_| Errno::ValueOverflow)?); // argv
        stack.push(env_ptr.try_into().map_err(|_| Errno::ValueOverflow)?); // envp

        stack.push(0);
        stack.push(0);

        let num_args = 5;

        let mut data_bytes: Vec<u8> = vec![0; stack.len() * size_of::<usize>()];

        #[cfg(target_pointer_width = "64")]
        NativeEndian::write_u64_into(&stack, &mut data_bytes);

        #[cfg(target_pointer_width = "32")]
        NativeEndian::write_u32_into(&stack, &mut data_bytes);

        let stack_addr = (LINKED_BASE - size_of::<usize>() - stack.len() * size_of::<usize>()) & !(16 - 1); // align to 16 byte boundary

        task.state.write_mem(stack_addr as u64, &data_bytes, true)?;

        // set up registers
        task.state.registers.useresp = stack_addr as u32;
        task.state.registers.esp = stack_addr as u32;
        task.state.registers.ebp = (stack_addr + num_args * size_of::<usize>()) as u32;
        task.state.registers.eip = elf.entry as u32;
        task.state.registers.ds = 0x23;
        task.state.registers.ss = 0x23;
        task.state.registers.cs = 0x1b;
        task.state.registers.eflags = 0b0000001000000010; // enable always set flag and interrupt enable flag*/

        /*
        // i have no idea what the hell is going on or why this all works
        let mut stack: Vec<u32> = vec![
            // whatever you put here seems to not matter at all
            0,
            // arguments go here in the order they show up in the function declaration
        ];

        // push the page directory
        unsafe {
            stack.append(&mut core::slice::from_raw_parts(&kernel_dir_internal as *const _ as *const u32, size_of::<PageDir>() / size_of::<u32>()).to_vec());
        }

        // push the address and number of modules
        stack.push(modules_addr.try_into().unwrap());
        stack.push(new_modules.len().try_into().unwrap());

        // push the address and number of memory map regions
        stack.push(regions_hole.try_into().unwrap());
        stack.push(regions.len().try_into().unwrap());

        // push the highest address that we've touched
        stack.push(highest_addr.try_into().unwrap());

        // assemble the stack
        let data_bytes = unsafe { core::slice::from_raw_parts(stack.as_slice().as_ptr() as *const _ as *const u8, stack.len() * size_of::<u32>()).to_vec() };

        let stack_addr = (stack_top - data_bytes.len()) & !(16 - 1); // align to 16 byte boundary

        debug!("writing stack mem @ {:#x} - {:#x}", stack_addr, stack_addr + data_bytes.len());

        unsafe {
            LOADER_DIR
                .as_mut()
                .unwrap()
                .map_memory_from(&mut kernel_dir, stack_addr, data_bytes.len(), |s| s.clone_from_slice(&data_bytes))
                .expect("failed to populate kernel's stack");
        }
        */

        let stack_end = KERNEL_PAGE_DIR_SPLIT - 1;

        match process.set_page_directory(process_page_dir.into_inner()) {
            Ok(_) => (),
            Err((err, page_dir)) => {
                free_page_dir(&page_dir);
                return Err(err);
            }
        }
        process.remove_all_threads();
        process
            .add_thread(crate::task::Thread {
                register_queue: super::RegisterQueue::new(super::RegisterQueueEntry::from_registers(crate::arch::Registers::new_task(entry_point, stack_end))),
                priority: 0,
                cpu: None,
                is_blocked: false,
            })
            .map_err(|_| Errno::OutOfMemory)?;

        Ok(())
    }
}
