pub mod sys;
pub mod tar;

use crate::array::ConsistentIndexArray;
use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use core::sync::atomic::AtomicUsize;
use log::debug;
use spin::Mutex;

/// contains the filesystem environment of a process (its namespace, its root directory, etc)
#[derive(Clone)]
pub struct FsEnvironment {
    pub namespace: Arc<Mutex<BTreeMap<String, Box<dyn Filesystem>>>>,
    cwd: Arc<Mutex<OpenFile>>,
    root: Arc<Mutex<OpenFile>>,
    file_descriptors: Arc<Mutex<ConsistentIndexArray<OpenFile>>>,
}

impl FsEnvironment {
    pub fn new() -> Self {
        let namespace = Arc::new(Mutex::new(BTreeMap::new()));
        let root = Arc::new(Mutex::new(OpenFile {
            descriptor: Box::new(NamespaceDir {
                namespace: namespace.clone(),
                seek_pos: AtomicUsize::new(0),
            }),
            path: vec!["..".to_string()],
            close_on_exec: false,
        }));
        Self {
            namespace,
            cwd: root.clone(),
            root,
            file_descriptors: Arc::new(Mutex::new(ConsistentIndexArray::new())),
        }
    }

    pub fn chmod(&self, file_descriptor: usize, permissions: common::Permissions) -> common::Result<()> {
        self.file_descriptors.lock().get(file_descriptor).ok_or(common::Error::BadFileDescriptor)?.descriptor.chmod(permissions)
    }

    pub fn chown(&self, file_descriptor: usize, owner: Option<common::UserId>, group: Option<common::GroupId>) -> common::Result<()> {
        self.file_descriptors
            .lock()
            .get(file_descriptor)
            .ok_or(common::Error::BadFileDescriptor)?
            .descriptor
            .chown(owner, group)
    }

    pub fn close(&self, file_descriptor: usize) {
        self.file_descriptors.lock().remove(file_descriptor)
    }

    pub fn link(&self, source: usize, target: usize) -> common::Result<()> {
        let file_descriptors = self.file_descriptors.lock();
        let source = &*file_descriptors.get(source).ok_or(common::Error::BadFileDescriptor)?.descriptor;
        file_descriptors.get(target).ok_or(common::Error::BadFileDescriptor)?.descriptor.link(source)
    }

    /// parses a path, removing any . or .. elements, and detects whether the new path is relative or absolute
    fn remove_dots(&self, mut container_path: Option<&[String]>, path: &str) -> (Vec<String>, bool) {
        let mut path_stack = Vec::new();
        let mut is_absolute = false;

        for component in path.split('/') {
            match component {
                "." => (),
                ".." => if path_stack.pop().is_none() && let Some(path) = container_path.take() {
                    is_absolute = true;
                    path_stack.extend_from_slice(path);
                }
                _ => path_stack.push(component.to_string()),
            }
        }

        (path_stack, is_absolute)
    }

    /// iterates path elements, double checking permissions and resolving symlinks, then opens the requested file
    fn open_internal(&self, at: &OpenFile, path: Vec<String>, flags: common::OpenFlags, absolute: bool) -> common::Result<usize> {
        let mut last_fd: Option<Box<dyn FileDescriptor>> = None;
        let mut buf = [0_u8; 512];

        let mut split = path.iter();

        if let Some(first) = path.first() && first == ".." {
            split.next();
            last_fd = Some(Box::new(NamespaceDir {
                namespace: self.namespace.clone(),
                seek_pos: AtomicUsize::new(0),
            }));
        }

        while let Some(component) = split.next() {
            let new_desc = match last_fd.as_ref() {
                Some(dir) => dir.open(component, common::OpenFlags::Read)?,
                None => at.descriptor.open(component, common::OpenFlags::Read)?,
            };

            let stat = new_desc.stat()?;
            // TODO: check permissions, allow opening directories

            match stat.mode.kind {
                common::FileKind::Directory => last_fd = Some(new_desc),
                common::FileKind::SymLink => {
                    let bytes_read = new_desc.read(&mut buf)?;
                    if bytes_read > 0 {
                        let target = core::str::from_utf8(&buf[..bytes_read]).map_err(|_| common::Error::BadInput)?.to_string();
                        // TODO: resolve symlink
                        debug!("link to {target:?}");
                        todo!();
                    } else {
                        return Err(common::Error::BadInput);
                    }
                }
                _ => {
                    if split.next().is_some() {
                        return Err(common::Error::NotDirectory);
                    }

                    let open_file = OpenFile {
                        descriptor: match last_fd {
                            Some(dir) => dir.open(component, flags & !common::OpenFlags::CloseOnExec)?,
                            None => at.descriptor.open(component, flags & !common::OpenFlags::CloseOnExec)?,
                        },
                        path: if absolute {
                            path
                        } else {
                            let mut new_path = Vec::with_capacity(at.path.len() + path.len());
                            new_path.extend_from_slice(&at.path);
                            new_path.extend_from_slice(&path);
                            new_path
                        },
                        close_on_exec: flags & common::OpenFlags::CloseOnExec != common::OpenFlags::None,
                    };

                    debug!("full path is {:?}", open_file.path);

                    return self.file_descriptors.lock().add(open_file).map_err(|_| common::Error::AllocError);
                }
            }
        }

        Err(common::Error::InvalidOperation)
    }

