use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use common::{Errno, OpenFlags};
use spin::Mutex;

pub struct ProcFs;

impl super::Filesystem for ProcFs {
    fn get_root_dir(&self) -> Box<dyn super::FileDescriptor> {
        Box::new(ProcRoot { seek_pos: Mutex::new(0) })
    }
}

/// procfs root directory
pub struct ProcRoot {
    seek_pos: Mutex<usize>,
}

impl super::FileDescriptor for ProcRoot {
    fn open(&self, name: &str, flags: OpenFlags) -> common::Result<alloc::boxed::Box<dyn super::FileDescriptor>> {
        if flags & (OpenFlags::Write | OpenFlags::Create) != OpenFlags::None {
            return Err(Errno::ReadOnlyFilesystem);
        }

        let pid = name.parse::<usize>().map_err(|_| Errno::InvalidArgument)?;
        if crate::get_global_state().process_table.read().contains(pid) {
            Ok(Box::new(ProcessDir { pid, seek_pos: Mutex::new(0) }))
        } else {
            Err(common::Errno::NoSuchFileOrDir)
        }
    }

    fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
        let mut seek_pos = self.seek_pos.lock();
        let process_table = crate::get_global_state().process_table.read();

        while *seek_pos < process_table.max_pid() {
            let pid = *seek_pos;
            *seek_pos += 1;

            if process_table.contains(pid) {
                let mut data = Vec::new();
                data.extend_from_slice(&(0_u32.to_ne_bytes()));
                data.extend_from_slice(pid.to_string().as_bytes());
                data.push(0);

                if buf.len() > data.len() {
                    buf[..data.len()].copy_from_slice(&data);
                    return Ok(data.len());
                } else {
                    buf.copy_from_slice(&data[..buf.len()]);
                    return Ok(buf.len());
                }
            }
        }

        if *seek_pos >= process_table.max_pid() {
            *seek_pos = process_table.max_pid();
        }
        Ok(0)
    }

    fn seek(&self, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
        seek_helper(&self.seek_pos, crate::get_global_state().process_table.read().max_pid(), offset, kind)
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

    fn dup(&self) -> common::Result<Box<dyn crate::fs::FileDescriptor>> {
        Ok(Box::new(Self {
            seek_pos: Mutex::new(*self.seek_pos.lock()),
        }))
    }
}

fn seek_helper(seek_pos: &Mutex<usize>, max_value: usize, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
    let mut seek_pos = seek_pos.lock();
    let seek_pos_i64: i64 = (*seek_pos).try_into().map_err(|_| Errno::ValueOverflow)?;

    match kind {
        common::SeekKind::Current => *seek_pos = (seek_pos_i64 + offset).try_into().map_err(|_| Errno::ValueOverflow)?,
        common::SeekKind::End => {
            let max_pid: i64 = max_value.try_into().map_err(|_| Errno::ValueOverflow)?;
            *seek_pos = (max_pid + offset).try_into().map_err(|_| Errno::ValueOverflow)?;
        }
        common::SeekKind::Set => *seek_pos = offset.try_into().map_err(|_| Errno::ValueOverflow)?,
    }

    (*seek_pos).try_into().map_err(|_| Errno::ValueOverflow)
}

pub struct ProcessDir {
    pid: usize,
    seek_pos: Mutex<usize>,
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
            fn open(&self, name: &str, flags: OpenFlags) -> common::Result<alloc::boxed::Box<dyn super::FileDescriptor>> {
                if flags & OpenFlags::Create != OpenFlags::None {
                    return Err(Errno::ReadOnlyFilesystem);
                }

                match name {
                    $($name => Ok(Box::new($type::new(self.pid)?)),)*
                    _ => Err(Errno::NoSuchFileOrDir),
                }
            }

            fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
                let mut seek_pos = self.seek_pos.lock();

                if *seek_pos >= PROC_FS_FILES.len() {
                    *seek_pos = PROC_FS_FILES.len();
                    return Ok(0);
                }

                let entry = &PROC_FS_FILES[*seek_pos];
                *seek_pos += 1;
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

            fn seek(&self, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
                seek_helper(&self.seek_pos, PROC_FS_FILES.len().try_into().map_err(|_| Errno::ValueOverflow)?, offset, kind)
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
                Ok(Box::new(Self { pid: self.pid, seek_pos: Mutex::new(*self.seek_pos.lock()) }))
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
    fn new(pid: usize) -> common::Result<Self> {
        Ok(Self { pid })
    }
}

impl super::FileDescriptor for CwdLink {
    fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
        let path = crate::get_global_state().process_table.read().get(self.pid).ok_or(Errno::NoSuchProcess)?.environment.get_cwd_path();
        let data = path.as_bytes();

