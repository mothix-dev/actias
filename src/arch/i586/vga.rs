// x86 vga text mode

use crate::console::{ColorCode, Color, RawTextConsole};

const BUFFER_WIDTH: usize = 80;
const BUFFER_HEIGHT: usize = 25;

#[repr(transparent)]
pub struct Buffer {
    chars: [[u16; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

pub struct VGAConsole {
    buffer: &'static mut Buffer,
}

impl VGAConsole {

}

impl RawTextConsole for VGAConsole {
    fn write_char(&mut self, x: u16, y: u16, color: ColorCode, c: u8) {
        self.buffer.chars[y as usize][x as usize] = (((color.background as u16) & 0xf) << 12) | (((color.foreground as u16) & 0xf) << 8) | (c as u16);
    }

    fn write_string(&mut self, x: u16, y: u16, color: ColorCode, string: &str) {
        
    }

    fn clear(&mut self) {
        for y in 0..BUFFER_HEIGHT {
            for x in 0..BUFFER_WIDTH {
                self.buffer.chars[y][x] = (b' ') as u16;
            }
        }
    }
}

pub fn create_console() -> VGAConsole {
    VGAConsole {
        buffer: unsafe { &mut *(0xc00b8000 as *mut Buffer) },
    }
}
