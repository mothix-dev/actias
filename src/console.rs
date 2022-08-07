//! generic text mode console interface and ANSI terminal emulator

use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec, vec::Vec,
};
use core::{
    fmt::Write,
    cmp::{max, min},
    str::FromStr,
};
use crate::{
    platform::create_console,
    fs::tree::{File, Directory, SymLink},
    types::{
        errno::Errno,
        keysym::KeySym,
        file::{Permissions, FileKind, FileStatus},
    },
};
use num_enum::FromPrimitive;
use x86::io::{inb, outb};

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
    /// print a string to the console
    fn puts(&mut self, string: &str);

    /// clear the console
    fn clear(&mut self);

    /// set the color of the console, changes what color strings are printed as
    fn set_color(&mut self, color: ColorCode);

    /// gets the current color of the console
    fn get_color(&self) -> ColorCode;

    /// send a key press to the console
    fn key_press(&mut self, key: KeySym, state: bool);

    /// get ctrl state from console
    fn get_ctrl(&self) -> bool;

    /// get shift state from console
    fn get_shift(&self) -> bool;

    /// get alt state from console
    fn get_alt(&self) -> bool;

    /// set the console to use raw or cooked mode
    fn set_raw_mode(&mut self, raw: bool);

    /// gets whether the console is in raw mode
    fn get_raw_mode(&self) -> bool;

    /// sets cursor position of console
    fn set_cursor(&mut self, x: usize, y: usize);

    /// gets input buffer for reading
    fn get_input_buffer(&mut self) -> &mut Vec<u8>;

    /// reads n bytes from the input buffer, returning read bytes as a vec
    fn read_bytes(&mut self, n: usize) -> Vec<u8>;
}

/// interface our fancy text console(s) use to talk to lower level 
pub trait RawTextConsole {
    fn write_char(&mut self, x: u16, y: u16, color: ColorCode, c: char);
    //fn write_string(&mut self, x: u16, y: u16, color: ColorCode, string: &str);
    fn clear(&mut self, x0: u16, y0: u16, x1: u16, y1: u16, color: ColorCode);
    fn copy(&mut self, y0: u16, y1: u16, height: u16); // we dont need to scroll horizontally
}

const MAX_KEYS_BUFFERED: usize = 1024;

/// ok well it's not that simple anymore but i don't care
pub struct SimpleConsole {
    /// raw console we're outputting with
    pub raw: Box<dyn RawTextConsole + Sync>,


    /// width of console
    pub width: u16,

    /// height of console
    pub height: u16,

    /// x position of cursor
    pub cursor_x: u16,

    /// y position of cursor
    pub cursor_y: u16,

    /// current color we're drawing with
    pub color: ColorCode,


    /// is this console in raw mode?
    pub raw_mode: bool,


    /// input cache, used for holding input characters or lines while waiting for the user to read them
    pub lines: Vec<u8>,

    /// current line in cooked mode
    pub line: Vec<char>,


    /// whether alt is held down or not
    pub alt_state: bool,

    /// whether ctrl is held down or not
    pub ctrl_state: bool,

    /// whether shift is held down or not
    pub shift_state: bool,


    /// state of ansi control command state machine
    pub control_mode: u8,

    /// buffer used to store data for ansi control commands
    pub control_buf: Vec<char>,
}

impl SimpleConsole {
    pub fn new(raw: Box<dyn RawTextConsole + Sync>, width: u16, height: u16) -> Self {
        Self::enable_cursor();

        let new = Self {
            raw, width, height,
            cursor_x: 0,
            cursor_y: 0,
            color: Default::default(),
            lines: Vec::with_capacity(MAX_KEYS_BUFFERED),
            line: Vec::with_capacity(MAX_KEYS_BUFFERED),
            alt_state: false,
            ctrl_state: false,
            shift_state: false,
            raw_mode: false,
            control_mode: 0,
            control_buf: Vec::with_capacity(MAX_KEYS_BUFFERED),
        };

        new.update_cursor();

        new
    }

    fn reset(&mut self) {
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.color = Default::default();
        self.lines.clear();
        self.line.clear();
        self.raw_mode = false;
        self.control_mode = 0;
        self.control_buf.clear();
        self.clear();
        self.update_cursor();
    }

