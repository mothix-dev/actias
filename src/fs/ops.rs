//! operations on files with file descriptors

use alloc::{
    vec::Vec,
    boxed::Box,
    string::{String, ToString},
};
use crate::errno::Errno;
use super::{
    tree::{File, get_file_from_path},
    vfs::{Permissions, ROOT_DIR},
    MAX_FILES,
};
use crate::util::array::VecBitSet;
use core::ops::Drop;

/// list of open files
static mut OPEN_FILES: Vec<Option<OpenFile>> = Vec::new();

/// bitset of available system file descriptors
static mut FILE_DESCRIPTOR_BITSET: VecBitSet = VecBitSet::new();

/// stores information about an open file
pub struct OpenFile<'a> {
    /// file descriptor number
    pub descriptor: usize,

    /// reference to file
    pub file: &'a mut Box<dyn File>,

    /// absolute path to file
    pub path: String,


}

/// opens a file for writing
pub fn open(path: &str) -> Result<FileDescriptor, Errno> {
    // TODO: modes

    let file = match get_file_from_path(unsafe { ROOT_DIR.as_mut().expect("file system not initialized") }, path) {
        Some(file) => file,
        None => return Err(Errno::NoSuchFileOrDir),
    };

    let descriptor = unsafe { FILE_DESCRIPTOR_BITSET.first_unset() };

    if descriptor >= MAX_FILES {
        Err(Errno::TooManyFilesOpen)
    } else {
        unsafe { FILE_DESCRIPTOR_BITSET.set(descriptor); }

        let open = OpenFile {
            descriptor,
            file,
            path: path.to_string(),
        };

        unsafe { OPEN_FILES[descriptor] = Some(open); }

        Ok(FileDescriptor::new(descriptor))
    }
}

/// closes a file given its descriptor number
pub fn close_file(descriptor: usize) {
    unsafe {
        FILE_DESCRIPTOR_BITSET.clear(descriptor);
        OPEN_FILES[descriptor] = None;
    }
}

/// closes a file descriptor
pub fn close(file: &mut FileDescriptor) {
    close_file(file.index);
    file.valid = false;
}

/// controls how FileDescriptor::seek() seeks
pub enum SeekType {
    /// set file writing offset to provided offset
    Set,

    /// add the provided offset to the current file offset
    Current,

    /// set the file offset to the end of the file plus the provided offset
    End,
}

/// file descriptor- contains a numbered reference to a file
pub struct FileDescriptor {
    /// index of this file descriptor into the file descriptor vec
    index: usize,

    /// offset for writing into the file
    pub offset: usize,

    /// whether this file descriptor is valid or not
    valid: bool,
}

impl FileDescriptor {
    /// creates a new file descriptor from a file descriptor id
    fn new(index: usize) -> Self {
        Self {
            index,
            offset: 0,
            valid: true,
        }
    }

