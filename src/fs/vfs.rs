//! virtual filesystems and filesystem interface

use bitmask_enum::bitmask;
use core::fmt;
use crate::errno::Errno;
use alloc::{
    vec::Vec,
    boxed::Box,
    string::String,
};
use super::tree::{File, Directory};

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

pub struct DirEnt<'a> {
    serial: usize,
    name: String,
    directory: &'a mut Box<dyn Directory>,
}

/// root directory of our filesystem
pub static mut ROOT_DIR: Option<Box<dyn Directory>> = None;

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

    fn get_name(&self) -> &str {
        ""
    }

    fn set_name(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }
}

pub fn init() {
    unsafe {
        ROOT_DIR = Some(Box::new(VfsRoot {
            files: Vec::new(),
            directories: Vec::new(),
        }));
    }
}
