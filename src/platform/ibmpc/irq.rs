//! IRQs

use crate::{
    arch::{
        LINKED_BASE,
        ints::{IDT, IDTEntry, IDTFlags, ExceptionStackFrame, SyscallRegisters},
        paging::PAGE_DIR,
    },
    tasks::{IN_TASK, CURRENT_TERMINATED, get_current_task_mut, switch_tasks, num_tasks},
    console::get_console,
};
use core::arch::asm;
use x86::io::{inb, outb};
use super::keyboard::{KEYMAP, KEYMAP_SHIFT, KEYMAP_CTRL, KEYMAP_META, KEYMAP_META_SHIFT, KEYMAP_META_CTRL};

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

    debug!("context switch!");

    // we don't want to preempt the kernel- all sorts of bad things could happen
    if !IN_TASK {
        outb(0x20, 0x20);
        return;
    }

    if num_tasks() == 0 {
        outb(0x20, 0x20);
        loop {
            asm!("sti; hlt");
        }
    } else if num_tasks() > 1 || CURRENT_TERMINATED { // only context switch if we need to
        // has the current task been terminated?
        if CURRENT_TERMINATED {
            // it no longer exists, so all we need to do is clear the flag
            CURRENT_TERMINATED = false;
        } else {
            // save state of current task
            get_current_task_mut().expect("no tasks?").state.save(&regs);
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
    }

    // reset interrupt controller
    outb(0x20, 0x20);
}

static mut EXTENDED: bool = false;

/// interrupt handler for keyboard
unsafe extern "x86-interrupt" fn keyboard_handler(_frame: ExceptionStackFrame) {
    let input = inb(0x60);

    if input == 0xe0 {
        EXTENDED = true;
    } else {
        let key = if EXTENDED {
            EXTENDED = false;
            input | 0x80
        } else {
            input & !0x80
        };
        
        let state: bool = input & 0x80 == 0; // true = press, false = release

        if state {
            debug!("key down {:#x}", key);
        } else {
            debug!("key up {:#x}", key);
        }

        if let Some(console) = get_console() {
            let keysym =
                if console.get_alt() && console.get_ctrl() {
                    KEYMAP_META_CTRL[key as usize]
                } else if console.get_alt() && console.get_shift() {
                    KEYMAP_META_SHIFT[key as usize]
                } else if console.get_alt() {
                    KEYMAP_META[key as usize]
                } else if console.get_ctrl() {
                    KEYMAP_CTRL[key as usize]
                } else if console.get_shift() {
                    KEYMAP_SHIFT[key as usize]
                } else {
                    KEYMAP[key as usize]
                };

            console.key_press(keysym, state);
        }
    }

    // reset master interrupt controller
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
    debug!("initializing irqs");

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

    // set up interrupt handler for PIT
    IDT[32] = IDTEntry::new(timer_handler_wrapper as *const (), IDTFlags::External);

    // set up interrupt handler for keyboard
    IDT[33] = IDTEntry::new(keyboard_handler as *const (), IDTFlags::External);

    // set up interrupt stubs
    for i in 34..40 {
        IDT[i] = IDTEntry::new(stub_handler as *const (), IDTFlags::External);
    }

    for i in 40..48 {
        IDT[i] = IDTEntry::new(stub_handler_2 as *const (), IDTFlags::External);
    }
}
