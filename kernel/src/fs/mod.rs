pub mod sys;
pub mod tar;

use crate::array::ConsistentIndexArray;
use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use common::Errno;
use core::sync::atomic::AtomicUsize;
use log::debug;
use spin::Mutex;

/// contains the filesystem environment of a process (its namespace, its root directory, etc)
#[derive(Clone)]
pub struct FsEnvironment {
    pub namespace: Arc<Mutex<BTreeMap<String, Box<dyn Filesystem>>>>,
    cwd: Arc<Mutex<OpenFile>>,
    root: Arc<Mutex<OpenFile>>,
    fs_list: Arc<Mutex<OpenFile>>,
    file_descriptors: Arc<Mutex<ConsistentIndexArray<OpenFile>>>,
}

struct ResolveResult {
    name: String,
    path: Vec<String>,
    container: Box<dyn FileDescriptor>,
}

impl FsEnvironment {
    pub fn new() -> Self {
        let namespace = Arc::new(Mutex::new(BTreeMap::new()));
        let fs_list = Arc::new(Mutex::new(OpenFile {
            descriptor: Box::new(NamespaceDir {
                namespace: namespace.clone(),
                seek_pos: AtomicUsize::new(0),
            }),
            path: Vec::new(),
            flags: common::OpenFlags::all(),
        }));
        Self {
            namespace,
            cwd: fs_list.clone(),
            root: fs_list.clone(),
            fs_list,
            file_descriptors: Arc::new(Mutex::new(ConsistentIndexArray::new())),
        }
    }

    /// implements POSIX fchmod()
    pub fn chmod(&self, file_descriptor: usize, permissions: common::Permissions) -> common::Result<()> {
        self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.chmod(permissions)
    }

    /// implements POSIX fchown()
    pub fn chown(&self, file_descriptor: usize, owner: Option<common::UserId>, group: Option<common::GroupId>) -> common::Result<()> {
        self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.descriptor.chown(owner, group)
    }

    /// implements POSIX close()
    pub fn close(&self, file_descriptor: usize) -> common::Result<()> {
        self.file_descriptors.lock().remove(file_descriptor).ok_or(Errno::BadFile).map(|_| ())
    }

    /// parses a path, removing any . or .. elements, and detects whether the new path is relative or absolute
    fn simplify_path(&self, container_path: &[String], path: &str) -> (Vec<String>, bool) {
        let mut path_stack = Vec::new();
        let mut is_absolute = false;

        for component in path.split('/') {
            match component {
                "." | "" => (),
                ".." => {
                    if path_stack.pop().is_none() && !is_absolute {
                        is_absolute = true;
                        path_stack.extend_from_slice(container_path);
                    }
                }
                _ => path_stack.push(component.to_string()),
            }
        }

        (path_stack, is_absolute)
    }

    /// iterates path elements, double checking permissions and resolving symlinks
    fn resolve_internal(&self, at: Box<dyn FileDescriptor>, mut path: Vec<String>, mut absolute_path: Option<Vec<String>>, mut name: String, no_follow: bool) -> common::Result<ResolveResult> {
        let mut last_fd = at;
        let mut buf = [0_u8; 512];

        let mut split = path.iter().chain(Some(&name)).enumerate();

        while let Some((index, component)) = split.next() {
            let new_desc = last_fd.open(component, common::OpenFlags::Read)?;

            let stat = new_desc.stat()?;
            // TODO: check permissions, symlink follow limits

            match stat.mode.kind {
                common::FileKind::Directory => {
                    if index < path.len() {
                        last_fd = new_desc
                    }
                }
                common::FileKind::SymLink => {
                    if no_follow {
                        return Err(Errno::NotDirectory);
                    }

                    // follow symlink
                    let bytes_read = new_desc.read(&mut buf)?;
                    if bytes_read == 0 {
                        return Err(Errno::InvalidArgument);
                    }

                    let target = core::str::from_utf8(&buf[..bytes_read]).map_err(|_| Errno::InvalidArgument)?;

                    match target.chars().next() {
                        Some('/') => {
                            // parse absolute path
                            let root = self.root.lock();
                            let (new_path, is_absolute) = self.simplify_path(&root.path, target);

                            if is_absolute {
                                last_fd = Box::new(LockedFileDescriptor::new(self.fs_list.clone()));
                                absolute_path = None;

                                // start over with the symlink path
                                drop(split);
                                path = new_path;
                                name = path.pop().unwrap_or_default();
                                split = path.iter().chain(Some(&name)).enumerate();
                            } else {
                                last_fd = Box::new(LockedFileDescriptor::new(self.root.clone()));
                                absolute_path = Some(concat_slices(&root.path, &path));

                                drop(split);
                                path = new_path;
                                name = path.pop().unwrap_or_default();
                                split = path.iter().chain(Some(&name)).enumerate();
                            }
                        }
                        Some(_) => {
                            // parse relative path
                            let container_path = &path[..index - 1];
                            let (new_path, is_absolute) = self.simplify_path(container_path, target);

                            if is_absolute {
                                last_fd = Box::new(LockedFileDescriptor::new(self.fs_list.clone()));
                                absolute_path = None;

                                drop(split);
                                path = new_path;
                                name = path.pop().unwrap_or_default();
                                split = path.iter().chain(Some(&name)).enumerate();
                            } else {
                                absolute_path = Some(concat_slices(container_path, &path));

                                drop(split);
                                path = new_path;
                                name = path.pop().unwrap_or_default();
                                split = path.iter().chain(Some(&name)).enumerate();
                            }
                        }
                        None => return Err(Errno::InvalidArgument),
                    }
                }
                _ => {
                    if index < path.len() {
                        return Err(Errno::NotDirectory);
                    }
                }
            }
        }

        Ok(ResolveResult {
            name,
            path: absolute_path.unwrap_or(path),
            container: last_fd,
        })
    }

