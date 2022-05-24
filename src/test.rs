//! tests

use core::arch::asm;
use crate::{
    mm::heap::{
        KERNEL_HEAP,
        alloc,
        alloc_aligned,
        free,
        KHEAP_INITIAL_SIZE,
        HEAP_MIN_SIZE
    },
    console::{
        ColorCode,
        get_console
    },
    fs::{
        tree::{
            File,
            Directory,
            LockType,
            get_file_from_path,
            get_directory_from_path,
        },
        vfs::Permissions,
    },
    errno::Errno,
};
use alloc::{
    boxed::Box,
    vec,
    vec::Vec,
    string::{
        String,
        ToString,
    }
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

/// test heap alloc/free
#[test_case]
fn heap_alloc_free() {
    debug!("{:?}", unsafe { KERNEL_HEAP });

    let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };

    #[cfg(debug_messages)]
    heap.print_holes();

    let heap_start = heap.index.get(0).0 as usize;

    debug!("heap start @ {:#x}", heap_start);

    let a = alloc::<u32>(8);
    let b = alloc::<u32>(8);

    debug!("a (8): {:#x}", a as usize);
    debug!("b (8): {:#x}", b as usize);

    #[cfg(debug_messages)]
    heap.print_holes();

    debug!("free a");

    free(a);

    #[cfg(debug_messages)]
    heap.print_holes();

    debug!("free b");

    free(b);

    #[cfg(debug_messages)]
    heap.print_holes();

    assert!(heap.index.size == 1);

    let c = alloc::<u32>(12);

    debug!("c (12): {:#x}", c as usize);

    assert!(c == a);

    let d = alloc::<u32>(1024);

    debug!("d (1024): {:#x}", d as usize);

    let e = alloc::<u32>(16);

    debug!("e (16): {:#x}", e as usize);

    #[cfg(debug_messages)]
    heap.print_holes();

    debug!("free c");

    free(c);

    #[cfg(debug_messages)]
    heap.print_holes();

    let f = alloc::<u32>(12);

    debug!("f (12): {:#x}", f as usize);

    #[cfg(debug_messages)]
    heap.print_holes();

    assert!(f == c);

    debug!("free e");

    free(e);

    debug!("free d");

    free(d);

    debug!("free f");

    free(f);

    assert!(heap.index.size == 1);

    #[cfg(debug_messages)]
    heap.print_holes();

    let g = alloc::<u32>(8);

    debug!("g (8): {:#x}", g as usize);

    #[cfg(debug_messages)]
    heap.print_holes();

    assert!(g == a);

    debug!("free g");
    
    free(g);

    #[cfg(debug_messages)]
    heap.print_holes();

    assert!(heap.index.size == 1);

    assert!(heap.index.get(0).0 as usize == heap_start);
}

/// test heap expand/contract
#[test_case]
fn heap_expand_contract() {
    let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };

    let heap_start = heap.index.get(0).0 as usize;
    
    let h = alloc::<u32>(2048);

    debug!("h (2048): {:#x}", h as usize);

    #[cfg(debug_messages)]
    heap.print_holes();

    debug!("size: {:#x}", heap.end_address - heap.start_address);

    let i = alloc::<u32>(KHEAP_INITIAL_SIZE);

    debug!("i ({}): {:#x}", KHEAP_INITIAL_SIZE, i as usize);

    #[cfg(debug_messages)]
    heap.print_holes();

    debug!("size: {:#x}", heap.end_address - heap.start_address);

    assert!(heap.end_address - heap.start_address > KHEAP_INITIAL_SIZE);

    debug!("free i");

    free(i);

    debug!("size: {:#x}", heap.end_address - heap.start_address);

    #[cfg(debug_messages)]
    heap.print_holes();

    assert!(heap.end_address - heap.start_address == HEAP_MIN_SIZE);

    debug!("free h");

    free(h);

    assert!(heap.index.get(0).0 as usize == heap_start);
}

/// test heap alloc alignment
#[test_case]
fn heap_alloc_align() {
    let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };
    
    for size in 1..32 {
        for i in 0..16 {
            let before = heap.index.get(0).0 as usize;
            let before_size = (unsafe { &*heap.index.get(0).0 }).size;

            debug!("before: addr @ {:#x}, size {:#x}", before, before_size);

            let alignment = 1 << i;
            let ptr = alloc_aligned::<u8>(size, alignment);
            //let ptr = alloc::<u8>(size);

            debug!("({}): {:#x} % {} == {}", size, ptr as usize, alignment, (ptr as usize) % alignment);

            #[cfg(debug_messages)]
            heap.print_holes();

            debug!("free");

            free(ptr);

            #[cfg(debug_messages)]
            heap.print_holes();

            assert!(heap.index.get(0).0 as usize == before);
            assert!((unsafe { &*heap.index.get(0).0 }).size == before_size);
        }
    }
}

/// test allocating aligned memory with existing allocation
#[test_case]
fn heap_alloc_align_2() {
    let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };

    let heap_start = heap.index.get(0).0 as usize;

    let h = alloc::<u32>(2048);

    debug!("h (2048): {:#x}", h as usize);

    for size in 1..32 {
        for i in 0..16 {
            let before = heap.index.get(0).0 as usize;
            let before_size = (unsafe { &*heap.index.get(0).0 }).size;

            debug!("before: addr @ {:#x}, size {:#x}", before, before_size);

            let alignment = 1 << i;
            let ptr = alloc_aligned::<u8>(size, alignment);

            debug!("({}): {:#x} % {} == {}", size, ptr as usize, alignment, (ptr as usize) % alignment);

            #[cfg(debug_messages)]
            heap.print_holes();

            debug!("free");
            
            free(ptr);

            #[cfg(debug_messages)]
            heap.print_holes();

            assert!(heap.index.get(0).0 as usize == before);
            assert!((unsafe { &*heap.index.get(0).0 }).size == before_size);
        }
    }

    debug!("free h");

    free(h);

    assert!(heap.index.get(0).0 as usize == heap_start);
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
fn file_lookup_pre() {
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
}

#[test_case]
fn file_lookup() {
    //#[cfg(debug_messages)]
    {
        fn print_tree<'a>(dir: &'a Box<dyn Directory>, indent: usize) {
            let mut spaces: Vec<u8> = Vec::new();

            for _i in 0..indent {
                spaces.push(b' ');
            }

            log!("{}{}", core::str::from_utf8(&spaces).unwrap(), dir.get_name());

            log!("a");
            let dirs = dir.get_directories();
            log!("b {}", dirs.len());
            let name = dirs[0].get_name();
            log!("{}", name);
            let name = dirs[1].get_name();
            log!("{}", name);
            for dir2 in dirs {
                log!("c");
                //print_tree(dir2, indent + 4);
                log!("{}", dir2.get_name());
            }

            for _i in 0..4 {
                spaces.push(b' ');
            }

            for file in dir.get_files() {
                log!("{}{}", core::str::from_utf8(&spaces).unwrap(), file.get_name());
            }
        }

        unsafe {
            get_directory_from_path(TEST_DIR.as_mut().unwrap(), "test1");
            print_tree(TEST_DIR.as_ref().unwrap(), 0);
        }
    }
}
