//! IRQs

use super::io::outb;
use crate::{
    arch::{
        LINKED_BASE,
        ints::{IDT, IDTEntry, IDTFlags, ExceptionStackFrame, SyscallRegisters},
        paging::PAGE_DIR,
        halt,
    },
    tasks::{IN_TASK, CURRENT_TERMINATED, get_current_task_mut, switch_tasks, num_tasks},
};

/// interrupt stub handler for unhandled interrupts
unsafe extern "x86-interrupt" fn stub_handler(_frame: ExceptionStackFrame) {
    log!("unknown interrupt");

    // reset master interrupt controller
    outb(0x20, 0x20);
}

unsafe extern "x86-interrupt" fn stub_handler_2(_frame: ExceptionStackFrame) {
    log!("unknown interrupt");

    // reset slave interrupt controller
    outb(0xa0, 0x20);
    outb(0x20, 0x20);
}

/// timer interrupt handler, currently just switches tasks
#[no_mangle]
pub unsafe extern "C" fn timer_handler(mut regs: SyscallRegisters) {
    // TODO: task priority, task execution timers

    // we don't want to preempt the kernel- all sorts of bad things could happen
    if !IN_TASK {
        outb(0x20, 0x20);
        return;
    }

    // has the current task been terminated?
    if CURRENT_TERMINATED {
        // it no longer exists, so all we need to do is clear the flag
        CURRENT_TERMINATED = false;
    } else {
        // save state of current task
        get_current_task_mut().expect("no tasks?").state.save(&regs);
    }

    if num_tasks() == 0 {
        outb(0x20, 0x20);
        halt();
    }

    // switch to next task
    switch_tasks();

    // load state of new current task
    let current = get_current_task_mut().expect("no tasks?");

    current.state.load(&mut regs);

    // get reference to global page directory
    let dir = PAGE_DIR.as_mut().expect("paging not initialized");

    // has the kernel page directory been updated?
    if current.state.page_updates != dir.page_updates {
        // get page directory index of the start of the kernel's address space
        let idx = LINKED_BASE >> 22;

        // copy from the kernel's page directory to the task's
        current.state.copy_pages_from(dir, idx, 1024);

        // the task's page directory is now up to date (at least for our purposes)
        current.state.page_updates = dir.page_updates;
    }

    // switch to task's page directory
    get_current_task_mut().expect("no tasks?").state.pages.switch_to();

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

// init IRQs
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

    // initialize timer at 100 Hz
    init_timer(100);

    // set up interrupt stubs
    for i in 33..40 {
        IDT[i] = IDTEntry::new(stub_handler as *const (), IDTFlags::External);
    }

    for i in 40..48 {
        IDT[i] = IDTEntry::new(stub_handler_2 as *const (), IDTFlags::External);
    }

    // set up interrupt handler for PIT
    IDT[32] = IDTEntry::new(timer_handler_wrapper as *const (), IDTFlags::External);
}