    /// resolves a relative and absolute path to the container at the given file descriptor number, returning the filename, absolute path to the file, and the file descriptor containing it
    fn resolve_container(&self, at: usize, path: &str, at_cwd: bool, no_follow: bool) -> common::Result<ResolveResult> {
        match path.chars().next() {
            Some('/') => {
                // parse absolute path
                let root = self.root.lock();
                let (mut path, is_absolute) = self.simplify_path(&root.path, path);
                let name = path.pop().unwrap_or_default();

                if is_absolute {
                    // simplified path resolves from /../
                    drop(root);
                    self.resolve_internal(Box::new(LockedFileDescriptor::new(self.fs_list.clone())), path, None, name, no_follow)
                } else {
                    // simplified path resolves from root
                    let new_path = concat_slices(&root.path, &path);
                    drop(root);
                    self.resolve_internal(Box::new(LockedFileDescriptor::new(self.root.clone())), path, Some(new_path), name, no_follow)
                }
            }
            Some(_) => {
                // parse relative path
                if at_cwd {
                    let cwd = self.cwd.lock();
                    let (mut path, is_absolute) = self.simplify_path(&cwd.path, path);
                    let name = path.pop().unwrap_or_default();

                    if is_absolute {
                        // simplified path resolves from /../
                        drop(cwd);
                        self.resolve_internal(Box::new(LockedFileDescriptor::new(self.fs_list.clone())), path, None, name, no_follow)
                    } else {
                        // simplified path resolves from cwd
                        let new_path = concat_slices(&cwd.path, &path);
                        drop(cwd);
                        self.resolve_internal(Box::new(LockedFileDescriptor::new(self.cwd.clone())), path, Some(new_path), name, no_follow)
                    }
                } else {
                    let file_descriptors = self.file_descriptors.lock();
                    let fd = file_descriptors.get(at).ok_or(Errno::BadFile)?;
                    let (mut path, is_absolute) = self.simplify_path(&fd.path, path);
                    let name = path.pop().unwrap_or_default();

                    if is_absolute {
                        // simplified path resolves from /../
                        drop(file_descriptors);
                        self.resolve_internal(Box::new(LockedFileDescriptor::new(self.fs_list.clone())), path, None, name, no_follow)
                    } else {
                        // simplified path resolves from the given file descriptor
                        let new_path = concat_slices(&fd.path, &path);
                        drop(file_descriptors);
                        self.resolve_internal(Box::new(FDLookup::new(self.file_descriptors.clone(), at)), path, Some(new_path), name, no_follow)
                    }
                }
            }
            None => Err(Errno::InvalidArgument),
        }
    }

