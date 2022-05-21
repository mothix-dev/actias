//! tasks and task switching

use crate::arch::tasks::TaskState;
use alloc::vec::Vec;

/// structure for task, contains task state, flags, etc
pub struct Task {
    pub state: TaskState,
    pub id: usize,
}

/// list of all available tasks
pub static mut TASKS: Vec<Task> = Vec::new();

/// what task we're currently on
pub static mut CURRENT_TASK: usize = 0;

/// get a reference to the next task to switch to
pub fn get_next_task() -> &'static Task {
    unsafe {
        let next = (CURRENT_TASK + 1) % TASKS.len();
        TASKS.get(next).expect("no tasks?")
    }
}

/// get a mutable reference to the next task to switch to
pub fn get_next_task_mut() -> &'static mut Task {
    unsafe {
        let next = (CURRENT_TASK + 1) % TASKS.len();
        TASKS.get_mut(next).expect("no tasks?")
    }
}

/// get a reference to the current task
pub fn get_current_task() -> &'static Task {
    unsafe {
        TASKS.get(CURRENT_TASK).expect("no tasks?")
    }
}

/// get a mutable reference to the current task
pub fn get_current_task_mut() -> &'static mut Task {
    unsafe {
        TASKS.get_mut(CURRENT_TASK).expect("no tasks?")
    }
}

/// switch to the next task, making it the current task
pub fn switch_tasks() {
    unsafe {
        CURRENT_TASK = (CURRENT_TASK + 1) % TASKS.len();
    }
}

/// add new task
pub fn add_task(task: Task) {
    unsafe {
        TASKS.push(task);
    }
}