        if buf.len() > data.len() {
            buf[..data.len()].copy_from_slice(data);
            Ok(data.len())
        } else {
            buf.copy_from_slice(&data[..buf.len()]);
            Ok(buf.len())
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
                kind: common::FileKind::SymLink,
            },
            ..Default::default()
        })
    }

    fn dup(&self) -> common::Result<Box<dyn crate::fs::FileDescriptor>> {
        Ok(Box::new(Self { pid: self.pid }))
    }
}

/// directory containing links to all open files in a process
pub struct FilesDir {
    pid: usize,
    seek_pos: Mutex<usize>,
}

impl FilesDir {
    fn new(pid: usize) -> common::Result<Self> {
        Ok(Self { pid, seek_pos: Mutex::new(0) })
    }

    fn get_file_descriptors(&self) -> common::Result<Arc<Mutex<crate::array::ConsistentIndexArray<super::OpenFile>>>> {
        let process_table = crate::get_global_state().process_table.read();
        let process = process_table.get(self.pid).ok_or(Errno::NoSuchProcess)?;
        Ok(process.environment.file_descriptors.clone())
    }
}

impl super::FileDescriptor for FilesDir {
    fn open(&self, name: &str, flags: OpenFlags) -> common::Result<alloc::boxed::Box<dyn super::FileDescriptor>> {
        if flags & (OpenFlags::Write | OpenFlags::Create) != OpenFlags::None {
            return Err(Errno::ReadOnlyFilesystem);
        }

        let fd = name.parse::<usize>().map_err(|_| Errno::InvalidArgument)?;
        if self.get_file_descriptors()?.lock().contains(fd) {
            Ok(Box::new(ProcessFd { pid: self.pid, fd }))
        } else {
            Err(common::Errno::NoSuchFileOrDir)
        }
    }

    fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
        let mut seek_pos = self.seek_pos.lock();
        let file_descriptors = self.get_file_descriptors()?;
        let file_descriptors = file_descriptors.lock();

        while *seek_pos <= file_descriptors.max_index() {
            let fd = *seek_pos;
            *seek_pos += 1;

            if file_descriptors.contains(fd) {
                let mut data = Vec::new();
                data.extend_from_slice(&(0_u32.to_ne_bytes()));
                data.extend_from_slice(fd.to_string().as_bytes());
                data.push(0);

                if buf.len() > data.len() {
                    buf[..data.len()].copy_from_slice(&data);
                    return Ok(data.len());
                } else {
                    buf.copy_from_slice(&data[..buf.len()]);
                    return Ok(buf.len());
                }
            }
        }

        if *seek_pos >= file_descriptors.max_index() {
            *seek_pos = file_descriptors.max_index();
        }
        Ok(0)
    }

    fn seek(&self, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
        seek_helper(&self.seek_pos, crate::get_global_state().process_table.read().max_pid(), offset, kind)
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

    fn dup(&self) -> common::Result<Box<dyn crate::fs::FileDescriptor>> {
        Ok(Box::new(Self {
            pid: self.pid,
            seek_pos: Mutex::new(*self.seek_pos.lock()),
        }))
    }
}

/// provides a symlink to the file pointed at by a file descriptor
pub struct ProcessFd {
    pid: usize,
    fd: usize,
}

impl super::FileDescriptor for ProcessFd {
    fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
        let path = crate::get_global_state()
            .process_table
            .read()
            .get(self.pid)
            .ok_or(Errno::NoSuchProcess)?
            .environment
            .get_path(self.fd)?;
        let data = path.as_bytes();

        if buf.len() > data.len() {
            buf[..data.len()].copy_from_slice(data);
            Ok(data.len())
        } else {
            buf.copy_from_slice(&data[..buf.len()]);
            Ok(buf.len())
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
                kind: common::FileKind::SymLink,
            },
            ..Default::default()
        })
    }

    fn dup(&self) -> common::Result<Box<dyn crate::fs::FileDescriptor>> {
        Ok(Box::new(Self { pid: self.pid, fd: self.fd }))
    }
}

/// allots a process to read its own pid by
pub struct PidFile {
    data: String,
    seek_pos: Mutex<usize>,
}

impl PidFile {
    fn new(pid: usize) -> common::Result<Self> {
        Ok(Self {
            data: pid.to_string(),
            seek_pos: Mutex::new(0),
        })
    }
}

impl super::FileDescriptor for PidFile {
    fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
        let mut seek_pos = self.seek_pos.lock();
        let len_bytes = self.data.as_bytes().len();

        if *seek_pos >= len_bytes {
            *seek_pos = len_bytes;
            return Ok(0);
        }

        let pos = *seek_pos;
        *seek_pos += 1;

