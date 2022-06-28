//! tasks and task switching

use crate::arch::{
    tasks::TaskState,
    paging::free_page_phys,
};
use alloc::{
    collections::BTreeMap,
    vec::Vec,
};
use core::fmt;

/// structure for task, contains task state, flags, etc
pub struct Task {
    pub state: TaskState,
    pub id: usize,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
}

impl Task {
    /// creates a new task
    pub fn new() -> Self {
        Self::from_state(Default::default())
    }

    /// creates a new task with the provided state
    pub fn from_state(state: TaskState) -> Self {
        unsafe { TOTAL_TASKS += 1; }

        let id = unsafe { TOTAL_TASKS };

        debug!("new task with pid {}", id);

        Self {
            state, id,
            children: Vec::new(),
            parent: None,
        }
    }

    /// recreates this task with a default state
    pub fn recreate(&self) -> Self {
        Self {
            state: Default::default(),
            id: self.id,
            children: self.children.to_vec(),
            parent: self.parent,
        }
    }
}

impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Task")
         .field("id", &self.id)
         .field("children", &self.children)
         .field("parent", &self.parent)
         .finish_non_exhaustive()
    }
}

impl Default for Task {
    fn default() -> Self {
        Self::new()
    }
}

/// list of all available tasks
pub static mut TASKS: Vec<Task> = Vec::new();

/// what task we're currently on
pub static mut CURRENT_TASK: usize = 0;

/// whether we're currently running a task
pub static mut IN_TASK: bool = false;

/// whether the current task was terminated before next task switch
pub static mut CURRENT_TERMINATED: bool = false;

/// count of all task ids, we don't want duplicates
pub static mut TOTAL_TASKS: usize = 0;

/// keeps track of all pages we've copied and how many references to them exist
pub static mut PAGE_REFERENCES: Option<BTreeMap<u64, PageReference>> = None;

/// used to keep track of references to a copied page
#[derive(Debug)]
pub struct PageReference {
    /// how many references to this page exist
    pub references: usize,

    /// owner pid of this page
    pub owner: usize,

    /// physical address of the page this references
    pub phys: u64,
}

impl PageReference {
    pub fn has_owner(&self) -> bool {
        get_task(self.owner).is_some()
    }

    pub fn remove_ref(&mut self) {
        debug!("{} references to {:#x}", self.references, self.phys);
        // are there any other processes using this page?
        if self.references > 0 {
            // yes, decrease the reference counter
            self.references -= 1;

            debug!("now {} references to {:#x}", self.references, self.phys);
        } else if !self.has_owner() { // does the owner process still exist?
            debug!("freeing reference");
            // no, free this page up for use
            free_page_phys(self.phys);
            get_page_references().remove(&self.phys);
        }
    }
}

/// gets a reference to the page references map
pub fn get_page_references() -> &'static mut BTreeMap<u64, PageReference> {
    if let Some(references) = unsafe { PAGE_REFERENCES.as_mut() } {
        references
    } else {
        unsafe {
            PAGE_REFERENCES = Some(BTreeMap::new());

            PAGE_REFERENCES.as_mut().unwrap()
        }
    }
}

/// removes a reference to the specified page
pub fn remove_page_reference(phys: u64) {
    debug!("removing reference to {:#x}", phys);
    
    get_page_references().get_mut(&phys).expect("tried to remove a reference to a non referenced page").remove_ref();
}

/// adds a reference to the specified page
pub fn add_page_reference(phys: u64, owner: usize) {
    if let Some(reference) = get_page_references().get_mut(&phys) {
        debug!("found existing reference to {:#x}", phys);

        reference.references += 1;
    } else {
        debug!("creating new reference to {:#x}", phys);

        get_page_references().insert(phys, PageReference {
            references: 1,
            owner, phys,
        });
    }
}

/// scan for references that can be freed and removed, and free and remove them
pub fn garbage_collect() {
    let mut to_remove: Vec<u64> = Vec::new();

    for (phys, reference) in get_page_references().iter_mut() {
        if reference.references == 0 && !reference.has_owner() {
            debug!("garbage collector: freeing page @ {:#x}", phys);

            free_page_phys(*phys);

            to_remove.push(*phys);
        }
    }

    for key in to_remove {
        debug!("garbage collector: removing reference to {:#x}", key);

        get_page_references().remove(&key);
    }
}

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
        if !TASKS.is_empty() {
            CURRENT_TASK = (CURRENT_TASK + 1) % TASKS.len();
        }
    }
}

/// add new task
pub fn add_task(task: Task) {
    debug!("added task {:?}", task);

    unsafe {
        TASKS.push(task);
    }
}

/// remove existing task
pub fn remove_task(pid: usize) {
    unsafe {
        if let Some(id) = pid_to_id(pid) {
            if let Some(task) = TASKS.get_mut(id) {
                debug!("removing task {:?}", task);

                // list of all copied pages
                for child in task.children.iter() {
                    if let Some(child) = get_task_mut(*child) {
                        debug!("child: {:?}", child.id);

                        child.parent = None;
                    }
                }

                task.state.free_pages();

                garbage_collect();

                TASKS.remove(id);

                if id == CURRENT_TASK {
                    if !TASKS.is_empty() {
                        CURRENT_TASK = (CURRENT_TASK - 1) % TASKS.len();
                    }
                    CURRENT_TERMINATED = true;
                }
            }
        }
    }
}

/// get reference to existing task
pub fn get_task(id: usize) -> Option<&'static Task> {
    for task in unsafe { TASKS.iter() } {
        if task.id == id {
            return Some(task);
        }
    }
    None
}

/// get mutable reference to existing task
pub fn get_task_mut(id: usize) -> Option<&'static mut Task> {
    for task in unsafe { TASKS.iter_mut() } {
        if task.id == id {
            return Some(task);
        }
    }
    None
}

/// get internal id of task with given pid
fn pid_to_id(pid: usize) -> Option<usize> {
    (unsafe { &mut TASKS }).iter().position(|task| task.id == pid)
}

/// get number of tasks
pub fn num_tasks() -> usize {
    unsafe { TASKS.len() }
}
