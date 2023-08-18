//! procfs filesystem

use core::mem::size_of;

use crate::process::Buffer;

use super::{
    kernel::FileDescriptor,
    user::{ResponseInProgress, UserspaceFs},
};
use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use async_trait::async_trait;
use common::{Errno, EventResponse, FileKind, FileMode, FileStat, OpenFlags, Permissions, Result};
use spin::Mutex;

/// procfs root directory
pub struct ProcRoot;

#[async_trait]
impl FileDescriptor for ProcRoot {
    async fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn FileDescriptor>> {
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

    async fn read(&self, position: i64, buffer: Buffer) -> Result<usize> {
        let position: usize = position.try_into().map_err(|_| Errno::ValueOverflow)?;

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

pub struct ProcSelfLink;

#[async_trait]
impl FileDescriptor for ProcSelfLink {
    async fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::GroupRead | Permissions::OtherRead,
                kind: FileKind::SymLink,
            },
            size: 24,
            ..Default::default()
        })
    }

    async fn read(&self, _position: i64, buffer: Buffer) -> Result<usize> {
        let pid = crate::get_global_state().cpus.read()[0].scheduler.get_current_task().and_then(|task| task.lock().pid);

        if let Some(pid) = pid {
            buffer.copy_from(pid.to_string().as_bytes()).await
        } else {
            Err(Errno::NoSuchProcess)
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

        #[async_trait]
        impl FileDescriptor for $class_name {
            async fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn FileDescriptor>> {
                if flags & OpenFlags::Create != OpenFlags::None {
                    return Err(Errno::ReadOnlyFilesystem);
                }

                match name.as_str() {
                    $($name => Ok(Arc::new($type::new(self.pid, flags)?)),)*
                    _ => Err(Errno::NoSuchFileOrDir),
                }
            }

            async fn read(&self, position: i64, buffer: Buffer) -> Result<usize> {
                let position: usize = position.try_into().map_err(|_| Errno::ValueOverflow)?;

                let mut data = Vec::new();

                if position < Self::files().len() {
                    let entry = Self::files()[position];
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

make_procfs![
    as ProcessDir,
    "cwd" => CwdLink,
    "files" => FilesDir,
    "filesystem" => FsEventsDir,
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

#[async_trait]
impl FileDescriptor for CwdLink {
    async fn read(&self, _position: i64, buffer: Buffer) -> Result<usize> {
        let path = crate::get_global_state().process_table.read().get(self.pid).ok_or(Errno::NoSuchProcess)?.environment.get_cwd_path();
        buffer.copy_from(path.as_bytes()).await
    }

    async fn stat(&self) -> Result<FileStat> {
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

#[async_trait]
impl FileDescriptor for RootLink {
    async fn read(&self, _position: i64, buffer: Buffer) -> Result<usize> {
        let path = crate::get_global_state().process_table.read().get(self.pid).ok_or(Errno::NoSuchProcess)?.environment.get_root_path();
        buffer.copy_from(path.as_bytes()).await
    }

    async fn stat(&self) -> Result<FileStat> {
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

#[async_trait]
impl FileDescriptor for FilesDir {
    async fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn FileDescriptor>> {
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

    async fn read(&self, position: i64, buffer: Buffer) -> Result<usize> {
        let position: usize = position.try_into().map_err(|_| Errno::ValueOverflow)?;
        let file_descriptors = self.get_file_descriptors()?;
        let file_descriptors = file_descriptors.lock();
        let mut data = Vec::new();

        if let Some((fd, _file)) = file_descriptors.as_slice().iter().enumerate().filter(|(_, i)| i.is_some()).nth(position) {
            data.extend_from_slice(&(0_u32.to_ne_bytes()));
            data.extend_from_slice(fd.to_string().as_bytes());
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

/// provides a symlink to the file pointed at by a file descriptor
pub struct ProcessFd {
    pid: usize,
    fd: usize,
}

#[async_trait]
impl FileDescriptor for ProcessFd {
    async fn read(&self, _position: i64, buffer: Buffer) -> Result<usize> {
        let process_table = crate::get_global_state().process_table.read();
        let process = process_table.get(self.pid).ok_or(Errno::NoSuchProcess)?;
        let path = process.environment.get_path(self.fd)?;

        buffer.copy_from(path.as_bytes()).await
    }

    async fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::OwnerExecute | Permissions::GroupRead | Permissions::GroupExecute | Permissions::OtherRead | Permissions::OtherExecute,
                kind: FileKind::SymLink,
            },
            ..Default::default()
        })
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

#[async_trait]
impl FileDescriptor for FsName {
    async fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::OwnerWrite | Permissions::GroupRead | Permissions::OtherRead,
                kind: FileKind::Regular,
            },
            ..Default::default()
        })
    }

    async fn write(&self, _position: i64, buffer: Buffer) -> Result<usize> {
        const BUF_LEN: usize = 256;
        let mut buf = [0; BUF_LEN];
        let bytes_written = buffer.copy_into(&mut buf).await?;

        if let Ok(str) = core::str::from_utf8(&buf[..bytes_written]) && let Some(process) = crate::get_global_state().process_table.read().get(self.pid) {
            let filesystem = Arc::new(UserspaceFs::new());
            *process.filesystem.lock() = Some(filesystem.clone());
            process.environment.namespace.write().insert(str.to_string(), filesystem);
        }

        Ok(bytes_written)
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

#[async_trait]
impl FileDescriptor for FsFromKernel {
    async fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead,
                kind: FileKind::CharSpecial,
            },
            ..Default::default()
        })
    }

    async fn read(&self, _position: i64, buffer: Buffer) -> Result<usize> {
        self.filesystem.wait_for_event(buffer).await
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

#[async_trait]
impl FileDescriptor for FsToKernel {
    async fn stat(&self) -> Result<FileStat> {
        Ok(FileStat {
            mode: FileMode {
                permissions: Permissions::OwnerRead | Permissions::OwnerWrite,
                kind: FileKind::CharSpecial,
            },
            ..Default::default()
        })
    }

    async fn read(&self, _position: i64, buffer: Buffer) -> Result<usize> {
        let response = self.response.lock().take();
        if let Some(response) = response {
            response.read(buffer).await
        } else {
            Err(Errno::TryAgain)
        }
    }

    async fn write(&self, _position: i64, buffer: Buffer) -> Result<usize> {
        let response = self.response.lock().take();
        if let Some(response) = response {
            response.write(buffer).await
        } else {
            let mut buf = [0; core::mem::size_of::<EventResponse>()];
            let bytes_written = buffer.copy_into(&mut buf).await?;

            if bytes_written < size_of::<EventResponse>() {
                return Err(Errno::TryAgain);
            }

            let response = EventResponse::try_from(buf).map_err(|_| Errno::InvalidArgument)?;
            if let Some(response) = self.filesystem.respond(&response)? {
                *self.response.lock() = Some(response);
            };

            Ok(bytes_written)
        }
    }
}
