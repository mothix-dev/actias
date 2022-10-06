use alloc::{
    collections::VecDeque,
    vec::Vec,
};

pub struct TaskQueue {
    /// current tasks on every cpu
    pub current: Vec<Option<TaskQueueEntry>>,

    pub active_queue: VecDeque<TaskQueueEntry>,
}

impl TaskQueue {
    pub fn new(num_cpus: usize) -> Self {
        let mut current = Vec::with_capacity(num_cpus);
        for _i in 0..num_cpus {
            current.push(None);
        }

        Self {
            current,
            active_queue: VecDeque::new(),
        }
    }

    pub fn consume(&mut self, cpu_num: usize) {
        self.current[cpu_num] = self.active_queue.pop_front();
    }

    pub fn insert(&mut self, entry: TaskQueueEntry) {
        match self.active_queue.iter().position(|e| e.priority < entry.priority) {
            Some(index) => self.active_queue.insert(index, entry),
            None => self.active_queue.push_back(entry),
        }
    }
}

pub struct TaskQueueEntry {
    pub priority: u8,
}
