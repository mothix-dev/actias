use super::{
    cpu::{CPUThread, ThreadID},
    get_cpus, get_process, remove_process,
    switch::{manual_context_switch, ContextSwitchMode},
    ProcessID,
};
use crate::{
    arch::KERNEL_PAGE_DIR_SPLIT,
    mm::{
        paging::{find_hole, get_kernel_page_dir, get_page_dir, get_page_manager, validate_region, PageDirectory},
        shared::TempMemoryShare,
    },
    task::RegisterQueueEntry,
};
use alloc::vec::Vec;
use common::types::{Errno, MmapAccess, MmapFlags, Result, Syscalls};
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
                    thread.send_message(super::cpu::Message::KillProcess(id.process)).unwrap();

                    let id = ThreadID { core: core_num, thread: thread_num };

                    assert!(crate::arch::send_interrupt_to_cpu(id, crate::arch::MESSAGE_INT), "failed to send interrupt");

                    // inefficient and slow :(
                    trace!("waiting for {id}");
                    while !thread.message_queue.lock().is_empty() {
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

    let priority;
    let is_blocked;
    let message_handlers_clone;

    {
        let process = get_process(id.process).ok_or(Errno::NoSuchProcess)?;
        let thread = process.threads.get(id.thread as usize).ok_or(Errno::NoSuchProcess)?;

        priority = thread.priority;
        is_blocked = thread.is_blocked;

        // TODO: allow clone() to fail gracefully here
        message_handlers_clone = process.message_handlers.clone();
    }

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
            if !page.copy_on_write && !page.shared {
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
            }

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
    }

    // create new process
    trace!("creating new process");
    let process_id = crate::task::create_process(new_fork_page_dir)?;
    let thread_id;

    {
        trace!("getting process");
        let mut process = get_process(process_id).ok_or(Errno::NoSuchProcess)?;

        trace!("adding thread");
        thread_id = process
            .threads
            .add(crate::task::Thread {
                register_queue: super::RegisterQueue::new(RegisterQueueEntry::from_registers(*regs)),
                priority,
                cpu: None,
                is_blocked,
            })
            .map_err(|_| Errno::OutOfMemory)?;

        process.message_handlers = message_handlers_clone;
    }

    // update the page reference counter with our new pages
    for addr in referenced_pages.iter() {
        // FIXME: BTreeMap used in the page ref counter doesn't expect alloc to fail, this can probably crash the kernel if we run out of memory!
        crate::mm::paging::PAGE_REF_COUNTER.lock().add_references(*addr, 2);
    }

    // queue new process for execution
    match super::queue_process(ProcessID {
        process: process_id,
        thread: thread_id as u32,
    }) {
        Ok(_) => Ok(process_id),
        Err(err) => {
            remove_process(process_id);
            Err(err)
        }
    }
}

pub const MINIMUM_MAPPING_ADDR: usize = 0x4000;

fn syscall_mmap(thread: &CPUThread, shm_id: usize, mut start_addr: usize, mut length: usize, flags: usize) -> Result<usize> {
    let page_size = crate::arch::PageDirectory::PAGE_SIZE;

    if (flags >> 8) & 7 == 0 {
        // having neither read nor write nor execute set is completely pointless
        return Err(Errno::InvalidArgument);
    }

    let protection = {
        let mut prot = MmapAccess::from((flags >> 8) as u8); // prot /ref

        if (flags & MmapFlags::CopyOnWrite.bits() as usize) > 0 {
            prot &= !MmapAccess::Write;
        }

        prot
    };
    let flags = MmapFlags::from(flags as u8);

    if (flags & MmapFlags::Anonymous).bits() == 0 && length == 0 {
        length = crate::mm::shared::SHARED_MEMORY_AREAS.lock().get(shm_id).ok_or(Errno::InvalidArgument)?.physical_addresses.len() * page_size;
    }

    // validate arguments
    if length == 0 || start_addr < MINIMUM_MAPPING_ADDR || usize::MAX - start_addr < length {
        trace!("mmap: bad slice @ {start_addr:#x} + {length:#x}");
        return Err(Errno::InvalidArgument);
    }

    // make sure addresses can fit in a usize
    let mut end_addr = start_addr + (length - 1);

    let id = thread.task_queue.lock().current().ok_or(Errno::NoSuchProcess)?.id();

    if (flags & MmapFlags::Fixed).bits() > 0 || (flags & MmapFlags::FixedNoReplace).bits() > 0 {
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

    if (flags & MmapFlags::FixedNoReplace).bits() > 0 {
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
                        writable: (protection & MmapAccess::Write).bits() > 0,
                        copy_on_write: (flags & MmapFlags::CopyOnWrite).bits() > 0,
                        executable: (protection & MmapAccess::Execute).bits() > 0,
                        referenced: shared,
                        shared,
                    }),
                )
                .map_err(|_| Errno::OutOfMemory)?;
        }

        Ok(())
    };

    let num_pages = (end_addr - start_addr + 1) / page_size; // how many pages do we want to map?

    if (flags & MmapFlags::Anonymous).bits() > 0 {
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
    } else if (flags & MmapFlags::CopyOnWrite).bits() > 0 && (flags & MmapFlags::Fixed).bits() > 0 {
        // copy on write and fixed flags are set, mark this mapping as copy on write and unmap the pages that existed there beforehand

        let mut original_pages = Vec::new();

        {
            let process = get_process(id.process).ok_or(Errno::NoSuchProcess)?;

            for addr in (start_addr..=end_addr).step_by(page_size) {
                if let Some(page) = process.page_directory.get_page(addr) {
                    original_pages.try_reserve(1).map_err(|_| Errno::OutOfMemory)?;
                    original_pages.push(page);
                }
            }
        }

        let mut shm_lock = crate::mm::shared::SHARED_MEMORY_AREAS.lock();
        let shm = shm_lock.get_mut(shm_id).ok_or(Errno::InvalidArgument)?;

        // check to see if protection flags are valid
        if (!shm.access & protection).bits() > 0 {
            return Err(Errno::PermissionDenied);
        }

        map_memory(&shm.physical_addresses, true)?;

        shm.references += num_pages;

        for page in original_pages {
            crate::mm::paging::free_page(page);
        }
    } else {
        let mut shm_lock = crate::mm::shared::SHARED_MEMORY_AREAS.lock();
        let shm = shm_lock.get_mut(shm_id).ok_or(Errno::InvalidArgument)?;

        // check to see if protection flags are valid
        if (!shm.access & protection).bits() > 0 {
            return Err(Errno::PermissionDenied);
        }

        map_memory(&shm.physical_addresses, true)?;

        shm.references += num_pages;
    }

    Ok(start_addr)
}

