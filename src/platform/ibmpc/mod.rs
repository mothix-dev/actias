pub mod debug;
pub mod io;
pub mod vga;

/*use crate::console::{TextConsole, SimpleConsole, PANIC_COLOR};

pub fn create_panic_console() -> SimpleConsole<'static> {
    let mut raw = vga::create_console();
    let mut console = SimpleConsole::new(&mut raw, 80, 25);

    console.color = PANIC_COLOR;
    console.clear();

    console
}*/
