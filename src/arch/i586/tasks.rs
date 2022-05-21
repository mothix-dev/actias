//! low level i586-specific task switching

use super::ints::SyscallRegisters;
use super::paging::PageDirectory;
use crate::arch::paging::PAGE_DIR;

pub struct TaskState {
    pub registers: SyscallRegisters,
    pub pages: PageDirectory,
    pub page_updates: usize,
}

impl TaskState {
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

    pub fn save(&mut self, regs: &SyscallRegisters) {
        self.registers = regs.clone();
    }

    pub fn load(&self, regs: &mut SyscallRegisters) {
        unsafe {
            *regs = self.registers; // replace all registers with our own (:
        }
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

            //log!("self.pages.tables[{}] = {:?}, self.pages.tables_physical[{}] = {:#x}", i, self.pages.tables[i], i, unsafe { (*self.pages.tables_physical)[i] });
        }
    }
}

impl Default for TaskState {
    fn default() -> Self {
        Self::new()
    }
}
