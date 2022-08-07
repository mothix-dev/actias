use core::{fmt, fmt::Write};
use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};

struct Logger;

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            if let Some(path) = record.module_path() {
                write!(self, "[{} - {}] {}", record.level(), path, record.args());
            } else {
                write!(self, "[{}] {}", record.level(), record.args());
            }
        }
    }

    fn flush(&self) {}
}

impl fmt::Write for Logger {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        unsafe {
            crate::platform::debug::puts(s);
        }
        Ok(())
    }
}

static LOGGER: Logger = Logger;

pub fn init() -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER).map(|()| log::set_max_level(LevelFilter::Trace))
}
