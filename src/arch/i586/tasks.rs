//! low level i586-specific task switching

use super::ints::SyscallRegisters;
use super::paging::PageDirectory;

#[derive(Default)]
pub struct TaskState {
    pub registers: SyscallRegisters,
    pub pages: PageDirectory,
}

impl TaskState {
    pub fn new() -> Self {
        Self {
            registers: Default::default(),
            pages: PageDirectory::new(),
        }
    }

    pub fn save(&mut self, regs: &SyscallRegisters) {
        self.registers = regs.clone();
    }

    pub fn load(&self, regs: &mut SyscallRegisters) {
        unsafe {
            *regs = self.registers; // replace all registers with our own (:
        }
    }
}
