use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use common::{Errno, FileKind, FileMode, FileStat, OpenFlags, Permissions, Result};
use spin::Mutex;

/// procfs root directory
pub struct ProcRoot;

impl super::FileDescriptor for ProcRoot {
    fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn super::FileDescriptor>> {
        if flags & (OpenFlags::Write | OpenFlags::Create) != OpenFlags::None {
            return Err(Errno::ReadOnlyFilesystem);
        }

        let pid = name.parse::<usize>().map_err(|_| Errno::InvalidArgument)?;
        if crate::get_global_state().process_table.read().contains(pid) {
            Ok(Arc::new(ProcessDir { pid }))
        } else {
            Err(Errno::NoSuchFileOrDir)
        }
    }

    fn read(&self, position: i64, _length: usize, callback: Box<dyn for<'a> super::RequestCallback<&'a [u8]>>) {
        let position: usize = match position.try_into() {
            Ok(position) => position,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };

        if let Some((pid, _process)) = crate::get_global_state().process_table.read().iter().nth(position) {
            let mut data = Vec::new();
            data.extend_from_slice(&(0_u32.to_ne_bytes()));
            data.extend_from_slice(pid.to_string().as_bytes());
            data.push(0);

            callback(Ok(&data), false);
        } else {
            callback(Ok(&[]), false);
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

pub struct ProcessDir {
    pid: usize,
}

// https://danielkeep.github.io/tlborm/book/blk-counting.html
macro_rules! count {
    () => (0usize);
    ( $x:tt $($xs:tt)* ) => (1usize + count!($($xs)*));
}

macro_rules! make_procfs {
    ( $($name:tt => $type:ident),+ $(,)? ) => {
        const PROC_FS_FILES: [&'static str; count!($($name)*)] = [$($name ,)*];

        impl super::FileDescriptor for ProcessDir {
            fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn super::FileDescriptor>> {
                if flags & OpenFlags::Create != OpenFlags::None {
                    return Err(Errno::ReadOnlyFilesystem);
                }

                match name.as_str() {
                    $($name => Ok(Arc::new($type::new(self.pid)?)),)*
                    _ => Err(Errno::NoSuchFileOrDir),
                }
            }

            fn read(&self, position: i64, _length: usize, callback: Box<dyn for<'a> super::RequestCallback<&'a [u8]>>) {
                let position: usize = match position.try_into() {
                    Ok(position) => position,
                    Err(_) => return callback(Err(Errno::ValueOverflow), false),
                };

                if position >= PROC_FS_FILES.len() {
                    callback(Ok(&[]), false);
                } else {
                    let entry = PROC_FS_FILES[position];
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

make_procfs![
    "cwd" => CwdLink,
    "files" => FilesDir,
    "memory" => MemoryDir,
    "pid" => PidFile,
];

pub struct CwdLink {
    pid: usize,
}

impl CwdLink {
    fn new(pid: usize) -> Result<Self> {
        Ok(Self { pid })
    }
}

impl super::FileDescriptor for CwdLink {
    fn read(&self, _position: i64, _length: usize, callback: Box<dyn for<'a> super::RequestCallback<&'a [u8]>>) {
        let path = match crate::get_global_state().process_table.read().get(self.pid) {
            Some(process) => process.environment.get_cwd_path(),
            None => return callback(Err(Errno::NoSuchProcess), false),
        };
        let data = path.as_bytes();

        callback(Ok(data), false);
    }

    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::OwnerExecute | Permissions::GroupRead | Permissions::GroupExecute | Permissions::OtherRead | Permissions::OtherExecute,
                kind: FileKind::SymLink,
            },
            ..Default::default()
        })
    }
}

/// directory containing links to all open files in a process
pub struct FilesDir {
    pid: usize,
}

impl FilesDir {
    fn new(pid: usize) -> Result<Self> {
        Ok(Self { pid })
    }

    fn get_file_descriptors(&self) -> Result<Arc<Mutex<crate::array::ConsistentIndexArray<super::OpenFile>>>> {
        let process_table = crate::get_global_state().process_table.read();
        let process = process_table.get(self.pid).ok_or(Errno::NoSuchProcess)?;
        Ok(process.environment.file_descriptors.clone())
    }
}

impl super::FileDescriptor for FilesDir {
    fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn super::FileDescriptor>> {
        if flags & (OpenFlags::Write | OpenFlags::Create) != OpenFlags::None {
            return Err(Errno::ReadOnlyFilesystem);
        }

        let fd = name.parse::<usize>().map_err(|_| Errno::InvalidArgument)?;
        if self.get_file_descriptors()?.lock().contains(fd) {
            Ok(Arc::new(ProcessFd { pid: self.pid, fd }))
        } else {
            Err(Errno::NoSuchFileOrDir)
        }
    }

    fn read(&self, position: i64, _length: usize, callback: Box<dyn for<'a> super::RequestCallback<&'a [u8]>>) {
        let position: usize = match position.try_into() {
            Ok(position) => position,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };

        let file_descriptors = match self.get_file_descriptors() {
            Ok(fds) => fds,
            Err(err) => return callback(Err(err), false),
        };
        let file_descriptors = file_descriptors.lock();

        if let Some((fd, _file)) = file_descriptors.as_slice().iter().enumerate().filter(|(_, i)| i.is_some()).nth(position) {
            let mut data = Vec::new();
            data.extend_from_slice(&(0_u32.to_ne_bytes()));
            data.extend_from_slice(fd.to_string().as_bytes());
            data.push(0);

            callback(Ok(&data), false);
        } else {
            callback(Ok(&[]), false);
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

/// provides a symlink to the file pointed at by a file descriptor
pub struct ProcessFd {
    pid: usize,
    fd: usize,
}

impl super::FileDescriptor for ProcessFd {
    fn read(&self, _position: i64, _length: usize, callback: Box<dyn for<'a> super::RequestCallback<&'a [u8]>>) {
        let process_table = crate::get_global_state().process_table.read();
        let process = match process_table.get(self.pid) {
            Some(process) => process,
            None => return callback(Err(Errno::NoSuchProcess), false),
        };
        let path = match process.environment.get_path(self.fd) {
            Ok(path) => path,
            Err(err) => return callback(Err(err), false),
        };
        let data = path.as_bytes();

        callback(Ok(data), false);
    }

    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::OwnerExecute | Permissions::GroupRead | Permissions::GroupExecute | Permissions::OtherRead | Permissions::OtherExecute,
                kind: FileKind::SymLink,
            },
            ..Default::default()
        })
    }
}

/// allots a process to read its own pid by
pub struct PidFile {
    data: String,
}

impl PidFile {
    fn new(pid: usize) -> Result<Self> {
        Ok(Self { data: pid.to_string() })
    }
}

impl super::FileDescriptor for PidFile {
    fn read(&self, position: i64, length: usize, callback: Box<dyn for<'a> super::RequestCallback<&'a [u8]>>) {
        let position: usize = match position.try_into() {
            Ok(position) => position,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };

        callback(Ok(&self.data.as_bytes()[position..position + length]), false);
    }

    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::GroupRead | Permissions::OtherRead,
                kind: FileKind::Regular,
            },
            ..Default::default()
        })
    }
}