    fn scroll_up(&mut self, num: u16) {
        self.raw.copy(num, 0, self.height - num);
        self.raw.clear(0, self.height - num, self.width - 1, self.height - 1, self.color);
    }

    fn scroll_down(&mut self, num: u16) {
        self.raw.copy(0, num, self.height - num);
        self.raw.clear(0, 0, self.width - 1, num - 1, self.color);
    }

    fn newline(&mut self) {
        self.cursor_x = 0;
        self.cursor_y += 1;
        if self.cursor_y >= self.height { // scroll screen
            self.scroll_up(1);
            self.cursor_y = self.height - 1;
        }
    }

    fn putc(&mut self, c: char) {
        match c {
            '\n' => {
                self.newline();
            },
            '\r' => {
                self.cursor_x = 0;
            },
            '\x08' => { // rust doesn't have \b lmao
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                }
            },
            '\t' => {
                self.cursor_x = ((self.cursor_x / 8) * 8) + 8;
                if self.cursor_x >= self.width {
                    self.newline();
                }
            },
            '\x1b' => {
                self.control_mode = 1;
            },
            _ => {
                // ansi control sequence state machine
                if self.control_mode == 1 && self.control_buf.is_empty() && c == '[' {
                    self.control_mode = 2;
                } else if self.control_mode == 1 && self.control_buf.is_empty() && c == 'c' {
                    self.reset();
                    self.control_mode = 0;
                } else if self.control_mode == 1 {
                    self.control_mode = 0;
                } else if self.control_mode == 2 {
                    if (c as u32 >= 'a' as u32 && c as u32 <= 'z' as u32) || (c as u32 >= 'A' as u32 && c as u32 <= 'Z' as u32) {
                        self.control_mode = 0;

                        // convert buffer into string for ease of use
                        let string = self.control_buf.iter().copied().collect::<String>();

                        // parse command
                        match c {
                            // move cursor up
                            'A' => {
                                let amt = string.parse::<u16>().unwrap_or(1);

                                if self.cursor_y - amt > 0 {
                                    self.cursor_y -= amt;
                                } else {
                                    self.cursor_y = 0;
                                }
                            },

                            // move cursor down
                            'B' => {
                                let amt = string.parse::<u16>().unwrap_or(1);

                                if self.cursor_y + amt < self.height - 1 {
                                    self.cursor_y += amt;
                                } else {
                                    self.cursor_y = self.height - 1;
                                }
                            },

                            // move cursor right
                            'C' => {
                                let amt = string.parse::<u16>().unwrap_or(1);

                                if self.cursor_x + amt < self.width - 1 {
                                    self.cursor_x += amt;
                                } else {
                                    self.cursor_x = self.width - 1;
                                }
                            },
                            
                            // move cursor left
                            'D' => {
                                let amt = string.parse::<u16>().unwrap_or(1);

                                if self.cursor_x - amt > 0 {
                                    self.cursor_x -= amt;
                                } else {
                                    self.cursor_x = 0;
                                }
                            },

                            // move cursor to beginning of line n lines down
                            'E' => {
                                let amt = string.parse::<u16>().unwrap_or(1);

                                if self.cursor_y + amt < self.height - 1 {
                                    self.cursor_y += amt;
                                } else {
                                    self.cursor_y = self.height - 1;
                                }

                                self.cursor_x = 0;
                            },

                            // move cursor to beginning of line n lines up
                            'F' => {
                                let amt = string.parse::<u16>().unwrap_or(1);

                                if self.cursor_y - amt > 0 {
                                    self.cursor_y -= amt;
                                } else {
                                    self.cursor_y = 0;
                                }

                                self.cursor_x = 0;
                            },

                            // move cursor to horizontal position
                            'G' => self.cursor_x = min(max(0, string.parse::<u16>().unwrap_or(1)), self.width - 1),

                            // move cursor to specified position
                            'H' | 'f' => {
                                let mut split = string.split(';');
                                self.cursor_y = split.next().unwrap_or("").parse::<u16>().unwrap_or(1) - 1;
                                self.cursor_x = split.next().unwrap_or("").parse::<u16>().unwrap_or(1) - 1;
                            },

                            // clear the entire screen or part of the screen
                            'J' => match string.parse::<u16>().unwrap_or(0) {
                                0 => {
                                    self.raw.clear(self.cursor_x, self.cursor_y, self.width - 1, self.cursor_y, self.color);
                                    self.raw.clear(0, self.cursor_y, self.width - 1, self.height - 1, self.color);
                                },
                                1 => {
                                    self.raw.clear(self.cursor_x, self.cursor_y, 0, self.cursor_y, self.color);
                                    self.raw.clear(0, 0, self.width - 1, self.cursor_y, self.color);
                                },
                                _ => self.raw.clear(0, 0, self.width - 1, self.height - 1, self.color),
                            },

                            // clear from the cursor to the start or end of the line
                            'K' => match string.parse::<u16>().unwrap_or(0) {
                                0 => self.raw.clear(self.cursor_x, self.cursor_y, self.width - 1, self.cursor_y, self.color),
                                1 => self.raw.clear(self.cursor_x, self.cursor_y, 0, self.cursor_y, self.color),
                                _ => self.raw.clear(0, self.cursor_y, self.width - 1, self.cursor_y, self.color),
                            },

                            // scroll screen up, adding new lines at the bottom
                            'S' => self.scroll_up(string.parse::<u16>().unwrap_or(1)),

                            // scroll screen down, adding new lines at the top
                            'T' => self.scroll_down(string.parse::<u16>().unwrap_or(1)),

                            // change the color!
                            'm' => match string.parse::<u16>().unwrap_or(0) {
                                // reset color to default
                                0 => self.color = Default::default(),

                                // increased intensity
                                1 => if (self.color.foreground as u8) < 8 {
                                    self.color.foreground = Color::from(self.color.foreground as u8 + 8);
                                },

                                // decreased intensity
                                2 => if (self.color.foreground as u8) >= 8 {
                                    self.color.foreground = Color::from(self.color.foreground as u8 - 8);
                                },

                                // swap foreground and background colors
                                7 => core::mem::swap(&mut self.color.foreground, &mut self.color.background),

                                // set foreground color
                                30 => self.color.foreground = Color::Black,
                                31 => self.color.foreground = Color::Blue,
                                32 => self.color.foreground = Color::Green,
                                33 => self.color.foreground = Color::Cyan,
                                34 => self.color.foreground = Color::Red,
                                35 => self.color.foreground = Color::Magenta,
                                36 => self.color.foreground = Color::Brown,
                                37 => self.color.foreground = Color::LightGray,

                                // reset foreground color to default
                                39 => self.color.foreground = Color::LightGray,

                                // set background color
                                40 => self.color.background = Color::Black,
                                41 => self.color.background = Color::Blue,
                                42 => self.color.background = Color::Green,
                                43 => self.color.background = Color::Cyan,
                                44 => self.color.background = Color::Red,
                                45 => self.color.background = Color::Magenta,
                                46 => self.color.background = Color::Brown,
                                47 => self.color.background = Color::LightGray,

                                // reset background color to default
                                49 => self.color.background = Color::Black,

                                // set foreground with intensity bit
                                90 => self.color.foreground = Color::DarkGray,
                                91 => self.color.foreground = Color::LightBlue,
                                92 => self.color.foreground = Color::LightGreen,
                                93 => self.color.foreground = Color::LightCyan,
                                94 => self.color.foreground = Color::LightRed,
                                95 => self.color.foreground = Color::Pink,
                                96 => self.color.foreground = Color::Yellow,
                                97 => self.color.foreground = Color::White,

                                // set background with intensity bit
                                100 => self.color.background = Color::DarkGray,
                                101 => self.color.background = Color::LightBlue,
                                102 => self.color.background = Color::LightGreen,
                                103 => self.color.background = Color::LightCyan,
                                104 => self.color.background = Color::LightRed,
                                105 => self.color.background = Color::Pink,
                                106 => self.color.background = Color::Yellow,
                                107 => self.color.background = Color::White,

                                _ => ()
                            },

                            // report the cursor's position
                            'n' => if string.parse::<u16>().unwrap_or(0) == 6 {
                                let mut bytes: Vec<u8> = format!("\x1b[{};{}R", self.cursor_y + 1, self.cursor_x + 1).bytes().collect();
                                
                                if self.lines.len() + bytes.len() <= MAX_KEYS_BUFFERED {
                                    self.lines.append(&mut bytes);
                                }
                            },

                            _ => debug!("unsupported ANSI control string: \"{}{}\"", string, c),
                        }

                        self.control_buf.clear();
                    } else if !(c as u32 >= '0' as u32 && c as u32 <= '9' as u32) && c as u32 != ':' as u32 && c as u32 != ';' as u32 {
                        self.control_mode = 0;

                        self.control_buf.clear();
                    } else {
                        self.control_buf.push(c);
                    }
                } else {
                    self.raw.write_char(self.cursor_x, self.cursor_y, self.color, c);
                    self.cursor_x += 1;
                    if self.cursor_x >= self.width {
                        self.newline();
                    }
                }
            },
        }
    }

    fn enable_cursor() {
        unsafe {
            outb(0x3d4, 0x0a);
            outb(0x3d5, (inb(0x3d5) & 0xc0) | 0xd);
        
            outb(0x3d4, 0x0b);
            outb(0x3d5, (inb(0x3d5) & 0xe0) | 0xe);
        }
    }

    fn update_cursor(&self) {
        let pos = self.cursor_y * self.width + self.cursor_x;

        unsafe {
            outb(0x3d4, 0x0f);
            outb(0x3d5, (pos & 0xff) as u8);
            outb(0x3d4, 0x0e);
            outb(0x3d5, ((pos >> 8) & 0xff) as u8);
        }
    }

    // input a string into the line buffer or raw input buffer
    fn puts_inputted(&mut self, s: &str) {
        if (self.raw_mode && self.lines.len() + s.len() <= MAX_KEYS_BUFFERED) || (!self.raw_mode && self.lines.len() + self.line.len() + s.len() <= MAX_KEYS_BUFFERED) {
            for c in s.chars() {
                self.putc_inputted(c);
            }
        }
    }

    // input a character into the line buffer or raw input buffer
    fn putc_inputted(&mut self, c: char) {
        if self.raw_mode {
            if (c as u16) < 0x0100 {
                self.lines.push(c as u8);
            }
        } else {
            self.putc(c);

            self.line.push(c);

            self.update_cursor();
        }
    }

    // input the provided terminal input sequence, adding an escape to the beginning if applicable
    fn input_sequence(&mut self, seq: &str) {
        if self.raw_mode {
            self.puts_inputted(&format!("\x1b{}", seq));
        } else {
            self.puts_inputted(&format!("^[{}", seq));
        }
    }
}

