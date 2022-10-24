use super::{
    cpu::{CPUThread, ThreadID},
    get_cpus, get_process, remove_process,
    switch::{manual_context_switch, ContextSwitchMode},
    ProcessID,
};
use crate::{
    arch::KERNEL_PAGE_DIR_SPLIT,
    mm::paging::{find_hole, get_kernel_page_dir, get_page_dir, get_page_manager, PageDirectory},
};
use alloc::vec::Vec;
use common::types::{Errno, MmapArguments, MmapFlags, MmapProtection, Result, Syscalls};
use core::mem::size_of;
use log::{debug, error, trace};

/// exits the current process, cleans up memory, and performs a context switch to the next process if applicable
pub fn exit_current_process(thread_id: ThreadID, thread: &super::cpu::CPUThread, regs: &mut crate::arch::Registers) {
    let cpus = get_cpus().expect("CPUs not initialized");

    // make sure we're not on the process' page directory
    unsafe {
        crate::mm::paging::get_kernel_page_dir().switch_to();
    }

    let id = thread.task_queue.lock().current().unwrap().id();
    let num_threads = get_process(id.process).unwrap().threads.num_entries();

    debug!("exiting process {}", id.process);

    // perform context switch so we're not on this thread anymore
    manual_context_switch(thread.timer, Some(thread_id), regs, ContextSwitchMode::Remove);

    // remove any more threads of the process
    thread.task_queue.lock().remove_process(id.process);

    if num_threads > 1 && cpus.cores.len() > 1 {
        // tell all other CPUs to kill this process
        for (core_num, core) in cpus.cores.iter().enumerate() {
            for (thread_num, thread) in core.threads.iter().enumerate() {
                if (thread_id.core != core_num || thread_id.thread != thread_num) && thread.has_started() {
                    thread.kill_queue.lock().push_back(super::cpu::KillQueueEntry::Process(id.process));

                    let id = ThreadID { core: core_num, thread: thread_num };

                    assert!(crate::arch::send_interrupt_to_cpu(id, crate::arch::KILL_PROCESS_INT), "failed to send interrupt");

                    // inefficient and slow :(
                    trace!("waiting for {id}");
                    while !thread.kill_queue.lock().is_empty() {
                        crate::arch::spin();
                    }
                }
            }
        }
    }

    remove_process(id.process);
}

/// exits current thread, calls exit_current_process if it's the last remaining thread
pub fn exit_current_thread(thread_id: ThreadID, thread: &CPUThread, regs: &mut crate::arch::Registers) {
    debug!("exiting current thread");
    let id = thread.task_queue.lock().current().unwrap().id();
    let num_threads = get_process(id.process).unwrap().threads.num_entries();

    if num_threads > 1 {
        manual_context_switch(thread.timer, Some(thread_id), regs, ContextSwitchMode::Remove);
    } else {
        exit_current_process(thread_id, thread, regs);
    }
}