/// allows for processes to manipulate their memory map by manipulating files
pub struct MemoryDir {
    pid: usize,
    map: Arc<Mutex<crate::mm::ProcessMap>>,
}

impl MemoryDir {
    fn new(pid: usize) -> Result<Self> {
        Ok(Self {
            pid,
            map: crate::get_global_state().process_table.read().get(pid).ok_or(Errno::NoSuchProcess)?.memory_map.clone(),
        })
    }
}

impl super::FileDescriptor for MemoryDir {
    fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn super::FileDescriptor>> {
        if flags & OpenFlags::Write != OpenFlags::None {
            return Err(Errno::OperationNotSupported);
        }

        let base = usize::from_str_radix(&name, 16).map_err(|_| Errno::InvalidArgument)?;

        if flags & OpenFlags::Create != OpenFlags::None {
            // create file, directory, or symlink based on flags
            // mapping for file will be inserted when it's given permissions with chmod and a size with truncate
            // mapping for directory will be inserted when the `target` file/symlink is created in it as with normal files/symlinks
            // mapping for symlink will be inserted when it's given a target and permissions with chmod
            // TODO: how should symlinks be created? maybe a special flag
            todo!();
        } else {
            for map in self.map.lock().map.iter() {
                if map.region().base == base {
                    match map.kind() {
                        crate::mm::MappingKind::Anonymous => {
                            return Ok(Arc::new(AnonMem {
                                pid: self.pid,
                                map: self.map.clone(),
                                base,
                            }))
                        }
                        crate::mm::MappingKind::File { .. } => todo!(), // need to get the path somehow and make a symlink
                    }
                }
            }

            Err(Errno::NoSuchFileOrDir)
        }
    }

    fn read(&self, position: i64, _length: usize, callback: Box<dyn for<'a> super::RequestCallback<&'a [u8]>>) {
        let map = self.map.lock();

        let position: usize = match position.try_into() {
            Ok(position) => position,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };

        let map = match map.map.get(position) {
            Some(map) => map,
            None => return callback(Ok(&[]), false),
        };
        let entry = format!("{:0width$x}", map.region().base, width = core::mem::size_of::<usize>() * 2);

        let mut data = Vec::new();
        data.extend_from_slice(&(0_u32.to_ne_bytes()));
        data.extend_from_slice(entry.as_bytes());
        data.push(0);

        callback(Ok(&data), false);
    }

    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead
                    | Permissions::OwnerWrite
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

pub struct AnonMem {
    pid: usize,
    map: Arc<Mutex<crate::mm::ProcessMap>>,
    base: usize,
}

impl super::FileDescriptor for AnonMem {
    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::OwnerWrite | Permissions::GroupRead,
                kind: FileKind::Regular,
            },
            ..Default::default()
        })
    }

    fn truncate(&self, _len: i64) -> Result<()> {
        // change the size of the mapping to the given size
        todo!();
    }

    fn read(&self, _position: i64, _length: usize, _callback: Box<dyn for<'a> super::RequestCallback<&'a [u8]>>) {
        // read much like sysfs/mem
        todo!();
    }

    fn write(&self, _position: i64, _length: usize, _callback: Box<dyn for<'a> super::RequestCallback<&'a mut [u8]>>) {
        // write much like sysfs/mem
        todo!();
    }

    fn get_page(&self, _position: i64, _callback: Box<dyn FnOnce(Option<crate::arch::PhysicalAddress>, bool)>) {
        // get page much like sysfs/mem, gotta figure out how to do this without deadlocks
        todo!();
    }
}
