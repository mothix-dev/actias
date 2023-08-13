use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use common::{Errno, FileKind, FileMode, FileStat, OpenFlags, Permissions, Result};
use log::{error, log, Level};
use spin::Mutex;

use crate::{
    arch::{PhysicalAddress, PROPERTIES},
    mm::ContiguousRegion,
};

pub struct SysFsRoot;

// https://danielkeep.github.io/tlborm/book/blk-counting.html
macro_rules! count {
    () => (0usize);
    ( $x:tt $($xs:tt)* ) => (1usize + count!($($xs)*));
}

macro_rules! make_sysfs {
    ( $($name:tt => $type:ident),+ $(,)? ) => {
        const SYS_FS_FILES: [&'static str; count!($($name)*)] = [$($name ,)*];

        impl super::FileDescriptor for SysFsRoot {
            fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn super::FileDescriptor>> {
                if flags & OpenFlags::Create != OpenFlags::None {
                    return Err(Errno::ReadOnlyFilesystem);
                }

                match name.as_str() {
                    $($name => Ok(Arc::new($type::new())),)*
                    _ => Err(Errno::NoSuchFileOrDir),
                }
            }


            fn read(&self, position: i64, _length: usize, callback: Box<dyn for<'a> super::RequestCallback<&'a [u8]>>) {
                let position: usize = match position.try_into() {
                    Ok(position) => position,
                    Err(_) => return callback(Err(Errno::ValueOverflow), false),
                };

                if position >= SYS_FS_FILES.len() {
                    callback(Ok(&[]), false);
                } else {
                    let entry = SYS_FS_FILES[position];
                    let mut data = Vec::new();
                    data.extend_from_slice(&(0_u32.to_ne_bytes()));
                    data.extend_from_slice(entry.as_bytes());
                    data.push(0);

                    callback(Ok(&data), false);
                }
            }

            fn stat(&self) -> Result<FileStat> {
                Ok(FileStat {
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
                })
            }
        }
    };
}

make_sysfs![
    "log" => LogDir,
    "mem" => MemFile,
];

/// directory containing files for each log level, to allow programs to easily write to the kernel log if there's no other output method available
struct LogDir;

impl LogDir {
    fn new() -> Self {
        Self
    }
}

const LOG_LEVELS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

impl super::FileDescriptor for LogDir {
    fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn super::FileDescriptor>> {
        if flags & OpenFlags::Create != OpenFlags::None {
            return Err(Errno::ReadOnlyFilesystem);
        }

        match name.as_str() {
            "error" => Ok(Arc::new(Logger::new(Level::Error))),
            "warn" => Ok(Arc::new(Logger::new(Level::Warn))),
            "info" => Ok(Arc::new(Logger::new(Level::Info))),
            "debug" => Ok(Arc::new(Logger::new(Level::Debug))),
            "trace" => Ok(Arc::new(Logger::new(Level::Trace))),
            _ => Err(Errno::NoSuchFileOrDir),
        }
    }

    fn read(&self, position: i64, _length: usize, callback: Box<dyn for<'a> super::RequestCallback<&'a [u8]>>) {
        let position: usize = match position.try_into() {
            Ok(position) => position,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };

        if position >= 5 {
            callback(Ok(&[]), false);
        } else {
            let entry = LOG_LEVELS[position];
            let mut data = Vec::new();
            data.extend_from_slice(&(0_u32.to_ne_bytes()));
            data.extend_from_slice(entry.as_bytes());
            data.push(0);

            callback(Ok(&data), false);
        }
    }

    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::OwnerExecute | Permissions::GroupRead | Permissions::GroupExecute | Permissions::OtherRead | Permissions::OtherExecute,
                kind: FileKind::Directory,
            },
            ..Default::default()
        })
    }
}

/// simple way to allow programs to print debug info to the kernel log
struct Logger {
    level: Level,
}

impl Logger {
    fn new(level: Level) -> Self {
        Self { level }
    }
}

