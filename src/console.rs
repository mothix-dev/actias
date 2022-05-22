//! generic text mode console interface and ANSI terminal emulator

/*
TODO: define basic trait for text consoles
 - raw (i.e. serial), screen (vt100 emulator) modes
 - screen modes use other trait to write single or multiple bytes with a certain color at a position on the screen,
    making portability easier
*/

use num_enum::FromPrimitive;
use alloc::boxed::Box;
use core::fmt::Write;
use crate::platform::create_console;

/// text colors
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromPrimitive)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    #[num_enum(default)]
    White = 15,
}

/// text colors for a specific character (foreground + background)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorCode {
    pub foreground: Color,
    pub background: Color,
}

impl Default for ColorCode {
    /// default color
    fn default() -> Self {
        Self {
            foreground: Color::LightGray,
            background: Color::Black,
        }
    }
}

/// color of panic screens
pub const PANIC_COLOR: ColorCode = ColorCode {
    foreground: Color::White,
    background: Color::Red,
};

/// trait for a text console
pub trait TextConsole: Write {
    fn puts(&mut self, string: &str);
    fn clear(&mut self);
    fn set_color(&mut self, color: ColorCode);
    fn get_color(&self) -> ColorCode;
}

/// interface our fancy text console(s) use to talk to lower level 
pub trait RawTextConsole {
    fn write_char(&mut self, x: u16, y: u16, color: ColorCode, c: u8);
    //fn write_string(&mut self, x: u16, y: u16, color: ColorCode, string: &str);
    fn clear(&mut self, x0: u16, y0: u16, x1: u16, y1: u16, color: ColorCode);
    fn copy(&mut self, y0: u16, y1: u16, height: u16); // we dont need to scroll horizontally
}

/// simple text console, doesn't implement ANSI control codes
pub struct SimpleConsole {
    pub raw: Box<dyn RawTextConsole + Sync>,
    pub width: u16,
    pub height: u16,
    pub cursor_x: u16,
    pub cursor_y: u16,
    pub color: ColorCode,
}

impl SimpleConsole {
    pub fn new(raw: Box<dyn RawTextConsole + Sync>, width: u16, height: u16) -> Self {
        Self {
            raw, width, height,
            cursor_x: 0,
            cursor_y: 0,
            color: ColorCode::default(),
        }
    }

    fn newline(&mut self) {
        self.cursor_x = 0;
        self.cursor_y += 1;
        if self.cursor_y >= self.height { // scroll screen
            self.raw.copy(1, 0, self.height - 1);
            self.raw.clear(0, self.height - 1, self.width, self.height, self.color);
            self.cursor_y = self.height - 1;
        }
    }
}

impl TextConsole for SimpleConsole {
    fn puts(&mut self, string: &str) {
        for c in string.bytes() {
            match c {
                b'\n' => {
                    self.newline();
                },
                b'\r' => {
                    self.cursor_x = 0;
                },
                b'\x08' => { // rust doesn't have \b lmao
                    if self.cursor_x > 0 {
                        self.cursor_x -= 1;
                    }
                },
                b'\t' => {
                    self.cursor_x = ((self.cursor_x / 4) * 4) + 4;
                    if self.cursor_x >= self.width {
                        self.newline();
                    }
                },
                _ => {
                    self.raw.write_char(self.cursor_x, self.cursor_y, self.color, c);
                    self.cursor_x += 1;
                    if self.cursor_x >= self.width {
                        self.newline();
                    }
                },
            }
        }
    }

    fn clear(&mut self) {
        self.raw.clear(0, 0, self.width, self.height, self.color);
    }

    fn set_color(&mut self, color: ColorCode) {
        self.color = color;
    }

    fn get_color(&self) -> ColorCode {
        self.color
    }
}

impl core::fmt::Write for SimpleConsole {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.puts(s);
        Ok(())
    }
}

/// global text console
static mut CONSOLE: Option<Box<dyn TextConsole + Sync>> = None;

pub fn init() {
    debug!("initializing console");
    unsafe {
        CONSOLE = Some(Box::new(create_console()));
    }
}

pub fn get_console() -> Option<&'static mut Box<(dyn TextConsole + Sync + 'static)>> {
    unsafe {
        CONSOLE.as_mut()
    }
}
