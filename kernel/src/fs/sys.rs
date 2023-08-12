use alloc::{boxed::Box, string::String, vec::Vec};
use common::{Errno, OpenFlags, Permissions};
use log::{error, log, Level};

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
            fn open(&self, name: String, flags: OpenFlags) -> common::Result<Box<dyn super::FileDescriptor>> {
                if flags & OpenFlags::Create != OpenFlags::None {
                    return Err(Errno::ReadOnlyFilesystem);
                }

                match name.as_str() {
                    $($name => Ok(Box::new($type::new())),),*
                    _ => Err(Errno::NoSuchFileOrDir),
                }
            }


            fn read(&self, position: i64, _length: usize, mut callback: Box<dyn for<'a> super::RequestCallback<&'a [u8]>>) {
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
        }
    };
}

make_sysfs![
    "log" => LogFs,
];

/// directory containing files for each log level, to allow programs to easily write to the kernel log if there's no other output method available
struct LogFs;

impl LogFs {
    fn new() -> Self {
        Self
    }
}

const LOG_LEVELS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

impl super::FileDescriptor for LogFs {
    fn open(&self, name: String, flags: OpenFlags) -> common::Result<alloc::boxed::Box<dyn super::FileDescriptor>> {
        if flags & OpenFlags::Create != OpenFlags::None {
            return Err(Errno::ReadOnlyFilesystem);
        }

        match name.as_str() {
            "error" => Ok(Box::new(Logger::new(Level::Error))),
            "warn" => Ok(Box::new(Logger::new(Level::Warn))),
            "info" => Ok(Box::new(Logger::new(Level::Info))),
            "debug" => Ok(Box::new(Logger::new(Level::Debug))),
            "trace" => Ok(Box::new(Logger::new(Level::Trace))),
            _ => Err(Errno::NoSuchFileOrDir),
        }
    }

    fn read(&self, position: i64, _length: usize, mut callback: Box<dyn for<'a> super::RequestCallback<&'a [u8]>>) {
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

    fn write(&self, _position: i64, length: usize, mut callback: Box<dyn for<'a> super::RequestCallback<&'a mut [u8]>>) {
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
