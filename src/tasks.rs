//! tasks and task switching

use crate::{
    arch::{
        tasks::TaskState,
        paging::free_page_phys,
    },
    fs::ops::{OpenFile, open},
    util::array::VecBitSet,
    types::{
        errno::Errno,
        file::{FileDescriptor, OpenFlags, Permissions},
        UserID, GroupID,
    },
};
use alloc::{
    collections::BTreeMap,
    vec, vec::Vec,
};
use core::fmt;

/// arbitrary limit for the maximum amount of files a task can have open at once
const MAX_OPEN_FILES: usize = 2048;

#[derive(Debug, Copy, Clone)]
pub enum BlockKind {
    None,
    Read(FileDescriptor),
    Write(FileDescriptor),
}

/// structure for task, contains task state, flags, etc
pub struct Task {
    /// state of this task. contains saved registers, page directory, etc
    pub state: TaskState,
    
    /// pid of this task
    pub id: usize,

    /// pids of children of this task
    pub children: Vec<usize>,

    /// pid of parent of this task, if one exists
    pub parent: Option<usize>,

    /// list of all files this task has open
    pub files: Vec<Option<OpenFile>>,

    /// bitset to accelerate finding a slot for a new open file
    pub files_bit_set: VecBitSet,

    /// whether this task is blocked and has to wait for io
    pub blocked: bool,

    /// what kind of blocking this is
    pub block_kind: BlockKind,

    /// error (if any) we've encountered while blocking for a file
    pub blocked_err: Errno,

    /// whether this task has just been unblocked and needs to have an io operation completed
    pub just_unblocked: bool,

    /// real user id of the task
    pub real_uid: UserID,

    /// real group of the task
    pub real_gid: GroupID,

    /// effective user id of the task. this is used for permission checks, etc
    pub effective_uid: UserID,

    /// effective group id of the task
    pub effective_gid: GroupID,

    /// saved user id of the task. allows unprivileged processes to switch between their real and effective uids
    pub saved_uid: UserID,

    /// saved group id of the task
    pub saved_gid: GroupID,
}

impl Task {
    /// creates a new task
    pub fn new(uid: UserID, gid: GroupID) -> Self {
        Self {
            state: Default::default(),
            id: 0,
            children: Vec::new(),
            parent: None,
            files: vec![],
            files_bit_set: VecBitSet::new(),
            blocked: false,
            block_kind: BlockKind::None,
            blocked_err: Errno::None,
            just_unblocked: false,
            real_uid: uid,
            real_gid: gid,
            effective_uid: uid,
            effective_gid: gid,
            saved_uid: uid,
            saved_gid: gid,
        }
    }

    /// recreates this task with a default state
    pub fn recreate(&self) -> Self {
        Self {
            state: Default::default(),
            id: self.id,
            children: self.children.to_vec(),
            parent: self.parent,
            files: self.files.to_vec(),
            files_bit_set: self.files_bit_set.clone(),
            blocked: self.blocked,
            block_kind: self.block_kind,
            blocked_err: self.blocked_err,
            just_unblocked: self.just_unblocked,
            real_uid: self.real_uid,
            real_gid: self.real_gid,
            effective_uid: self.effective_uid,
            effective_gid: self.effective_gid,
            saved_uid: self.saved_uid,
            saved_gid: self.saved_gid,
        }
    }

    pub fn clone(&self, state: TaskState) -> Self {
        Self {
            state,
            id: 0,
            children: Vec::new(),
            parent: Some(self.id),
            files: self.files.to_vec(),
            files_bit_set: self.files_bit_set.clone(),
            blocked: self.blocked,
            block_kind: self.block_kind,
            blocked_err: self.blocked_err,
            just_unblocked: self.just_unblocked,
            real_uid: self.real_uid,
            real_gid: self.real_gid,
            effective_uid: self.effective_uid,
            effective_gid: self.effective_gid,
            saved_uid: self.saved_uid,
            saved_gid: self.saved_gid,
        }
    }

