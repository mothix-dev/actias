use alloc::{boxed::Box, collections::BTreeMap, string::String, sync::Arc};
use log::debug;
use spin::RwLock;

use crate::array::ConsistentIndexArray;

/// contains the filesystem environment of a process (its namespace, its root directory, etc)
#[derive(Clone)]
pub struct FsEnvironment {
    pub namespace: Arc<RwLock<BTreeMap<String, Box<dyn Filesystem>>>>,
    //pub cwd: Arc<Mutex<Box<dyn Filesystem>>>,
    //pub root: Arc<Mutex<Box<dyn Filesystem>>>,
    pub file_descriptors: Arc<RwLock<ConsistentIndexArray<Box<dyn FileDescriptor>>>>,
}

impl FsEnvironment {
    pub fn new(/*cwd: Box<dyn Filesystem>, root: Box<dyn Filesystem>*/) -> Self {
        Self {
            namespace: Arc::new(RwLock::new(BTreeMap::new())),
            //cwd: Arc::new(Mutex::new(cwd)),
            //root: Arc::new(Mutex::new(root)),
            file_descriptors: Arc::new(RwLock::new(ConsistentIndexArray::new())),
        }
    }
}

pub trait Filesystem {
    /// gets a unique file descriptor for the root directory of the filesystem
    fn get_root_dir(&self) -> Box<dyn FileDescriptor>;
}

/// the in-kernel interface for a file descriptor
pub trait FileDescriptor {
    /// changes the access permissions of the file pointed to by this file descriptor
    fn chmod(&self, permissions: common::Permissions) -> common::Result<()>;

    /// changes the owner and/or group for the file pointed to by this file descriptor
    fn chown(&self, owner: Option<common::UserId>, group: Option<common::GroupId>) -> common::Result<()>;

    /// creates a hard (non-symbolic) link to a file in the same filesystem pointed to by `source`.
    /// the file pointed to by this file descriptor will be replaced with the file pointed to by `source` in the filesystem,
    /// however this open file descriptor will still point to the inode that existed previously.
    fn link(&self, source: &dyn FileDescriptor) -> common::Result<()>;

    /// opens the file with the given name in the directory pointed to by this file descriptor, returning a new file descriptor to the file on success.
    /// the filename must not contain slash characters
    fn open(&self, name: &str, flags: common::OpenFlags) -> common::Result<Box<dyn FileDescriptor>>;

    /// reads data from this file descriptor into the given buffer. upon success, the amount of bytes read is returned.
    ///
    /// if this file descriptor points to a symlink, its target will be read.
    /// if this file descriptor points to a directory, its entries will be read in order.
    fn read(&self, buf: &mut [u8]) -> common::Result<usize>;

    /// changes the position where writes will occur in this file descriptor, or returns an error if it doesn’t support seeking
    fn seek(&self, offset: isize, kind: common::SeekKind) -> common::Result<usize>;

    /// gets information about the file pointed to by this file descriptor
    fn stat(&self) -> common::Result<common::FileStat>;

    /// shrinks or extends the file pointed to by this file descriptor to the given length
    fn truncate(&self, len: usize) -> common::Result<()>;

    /// removes a reference to a file from the filesystem, where it can then be deleted if no processes are using it or if there are no hard links to it
    fn unlink(&self) -> common::Result<()>;

    /// writes data from this buffer to this file descriptor
    fn write(&self, buf: &[u8]) -> common::Result<usize>;
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
