#![no_std]
#![no_main]
#![feature(panic_info_message)]

use core::{
    mem::size_of,
    panic::PanicInfo,
};
use interface::{
    syscalls, syscalls::OpenFlags,
    println, eprintln
};

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let (file,line) = match info.location() {
        Some(loc) => (loc.file(), loc.line()),
        None => ("", 0),
    };

    if let Some(m) = info.message() {
        eprintln!("PANIC: file='{}', line={} :: {}", file, line, m);
    } else if let Some(m) = info.payload().downcast_ref::<&str>() {
        eprintln!("PANIC: file='{}', line={} :: {}", file, line, m);
    } else {
        eprintln!("PANIC: file='{}', line={} :: ?", file, line);
    }

    syscalls::exit();
}

#[no_mangle]
pub extern "cdecl" fn _start(argc: usize, argv: *const *const u8, envp: *const *const u8) {
    let stdin = syscalls::open(b"/dev/console/console\0", OpenFlags::Read).unwrap();
    let stdout = syscalls::open(b"/dev/console/console\0", OpenFlags::Write).unwrap();
    let stderr = syscalls::open(b"/dev/console/console\0", OpenFlags::Write).unwrap();

    syscalls::test_log(b"args:\0").unwrap();
    let mut i = 0;
    loop {
        let ptr = unsafe { *((argv as usize + i * size_of::<usize>()) as *const *const u8) };
        if ptr.is_null() {
            break;
        } else {
            syscalls::test_log_ptr(ptr).unwrap();
        }
        i += 1;
    }

    syscalls::test_log(b"env:\0").unwrap();
    let mut i = 0;
    loop {
        let ptr = unsafe { *((envp as usize + i * size_of::<usize>()) as *const *const u8) };
        if ptr.is_null() {
            break;
        } else {
            syscalls::test_log_ptr(ptr).unwrap();
        }
        i += 1;
    }

    //syscalls::test_log_ptr(0xb0000000 as *const _);

    println!("UwU OwO");

    /*loop {
        
    }*/

    let mut buf: [u8; 128] = [0; 128];
    let bytes_read = syscalls::read(&stdin, &mut buf).unwrap().try_into().unwrap();

    println!("read {} bytes: {:?}", bytes_read, core::str::from_utf8(&buf[..bytes_read]));

    panic!("test");

    /*loop {
        syscalls::test_log(b"completely independent process!\0").unwrap();

        for _i in 0..1024 * 1024 * 2 { // slow things down
            unsafe {
                asm!("nop");
            }
        }
    }*/
}
