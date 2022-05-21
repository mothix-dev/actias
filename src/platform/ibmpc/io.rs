use core::arch::asm;

/// Write a byte to the specified port
#[inline(always)]
pub unsafe fn outb(port: u16, val: u8) {
    asm!("out dx, al", in("al") val, in("dx") port, options(preserves_flags, nomem, nostack));
}

/// Read a single byte from the specified port
#[inline(always)]
pub unsafe fn inb(port: u16) -> u8 {
    let ret : u8;
    asm!("in al, dx", out("al") ret, in("dx") port, options(preserves_flags, nomem, nostack));
    ret
}

/// Write a word (16-bits) to the specified port
#[inline(always)]
pub unsafe fn outw(port: u16, val: u16) {
    asm!("out dx, ax", in("ax") val, in("dx") port, options(preserves_flags, nomem, nostack));
}

/// Read a word (16-bits) from the specified port
#[inline(always)]
pub unsafe fn inw(port: u16) -> u16 {
    let ret : u16;
    asm!("in ax, dx", out("ax") ret, in("dx") port, options(preserves_flags, nomem, nostack));
    ret
}

/// Write a long/double-word (32-bits) to the specified port
#[inline(always)]
pub unsafe fn outl(port: u16, val: u32) {
    asm!("out dx, eax", in("eax") val, in("dx") port, options(preserves_flags, nomem, nostack));
}

/// Read a long/double-word (32-bits) from the specified port
#[inline(always)]
pub unsafe fn inl(port: u16) -> u32 {
    let ret : u32;
    asm!("in eax, dx", out("eax") ret, in("dx") port, options(preserves_flags, nomem, nostack));
    ret
}
