//! simple ustar parser

use common::types::{file::Permissions, GroupID, UserID};
use core::{ffi::CStr, fmt, mem::size_of, str};
use generic_array::{
    typenum::{U12, U8},
    ArrayLength, GenericArray,
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

fn from_c_str(c: &[u8]) -> &str {
    match CStr::from_bytes_until_nul(c) {
        Ok(string) => string.to_str().unwrap(),
        Err(_) => core::str::from_utf8(c).unwrap(),
    }
}

impl Header {
    pub fn name(&self) -> &str {
        from_c_str(&self.name)
    }

    pub fn mode(&self) -> Permissions {
        let num: u16 = usize::from(&self.mode).try_into().unwrap();
        num.try_into().unwrap()
    }

    pub fn owner_uid(&self) -> UserID {
        UserID::from(&self.owner_uid)
    }

    pub fn owner_gid(&self) -> GroupID {
        GroupID::from(&self.owner_gid)
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
        from_c_str(&self.link_name)
    }

    pub fn ustar_indicator(&self) -> &str {
        from_c_str(&self.ustar_indicator)
    }

    pub fn ustar_version(&self) -> &str {
        from_c_str(&self.ustar_version)
    }

    pub fn owner_user_name(&self) -> &str {
        from_c_str(&self.owner_user_name)
    }

    pub fn owner_group_name(&self) -> &str {
        from_c_str(&self.owner_group_name)
    }

    pub fn device_major(&self) -> usize {
        usize::from(&self.device_major)
    }

    pub fn device_minor(&self) -> usize {
        usize::from(&self.device_minor)
    }

    pub fn filename_prefix(&self) -> &str {
        from_c_str(&self.filename_prefix)
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

impl<N: ArrayLength<u8>> TarNumber<N> {
    fn to_str(&self) -> &str {
        // get length of string. numeric values are supposed to end in either a null byte or a space and we don't want rust tripping over those values
        let length = self.data.iter().position(|c| *c == 0 || *c == 32).unwrap_or(self.data.len());

        // convert the raw bytes into a string
        core::str::from_utf8(&self.data[0..length]).unwrap()
    }
}

impl<N: ArrayLength<u8>> From<&TarNumber<N>> for usize {
    fn from(num: &TarNumber<N>) -> Self {
        Self::from_str_radix(num.to_str(), 8).unwrap_or(0) //.expect("couldn't parse numeric value")
    }
}

impl<N: ArrayLength<u8>> From<&TarNumber<N>> for u32 {
    fn from(num: &TarNumber<N>) -> Self {
        Self::from_str_radix(num.to_str(), 8).unwrap_or(0) //.expect("couldn't parse numeric value")
    }
}

impl<N: ArrayLength<u8>> fmt::Debug for TarNumber<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value: usize = usize::from(self);
        f.debug_struct("TarNumber").field("value", &value).field("raw", &self.data).finish()
    }
}

/// type of file that can be stored in a tar archive
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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
        Self { data, offset: 0 }
    }

    pub fn recreate(&self) -> Self {
        Self::new(self.data)
    }
}

impl<'a> Iterator for TarIterator<'a> {
    type Item = TarEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        // spit out first header
        if self.offset >= self.data.len() || self.offset + size_of::<Header>() > self.data.len() {
            // make sure we don't overflow the buffer
            None
        } else {
            let header = unsafe { &*(&self.data[self.offset] as *const _ as *const Header) }; // pointer magic (:

            // trace!("got header {:?}", header);

            if header.name().is_empty() {
                None
            } else {
                let file_size = header.file_size();

                let contents_offset = if file_size == 0 {
                    self.offset + size_of::<Header>() // dont bother aligning to nearest block if there's no contents, as it just screws things up
                } else {
                    ((self.offset + size_of::<Header>()) & !(BLOCK_SIZE - 1)) + BLOCK_SIZE
                };
                let contents_end = contents_offset + file_size;

                self.offset = (contents_end & !(BLOCK_SIZE - 1)) + BLOCK_SIZE;

                Some(TarEntry {
                    header,
                    contents: &self.data[contents_offset..contents_end],
                })
            }
        }
    }
}