    pub fn open(&self, at: usize, path: &str, flags: common::OpenFlags) -> common::Result<usize> {
        match path.chars().next() {
            Some('/') => self.open_internal(&self.root.lock(), self.remove_dots(None, path).0, flags, true), // parse absolute path
            Some(_) => {
                // parse relative path
                if flags & common::OpenFlags::AtCWD != common::OpenFlags::None {
                    let cwd = self.cwd.lock();
                    let (path, is_absolute) = self.remove_dots(Some(&cwd.path), path);

                    if is_absolute {
                        drop(cwd); // doesn't hurt to release the lock here
                        self.open_internal(&self.root.lock(), path, flags & !common::OpenFlags::AtCWD, true)
                    } else {
                        self.open_internal(&cwd, path, flags & !common::OpenFlags::AtCWD, false)
                    }
                } else {
                    let file_descriptors = self.file_descriptors.lock();
                    let at = file_descriptors.get(at).ok_or(common::Error::BadFileDescriptor)?;
                    let (path, is_absolute) = self.remove_dots(Some(&at.path), path);

                    if is_absolute {
                        drop(file_descriptors);
                        self.open_internal(&self.root.lock(), path, flags, true)
                    } else {
                        self.open_internal(at, path, flags, false)
                    }
                }
            }
            None => Err(common::Error::BadInput),
        }
    }

    pub fn read(&self, file_descriptor: usize, buf: &mut [u8]) -> common::Result<usize> {
        self.file_descriptors.lock().get(file_descriptor).ok_or(common::Error::BadFileDescriptor)?.descriptor.read(buf)
    }

    pub fn seek(&self, file_descriptor: usize, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
        self.file_descriptors.lock().get(file_descriptor).ok_or(common::Error::BadFileDescriptor)?.descriptor.seek(offset, kind)
    }

    pub fn stat(&self, file_descriptor: usize) -> common::Result<common::FileStat> {
        self.file_descriptors.lock().get(file_descriptor).ok_or(common::Error::BadFileDescriptor)?.descriptor.stat()
    }

    pub fn truncate(&self, file_descriptor: usize, len: u64) -> common::Result<()> {
        self.file_descriptors.lock().get(file_descriptor).ok_or(common::Error::BadFileDescriptor)?.descriptor.truncate(len)
    }

    pub fn unlink(&self, file_descriptor: usize) -> common::Result<()> {
        self.file_descriptors.lock().get(file_descriptor).ok_or(common::Error::BadFileDescriptor)?.descriptor.unlink()
    }

    pub fn write(&self, file_descriptor: usize, buf: &[u8]) -> common::Result<usize> {
        self.file_descriptors.lock().get(file_descriptor).ok_or(common::Error::BadFileDescriptor)?.descriptor.write(buf)
    }
}

impl Filesystem for FsEnvironment {
    fn get_root_dir(&self) -> Box<dyn FileDescriptor> {
        Box::new(NamespaceDir {
            namespace: self.namespace.clone(),
            seek_pos: AtomicUsize::new(0),
        })
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
    close_on_exec: bool,
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
        Err(common::Error::InvalidOperation)
    }

    /// changes the owner and/or group for the file pointed to by this file descriptor
    fn chown(&self, owner: Option<common::UserId>, group: Option<common::GroupId>) -> common::Result<()> {
        Err(common::Error::InvalidOperation)
    }