/// forks the current process, returning the ID of the newly created process
pub fn fork_current_process(thread: &CPUThread, regs: &mut crate::arch::Registers) -> Result<u32> {
    debug!("forking current process");

    let id = thread.task_queue.lock().current().ok_or(Errno::NoSuchProcess)?.id();

    let cpus = get_cpus().expect("CPUs not initialized");
    let process = get_process(id.process).ok_or(Errno::NoSuchProcess)?;
    let thread = process.threads.get(id.thread as usize).ok_or(Errno::NoSuchProcess)?;

    let priority = thread.priority;
    let is_blocked = thread.is_blocked;

    drop(process);

    // copy page directory
    trace!("copying page directory");
    let mut new_orig_page_dir = crate::arch::PageDirectory::new();
    let mut new_fork_page_dir = crate::arch::PageDirectory::new();
    let mut referenced_pages = Vec::new();

    debug!(
        "new_orig_page_dir @ {:#x}, new_fork_page_dir @ {:#x}",
        new_orig_page_dir.tables_physical_addr, new_fork_page_dir.tables_physical_addr
    );

    let page_size = crate::arch::PageDirectory::PAGE_SIZE;

    for addr in (0..KERNEL_PAGE_DIR_SPLIT).step_by(page_size) {
        let mut page = get_process(id.process).ok_or(Errno::NoSuchProcess)?.page_directory.get_page(addr);

        // does this page exist?
        if let Some(page) = page.as_mut() {
            trace!("modifying page {addr:#x} (phys {:#x})", page.addr);
            // if this page is writable, set it as non-writable and set it to copy on write
            //
            // pages have to be set as non writable in order for copy on write to work since attempting to write to a non writable page causes a page fault exception,
            // which we can then use to copy the page and resume execution as normal
            if page.writable {
                page.writable = false;
                page.copy_on_write = true;
                page.referenced = true;
            }

            // add this page's address to our list of referenced pages
            referenced_pages.try_reserve(1).map_err(|_| Errno::OutOfMemory)?;
            referenced_pages.push(page.addr);

            // set page in page directories
            trace!("setting page");
            new_orig_page_dir.set_page(addr, Some(*page)).map_err(|_| Errno::OutOfMemory)?;
            new_fork_page_dir.set_page(addr, Some(*page)).map_err(|_| Errno::OutOfMemory)?;
        }
    }

    // update the page directory of the process we're forking from
    unsafe {
        let mut process = get_process(id.process).ok_or(Errno::NoSuchProcess)?;

        get_kernel_page_dir().switch_to();

        let old_page_directory = core::mem::replace(&mut process.page_directory.task, new_orig_page_dir);

        match process.page_directory.force_sync() {
            Ok(_) => (),
            Err(err) => {
                error!("failed to synchronize page directory for forking process: {err:?}");
                process.page_directory.task = old_page_directory;
                process.page_directory.switch_to();
                return Err(Errno::OutOfMemory);
            }
        }

        process.page_directory.switch_to();

        drop(process);
    }

    // create new process
    trace!("creating new process");
    let process_id = crate::task::create_process(new_fork_page_dir)?;

    trace!("getting process");
    let mut process = get_process(process_id).ok_or(Errno::NoSuchProcess)?;

    trace!("adding thread");
    process
        .threads
        .add(crate::task::Thread {
            registers: *regs,
            priority,
            cpu: None,
            is_blocked,
        })
        .map_err(|_| Errno::OutOfMemory)?;

    // release lock
    drop(process);

    // update the page reference counter with our new pages
    for addr in referenced_pages.iter() {
        // FIXME: BTreeMap used in the page ref counter doesn't expect alloc to fail, this can probably crash the kernel if we run out of memory!
        crate::mm::paging::PAGE_REF_COUNTER.lock().add_references(*addr, 2);
    }

    // queue new process for execution
    let thread_id = cpus.find_thread_to_add_to().unwrap_or_default();

    debug!("queueing forked process on CPU {thread_id}");
    match cpus.get_thread(thread_id) {
        Some(thread) => match thread
            .task_queue
            .lock()
            .insert(crate::task::queue::TaskQueueEntry::new(ProcessID { process: process_id, thread: 1 }, 0))
        {
            Ok(_) => Ok(process_id),
            Err(err) => {
                remove_process(process_id);
                Err(err)
            }
        },
        None => {
            remove_process(process_id);
            Err(Errno::NoSuchProcess)
        }
    }
}

pub const MINIMUM_MAPPING_ADDR: usize = 0x4000;

