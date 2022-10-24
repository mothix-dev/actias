//! smooth brain scheduler

pub mod cpu;
pub mod exec;
pub mod queue;
pub mod switch;
pub mod syscalls;

use crate::{arch::Registers, mm::sync::PageDirSync, util::array::ConsistentIndexArray};
use common::types::{Errno, ProcessID, Result};
use core::sync::atomic::{AtomicBool, Ordering};
use log::{debug, error, trace, warn};
use spin::Mutex;

use queue::PageUpdateEntry;

/// the maximum allowed amount of processes
///
/// we could use u32::MAX but POSIX assumes pid_t is signed and compatibility with it is nice
pub const MAX_PROCESSES: u32 = u32::pow(2, 31) - 2;

pub const MAX_THREADS: u32 = u32::pow(2, 31) - 2;

pub struct Process {
    /// the page directory of this process
    pub page_directory: PageDirSync<'static, crate::arch::PageDirectory<'static>>,

    /// all the threads of this process
    pub threads: ConsistentIndexArray<Thread>,
}

impl Process {
    pub fn set_page_directory(&mut self, page_dir: crate::arch::PageDirectory<'static>) -> core::result::Result<(), (Errno, crate::arch::PageDirectory<'static>)> {
        let old_page_dir = core::mem::replace(&mut self.page_directory.task, page_dir);
        match self.page_directory.force_sync() {
            Ok(_) => {
                crate::mm::paging::free_page_dir(&old_page_dir);
                Ok(())
            }
            Err(err) => {
                error!("failed to set page directory of process: {err:?}");
                let page_dir = core::mem::replace(&mut self.page_directory.task, old_page_dir);
                Err((Errno::OutOfMemory, page_dir)) // pass the page dir back so it can be dealt with
            }
        }
    }

    pub fn remove_all_threads(&mut self) {
        self.threads.clear();
        self.page_directory.should_update_pages = false;
    }

    pub fn add_thread(&mut self, thread: Thread) -> Result<u32> {
        if self.threads.num_entries() >= MAX_THREADS as usize {
            Err(Errno::TryAgain)
        } else {
            let idx = self.threads.add(thread).map_err(|_| Errno::OutOfMemory).and_then(|i| i.try_into().map_err(|_| Errno::ValueOverflow))?;

            if !self.page_directory.should_update_pages && self.threads.num_entries() > 1 {
                self.page_directory.should_update_pages = true;
            }

            Ok(idx)
        }
    }

    pub fn remove_thread(&mut self, index: u32) {
        self.threads.remove(index as usize);

        if self.page_directory.should_update_pages && self.threads.num_entries() <= 1 {
            self.page_directory.should_update_pages = false;
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        crate::mm::paging::free_page_dir(&self.page_directory.task);
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

// this is all very jank but it seems to work? wonder whether the overhead of locking individual processes is at all worth it
static PROCESSES_LOCK: AtomicBool = AtomicBool::new(false);
static mut PROCESSES: ConsistentIndexArray<Mutex<Process>> = ConsistentIndexArray::new();

fn take_processes_lock() {
    //trace!("taking process list lock");
    if PROCESSES_LOCK.swap(true, Ordering::Acquire) {
        debug!("(CPU {}) processes list is locked", crate::arch::get_thread_id());
        while PROCESSES_LOCK.swap(true, Ordering::Acquire) {}
    }
}

fn release_processes_lock() {
    //trace!("releasing process list lock");
    PROCESSES_LOCK.store(false, Ordering::Release);
}

pub fn get_process(id: u32) -> Option<spin::MutexGuard<'static, Process>> {
    take_processes_lock();

    let res = unsafe { PROCESSES.get(id as usize).map(|p| p.lock()) };

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
                    should_update_pages: false,
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

            let pid = index as u32;

            let mut process = get_process(pid).ok_or(Errno::TryAgain)?;

            process.page_directory.process_id = pid;
            match process.page_directory.force_sync() {
                Ok(_) => (),
                Err(err) => {
                    error!("failed to synchronize page directory for new process: {err:?}");
                    remove_process(pid);
                    return Err(Errno::OutOfMemory);
                }
            }

            drop(process);

            Ok(pid)
        }
    }
}

pub fn remove_process(id: u32) {
    take_processes_lock();

    unsafe {
        PROCESSES.remove(id as usize);
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

pub fn process_page_updates(thread_id: cpu::ThreadID) {
    trace!("CPU {thread_id} processing page updates");
    get_cpus().expect("CPUs not initialized").get_thread(thread_id).unwrap().process_page_updates();
    trace!("CPU {thread_id} finished processing page updates");
}

static PAGE_UPDATE_MUTEX: AtomicBool = AtomicBool::new(false);

fn take_page_update_lock(thread_id: cpu::ThreadID) {
    let thread = get_cpus().expect("CPUs not initialized").get_thread(thread_id).unwrap();

    if PAGE_UPDATE_MUTEX.swap(true, Ordering::Acquire) {
        // if some other cpu is already trying to update pages, do this as much as we fucking can because otherwise everything goes to shit
        while PAGE_UPDATE_MUTEX.swap(true, Ordering::Acquire) {
            crate::arch::spin();
            thread.process_page_updates();
        }
    }
}

fn release_page_update_lock() {
    PAGE_UPDATE_MUTEX.store(false, Ordering::Release);
}

pub fn update_page(entry: PageUpdateEntry) {
    let thread_id = crate::arch::get_thread_id();

    debug!("(CPU {thread_id}) updating page {entry:?}");

    if let Some(cpus) = get_cpus() {
        match entry {
            // send to all cpus, wait for all cpus
            PageUpdateEntry::Kernel { addr: _ } => {
                for (core_num, core) in cpus.cores.iter().enumerate() {
                    for (thread_num, thread) in core.threads.iter().enumerate() {
                        if (thread_id.core != core_num || thread_id.thread != thread_num) && thread.has_started() {
                            take_page_update_lock(thread_id);

                            trace!("sending to thread {core_num}:{thread_num}");
                            thread.page_update_queue.lock().insert(entry);

                            let id = cpu::ThreadID { core: core_num, thread: thread_num };

                            assert!(crate::arch::send_interrupt_to_cpu(id, crate::arch::PAGE_REFRESH_INT), "failed to send interrupt");

                            // inefficient and slow :(
                            trace!("waiting for {id}");
                            while !thread.page_update_queue.lock().is_empty() {
                                crate::arch::spin();
                            }

                            release_page_update_lock();
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
                            take_page_update_lock(thread_id);

                            thread.page_update_queue.lock().insert(entry);

                            let id = cpu::ThreadID { core: core_num, thread: thread_num };

                            assert!(crate::arch::send_interrupt_to_cpu(id, crate::arch::PAGE_REFRESH_INT), "failed to send interrupt");

                            // inefficient and slow :(
                            trace!("waiting for {id}");
                            while !thread.page_update_queue.lock().is_empty() {
                                crate::arch::spin();
                            }

                            release_page_update_lock();
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
