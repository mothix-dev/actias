//! procfs filesystem

use super::kernel::FileDescriptor;
use crate::{
    arch::{PhysicalAddress, PROPERTIES},
    mm::ContiguousRegion,
    process::Buffer,
};
use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use async_trait::async_trait;
use common::{Errno, FileKind, FileMode, FileStat, OpenFlags, Permissions, Result};
use log::{log, Level};

pub struct SysFsRoot;

// https://danielkeep.github.io/tlborm/book/blk-counting.html
macro_rules! count {
    () => (0usize);
    ( $x:tt $($xs:tt)* ) => (1usize + count!($($xs)*));
}

macro_rules! make_sysfs {
    ( $($name:tt => $type:ident),+ $(,)? ) => {
        const SYS_FS_FILES: [&'static str; count!($($name)*)] = [$($name ,)*];

        #[async_trait]
        impl FileDescriptor for SysFsRoot {
            async fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn FileDescriptor>> {
                if flags & OpenFlags::Create != OpenFlags::None {
                    return Err(Errno::ReadOnlyFilesystem);
                }

                match name.as_str() {
                    $($name => Ok(Arc::new($type::new())),)*
                    _ => Err(Errno::NoSuchFileOrDir),
                }
            }


            async fn read(&self, position: i64, buffer: Buffer) -> Result<usize> {
                let position: usize = position.try_into().map_err(|_| Errno::ValueOverflow)?;

                let mut data = Vec::new();
                if position < SYS_FS_FILES.len() {
                    let entry = SYS_FS_FILES[position];
                    data.extend_from_slice(&(0_u32.to_ne_bytes()));
                    data.extend_from_slice(entry.as_bytes());
                    data.push(0);
                }

                buffer.copy_from(&data).await
            }

            async fn stat(&self) -> Result<FileStat> {
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

#[async_trait]
impl FileDescriptor for LogDir {
    async fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn FileDescriptor>> {
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

    async fn read(&self, position: i64, buffer: Buffer) -> Result<usize> {
        let position: usize = position.try_into().map_err(|_| Errno::ValueOverflow)?;

        let mut data = Vec::new();
        if position < 5 {
            let entry = LOG_LEVELS[position];
            data.extend_from_slice(&(0_u32.to_ne_bytes()));
            data.extend_from_slice(entry.as_bytes());
            data.push(0);
        }

        buffer.copy_from(&data).await
    }

    async fn stat(&self) -> Result<FileStat> {
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

#[async_trait]
impl FileDescriptor for Logger {
    async fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerWrite | Permissions::GroupWrite | Permissions::OtherWrite,
                kind: FileKind::CharSpecial,
            },
            ..Default::default()
        })
    }

    async fn write(&self, _position: i64, buffer: Buffer) -> Result<usize> {
        buffer
            .map_in(|slice| {
                log!(target: "sysfs/log", self.level, "{}", core::str::from_utf8(slice).map_err(|_| Errno::InvalidArgument)?);
                Ok(slice.len())
            })
            .await
            .map_err(Errno::from)
            .and_then(|err| err)
    }
}

/// allows processes to access physical memory
struct MemFile;

impl MemFile {
    fn new() -> Self {
        Self
    }
}

#[async_trait]
impl FileDescriptor for MemFile {
    async fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::OwnerWrite | Permissions::GroupRead | Permissions::GroupWrite,
                kind: FileKind::Regular,
            },
            ..Default::default()
        })
    }

    async fn read(&self, position: i64, buffer: Buffer) -> Result<usize> {
        let mut page_directory = crate::mm::LockedPageDir(crate::get_global_state().page_directory.clone());
        let addr: PhysicalAddress = position.try_into().map_err(|_| Errno::ValueOverflow)?;
        let length_phys: PhysicalAddress = buffer.len().try_into().map_err(|_| Errno::ValueOverflow)?;
        let region = ContiguousRegion::new(addr, length_phys);
        let aligned_region = region.align_covering(PROPERTIES.page_size.try_into().unwrap());
        let addrs = (aligned_region.base..=(aligned_region.base + aligned_region.length)).step_by(PROPERTIES.page_size).collect::<Vec<_>>();
        let offset = (addr - aligned_region.base).try_into().unwrap();

        buffer
            .map_in_mut(|to_write| unsafe {
                crate::mm::map_memory(&mut page_directory, &addrs, |from| {
                    let from = &from[offset..];

                    let bytes_written = to_write.len().min(from.len());
                    to_write[..bytes_written].copy_from_slice(&from[..bytes_written]);
                    bytes_written
                })
                .map_err(Errno::from)
            })
            .await
            .and_then(|res| res)
    }

    async fn write(&self, position: i64, buffer: Buffer) -> Result<usize> {
        let mut page_directory = crate::mm::LockedPageDir(crate::get_global_state().page_directory.clone());
        let addr: PhysicalAddress = position.try_into().map_err(|_| Errno::ValueOverflow)?;
        let length_phys: PhysicalAddress = buffer.len().try_into().map_err(|_| Errno::ValueOverflow)?;
        let region = ContiguousRegion::new(addr, length_phys);
        let aligned_region = region.align_covering(PROPERTIES.page_size.try_into().unwrap());
        let addrs = (aligned_region.base..=(aligned_region.base + aligned_region.length)).step_by(PROPERTIES.page_size).collect::<Vec<_>>();
        let offset = (addr - aligned_region.base).try_into().unwrap();

        buffer
            .map_in(|from| unsafe {
                crate::mm::map_memory(&mut page_directory, &addrs, |to_write| {
                    let to_write = &mut to_write[offset..];

                    let bytes_written = to_write.len().min(from.len());
                    to_write[..bytes_written].copy_from_slice(&from[..bytes_written]);
                    bytes_written
                })
                .map_err(Errno::from)
            })
            .await
            .and_then(|res| res)
    }

    async fn get_page(&self, position: i64) -> Option<PhysicalAddress> {
        // TODO: restrict this to only reserved areas in the memory map
        position.try_into().ok()
    }
}
