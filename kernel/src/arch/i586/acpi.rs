//! acpi and acpi accessories

use alloc::vec::Vec;
use core::{mem::size_of, slice};

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SDTHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: u32,
    pub creator_revision: u32,
}

pub trait SDTPointer {}

impl SDTPointer for u32 {}
impl SDTPointer for u64 {}

pub struct SDT<S: SDTPointer + Clone> {
    pub header: SDTHeader,
    pub sdt_pointers: Vec<S>,
}

impl<S: SDTPointer + Clone> SDT<S> {
    pub unsafe fn from_raw_pointer(ptr: *const u8) -> Self {
        let header = *(ptr as *const SDTHeader);
        let num_sdt_pointers = (header.length as usize - size_of::<SDTHeader>()) / size_of::<S>();
        let sdt_pointers = slice::from_raw_parts(ptr.add(size_of::<SDTHeader>()) as *const S, num_sdt_pointers).to_vec();

        Self { header, sdt_pointers }
    }

    pub fn verify_checksum(&self) -> bool {
        let mut sum: u8 = 0;

        let header = unsafe { slice::from_raw_parts(&self.header as *const _ as *const u8, size_of::<SDTHeader>()) };

        for byte in header.iter() {
            sum = sum.wrapping_add(*byte);
        }

        let contents = unsafe { slice::from_raw_parts(self.sdt_pointers.as_ptr() as *const _ as *const u8, self.sdt_pointers.len() * size_of::<S>()) };

        for byte in contents.iter() {
            sum = sum.wrapping_add(*byte);
        }

        sum == 0
    }
}
