#![no_std]
#![no_main]
#![feature(let_chains)]

use common::{Errno, EventKind, EventResponse, FileKind, FileMode, FileStat, FilesystemEvent, OpenFlags, Permissions, ResponseData, Result, Syscalls};
use core::{arch::asm, mem::size_of};

#[inline]
#[cfg(target_arch = "x86")]
unsafe fn syscall_0_args(num: Syscalls) -> Result<u32> {
    let res_ok: u32;
    let res_err: u32;
    let num = num as u32;

    asm!(
        "int 0x80",
        inlateout("eax") num => res_ok,
        out("ebx") res_err,
    );

    if res_err == 0 {
        Ok(res_ok)
    } else {
        Err(Errno::from(res_err))
    }
}

#[inline]
#[cfg(target_arch = "x86")]
unsafe fn syscall_1_args(num: Syscalls, arg0: u32) -> Result<u32> {
    let res_ok: u32;
    let res_err: u32;
    let num = num as u32;

    asm!(
        "int 0x80",
        inlateout("eax") num => res_ok,
        inlateout("ebx") arg0 => res_err,
    );

    if res_err == 0 {
        Ok(res_ok)
    } else {
        Err(Errno::from(res_err))
    }
}

#[inline]
#[cfg(target_arch = "x86")]
unsafe fn syscall_2_args(num: Syscalls, arg0: u32, arg1: u32) -> Result<u32> {
    let res_ok: u32;
    let res_err: u32;
    let num = num as u32;

    asm!(
        "int 0x80",
        inlateout("eax") num => res_ok,
        inlateout("ebx") arg0 => res_err,
        in("ecx") arg1,
    );

    if res_err == 0 {
        Ok(res_ok)
    } else {
        Err(Errno::from(res_err))
    }
}

#[inline]
#[cfg(target_arch = "x86")]
unsafe fn syscall_3_args(num: Syscalls, arg0: u32, arg1: u32, arg2: u32) -> Result<u32> {
    let res_ok: u32;
    let res_err: u32;
    let num = num as u32;

    asm!(
        "int 0x80",
        inlateout("eax") num => res_ok,
        inlateout("ebx") arg0 => res_err,
        in("ecx") arg1,
        in("edx") arg2,
    );

    if res_err == 0 {
        Ok(res_ok)
    } else {
        Err(Errno::from(res_err))
    }
}

#[inline]
#[cfg(target_arch = "x86")]
unsafe fn syscall_4_args(num: Syscalls, arg0: u32, arg1: u32, arg2: u32, arg3: u32) -> Result<u32> {
    let res_ok: u32;
    let res_err: u32;
    let num = num as u32;

    asm!(
        "int 0x80",
        inlateout("eax") num => res_ok,
        inlateout("ebx") arg0 => res_err,
        in("ecx") arg1,
        in("edx") arg2,
        in("edi") arg3,
    );

    if res_err == 0 {
        Ok(res_ok)
    } else {
        Err(Errno::from(res_err))
    }
}

fn close(fd: usize) -> Result<()> {
    unsafe { syscall_1_args(common::Syscalls::Close, fd.try_into().unwrap()).map(|_| ()) }
}

fn read(fd: usize, slice: &mut [u8]) -> Result<usize> {
    // dirty hack to map the slice in before async page faults are functional
    unsafe {
        core::ptr::read_volatile(&slice[0]);
        core::ptr::read_volatile(&slice[slice.len() - 1]);
    }
    unsafe { syscall_3_args(common::Syscalls::Read, fd.try_into().unwrap(), slice.as_mut_ptr() as u32, slice.len() as u32).map(|bytes| bytes.try_into().unwrap()) }
}

fn write(fd: usize, slice: &[u8]) -> Result<usize> {
    unsafe {
        core::ptr::read_volatile(&slice[0]);
        core::ptr::read_volatile(&slice[slice.len() - 1]);
    }
    unsafe { syscall_3_args(common::Syscalls::Write, fd.try_into().unwrap(), slice.as_ptr() as u32, slice.len() as u32).map(|bytes| bytes.try_into().unwrap()) }
}

fn seek(fd: usize, offset: isize, kind: common::SeekKind) -> Result<isize> {
    unsafe { syscall_3_args(common::Syscalls::Seek, fd.try_into().unwrap(), (offset as i32) as u32, kind as u32).map(|val| (val as i32) as isize) }
}

fn write_message(message: &str) {
    write(1, message.as_bytes()).unwrap();
}

