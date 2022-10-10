use super::queue::TaskQueue;
use crate::arch::ThreadInfo;
use alloc::vec::Vec;
use core::fmt;
use spin::Mutex;

/// describes a CPU and its layout of cores and threads
///
/// this kind of knowledge of the CPU's topology is required for more intelligent load balancing
#[derive(Debug)]
pub struct CPU {
    pub cores: Vec<CPUCore>,
}

impl CPU {
    pub fn new() -> Self {
        Self { cores: Vec::new() }
    }

    /// adds a core with the given number of threads to this CPU representation
    pub fn add_core(&mut self) {
        self.cores.push(CPUCore { threads: Vec::new(), num_tasks: 0 });
    }

    /// gets a reference to a CPU thread given its ID
    pub fn get_thread(&self, id: ThreadID) -> Option<&CPUThread> {
        self.cores.get(id.core)?.threads.get(id.thread)
    }

    /// gets a mutable reference to a CPU thread given its ID
    pub fn get_thread_mut(&mut self, id: ThreadID) -> Option<&mut CPUThread> {
        self.cores.get_mut(id.core)?.threads.get_mut(id.thread)
    }

    /// searches through threads in this CPU in hierarchical order to find a thread with at least one extra task
    pub fn find_thread_to_steal_from(&self, id: ThreadID) -> Option<ThreadID> {
        // search threads in the same core as the provided ID
        if let Some(thread_id) = self.cores.get(id.core)?.find_busiest_thread() {
            return Some(ThreadID { core: id.core, thread: thread_id });
        }

        // search all other cores
        for (core_id, core) in self.cores.iter().enumerate() {
            // skip the original core, no need to search it twice
            // it's possible that since the CPU struct itself won't be locked that maybe another task will pop up by now,
            // but it probably doesn't matter and is likely not worth the overhead
            if core_id == id.core {
                continue;
            }

            if let Some(thread_id) = core.find_busiest_thread() {
                return Some(ThreadID { core: core_id, thread: thread_id });
            }
        }

        // no tasks?
        None
    }
}

impl Default for CPU {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct CPUCore {
    pub threads: Vec<CPUThread>,
    pub num_tasks: usize,
}

impl CPUCore {
    /// adds a new thread to this core
    pub fn add_thread(&mut self, info: ThreadInfo, timer: Option<usize>) {
        self.threads.push(CPUThread::new(info, timer));
    }

    /// finds the thread in this core with the most tasks waiting in its queue
    pub fn find_busiest_thread(&self) -> Option<usize> {
        let mut thread_id = None;
        let mut num_tasks = 0;

        for (id, thread) in self.threads.iter().enumerate() {
            let cur_num_tasks = thread.queue.lock().len();
            if cur_num_tasks > num_tasks {
                thread_id = Some(id);
                num_tasks = cur_num_tasks;
            }
        }

        thread_id
    }

    /// finds the thread in this core with the least tasks waiting in its queue
    pub fn find_emptiest_thread(&self) -> Option<usize> {
        let mut thread_id = None;
        let mut num_tasks = usize::MAX;

        for (id, thread) in self.threads.iter().enumerate() {
            let cur_num_tasks = thread.queue.lock().len();
            if cur_num_tasks < num_tasks {
                thread_id = Some(id);
                num_tasks = cur_num_tasks;
            }
        }

        thread_id
    }
}

#[derive(Debug)]
pub struct CPUThread {
    pub queue: Mutex<TaskQueue>,
    pub timer: Option<usize>,
    pub info: ThreadInfo,
}

impl CPUThread {
    pub fn new(info: ThreadInfo, timer: Option<usize>) -> Self {
        Self {
            queue: Mutex::new(TaskQueue::new()),
            timer,
            info,
        }
    }
}

/// an ID of a CPU thread
#[derive(Copy, Clone, Debug)]
pub struct ThreadID {
    pub core: usize,
    pub thread: usize,
}

impl fmt::Display for ThreadID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.core, self.thread)
    }
}
