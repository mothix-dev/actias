use alloc::vec::Vec;
use common::types::{Errno, Result};
use core::mem::size_of;

/// describes all the ABIs the kernel knows about
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ABI {
    /// arguments are passed in reverse order on the stack (nothing in registers), caller cleans up the stack
    Cdecl,

    /// first two arguments that fit are passed in ecx and edx, rest are passed on the stack, callee cleans up the stack
    Fastcall,
}

pub struct CallBuilder {
    abi: ABI,
    stack: Vec<u8>,
    registers: Vec<Option<usize>>,
    return_addr: Option<usize>,
}

impl CallBuilder {
    pub fn new(abi: ABI) -> Result<Self> {
        let mut new = Self {
            abi,
            stack: Vec::new(),
            registers: Vec::new(),
            return_addr: None,
        };

        match new.abi {
            ABI::Fastcall => {
                new.registers.try_reserve(2).map_err(|_| Errno::OutOfMemory)?;
                new.registers.push(None);
                new.registers.push(None);
            }
            _ => (),
        }

        Ok(new)
    }

    pub fn return_addr(mut self, addr: Option<usize>) -> Self {
        self.return_addr = addr;
        self
    }

    fn add_to_stack<T: Sized>(&mut self, argument: &T) -> Result<()> {
        self.stack.try_reserve(size_of::<T>()).map_err(|_| Errno::OutOfMemory)?;
        unsafe { self.stack.extend_from_slice(core::slice::from_raw_parts(argument as *const _ as *const u8, size_of::<T>())) }
        Ok(())
    }

    pub fn argument<T: Sized>(mut self, argument: &T) -> Result<Self> {
        match self.abi {
            ABI::Cdecl => {
                if self.stack.is_empty() {
                    self.add_to_stack(&0_usize)?;
                }

                self.add_to_stack(argument)?;
            }
            ABI::Fastcall => {
                if size_of::<T>() <= size_of::<usize>() && self.registers.len() < 4 {
                    self.registers.push(Some(unsafe { *(argument as *const _ as *const usize) }));
                } else {
                    if self.stack.is_empty() {
                        self.add_to_stack(&0_usize)?;
                    }

                    self.add_to_stack(argument)?;
                }
            }
        }

        Ok(self)
    }

    pub fn finish(mut self) -> Result<BuiltCallArguments> {
        let should_write_stack = match self.abi {
            ABI::Cdecl | ABI::Fastcall => {
                if let Some(addr) = self.return_addr {
                    if self.stack.is_empty() {
                        self.add_to_stack(&addr)?;
                    } else {
                        self.stack[0..4].copy_from_slice(unsafe { core::slice::from_raw_parts((&addr) as *const _ as *const u8, size_of::<usize>()) });
                    }

                    true
                } else {
                    self.stack.len() > size_of::<usize>()
                }
            } //_ => false,
        };

        let should_write_registers = match self.abi {
            ABI::Fastcall => self.registers.len() > 2,
            _ => false,
        };

        Ok(BuiltCallArguments {
            stack: self.stack,
            should_write_stack,
            registers: self.registers,
            should_write_registers,
        })
    }
}

#[derive(Clone, Debug)]
pub struct BuiltCallArguments {
    /// values to be placed onto the stack in reverse order
    pub stack: Vec<u8>,

    /// whether the stack of the process should be updated with the values here
    pub should_write_stack: bool,

    /// values to be placed into registers
    pub registers: Vec<Option<usize>>,

    /// whether the registers of the process should be updated with the values here
    pub should_write_registers: bool,
}
