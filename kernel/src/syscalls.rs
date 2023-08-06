use crate::arch::bsp::RegisterContext;
use common::syscalls::Syscalls;
use log::error;

pub type Registers = <crate::arch::InterruptManager as crate::arch::bsp::InterruptManager>::Registers;

/// low-level syscall handler. handles the parsing, execution, and error handling of syscalls
pub fn syscall_handler(regs: &mut Registers, num: u32, _arg0: usize, _arg1: usize, _arg2: usize, _arg3: usize) {
    let syscall = Syscalls::try_from(num);
    match syscall {
        Ok(Syscalls::IsComputerOn) => regs.syscall_return(Ok(1)),
        Err(err) => error!("invalid syscall {num} ({err})"),
    }
}