fn open(at: usize, path: &str, flags: OpenFlags) -> Result<usize> {
    unsafe {
        core::ptr::read_volatile(&path.as_bytes()[0]);
        core::ptr::read_volatile(&path.as_bytes()[path.len() - 1]);
    }
    unsafe {
        syscall_4_args(
            common::Syscalls::Open,
            at.try_into().unwrap(),
            path.as_bytes().as_ptr() as u32,
            path.as_bytes().len() as u32,
            flags.into(),
        )
        .map(|fd| fd as usize)
    }
}

fn fork() -> Result<usize> {
    unsafe { syscall_0_args(Syscalls::Fork).map(|pid| pid.try_into().unwrap()) }
}

#[no_mangle]
pub extern "C" fn _start() {
    /*unsafe {
        *(0xdfffcffc as *mut u32) = 0xe621;
    }

    if unsafe { syscall_0_args(Syscalls::IsComputerOn).unwrap() } == 1 {
        write_message("computer is on!");
    }

    let uwu = unsafe { *(0xdfffcffd as *mut u8) };
    if uwu != 0xe6 {
        write_message(":(");
    }

    let child_pid = unsafe { syscall_0_args(Syscalls::Fork).unwrap() };

    if child_pid == 0 {
        write_message("child process");

        unsafe {
            *(0xdfffcffc as *mut u32) = 0xe926;
        }
    } else {
        write_message("parent process");

        for _i in 0..1048576 {
            unsafe {
                asm!("pause");
            }
        }

        let uwu = unsafe { *(0xdfffcffd as *mut u8) };
        if uwu == 0xe9 {
            write_message(":(");
        } else if uwu == 0xe6 {
            write_message(":)");
        }
    }*/

    write_message(":3c");

    let fd = open(0, "/../sysfs/mem", OpenFlags::ReadWrite | OpenFlags::AtCWD).unwrap();
    seek(fd, 0xb8000, common::SeekKind::Set).unwrap();
    write(fd, &[0x55, 0x0f, 0x77, 0x0f, 0x55, 0x0f]).unwrap();
    close(fd).unwrap();

    /*let proc_dir = open(0, "/../procfs/self", OpenFlags::Read | OpenFlags::AtCWD | OpenFlags::Directory).unwrap();

    let fd = open(proc_dir, "files", OpenFlags::Read | OpenFlags::Directory).unwrap();
    let mut buf = [0; 256];
    loop {
        let bytes_read = read(fd, &mut buf).unwrap();
        if bytes_read == 0 {
            break;
        }
        //write(1, &buf[..bytes_read]).unwrap();
        let str = core::str::from_utf8(&buf[4..bytes_read - 1]).unwrap();

        write_message(str);

        let fd = open(fd, str, OpenFlags::Read | OpenFlags::SymLink | OpenFlags::NoFollow).unwrap();

        let bytes_read = read(fd, &mut buf).unwrap();
        if bytes_read == 0 {
            break;
        }
        let str = core::str::from_utf8(&buf[..bytes_read]).unwrap();

        write_message(str);

        close(fd).unwrap();
    }
    close(fd).unwrap();

    write_message("cwd:");
    let fd = open(proc_dir, "cwd", OpenFlags::Read | OpenFlags::SymLink | OpenFlags::NoFollow).unwrap();
    let mut buf = [0; 256];
    let bytes_read = read(fd, &mut buf).unwrap();
    if bytes_read != 0 {
        let str = core::str::from_utf8(&buf[..bytes_read]).unwrap();
        write_message(str);
    }
    close(fd).unwrap();

    write_message("root:");
    let fd = open(proc_dir, "root", OpenFlags::Read | OpenFlags::SymLink | OpenFlags::NoFollow).unwrap();
    let mut buf = [0; 256];
    let bytes_read = read(fd, &mut buf).unwrap();
    if bytes_read != 0 {
        let str = core::str::from_utf8(&buf[..bytes_read]).unwrap();
        write_message(str);
    }
    close(fd).unwrap();*/

    let fd = open(0, "/../procfs/self/filesystem/name", OpenFlags::Write | OpenFlags::AtCWD).unwrap();
    write(fd, "test".as_bytes()).unwrap();
    close(fd).unwrap();

    let fd = open(0, "/..", OpenFlags::Read | OpenFlags::Directory | OpenFlags::AtCWD).unwrap();
    let mut buf = [0; 256];
    loop {
        let bytes_read = read(fd, &mut buf).unwrap();
        if bytes_read == 0 {
            break;
        }
        let str = core::str::from_utf8(&buf[4..bytes_read - 1]).unwrap();

        write_message(str);
    }
    close(fd).unwrap();

    if fork().unwrap() == 0 {
        // child process
        let fd = open(0, "/../test/uwu", OpenFlags::Read | OpenFlags::AtCWD).unwrap();
        write_message("opened successfully!");

        write(fd, "UwU OwO".as_bytes()).unwrap();
        close(fd).unwrap();
    } else {
        // parent process
        let fd = open(0, "/../procfs/self/filesystem/from_kernel", OpenFlags::Read | OpenFlags::AtCWD).unwrap();
        let fd2 = open(0, "/../procfs/self/filesystem/to_kernel", OpenFlags::Write | OpenFlags::AtCWD).unwrap();

        let mut buf = [0; 1024];
        loop {
            read(fd, &mut buf).unwrap();
            write_message("got event!");

            let event = unsafe { &*(buf.as_ptr() as *const _ as *const FilesystemEvent) };
            let event_size = size_of::<FilesystemEvent>();

            fn write_response(fd2: usize, response: EventResponse) {
                write(fd2, unsafe { core::slice::from_raw_parts(&response as *const _ as *const u8, core::mem::size_of::<EventResponse>()) }).unwrap();
            }
            fn write_stat(fd2: usize, stat: FileStat) {
                write(fd2, unsafe { core::slice::from_raw_parts(&stat as *const _ as *const u8, size_of::<FileStat>()) }).unwrap();
            }

            match event.kind {
                EventKind::Close => (),
                EventKind::Open { name_length, .. } => {
                    let name = core::str::from_utf8(&buf[event_size..event_size + name_length]);

                    let data = if let Ok(name) = name && name == "uwu" && event.handle == 0 {
                        ResponseData::Handle { handle: 1 }
                    } else {
                        ResponseData::Error { error: Errno::NoSuchFileOrDir }
                    };
                    write_response(fd2, EventResponse { id: event.id, data });
                }
                EventKind::Stat => {
                    if event.handle == 0 {
                        write_response(fd2, EventResponse {
                            id: event.id,
                            data: ResponseData::None,
                        });
                        write_stat(fd2, FileStat {
                            mode: FileMode {
                                permissions: Permissions::OwnerRead
                                    | Permissions::OwnerExecute
                                    | Permissions::GroupRead
                                    | Permissions::GroupExecute
                                    | Permissions::OtherRead
                                    | Permissions::OtherExecute,
                                kind: FileKind::Directory,
                            },
                            ..Default::default()
                        });
                    } else if event.handle == 1 {
                        write_response(fd2, EventResponse {
                            id: event.id,
                            data: ResponseData::None,
                        });
                        write_stat(fd2, FileStat {
                            mode: FileMode {
                                permissions: Permissions::OwnerWrite | Permissions::GroupWrite | Permissions::OtherWrite,
                                kind: FileKind::CharSpecial,
                            },
                            ..Default::default()
                        });
                    } else {
                        write_response(fd2, EventResponse {
                            id: event.id,
                            data: ResponseData::Error { error: Errno::TryAgain },
                        });
                    }
                }
                EventKind::Write { .. } => {
                    if event.handle == 1 {
                        write_response(fd2, EventResponse {
                            id: event.id,
                            data: ResponseData::None,
                        });
                        let bytes_read = read(fd2, &mut buf).unwrap();
                        write(1, &buf[..bytes_read]).unwrap();
                    } else {
                        write_response(fd2, EventResponse {
                            id: event.id,
                            data: ResponseData::Error { error: Errno::TryAgain },
                        });
                    }
                }
                _ => write_response(fd2, EventResponse {
                    id: event.id,
                    data: ResponseData::Error { error: Errno::NotSupported },
                }),
            }
        }
    }

    unsafe {
        syscall_0_args(Syscalls::Exit).unwrap();
    }

    write_message("not supposed to be here");

    #[allow(clippy::empty_loop)]
    loop {}
}

#[panic_handler]
pub fn panic_implementation(_info: &core::panic::PanicInfo) -> ! {
    let _ = write(2, "panic!".as_bytes());

    unsafe {
        let _ = syscall_0_args(Syscalls::Exit);
    }

    #[allow(clippy::empty_loop)]
    loop {}
}
