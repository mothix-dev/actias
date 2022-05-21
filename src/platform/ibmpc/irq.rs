//! IRQs

use super::io::outb;
use crate::arch::ints::{IDT, IDTEntry, IDTFlags, ExceptionStackFrame, SyscallRegisters};
use crate::tasks::{get_current_task_mut, get_current_task, switch_tasks};

/// interrupt stub handler for unhandled interrupts
unsafe extern "x86-interrupt" fn stub_handler(_frame: ExceptionStackFrame) {
    log!("unknown interrupt");

    outb(0x20, 0x20);
}

unsafe extern "x86-interrupt" fn stub_handler_2(_frame: ExceptionStackFrame) {
    log!("unknown interrupt");

    outb(0xa0, 0x20);
    outb(0x20, 0x20);
}

#[no_mangle]
pub unsafe extern "C" fn timer_handler(mut regs: SyscallRegisters) {
    // save state of current task
    get_current_task_mut().state.save(&regs);

    // switch to next task
    switch_tasks();

    // load state of new current task
    get_current_task().state.load(&mut regs);

    // reset interrupt controller
    outb(0x20, 0x20);
}

/// initializes PIT at specified frequency in Hz
pub fn init_timer(rate: u32) {
    let divisor = 1193180 / rate;

    let l = (divisor & 0xff) as u8;
    let h = ((divisor >> 8) & 0xff) as u8;

    unsafe {
        outb(0x43, 0x36);
        outb(0x40, l);
        outb(0x40, h);
    }
}

extern "C" {
    /// wrapper around timer_handler to save and restore state
    fn timer_handler_wrapper() -> !;
}

pub unsafe fn init() {
    // set up interrupt controller
    outb(0x20, 0x11);
    outb(0xa0, 0x11);
    outb(0x21, 0x20);
    outb(0xa1, 0x28);
    outb(0x21, 0x04);
    outb(0xa1, 0x02);
    outb(0x21, 0x01);
    outb(0xa1, 0x01);
    outb(0x21, 0x0);
    outb(0xa1, 0x0);

    // initialize timer at 200 Hz
    init_timer(200);

    // set up interrupt stubs
    for i in 33..40 {
        IDT[i] = IDTEntry::new(stub_handler as *const (), IDTFlags::External);
    }

    for i in 40..48 {
        IDT[i] = IDTEntry::new(stub_handler_2 as *const (), IDTFlags::External);
    }

    IDT[32] = IDTEntry::new(timer_handler_wrapper as *const (), IDTFlags::External);
}