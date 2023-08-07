pub mod sys;
pub mod tar;

use core::sync::atomic::AtomicUsize;

use crate::array::ConsistentIndexArray;
use alloc::{boxed::Box, collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use log::debug;
use spin::Mutex;

/// contains the filesystem environment of a process (its namespace, its root directory, etc)
#[derive(Clone)]
pub struct FsEnvironment {
    pub namespace: Arc<Mutex<BTreeMap<String, Box<dyn Filesystem>>>>,
    pub cwd: Arc<Mutex<Box<dyn FileDescriptor>>>,
    pub root: Arc<Mutex<Box<dyn FileDescriptor>>>,
    pub file_descriptors: Arc<Mutex<ConsistentIndexArray<Box<dyn FileDescriptor>>>>,
}

impl FsEnvironment {
    pub fn new() -> Self {
        let namespace = Arc::new(Mutex::new(BTreeMap::new()));
        let root: Arc<Mutex<Box<dyn FileDescriptor>>> = Arc::new(Mutex::new(Box::new(NamespaceDir {
            namespace: namespace.clone(),
            seek_pos: AtomicUsize::new(0),
        })));
        Self {
            namespace,
            cwd: root.clone(),
            root,
            file_descriptors: Arc::new(Mutex::new(ConsistentIndexArray::new())),
        }
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
    fn seek(&self, offset: isize, kind: common::SeekKind) -> common::Result<usize> {
        Err(common::Error::InvalidOperation)
    }

    /// gets information about the file pointed to by this file descriptor
    fn stat(&self) -> common::Result<common::FileStat>;

    /// shrinks or extends the file pointed to by this file descriptor to the given length
    fn truncate(&self, len: usize) -> common::Result<()> {
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
        } else {
            self.seek_pos.store(num_keys, core::sync::atomic::Ordering::SeqCst);
            Ok(0)
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
            },
            common::SeekKind::End => {
                let len: isize = self.namespace.lock().keys().count().try_into().map_err(|_| common::Error::Overflow)?;
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

#[allow(clippy::borrowed_box)]
pub fn print_tree(descriptor: &Box<dyn FileDescriptor>) {
    let mut buf = [0_u8; 512];

    fn print_tree_internal(buf: &mut [u8], descriptor: &Box<dyn FileDescriptor>, indent: usize) {
        loop {
            let bytes_read = descriptor.read(buf).expect("failed to read directory entry");
            if bytes_read == 0 {
                break;
            }

            let name = core::str::from_utf8(&buf[8..bytes_read - 1]).expect("invalid utf8");

            if let Ok(new_desc) = descriptor.open(name, common::OpenFlags::Read | common::OpenFlags::Directory) {
                debug!("{:width$}{name}/", "", width = indent);
                print_tree_internal(buf, &new_desc, indent + 4);
            } else {
                debug!("{:width$}{name}", "", width = indent);
            }
        }
    }

    print_tree_internal(&mut buf, descriptor, 0);
}
