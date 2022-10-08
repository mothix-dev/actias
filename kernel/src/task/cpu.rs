use super::queue::TaskQueue;
use alloc::vec::Vec;
use core::fmt;
use spin::Mutex;

/// describes a CPU and its layout of cores and threads
///
/// this kind of knowledge of the CPU's geometry is required for more intelligent load balancing
pub struct CPU {
    cores: Vec<CPUCore>,
    last_thread_id: usize,
}

impl CPU {
    pub fn new() -> Self {
        Self { cores: Vec::new(), last_thread_id: 0 }
    }

    /// adds a core with the given number of threads to this CPU representation
    pub fn add_core(&mut self, num_threads: usize) {
        let mut threads = Vec::new();

        for _thread in 0..num_threads {
            threads.push(CPUThread { queue: Mutex::new(TaskQueue::new()) });
            self.last_thread_id += 1;
        }

        self.cores.push(CPUCore { threads, num_tasks: 0 });
    }

    /// gets a CPU thread given its ID
    pub fn get_thread(&self, id: ThreadID) -> Option<&CPUThread> {
        self.cores.get(id.core)?.threads.get(id.thread)
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

impl fmt::Debug for CPU {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CPU").field("cores", &self.cores).finish_non_exhaustive()
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
}

/// an ID of a CPU thread
#[derive(Copy, Clone, Debug)]
pub struct ThreadID {
    pub core: usize,
    pub thread: usize,
}
