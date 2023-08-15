//! kernel-space filesystems

use super::{HandleNum, Request, RequestCallback};
use crate::{
    arch::{PhysicalAddress, PROPERTIES},
    array::ConsistentIndexArray,
};
use alloc::{boxed::Box, string::String, sync::Arc};
use common::{Errno, FileStat, GroupId, OpenFlags, Permissions, Result, UnlinkFlags, UserId};
use spin::Mutex;

pub struct KernelFs {
    file_handles: Mutex<ConsistentIndexArray<Arc<dyn FileDescriptor>>>,
}

// its literally in a mutex!!
unsafe impl Send for KernelFs {}
unsafe impl Sync for KernelFs {}

impl KernelFs {
    pub fn new(root: Arc<dyn FileDescriptor>) -> Self {
        let mut file_handles = ConsistentIndexArray::new();
        file_handles.set(0, root).unwrap();

        Self {
            file_handles: Mutex::new(file_handles),
        }
    }
}

impl super::Filesystem for KernelFs {
    fn get_root_dir(&self) -> HandleNum {
        0
    }

    fn make_request(&self, handle: HandleNum, request: Request) {
        let descriptor = match self.file_handles.lock().get(handle) {
            Some(descriptor) => descriptor.clone(),
            None => return request.callback_error(Errno::BadFile, false),
        };

        match request {
            Request::Chmod { permissions, callback } => {
                let res = descriptor.chmod(permissions);
                callback(res, false);
            }
            Request::Chown { owner, group, callback } => {
                let res = descriptor.chown(owner, group);
                callback(res, false);
            }
            Request::Close => {
                if handle != 0 {
                    self.file_handles.lock().remove(handle);
                }
            }
            Request::Open { name, flags, callback } => {
                let res = descriptor.open(name, flags).and_then(|desc| self.file_handles.lock().add(desc).map_err(|_| Errno::OutOfMemory));
                callback(res, false);
            }
            Request::Read { position, length, callback } => descriptor.read(position, length, callback),
            Request::Stat { callback } => {
                let res = descriptor.stat();
                callback(res, false);
            }
            Request::Truncate { length, callback } => {
                let res = descriptor.truncate(length);
                callback(res, false);
            }
            Request::Unlink { name, flags, callback } => {
                let res = descriptor.unlink(name, flags);
                callback(res, false);
            }
            Request::Write { length, position, callback } => descriptor.write(position, length, callback),
        }
    }

    fn get_page(&self, handle: HandleNum, position: i64, callback: Box<dyn FnOnce(Option<PhysicalAddress>, bool)>) {
        match self.file_handles.lock().get(handle) {
            Some(descriptor) => descriptor.get_page(position, callback),
            None => callback(None, false),
        };
    }
}

#[allow(unused_variables)]
pub trait FileDescriptor: Send + Sync {
    /// changes the access permissions of the file pointed to by this file descriptor
    fn chmod(&self, permissions: Permissions) -> Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// changes the owner and/or group for the file pointed to by this file descriptor
    fn chown(&self, owner: UserId, group: GroupId) -> Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// opens the file with the given name in the directory pointed to by this file descriptor, returning a new file descriptor to the file on success.
    /// the filename must not contain slash characters
    fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn FileDescriptor>> {
        Err(Errno::FuncNotSupported)
    }

    /// reads data from this file descriptor.
    ///
    /// if this file descriptor points to a symlink, its target will be read.
    ///
    /// if this file descriptor points to a directory, its entries will be read in order, one per every read() call,
    /// where every directory entry is formatted as its serial number as a native-endian u32 (4 bytes), followed by the bytes of its name with no null terminator.
    /// if a directory entry exceeds the given buffer length, it should be truncated to the buffer length.
    fn read(&self, position: i64, length: usize, callback: Box<dyn for<'a> RequestCallback<&'a [u8]>>) {
        callback(Err(Errno::FuncNotSupported), false);
    }

    /// gets information about the file pointed to by this file descriptor
    fn stat(&self) -> Result<FileStat>;

    /// shrinks or extends the file pointed to by this file descriptor to the given length
    fn truncate(&self, length: i64) -> Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// removes a reference to a file in the directory pointed to by this file descriptor from the filesystem,
    /// where it can then be deleted if no processes are using it or if there are no hard links to it
    fn unlink(&self, name: String, flags: UnlinkFlags) -> Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// writes data from this buffer to this file descriptor
    fn write(&self, position: i64, length: usize, callback: Box<dyn for<'a> RequestCallback<&'a mut [u8]>>) {
        callback(Err(Errno::FuncNotSupported), false);
    }

    /// see `Filesystem::get_page`
    fn get_page(&self, position: i64, callback: Box<dyn FnOnce(Option<PhysicalAddress>, bool)>) {
        let phys_addr = match crate::get_global_state().page_manager.lock().alloc_frame(None) {
            Ok(phys_addr) => phys_addr,
            Err(_) => return callback(None, false),
        };

        self.read(
            position,
            PROPERTIES.page_size,
            Box::new(move |res, blocked| {
                let slice = match res {
                    Ok(slice) => slice,
                    Err(_) => return callback(None, blocked),
                };

                let mut page_directory = crate::mm::LockedPageDir(crate::get_global_state().page_directory.clone());
                unsafe {
                    match crate::mm::map_memory(&mut page_directory, &[phys_addr], |page_slice| {
                        page_slice[..slice.len()].copy_from_slice(slice);
                        page_slice[slice.len()..].fill(0);
                    }) {
                        Ok(_) => callback(Some(phys_addr), blocked),
                        Err(_) => callback(None, blocked),
                    }
                }
            }),
        );
    }
}
