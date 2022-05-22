//! low level i586-specific task switching

use super::ints::SyscallRegisters;
use super::paging::{PAGE_DIR, PageDirectory, PageTableFlags};
use crate::arch::{PAGE_SIZE, LINKED_BASE};
use core::arch::asm;
use crate::tasks::{CURRENT_TASK, IN_TASK, Task, remove_task, get_task, get_task_mut, add_task, pid_to_id};

pub struct TaskState {
    pub registers: SyscallRegisters,
    pub pages: PageDirectory,
    pub page_updates: usize,
}

impl TaskState {
    /// creates a new task state, copying pages from kernel directory
    pub fn new() -> Self {
        let global_dir = unsafe { PAGE_DIR.as_mut().expect("paging not initialized") };

        let mut state = Self {
            registers: Default::default(),
            pages: PageDirectory::new(),
            page_updates: global_dir.page_updates,
        };

        state.copy_pages_from(global_dir, 0, 1024);

        state
    }

    /// copies registers to task state
    pub fn save(&mut self, regs: &SyscallRegisters) {
        self.registers = *regs;
    }

    /// replaces registers with task state
    pub fn load(&self, regs: &mut SyscallRegisters) {
        *regs = self.registers; // replace all registers with our own (:
    }

    /// copy pages from existing page directory, in range start..end (start is inclusive, end is not)
    pub fn copy_pages_from(&mut self, dir: &mut PageDirectory, start: usize, end: usize) {
        assert!(start <= end);
        assert!(end <= 1024);

        for i in start..end {
            self.pages.tables[i] = dir.tables[i];

            unsafe {
                (*self.pages.tables_physical)[i] = (*dir.tables_physical)[i];
            }
        }
    }

    /// copy pages from existing page directory, in range start..end (start is inclusive, end is not)
    /// all pages copied have the read/write flag unset, and if it was previously set, the copy on write flag
    /// writing to any copied page will cause it to copy itself and all its data, and all writes will go to a new page
    pub fn copy_on_write_from(&mut self, dir: &mut PageDirectory, start: usize, end: usize) {
        assert!(start <= end);
        assert!(end <= 1024);

        for i in start..end {
            if dir.tables[i].is_null() {
                self.pages.tables[i] = core::ptr::null_mut();

                unsafe {
                    (*self.pages.tables_physical)[i] = 0;
                }
            } else {
                for addr in ((i << 22)..((i + 1) << 22)).step_by(PAGE_SIZE) {
                    let page = unsafe { &mut *self.pages.get_page(addr as u32, true).expect("couldn't create page table") };
                    let orig_page = unsafe { &mut *dir.get_page(addr as u32, false).expect("couldn't get page table") };

                    // disable write flag, enable copy on write
                    let mut flags: PageTableFlags = orig_page.get_flags().into();
                    
                    if flags & PageTableFlags::ReadWrite != 0 {
                        flags &= !PageTableFlags::ReadWrite;
                        flags |= PageTableFlags::CopyOnWrite;
                    }

                    page.set_flags(flags);
                    page.set_address(orig_page.get_address());
                }
            }
        }
    }

    /// allocate a page at the specified address
    /// we can't use the page directory's alloc_frame function, since it'll overwrite data
    pub fn alloc_page(&mut self, addr: u32, is_kernel: bool, is_writeable: bool, invalidate: bool) {
        assert!(addr % PAGE_SIZE as u32 == 0, "address is not page aligned");

        let page = self.pages.get_page(addr, true).unwrap();

        unsafe {
            let dir = PAGE_DIR.as_mut().unwrap();

            if let Some(addr) = dir.alloc_frame(page, is_kernel, is_writeable) {
                if invalidate {
                    asm!("invlpg [{0}]", in(reg) addr); // invalidate this page in the TLB
                }
            }
        }
    }

    /// free a page at the specified address
    pub fn free_page(&mut self, addr: u32) {
        assert!(addr % PAGE_SIZE as u32 == 0, "address is not page aligned");

        if let Some(page) = self.pages.get_page(addr, false) {
            unsafe {
                let dir = PAGE_DIR.as_mut().unwrap();

                if let Some(addr) = dir.free_frame(page) {
                    asm!("invlpg [{0}]", in(reg) addr); // invalidate this page in the TLB
                }
            }
        }
    }
}

impl Default for TaskState {
    fn default() -> Self {
        Self::new()
    }
}

/// exits current task, cpu idles until next task switch
pub fn exit_current_task() {
    if let Err(msg) = kill_task(unsafe { CURRENT_TASK }) {
        panic!("couldn't kill task: {}", msg);
    }

    // idle the cpu until the next task switch
    unsafe { IN_TASK = true; }

    loop {
        unsafe { asm!("sti; hlt"); }
    }
}

/// kills specified task
pub fn kill_task(id: usize) -> Result<(), &'static str> {
    // TODO: signals, etc

    if let Some(task) = get_task(id) {
        remove_task(id);

        log!("task {} (pid {}) exited", id, task.id);

        Ok(())
    } else {
        Err("couldn't get task")
    }
}

/// kills task specified with PID
pub fn kill_task_pid(pid: usize) -> Result<(), &'static str> {
    if let Some(id) = pid_to_id(pid) {
        kill_task(id)
    } else {
        Err("PID not found")
    }
}

/// forks task, creating another identical task
pub fn fork_task(id: usize) -> Result<&'static mut Task, &'static str> {
    let current =
        if let Some(task) = get_task_mut(id) {
            task
        } else {
            return Err("couldn't get task")
        };

    // create new task state
    let mut state = TaskState {
        registers: current.state.registers,
        pages: PageDirectory::new(),
        page_updates: current.state.page_updates,
    };

    // copy kernel pages, copy parent task's pages as copy on write
    let kernel_start = LINKED_BASE >> 22;
    let dir = unsafe { PAGE_DIR.as_mut().expect("no paging?") };
    state.copy_on_write_from(&mut current.state.pages, 0, kernel_start);
    state.copy_pages_from(dir, kernel_start, 1024);
    
    let task = Task::from_state(state);
    let id = task.id;

    // create new task with provided state
    add_task(task);

    // return reference to new task
    Ok(get_task_mut(pid_to_id(id).unwrap()).unwrap())
}
