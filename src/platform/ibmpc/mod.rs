pub mod debug;
pub mod io;
pub mod vga;

use crate::console::{TextConsole, SimpleConsole};

pub fn create_console() -> SimpleConsole {
    let raw = vga::create_console();
    let mut console = SimpleConsole::new(raw, 80, 25);

    console.clear();

    console
}
