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

/// whether we're currently running a task
pub static mut IN_TASK: bool = false;

/// whether the current task was terminated before next task switch
pub static mut CURRENT_TERMINATED: bool = false;

/// get a reference to the next task to switch to
pub fn get_next_task() -> Option<&'static Task> {
    unsafe {
        let next = (CURRENT_TASK + 1) % TASKS.len();
        TASKS.get(next)
    }
}

/// get a mutable reference to the next task to switch to
pub fn get_next_task_mut() -> Option<&'static mut Task> {
    unsafe {
        let next = (CURRENT_TASK + 1) % TASKS.len();
        TASKS.get_mut(next)
    }
}

/// get a reference to the current task
pub fn get_current_task() -> Option<&'static Task> {
    unsafe {
        TASKS.get(CURRENT_TASK)
    }
}

/// get a mutable reference to the current task
pub fn get_current_task_mut() -> Option<&'static mut Task> {
    unsafe {
        TASKS.get_mut(CURRENT_TASK)
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

/// remove existing task
pub fn remove_task(id: usize) {
    unsafe {
        if id < TASKS.len() {
            TASKS.remove(id);
        }
        if id == CURRENT_TASK {
            CURRENT_TASK = (CURRENT_TASK - 1) % TASKS.len();
            CURRENT_TERMINATED = true;
        }
    }
}
