//! process management

use crate::{arch::PROPERTIES, array::VecBitSet, mm::MemoryProtection, sched::Task};
use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use common::Errno;
use spin::{Mutex, RwLock};

pub enum AddProcessError {
    NoMorePIDs,
    NoMoreProcesses,
}

impl core::fmt::Debug for AddProcessError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoMorePIDs => write!(f, "no more PIDs"),
            Self::NoMoreProcesses => write!(f, "process limit reached"),
        }
    }
}

pub struct ProcessTable {
    used_map: VecBitSet,
    process_map: BTreeMap<usize, Process>,
    next_pid: usize,
    max_pid: usize,
    num_processes: usize,
    max_processes: usize,
}

impl ProcessTable {
    /// creates a new process table with the given limits (exclusive)
    pub fn new(max_pid: usize, max_processes: usize) -> Self {
        Self {
            used_map: VecBitSet::new(),
            process_map: BTreeMap::new(),
            next_pid: 1,
            max_pid,
            num_processes: 0,
            max_processes,
        }
    }

    fn find_pid(&mut self) -> Option<usize> {
        for _i in 0..=self.max_pid {
            let pid = self.next_pid;
            self.next_pid = (self.next_pid + 1) % self.max_pid;

            if !self.used_map.test(self.next_pid) {
                return Some(pid);
            }
        }

        None
    }

    /// inserts the given process into the process table, returning the PID allocated for it
    pub fn insert(&mut self, process: Process) -> Result<usize, AddProcessError> {
        let pid = self.find_pid().ok_or(AddProcessError::NoMorePIDs)?;

        if self.num_processes >= self.max_processes {
            return Err(AddProcessError::NoMoreProcesses);
        }

        self.num_processes += 1;
        self.used_map.set(pid);
        self.process_map.insert(pid, process);

        Ok(pid)
    }

    /// removes the process at the given PID from the process table
    pub fn remove(&mut self, pid: usize) {
        if self.process_map.remove(&pid).is_some() {
            self.used_map.clear(pid);
            self.num_processes -= 1;
        }
    }

    /// gets a reference to the process associated with the given PID
    pub fn get(&self, pid: usize) -> Option<&Process> {
        self.process_map.get(&pid)
    }

    /// gets a mutable reference to the process associated with the given PID
    pub fn get_mut(&mut self, pid: usize) -> Option<&mut Process> {
        self.process_map.get_mut(&pid)
    }

    /// gets an iterator over every process in the map
    pub fn iter(&self) -> alloc::collections::btree_map::Iter<'_, usize, Process> {
        self.process_map.iter()
    }

    /// gets a mutable iterator over every process in the map
    pub fn iter_mut(&mut self) -> alloc::collections::btree_map::IterMut<'_, usize, Process> {
        self.process_map.iter_mut()
    }

    /// gets the PID limit
    pub fn max_pid(&self) -> usize {
        self.max_pid
    }

    /// checks whether the given pid exists in the table
    pub fn contains(&self, pid: usize) -> bool {
        self.used_map.test(pid)
    }
}

pub struct Process {
    pub threads: RwLock<Vec<Arc<Mutex<Task>>>>,
    pub memory_map: Arc<Mutex<crate::mm::ProcessMap>>,
    pub environment: Arc<crate::fs::FsEnvironment>,
    pub filesystem: Mutex<Option<Arc<crate::fs::user::UserspaceFs>>>,
}

/// a buffer in the memory map of a specific process
pub struct ProcessBuffer {
    base: usize,
    length: usize,
    memory_map: Arc<Mutex<crate::mm::ProcessMap>>,
    pid: usize,
}

impl ProcessBuffer {
    pub fn from_current_process(base: usize, length: usize) -> common::Result<Self> {
        let pid = crate::sched::get_current_pid()?;
        let memory_map = crate::get_global_state().process_table.read().get(pid).ok_or(Errno::NoSuchProcess)?.memory_map.clone();

        Ok(Self { base, length, memory_map, pid })
    }

    /// maps this buffer into memory and calls the given function with a slice over it
    pub fn map_in<F: FnOnce(&[u8]) -> R, R>(&self, op: F) -> common::Result<R> {
        let addrs = self.memory_map.lock().map_in_area(&self.memory_map, self.base, self.length, MemoryProtection::Read)?;

        unsafe {
            if crate::sched::get_current_pid() == Ok(self.pid) {
                // page directory most likely hasn't changed so just read directly from memory
                let buf = core::slice::from_raw_parts(self.base as *mut u8, self.length);
                Ok(op(buf))
            } else {
                crate::mm::map_memory(&mut self.memory_map.lock().page_directory, &addrs, |slice| {
                    let aligned_addr = (self.base / PROPERTIES.page_size) * PROPERTIES.page_size;
                    let offset = self.base - aligned_addr;

                    op(&slice[offset..offset + self.length])
                })
                .map_err(Errno::from)
            }
        }
    }

    /// maps this buffer into memory and calls the given function with a mutable slice over it
    pub fn map_in_mut<F: FnOnce(&mut [u8]) -> R, R>(&self, op: F) -> common::Result<R> {
        let addrs = self
            .memory_map
            .lock()
            .map_in_area(&self.memory_map, self.base, self.length, MemoryProtection::Read | MemoryProtection::Write)?;

        unsafe {
            if crate::sched::get_current_pid() == Ok(self.pid) {
                // TODO: check to make sure this is really the same process
                let buf = core::slice::from_raw_parts_mut(self.base as *mut u8, self.length);
                Ok(op(buf))
            } else {
                crate::mm::map_memory(&mut self.memory_map.lock().page_directory, &addrs, |slice| {
                    let aligned_addr = (self.base / PROPERTIES.page_size) * PROPERTIES.page_size;
                    let offset = self.base - aligned_addr;

                    op(&mut slice[offset..offset + self.length])
                })
                .map_err(Errno::from)
            }
        }
    }

    /// maps this buffer into memory and copies its contents into the given slice, returning the number of bytes copied
    pub fn copy_into(&self, to_write: &mut [u8]) -> common::Result<usize> {
        self.map_in(|buf| {
            let bytes_written = to_write.len().min(buf.len());
            to_write[..bytes_written].copy_from_slice(&buf[..bytes_written]);
            bytes_written
        })
    }

    /// maps this buffer into memory and copies the contents of the given slice into it, returning the number of bytes copied
    pub fn copy_from(&self, to_read: &[u8]) -> common::Result<usize> {
        self.map_in_mut(|buf| {
            let bytes_read = to_read.len().min(buf.len());
            buf[..bytes_read].copy_from_slice(&to_read[..bytes_read]);
            bytes_read
        })
    }
}
