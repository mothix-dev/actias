//! virtual filesystems and filesystem interface

use bitmask_enum::bitmask;
use core::fmt;
use crate::{
    tar::TarIterator,
    types::Errno,
};
use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec, vec::Vec,
};
use super::tree::{
    File, Directory, SymLink,
    get_directory_from_path, get_file_from_path,
};

/// standard unix permissions bit field
#[bitmask(u16)]
pub enum Permissions {
    SetUID          = Self(1 << 11),
    SetGID          = Self(1 << 10),
    Sticky          = Self(1 << 9),
    OwnerRead       = Self(1 << 8),
    OwnerWrite      = Self(1 << 7),
    OwnerExecute    = Self(1 << 6),
    GroupRead       = Self(1 << 5),
    GroupWrite      = Self(1 << 4),
    GroupExecute    = Self(1 << 3),
    OtherRead       = Self(1 << 2),
    OtherWrite      = Self(1 << 1),
    OtherExecute    = Self(1 << 0),
    None            = Self(0),
}

impl fmt::Display for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", if *self & Self::OwnerRead != 0 { "r" } else { "-" })?;
        write!(f, "{}", if *self & Self::OwnerWrite != 0 { "w" } else { "-" })?;
        write!(f, "{}", if *self & Self::OwnerExecute != 0 && *self & Self::SetUID != 0 { "s" } else if *self & Self::SetUID != 0 { "S" } else if *self & Self::OwnerExecute != 0 { "x" } else { "-" })?;
        write!(f, "{}", if *self & Self::GroupRead != 0 { "r" } else { "-" })?;
        write!(f, "{}", if *self & Self::GroupWrite != 0 { "w" } else { "-" })?;
        write!(f, "{}", if *self & Self::GroupExecute != 0 && *self & Self::SetGID != 0 { "s" } else if *self & Self::SetGID != 0 { "S" } else if *self & Self::GroupExecute != 0 { "x" } else { "-" })?;
        write!(f, "{}", if *self & Self::OtherRead != 0 { "r" } else { "-" })?;
        write!(f, "{}", if *self & Self::OtherWrite != 0 { "w" } else { "-" })?;
        write!(f, "{}", if *self & Self::Sticky != 0 { "t" } else if *self & Self::OtherExecute != 0 { "x" } else { "-" })
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

    fn get_owner(&self) -> usize {
        0
    }

    fn set_owner(&mut self, _owner: usize) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    fn get_group(&self) -> usize {
        0
    }

    fn set_group(&mut self, _group: usize) -> Result<(), Errno> {
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

    fn create_file(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    fn create_directory(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }
    
    fn create_link(&mut self, _name: &str, _target: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    fn delete_file(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    fn delete_directory(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }
    
    fn delete_link(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
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

    fn get_owner(&self) -> usize {
        0
    }

    fn set_owner(&mut self, _owner: usize) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    fn get_group(&self) -> usize {
        0
    }

    fn set_group(&mut self, _group: usize) -> Result<(), Errno> {
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

    fn create_file(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    fn create_directory(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }
    
    fn create_link(&mut self, _name: &str, _target: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    fn delete_file(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    fn delete_directory(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }
    
    fn delete_link(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
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

    fn get_owner(&self) -> usize {
        self.dir.get_owner()
    }

    fn set_owner(&mut self, owner: usize) -> Result<(), Errno> {
        self.dir.set_owner(owner)
    }

    fn get_group(&self) -> usize {
        self.dir.get_group()
    }

    fn set_group(&mut self, group: usize) -> Result<(), Errno> {
        self.dir.set_group(group)
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

    fn create_file(&mut self, name: &str) -> Result<(), Errno> {
        self.dir.create_file(name)
    }

    fn create_directory(&mut self, name: &str) -> Result<(), Errno> {
        self.dir.create_directory(name)
    }

    fn create_link(&mut self, name: &str, target: &str) -> Result<(), Errno> {
        self.dir.create_link(name, target)
    }

    fn delete_file(&mut self, name: &str) -> Result<(), Errno> {
        self.dir.delete_file(name)
    }

    fn delete_directory(&mut self, name: &str) -> Result<(), Errno> {
        self.dir.delete_directory(name)
    }

    fn delete_link(&mut self, name: &str) -> Result<(), Errno> {
        self.dir.delete_link(name)
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

pub fn remove_mount_point(name: &str) {
    let dir = get_directory_from_path(unsafe { ROOT_DIR.as_mut().unwrap() }, "/fs").expect("couldn't get filesystem directory");

    let mounts = dir.get_directories_mut();

    for i in 0..mounts.len() {
        if mounts[i].get_name() == name {
            mounts.remove(i);
            break;
        }
    }
}

pub fn add_device(name: &str, tree: Box<dyn Directory>) {
    let dir = get_directory_from_path(unsafe { ROOT_DIR.as_mut().unwrap() }, "/dev").expect("couldn't get device directory");
    let permissions = dir.get_permissions();

    dir.get_directories_mut().push(Box::new(MountPoint { // we can just do this again since it works lmao
        dir: tree,
        permissions,
        name: name.to_string(),
    }))
}

pub fn remove_device(name: &str) {
    let dir = get_directory_from_path(unsafe { ROOT_DIR.as_mut().unwrap() }, "/dev").expect("couldn't get device directory");

    let devices = dir.get_directories_mut();

    for i in 0..devices.len() {
        if devices[i].get_name() == name {
            devices.remove(i);
            break;
        }
    }
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

    // add console device
    add_device("console", crate::console::make_console_device());

    // mount initrd
    if let Some(initrd) = crate::platform::get_initrd() {
        add_mount_point("initrd", crate::tar::make_tree(TarIterator::new(initrd)));
    }

    //super::tree::print_tree(unsafe { ROOT_DIR.as_ref().unwrap() });
}
