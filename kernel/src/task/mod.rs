//! smooth brain scheduler

use alloc::{collections::VecDeque, vec::Vec};
use spin::Mutex;

/*
each logical CPU has its own task queue
if a CPU's task queue is empty, it will search thru other CPU task queues (in order of distance, SMT threads first, then other cores, then other CPUs?) to find a task to run (todo: do we want to steal tasks from other physical processors? will that fuck up cache?)
if no task can be found, the CPU will enter a power-saving state until the next task switch
hopefully this behavior can be done independently, with CPU task queues being locked with a spinlock (or maybe a more advanced mutex could be possible? halt the CPU, then resume it with an interrupt when the lock is released)

context switch would go something like
 - save state of process
 - lock task queue
    - get next task
    - insert process back into queue if it's been forcefully preempted (i.e. didn't voluntarily block)
 - release lock
 - load state of process
*/

/// a per-CPU task queue
pub struct TaskQueue {
    /// tasks waiting for CPU time
    queue: VecDeque<TaskQueueEntry>,
}

impl TaskQueue {
    pub fn new() -> Self {
        Self { queue: VecDeque::new() }
    }

    /// gets the first task in the queue
    pub fn consume(&mut self) -> Option<TaskQueueEntry> {
        self.queue.pop_front()
    }

    /// inserts a task into the queue
    pub fn insert(&mut self, entry: TaskQueueEntry) {
        // this may not be the most efficient way to do this
        match self.queue.iter().position(|e| e.priority < entry.priority) {
            Some(index) => self.queue.insert(index, entry),
            None => self.queue.push_back(entry),
        }
    }

    /// checks whether this taskqueue is empty
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl Default for TaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// an entry in a task queue
pub struct TaskQueueEntry {
    pub priority: u8,
}

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
        //let core = self.cores.len();

        let mut threads = Vec::new();

        for _thread in 0..num_threads {
            threads.push(CPUThread {
                queue: Mutex::new(TaskQueue::new(/*ThreadID { core, thread }*/)),
            });
            self.last_thread_id += 1;
        }

        self.cores.push(CPUCore { threads });
    }

    /// gets a CPU thread given its ID
    pub fn get_thread(&self, id: ThreadID) -> Option<&CPUThread> {
        self.cores.get(id.core)?.threads.get(id.thread)
    }

    /// searches through threads in this CPU in hierarchical order to find a thread with at least one extra task
    pub fn find_thread_to_steal_from(&self, id: ThreadID) -> Option<ThreadID> {
        // search threads in the same core as the provided ID
        // Iterator::find() exists, but would be much less readable so i'm not using it
        for (thread_id, thread) in self.cores.get(id.core)?.threads.iter().enumerate() {
            // is the queue for this thread empty?
            if !thread.queue.lock().is_empty() {
                return Some(ThreadID { core: id.core, thread: thread_id });
            }
        }

        // search all other cores
        for (core_id, core) in self.cores.iter().enumerate() {
            // skip the original core, no need to search it twice
            // it's possible that since the CPU struct itself won't be locked that maybe another task will pop up by now,
            // but it probably doesn't matter and is likely not worth the overhead
            if core_id == id.core {
                continue;
            }

            for (thread_id, thread) in core.threads.iter().enumerate() {
                // is the queue for this thread empty?
                if !thread.queue.lock().is_empty() {
                    return Some(ThreadID { core: core_id, thread: thread_id });
                }
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

pub struct CPUCore {
    pub threads: Vec<CPUThread>,
}

pub struct CPUThread {
    pub queue: Mutex<TaskQueue>,
}

/// an ID of a CPU thread
#[derive(Copy, Clone, Debug)]
pub struct ThreadID {
    pub core: usize,
    pub thread: usize,
}