    /// implements POSIX openat() with the exception of AT_FDCWD being a flag instead of a dedicated file descriptor
    pub fn open(&self, at: usize, path: &str, flags: common::OpenFlags) -> common::Result<usize> {
        let result = self.resolve_container(
            at,
            path,
            flags & common::OpenFlags::AtCWD != common::OpenFlags::None,
            flags & common::OpenFlags::NoFollow != common::OpenFlags::None,
        )?;

        let open_file = OpenFile {
            descriptor: result.container.open(&result.name, flags & !(common::OpenFlags::CloseOnExec | common::OpenFlags::AtCWD))?,
            path: result.path,
            flags,
        };

        self.file_descriptors.lock().add(open_file).map_err(|_| Errno::OutOfMemory)
    }

    /// implements POSIX read()
    pub fn read(&self, file_descriptor: usize, buf: &mut [u8]) -> common::Result<usize> {
        self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.read(buf)
    }

    /// implements POSIX lseek()
    pub fn seek(&self, file_descriptor: usize, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
        self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.seek(offset, kind)
    }

    /// implements POSIX fstat()
    pub fn stat(&self, file_descriptor: usize) -> common::Result<common::FileStat> {
        self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.stat()
    }

    /// implements POSIX ftruncate() with the exception of length being unsigned instead of signed
    pub fn truncate(&self, file_descriptor: usize, len: u64) -> common::Result<()> {
        self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.truncate(len)
    }

    /// implements POSIX unlinkat() with the exception of AT_FDCWD being a flag instead of a dedicated file descriptor
    pub fn unlink(&self, at: usize, path: &str, flags: common::UnlinkFlags) -> common::Result<()> {
        let result = self.resolve_container(at, path, flags & common::UnlinkFlags::AtCWD != common::UnlinkFlags::None, false)?;
        result.container.unlink(&result.name, flags & !common::UnlinkFlags::AtCWD)
    }

    /// implements POSIX write()
    pub fn write(&self, file_descriptor: usize, buf: &[u8]) -> common::Result<usize> {
        self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.write(buf)
    }

    /// implements POSIX dup()
    pub fn dup(&self, file_descriptor: usize) -> common::Result<usize> {
        let mut new_descriptor = self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.duplicate()?;
        new_descriptor.flags &= !common::OpenFlags::CloseOnExec;
        self.file_descriptors.lock().add(new_descriptor).map_err(|_| Errno::OutOfMemory)
    }

    /// implements POSIX dup2()
    pub fn dup2(&self, file_descriptor: usize, new_fd: usize) -> common::Result<()> {
        if file_descriptor == new_fd {
            Ok(())
        } else {
            let mut new_descriptor = self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?.duplicate()?;
            new_descriptor.flags &= !common::OpenFlags::CloseOnExec;
            self.file_descriptors.lock().set(new_fd, new_descriptor).map_err(|_| Errno::OutOfMemory)
        }
    }

    /// changes the root directory of this environment to the directory pointed to by the given file descriptor,
    /// removing the file descriptor from the list of available file descriptors in the process.
    /// if the file descriptor needs to be kept around it must first be duplicated.
    pub fn chroot(&mut self, file_descriptor: usize) -> common::Result<()> {
        self.root = Arc::new(Mutex::new(self.file_descriptors.lock().remove(file_descriptor).ok_or(Errno::BadFile)?));
        Ok(())
    }

    pub fn exec(&self, file_descriptor: usize) -> common::Result<(crate::mm::ProcessMap, usize)> {
        crate::exec::exec(self.file_descriptors.lock().get(file_descriptor).ok_or(Errno::BadFile)?)
    }
}

fn concat_slices(a: &[String], b: &[String]) -> Vec<String> {
    let mut new_vec = a.to_vec();
    new_vec.reserve_exact(b.len());
    new_vec.extend_from_slice(b);
    new_vec
}

impl Filesystem for FsEnvironment {
    fn get_root_dir(&self) -> Box<dyn FileDescriptor> {
        /*Box::new(NamespaceDir {
            namespace: self.namespace.clone(),
            seek_pos: AtomicUsize::new(0),
        })*/
        self.root.lock().duplicate().expect("couldn't duplicate root directory").descriptor
    }
}

impl Default for FsEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

struct OpenFile {
    descriptor: Box<dyn FileDescriptor>,
    path: Vec<String>,
    flags: common::OpenFlags,
}

impl OpenFile {
    fn duplicate(&self) -> common::Result<Self> {
        Ok(Self {
            descriptor: self.descriptor.dup()?,
            path: self.path.clone(),
            flags: self.flags,
        })
    }
}

