//! smooth brain scheduler

pub mod cpu;
pub mod exec;
pub mod queue;
pub mod switch;
pub mod syscalls;

use crate::{
    arch::{Registers, KERNEL_PAGE_DIR_SPLIT},
    mm::{
        paging::{get_kernel_page_dir, PageDirectory},
        sync::PageDirSync,
    },
    util::array::ConsistentIndexArray,
};
use alloc::vec::Vec;
use common::types::{Errno, Result};
use core::{
    fmt,
    sync::atomic::{AtomicBool, Ordering},
};
use log::{debug, trace, warn};
use spin::Mutex;

use queue::PageUpdateEntry;

/// the maximum allowed amount of processes
///
/// we could use u32::MAX but POSIX assumes pid_t is signed and compatibility with it is nice
pub const MAX_PROCESSES: u32 = u32::pow(2, 31) - 2;

pub struct Process {
    /// the page directory of this process
    pub page_directory: PageDirSync<'static, crate::arch::PageDirectory<'static>>,

    /// all the threads of this process
    pub threads: ConsistentIndexArray<Thread>,
}

impl Process {
    pub fn set_page_directory(&mut self, page_dir: crate::arch::PageDirectory<'static>) {
        crate::mm::paging::free_page_dir(&mut self.page_directory.task);
        self.page_directory.task = page_dir;
        self.page_directory.force_sync();
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        crate::mm::paging::free_page_dir(&mut self.page_directory.task);
    }
}

pub struct Thread {
    /// this thread's registers
    pub registers: Registers,

    /// this thread's priority
    pub priority: i8,

    /// the CPU this thread was last on
    pub cpu: Option<cpu::ThreadID>,

    /// whether this thread is blocked or not
    pub is_blocked: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ProcessID {
    pub process: u32,
    pub thread: u32,
}

impl fmt::Display for ProcessID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.process, self.thread)
    }
}

// this is all very jank but it seems to work? wonder whether the overhead of locking individual processes is at all worth it
static PROCESSES_LOCK: AtomicBool = AtomicBool::new(false);
static mut PROCESSES: ConsistentIndexArray<Mutex<Process>> = ConsistentIndexArray::new();

fn take_processes_lock() {
    //trace!("taking process list lock");
    if PROCESSES_LOCK.swap(true, Ordering::Acquire) {
        debug!("processes list is locked, spinning");
        while PROCESSES_LOCK.swap(true, Ordering::Acquire) {}
    }
}

fn release_processes_lock() {
    //trace!("releasing process list lock");
    PROCESSES_LOCK.store(false, Ordering::Release);
}

pub fn get_process(id: u32) -> Option<spin::MutexGuard<'static, Process>> {
    take_processes_lock();

    let res = unsafe { PROCESSES.get(id as usize - 1).map(|p| p.lock()) };

    release_processes_lock();

    res
}

pub fn create_process(page_dir: crate::arch::PageDirectory<'static>) -> Result<u32> {
    take_processes_lock();

    if unsafe { PROCESSES.num_entries() >= MAX_PROCESSES as usize } {
        release_processes_lock();

        // POSIX specifies that EAGAIN should be returned if we've hit the process limit
        Err(Errno::TryAgain)
    } else {
        let index = match unsafe {
            PROCESSES.add(Mutex::new(Process {
                page_directory: PageDirSync {
                    kernel: crate::mm::paging::get_kernel_page_dir().0,
                    task: page_dir,
                    process_id: 0,
                    kernel_space_updates: 0,
                },
                threads: ConsistentIndexArray::new(),
            }))
        } {
            Ok(index) => index,
            Err(_) => {
                release_processes_lock();
                return Err(Errno::OutOfMemory);
            }
        };

        if index >= MAX_PROCESSES as usize {
            unsafe {
                PROCESSES.remove(index);
            }

            release_processes_lock();

            Err(Errno::TryAgain)
        } else {
            release_processes_lock();

            let pid = index as u32 + 1;

            let mut process = get_process(pid).ok_or(Errno::TryAgain)?;

            process.page_directory.process_id = pid;
            process.page_directory.force_sync();

            drop(process);

            Ok(pid)
        }
    }
}

pub fn remove_process(id: u32) {
    take_processes_lock();

    unsafe {
        PROCESSES.remove(id as usize - 1);
    }

    release_processes_lock();
}

pub fn remove_thread(id: ProcessID) {
    take_processes_lock();

    if let Some(mut process) = unsafe { PROCESSES.get(id.process as usize - 1).map(|p| p.lock()) } {
        process.threads.remove(id.thread as usize);
    }

    release_processes_lock();
}

pub fn num_processes() -> usize {
    take_processes_lock();

    let res = unsafe { PROCESSES.num_entries() };

    release_processes_lock();

    res
}

