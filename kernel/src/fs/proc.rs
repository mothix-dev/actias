//! procfs filesystem

use core::mem::size_of;

use crate::process::Buffer;

use super::{
    kernel::FileDescriptor,
    user::{ResponseInProgress, UserspaceFs},
};
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use common::{Errno, EventResponse, FileKind, FileMode, FileStat, OpenFlags, Permissions, Result};
use spin::Mutex;

/// procfs root directory
pub struct ProcRoot;

impl FileDescriptor for ProcRoot {
    fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn FileDescriptor>> {
        if flags & (OpenFlags::Write | OpenFlags::Create) != OpenFlags::None {
            return Err(Errno::ReadOnlyFilesystem);
        }

        if name == "self" {
            Ok(Arc::new(ProcSelfLink))
        } else {
            let pid = name.parse::<usize>().map_err(|_| Errno::InvalidArgument)?;
            if crate::get_global_state().process_table.read().contains(pid) {
                Ok(Arc::new(ProcessDir { pid }))
            } else {
                Err(Errno::NoSuchFileOrDir)
            }
        }
    }

    fn read(&self, position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        let position: usize = match position.try_into() {
            Ok(position) => position,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };

        let mut data = Vec::new();

        if position == 0 {
            data.extend_from_slice(&(0_u32.to_ne_bytes()));
            data.extend_from_slice("self".as_bytes());
            data.push(0);
        } else if let Some((pid, _process)) = crate::get_global_state().process_table.read().iter().nth(position - 1) {
            data.extend_from_slice(&(0_u32.to_ne_bytes()));
            data.extend_from_slice(pid.to_string().as_bytes());
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

pub struct ProcSelfLink;

impl FileDescriptor for ProcSelfLink {
    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::GroupRead | Permissions::OtherRead,
                kind: FileKind::SymLink,
            },
            size: 24,
            ..Default::default()
        })
    }

    fn read(&self, _position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        let pid = crate::get_global_state().cpus.read()[0].scheduler.get_current_task().and_then(|task| task.lock().pid);

        if let Some(pid) = pid {
            let res = buffer.copy_from(pid.to_string().as_bytes());
            callback(res, false);
        } else {
            callback(Err(Errno::NoSuchProcess), false);
        }
    }
}