impl FileDescriptor for OpenFile {
    fn chmod(&self, permissions: common::Permissions) -> common::Result<()> {
        self.descriptor.chmod(permissions)
    }

    fn chown(&self, owner: Option<common::UserId>, group: Option<common::GroupId>) -> common::Result<()> {
        self.descriptor.chown(owner, group)
    }

    fn open(&self, name: &str, flags: common::OpenFlags) -> common::Result<Box<dyn FileDescriptor>> {
        self.descriptor.open(name, flags)
    }

    fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
        if self.flags & common::OpenFlags::Read != common::OpenFlags::None {
            self.descriptor.read(buf)
        } else {
            Err(Errno::FuncNotSupported)
        }
    }

    fn seek(&self, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
        self.descriptor.seek(offset, kind)
    }

    fn stat(&self) -> common::Result<common::FileStat> {
        self.descriptor.stat()
    }

    fn truncate(&self, len: u64) -> common::Result<()> {
        if self.flags & common::OpenFlags::Write != common::OpenFlags::None {
            self.descriptor.truncate(len)
        } else {
            Err(Errno::ReadOnlyFilesystem)
        }
    }

    fn unlink(&self, name: &str, flags: common::UnlinkFlags) -> common::Result<()> {
        self.descriptor.unlink(name, flags)
    }

    fn write(&self, buf: &[u8]) -> common::Result<usize> {
        if self.flags & common::OpenFlags::Write != common::OpenFlags::None {
            self.descriptor.write(buf)
        } else {
            Err(Errno::ReadOnlyFilesystem)
        }
    }

    fn dup(&self) -> common::Result<Box<dyn FileDescriptor>> {
        self.descriptor.dup()
    }
}

pub trait Filesystem {
    /// gets a unique file descriptor for the root directory of the filesystem
    fn get_root_dir(&self) -> Box<dyn FileDescriptor>;
}

/// the in-kernel interface for a file descriptor
#[allow(unused_variables)]
pub trait FileDescriptor {
    /// changes the access permissions of the file pointed to by this file descriptor
    fn chmod(&self, permissions: common::Permissions) -> common::Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// changes the owner and/or group for the file pointed to by this file descriptor
    fn chown(&self, owner: Option<common::UserId>, group: Option<common::GroupId>) -> common::Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// opens the file with the given name in the directory pointed to by this file descriptor, returning a new file descriptor to the file on success.
    /// the filename must not contain slash characters
    fn open(&self, name: &str, flags: common::OpenFlags) -> common::Result<Box<dyn FileDescriptor>> {
        Err(Errno::FuncNotSupported)
    }

    /// reads data from this file descriptor into the given buffer. upon success, the amount of bytes read is returned.
    ///
    /// if this file descriptor points to a symlink, its target will be read.
    ///
    /// if this file descriptor points to a directory, its entries will be read in order, one per every read() call,
    /// where every directory entry is formatted as its serial number as a native-endian u32 (4 bytes), followed by the bytes of its name with no null terminator.
    /// if a directory entry exceeds the given buffer length, it should be truncated to the buffer length.
    fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
        Err(Errno::FuncNotSupported)
    }

    /// changes the position where writes will occur in this file descriptor, or returns an error if it doesn’t support seeking
    fn seek(&self, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
        Err(Errno::FuncNotSupported)
    }

    /// gets information about the file pointed to by this file descriptor
    fn stat(&self) -> common::Result<common::FileStat>;

    /// shrinks or extends the file pointed to by this file descriptor to the given length
    fn truncate(&self, len: u64) -> common::Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// removes a reference to a file in the directory pointed to by this file descriptor from the filesystem,
    /// where it can then be deleted if no processes are using it or if there are no hard links to it
    fn unlink(&self, name: &str, flags: common::UnlinkFlags) -> common::Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// writes data from this buffer to this file descriptor
    fn write(&self, buf: &[u8]) -> common::Result<usize> {
        Err(Errno::FuncNotSupported)
    }

    /// duplicates this file descriptor
    fn dup(&self) -> common::Result<Box<dyn FileDescriptor>>;
}

pub struct NamespaceDir {
    namespace: Arc<Mutex<BTreeMap<String, Box<dyn Filesystem>>>>,
    seek_pos: AtomicUsize,
}

