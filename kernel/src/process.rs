//! process management

use crate::{array::VecBitSet, sched::Task, mm::PageDirSync};
use alloc::{collections::BTreeMap, vec::Vec, sync::Arc};
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
}

pub struct Process {
    pub threads: RwLock<Vec<Arc<Mutex<Task>>>>,
    pub page_directory: Arc<Mutex<PageDirSync<crate::arch::PageDirectory>>>,
}
