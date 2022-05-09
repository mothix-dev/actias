// x86 vga text mode

use crate::console::{ColorCode, RawTextConsole};
use core::cmp::Ordering;

const BUFFER_WIDTH: usize = 80;
const BUFFER_HEIGHT: usize = 25;

/// describes the layout of video ram, makes casting a pointer to it easier
#[repr(transparent)]
pub struct Buffer {
    chars: [[u16; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

/// raw VGA text console
pub struct VGAConsole {
    buffer: &'static mut Buffer,
}

impl RawTextConsole for VGAConsole {
    fn write_char(&mut self, x: u16, y: u16, color: ColorCode, c: u8) {
        self.buffer.chars[y as usize][x as usize] = (((color.background as u16) & 0xf) << 12) | (((color.foreground as u16) & 0xf) << 8) | (c as u16);
    }

    fn clear(&mut self, x0: u16, y0: u16, x1: u16, y1: u16, color: ColorCode) {
        let color2 = (((color.background as u16) & 0xf) << 12) | (((color.foreground as u16) & 0xf) << 8);
        for y in y0..y1 {
            for x in x0..x1 {
                self.buffer.chars[y as usize][x as usize] = color2 | ((b' ') as u16);
            }
        }
    }

    fn copy(&mut self, y0: u16, y1: u16, height: u16) {
        match y0.cmp(&y1) {
            Ordering::Less => { // scroll up
                let diff = y1 - y0;
                for y in (y0 .. y0 + height).rev() {
                    self.buffer.chars[(y + diff) as usize] = self.buffer.chars[y as usize];
                }
            },
            Ordering::Greater => { // scroll down
                let diff = y0 - y1;
                for y in y0 .. y0 + height {
                    self.buffer.chars[(y - diff) as usize] = self.buffer.chars[y as usize];
                }
            },
            _ => (),
        }
    }
}

/// creates a raw console
pub fn create_console() -> VGAConsole {
    VGAConsole {
        buffer: unsafe { &mut *(0xc00b8000 as *mut Buffer) }, // lowest 4 mb are mapped up to 0xc0000000 (3gb), this includes video ram lmao
    }
}
