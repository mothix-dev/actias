//! virtual filesystems and filesystem interface

use bitmask_enum::bitmask;
use core::fmt;
use crate::{
    errno::Errno,
    tar::TarIterator,
};
use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec, vec::Vec,
};
use super::tree::{
    File, Directory, SymLink,
    print_tree, get_directory_from_path, get_file_from_path,
};

/// standard unix permissions bit field
#[bitmask(u16)]
pub enum Permissions {
    None            = Self(0),
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
    links: Vec<Box<dyn SymLink>>,
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

    fn get_links(&self) -> &Vec<Box<dyn SymLink>> {
        &self.links
    }

    fn get_links_mut(&mut self) -> &mut Vec<Box<dyn SymLink>> {
        &mut self.links
    }

    fn get_name(&self) -> &str {
        ""
    }

    fn set_name(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }
}

pub struct VfsDir {
    files: Vec<Box<dyn File>>,
    directories: Vec<Box<dyn Directory>>,
    links: Vec<Box<dyn SymLink>>,
    permissions: Permissions,
    name: String,
}

impl Directory for VfsDir {
    fn get_permissions(&self) -> Permissions {
        self.permissions
    }

    fn set_permissions(&mut self, permissions: Permissions) -> Result<(), Errno> {
        self.permissions = permissions;
        Ok(())
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

    fn get_links(&self) -> &Vec<Box<dyn SymLink>> {
        &self.links
    }

    fn get_links_mut(&mut self) -> &mut Vec<Box<dyn SymLink>> {
        &mut self.links
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn set_name(&mut self, name: &str) -> Result<(), Errno> {
        self.name = name.to_string();
        Ok(())
    }
}

/// makes a directory in the vfs
pub fn vfs_mkdir(path: &str) {
    let elements = path.split('/').collect::<Vec<_>>();

    fn make_dir(elements: &Vec<&str>, extent: usize) {
        if extent > elements.len() {
            return;
        }

        let mut partial = elements[0..extent].to_vec();

        let dirname = partial.pop().unwrap().to_string();

        let path = partial.join("/");

        let dir = get_directory_from_path(unsafe { ROOT_DIR.as_mut().unwrap() }, &path).unwrap();
        
        let permissions = dir.get_permissions();

        let should_make = || {
            for dir2 in dir.get_directories() {
                if dir2.get_name() == dirname {
                    return false;
                }
            }
            true
        };

        if !dirname.is_empty() && should_make() {
            dir.get_directories_mut().push(Box::new(VfsDir {
                files: Vec::new(),
                directories: Vec::new(),
                links: Vec::new(),
                permissions,
                name: dirname,
            }));
        }

        make_dir(elements, extent + 1);
    }

    make_dir(&elements, 1);
}

pub struct MountPoint {
    dir: Box<dyn Directory>,
    permissions: Permissions,
    name: String,
}

impl Directory for MountPoint {
    fn get_permissions(&self) -> Permissions {
        self.permissions
    }

    fn set_permissions(&mut self, permissions: Permissions) -> Result<(), Errno> {
        self.permissions = permissions;
        Ok(())
    }

    fn get_files(&self) -> &Vec<Box<dyn File>> {
        self.dir.get_files()
    }

    fn get_files_mut(&mut self) -> &mut Vec<Box<dyn File>> {
        self.dir.get_files_mut()
    }

    fn get_directories(&self) -> &Vec<Box<dyn Directory>> {
        self.dir.get_directories()
    }

    fn get_directories_mut(&mut self) -> &mut Vec<Box<dyn Directory>> {
        self.dir.get_directories_mut()
    }

    fn get_links(&self) -> &Vec<Box<dyn SymLink>> {
        self.dir.get_links()
    }

    fn get_links_mut(&mut self) -> &mut Vec<Box<dyn SymLink>> {
        self.dir.get_links_mut()
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn set_name(&mut self, name: &str) -> Result<(), Errno> {
        self.name = name.to_string();
        Ok(())
    }
}

pub fn add_mount_point(name: &str, tree: Box<dyn Directory>) {
    let dir = get_directory_from_path(unsafe { ROOT_DIR.as_mut().unwrap() }, "/fs").expect("couldn't get filesystem directory");
    let permissions = dir.get_permissions();

    dir.get_directories_mut().push(Box::new(MountPoint {
        dir: tree,
        permissions,
        name: name.to_string(),
    }))
}

pub fn read_file(path: &str) -> Result<Vec<u8>, Errno> {
    let file = get_file_from_path(unsafe { ROOT_DIR.as_mut().unwrap() }, path).ok_or(Errno::NoSuchFileOrDir)?;

    let mut buf = vec![0; file.get_size()];
    file.read_at(buf.as_mut_slice(), 0)?;

    Ok(buf)
}

pub fn init() {
    // create root dir
    unsafe {
        ROOT_DIR = Some(Box::new(VfsRoot {
            files: Vec::new(),
            directories: Vec::new(),
            links: Vec::new(),
        }));
    }

    // create directories
    vfs_mkdir("/dev");
    vfs_mkdir("/proc");
    vfs_mkdir("/fs");

    // mount initrd
    if let Some(initrd) = crate::platform::get_initrd() {
        add_mount_point("initrd", crate::tar::make_tree(TarIterator::new(initrd)));
    }
}