fn syscall_mmap(thread_id: ThreadID, thread: &CPUThread, addr: usize) -> Result<u32> {
    if !validate_region(thread_id, addr, size_of::<MmapArguments>()) {
        Err(Errno::BadAddress)
    } else {
        let page_size = crate::arch::PageDirectory::PAGE_SIZE;
        let mut args = unsafe { &mut *(addr as *mut u8 as *mut MmapArguments) };

        if (args.flags & MmapFlags::Anonymous).bits() == 0 && args.length == 0 {
            args.length = crate::mm::shared::SHARED_MEMORY_AREAS
                .lock()
                .get(args.id as usize)
                .ok_or(Errno::InvalidArgument)?
                .physical_addresses
                .len() as u64
                * page_size as u64;
        }

        // validate arguments
        if args.length == 0 || args.address < MINIMUM_MAPPING_ADDR as u64 || u64::MAX - args.address < args.length {
            trace!("mmap: bad args {args:?}");
            return Err(Errno::InvalidArgument);
        }

        // make sure addresses can fit in a usize
        let mut start_addr: usize = args.address.try_into().map_err(|_| Errno::InvalidArgument)?;
        let mut end_addr: usize = (args.address + (args.length - 1)).try_into().map_err(|_| Errno::InvalidArgument)?;

        let id = thread.task_queue.lock().current().ok_or(Errno::NoSuchProcess)?.id();

        if (args.flags & MmapFlags::Fixed).bits() > 0 || (args.flags & MmapFlags::FixedNoReplace).bits() > 0 {
            // make sure address is page aligned if moving the address around isn't allowed
            if start_addr % page_size != 0 || end_addr % page_size != 0 {
                trace!("mmap: fixed flag set and address isn't aligned");
                return Err(Errno::InvalidArgument);
            }
        } else {
            // round addresses to nearest multiple of page size, ensuring the resulting region completely covers the provided region
            start_addr = (start_addr / page_size) * page_size;
            end_addr = (end_addr / page_size) * page_size + (page_size - 1); // - 1 to account for top of address space

            let len = end_addr - start_addr;

            let process = get_process(id.process).ok_or(Errno::NoSuchProcess)?;

            // if this mapping would overwrite existing memory, try to find somewhere where it wouldn't
            if let Some(hole) = find_hole(&process.page_directory, start_addr, KERNEL_PAGE_DIR_SPLIT, len) {
                start_addr = hole;
                end_addr = hole + len;
            } else if let Some(hole) = find_hole(&process.page_directory, MINIMUM_MAPPING_ADDR, start_addr, len) {
                start_addr = hole;
                end_addr = hole + len;
            } else {
                // we can't, give up
                return Err(Errno::OutOfMemory);
            }
        }

        // make sure we're not touching kernel memory
        if start_addr >= KERNEL_PAGE_DIR_SPLIT || end_addr >= KERNEL_PAGE_DIR_SPLIT {
            trace!("mmap: aligned area ({start_addr:#x} - {end_addr:#x}) is in kernel memory");
            return Err(Errno::InvalidArgument);
        }

        debug!("mapping memory ({start_addr:#x} - {end_addr:#x})");

        if (args.flags & MmapFlags::FixedNoReplace).bits() > 0 {
            let process = get_process(id.process).ok_or(Errno::NoSuchProcess)?;

            // make sure pages won't be replaced here
            for addr in (start_addr..=end_addr).step_by(page_size) {
                if process.page_directory.get_page(addr).is_some() {
                    // can't replace pages!
                    return Err(Errno::Exists);
                }
            }
        }

        let map_memory = |physical_addresses: &[u64], shared: bool| -> Result<()> {
            for (index, addr) in (start_addr..=end_addr).step_by(page_size).enumerate() {
                if index > physical_addresses.len() {
                    break;
                }

                get_process(id.process)
                    .ok_or(Errno::NoSuchProcess)?
                    .page_directory
                    .set_page(
                        addr,
                        Some(crate::mm::paging::PageFrame {
                            addr: physical_addresses[index],
                            present: true,
                            user_mode: true,
                            writable: (args.protection & MmapProtection::Write).bits() > 0,
                            executable: (args.protection & MmapProtection::Execute).bits() > 0,
                            referenced: shared,
                            shared,
                            ..Default::default()
                        }),
                    )
                    .map_err(|_| Errno::OutOfMemory)?;
            }

            Ok(())
        };

        let num_pages = (end_addr - start_addr + 1) / page_size; // how many pages do we want to map?

        if (args.flags & MmapFlags::Anonymous).bits() > 0 {
            // anonymous flag is set, map in new memory

            let mut physical_addresses = Vec::new();
            physical_addresses.try_reserve(num_pages).map_err(|_| Errno::OutOfMemory)?;

            // allocate new memory
            for _i in 0..num_pages {
                match get_page_manager().alloc_frame() {
                    Ok(addr) => physical_addresses.push(addr),
                    Err(_) => {
                        // free memory we're not gonna be using
                        for addr in physical_addresses.iter() {
                            get_page_manager().set_frame_free(*addr);
                        }

                        return Err(Errno::OutOfMemory);
                    }
                }
            }

            map_memory(&physical_addresses, false)?;

            // zero out new mapping
            let slice = unsafe { core::slice::from_raw_parts_mut(start_addr as *mut u8, end_addr - start_addr) };
            for i in slice.iter_mut() {
                *i = 0;
            }
        } else {
            let mut shm_lock = crate::mm::shared::SHARED_MEMORY_AREAS.lock();
            let shm = shm_lock.get_mut(args.id as usize).ok_or(Errno::InvalidArgument)?;

            map_memory(&shm.physical_addresses, true)?;

            shm.references += num_pages;
        }

        args.address = start_addr as u64;
        args.length = (end_addr - start_addr) as u64;

        Ok(0)
    }
}

