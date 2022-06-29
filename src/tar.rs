//! simple ustar parser

use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{
    ffi::CStr,
    fmt,
    str,
    mem::size_of,
};
use generic_array::{
    ArrayLength, GenericArray,
    typenum::{U8, U12},
};
use crate::{
    fs::{
        tree::{File, Directory, SymLink, get_directory_from_path},
        vfs::Permissions,
        basename, dirname,
    },
    types::Errno,
};

const BLOCK_SIZE: usize = 512;

/// header of a file in a tar archive. contains many kinds of information about the file
#[repr(C)]
pub struct Header {
    name: [u8; 100],
    mode: TarNumber<U8>,
    owner_uid: TarNumber<U8>,
    owner_gid: TarNumber<U8>,
    file_size: TarNumber<U12>,
    mod_time: TarNumber<U12>,
    checksum: TarNumber<U8>,
    kind: EntryKind,
    link_name: [u8; 100],
    ustar_indicator: [u8; 6],
    ustar_version: [u8; 2],
    owner_user_name: [u8; 32],
    owner_group_name: [u8; 32],
    device_major: TarNumber<U8>,
    device_minor: TarNumber<U8>,
    filename_prefix: [u8; 155],
}

impl Header {
    pub fn name(&self) -> &str {
        CStr::from_bytes_until_nul(&self.name).unwrap().to_str().unwrap()
    }

    pub fn mode(&self) -> Permissions {
        let num: u16 = usize::from(&self.mode).try_into().unwrap();
        num.try_into().unwrap()
    }

    pub fn owner_uid(&self) -> usize {
        usize::from(&self.owner_uid)
    }

    pub fn owner_gid(&self) -> usize {
        usize::from(&self.owner_gid)
    }

    pub fn file_size(&self) -> usize {
        usize::from(&self.file_size)
    }

    pub fn mod_time(&self) -> usize {
        usize::from(&self.mod_time)
    }

    pub fn checksum(&self) -> usize {
        usize::from(&self.checksum)
    }

    pub fn kind(&self) -> EntryKind {
        self.kind
    }

    pub fn link_name(&self) -> &str {
        CStr::from_bytes_until_nul(&self.link_name).unwrap().to_str().unwrap()
    }

    pub fn ustar_indicator(&self) -> &str {
        CStr::from_bytes_until_nul(&self.ustar_indicator).unwrap().to_str().unwrap()
    }

    pub fn ustar_version(&self) -> &str {
        str::from_utf8(&self.ustar_version).unwrap()
    }

    pub fn owner_user_name(&self) -> &str {
        CStr::from_bytes_until_nul(&self.owner_user_name).unwrap().to_str().unwrap()
    }

    pub fn owner_group_name(&self) -> &str {
        CStr::from_bytes_until_nul(&self.owner_group_name).unwrap().to_str().unwrap()
    }

    pub fn device_major(&self) -> usize {
        usize::from(&self.device_major)
    }

    pub fn device_minor(&self) -> usize {
        usize::from(&self.device_minor)
    }

    pub fn filename_prefix(&self) -> &str {
        CStr::from_bytes_until_nul(&self.filename_prefix).unwrap().to_str().unwrap()
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Header")
            .field("name", &self.name())
            .field("mode", &self.mode())
            .field("owner_uid", &self.owner_uid())
            .field("owner_gid", &self.owner_gid())
            .field("file_size", &self.file_size())
            .field("mod_time", &self.mod_time())
            .field("checksum", &self.checksum())
            .field("kind", &self.kind())
            .field("link_name", &self.link_name())
            .field("ustar_indicator", &self.ustar_indicator())
            .field("ustar_version", &self.ustar_version())
            .field("owner_user_name", &self.owner_user_name())
            .field("owner_group_name", &self.owner_group_name())
            .field("device_major", &self.device_major())
            .field("device_minor", &self.device_minor())
            .field("filename_prefix", &self.filename_prefix())
            .finish()
    }
}

/// representation of a number in a tar file
struct TarNumber<N: ArrayLength<u8>> {
    data: GenericArray<u8, N>,
}

impl<N: ArrayLength<u8>> From<&TarNumber<N>> for usize {
    fn from(num: &TarNumber<N>) -> usize {
        // get length of string. numeric values are supposed to end in either a null byte or a space and we don't want rust tripping over those values
        let length = (|| {
            for i in 0..num.data.len() {
                let char = num.data[i];

                if char == 0 || char == 32 {
                    return i;
                }
            }
            num.data.len()
        })();

        // convert the raw bytes into a string
        let str = core::str::from_utf8(&num.data[0..length]).unwrap();

        // convert the string into a number
        Self::from_str_radix(str, 8).expect("couldn't parse numeric value")
    }
}

