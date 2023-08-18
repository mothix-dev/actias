//! kernel-space filesystems

use super::HandleNum;
use crate::{arch::PhysicalAddress, array::ConsistentIndexArray, process::Buffer};
use alloc::{boxed::Box, string::String, sync::Arc};
use async_trait::async_trait;
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

#[async_trait]
impl super::Filesystem for KernelFs {
    fn get_root_dir(&self) -> HandleNum {
        0
    }

    async fn chmod(&self, handle: HandleNum, permissions: Permissions) -> Result<()> {
        let descriptor = self.file_handles.lock().get(handle).ok_or(Errno::TryAgain)?.clone();
        descriptor.chmod(permissions).await
    }

    async fn chown(&self, handle: HandleNum, owner: UserId, group: GroupId) -> Result<()> {
        let descriptor = self.file_handles.lock().get(handle).ok_or(Errno::TryAgain)?.clone();
        descriptor.chown(owner, group).await
    }

    async fn close(&self, handle: HandleNum) {
        if handle != 0 {
            self.file_handles.lock().remove(handle);
        }
    }

    async fn open(&self, handle: HandleNum, name: String, flags: OpenFlags) -> Result<HandleNum> {
        let descriptor = self.file_handles.lock().get(handle).ok_or(Errno::TryAgain)?.clone();
        descriptor.open(name, flags).await.and_then(|desc| self.file_handles.lock().add(desc).map_err(|_| Errno::OutOfMemory))
    }

    async fn read(&self, handle: HandleNum, position: i64, buffer: Buffer) -> Result<usize> {
        let descriptor = self.file_handles.lock().get(handle).ok_or(Errno::TryAgain)?.clone();
        descriptor.read(position, buffer).await
    }

    async fn stat(&self, handle: HandleNum) -> Result<FileStat> {
        let descriptor = self.file_handles.lock().get(handle).ok_or(Errno::TryAgain)?.clone();
        descriptor.stat().await
    }

    async fn truncate(&self, handle: HandleNum, length: i64) -> Result<()> {
        let descriptor = self.file_handles.lock().get(handle).ok_or(Errno::TryAgain)?.clone();
        descriptor.truncate(length).await
    }

    async fn unlink(&self, handle: HandleNum, name: String, flags: UnlinkFlags) -> Result<()> {
        let descriptor = self.file_handles.lock().get(handle).ok_or(Errno::TryAgain)?.clone();
        descriptor.unlink(name, flags).await
    }

    async fn write(&self, handle: HandleNum, position: i64, buffer: Buffer) -> Result<usize> {
        let descriptor = self.file_handles.lock().get(handle).ok_or(Errno::TryAgain)?.clone();
        descriptor.write(position, buffer).await
    }

    async fn get_page(&self, handle: HandleNum, position: i64) -> Option<PhysicalAddress> {
        let descriptor = self.file_handles.lock().get(handle)?.clone();
        descriptor.get_page(position).await
    }
}

#[allow(unused_variables)]
#[async_trait]
pub trait FileDescriptor: Send + Sync {
    /// changes the access permissions of the file pointed to by this file descriptor
    async fn chmod(&self, permissions: Permissions) -> Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// changes the owner and/or group for the file pointed to by this file descriptor
    async fn chown(&self, owner: UserId, group: GroupId) -> Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// opens the file with the given name in the directory pointed to by this file descriptor, returning a new file descriptor to the file on success.
    /// the filename must not contain slash characters
    async fn open(&self, name: String, flags: OpenFlags) -> Result<Arc<dyn FileDescriptor>> {
        Err(Errno::FuncNotSupported)
    }

    /// reads data from this file descriptor.
    ///
    /// if this file descriptor points to a symlink, its target will be read.
    ///
    /// if this file descriptor points to a directory, its entries will be read in order, one per every read() call,
    /// where every directory entry is formatted as its serial number as a native-endian u32 (4 bytes), followed by the bytes of its name with no null terminator.
    /// if a directory entry exceeds the given buffer length, it should be truncated to the buffer length.
    async fn read(&self, position: i64, buffer: Buffer) -> Result<usize> {
        Err(Errno::FuncNotSupported)
    }

    /// gets information about the file pointed to by this file descriptor
    async fn stat(&self) -> Result<FileStat>;

    /// shrinks or extends the file pointed to by this file descriptor to the given length
    async fn truncate(&self, length: i64) -> Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// removes a reference to a file in the directory pointed to by this file descriptor from the filesystem,
    /// where it can then be deleted if no processes are using it or if there are no hard links to it
    async fn unlink(&self, name: String, flags: UnlinkFlags) -> Result<()> {
        Err(Errno::FuncNotSupported)
    }

    /// writes data from this buffer to this file descriptor
    async fn write(&self, position: i64, buffer: Buffer) -> Result<usize> {
        Err(Errno::FuncNotSupported)
    }

    /// see `Filesystem::get_page`
    async fn get_page(&self, position: i64) -> Option<PhysicalAddress> {
        let phys_addr = crate::get_global_state().page_manager.lock().alloc_frame(None).ok()?;
        self.read(position, Buffer::Page(phys_addr)).await.ok()?;
        Some(phys_addr)
    }
}