        if pos + buf.len() > self.data.as_bytes().len() {
            let len = self.data.as_bytes().len() - pos;
            buf[..len].copy_from_slice(&self.data.as_bytes()[pos..]);
            Ok(len)
        } else {
            buf.copy_from_slice(&self.data.as_bytes()[pos..pos + buf.len()]);
            Ok(buf.len())
        }
    }

    fn seek(&self, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
        seek_helper(&self.seek_pos, self.data.len(), offset, kind)
    }

    fn stat(&self) -> common::Result<common::FileStat> {
        Ok(common::FileStat {
            mode: common::FileMode {
                permissions: common::Permissions::OwnerRead | common::Permissions::GroupRead | common::Permissions::OtherRead,
                kind: common::FileKind::Regular,
            },
            ..Default::default()
        })
    }

    fn dup(&self) -> common::Result<Box<dyn super::FileDescriptor>> {
        Ok(Box::new(Self {
            data: self.data.clone(),
            seek_pos: Mutex::new(*self.seek_pos.lock()),
        }))
    }
}

/// allows for processes to manipulate their memory map by manipulating files
pub struct MemoryDir {
    pid: usize,
    map: Arc<Mutex<crate::mm::ProcessMap>>,
    seek_pos: Mutex<usize>,
}

impl MemoryDir {
    fn new(pid: usize) -> common::Result<Self> {
        Ok(Self {
            pid,
            map: crate::get_global_state().process_table.read().get(pid).ok_or(Errno::NoSuchProcess)?.memory_map.clone(),
            seek_pos: Mutex::new(0),
        })
    }
}

impl super::FileDescriptor for MemoryDir {
    fn open(&self, name: &str, flags: common::OpenFlags) -> common::Result<Box<dyn super::FileDescriptor>> {
        if flags & OpenFlags::Write != OpenFlags::None {
            return Err(Errno::OperationNotSupported);
        }

        let base = usize::from_str_radix(name, 16).map_err(|_| Errno::InvalidArgument)?;

        for map in self.map.lock().map.iter() {
            if map.region().base == base {
                return Ok(Box::new(AnonFile {
                    pid: self.pid,
                    map: self.map.clone(),
                    base,
                    exists: true,
                    seek_pos: Mutex::new(0),
                }));
            }
        }

        Err(Errno::NoSuchFileOrDir)
    }

    fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
        let mut seek_pos = self.seek_pos.lock();
        let map = self.map.lock();

        if *seek_pos >= map.map.len() {
            *seek_pos = map.map.len();
            return Ok(0);
        }

        // format base address as hexadecimal and pad it with zeroes
        let entry = format!("{:0width$x}", map.map.get(*seek_pos).unwrap().region().base, width = core::mem::size_of::<usize>() * 2);
        *seek_pos += 1;

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

    fn stat(&self) -> common::Result<common::FileStat> {
        Ok(common::FileStat {
            mode: common::FileMode {
                permissions: common::Permissions::OwnerRead
                    | common::Permissions::OwnerWrite
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

    fn seek(&self, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
        seek_helper(&self.seek_pos, self.map.lock().map.len(), offset, kind)
    }

    fn dup(&self) -> common::Result<Box<dyn crate::fs::FileDescriptor>> {
        Ok(Box::new(Self {
            pid: self.pid,
            map: self.map.clone(),
            seek_pos: Mutex::new(*self.seek_pos.lock()),
        }))
    }
}

pub struct AnonFile {
    pid: usize,
    map: Arc<Mutex<crate::mm::ProcessMap>>,
    base: usize,
    exists: bool,
    seek_pos: Mutex<usize>,
}

impl super::FileDescriptor for AnonFile {
    fn chmod(&self, _permissions: common::Permissions) -> common::Result<()> {
        if !self.exists {
            Err(Errno::OperationNotSupported)
        } else {
            todo!();
        }
    }

    fn stat(&self) -> common::Result<common::FileStat> {
        Ok(common::FileStat {
            mode: common::FileMode {
                permissions: common::Permissions::OwnerRead | common::Permissions::OwnerWrite | common::Permissions::GroupRead | common::Permissions::OtherRead,
                kind: common::FileKind::Regular,
            },
            ..Default::default()
        })
    }

    fn truncate(&self, _len: u64) -> common::Result<()> {
        if !self.exists {
            todo!();
        } else {
            Err(Errno::OperationNotSupported)
        }
    }

    fn dup(&self) -> common::Result<Box<dyn super::FileDescriptor>> {
        Ok(Box::new(Self {
            pid: self.pid,
            map: self.map.clone(),
            base: self.base,
            exists: self.exists,
            seek_pos: Mutex::new(*self.seek_pos.lock()),
        }))
    }
}
