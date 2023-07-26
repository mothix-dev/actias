use core::{
    fmt,
    fmt::Write,
    //sync::atomic::{AtomicU32, Ordering},
};
use log::{LevelFilter, Log, Metadata, Record, SetLoggerError};

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

/// simple logger implementation over serial
struct Logger {
    max_level: LevelFilter,
    //lock: AtomicU32,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.max_level
    }

    #[allow(unused_must_use)]
    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            /*let apic_id = crate::arch::apic::get_local_apic().map(|apic| apic.id() as u32 + 1).unwrap_or(1);

            // acquire lock if this cpu doesn't have it already
            let has_lock = if self.lock.load(Ordering::Acquire) != apic_id {
                // how the fuck does ordering work
                while self.lock.compare_exchange(0, apic_id, Ordering::SeqCst, Ordering::Acquire).is_err() {
                    crate::arch::spin();
                }
                true
            } else {
                false
            };*/

            let level = record.level();
            let width = 5;
            let args = record.args();

            if let Some(path) = record.module_path() {
                writeln!(&mut SerialWriter, "{level:width$} [{path}] {args}");
            } else {
                writeln!(&mut SerialWriter, "{level:width$} [?] {args}");
            }

            /*if has_lock {
                // release lock
                self.lock.store(0, Ordering::Release);
            }*/
        }
    }

    fn flush(&self) {}
}

/// our logger that we will log things with
static LOGGER: Logger = Logger {
    max_level: LevelFilter::Trace,
    //lock: AtomicU32::new(0),
};

/// initialize the logger, setting the max level in the process
pub fn init() -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER).map(|_| log::set_max_level(LOGGER.max_level))
}
