use alloc::{boxed::Box, vec::Vec};
use common::{Errno, OpenFlags, Permissions};
use core::sync::atomic::AtomicUsize;
use log::{log, Level};

pub struct SysFs;

impl super::Filesystem for SysFs {
    fn get_root_dir(&self) -> Box<dyn super::FileDescriptor> {
        Box::new(SysFsRoot { seek_pos: AtomicUsize::new(0) })
    }
}

pub struct SysFsRoot {
    seek_pos: AtomicUsize,
}

// https://danielkeep.github.io/tlborm/book/blk-counting.html
macro_rules! count {
    () => (0usize);
    ( $x:tt $($xs:tt)* ) => (1usize + count!($($xs)*));
}

macro_rules! make_sysfs {
    ( $($name:tt => $type:ident),+ $(,)? ) => {
        const SYS_FS_FILES: [&'static str; count!($($name)*)] = [$($name ,)*];

        impl super::FileDescriptor for SysFsRoot {
            fn open(&self, name: &str, flags: OpenFlags) -> common::Result<alloc::boxed::Box<dyn super::FileDescriptor>> {
                if flags & OpenFlags::Create != OpenFlags::None {
                    return Err(Errno::ReadOnlyFilesystem);
                }

                match name {
                    $($name => Ok(Box::new($type::new())),),*
                    _ => Err(Errno::NoSuchFileOrDir),
                }
            }

            fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
                let pos = self.seek_pos.fetch_add(1, core::sync::atomic::Ordering::SeqCst);

                if pos >= SYS_FS_FILES.len() {
                    self.seek_pos.store(SYS_FS_FILES.len(), core::sync::atomic::Ordering::SeqCst);
                    Ok(0)
                } else {
                    let entry = &SYS_FS_FILES[pos];
                    let mut data = Vec::new();
                    data.extend_from_slice(&(0_u32.to_ne_bytes()));
                    data.extend_from_slice(entry.as_bytes());
                    data.push(0);

                    if buf.len() > data.len() {
                        buf[..data.len()].copy_from_slice(&data);
                        Ok(data.len())
                    } else {
                        buf.copy_from_slice(&data[..buf.len()]);
                        Ok(buf.len())
                    }
                }
            }

            fn seek(&self, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
                super::seek_helper(&self.seek_pos, offset, kind, SYS_FS_FILES.len().try_into().map_err(|_| Errno::ValueOverflow)?)
            }

            fn stat(&self) -> common::Result<common::FileStat> {
                Ok(common::FileStat {
                    mode: common::FileMode {
                        permissions: common::Permissions::OwnerRead
                        | common::Permissions::OwnerExecute
                        | common::Permissions::GroupRead
                        | common::Permissions::GroupExecute
                        | common::Permissions::OtherRead
                        | common::Permissions::OtherExecute,
                        kind: common::FileKind::Directory,
                    },
                    ..Default::default()
                })
            }

            fn dup(&self) -> common::Result<Box<dyn super::FileDescriptor>> {
                Ok(Box::new(Self { seek_pos: AtomicUsize::new(self.seek_pos.load(core::sync::atomic::Ordering::SeqCst)) }))
            }
        }
    };
}

make_sysfs![
    "log" => LogFs,
];

/// directory containing files for each log level, to allow programs to easily write to the kernel log if there's no other output method available
struct LogFs {
    seek_pos: AtomicUsize,
}

impl LogFs {
    fn new() -> Self {
        Self {
            seek_pos: AtomicUsize::new(0),
        }
    }
}

const LOG_LEVELS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

impl super::FileDescriptor for LogFs {
    fn open(&self, name: &str, flags: OpenFlags) -> common::Result<alloc::boxed::Box<dyn super::FileDescriptor>> {
        if flags & OpenFlags::Create != OpenFlags::None {
            return Err(Errno::ReadOnlyFilesystem);
        }

        match name {
            "error" => Ok(Box::new(Logger::new(Level::Error))),
            "warn" => Ok(Box::new(Logger::new(Level::Warn))),
            "info" => Ok(Box::new(Logger::new(Level::Info))),
            "debug" => Ok(Box::new(Logger::new(Level::Debug))),
            "trace" => Ok(Box::new(Logger::new(Level::Trace))),
            _ => Err(Errno::NoSuchFileOrDir),
        }
    }

    fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
        let pos = self.seek_pos.fetch_add(1, core::sync::atomic::Ordering::SeqCst);

        if pos >= LOG_LEVELS.len() {
            self.seek_pos.store(LOG_LEVELS.len(), core::sync::atomic::Ordering::SeqCst);
            Ok(0)
        } else {
            let entry = &LOG_LEVELS[pos];
            let mut data = Vec::new();
            data.extend_from_slice(&(0_u32.to_ne_bytes()));
            data.extend_from_slice(entry.as_bytes());
            data.push(0);

            if buf.len() > data.len() {
                buf[..data.len()].copy_from_slice(&data);
                Ok(data.len())
            } else {
                buf.copy_from_slice(&data[..buf.len()]);
                Ok(buf.len())
            }
        }
    }

    fn seek(&self, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
        super::seek_helper(&self.seek_pos, offset, kind, LOG_LEVELS.len().try_into().map_err(|_| Errno::ValueOverflow)?)
    }

    fn stat(&self) -> common::Result<common::FileStat> {
        Ok(common::FileStat {
            mode: common::FileMode {
                permissions: common::Permissions::OwnerRead
                | common::Permissions::OwnerExecute
                | common::Permissions::GroupRead
                | common::Permissions::GroupExecute
                | common::Permissions::OtherRead
                | common::Permissions::OtherExecute,
                kind: common::FileKind::Directory,
            },
            ..Default::default()
        })
    }

    fn dup(&self) -> common::Result<Box<dyn super::FileDescriptor>> {
        Ok(Box::new(Self { seek_pos: AtomicUsize::new(self.seek_pos.load(core::sync::atomic::Ordering::SeqCst)) }))
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
    fn stat(&self) -> common::Result<common::FileStat> {
        Ok(common::FileStat {
            mode: common::FileMode {
                permissions: Permissions::OwnerWrite | Permissions::GroupWrite | Permissions::OtherWrite,
                kind: common::FileKind::CharSpecial,
            },
            ..Default::default()
        })
    }

    fn write(&self, buf: &[u8]) -> common::Result<usize> {
        log!(target: "sysfs/log", self.level, "{}", core::str::from_utf8(buf).map_err(|_| Errno::InvalidArgument)?);
        Ok(buf.len())
    }

    fn dup(&self) -> common::Result<Box<dyn super::FileDescriptor>> {
        Ok(Box::new(Self { level: self.level }))
    }
}
