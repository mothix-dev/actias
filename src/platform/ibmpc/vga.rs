//! x86 vga text mode

use crate::console::{ColorCode, RawTextConsole};
use core::cmp::Ordering;

/// raw VGA text console
pub struct VGAConsole {
    /// video memory represented as a slice
    pub buffer: &'static mut [u16],

    /// the width of the console, in characters
    pub width: usize,

    /// the height of the console
    pub height: usize,
}

impl RawTextConsole for VGAConsole {
    fn write_char(&mut self, x: u16, y: u16, color: ColorCode, c: char) {
        let character =
            if c as u32 > 0x100 {
                b'?'
            } else {
                c as u8
            };
        self.buffer[y as usize * self.width + x as usize] = (((color.background as u16) & 0xf) << 12) | (((color.foreground as u16) & 0xf) << 8) | (character as u16);
    }

    fn clear(&mut self, x0: u16, y0: u16, x1: u16, y1: u16, color: ColorCode) {
        let color2 = (((color.background as u16) & 0xf) << 12) | (((color.foreground as u16) & 0xf) << 8);
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.buffer[y as usize * self.width + x as usize] = color2 | ((b' ') as u16);
            }
        }
    }

    fn copy(&mut self, y0: u16, y1: u16, height: u16) {
        match y0.cmp(&y1) {
            Ordering::Less => { // scroll up
                let diff = y1 - y0;
                for y in (y0 .. y0 + height).rev() {
                    //self.buffer.chars[(y + diff) as usize] = self.buffer.chars[y as usize];

                    for i in 0..self.width {
                        self.buffer[(y + diff) as usize * self.width + i] = self.buffer[y as usize * self.width + i];
                    }
                }
            },
            Ordering::Greater => { // scroll down
                let diff = y0 - y1;
                for y in y0 .. y0 + height {
                    //self.buffer.chars[(y - diff) as usize] = self.buffer.chars[y as usize];

                    for i in 0..self.width {
                        self.buffer[(y - diff) as usize * self.width + i] = self.buffer[y as usize * self.width + i];
                    }
                }
            },
            _ => (),
        }
    }
}
