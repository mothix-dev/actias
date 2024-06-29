use core::fmt::Write;
use log::{LevelFilter, Log, Metadata, Record};
use spin::Mutex;

/// simple logger implementation
pub struct Logger<T: Send + Write> {
    pub max_level: LevelFilter,
    pub writer: Mutex<T>,
}

impl<T: Send + Write> Logger<T> {
    pub const fn new(max_level: LevelFilter, writer: T) -> Self {
        Self {
            max_level,
            writer: Mutex::new(writer),
        }
    }
}

impl<T: Send + Write> Log for Logger<T> {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.max_level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let level = record.level();
        let width = 5;
        let target = record.target();
        let args = record.args();

        let Some(mut writer) = self.writer.try_lock() else {
            // if the lock can't be acquired, just return- it's probably fine
            return;
        };

        let _ = write!(&mut writer, "{level:width$} ");
        if let Some(path) = record.module_path() {
            if target != path {
                let _ = write!(&mut writer, "({target}) ");
            }
            let _ = write!(&mut writer, "[{path}] ");
        } else {
            let _ = write!(&mut writer, "[?] ({target}) ");
        }
        let _ = writeln!(&mut writer, "{args}");
    }

    fn flush(&self) {}
}