impl TextConsole for SimpleConsole {
    fn puts(&mut self, string: &str) {
        for c in string.chars() {
            self.putc(c);
        }

        self.update_cursor();
    }

    fn clear(&mut self) {
        self.raw.clear(0, 0, self.width - 1, self.height - 1, self.color);
    }

    fn set_color(&mut self, color: ColorCode) {
        self.color = color;
    }

    fn get_color(&self) -> ColorCode {
        self.color
    }

    fn key_press(&mut self, key: KeySym, state: bool) {
        debug!("got key {}", key);
        match key {
            KeySym::Null => (),
            KeySym::Ctrl | KeySym::LeftCtrl | KeySym::RightCtrl => self.ctrl_state = state,
            KeySym::Shift | KeySym::LeftShift | KeySym::RightShift => self.shift_state = state,
            KeySym::Alt | KeySym::AltGr => self.alt_state = state,

            _ => if state {
                match key {
                    KeySym::Escape => self.input_sequence(""),

                    KeySym::Up => self.input_sequence("[A"),
                    KeySym::Down => self.input_sequence("[B"),
                    KeySym::Right => self.input_sequence("[C"),
                    KeySym::Left => self.input_sequence("[D"),

                    KeySym::End => self.input_sequence("[F"),
                    KeySym::KP5 => self.input_sequence("[G"),
                    KeySym::Home => self.input_sequence("[H"),

                    KeySym::Insert => self.input_sequence("[2~"),
                    KeySym::Delete => self.input_sequence("[3~"),
                    KeySym::PageUp => self.input_sequence("[5~"),
                    KeySym::PageDown => self.input_sequence("[6~"),

                    KeySym::F1 => self.input_sequence("[11~"),
                    KeySym::F2 => self.input_sequence("[12~"),
                    KeySym::F3 => self.input_sequence("[13~"),
                    KeySym::F4 => self.input_sequence("[14~"),
                    KeySym::F5 => self.input_sequence("[15~"),

                    KeySym::F6 => self.input_sequence("[17~"),
                    KeySym::F7 => self.input_sequence("[18~"),
                    KeySym::F8 => self.input_sequence("[19~"),
                    KeySym::F9 => self.input_sequence("[20~"),
                    KeySym::F10 => self.input_sequence("[21~"),

                    KeySym::F11 => self.input_sequence("[23~"),
                    KeySym::F12 => self.input_sequence("[24~"),
                    KeySym::F13 => self.input_sequence("[25~"),
                    KeySym::F14 => self.input_sequence("[26~"),

                    KeySym::F15 => self.input_sequence("[28~"),
                    KeySym::F16 => self.input_sequence("[29~"),

                    KeySym::F17 => self.input_sequence("[31~"),
                    KeySym::F18 => self.input_sequence("[32~"),
                    KeySym::F19 => self.input_sequence("[33~"),
                    KeySym::F20 => self.input_sequence("[34~"),

                    _ => if self.raw_mode {
                        if (key as u16) < 0x0100 && self.lines.len() < MAX_KEYS_BUFFERED {
                            self.lines.push(key as u8);
                        }
                    } else {
                        match key {
                            KeySym::Backspace => if !self.line.is_empty() {
                                self.puts("\x08 \x08");
                                self.line.pop();
                            },
                            KeySym::Linefeed => {
                                if self.lines.len() + self.line.len() < MAX_KEYS_BUFFERED {
                                    // output a newline and add it to line
                                    self.putc('\n');
                                    self.line.push('\n');
        
                                    // add cooked line to buffer
                                    self.lines.append(&mut (self.line.iter().copied().collect::<String>().bytes().collect()));
        
                                    // clear current line
                                    self.line.clear();
                                }
                            },
                            _ => if (key as u16) < 0x0100 && self.lines.len() + self.line.len() < MAX_KEYS_BUFFERED {
                                self.putc_inputted(key as u8 as char); // writes the char to screen and inputs it in the line buffer
                            },
                        }
    
                        self.update_cursor();
                    },
                }
            }
        }
    }