fn syscall_unmap(thread: &CPUThread, address: usize, length: usize) -> Result<u32> {
    let page_size = crate::arch::PageDirectory::PAGE_SIZE;

    // validate arguments
    if length == 0 || usize::MAX - address < length {
        trace!("unmap: addr {address:#x} + length {length:#x} would overflow");
        return Err(Errno::InvalidArgument);
    }

    // round addresses to nearest multiple of page size
    let start_addr = (address / page_size) * page_size;
    let end_addr = ((address + (length - 1)) / page_size) * page_size + (page_size - 1); // - 1 to account for top of address space

    // make sure we're not touching kernel memory
    if start_addr >= KERNEL_PAGE_DIR_SPLIT || end_addr >= KERNEL_PAGE_DIR_SPLIT {
        trace!("unmap: aligned area ({start_addr:#x} - {end_addr:#x}) is in kernel memory");
        return Err(Errno::InvalidArgument);
    }

    debug!("unmapping memory ({start_addr:#x} - {end_addr:#x})");

    let id = thread.task_queue.lock().current().ok_or(Errno::NoSuchProcess)?.id();
    let mut process = get_process(id.process).ok_or(Errno::NoSuchProcess)?;

    // unmap memory
    for addr in (start_addr..=end_addr).step_by(page_size) {
        if let Some(page) = process.page_directory.get_page(addr) {
            process.page_directory.set_page(addr, None).map_err(|_| Errno::OutOfMemory)?;

            crate::mm::paging::free_page(page);
        }
    }

    Ok(0)
}

fn syscall_getpid(thread_id: ThreadID, thread: &CPUThread, addr: usize) -> Result<u32> {
    if !validate_region(thread_id, addr, size_of::<ProcessID>()) {
        Err(Errno::BadAddress)
    } else {
        let id = thread.task_queue.lock().current().ok_or(Errno::NoSuchProcess)?.id();

        unsafe {
            *(addr as *mut u8 as *mut ProcessID) = id;
        };

        Ok(0)
    }
}

