//! low level i586-specific task switching

use super::ints::SyscallRegisters;
use super::paging::{PAGE_DIR, PageDirectory, PageTableFlags};
use crate::arch::PAGE_SIZE;
use core::arch::asm;

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
                    
                    if u16::from(flags & PageTableFlags::ReadWrite) > 0 {
                        flags &= !PageTableFlags::ReadWrite;
                        flags |= PageTableFlags::CopyOnWrite;
                    }

                    page.set_flags(flags);
                    page.set_address(orig_page.get_address());
                }
            }
        }
    }

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

    pub fn free_page(&mut self, addr: u32) {
        assert!(addr % PAGE_SIZE as u32 == 0, "address is not page aligned");

        if let Some(page) = self.pages.get_page(addr.try_into().unwrap(), false) {
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
