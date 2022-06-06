//! vfs tree

use crate::errno::Errno;
use alloc::{
    vec::Vec,
    boxed::Box,
};
use super::{
    vfs::Permissions,
    dirname,
    basename,
};

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
    //fn write(&mut self, bytes: &[u8]) -> Result<usize, Errno>;

    /// write all bytes contained in slice to file at offset
    fn write_at(&mut self, bytes: &[u8], offset: usize) -> Result<usize, Errno>;

    /// checks if there's enough room to write the provided amount of bytes into the file
    //fn can_write(&self, space: usize) -> bool;

    /// checks if there's enough room to write the provided amount of bytes into the file at the provided offset
    fn can_write_at(&self, space: usize, offset: usize) -> bool;


    /// read from file into provided slice
    //fn read(&self, bytes: &mut [u8]) -> Result<usize, Errno>;

    /// read from file at offset into provided slice
    fn read_at(&self, bytes: &mut [u8], offset: usize) -> Result<usize, Errno>;

    /// checks if there's enough room to read the provided amount of bytes from the file
    //fn can_read(&self, space: usize) -> bool;

    /// checks if there's enough room to read the provided amount of bytes from the file at the provided offset
    fn can_read_at(&self, space: usize, offset: usize) -> bool;


    /// seek file
    /// seek behavior depends on the SeekType provided
    //fn seek(&mut self, offset: isize, kind: SeekType) -> Result<usize, Errno>;


    /// truncate file, setting its size to the provided size
    fn truncate(&mut self, size: usize) -> Result<(), Errno>;


    /// lock file
    /// lock behavior depends on the LockType provided
    fn lock(&mut self, kind: LockType, size: isize) -> Result<(), Errno>;


    /// gets name of file
    fn get_name(&self) -> &str;

    /// sets name of file
    fn set_name(&mut self, name: &str) -> Result<(), Errno>;


    /// gets size of file
    fn get_size(&self) -> usize;
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


    /// gets name of directory
    fn get_name(&self) -> &str;

    /// sets name of directory
    fn set_name(&mut self, name: &str) -> Result<(), Errno>;
}

/// gets a file object from the given path
/// path should not be absolute (i.e. starting with /)
pub fn get_file_from_path<'a>(dir: &'a mut Box<dyn Directory>, path: &str) -> Option<&'a mut Box<dyn File>> {
    if path.chars().count() == 0 || path.chars().nth(0).unwrap() == '/' { // sanity check
        None
    } else {
        let dir_name = dirname(path);
        let file_name = basename(path)?;
        let directory = if dir_name.len() > 0 { get_directory_from_path(dir, &dir_name)? } else { dir };
        for file in directory.get_files_mut() {
            if file.get_name() == file_name {
                return Some(file);
            }
        }

        None
    }
}

/// gets a directory object from the given path
/// path should not be absolute (i.e. starting with /)
pub fn get_directory_from_path<'a>(dir: &'a mut Box<dyn Directory>, path: &str) -> Option<&'a mut Box<dyn Directory>> {
    if path.chars().count() == 0 || path.chars().nth(0).unwrap() == '/' { // sanity check
        None
    } else {
        // recurse over tree
        fn walk_tree<'a>(dir: &'a mut Box<dyn Directory>, mut path: Vec<&str>) -> Option<&'a mut Box<dyn Directory>> {
            if let Some(name) = path.pop() {
                for directory in dir.get_directories_mut() {
                    if directory.get_name() == name {
                        return walk_tree(directory, path);
                    }
                }

                None
            } else {
                Some(dir)
            }
        }

        let split = path.split('/').rev().collect::<Vec<_>>(); // reversed because popping off the end is faster
        walk_tree(dir, split)
    }
}
