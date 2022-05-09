/*
 * Rust BareBones OS
 * - By John Hodge (Mutabah/thePowersGang) 
 *
 * unwind.rs
 * - Stack unwind (panic) handling
 *
 * == LICENCE ==
 * This code has been put into the public domain, there are no restrictions on
 * its use, and the author takes no liability.
 */

use core::fmt;
use crate::arch::vga::create_console;
use crate::console::{TextConsole, SimpleConsole, PANIC_COLOR};

#[panic_handler]
pub fn panic_implementation(info: &::core::panic::PanicInfo) -> ! {
    let (file,line) = match info.location() {
        Some(loc) => (loc.file(), loc.line(),),
        None => ("", 0),
    };

    if let Some(m) = info.message() {
        log!("PANIC file='{}', line={} :: {}", file, line, m);
    } else if let Some(m) = info.payload().downcast_ref::<&str>() {
        log!("PANIC file='{}', line={} :: {}", file, line, m);
    } else {
        log!("PANIC file='{}', line={} :: ?", file, line);
    }

    // do this after in case anything breaks
    let mut raw = create_console();
    let mut console = SimpleConsole::new(&mut raw, 80, 25);

    console.color = PANIC_COLOR;
    console.clear();

    if let Some(m) = info.message() {
        fmt::write(&mut console, format_args!("PANIC file='{}', line={} :: {}\n", file, line, m)).expect("lol. lmao");
    } else if let Some(m) = info.payload().downcast_ref::<&str>() {
        fmt::write(&mut console, format_args!("PANIC file='{}', line={} :: {}\n", file, line, m)).expect("lol. lmao");
    } else {
        fmt::write(&mut console, format_args!("PANIC file='{}', line={} :: ?\n", file, line)).expect("lol. lmao");
    }
    loop {}
}

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone,Copy)]
pub enum _Unwind_Reason_Code {
    _URC_NO_REASON = 0,
    _URC_FOREIGN_EXCEPTION_CAUGHT = 1,
    _URC_FATAL_PHASE2_ERROR = 2,
    _URC_FATAL_PHASE1_ERROR = 3,
    _URC_NORMAL_STOP = 4,
    _URC_END_OF_STACK = 5,
    _URC_HANDLER_FOUND = 6,
    _URC_INSTALL_CONTEXT = 7,
    _URC_CONTINUE_UNWIND = 8,
}

#[allow(non_camel_case_types)]
#[derive(Clone,Copy)]
pub struct _Unwind_Context;

#[allow(non_camel_case_types)]
pub type _Unwind_Action = u32;
static _UA_SEARCH_PHASE: _Unwind_Action = 1;

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone,Copy)]
pub struct _Unwind_Exception {
    exception_class: u64,
    exception_cleanup: fn(_Unwind_Reason_Code,*const _Unwind_Exception),
    private: [u64; 2],
}

#[no_mangle]
#[allow(non_snake_case)]
#[allow(clippy::empty_loop)]
pub fn _Unwind_Resume() {
    loop {}
}