fn syscall_unmap(thread: &CPUThread, address: usize, length: usize) -> Result<usize> {
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

fn syscall_getpid(thread_id: ThreadID, thread: &CPUThread, addr: usize) -> Result<usize> {
    if !validate_region(&get_page_dir(Some(thread_id)), addr, size_of::<ProcessID>()) {
        Err(Errno::BadAddress)
    } else {
        let id = thread.task_queue.lock().current().ok_or(Errno::NoSuchProcess)?.id();

        unsafe {
            *(addr as *mut u8 as *mut ProcessID) = id;
        };

        Ok(0)
    }
}

fn syscall_share_memory(thread: &CPUThread, address: usize, length: usize, access: usize) -> Result<usize> {
    let page_size = crate::arch::PageDirectory::PAGE_SIZE;

    // validate arguments
    if length == 0 || usize::MAX - address < length {
        trace!("share_memory: addr {address:#x} + length {length:#x} would overflow");
        return Err(Errno::InvalidArgument);
    }

    if access & 7 == 0 {
        // having neither read nor write nor execute set is completely pointless
        return Err(Errno::InvalidArgument);
    }

    let access = MmapAccess::from(access as u8);

    let start_addr = (address / page_size) * page_size;
    let end_addr = ((address + (length - 1)) / page_size) * page_size + (page_size - 1);

    if start_addr >= KERNEL_PAGE_DIR_SPLIT || end_addr >= KERNEL_PAGE_DIR_SPLIT {
        trace!("share_memory: aligned area ({start_addr:#x} - {end_addr:#x}) is in kernel memory");
        return Err(Errno::InvalidArgument);
    }

    let id = thread.task_queue.lock().current().ok_or(Errno::NoSuchProcess)?.id();

    let mut entries = TempMemoryShare::new(id, start_addr, end_addr)?;

    for addr in (start_addr..=end_addr).step_by(page_size) {
        entries.add_addr(addr)?;
    }

    entries.share(access).map(|id| id as usize)
}

fn syscall_send_message(thread_id: ThreadID, cpu_thread: &CPUThread, regs: &mut crate::arch::Registers, target: usize, message: usize, data_start: usize, data_len: usize) -> Result<()> {
    // validate arguments
    if target > super::HIGHEST_PROCESS_NUM as usize {
        return Err(Errno::ValueOverflow);
    }

    if message > super::ipc::HIGHEST_MESSAGE_NUM as usize {
        return Err(Errno::ValueOverflow);
    }

    let message = message as u32;

    if data_start == 0 && data_len > 0 {
        return Err(Errno::BadAddress);
    }

    // allocate memory for and copy data
    let data = if data_start > 0 && data_len > 0 {
        if usize::MAX - data_start < data_len {
            return Err(Errno::ValueOverflow);
        }

        if data_start >= KERNEL_PAGE_DIR_SPLIT || (data_start + data_len) >= KERNEL_PAGE_DIR_SPLIT {
            return Err(Errno::InvalidArgument);
        }

        if data_len > crate::arch::PageDirectory::PAGE_SIZE {
            return Err(Errno::InvalidArgument);
        }

        let mut page_dir = get_page_dir(Some(thread_id));

        let addr = get_page_manager().alloc_frame().map_err(|_| Errno::OutOfMemory)?;

        unsafe {
            crate::mm::paging::map_memory(&mut page_dir, &[addr], |s| {
                (&mut s[..data_len]).copy_from_slice(core::slice::from_raw_parts(data_start as *const u8, data_len));
                for i in (&mut s[data_len..]).iter_mut() {
                    *i = 0;
                }
            })?;
        }

        Some((addr, data_len))
    } else {
        None
    };

    // send message
    match super::ipc::send_message(thread_id, cpu_thread, regs, target as u32, message, data) {
        Ok(_) => Ok(()),
        Err(err) => {
            if let Some((addr, _)) = data {
                get_page_manager().set_frame_free(addr);
            }
            Err(err)
        }
    }
}

fn syscall_set_message_handler(thread_id: ThreadID, thread: &CPUThread, message: usize, priority: isize, function_ptr: usize, has_data: bool) -> Result<()> {
    if message > super::ipc::HIGHEST_MESSAGE_NUM as usize {
        return Err(Errno::ValueOverflow);
    }

    let message = message as u32;

    let priority = priority.try_into().map_err(|_| Errno::ValueOverflow)?;

    if !validate_region(&get_page_dir(Some(thread_id)), function_ptr, 1) {
        return Err(Errno::BadAddress);
    }

    let current_pid = thread.task_queue.lock().current().ok_or(Errno::NoSuchProcess)?.id();

    let mut process = get_process(current_pid.process).ok_or(Errno::NoSuchProcess)?;

    // TODO: allow this to fail gracefully
    process.message_handlers.insert(message, super::MessageHandler {
        entry_point: function_ptr,
        priority,
        has_data,
    });

    debug!("set message handler {message} in process {current_pid} to {function_ptr:#x}, priority {priority}");

    Ok(())
}

fn syscall_exit_message_handler(thread: &CPUThread, regs: &mut crate::arch::Registers) -> Result<()> {
    let current_pid = thread.task_queue.lock().current().ok_or(Errno::NoSuchProcess)?.id();

    let mut process = get_process(current_pid.process).ok_or(Errno::NoSuchProcess)?;

    let entry = {
        let thread = process.threads.get_mut(current_pid.thread as usize).ok_or(Errno::NoSuchProcess)?;

        let entry = thread.register_queue.pop().ok_or(Errno::TryAgain)?;

        regs.transfer(&thread.register_queue.current().registers);

        entry
    };

    // can't fail gracefully here anymore, as we've already switched back to whatever the thread was doing previously

    if let Some(addr) = entry.message_data {
        if let Some(page) = process.page_directory.get_page(addr) && process.page_directory.set_page(addr, None).is_ok() {
            crate::mm::paging::free_page(page);
        }
    }

    Ok(())
}

/// low-level syscall handler. handles the parsing, execution, and error handling of syscalls
pub fn syscall_handler(regs: &mut crate::arch::Registers, num: u32, arg0: usize, arg1: usize, arg2: usize, arg3: usize) {
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
            regs.syscall_return(res.map(|id| id as usize));
        }
        Ok(Syscalls::Mmap) => regs.syscall_return(syscall_mmap(thread, arg0, arg1, arg2, arg3)),
        Ok(Syscalls::Unmap) => regs.syscall_return(syscall_unmap(thread, arg0, arg1)),
        Ok(Syscalls::GetProcessID) => regs.syscall_return(syscall_getpid(thread_id, thread, arg0)),
        Ok(Syscalls::ShareMemory) => regs.syscall_return(syscall_share_memory(thread, arg0, arg1, arg2)),
        Ok(Syscalls::SendMessage) => {
            let res = syscall_send_message(thread_id, thread, regs, arg0, arg1, arg2, arg3).map(|_| 0);
            regs.syscall_return(res);
        },
        Ok(Syscalls::MessageHandler) => regs.syscall_return(syscall_set_message_handler(thread_id, thread, arg0, arg1 as isize, arg2, arg3 > 0).map(|_| 0)),
        Ok(Syscalls::ExitMessageHandler) => {
            let res = syscall_exit_message_handler(thread, regs).map(|_| 0);
            if res.is_err() {
                // only return a value if an error occurred, since otherwise the registers could be anything and overwriting them is a terrible idea
                regs.syscall_return(res);
            }
        }
        Err(err) => {
            // invalid syscall, yoink the thread
            let pid = thread.task_queue.lock().current().unwrap().id();
            error!("invalid syscall {num} in process {pid} ({err})");
            exit_current_thread(thread_id, thread, regs);
        }
    }

    thread.leave_kernel();
}
