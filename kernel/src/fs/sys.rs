use alloc::{boxed::Box, vec::Vec};
use common::{OpenFlags, Permissions};
use core::sync::atomic::AtomicUsize;

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
    ($($name:tt => $type:ident),+ $(,)?) => {
        const SYS_FS_FILES: [&'static str; count!($($name),*)] = [$($name),*];

        impl super::FileDescriptor for SysFsRoot {
            fn open(&self, name: &str, flags: OpenFlags) -> common::Result<alloc::boxed::Box<dyn super::FileDescriptor>> {
                if flags & (OpenFlags::Write | OpenFlags::Create | OpenFlags::Directory) != OpenFlags::None {
                    return Err(common::Error::ReadOnly);
                }

                match name {
                    $($name => Ok(Box::new($type::new())),),*
                    _ => Err(common::Error::DoesntExist),
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
                    let len: u32 = entry.len().try_into().map_err(|_| common::Error::Overflow)?;
                    data.extend_from_slice(&(len.to_ne_bytes()));
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

            fn seek(&self, offset: isize, kind: common::SeekKind) -> common::Result<usize> {
                match kind {
                    common::SeekKind::Current => match offset.cmp(&0) {
                        core::cmp::Ordering::Greater => {
                            let val = offset.try_into().map_err(|_| common::Error::Overflow)?;
                            let old_val = self.seek_pos.fetch_add(val, core::sync::atomic::Ordering::SeqCst);
                            Ok(old_val + val)
                        }
                        core::cmp::Ordering::Less => {
                            let val = (-offset).try_into().map_err(|_| common::Error::Overflow)?;
                            let old_val = self.seek_pos.fetch_sub(val, core::sync::atomic::Ordering::SeqCst);
                            Ok(old_val - val)
                        }
                        core::cmp::Ordering::Equal => Ok(self.seek_pos.load(core::sync::atomic::Ordering::SeqCst)),
                    }
                    common::SeekKind::End => {
                        let len: isize = SYS_FS_FILES.len().try_into().map_err(|_| common::Error::Overflow)?;
                        let new_val = (len + offset).try_into().map_err(|_| common::Error::Overflow)?;
                        self.seek_pos.store(new_val, core::sync::atomic::Ordering::SeqCst);
                        Ok(new_val)
                    }
                    common::SeekKind::Set => {
                        let new_val = offset.try_into().map_err(|_| common::Error::Overflow)?;
                        self.seek_pos.store(new_val, core::sync::atomic::Ordering::SeqCst);
                        Ok(new_val)
                    }
                }
            }

            fn stat(&self) -> common::Result<common::FileStat> {
                Ok(common::FileStat {
                    permissions: common::Permissions::OwnerRead
                        | common::Permissions::OwnerExecute
                        | common::Permissions::GroupRead
                        | common::Permissions::GroupExecute
                        | common::Permissions::OtherRead
                        | common::Permissions::OtherExecute,
                    ..Default::default()
                })
            }
        }
    };
}

make_sysfs![
    "debug" => Debug,
];

/// simple way to allow programs to print debug info to the kernel log
struct Debug;

impl Debug {
    fn new() -> Self {
        Self
    }
}

impl super::FileDescriptor for Debug {
    fn stat(&self) -> common::Result<common::FileStat> {
        Ok(common::FileStat {
            permissions: Permissions::OwnerWrite | Permissions::GroupWrite | Permissions::OtherWrite,
            ..Default::default()
        })
    }

    fn write(&self, buf: &[u8]) -> common::Result<usize> {
        use log::debug;
        debug!("{}", core::str::from_utf8(buf).map_err(|_| common::Error::BadInput)?);
        Ok(buf.len())
    }
}