impl FileDescriptor for NamespaceDir {
    fn open(&self, name: &str, flags: common::OpenFlags) -> common::Result<alloc::boxed::Box<dyn FileDescriptor>> {
        if flags & (common::OpenFlags::Write | common::OpenFlags::Create) != common::OpenFlags::None {
            return Err(Errno::ReadOnlyFilesystem);
        }

        if let Some(filesystem) = self.namespace.lock().get(name) {
            Ok(filesystem.get_root_dir())
        } else {
            Err(common::Errno::NoSuchFileOrDir)
        }
    }

    fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
        let pos = self.seek_pos.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        let namespace = self.namespace.lock();
        let num_keys = namespace.keys().count();

        // TODO: figure out how to do this sensibly
        if let Some(entry) = namespace.keys().nth(pos) {
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
        } else {
            self.seek_pos.store(num_keys, core::sync::atomic::Ordering::SeqCst);
            Ok(0)
        }
    }

    fn seek(&self, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
        seek_helper(&self.seek_pos, offset, kind, self.namespace.lock().keys().count().try_into().map_err(|_| Errno::ValueOverflow)?)
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

    fn dup(&self) -> common::Result<Box<dyn FileDescriptor>> {
        Ok(Box::new(Self {
            namespace: self.namespace.clone(),
            seek_pos: AtomicUsize::new(self.seek_pos.load(core::sync::atomic::Ordering::SeqCst)),
        }))
    }
}

#[allow(clippy::borrowed_box)]
pub fn print_tree(descriptor: &Box<dyn FileDescriptor>) {
    let mut buf = [0_u8; 512];

    fn print_tree_internal(buf: &mut [u8], descriptor: &Box<dyn FileDescriptor>, indent: usize) {
        loop {
            let bytes_read = descriptor.read(buf).expect("failed to read directory entry");
            if bytes_read == 0 {
                break;
            }

            let name = core::str::from_utf8(&buf[4..bytes_read - 1]).expect("invalid utf8").to_string();
            let new_desc = descriptor.open(&name, common::OpenFlags::Read).expect("failed to open file");

            match new_desc.stat().expect("failed to stat file").mode.kind {
                common::FileKind::Directory => {
                    debug!("{:width$}{name}/", "", width = indent);
                    print_tree_internal(buf, &new_desc, indent + 4);
                }
                common::FileKind::SymLink => {
                    let bytes_read = new_desc.read(buf).expect("failed to read symlink target");
                    if bytes_read > 0 {
                        let target = core::str::from_utf8(&buf[..bytes_read]).expect("invalid utf8").to_string();
                        debug!("{:width$}{name} -> {target}", "", width = indent);
                    } else {
                        debug!("{:width$}{name} -> (unknown)", "", width = indent);
                    }
                }
                _ => debug!("{:width$}{name}", "", width = indent),
            }
        }
    }

    print_tree_internal(&mut buf, descriptor, 0);
}

pub fn seek_helper(seek_pos: &AtomicUsize, offset: i64, kind: common::SeekKind, len: i64) -> common::Result<u64> {
    match kind {
        common::SeekKind::Current => match offset.cmp(&0) {
            core::cmp::Ordering::Greater => {
                let val = offset.try_into().map_err(|_| Errno::ValueOverflow)?;
                let old_val = seek_pos.fetch_add(val, core::sync::atomic::Ordering::SeqCst);
                (old_val + val).try_into().map_err(|_| Errno::ValueOverflow)
            }
            core::cmp::Ordering::Less => {
                let val = (-offset).try_into().map_err(|_| Errno::ValueOverflow)?;
                let old_val = seek_pos.fetch_sub(val, core::sync::atomic::Ordering::SeqCst);
                (old_val - val).try_into().map_err(|_| Errno::ValueOverflow)
            }
            core::cmp::Ordering::Equal => seek_pos.load(core::sync::atomic::Ordering::SeqCst).try_into().map_err(|_| Errno::ValueOverflow),
        },
        common::SeekKind::End => {
            let new_val = (len + offset).try_into().map_err(|_| Errno::ValueOverflow)?;
            seek_pos.store(new_val, core::sync::atomic::Ordering::SeqCst);
            new_val.try_into().map_err(|_| Errno::ValueOverflow)
        }
        common::SeekKind::Set => {
            let new_val = offset.try_into().map_err(|_| Errno::ValueOverflow)?;
            seek_pos.store(new_val, core::sync::atomic::Ordering::SeqCst);
            new_val.try_into().map_err(|_| Errno::ValueOverflow)
        }
    }
}

