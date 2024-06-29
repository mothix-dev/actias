use crate::logger::Logger;
use core::{fmt, fmt::Write};
use log::{LevelFilter, SetLoggerError};
use x86::io::{inb, outb};

/// Write a string to the output channel
///
/// # Safety
///
/// This method is unsafe because it does port accesses without synchronisation
pub unsafe fn serial_puts(s: &str) {
    for b in s.bytes() {
        serial_putb(b);
    }
}

/// Write a single byte to the output channel
///
/// # Safety
///
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
struct SerialWriter;

impl Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        unsafe {
            serial_puts(s);
        }
        Ok(())
    }
}

/// our logger that we will log things with
static LOGGER: Logger<SerialWriter> = Logger::new(LevelFilter::Info, SerialWriter);

/// initialize the logger, setting the max level in the process
pub fn init() -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER).map(|_| log::set_max_level(LOGGER.max_level))
}