macro_rules! make_procfs {
    ( as $class_name:ident , $($name:tt => $type:ident),+ $(,)? ) => {
        pub struct $class_name {
            pid: usize,
        }

        impl $class_name {
            pub fn new(pid: usize, flags: OpenFlags) -> Result<Self> {
                if flags & OpenFlags::Write != OpenFlags::None {
                    Err(Errno::OperationNotPermitted)
                } else {
                    Ok(Self { pid })
                }
            }

            fn files() -> &'static [&'static str] {
                &[$($name ,)*]
            }
        }

        impl FileDescriptor for $class_name {
            fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn FileDescriptor>> {
                if flags & OpenFlags::Create != OpenFlags::None {
                    return Err(Errno::ReadOnlyFilesystem);
                }

                match name.as_str() {
                    $($name => Ok(Arc::new($type::new(self.pid, flags)?)),)*
                    _ => Err(Errno::NoSuchFileOrDir),
                }
            }

            fn read(&self, position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
                let position: usize = match position.try_into() {
                    Ok(position) => position,
                    Err(_) => return callback(Err(Errno::ValueOverflow), false),
                };

                let mut data = Vec::new();

                if position < Self::files().len() {
                    let entry = Self::files()[position];
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

make_procfs![
    as ProcessDir,
    "cwd" => CwdLink,
    "files" => FilesDir,
    "filesystem" => FsEventsDir,
    "memory" => MemoryDir,
    "root" => RootLink,
];

pub struct CwdLink {
    pid: usize,
}

impl CwdLink {
    fn new(pid: usize, flags: OpenFlags) -> Result<Self> {
        if flags & OpenFlags::Write != OpenFlags::None {
            Err(Errno::OperationNotPermitted)
        } else {
            Ok(Self { pid })
        }
    }
}

impl FileDescriptor for CwdLink {
    fn read(&self, _position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        let path = match crate::get_global_state().process_table.read().get(self.pid) {
            Some(process) => process.environment.get_cwd_path(),
            None => return callback(Err(Errno::NoSuchProcess), false),
        };

        let res = buffer.copy_from(path.as_bytes());
        callback(res, false);
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

pub struct RootLink {
    pid: usize,
}

impl RootLink {
    fn new(pid: usize, flags: OpenFlags) -> Result<Self> {
        if flags & OpenFlags::Write != OpenFlags::None {
            Err(Errno::OperationNotPermitted)
        } else {
            Ok(Self { pid })
        }
    }
}

impl FileDescriptor for RootLink {
    fn read(&self, _position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        let path = match crate::get_global_state().process_table.read().get(self.pid) {
            Some(process) => process.environment.get_root_path(),
            None => return callback(Err(Errno::NoSuchProcess), false),
        };

        let res = buffer.copy_from(path.as_bytes());
        callback(res, false);
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
    fn new(pid: usize, flags: OpenFlags) -> Result<Self> {
        if flags & OpenFlags::Write != OpenFlags::None {
            Err(Errno::OperationNotPermitted)
        } else {
            Ok(Self { pid })
        }
    }

    fn get_file_descriptors(&self) -> Result<Arc<Mutex<crate::array::ConsistentIndexArray<super::OpenFile>>>> {
        let process_table = crate::get_global_state().process_table.read();
        let process = process_table.get(self.pid).ok_or(Errno::NoSuchProcess)?;
        Ok(process.environment.file_descriptors.clone())
    }
}

impl FileDescriptor for FilesDir {
    fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn FileDescriptor>> {
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

    fn read(&self, position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        let position: usize = match position.try_into() {
            Ok(position) => position,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };

        let file_descriptors = match self.get_file_descriptors() {
            Ok(fds) => fds,
            Err(err) => return callback(Err(err), false),
        };
        let file_descriptors = file_descriptors.lock();

        let mut data = Vec::new();

        if let Some((fd, _file)) = file_descriptors.as_slice().iter().enumerate().filter(|(_, i)| i.is_some()).nth(position) {
            data.extend_from_slice(&(0_u32.to_ne_bytes()));
            data.extend_from_slice(fd.to_string().as_bytes());
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

/// provides a symlink to the file pointed at by a file descriptor
pub struct ProcessFd {
    pid: usize,
    fd: usize,
}

impl FileDescriptor for ProcessFd {
    fn read(&self, _position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        let process_table = crate::get_global_state().process_table.read();
        let process = match process_table.get(self.pid) {
            Some(process) => process,
            None => return callback(Err(Errno::NoSuchProcess), false),
        };
        let path = match process.environment.get_path(self.fd) {
            Ok(path) => path,
            Err(err) => return callback(Err(err), false),
        };

        let res = buffer.copy_from(path.as_bytes());
        callback(res, false);
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

/// allows for processes to manipulate their memory map by manipulating files
pub struct MemoryDir {
    pid: usize,
    map: Arc<Mutex<crate::mm::ProcessMap>>,
}

impl MemoryDir {
    fn new(pid: usize, flags: OpenFlags) -> Result<Self> {
        if flags & OpenFlags::Write != OpenFlags::None {
            Err(Errno::OperationNotPermitted)
        } else {
            Ok(Self {
                pid,
                map: crate::get_global_state().process_table.read().get(pid).ok_or(Errno::NoSuchProcess)?.memory_map.clone(),
            })
        }
    }
}

impl FileDescriptor for MemoryDir {
    fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn FileDescriptor>> {
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

    fn read(&self, position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        let map = self.map.lock();

        let position: usize = match position.try_into() {
            Ok(position) => position,
            Err(_) => return callback(Err(Errno::ValueOverflow), false),
        };

        let map = match map.map.get(position) {
            Some(map) => map,
            None => return callback(Ok(0), false),
        };
        let entry = format!("{:0width$x}", map.region().base, width = core::mem::size_of::<usize>() * 2);

        let mut data = Vec::new();
        data.extend_from_slice(&(0_u32.to_ne_bytes()));
        data.extend_from_slice(entry.as_bytes());
        data.push(0);

        let res = buffer.copy_from(&data);
        callback(res, false);
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

impl FileDescriptor for AnonMem {
    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::OwnerWrite | Permissions::GroupRead,
                kind: FileKind::Regular,
            },
            ..Default::default()
        })
    }

    fn truncate(&self, length: i64) -> Result<()> {
        let current_pid = crate::get_global_state().cpus.read()[0].scheduler.get_current_task().and_then(|task| task.lock().pid);

        // change the size of the mapping to the given size
        self.map
            .lock()
            .resize(&self.map, self.base, length.try_into().map_err(|_| Errno::ValueOverflow)?, current_pid == Some(self.pid))?;
        Ok(())
    }

    fn read(&self, _position: i64, _buffer: Buffer, _callback: Box<dyn super::RequestCallback<usize>>) {
        // read much like sysfs/mem
        todo!();
    }

    fn write(&self, _position: i64, _buffer: Buffer, _callback: Box<dyn super::RequestCallback<usize>>) {
        // write much like sysfs/mem
        todo!();
    }

    fn get_page(&self, _position: i64, _callback: Box<dyn FnOnce(Option<crate::arch::PhysicalAddress>, bool)>) {
        // get page much like sysfs/mem, gotta figure out how to do this without deadlocks
        todo!();
    }
}

make_procfs![
    as FsEventsDir,
    "name" => FsName,
    "from_kernel" => FsFromKernel,
    "to_kernel" => FsToKernel,
];

/// file used for getting a process's filesystem name or for registering a filesystem for the current process
struct FsName {
    pid: usize,
}

impl FsName {
    fn new(pid: usize, flags: OpenFlags) -> Result<Self> {
        if flags & OpenFlags::Write != OpenFlags::None {
            let current_pid = crate::get_global_state().cpus.read()[0].scheduler.get_current_task().and_then(|task| task.lock().pid);

            if current_pid != Some(pid) {
                return Err(Errno::OperationNotPermitted);
            }
        }

        Ok(Self { pid })
    }
}

impl FileDescriptor for FsName {
    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::OwnerWrite | Permissions::GroupRead | Permissions::OtherRead,
                kind: FileKind::Regular,
            },
            ..Default::default()
        })
    }

    fn write(&self, _position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        const BUF_LEN: usize = 256;
        let mut buf = [0; BUF_LEN];
        let bytes_written = match buffer.copy_into(&mut buf) {
            Ok(bytes) => bytes,
            Err(err) => return callback(Err(err), false),
        };

        if let Ok(str) = core::str::from_utf8(&buf[..bytes_written]) && let Some(process) = crate::get_global_state().process_table.read().get(self.pid) {
            let filesystem = Arc::new(UserspaceFs::new());
            *process.filesystem.lock() = Some(filesystem.clone());
            process.environment.namespace.write().insert(str.to_string(), filesystem);
        }

        callback(Ok(bytes_written), false);
    }
}

/// file used for receiving filesystem events from the kernel
struct FsFromKernel {
    filesystem: Arc<UserspaceFs>,
}

impl FsFromKernel {
    fn new(pid: usize, flags: OpenFlags) -> Result<Self> {
        let current_pid = crate::get_global_state().cpus.read()[0].scheduler.get_current_task().and_then(|task| task.lock().pid);

        if flags & OpenFlags::Write != OpenFlags::None || current_pid != Some(pid) {
            return Err(Errno::OperationNotPermitted);
        }

        if let Some(process) = crate::get_global_state().process_table.read().get(pid) && let Some(filesystem) = process.filesystem.lock().clone() {
            Ok(Self { filesystem })
        } else {
            Err(Errno::OperationNotPermitted)
        }
    }
}

impl FileDescriptor for FsFromKernel {
    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead,
                kind: FileKind::CharSpecial,
            },
            ..Default::default()
        })
    }

    fn read(&self, _position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        self.filesystem.wait_for_event(buffer, callback);
    }
}

/// file for notifying kernel of completed filesystem events and for reading/writing data for those events
struct FsToKernel {
    filesystem: Arc<UserspaceFs>,
    response: Mutex<Option<ResponseInProgress>>,
}

// once again: it's in a fucking mutex
unsafe impl Send for FsToKernel {}
unsafe impl Sync for FsToKernel {}

impl FsToKernel {
    fn new(pid: usize, _flags: OpenFlags) -> Result<Self> {
        let current_pid = crate::get_global_state().cpus.read()[0].scheduler.get_current_task().and_then(|task| task.lock().pid);

        if current_pid != Some(pid) {
            return Err(Errno::OperationNotPermitted);
        }

        if let Some(process) = crate::get_global_state().process_table.read().get(pid) && let Some(filesystem) = process.filesystem.lock().clone() {
            Ok(Self { filesystem, response: None.into() })
        } else {
            Err(Errno::OperationNotPermitted)
        }
    }
}

impl FileDescriptor for FsToKernel {
    fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::OwnerWrite,
                kind: FileKind::CharSpecial,
            },
            ..Default::default()
        })
    }

    fn read(&self, _position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        let response = self.response.lock().take();
        if let Some(response) = response {
            response.read(buffer, callback);
        } else {
            callback(Err(Errno::TryAgain), false);
        }
    }

    fn write(&self, _position: i64, buffer: Buffer, callback: Box<dyn super::RequestCallback<usize>>) {
        let response = self.response.lock().take();
        if let Some(response) = response {
            response.write(buffer, callback);
        } else {
            let mut buf = [0; core::mem::size_of::<EventResponse>()];
            let bytes_written = match buffer.copy_into(&mut buf) {
                Ok(bytes) => bytes,
                Err(err) => return callback(Err(err), false),
            };

            if bytes_written < size_of::<EventResponse>() {
                return callback(Err(Errno::TryAgain), false);
            }

            // TODO: check fields to ensure validity of data
            let response = unsafe { &*(buf.as_ptr() as *const _ as *const EventResponse) };
            match self.filesystem.respond(response) {
                Ok(Some(response)) => *self.response.lock() = Some(response),
                Ok(None) => (),
                Err(err) => return callback(Err(err), false),
            };

            callback(Ok(bytes_written), false);
        }
    }
}