impl<N: ArrayLength<u8>> fmt::Debug for TarNumber<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value: usize = usize::from(self);
        f.debug_struct("TarNumber")
            .field("value", &value)
            .field("raw", &self.data)
            .finish()
    }
}

/// type of file that can be stored in a tar archive
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum EntryKind {
    NormalFile = 48,
    HardLink = 49,
    SymLink = 50,
    CharSpecial = 51,
    BlockSpecial = 52,
    Directory = 53,
    FIFO = 54,
    ContiguousFile = 55,
    VendorSpecificA = 65,
    VendorSpecificB = 66,
    VendorSpecificC = 67,
    VendorSpecificD = 68,
    VendorSpecificE = 69,
    VendorSpecificF = 70,
    VendorSpecificG = 71,
    VendorSpecificH = 72,
    VendorSpecificI = 73,
    VendorSpecificJ = 74,
    VendorSpecificK = 75,
    VendorSpecificL = 76,
    VendorSpecificM = 77,
    VendorSpecificN = 78,
    VendorSpecificO = 79,
    VendorSpecificP = 80,
    VendorSpecificQ = 81,
    VendorSpecificR = 82,
    VendorSpecificS = 83,
    VendorSpecificT = 84,
    VendorSpecificU = 85,
    VendorSpecificV = 86,
    VendorSpecificW = 87,
    VendorSpecificX = 88,
    VendorSpecificY = 89,
    VendorSpecificZ = 90,
    GlobalExtendedHeader = 103,
    ExtendedHeaderNext = 120,
}

/// entry in a tar file, as returned by TarIterator
#[derive(Debug)]
pub struct TarEntry<'a> {
    pub header: &'a Header,
    pub contents: &'a [u8],
}

/// struct to enable iterating over a tar file
#[derive(Debug)]
pub struct TarIterator<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> TarIterator<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            offset: 0,
        }
    }

    pub fn recreate(&self) -> Self {
        Self::new(self.data)
    }
}

impl<'a> Iterator for TarIterator<'a> {
    type Item = TarEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        // spit out first header
        if self.offset >= self.data.len() || self.offset + size_of::<Header>() > self.data.len() { // make sure we don't overflow the buffer
            None
        } else {
            let header = unsafe { &*(((&self.data[self.offset] as *const _) as usize) as *const Header) }; // pointer magic (:

            if header.name().is_empty() {
                None
            } else {
                let file_size = header.file_size();

                let contents_offset = 
                    if file_size == 0 {
                        self.offset + size_of::<Header>() // dont bother aligning to nearest block if there's no contents, as it just screws things up
                    } else {
                        ((self.offset + size_of::<Header>()) & !(BLOCK_SIZE - 1)) + BLOCK_SIZE
                    };
                let contents_end = contents_offset + file_size;

                self.offset = (contents_end & !(BLOCK_SIZE - 1)) + BLOCK_SIZE;

                Some(TarEntry {
                    header,
                    contents: &self.data[contents_offset..contents_end]
                })
            }
        }
    }
}

pub struct TarDirectory {
    files: Vec<Box<dyn File>>,
    directories: Vec<Box<dyn Directory>>,
    links: Vec<Box<dyn SymLink>>,
    permissions: Permissions,
    name: String,
}

impl Directory for TarDirectory {
    fn get_permissions(&self) -> Permissions {
        self.permissions
    }

    fn set_permissions(&mut self, _permissions: Permissions) -> Result<(), Errno> {
        Err(Errno::ReadOnlyFileSystem)
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

    fn set_name(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::ReadOnlyFileSystem)
    }
}

pub struct TarFile {
    name: String,
    permissions: Permissions,
    contents: &'static [u8],
}

impl File for TarFile {
    fn get_permissions(&self) -> Permissions {
        self.permissions
    }

    fn set_permissions(&mut self, _permissions: Permissions) -> Result<(), Errno> {
        Err(Errno::ReadOnlyFileSystem)
    }

    fn write_at(&mut self, _bytes: &[u8], _offset: usize) -> Result<usize, Errno> {
        Err(Errno::ReadOnlyFileSystem)
    }

    fn can_write_at(&self, _space: usize, _offset: usize) -> bool {
        false
    }

    fn read_at(&self, bytes: &mut [u8], _offset: usize) -> Result<usize, Errno> {
        let size = if bytes.len() > self.contents.len() { self.contents.len() } else { bytes.len() };
        bytes[..size].copy_from_slice(&self.contents[..size]);
        Ok(size)
    }

    fn can_read_at(&self, space: usize, offset: usize) -> bool {
        (space - offset) >= self.contents.len()
    }
    
