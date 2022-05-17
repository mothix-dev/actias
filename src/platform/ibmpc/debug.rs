/*
 * Rust BareBones OS
 * - By John Hodge (Mutabah/thePowersGang) 
 *
 * arch/x86/debug.rs
 * - Debug output channel
 *
 * Writes debug to the standard PC serial port (0x3F8 .. 0x3FF)
 * 
 * == LICENCE ==
 * This code has been put into the public domain, there are no restrictions on
 * its use, and the author takes no liability.
 */

use super::io::{inb, outb};

/// Write a string to the output channel
///
/// This method is unsafe because it does port accesses without synchronisation
pub unsafe fn puts(s: &str) {
    for b in s.bytes() {
        putb(b);
    }
}

/// Write a single byte to the output channel
///
/// This method is unsafe because it does port accesses without synchronisation
pub unsafe fn putb(b: u8) {
    // Wait for the serial port's fifo to not be empty
    while (inb(0x3F8 + 5) & 0x20) == 0 {
        // Do nothing
    }
    // Send the byte out the serial port
    outb(0x3F8, b);
    
    // Also send to the bochs 0xe9 hack
    outb(0xe9, b);
}

/// exit code for qemu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

/// exit qemu with code, useful for unit testing
pub fn exit_qemu(exit_code: QemuExitCode) {
    unsafe {
        outb(0xf4, exit_code as u8)
    }
}

pub fn exit_failure() {
    exit_qemu(QemuExitCode::Failed);
}

pub fn exit_success() {
    exit_qemu(QemuExitCode::Success);
}