static mut CPUS: Option<cpu::CPU> = None;

pub fn get_cpus() -> Option<&'static cpu::CPU> {
    unsafe { CPUS.as_ref() }
}

pub fn set_cpus(cpus: cpu::CPU) {
    unsafe {
        if CPUS.is_some() {
            panic!("can't set CPUs twice");
        }

        debug!("setting CPUs: {:#?}", cpus);

        CPUS = Some(cpus);
    }
}

pub fn process_page_updates() {
    let thread_id = crate::arch::get_thread_id();

    get_cpus().expect("CPUs not initialized").get_thread(thread_id).unwrap().process_page_updates();
}

pub fn update_page(entry: PageUpdateEntry) {
    trace!("updating page {entry:?}");

    let thread_id = crate::arch::get_thread_id();

    if let Some(cpus) = get_cpus() {
        match entry {
            // send to all cpus, wait for all cpus
            PageUpdateEntry::Kernel { addr: _ } => {
                for (core_num, core) in cpus.cores.iter().enumerate() {
                    for (thread_num, thread) in core.threads.iter().enumerate() {
                        if (thread_id.core != core_num || thread_id.thread != thread_num) && thread.has_started() {
                            thread.page_update_queue.lock().insert(entry);

                            let id = cpu::ThreadID { core: core_num, thread: thread_num };

                            assert!(crate::arch::send_interrupt_to_cpu(id, crate::arch::PAGE_REFRESH_INT), "failed to send interrupt");

                            // inefficient and slow :(
                            //debug!("waiting for {id}");
                            while !thread.page_update_queue.lock().is_empty() {
                                crate::arch::spin();
                            }
                        }
                    }
                }
            }

            // only send to and wait for cpus with the same process
            PageUpdateEntry::Task { process_id, addr: _ } => {
                for (core_num, core) in cpus.cores.iter().enumerate() {
                    for (thread_num, thread) in core.threads.iter().enumerate() {
                        if (thread_id.core != core_num || thread_id.thread != thread_num) && thread.has_started()
                            && let Some(current_id) = thread.task_queue.lock().current().map(|c| c.id()) && current_id.process == process_id {
                            thread.page_update_queue.lock().insert(entry);

                            let id = cpu::ThreadID { core: core_num, thread: thread_num };

                            assert!(crate::arch::send_interrupt_to_cpu(id, crate::arch::PAGE_REFRESH_INT), "failed to send interrupt");

                            // inefficient and slow :(
                            //debug!("waiting for {id}");
                            while !thread.page_update_queue.lock().is_empty() {
                                crate::arch::spin();
                            }
                        }
                    }
                }
            }
        }
    }
}

/// sends a non-maskable interrupt to all known CPUs
pub fn nmi_all_other_cpus() {
    warn!("sending NMI to all other CPUs");

    let local_id = crate::arch::get_thread_id();

    if let Some(cpus) = unsafe { CPUS.as_ref() } {
        for (core_id, core) in cpus.cores.iter().enumerate() {
            for thread_id in 0..core.threads.len() {
                let remote_id = cpu::ThreadID { core: core_id, thread: thread_id };

                if remote_id != local_id {
                    crate::arch::send_nmi_to_cpu(remote_id);
                }
            }
        }
    }
}

/// forks the current process, returning the ID of the newly created process
pub fn fork_current_process(thread: &cpu::CPUThread, regs: &mut crate::arch::Registers) -> Result<u32> {
    trace!("forking current process");

    let id = thread.task_queue.lock().current().ok_or(Errno::NoSuchProcess)?.id();

    let cpus = crate::task::get_cpus().expect("CPUs not initialized");
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

    let page_size = crate::arch::PageDirectory::PAGE_SIZE;

    for addr in (0..KERNEL_PAGE_DIR_SPLIT).step_by(page_size) {
        let mut page = get_process(id.process).ok_or(Errno::NoSuchProcess)?.page_directory.get_page(addr);

        // does this page exist?
        if let Some(page) = page.as_mut() {
            debug!("modifying page {addr:#x}");
            // if this page is writable, set it as non-writable and set it to copy on write
            //
            // pages have to be set as non writable in order for copy on write to work since attempting to write to a non writable page causes a page fault exception,
            // which we can then use to copy the page and resume execution as normal
            if page.writable {
                page.writable = false;
                page.copy_on_write = true;
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

        process.page_directory.task = new_orig_page_dir;
        process.page_directory.force_sync();

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

    debug!("queueing process on CPU {thread_id}");
    cpus.get_thread(thread_id)
        .unwrap()
        .task_queue
        .lock()
        .insert(crate::task::queue::TaskQueueEntry::new(ProcessID { process: process_id, thread: 0 }, 0));

    Ok(process_id)
}