    fn get_ctrl(&self) -> bool {
        self.ctrl_state
    }

    fn get_shift(&self) -> bool {
        self.shift_state
    }

    fn get_alt(&self) -> bool {
        self.alt_state
    }

    fn set_raw_mode(&mut self, raw: bool) {
        self.raw_mode = raw;
    }

    fn get_raw_mode(&self) -> bool {
        self.raw_mode
    }

    fn set_cursor(&mut self, x: usize, y: usize) {
        self.cursor_x = min(x as u16, self.width - 1);
        self.cursor_y = min(y as u16, self.height - 1);

        self.update_cursor();
    }

    fn get_input_buffer(&mut self) -> &mut Vec<u8> {
        &mut self.lines
    }

    fn read_bytes(&mut self, len: usize) -> Vec<u8> {
        if len >= self.lines.len() {
            self.lines.drain(..).collect()
        } else {
            self.lines.drain(..len).collect()
        }
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

pub struct ConsoleFile {
    pub permissions: Permissions,
    pub name: String,
}

impl File for ConsoleFile {
    fn get_permissions(&self) -> Permissions {
        self.permissions
    }

    fn set_permissions(&mut self, permissions: Permissions) -> Result<(), Errno> {
        self.permissions = permissions;
        Ok(())
    }
    
    fn write_at(&mut self, bytes: &[u8], _offset: u64) -> Result<usize, Errno> {
        if let Ok(str) = String::from_utf8(bytes.to_vec()) {
            get_console().unwrap().puts(&str);
            Ok(bytes.len())
        } else {
            Err(Errno::IllegalSequence) // probably not the right errno but it fits
        }
    }

    fn can_write_at(&self, _space: usize, _offset: u64) -> bool {
        true
    }

    fn read_at(&self, bytes: &mut [u8], _offset: u64) -> Result<usize, Errno> {
        let read = get_console().unwrap().read_bytes(bytes.len());

        bytes[..read.len()].copy_from_slice(&read);

        Ok(read.len())
    }

    fn can_read_at(&self, space: usize, _offset: u64) -> bool {
        get_console().unwrap().get_input_buffer().len() >= space
    }

    fn stat(&self, status: &mut FileStatus) -> Result<(), Errno> {
        *status = FileStatus {
            user_id: self.get_owner(),
            group_id: self.get_group(),
            size: self.get_size(),
            kind: FileKind::CharSpecial,
            .. Default::default()
        };

        Ok(())
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn set_name(&mut self, name: &str) -> Result<(), Errno> {
        self.name = name.to_string();
        Ok(())
    }

    fn get_size(&self) -> u64 {
        0
    }
}

pub struct SettingFile<T: FromStr + ToString> {
    pub permissions: Permissions,
    pub name: String,
    pub get_setting: fn() -> T,
    pub set_setting: fn(T),
}

impl<T: FromStr + ToString> File for SettingFile<T> {
    fn get_permissions(&self) -> Permissions {
        self.permissions
    }

    fn set_permissions(&mut self, permissions: Permissions) -> Result<(), Errno> {
        self.permissions = permissions;
        Ok(())
    }

    fn write_at(&mut self, bytes: &[u8], _offset: u64) -> Result<usize, Errno> {
        if let Ok(str) = String::from_utf8(bytes.to_vec()) {
            if let Ok(val) = str.parse::<T>() {
                (self.set_setting)(val);

                Ok(str.len())
            } else {
                Err(Errno::IllegalSequence)
            }
        } else {
            Err(Errno::IllegalSequence) // probably not the right errno but it fits
        }
    }

    fn can_write_at(&self, _space: usize, _offset: u64) -> bool {
        true
    }

    fn read_at(&self, bytes: &mut [u8], _offset: u64) -> Result<usize, Errno> {
        let string = (self.get_setting)().to_string();
        let string = string.as_bytes();

        if bytes.len() > string.len() {
            bytes[..string.len()].clone_from_slice(string);

            Ok(string.len())
        } else {
            Err(Errno::ResultTooLarge)
        }
    }

    fn can_read_at(&self, space: usize, _offset: u64) -> bool {
        space >= (self.get_setting)().to_string().len()
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn set_name(&mut self, name: &str) -> Result<(), Errno> {
        self.name = name.to_string();
        Ok(())
    }

    fn get_size(&self) -> u64 {
        0
    }
}

pub struct DriverDir {
    files: Vec<Box<dyn File>>,
    directories: Vec<Box<dyn Directory>>,
    links: Vec<Box<dyn SymLink>>,
}

impl Directory for DriverDir {
    fn get_permissions(&self) -> Permissions {
        Permissions::None
    }

    fn get_files(&self) -> &Vec<Box<dyn File>> {
        &self.files
    }

    fn get_files_mut(&mut self) -> &mut Vec<Box<dyn File>> {
        &mut self.files
    }

    fn get_directories(&self) -> &Vec<Box<dyn Directory>> {
        &self.directories
    }

    fn get_directories_mut(&mut self) -> &mut Vec<Box<dyn Directory>> {
        &mut self.directories
    }

    fn get_links(&self) -> &Vec<Box<dyn SymLink>> {
        &self.links
    }

    fn get_links_mut(&mut self) -> &mut Vec<Box<dyn SymLink>> {
        &mut self.links
    }

    fn get_name(&self) -> &str {
        ""
    }
}

pub fn make_console_device() -> Box<dyn Directory> {
    Box::new(DriverDir {
        files: vec![
            Box::new(ConsoleFile {
                permissions: Permissions::OwnerRead | Permissions::OwnerWrite | Permissions::GroupRead | Permissions::GroupWrite,
                name: "console".to_string(),
            }),
            Box::new(SettingFile::<bool> {
                permissions: Permissions::OwnerRead | Permissions::OwnerWrite | Permissions::GroupRead | Permissions::GroupWrite,
                name: "raw_mode".to_string(),
                get_setting: || get_console().unwrap().get_raw_mode(),
                set_setting: |s| get_console().unwrap().set_raw_mode(s),
            }),
        ],
        directories: vec![],
        links: vec![],
    })
}