fn syscall_share_memory(thread_id: ThreadID, thread: &CPUThread, address: usize, length: usize) -> Result<u32> {
    let page_size = crate::arch::PageDirectory::PAGE_SIZE;

    // validate arguments
    if length == 0 || usize::MAX - address < length {
        trace!("share_memory: addr {address:#x} + length {length:#x} would overflow");
        return Err(Errno::InvalidArgument);
    }

    let start_addr = (address / page_size) * page_size;
    let end_addr = ((address + (length - 1)) / page_size) * page_size + (page_size - 1);

    if start_addr >= KERNEL_PAGE_DIR_SPLIT || end_addr >= KERNEL_PAGE_DIR_SPLIT {
        trace!("share_memory: aligned area ({start_addr:#x} - {end_addr:#x}) is in kernel memory");
        return Err(Errno::InvalidArgument);
    }

    let mut phys_addresses = Vec::new();
    phys_addresses.try_reserve((end_addr - start_addr + 1) / page_size).map_err(|_| Errno::OutOfMemory)?;

    let id = thread.task_queue.lock().current().ok_or(Errno::NoSuchProcess)?.id();

    for addr in (start_addr..=end_addr).step_by(page_size) {
        let mut page = get_process(id.process).ok_or(Errno::NoSuchProcess)?.page_directory.get_page(addr).ok_or(Errno::BadAddress)?;

        if page.shared {
            return Err(Errno::BadAddress);
        } else if !page.writable && page.copy_on_write && page.referenced {
            crate::mm::paging::copy_on_write(thread_id, thread, addr)?;
            page = get_process(id.process).ok_or(Errno::NoSuchProcess)?.page_directory.get_page(addr).ok_or(Errno::BadAddress)?;
        }

        phys_addresses.push(page.addr);

        page.shared = true;
        get_process(id.process).ok_or(Errno::NoSuchProcess)?.page_directory.set_page(addr, Some(page)).map_err(|_| Errno::OutOfMemory)?;
    }

    crate::mm::shared::share_area(&phys_addresses)
}

fn validate_region(thread_id: ThreadID, start: usize, len: usize) -> bool {
    let page_size = crate::arch::PageDirectory::PAGE_SIZE;
    let start = (start / page_size) * page_size;
    let end = ((start + len) / page_size) * page_size + page_size;

    let page_dir = get_page_dir(Some(thread_id));
    for addr in (start..end).step_by(page_size) {
        if page_dir.get_page(addr).is_none() {
            return false;
        }
    }

    true
}

/// low-level syscall handler. handles the parsing, execution, and error handling of syscalls
pub fn syscall_handler(regs: &mut crate::arch::Registers, num: u32, arg0: usize, arg1: usize, _arg2: usize) {
    let thread_id = crate::arch::get_thread_id();
    let cpus = get_cpus().expect("CPUs not initialized");
    let thread = cpus.get_thread(thread_id).expect("couldn't get CPU thread");

    thread.check_enter_kernel();

    match regs.task_sanity_check() {
        Ok(_) => (),
        Err(err) => {
            let pid = thread.task_queue.lock().current().unwrap().id();
            error!("process {pid} failed sanity check: {err:?}");
            exit_current_thread(thread_id, thread, regs);
        }
    }

    let syscall = Syscalls::try_from(num);
    trace!("(CPU {thread_id}) process got syscall {syscall:?}");
    match syscall {
        Ok(Syscalls::IsComputerOn) => regs.syscall_return(Ok(1)),
        Ok(Syscalls::ExitProcess) => exit_current_process(thread_id, thread, regs),
        Ok(Syscalls::ExitThread) => exit_current_thread(thread_id, thread, regs),
        Ok(Syscalls::Fork) => {
            // whatever we put here will end up in the newly forked process, since we're gonna be overwriting these values in the original process
            //
            // the fork syscall should return 0 in the child process and the PID of the child process in the parent
            regs.syscall_return(Ok(0));

            let res = fork_current_process(thread, regs);
            regs.syscall_return(res);
        }
        Ok(Syscalls::Mmap) => regs.syscall_return(syscall_mmap(thread_id, thread, arg0)),
        Ok(Syscalls::Unmap) => regs.syscall_return(syscall_unmap(thread, arg0, arg1)),
        Ok(Syscalls::GetProcessID) => regs.syscall_return(syscall_getpid(thread_id, thread, arg0)),
        Ok(Syscalls::ShareMemory) => regs.syscall_return(syscall_share_memory(thread_id, thread, arg0, arg1)),
        Err(err) => {
            // invalid syscall, yoink the thread
            let pid = thread.task_queue.lock().current().unwrap().id();
            error!("invalid syscall {num} in process {pid} ({err})");
            exit_current_thread(thread_id, thread, regs);
        }
    }

    thread.leave_kernel();
}