    /// get reference to our file
    fn get_reference(&self) -> Option<&OpenFile<'static>> {
        if self.valid {
            unsafe { OPEN_FILES.get(self.index)?.as_ref() }
        } else {
            None
        }
    }

    /// get mutable reference to our file
    fn get_mut_reference(&self) -> Option<&mut OpenFile<'static>> {
        if self.valid {
            unsafe { OPEN_FILES.get_mut(self.index)?.as_mut() }
        } else {
            None
        }
    }


    /// get permissions for file
    pub fn get_permissions(&mut self) -> Result<Permissions, Errno> {
        match self.get_reference() {
            Some(file) => Ok(file.file.get_permissions()),
            None => Err(Errno::BadFile),
        }
    }

    /// set permissions for file
    pub fn set_permissions(&mut self, permissions: Permissions) -> Result<(), Errno> {
        match self.get_mut_reference() {
            Some(file) => file.file.set_permissions(permissions),
            None => Err(Errno::BadFile),
        }
    }


    /// write all bytes contained in slice to file
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize, Errno> {
        match self.get_mut_reference() {
            Some(file) => {
                let amt = file.file.write_at(bytes, self.offset)?;
                self.offset += amt;
                Ok(amt)
            },
            None => Err(Errno::BadFile),
        }
    }

    /// write all bytes contained in slice to file at offset
    pub fn write_at(&mut self, bytes: &[u8], offset: usize) -> Result<usize, Errno> {
        match self.get_mut_reference() {
            Some(file) => file.file.write_at(bytes, offset),
            None => Err(Errno::BadFile),
        }
    }

    /// checks if there's enough room to write the provided amount of bytes into the file
    pub fn can_write(&mut self, space: usize) -> bool {
        match self.get_reference() {
            Some(file) => file.file.can_write_at(space, self.offset),
            None => false,
        }
    }

    /// checks if there's enough room to write the provided amount of bytes into the file at the provided offset
    pub fn can_write_at(&mut self, space: usize, offset: usize) -> bool {
        match self.get_reference() {
            Some(file) => file.file.can_write_at(space, offset),
            None => false,
        }
    }


    /// read from file into provided slice
    pub fn read(&mut self, bytes: &mut [u8]) -> Result<usize, Errno> {
        match self.get_mut_reference() {
            Some(file) => {
                let amt = file.file.read_at(bytes, self.offset)?;
                self.offset += amt;
                Ok(amt)
            },
            None => Err(Errno::BadFile),
        }
    }

    /// read from file at offset into provided slice
    pub fn read_at(&mut self, bytes: &mut [u8], offset: usize) -> Result<usize, Errno> {
        match self.get_reference() {
            Some(file) => file.file.read_at(bytes, offset),
            None => Err(Errno::BadFile),
        }
    }

    /// checks if there's enough room to read the provided amount of bytes from the file
    pub fn can_read(&mut self, space: usize) -> bool {
        match self.get_reference() {
            Some(file) => file.file.can_read_at(space, self.offset),
            None => false,
        }
    }

    /// checks if there's enough room to read the provided amount of bytes from the file at the provided offset
    pub fn can_read_at(&mut self, space: usize, offset: usize) -> bool {
        match self.get_reference() {
            Some(file) => file.file.can_read_at(space, offset),
            None => false,
        }
    }


    /// seek file
    /// seek behavior depends on the SeekType provided
    pub fn seek(&mut self, offset: isize, kind: SeekType) -> Result<usize, Errno> {
        match self.get_reference() {
            Some(file) => {
                let size = file.file.get_size();

                match kind {
                    SeekType::Set => self.offset = offset as usize,
                    SeekType::Current => {
                        if offset > 0 {
                            self.offset = self.offset.wrapping_add(offset as usize); // we can wrap since if it goes below zero it'll be bigger than the file size, and thus fail
                        } else {
                            self.offset = self.offset.wrapping_sub((-offset) as usize);
                        }
                    },
                    SeekType::End => {
                        if offset > 0 {
                            return Err(Errno::InvalidSeek);
                        } else {
                            self.offset = size.wrapping_sub((-offset) as usize);
                        }
                    },
                }

                if self.offset > size {
                    Err(Errno::InvalidSeek)
                } else {
                    Ok(self.offset)
                }
            },
            None => Err(Errno::BadFile),
        }
    }


    /// truncate file, setting its size to the provided size
    pub fn truncate(&mut self, size: usize) -> Result<(), Errno> {
        match self.get_mut_reference() {
            Some(file) => file.file.truncate(size),
            None => Err(Errno::BadFile),
        }
    }


    /// lock file
    /// lock behavior depends on the LockKind provided
    /*pub fn lock(&mut self, kind: LockKind, size: isize) -> Result<(), Errno> {
        match self.get_mut_reference() {
            Some(file) => file.file.lock(kind, size),
            None => Err(Errno::BadFile),
        }
    }*/


    /// gets name of file
    pub fn get_name(&mut self) -> &str {
        match self.get_reference() {
            Some(file) => file.file.get_name(),
            None => "",
        }
    }

    /// sets name of file
    pub fn set_name(&mut self, name: &str) -> Result<(), Errno> {
        match self.get_mut_reference() {
            Some(file) => file.file.set_name(name),
            None => Err(Errno::BadFile),
        }
    }
}

impl Drop for FileDescriptor {
    fn drop(&mut self) {
        close(self);
    }
}
