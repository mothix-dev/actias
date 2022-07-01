//! operations on files with file descriptors

use alloc::{
    boxed::Box,
    string::{String, ToString},
};
use bitmask_enum::bitmask;
use crate::types::Errno;
use num_enum::FromPrimitive;
use super::{
    tree::{File, get_file_from_path, get_directory_from_path},
    vfs::{Permissions, ROOT_DIR},
    dirname, basename,
};

/// numerical file descriptor
pub type FileDescriptor = usize;

/// describes how a file will be opened
#[bitmask(u8)]
#[derive(PartialOrd, FromPrimitive)]
pub enum OpenFlags {
    #[num_enum(default)]
    None        = Self(0),
    Read        = Self(1 << 0),
    Write       = Self(1 << 1),
    Append      = Self(1 << 2),
    Create      = Self(1 << 3),
    Truncate    = Self(1 << 4),
    NonBlocking = Self(1 << 5),
}

/// stores information about an open file
pub struct OpenFile {
    /// reference to file
    pub file: &'static mut Box<dyn File>,

    /// absolute path to file
    pub path: String,

    /// offset into the file
    pub offset: usize,

    /// can we read from this file?
    pub can_read: bool,

    /// can we write to this file?
    pub can_write: bool,

    /// should we block the current thread while trying to access this file?
    pub should_block: bool,

    /// should we always append to this file when writing to it?
    pub should_append: bool,
}

impl Clone for OpenFile {
    fn clone(&self) -> Self {
        let file = get_file_from_path(unsafe { ROOT_DIR.as_mut().expect("file system not initialized") }, &self.path).expect("couldn't open file");

        OpenFile {
            file,
            path: self.path.to_string(),
            offset: self.offset,
            can_read: self.can_read,
            can_write: self.can_write,
            should_block: self.should_block,
            should_append: self.should_append,
        }
    }
}

/// opens a file
pub fn open(path: &str, flags: OpenFlags) -> Result<OpenFile, Errno> {
    let file =
        if let Some(file) = get_file_from_path(unsafe { ROOT_DIR.as_mut().expect("file system not initialized") }, path) {
            file
        } else if flags & OpenFlags::Create != OpenFlags::None {
            let dirname = dirname(path);
            let filename = basename(path).ok_or(Errno::NoSuchFileOrDir)?;

            let dir = get_directory_from_path(unsafe { ROOT_DIR.as_mut().expect("file system not initialized") }, &dirname).ok_or(Errno::NoSuchFileOrDir)?;

            dir.create_file(filename)?;

            get_file_from_path(dir, filename).ok_or(Errno::NoSuchFileOrDir)?
        } else {
            Err(Errno::NoSuchFileOrDir)?
        };
    
    if flags & OpenFlags::Truncate != OpenFlags::None {
        file.truncate(0)?;
    }

    let mut opened = OpenFile {
        file,
        path: path.to_string(),
        offset: 0,
        can_read: false,
        can_write: false,
        should_block: flags & OpenFlags::NonBlocking == OpenFlags::None,
        should_append: flags & OpenFlags::Append != OpenFlags::None,
    };

    if flags & OpenFlags::Read != OpenFlags::None {
        opened.can_read = true;
    }

    if flags & OpenFlags::Write != OpenFlags::None {
        opened.can_write = true;
    }

    if flags & OpenFlags::Append != OpenFlags::None {
        opened.can_write = true;
        opened.seek(0, SeekKind::End)?;
    }

    Ok(opened)
}

/// controls how FileDescriptor::seek() seeks
#[repr(u8)]
#[derive(FromPrimitive)]
pub enum SeekKind {
    /// set file writing offset to provided offset
    #[num_enum(default)]
    Set = 0,

    /// add the provided offset to the current file offset
    Current,

    /// set the file offset to the end of the file plus the provided offset
    End,
}

impl OpenFile {
    /// get permissions for file
    pub fn get_permissions(&mut self) -> Permissions {
        self.file.get_permissions()
    }

