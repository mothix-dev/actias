//! procfs filesystem

use super::kernel::FileDescriptor;
use crate::{
    arch::{PhysicalAddress, PROPERTIES},
    mm::ContiguousRegion,
    process::Buffer,
};
use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use common::{Errno, FileKind, FileMode, FileStat, OpenFlags, Permissions, Result};
use log::{log, Level};
use spin::Mutex;

pub struct SysFsRoot;

// https://danielkeep.github.io/tlborm/book/blk-counting.html
macro_rules! count {
    () => (0usize);
    ( $x:tt $($xs:tt)* ) => (1usize + count!($($xs)*));
}

macro_rules! make_sysfs {
    ( $($name:tt => $type:ident),+ $(,)? ) => {
        const SYS_FS_FILES: [&'static str; count!($($name)*)] = [$($name ,)*];

        impl FileDescriptor for SysFsRoot {
            fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn FileDescriptor>> {
                if flags & OpenFlags::Create != OpenFlags::None {
                    return Err(Errno::ReadOnlyFilesystem);
                }

                match name.as_str() {
                    $($name => Ok(Arc::new($type::new())),)*
                    _ => Err(Errno::NoSuchFileOrDir),
                }
            }


            fn read(&self, position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
                let position: usize = match position.try_into() {
                    Ok(position) => position,
                    Err(_) => return callback(Err(Errno::ValueOverflow), false),
                };

                let mut data = Vec::new();
                if position < SYS_FS_FILES.len() {
                    let entry = SYS_FS_FILES[position];
                    data.extend_from_slice(&(0_u32.to_ne_bytes()));
                    data.extend_from_slice(entry.as_bytes());
                    data.push(0);
                }

                let res = buffer.copy_from(&data);
                callback(res, false);
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

impl FileDescriptor for LogDir {
    fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn FileDescriptor>> {
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

    fn read(&self, position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        let position: usize = match position.try_into() {
            Ok(position) => position,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };

        let mut data = Vec::new();
        if position < 5 {
            let entry = LOG_LEVELS[position];
            data.extend_from_slice(&(0_u32.to_ne_bytes()));
            data.extend_from_slice(entry.as_bytes());
            data.push(0);
        }

        let res = buffer.copy_from(&data);
        callback(res, false);
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

impl FileDescriptor for Logger {
    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerWrite | Permissions::GroupWrite | Permissions::OtherWrite,
                kind: FileKind::CharSpecial,
            },
            ..Default::default()
        })
    }

    fn write(&self, _position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        let res = buffer
            .map_in(|slice| {
                log!(target: "sysfs/log", self.level, "{}", core::str::from_utf8(slice).map_err(|_| Errno::InvalidArgument)?);
                Ok(slice.len())
            })
            .map_err(Errno::from)
            .and_then(|err| err);
        callback(res, false);
    }
}

/// allows processes to access physical memory
struct MemFile;

impl MemFile {
    fn new() -> Self {
        Self
    }
}

impl FileDescriptor for MemFile {
    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::OwnerWrite | Permissions::GroupRead | Permissions::GroupWrite,
                kind: FileKind::Regular,
            },
            ..Default::default()
        })
    }

    fn read(&self, position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        let mut page_directory = crate::mm::LockedPageDir(crate::get_global_state().page_directory.clone());
        let addr: PhysicalAddress = match position.try_into() {
            Ok(addr) => addr,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };
        let length_phys: PhysicalAddress = match buffer.len().try_into() {
            Ok(length) => length,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };
        let region = ContiguousRegion::new(addr, length_phys);
        let aligned_region = region.align_covering(PROPERTIES.page_size.try_into().unwrap());
        let addrs = (aligned_region.base..=(aligned_region.base + aligned_region.length)).step_by(PROPERTIES.page_size).collect::<Vec<_>>();
        let offset = (addr - aligned_region.base).try_into().unwrap();
        let callback = Arc::new(Mutex::new(Some(callback)));

        let res = unsafe {
            crate::mm::map_memory(&mut page_directory, &addrs, |slice| buffer.copy_from(&slice[offset..offset + slice.len()]))
                .map_err(Errno::from)
                .and_then(|res| res)
        };
        (callback.lock().take().unwrap())(res, false);
    }

    fn write(&self, position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        let mut page_directory = crate::mm::LockedPageDir(crate::get_global_state().page_directory.clone());
        let addr: PhysicalAddress = match position.try_into() {
            Ok(addr) => addr,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };
        let length_phys: PhysicalAddress = match buffer.len().try_into() {
            Ok(length) => length,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };
        let region = ContiguousRegion::new(addr, length_phys);
        let aligned_region = region.align_covering(PROPERTIES.page_size.try_into().unwrap());
        let addrs = (aligned_region.base..=(aligned_region.base + aligned_region.length)).step_by(PROPERTIES.page_size).collect::<Vec<_>>();
        let offset = (addr - aligned_region.base).try_into().unwrap();
        let callback = Arc::new(Mutex::new(Some(callback)));

        let res = unsafe {
            crate::mm::map_memory(&mut page_directory, &addrs, |slice| {
                let length = slice.len();
                buffer.copy_into(&mut slice[offset..offset + length])
            })
            .map_err(Errno::from)
            .and_then(|res| res)
        };
        (callback.lock().take().unwrap())(res, false);
    }

    fn get_page(&self, position: i64, callback: Box<dyn FnOnce(Option<crate::arch::PhysicalAddress>, bool)>) {
        // TODO: restrict this to only reserved areas in the memory map
        callback(position.try_into().ok(), false);
    }
}