    fn truncate(&mut self, _size: usize) -> Result<(), Errno> {
        Err(Errno::ReadOnlyFileSystem)
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn set_name(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::ReadOnlyFileSystem)
    }

    fn get_size(&self) -> usize {
        self.contents.len()
    }
}

pub struct TarLink {
    name: String,
    permissions: Permissions,
    target: String,
}

impl SymLink for TarLink {
    fn get_permissions(&self) -> Permissions {
        self.permissions
    }

    fn set_permissions(&mut self, _permissions: Permissions) -> Result<(), Errno> {
        Err(Errno::ReadOnlyFileSystem)
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn set_name(&mut self, _name: &str) -> Result<(), Errno> {
        Err(Errno::ReadOnlyFileSystem)
    }

    fn get_target(&self) -> &str {
        &self.target
    }

    fn set_target(&mut self, _target: &str) -> Result<(), Errno> {
        Err(Errno::ReadOnlyFileSystem)
    }
}

/// makes a directory in the vfs
fn mkdir(root: &mut Box<dyn Directory>, path: &str, permissions: Permissions) {
    let elements = path.split('/').collect::<Vec<_>>();

    fn make_dir(root: &mut Box<dyn Directory>, elements: &Vec<&str>, extent: usize, permissions: Permissions) {
        if extent > elements.len() {
            return;
        }

        let mut partial = elements[0..extent].to_vec();

        let dirname = partial.pop().unwrap().to_string();

        let path = partial.join("/");

        let dir = get_directory_from_path(root, &path).unwrap();

        let should_make = || {
            for dir2 in dir.get_directories() {
                if dir2.get_name() == dirname {
                    return false;
                }
            }
            true
        };

        if !dirname.is_empty() && should_make() {
            dir.get_directories_mut().push(Box::new(TarDirectory {
                files: Vec::new(),
                directories: Vec::new(),
                links: Vec::new(),
                permissions,
                name: dirname,
            }));
        }

        make_dir(root, elements, extent + 1, permissions);
    }

    make_dir(root, &elements, 1, permissions);
}

/// consumes the provided iterator, turning it into a tree representation
pub fn make_tree(iter: TarIterator<'static>) -> Box<dyn Directory> {
    let mut root: Box<dyn Directory> = Box::new(TarDirectory {
        files: Vec::new(),
        directories: Vec::new(),
        links: Vec::new(),
        permissions: Permissions::None,
        name: "".to_string(),
    });

    let data = iter.data;

    for entry in iter {
        let name = format!("{}{}", entry.header.filename_prefix(), entry.header.name());
        //log!("{:?}, {:?}", entry.header.kind(), name);
        match entry.header.kind() {
            EntryKind::Directory => mkdir(&mut root, &name, entry.header.mode()),
            EntryKind::NormalFile | EntryKind::BlockSpecial | EntryKind::CharSpecial |
            EntryKind::FIFO | EntryKind::ContiguousFile => {
                let dirname = &dirname(&name);

                // make sure parent dir exists
                mkdir(&mut root, dirname, entry.header.mode());

                // get parent directory of file
                let dir = get_directory_from_path(&mut root, dirname).unwrap();

                // add file to parent directory
                dir.get_files_mut().push(Box::new(TarFile {
                    name: basename(&name).unwrap().to_string(),
                    permissions: entry.header.mode(),
                    contents: entry.contents,
                }));
            },
            EntryKind::HardLink => {
                let link_name = entry.header.link_name();

                let dirname = &dirname(&name);
                let dir = get_directory_from_path(&mut root, dirname).unwrap();

                if basename(link_name).is_some() {
                    for entry2 in TarIterator::new(data) {
                        let name2 = format!("{}{}", entry2.header.filename_prefix(), entry2.header.name());
                        if name2 == link_name {
                            dir.get_files_mut().push(Box::new(TarFile {
                                name: basename(&name).unwrap().to_string(),
                                permissions: entry.header.mode(),
                                contents: entry2.contents,
                            }));
                            break;
                        }
                    }
                } else {
                    log!("dir hard links not supported");
                }
            },
            EntryKind::SymLink => {
                let link_name = entry.header.link_name();
                //log!("{}: symlink -> {}", name, link_name);

                let dirname = &dirname(&name);

                // make sure parent dir exists
                mkdir(&mut root, dirname, entry.header.mode());

                // get parent directory of link
                let dir = get_directory_from_path(&mut root, dirname).unwrap();

                // add link to parent directory
                dir.get_links_mut().push(Box::new(TarLink {
                    name: basename(&name).unwrap().to_string(),
                    permissions: entry.header.mode(),
                    target: link_name.to_string(),
                }));
            },
            _ => (),
        }
    }

    root
}