    /// set permissions for file
    pub fn set_permissions(&mut self, permissions: Permissions) -> Result<(), Errno> {
        self.file.set_permissions(permissions)
    }


    /// write all bytes contained in slice to file
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize, Errno> {
        if self.can_write {
            if self.should_append {
                self.seek(0, SeekKind::End)?;
            }

            let amt = self.file.write_at(bytes, self.offset)?;
            self.offset += amt;
            Ok(amt)
        } else {
            Err(Errno::BadFile)
        }
    }

    /// write all bytes contained in slice to file at offset
    pub fn write_at(&mut self, bytes: &[u8], offset: usize) -> Result<usize, Errno> {
        if self.can_write {
            self.file.write_at(bytes, offset)
        } else {
            Err(Errno::BadFile)
        }
    }

    /// checks if there's enough room to write the provided amount of bytes into the file
    pub fn can_write(&mut self, space: usize) -> Result<bool, Errno> {
        if self.can_write {
            Ok(self.file.can_write_at(space, self.offset))
        } else {
            Err(Errno::BadFile)
        }
    }

    /// checks if there's enough room to write the provided amount of bytes into the file at the provided offset
    pub fn can_write_at(&mut self, space: usize, offset: usize) -> Result<bool, Errno> {
        if self.can_write {
            Ok(self.file.can_write_at(space, offset))
        } else {
            Err(Errno::BadFile)
        }
    }


    /// read from file into provided slice
    pub fn read(&mut self, bytes: &mut [u8]) -> Result<usize, Errno> {
        if self.can_read {
            let amt = self.file.read_at(bytes, self.offset)?;
            self.offset += amt;
            Ok(amt)
        } else {
            Err(Errno::BadFile)
        }
    }

    /// read from file at offset into provided slice
    pub fn read_at(&mut self, bytes: &mut [u8], offset: usize) -> Result<usize, Errno> {
        if self.can_read {
            self.file.read_at(bytes, offset)
        } else {
            Err(Errno::BadFile)
        }
    }

    /// checks if there's enough room to read the provided amount of bytes from the file
    pub fn can_read(&mut self, space: usize) -> Result<bool, Errno> {
        if self.can_read {
            Ok(self.file.can_read_at(space, self.offset))
        } else {
            Err(Errno::BadFile)
        }
    }

    /// checks if there's enough room to read the provided amount of bytes from the file at the provided offset
    pub fn can_read_at(&mut self, space: usize, offset: usize) -> Result<bool, Errno> {
        if self.can_read {
            Ok(self.file.can_read_at(space, offset))
        } else {
            Err(Errno::BadFile)
        }
    }


    /// seek file
    /// seek behavior depends on the SeekKind provided
    pub fn seek(&mut self, offset: isize, kind: SeekKind) -> Result<usize, Errno> {
        let size = self.file.get_size();

        match kind {
            SeekKind::Set => self.offset = offset as usize,
            SeekKind::Current => {
                if offset > 0 {
                    self.offset = self.offset.wrapping_add(offset as usize); // we can wrap since if it goes below zero it'll be bigger than the file size, and thus fail
                } else {
                    self.offset = self.offset.wrapping_sub((-offset) as usize);
                }
            },
            SeekKind::End => {
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
    }


    /// truncate file, setting its size to the provided size
    pub fn truncate(&mut self, size: usize) -> Result<(), Errno> {
        if self.can_write {
            self.file.truncate(size)
        } else {
            Err(Errno::BadFile)
        }
    }


    /// lock file
    /// lock behavior depends on the LockKind provided
    /*pub fn lock(&mut self, kind: LockKind, size: isize) -> Result<(), Errno> {
        self.file.lock(kind, size)
    }*/


    /// gets name of file
    pub fn get_name(&mut self) -> &str {
        self.file.get_name()
    }

    /// sets name of file
    pub fn set_name(&mut self, name: &str) -> Result<(), Errno> {
        self.file.set_name(name)
    }
}