impl super::FileDescriptor for Logger {
    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerWrite | Permissions::GroupWrite | Permissions::OtherWrite,
                kind: FileKind::CharSpecial,
            },
            ..Default::default()
        })
    }

    fn write(&self, _position: i64, length: usize, callback: Box<dyn for<'a> super::RequestCallback<&'a mut [u8]>>) {
        let mut buf = Vec::new();
        if buf.try_reserve_exact(length).is_err() {
            callback(Err(Errno::OutOfMemory), false);
            return;
        }
        for _i in 0..length {
            buf.push(0);
        }

        callback(Ok(&mut buf), false);

        match core::str::from_utf8(&buf) {
            Ok(str) => log!(target: "sysfs/log", self.level, "{str}"),
            Err(err) => error!("couldn't parse string for log level {}: {err}", self.level),
        }
    }
}

/// allows processes to access physical memory
struct MemFile;

impl MemFile {
    fn new() -> Self {
        Self
    }
}

impl super::FileDescriptor for MemFile {
    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::OwnerWrite | Permissions::GroupRead | Permissions::GroupWrite | Permissions::OtherRead | Permissions::OtherWrite,
                kind: FileKind::Regular,
            },
            ..Default::default()
        })
    }

    fn read(&self, position: i64, length: usize, callback: Box<dyn for<'a> super::RequestCallback<&'a [u8]>>) {
        let mut page_directory = crate::mm::LockedPageDir(crate::get_global_state().page_directory.clone());
        let addr: PhysicalAddress = match position.try_into() {
            Ok(addr) => addr,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };
        let length_phys: PhysicalAddress = match length.try_into() {
            Ok(length) => length,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };
        let region = ContiguousRegion::new(addr, length_phys);
        let aligned_region = region.align_covering(PROPERTIES.page_size.try_into().unwrap());
        let addrs = (aligned_region.base..=(aligned_region.base + aligned_region.length)).step_by(PROPERTIES.page_size).collect::<Vec<_>>();
        let offset = (addr - aligned_region.base).try_into().unwrap();
        let callback = Arc::new(Mutex::new(Some(callback)));

        if let Err(err) = unsafe {
            crate::mm::map_memory(&mut page_directory, &addrs, |slice| {
                (callback.lock().take().unwrap())(Ok(&slice[offset..offset + length]), false);
            })
        } {
            (callback.lock().take().unwrap())(Err(Errno::from(err)), false);
        }
    }

    fn write(&self, position: i64, length: usize, callback: Box<dyn for<'a> super::RequestCallback<&'a mut [u8]>>) {
        let mut page_directory = crate::mm::LockedPageDir(crate::get_global_state().page_directory.clone());
        let addr: PhysicalAddress = match position.try_into() {
            Ok(addr) => addr,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };
        let length_phys: PhysicalAddress = match length.try_into() {
            Ok(length) => length,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };
        let region = ContiguousRegion::new(addr, length_phys);
        let aligned_region = region.align_covering(PROPERTIES.page_size.try_into().unwrap());
        let addrs = (aligned_region.base..=(aligned_region.base + aligned_region.length)).step_by(PROPERTIES.page_size).collect::<Vec<_>>();
        let offset = (addr - aligned_region.base).try_into().unwrap();
        let callback = Arc::new(Mutex::new(Some(callback)));

        if let Err(err) = unsafe {
            crate::mm::map_memory(&mut page_directory, &addrs, |slice| {
                (callback.lock().take().unwrap())(Ok(&mut slice[offset..offset + length]), false);
            })
        } {
            (callback.lock().take().unwrap())(Err(Errno::from(err)), false);
        }
    }

    fn get_page(&self, position: i64, callback: Box<dyn FnOnce(Option<crate::arch::PhysicalAddress>, bool)>) {
        // TODO: restrict this to only reserved areas in the memory map
        callback(position.try_into().ok(), false);
    }
}