    /// open a file, returning a numerical file descriptor
    pub fn open(&mut self, path: &str, flags: OpenFlags, permissions: Permissions) -> Result<FileDescriptor, Errno> {
        let first_unused = self.files_bit_set.first_unset();

        if first_unused > MAX_OPEN_FILES {
            Err(Errno::TooManyFilesOpen)
        } else {
            self.files_bit_set.set(first_unused);

            if first_unused >= self.files.len() {
                for _i in self.files.len()..=first_unused {
                    self.files.push(None);
                }
            }

            *(self.files.get_mut(first_unused).ok_or(Errno::TooManyFilesOpen)?) = Some(open(path, flags, permissions)?);
            
            Ok(first_unused)
        }
    }

    /// closes a file descriptor, freeing its slot for use by the next file to be opened
    pub fn close(&mut self, desc: FileDescriptor) -> Result<(), Errno> {
        if let Some(openfile) = self.files.get_mut(desc) {
            *openfile = None;
            self.files_bit_set.clear(desc);

            Ok(())
        } else {
            Err(Errno::BadFile)
        }
    }

    /// gets the openfile object associated with a file descriptor
    pub fn get_open_file(&mut self, desc: FileDescriptor) -> Result<&mut OpenFile, Errno> {
        self.files.get_mut(desc).ok_or(Errno::BadFile)?.as_mut().ok_or(Errno::BadFile)
    }

    /// checks if a task should become unblocked
    pub fn check_blocked(&mut self) {
        let mut check = || {
            match self.block_kind {
                BlockKind::None => Ok(false),
                BlockKind::Read(desc) => Ok(self.get_open_file(desc)?.can_read(1)?),
                BlockKind::Write(desc) => Ok(self.get_open_file(desc)?.can_write(1)?),
            }
        };

        match check() {
            Ok(should_unblock) => if should_unblock {
                self.blocked = false;
                self.blocked_err = Errno::None;
                self.just_unblocked = true;
            },
            Err(err) => {
                self.blocked = false;
                self.blocked_err = err;
                self.just_unblocked = true;
            },
        }
    }

    /// sets a task as blocked
    pub fn block(&mut self, kind: BlockKind) {
        self.blocked = true;
        self.block_kind = kind;
    }
}

impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Task")
         .field("id", &self.id)
         .field("children", &self.children)
         .field("parent", &self.parent)
         .field("files", &self.files)
         .field("blocked", &self.blocked)
         .field("block_kind", &self.block_kind)
         .field("blocked_err", &self.blocked_err)
         .field("just_unblocked", &self.just_unblocked)
         .finish_non_exhaustive()
    }
}

impl Default for Task {
    fn default() -> Self {
        Self::new(0, 0)
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
/// 
/// returns true on a successful switch, false otherwise
pub fn switch_tasks() -> bool {
    unsafe {
        if TASKS.is_empty() {
            log!("no tasks?");
            false
        } else {
            let old_task = CURRENT_TASK;

            loop {
                CURRENT_TASK = (CURRENT_TASK + 1) % TASKS.len();
    
                // make sure our next task isn't blocked
                if let Some(task) = TASKS.get_mut(CURRENT_TASK) {
                    if task.blocked {
                        // see if task should be unblocked
                        task.check_blocked();
                    }

                    if task.blocked && CURRENT_TASK == old_task {
                        // we've iterated through all tasks and found none that aren't blocked, give up
                        return false;
                    } else if !task.blocked {
                        //log!("found non blocked task {}", CURRENT_TASK);
                        return true;
                    }
                }
            }
        }
    }
}

/// add new task
pub fn add_task(mut task: Task) -> usize {
    unsafe { TOTAL_TASKS += 1; }

    let id = unsafe { TOTAL_TASKS };
    task.id = id;

    debug!("added task {:?}", task);

    unsafe {
        TASKS.push(task);
    }

    id
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

                TASKS.remove(id);

                garbage_collect();

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
