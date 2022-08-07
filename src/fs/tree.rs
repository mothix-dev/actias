//! vfs tree

use crate::types::{
    errno::Errno,
    file::{Permissions, FileStatus},
    UserID, GroupID,
};
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec::Vec,
};
use super::{
    dirname,
    basename,
};

/// describes how a file should interact with the rest of the system
#[allow(unused_variables)]
pub trait File {
    /// get permissions for file
    fn get_permissions(&self) -> Permissions;

    /// set permissions for file
    fn set_permissions(&mut self, _permissions: Permissions) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }


    /// gets uid of the owner of this file
    fn get_owner(&self) -> UserID {
        0
    }

    /// sets uid of the owner of this file
    fn set_owner(&mut self, owner: UserID) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    /// gets gid of the owner of this file
    fn get_group(&self) -> GroupID {
        0
    }

    /// sets gid of the owner of this file
    fn set_group(&mut self, group: GroupID) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }


    /// write all bytes contained in slice to file
    //fn write(&mut self, bytes: &[u8]) -> Result<usize, Errno>;

    /// write all bytes contained in slice to file at offset
    fn write_at(&mut self, bytes: &[u8], offset: u64) -> Result<usize, Errno> {
        Err(Errno::NotSupported)
    }

    /// checks if there's enough room to write the provided amount of bytes into the file
    //fn can_write(&self, space: usize) -> bool;

    /// checks if there's enough room to write the provided amount of bytes into the file at the provided offset
    fn can_write_at(&self, space: usize, offset: u64) -> bool {
        false
    }


    /// read from file into provided slice
    //fn read(&self, bytes: &mut [u8]) -> Result<usize, Errno>;

    /// read from file at offset into provided slice
    fn read_at(&self, bytes: &mut [u8], offset: u64) -> Result<usize, Errno> {
        Err(Errno::NotSupported)
    }

    /// checks if there's enough room to read the provided amount of bytes from the file
    //fn can_read(&self, space: usize) -> bool;

    /// checks if there's enough room to read the provided amount of bytes from the file at the provided offset
    fn can_read_at(&self, space: usize, offset: u64) -> bool {
        false
    }


    /// seek file
    /// seek behavior depends on the SeekType provided
    //fn seek(&mut self, offset: isize, kind: SeekType) -> Result<usize, Errno>;


    /// truncate file, setting its size to the provided size
    fn truncate(&mut self, size: u64) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }


    /// lock file
    /// lock behavior depends on the LockKind provided
    //fn lock(&mut self, kind: LockKind, size: isize) -> Result<(), Errno>;


    /// gets status of an open file
    fn stat(&self, status: &mut FileStatus) -> Result<(), Errno> {
        *status = FileStatus {
            user_id: self.get_owner(),
            group_id: self.get_group(),
            size: self.get_size(),
            .. Default::default()
        };

        Ok(())
    }


    /// gets name of file
    fn get_name(&self) -> &str;

    /// sets name of file
    fn set_name(&mut self, name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }


    /// gets size of file
    fn get_size(&self) -> u64;
}

/// describes how a directory should interact with the rest of the system
#[allow(unused_variables)]
pub trait Directory {
    /// get permissions for directory
    fn get_permissions(&self) -> Permissions;

    /// set permissions for directory
    fn set_permissions(&mut self, permissions: Permissions) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }


    /// gets uid of the owner of this file
    fn get_owner(&self) -> UserID {
        0
    }

    /// sets uid of the owner of this file
    fn set_owner(&mut self, owner: UserID) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    /// gets gid of the owner of this file
    fn get_group(&self) -> GroupID {
        0
    }

    /// sets gid of the owner of this file
    fn set_group(&mut self, group: GroupID) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }


    /// gets files in directory
    fn get_files(&self) -> &Vec<Box<dyn File>>;

    /// gets files in directory
    fn get_files_mut(&mut self) -> &mut Vec<Box<dyn File>>;

    /// creates a file in the directory
    fn create_file(&mut self, name: &str, permissions: Permissions) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    /// deletes a file in the directory
    fn delete_file(&mut self, name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }


    /// gets directories in directory
    fn get_directories(&self) -> &Vec<Box<dyn Directory>>;

    /// gets directories in directory
    fn get_directories_mut(&mut self) -> &mut Vec<Box<dyn Directory>>;

    /// creates a subdirectory in the directory
    fn create_directory(&mut self, name: &str, permissions: Permissions) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    /// deletes a subdirectory in the directory
    fn delete_directory(&mut self, name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }


    /// gets links in directory
    fn get_links(&self) -> &Vec<Box<dyn SymLink>>;

    /// gets links in directory
    fn get_links_mut(&mut self) -> &mut Vec<Box<dyn SymLink>>;

    /// creates a symlink in the directory
    fn create_link(&mut self, name: &str, target: &str, permissions: Permissions) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    /// deletes a symlink in the directory
    fn delete_link(&mut self, name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }


    /// gets name of directory
    fn get_name(&self) -> &str;

    /// sets name of directory
    fn set_name(&mut self, name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }
}

/// describes how a symlink should interact with the rest of the system
#[allow(unused_variables)]
pub trait SymLink {
    /// get permissions for link
    fn get_permissions(&self) -> Permissions;

