//! operations on files with file descriptors

use alloc::{
    boxed::Box,
    string::{String, ToString},
};
use core::fmt;
use crate::types::{
    errno::Errno,
    file::{OpenFlags, SeekKind, Permissions, UnlinkFlags},
};
use super::{
    tree::{File, Directory, get_file_from_path, get_directory_from_path, get_absolute_path},
    vfs::ROOT_DIR,
    dirname, basename,
};

/// stores information about an open file
pub struct OpenFile {
    /// reference to file
    pub file: &'static mut Box<dyn File>,

    /// absolute path to file
    pub path: String,

    /// offset into the file
    pub offset: u64,

    /// can we read from this file?
    pub can_read: bool,

    /// can we write to this file?
    pub can_write: bool,

    /// should we block the current thread while trying to access this file?
    pub should_block: bool,

    /// should we always append to this file when writing to it?
    pub should_append: bool,
}

impl fmt::Debug for OpenFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenFile")
         .field("path", &self.path)
         .field("offset", &self.offset)
         .field("can_read", &self.can_read)
         .field("can_write", &self.can_write)
         .field("should_block", &self.should_block)
         .field("should_append", &self.should_append)
         .finish_non_exhaustive()
    }
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
pub fn open(path: &str, flags: OpenFlags, permissions: Permissions) -> Result<OpenFile, Errno> {
    let file =
        match get_file_from_path(unsafe { ROOT_DIR.as_mut().expect("file system not initialized") }, path) {
            Ok(file) => file,
            Err(Errno::NoSuchFileOrDir) => if flags & OpenFlags::Create != OpenFlags::None {
                let dirname = dirname(path);
                let filename = basename(path).ok_or(Errno::IsDirectory)?;
    
                let dir = get_directory_from_path(unsafe { ROOT_DIR.as_mut().expect("file system not initialized") }, &dirname)?;
    
                dir.create_file(filename, permissions)?;
    
                get_file_from_path(dir, filename)?
            } else {
                Err(Errno::NoSuchFileOrDir)?
            },
            Err(err) => Err(err)?,
        };
    
    if flags & OpenFlags::Truncate != OpenFlags::None {
        file.truncate(0)?;
    }

    let mut opened = OpenFile {
        file,
        path: get_absolute_path(unsafe { ROOT_DIR.as_mut().expect("file system not initialized") }, path)?,
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

    log!("opened file {:#?}", opened);

    Ok(opened)
}

pub fn unlink_at(dir: &mut Box<dyn Directory>, path: &str, flags: UnlinkFlags) -> Result<(), Errno> {
    if flags & UnlinkFlags::RemoveDir != UnlinkFlags::None {
        // rmdir
        //let dir = get_directory_from_path(dir, path)?;


        Err(Errno::NotSupported)
    } else {
        if path.is_empty() { // sanity check
            Err(Errno::NoSuchFileOrDir)
        } else {
            let dir_name = dirname(path);
            let file_name = basename(path).ok_or(Errno::IsDirectory)?;

            let dir = get_directory_from_path(dir, &dir_name)?;

            // TODO: check permissions of dir for sticky bit

            if let Some(link) = dir.get_links_mut().iter_mut().find(|f| f.get_name() == file_name) {
                dir.delete_link(file_name)
            } else if let Some(file) = dir.get_files_mut().iter_mut().find(|f| f.get_name() == file_name) {
                dir.delete_file(file_name)
            } else if let Some(file) = dir.get_directories_mut().iter_mut().find(|f| f.get_name() == file_name) {
                dir.delete_directory(file_name)
            } else {
                Err(Errno::NoSuchFileOrDir)
            }
        }
    }
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
            self.offset += amt as u64;
            Ok(amt)
        } else {
            Err(Errno::BadFile)
        }
    }

    /// write all bytes contained in slice to file at offset
    pub fn write_at(&mut self, bytes: &[u8], offset: u64) -> Result<usize, Errno> {
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
    pub fn can_write_at(&mut self, space: usize, offset: u64) -> Result<bool, Errno> {
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
            self.offset += amt as u64;
            Ok(amt)
        } else {
            Err(Errno::BadFile)
        }
    }

    /// read from file at offset into provided slice
    pub fn read_at(&mut self, bytes: &mut [u8], offset: u64) -> Result<usize, Errno> {
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
    pub fn can_read_at(&mut self, space: usize, offset: u64) -> Result<bool, Errno> {
        if self.can_read {
            Ok(self.file.can_read_at(space, offset))
        } else {
            Err(Errno::BadFile)
        }
    }


    /// seek file
    /// seek behavior depends on the SeekKind provided
    pub fn seek(&mut self, offset: isize, kind: SeekKind) -> Result<u64, Errno> {
        let size = self.file.get_size();

        match kind {
            SeekKind::Set => self.offset = offset as u64,
            SeekKind::Current => {
                if offset > 0 {
                    self.offset = self.offset.wrapping_add(offset as u64); // we can wrap since if it goes below zero it'll be bigger than the file size, and thus fail
                } else {
                    self.offset = self.offset.wrapping_sub((-offset) as u64);
                }
            },
            SeekKind::End => {
                if offset > 0 {
                    return Err(Errno::InvalidSeek);
                } else {
                    self.offset = size.wrapping_sub((-offset) as u64);
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
    pub fn truncate(&mut self, size: u64) -> Result<(), Errno> {
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
