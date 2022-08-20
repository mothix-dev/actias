use core::{fmt, fmt::Write};
use log::{LevelFilter, Log, Metadata, Record, SetLoggerError};

use x86::io::{inb, outb};

const USE_SERIAL: bool = true;
const USE_VGA_TEXT: bool = true;

/// Write a string to the output channel
///
/// # Safety
/// This method is unsafe because it does port accesses without synchronisation
pub unsafe fn serial_puts(s: &str) {
    for b in s.bytes() {
        serial_putb(b);
    }
}

/// Write a single byte to the output channel
///
/// # Safety
/// This method is unsafe because it does port accesses without synchronisation
pub unsafe fn serial_putb(b: u8) {
    // Wait for the serial port's fifo to not be empty
    while (inb(0x3F8 + 5) & 0x20) == 0 {
        // Do nothing
    }
    // Send the byte out the serial port
    outb(0x3F8, b);

    // Also send to the bochs 0xe9 hack
    outb(0xe9, b);
}

/// wrapper struct to allow us to "safely" write!() to the serial port
///
/// we don't worry about synchronization and locking since that creates more problems than it's worth for a simple debugging interface
struct SerialWriter;

impl Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        unsafe {
            serial_puts(s);
        }
        Ok(())
    }
}

/// text colors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// very simple text console that can be written to for logging purposes
struct VGATextWriter {
    /// x position of the cursor
    cursor_x: u16,

    /// y position of the cursor
    cursor_y: u16,

    /// width of the console
    width: u16,

    /// height of the console
    height: u16,

    /// video memory represented as a slice
    buffer: &'static mut [u16],

    /// color to write text as
    color: ColorCode,
}

impl VGATextWriter {
    fn enable_cursor() {
        unsafe {
            outb(0x3d4, 0x0a);
            outb(0x3d5, (inb(0x3d5) & 0xc0) | 0xd);

            outb(0x3d4, 0x0b);
            outb(0x3d5, (inb(0x3d5) & 0xe0) | 0xe);
        }
    }

    fn newline(&mut self) {
        self.cursor_x = 0;
        self.cursor_y += 1;
        if self.cursor_y >= self.height {
            // scroll screen
            for y in 1..self.height {
                //self.buffer.chars[(y - diff) as usize] = self.buffer.chars[y as usize];

                for i in 0..self.width {
                    self.buffer[((y - 1) * self.width + i) as usize] = self.buffer[(y * self.width + i) as usize];
                }
            }

            // clear new line
            let color2 = (((self.color.background as u16) & 0xf) << 12) | (((self.color.foreground as u16) & 0xf) << 8);
            for x in 0..=(self.width - 1) {
                self.buffer[((self.height - 1) * self.width + x) as usize] = color2 | ((b' ') as u16);
            }

            self.cursor_y = self.height - 1;
        }
    }
}

impl Write for VGATextWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            match c {
                '\n' => {
                    self.newline();
                }
                '\r' => {
                    self.cursor_x = 0;
                }
                '\x08' => {
                    // rust doesn't have \b lmao
                    if self.cursor_x > 0 {
                        self.cursor_x -= 1;
                    }
                }
                '\t' => {
                    self.cursor_x = ((self.cursor_x / 8) * 8) + 8;
                    if self.cursor_x >= self.width {
                        self.newline();
                    }
                }
                _ => {
                    self.buffer[(self.cursor_y * self.width + self.cursor_x) as usize] =
                        (((self.color.background as u16) & 0xf) << 12) | (((self.color.foreground as u16) & 0xf) << 8) | (if c as u32 > 0x100 { b'?' } else { c as u8 } as u16);

                    self.cursor_x += 1;
                    if self.cursor_x >= self.width {
                        self.newline();
                    }
                }
            }
        }

        // update cursor position
        let pos = self.cursor_y * self.width + self.cursor_x;

        unsafe {
            outb(0x3d4, 0x0f);
            outb(0x3d5, (pos & 0xff) as u8);
            outb(0x3d4, 0x0e);
            outb(0x3d5, ((pos >> 8) & 0xff) as u8);
        }

        Ok(())
    }
}

static mut VGA_WRITER: Option<VGATextWriter> = None;

/// simple logger implementation over serial
struct Logger {
    pub max_level: LevelFilter,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.max_level
    }

    #[allow(unused_must_use)]
    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            if USE_SERIAL {
                if let Some(path) = record.module_path() {
                    writeln!(&mut SerialWriter, "{:width$} [{}] {}", record.level(), path, record.args(), width = 5);
                } else {
                    writeln!(&mut SerialWriter, "{:width$} [unknown] {}", record.level(), record.args(), width = 5);
                }
            }
            if USE_VGA_TEXT {
                if let Some(vga_writer) = unsafe { VGA_WRITER.as_mut() } {
                    if let Some(path) = record.module_path() {
                        writeln!(vga_writer, "{:width$} [{}] {}", record.level(), path, record.args(), width = 5);
                    } else {
                        writeln!(vga_writer, "{:width$} [unknown] {}", record.level(), record.args(), width = 5);
                    }
                }
            }
        }
    }

    fn flush(&self) {}
}

/// our logger that we will log things with
static LOGGER: Logger = Logger { max_level: LevelFilter::Info };

/// initialize the logger, setting the max level in the process
pub fn init() -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER).map(|_| log::set_max_level(LOGGER.max_level))
}

pub fn init_vga(buffer: &'static mut [u16], width: u16, height: u16) {
    unsafe {
        VGATextWriter::enable_cursor();
        VGA_WRITER = Some(VGATextWriter {
            cursor_x: 0,
            cursor_y: 0,
            width,
            height,
            buffer,
            color: Default::default(),
        });
    }
}
