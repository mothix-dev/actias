/*
 * Rust BareBones OS
 * - By John Hodge (Mutabah/thePowersGang) 
 *
 * logging.rs
 * - Debug output using rust's core::fmt system
 *
 * This code has been put into the public domain, there are no restrictions on
 * its use, and the author takes no liability.
 */
use core::sync::atomic;
use core::fmt;
use crate::console::get_console;

/// A primitive lock for the logging output
///
/// This is not really a lock. Since there is no threading at the moment, all
/// it does is prevent writing when a collision would occur.
static LOGGING_LOCK: atomic::AtomicBool = atomic::AtomicBool::new(false);

/// A formatter object for debug output
pub struct DebugWriter(bool);

impl DebugWriter {
    /// Obtain a logger for the specified module
    pub fn get(module: &str) -> Self {
        // This "acquires" the lock (actually just disables output if paralel writes are attempted
        let mut ret = Self(! LOGGING_LOCK.swap(true, atomic::Ordering::Acquire));
        
        // Print the module name before returning (prefixes all messages)
        {
            use core::fmt::Write;
            let _ = write!(&mut ret, "[{}] ", module);
        }
        
        ret
    }
}

impl core::ops::Drop for DebugWriter {
    fn drop(&mut self) {
        // Write a terminating newline before releasing the lock
        {
            use core::fmt::Write;
            let _ = write!(self, "\r\n");
        }
        // On drop, "release" the lock
        if self.0 {
            LOGGING_LOCK.store(false, atomic::Ordering::Release);
        }
    }
}

impl fmt::Write for DebugWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // If the lock is owned by this instance, then we can safely write to the output
        if self.0 {
            unsafe {
                crate::platform::debug::puts(s);
            }
        }
        Ok(())
    }
}

/// A formatter object for console output
pub struct ConsoleWriter(bool);

impl ConsoleWriter {
    /// Obtain a logger for the specified module
    pub fn get(module: &str) -> Self {
        // This "acquires" the lock (actually just disables output if paralel writes are attempted
        let mut ret = Self(! LOGGING_LOCK.swap(true, atomic::Ordering::Acquire));
        
        // Print the module name before returning (prefixes all messages)
        {
            use core::fmt::Write;
            let _ = write!(&mut ret, "[{}] ", module);
        }
        
        ret
    }
}

impl core::ops::Drop for ConsoleWriter {
    fn drop(&mut self) {
        // Write a terminating newline before releasing the lock
        {
            use core::fmt::Write;
            let _ = write!(self, "\r\n");
        }
        // On drop, "release" the lock
        if self.0 {
            LOGGING_LOCK.store(false, atomic::Ordering::Release);
        }
    }
}

impl fmt::Write for ConsoleWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // If the lock is owned by this instance, then we can safely write to the output
        if self.0 {
            if let Some(console) = get_console() {
                console.puts(s);
            }
        }
        Ok(())
    }
}
