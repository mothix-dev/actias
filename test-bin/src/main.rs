#![no_std]
#![no_main]

use core::arch::asm;

fn write_message(message: &str) {
    let syscall_num = common::Syscalls::Write as u32;
    let fd = 1;
    let buf = message.as_bytes().as_ptr();
    let buf_len = message.as_bytes().len();
    unsafe {
        asm!("int 0x80", in("eax") syscall_num, in("ebx") fd, in("ecx") buf, in("edx") buf_len);
    }
}

#[no_mangle]
pub extern "C" fn _start() {
    unsafe {
        *(0xdffffffc as *mut u32) = 0xe621;
    }

    let syscall_num = common::Syscalls::IsComputerOn as u32;
    let ok: u32;
    let err: u32;
    unsafe {
        asm!("int 0x80", in("eax") syscall_num, lateout("eax") ok, out("ebx") err);
    }

    if err != 0 {
        loop {
            write_message("error!");
        }
    }

    if ok == 1 {
        write_message("computer is on!");
    }

    let uwu = unsafe { *(0xdffffffd as *mut u8) };
    if uwu != 0xe6 {
        write_message(":(");
    }

    let syscall_num = common::Syscalls::Exit as u32;
    unsafe {
        asm!("int 0x80", in("eax") syscall_num);
    }

    write_message("not supposed to be here");

    #[allow(clippy::empty_loop)]
    loop {}
}

#[panic_handler]
pub fn panic_implementation(info: &core::panic::PanicInfo) -> ! {
    write_message("panic!");

    let syscall_num = common::Syscalls::Exit as u32;
    unsafe {
        asm!("int 0x80", in("eax") syscall_num);
    }

    #[allow(clippy::empty_loop)]
    loop {}
}
