//! vfs tree

use crate::errno::Errno;
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec::Vec,
};
use super::{
    vfs::Permissions,
    dirname,
    basename,
};

/// controls how File::lock() locks
/*pub enum LockKind {
    /// unlock a locked section
    Unlock,

    /// lock a section
    Lock,

    /// test if a section is locked and lock it
    TestLock,

    /// test if a section is locked
    Test
}*/

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
    /// lock behavior depends on the LockKind provided
    //fn lock(&mut self, kind: LockKind, size: isize) -> Result<(), Errno>;


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


    /// gets links in directory
    fn get_links(&self) -> &Vec<Box<dyn SymLink>>;

    /// gets links in directory
    fn get_links_mut(&mut self) -> &mut Vec<Box<dyn SymLink>>;


    /// gets name of directory
    fn get_name(&self) -> &str;

    /// sets name of directory
    fn set_name(&mut self, name: &str) -> Result<(), Errno>;
}

pub trait SymLink {
    /// get permissions for link
    fn get_permissions(&self) -> Permissions;

    /// set permissions for link
    fn set_permissions(&mut self, permissions: Permissions) -> Result<(), Errno>;


    /// gets name of link
    fn get_name(&self) -> &str;

    /// sets name of link
    fn set_name(&mut self, name: &str) -> Result<(), Errno>;


    /// gets target of link
    fn get_target(&self) -> &str;

    /// sets target of link
    fn set_target(&mut self, target: &str) -> Result<(), Errno>;
}

/// cleans up path, removing .s and ..s
pub fn clean_up_path(path: &str) -> Option<String> {
    let mut split = path.split('/').collect::<Vec<_>>();

    let mut i = 0;
    while i < split.len() {
        if split[i] == "." {
            split.remove(i);
        } else if split[i] == ".." {
            if i == 0 {
                return None;
            }

            split.remove(i - 1);
            split.remove(i - 1);
            i -= 1;
        } else if split[i].is_empty() {
            split.remove(i);
        } else {
            i += 1;
        }
    }

    Some(split.join("/"))
}

/// gets a file object from the given path
pub fn get_file_from_path<'a>(dir: &'a mut Box<dyn Directory>, path: &str) -> Option<&'a mut Box<dyn File>> {
    if path.is_empty() { // sanity check
        None
    } else {
        let mut dir_name = dirname(path);
        let mut file_name = basename(path)?.to_string();

        while let Some(link) = get_directory_from_path(dir, &dir_name)?.get_links().iter().find(|l| l.get_name() == file_name) {
            let target = format!("{}/{}", dir_name, link.get_target());

            dir_name = dirname(&target); // if we own the strings the borrow checker won't yell at us
            file_name = basename(&target)?.to_string();
        }

        get_directory_from_path(dir, &dir_name)?.get_files_mut().iter_mut().find(|f| f.get_name() == file_name)
    }
}

/// gets a directory object from the given path
pub fn get_directory_from_path<'a>(dir: &'a mut Box<dyn Directory>, path: &str) -> Option<&'a mut Box<dyn Directory>> {
    if path.is_empty() { // sanity check
        Some(dir)
    } else {
        // recurse over tree to find symlinks
        fn get_link(dir: &mut Box<dyn Directory>, path: Vec<&str>, index: usize) -> Option<String> {
            if let Some(name) = path.get(index) {
                if name.is_empty() {
                    return get_link(dir, path, index + 1);
                } else {
                    for directory in dir.get_directories_mut() {
                        if &directory.get_name() == name {
                            return get_link(directory, path, index + 1);
                        }
                    }
                }

                for link in dir.get_links() {
                    if &link.get_name() == name {
                        //log!("found link {} to {}", link.get_name(), link.get_target());

                        return clean_up_path(&format!("{}/{}/{}", path[..index].join("/"), link.get_target(), path[index + 1..].join("/")));
                    }
                }
            }

            None
        }

        // recurse over tree to find the directory
        fn get_dir<'a>(dir: &'a mut Box<dyn Directory>, mut path: Vec<&str>) -> Option<&'a mut Box<dyn Directory>> {
            if let Some(name) = path.pop() {
                if name.is_empty() {
                    return get_dir(dir, path);
                } else {
                    for directory in dir.get_directories_mut() {
                        if directory.get_name() == name {
                            return get_dir(directory, path);
                        }
                    }
                }

                None
            } else {
                Some(dir)
            }
        }

        let mut path = path.to_string();

        while let Some(new) = get_link(dir, path.split('/').collect::<Vec<_>>(), 0) {
            path = new;
        }

        let split = path.split('/').rev().collect::<Vec<_>>(); // reversed because popping off the end is faster

        get_dir(dir, split)
    }
}

/// prints the contents of a directory recursively in tree-ish form
#[allow(clippy::borrowed_box)] // we need box here
pub fn print_tree(dir: &Box<dyn Directory>) {
    fn _print_tree(dir: &'_ Box<dyn Directory>, indent: usize) {
        let mut spaces: Vec<u8> = Vec::new();

        if indent > 0 {
            for _i in 0..indent - 2 {
                spaces.push(b' ');
            }

            spaces.push(b'-');
            spaces.push(b' ');
        }

        log!("{}{}/", core::str::from_utf8(&spaces).unwrap(), dir.get_name());

        let dirs = dir.get_directories();
        for dir2 in dirs {
            _print_tree(dir2, indent + 4);
        }
        
        spaces.clear();

        for _i in 0..indent + 2 {
            spaces.push(b' ');
        }

        spaces.push(b'-');
        spaces.push(b' ');

        for file in dir.get_files() {
            log!("{}{}", core::str::from_utf8(&spaces).unwrap(), file.get_name());
        }

        for link in dir.get_links() {
            log!("{}{} -> {}", core::str::from_utf8(&spaces).unwrap(), link.get_name(), link.get_target());
        }
    }

    _print_tree(dir, 0);
}