    /// creates a hard (non-symbolic) link to a file in the same filesystem pointed to by `source`.
    /// the file pointed to by this file descriptor will be replaced with the file pointed to by `source` in the filesystem,
    /// however this open file descriptor will still point to the inode that existed previously.
    fn link(&self, source: &dyn FileDescriptor) -> common::Result<()> {
        Err(common::Error::InvalidOperation)
    }

    /// opens the file with the given name in the directory pointed to by this file descriptor, returning a new file descriptor to the file on success.
    /// the filename must not contain slash characters
    fn open(&self, name: &str, flags: common::OpenFlags) -> common::Result<Box<dyn FileDescriptor>> {
        Err(common::Error::InvalidOperation)
    }

    /// reads data from this file descriptor into the given buffer. upon success, the amount of bytes read is returned.
    ///
    /// if this file descriptor points to a symlink, its target will be read.
    /// if this file descriptor points to a directory, its entries will be read in order.
    fn read(&self, buf: &mut [u8]) -> common::Result<usize> {
        Err(common::Error::InvalidOperation)
    }

    /// changes the position where writes will occur in this file descriptor, or returns an error if it doesn’t support seeking
    fn seek(&self, offset: i64, kind: common::SeekKind) -> common::Result<u64> {
        Err(common::Error::InvalidOperation)
    }

    /// gets information about the file pointed to by this file descriptor
    fn stat(&self) -> common::Result<common::FileStat>;

    /// shrinks or extends the file pointed to by this file descriptor to the given length
    fn truncate(&self, len: u64) -> common::Result<()> {
        Err(common::Error::InvalidOperation)
    }

    /// removes a reference to a file from the filesystem, where it can then be deleted if no processes are using it or if there are no hard links to it
    fn unlink(&self) -> common::Result<()> {
        Err(common::Error::InvalidOperation)
    }

    /// writes data from this buffer to this file descriptor
    fn write(&self, buf: &[u8]) -> common::Result<usize> {
        Err(common::Error::InvalidOperation)
    }
}

pub struct NamespaceDir {
    namespace: Arc<Mutex<BTreeMap<String, Box<dyn Filesystem>>>>,
    seek_pos: AtomicUsize,
}

impl FileDescriptor for NamespaceDir {
    fn open(&self, name: &str, flags: common::OpenFlags) -> common::Result<alloc::boxed::Box<dyn FileDescriptor>> {
        if flags & (common::OpenFlags::Write | common::OpenFlags::Create) != common::OpenFlags::None {
            return Err(common::Error::ReadOnly);
        }

        if let Some(filesystem) = self.namespace.lock().get(name) {
            Ok(filesystem.get_root_dir())
        } else {
            Err(common::Error::DoesntExist)
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
        seek_helper(&self.seek_pos, offset, kind, self.namespace.lock().keys().count().try_into().map_err(|_| common::Error::Overflow)?)
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
                let val = offset.try_into().map_err(|_| common::Error::Overflow)?;
                let old_val = seek_pos.fetch_add(val, core::sync::atomic::Ordering::SeqCst);
                (old_val + val).try_into().map_err(|_| common::Error::Overflow)
            }
            core::cmp::Ordering::Less => {
                let val = (-offset).try_into().map_err(|_| common::Error::Overflow)?;
                let old_val = seek_pos.fetch_sub(val, core::sync::atomic::Ordering::SeqCst);
                (old_val - val).try_into().map_err(|_| common::Error::Overflow)
            }
            core::cmp::Ordering::Equal => seek_pos.load(core::sync::atomic::Ordering::SeqCst).try_into().map_err(|_| common::Error::Overflow),
        },
        common::SeekKind::End => {
            let new_val = (len + offset).try_into().map_err(|_| common::Error::Overflow)?;
            seek_pos.store(new_val, core::sync::atomic::Ordering::SeqCst);
            new_val.try_into().map_err(|_| common::Error::Overflow)
        }
        common::SeekKind::Set => {
            let new_val = offset.try_into().map_err(|_| common::Error::Overflow)?;
            seek_pos.store(new_val, core::sync::atomic::Ordering::SeqCst);
            new_val.try_into().map_err(|_| common::Error::Overflow)
        }
    }
}
