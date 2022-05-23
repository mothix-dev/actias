//! virtual filesystems and filesystem interface

use bitmask_enum::bitmask;
use core::fmt;
use crate::errno::Errno;
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::string::String;

/// standard unix permissions bit field
#[bitmask(u16)]
pub enum Permissions {
    OwnerRead       = Self(1 << 8),
    OwnerWrite      = Self(1 << 7),
    OwnerExecute    = Self(1 << 6),
    GroupRead       = Self(1 << 5),
    GroupWrite      = Self(1 << 4),
    GroupExecute    = Self(1 << 3),
    OtherRead       = Self(1 << 2),
    OtherWrite      = Self(1 << 1),
    OtherExecute    = Self(1 << 0),
}

impl fmt::Display for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", if *self & Self::OwnerRead != 0 { "r" } else { "-" })?;
        write!(f, "{}", if *self & Self::OwnerWrite != 0 { "w" } else { "-" })?;
        write!(f, "{}", if *self & Self::OwnerExecute != 0 { "x" } else { "-" })?;
        write!(f, "{}", if *self & Self::GroupRead != 0 { "r" } else { "-" })?;
        write!(f, "{}", if *self & Self::GroupWrite != 0 { "w" } else { "-" })?;
        write!(f, "{}", if *self & Self::GroupExecute != 0 { "x" } else { "-" })?;
        write!(f, "{}", if *self & Self::OtherRead != 0 { "r" } else { "-" })?;
        write!(f, "{}", if *self & Self::OtherWrite != 0 { "w" } else { "-" })?;
        write!(f, "{}", if *self & Self::OtherExecute != 0 { "x" } else { "-" })
    }
}

/// file descriptor- contains a numbered reference to a file
#[repr(transparent)]
pub struct FileDescriptor(pub usize);

impl FileDescriptor {
    /*pub fn get_file() -> Option<&'static File> {
        // ...
    }

    pub fn get_file_mut() -> Option<&'static mut File> {
        // ...
    }*/
}

pub struct DirEnt<'a> {
    serial: usize,
    name: String,
    directory: &'a mut Box<dyn Directory>,
}

/// controls how File::seek() seeks
pub enum SeekType {
    /// set file writing offset to provided offset
    Set,

    /// add the provided offset to the current file offset
    Current,

    /// set the file offset to the end of the file plus the provided offset
    End,
}

/// controls how File::lock() locks
pub enum LockType {
    /// unlock a locked section
    Unlock,

    /// lock a section
    Lock,

    /// test if a section is locked and lock it
    TestLock,

    /// test if a section is locked
    Test
}

pub trait File {
    /// get permissions for file
    fn get_permissions(&self) -> Permissions;

    /// set permissions for file
    fn set_permissions(&mut self, permissions: Permissions) -> Result<(), Errno>;


    /// write all bytes contained in slice to file
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Errno>;

    /// write all bytes contained in slice to file at offset
    fn write_at(&mut self, bytes: &[u8], offset: usize) -> Result<usize, Errno>;

    /// checks if there's enough room to write the provided amount of bytes into the file
    fn can_write(&self, space: usize) -> bool;

    /// checks if there's enough room to write the provided amount of bytes into the file at the provided offset
    fn can_write_at(&self, space: usize, offset: usize) -> bool;


    /// read from file into provided slice
    fn read(&self, bytes: &mut [u8]) -> Result<usize, Errno>;

    /// read from file at offset into provided slice
    fn read_at(&self, bytes: &mut [u8], offset: usize) -> Result<usize, Errno>;

    /// checks if there's enough room to read the provided amount of bytes from the file
    fn can_read(&self, space: usize) -> bool;

    /// checks if there's enough room to read the provided amount of bytes from the file at the provided offset
    fn can_read_at(&self, space: usize, offset: usize) -> bool;


    /// seek file
    /// seek behavior depends on the SeekType provided
    fn seek(&mut self, offset: isize, kind: SeekType) -> Result<usize, Errno>;


    /// truncate file, setting its size to the provided size
    fn truncate(&mut self, size: usize) -> Result<(), Errno>;


    /// lock file
    /// lock behavior depends on the LockType provided
    fn lock(&mut self, kind: LockType, size: isize) -> Result<(), Errno>;
}

pub trait Directory {
    /// get permissions for directory
    fn get_permissions(&self) -> Permissions;

    /// set permissions for directory
    fn set_permissions(&mut self, permissions: Permissions) -> Result<(), Errno>;


    /// gets files in directory
    fn get_files(&self) -> &Vec<Box<dyn File>>;

    /// gets files in directory
    fn get_files_mut(&mut self) -> &mut Vec<Box<dyn File>>;


    /// gets directories in directory
    fn get_directories(&self) -> &Vec<Box<dyn Directory>>;

    /// gets directories in directory
    fn get_directories_mut(&mut self) -> &mut Vec<Box<dyn Directory>>;
}

/// list of open files
pub static mut OPEN_FILES: Vec<OpenFile> = Vec::new();

/// stores information about an open file
pub struct OpenFile<'a> {
    /// file descriptor number
    descriptor: usize,

    /// reference to file
    file: &'a mut Box<dyn File>,
}

/// root directory of our filesystem
pub static mut ROOT_DIR: VfsRoot = VfsRoot {
    files: Vec::new(),
    directories: Vec::new(),
};

pub struct VfsRoot {
    files: Vec<Box<dyn File>>,
    directories: Vec<Box<dyn Directory>>,
}

impl Directory for VfsRoot {
    fn get_permissions(&self) -> Permissions {
        Permissions::OwnerRead | Permissions::OwnerWrite | Permissions::GroupRead | Permissions::GroupWrite | Permissions::OtherRead
    }

    fn set_permissions(&mut self, _permissions: Permissions) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    fn get_files(&self) -> &Vec<Box<dyn File>> {
        &self.files
    }

    fn get_files_mut(&mut self) -> &mut Vec<Box<dyn File>> {
        &mut self.files
    }

    fn get_directories(&self) -> &Vec<Box<dyn Directory>> {
        &self.directories
    }

    fn get_directories_mut(&mut self) -> &mut Vec<Box<dyn Directory>> {
        &mut self.directories
    }
}