/// manages a FileDescriptor behind a Mutex, locking it automatically when methods are called over it
pub struct LockedFileDescriptor<D: FileDescriptor> {
    pub descriptor: Arc<Mutex<D>>,
}

impl<D: FileDescriptor> LockedFileDescriptor<D> {
    pub fn new(descriptor: Arc<Mutex<D>>) -> Self {
        Self { descriptor }
    }
}

impl<D: FileDescriptor> FileDescriptor for LockedFileDescriptor<D> {
    fn chmod(&self, permissions: common::Permissions) -> common::Result<()> {
        self.descriptor.lock().chmod(permissions)
    }

    fn chown(&self, owner: Option<common::UserId>, group: Option<common::GroupId>) -> common::Result<()> {
        self.descriptor.lock().chown(owner, group)
    }

    fn open(&self, name: &str, flags: common::OpenFlags) -> common::Result<Box<dyn FileDescriptor>> {
        self.descriptor.lock().open(name, flags)
    }

    fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
        self.descriptor.lock().read(buf)
    }

    fn seek(&self, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
        self.descriptor.lock().seek(offset, kind)
    }

    fn stat(&self) -> common::Result<common::FileStat> {
        self.descriptor.lock().stat()
    }

    fn truncate(&self, len: u64) -> common::Result<()> {
        self.descriptor.lock().truncate(len)
    }

    fn unlink(&self, name: &str, flags: common::UnlinkFlags) -> common::Result<()> {
        self.descriptor.lock().unlink(name, flags)
    }

    fn write(&self, buf: &[u8]) -> common::Result<usize> {
        self.descriptor.lock().write(buf)
    }

    fn dup(&self) -> common::Result<Box<dyn FileDescriptor>> {
        Err(Errno::FuncNotSupported)
    }
}

struct FDLookup {
    file_descriptors: Arc<Mutex<ConsistentIndexArray<OpenFile>>>,
    file_descriptor: usize,
}

impl FDLookup {
    fn new(file_descriptors: Arc<Mutex<ConsistentIndexArray<OpenFile>>>, file_descriptor: usize) -> Self {
        Self { file_descriptors, file_descriptor }
    }
}

impl FileDescriptor for FDLookup {
    fn chmod(&self, permissions: common::Permissions) -> common::Result<()> {
        self.file_descriptors.lock().get(self.file_descriptor).ok_or(Errno::BadFile)?.chmod(permissions)
    }

    fn chown(&self, owner: Option<common::UserId>, group: Option<common::GroupId>) -> common::Result<()> {
        self.file_descriptors.lock().get(self.file_descriptor).ok_or(Errno::BadFile)?.chown(owner, group)
    }

    fn open(&self, name: &str, flags: common::OpenFlags) -> common::Result<Box<dyn FileDescriptor>> {
        self.file_descriptors.lock().get(self.file_descriptor).ok_or(Errno::BadFile)?.open(name, flags)
    }

    fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
        self.file_descriptors.lock().get(self.file_descriptor).ok_or(Errno::BadFile)?.read(buf)
    }

    fn seek(&self, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
        self.file_descriptors.lock().get(self.file_descriptor).ok_or(Errno::BadFile)?.seek(offset, kind)
    }

    fn stat(&self) -> common::Result<common::FileStat> {
        self.file_descriptors.lock().get(self.file_descriptor).ok_or(Errno::BadFile)?.stat()
    }

    fn truncate(&self, len: u64) -> common::Result<()> {
        self.file_descriptors.lock().get(self.file_descriptor).ok_or(Errno::BadFile)?.truncate(len)
    }

    fn unlink(&self, name: &str, flags: common::UnlinkFlags) -> common::Result<()> {
        self.file_descriptors.lock().get(self.file_descriptor).ok_or(Errno::BadFile)?.unlink(name, flags)
    }

    fn write(&self, buf: &[u8]) -> common::Result<usize> {
        self.file_descriptors.lock().get(self.file_descriptor).ok_or(Errno::BadFile)?.write(buf)
    }

    fn dup(&self) -> common::Result<Box<dyn FileDescriptor>> {
        Err(Errno::FuncNotSupported)
    }
}