    /// set permissions for link
    fn set_permissions(&mut self, permissions: Permissions) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }


    /// gets uid of the owner of this file
    fn get_owner(&self) -> UserID {
        0
    }

    /// sets uid of the owner of this file
    fn set_owner(&mut self, owner: UserID) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    /// gets gid of the owner of this file
    fn get_group(&self) -> GroupID {
        0
    }

    /// sets gid of the owner of this file
    fn set_group(&mut self, group: GroupID) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }


    /// gets name of link
    fn get_name(&self) -> &str;

    /// sets name of link
    fn set_name(&mut self, name: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }


    /// gets target of link
    fn get_target(&self) -> &str;

    /// sets target of link
    fn set_target(&mut self, target: &str) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }
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

/// how many symlinks we can follow before giving up
const MAX_SYMLINKS: usize = 32;

// recurse over tree to find symlinks
fn get_link(dir: &mut Box<dyn Directory>, path: Vec<&str>, index: usize, depth: usize) -> Result<String, Errno> {
    if depth > MAX_SYMLINKS {
        Err(Errno::TooManySymLinks)
    } else if let Some(name) = path.get(index) {
        if name.is_empty() {
            return get_link(dir, path, index + 1, depth + 1);
        } else {
            for directory in dir.get_directories_mut() {
                if &directory.get_name() == name {
                    return get_link(directory, path, index + 1, depth + 1);
                }
            }
        }

        for link in dir.get_links() {
            if &link.get_name() == name {
                //log!("found link {} to {}", link.get_name(), link.get_target());

                return clean_up_path(&format!("{}/{}/{}", path[..index].join("/"), link.get_target(), path[index + 1..].join("/"))).ok_or(Errno::NoSuchFileOrDir);
            }
        }

        Err(Errno::NoSuchFileOrDir)
    } else {
        Err(Errno::NoSuchFileOrDir)
    }
}

/// given a path, follow any and all symlinks to obtain the absolute path
pub fn get_absolute_path(dir: &mut Box<dyn Directory>, path: &str) -> Result<String, Errno> {
    if path.is_empty() { // sanity check
        Err(Errno::NoSuchFileOrDir)
    } else {
        let mut path = path.to_string();

        // find and follow any directory links
        loop {
            match get_link(dir, path.split('/').collect::<Vec<_>>(), 0, 0) {
                Ok(new) => path = new,
                Err(Errno::NoSuchFileOrDir) => break,
                Err(err) => Err(err)?,
            }
        }

        // follow any file links if path points to a file
        if let Some(mut file_name) = basename(&path).map(|s| s.to_string()) {
            let mut dir_name = dirname(&path);
            let mut i = 0;

            while let Some(link) = get_directory_from_path(dir, &dir_name)?.get_links().iter().find(|l| l.get_name() == file_name) {
                if i > MAX_SYMLINKS {
                    Err(Errno::TooManySymLinks)?
                }

                let target = clean_up_path(&format!("{}/{}", dir_name, link.get_target())).ok_or(Errno::NoSuchFileOrDir)?;

                dir_name = dirname(&target); // if we own the strings the borrow checker won't yell at us
                file_name = basename(&target).ok_or(Errno::IsDirectory)?.to_string();

                i += 1;
            }

            Ok(format!("{}/{}", dir_name, file_name))
        } else {
            Ok(path)
        }
    }
}

/// gets a file object from the given path
pub fn get_file_from_path<'a>(dir: &'a mut Box<dyn Directory>, path: &str) -> Result<&'a mut Box<dyn File>, Errno> {
    if path.is_empty() { // sanity check
        Err(Errno::NoSuchFileOrDir)
    } else {
        let mut dir_name = dirname(path);
        let mut file_name = basename(path).ok_or(Errno::IsDirectory)?.to_string();
        let mut i = 0;

        while let Some(link) = get_directory_from_path(dir, &dir_name)?.get_links().iter().find(|l| l.get_name() == file_name) {
            if i > MAX_SYMLINKS {
                Err(Errno::TooManySymLinks)?
            }

            let target = format!("{}/{}", dir_name, link.get_target());

            dir_name = dirname(&target); // if we own the strings the borrow checker won't yell at us
            file_name = basename(&target).ok_or(Errno::IsDirectory)?.to_string();

            i += 1;
        }

        get_directory_from_path(dir, &dir_name)?.get_files_mut().iter_mut().find(|f| f.get_name() == file_name).ok_or(Errno::NoSuchFileOrDir)
    }
}

/// gets a directory object from the given path
pub fn get_directory_from_path<'a>(dir: &'a mut Box<dyn Directory>, path: &str) -> Result<&'a mut Box<dyn Directory>, Errno> {
    if path.is_empty() { // sanity check
        Ok(dir)
    } else {
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

        /*while let Some(new) = get_link(dir, path.split('/').collect::<Vec<_>>(), 0, 0) {
            path = new;
        }*/
        loop {
            match get_link(dir, path.split('/').collect::<Vec<_>>(), 0, 0) {
                Ok(new) => path = new,
                Err(Errno::NoSuchFileOrDir) => break,
                Err(err) => Err(err)?,
            }
        }

        let split = path.split('/').rev().collect::<Vec<_>>(); // reversed because popping off the end is faster

        get_dir(dir, split).ok_or(Errno::NoSuchFileOrDir)
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
