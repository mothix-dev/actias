//! tests

use core::arch::asm;
use crate::{
    console::{ColorCode, get_console},
    fs::{
        tree::{
            File, Directory, LockType,
            get_file_from_path, get_directory_from_path,
        },
        vfs::Permissions,
    },
    errno::Errno,
};
use alloc::{
    boxed::Box,
    vec,
    vec::Vec,
    string::{String, ToString},
};

/// custom test runner to run all tests
pub fn test_runner(tests: &[&dyn Testable]) {
    log!("=== Running {} tests", tests.len());
    for test in tests {
        test.run();
    }
    log!("=== Done");
}

/// custom testable trait
pub trait Testable {
    fn run(&self) -> ();
}

impl<T> Testable for T where T: Fn() {
    fn run(&self) {
        log!("--- {}...", core::any::type_name::<T>());
        self();
        log!("--- ok");
    }
}

/// test breakpoint interrupt
#[test_case]
fn int() {
    unsafe {
        asm!("int3");
    }
}

/// make sure writing to vga console doesn't crash
#[test_case]
fn vga_partial() {
    let console = get_console().unwrap();

    for _i in 0..256 {
        for bg in 0..16 {
            for fg in 0..16 {
                console.set_color(ColorCode {
                    foreground: fg.into(),
                    background: bg.into()
                });
                console.puts("OwO ");
            }
        }
    }
}

/// test global allocator and vec
#[test_case]
fn vec() {
    let mut vec: Vec<u32> = Vec::with_capacity(1);
    vec.push(3);
    vec.push(5);
    vec.push(9);
    vec.push(15);

    debug!("{:?}", vec);

    assert!(vec.len() == 4);
}

pub struct TestDirectory {
    pub files: Vec<Box<dyn File>>,
    pub directories: Vec<Box<dyn Directory>>,
    pub name: String,
}

impl Directory for TestDirectory {
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
        &self.name
    }

    fn set_name(&mut self, name: &str) -> Result<(), Errno> {
        self.name = name.to_string();
        Ok(())
    }
}

pub struct TestFile {
    pub name: String,
    pub contents: Vec<u8>,
}

impl TestFile {
    pub fn new(name: &str, contents: &str) -> Self {
        Self {
            name: name.to_string(),
            contents: contents.as_bytes().to_vec(),
        }
    }
}

impl File for TestFile {
    fn get_permissions(&self) -> Permissions {
        Permissions::OwnerRead | Permissions::OwnerWrite | Permissions::GroupRead | Permissions::GroupWrite | Permissions::OtherRead
    }

    fn set_permissions(&mut self, _permissions: Permissions) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    fn write_at(&mut self, _bytes: &[u8], _offset: usize) -> Result<usize, Errno> {
        Err(Errno::NotSupported)
    }

    fn can_write_at(&self, _space: usize, _offset: usize) -> bool {
        false
    }

    fn read_at(&self, bytes: &mut [u8], offset: usize) -> Result<usize, Errno> {
        let size = if bytes.len() > self.contents.len() { self.contents.len() } else { bytes.len() };
        for i in 0..size {
            bytes[i] = self.contents[i];
        }
        Ok(size)
    }

    fn can_read_at(&self, space: usize, offset: usize) -> bool {
        (space - offset) >= self.contents.len()
    }
    
    fn truncate(&mut self, _size: usize) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }

    fn lock(&mut self, _kind: LockType, _size: isize) -> Result<(), Errno> {
        Err(Errno::NotSupported)
    }


    fn get_name(&self) -> &str {
        &self.name
    }

    fn set_name(&mut self, name: &str) -> Result<(), Errno> {
        self.name = name.to_string();
        Ok(())
    }

    fn get_size(&self) -> usize {
        self.contents.len()
    }
}
static mut TEST_DIR: Option<Box<dyn Directory>> = None;

#[test_case]
fn file_create_tree() {
    unsafe {
        TEST_DIR = Some(Box::new(TestDirectory {
            files: vec![
                Box::new(TestFile::new("testfile4", "this is testfile4")),
            ],
            directories: vec![
                Box::new(TestDirectory {
                    files: vec![
                        Box::new(TestFile::new("testfile1", "this is testfile1")),
                        Box::new(TestFile::new("testfile2", "this is testfile2")),
                    ],
                    directories: vec![
                        Box::new(TestDirectory {
                            files: vec![
                                Box::new(TestFile::new("testfile5", "this is testfile5")),
                                Box::new(TestFile::new("testfile6", "this is testfile6")),
                            ],
                            directories: vec![],
                            name: "test3".to_string(),
                        }),
                    ],
                    name: "test1".to_string(),
                }),
                Box::new(TestDirectory {
                    files: vec![
                        Box::new(TestFile::new("testfile3", "this is testfile3")),
                    ],
                    directories: vec![],
                    name: "test2".to_string(),
                }),
            ],
            name: "/".to_string(),
        }));
    }

    fn print_tree<'a>(dir: &'a Box<dyn Directory>, indent: usize) {
        let mut spaces: Vec<u8> = Vec::new();

        if indent > 0 {
            for _i in 0..indent - 2 {
                spaces.push(b' ');
            }

            spaces.push(b'-');
            spaces.push(b' ');
        }

        log!("{}{}", core::str::from_utf8(&spaces).unwrap(), dir.get_name());

        let dirs = dir.get_directories();
        for dir2 in dirs {
            print_tree(dir2, indent + 4);
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
    }

    unsafe {
        print_tree(TEST_DIR.as_ref().unwrap(), 0);
    }
}

#[test_case]
fn file_lookup() {
    unsafe {
        assert!(get_directory_from_path(TEST_DIR.as_mut().unwrap(), "test1").map(|d| d.get_name()) == Some("test1"));
        assert!(get_directory_from_path(TEST_DIR.as_mut().unwrap(), "test1/test3").map(|d| d.get_name()) == Some("test3"));

        assert!(get_file_from_path(TEST_DIR.as_mut().unwrap(), "testfile4").map(|d| d.get_name()) == Some("testfile4"));
        assert!(get_file_from_path(TEST_DIR.as_mut().unwrap(), "test2/testfile3").map(|d| d.get_name()) == Some("testfile3"));
        assert!(get_file_from_path(TEST_DIR.as_mut().unwrap(), "test1/test3/testfile6").map(|d| d.get_name()) == Some("testfile6"));
    }
}

#[test_case]
fn file_read() {
    let path = "test1/test3/testfile6";

    log!("reading file @ {}", path);

    let mut file = get_file_from_path(unsafe { TEST_DIR.as_mut().unwrap() }, path).unwrap();

    let mut buf = vec![0; file.get_size()];
    log!("file.read_at returned {:?}", file.read_at(buf.as_mut_slice(), 0));

    let string = alloc::str::from_utf8(buf.as_slice()).unwrap();

    log!("read \"{}\"", string);

    assert!(string == "this is testfile6");
}
