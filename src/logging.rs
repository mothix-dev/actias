use core::fmt;
//use crate::console::get_console;

/// A formatter object for debug output
pub struct DebugWriter;

impl DebugWriter {
    /// Obtain a logger for the specified module
    pub fn get(module: &str) -> Self {
        let mut ret = Self;
        
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
    }
}

impl fmt::Write for DebugWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // im So Fucking Tired of these goddamn Locks
        unsafe {
            crate::platform::debug::puts(s);
        }
        Ok(())
    }
}

/*
/// A formatter object for console output
pub struct ConsoleWriter;

impl ConsoleWriter {
    /// Obtain a logger for the specified module
    pub fn get(module: &str) -> Self {
        // This "acquires" the lock (actually just disables output if paralel writes are attempted
        let mut ret = Self;
        
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
    }
}

impl fmt::Write for ConsoleWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // If the lock is owned by this instance, then we can safely write to the output
        if let Some(console) = get_console() {
            console.puts(s);
        }
        Ok(())
    }
}
*/
