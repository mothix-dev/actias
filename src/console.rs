// text mode console

/*
TODO: define basic trait for text consoles
 - raw (i.e. serial), screen (vt100 emulator) modes
 - screen modes use other trait to write single or multiple bytes with a certain color at a position on the screen,
    making portability easier
*/

use num_enum::FromPrimitive;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorCode {
    pub foreground: Color,
    pub background: Color,
}

pub trait TextConsole {
    fn puts(&mut self, string: &str);
}

pub trait RawTextConsole {
    fn write_char(&mut self, x: u16, y: u16, color: ColorCode, c: u8);
    fn write_string(&mut self, x: u16, y: u16, color: ColorCode, string: &str);
    fn clear(&mut self);
}
