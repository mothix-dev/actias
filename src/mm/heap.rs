//! heap functions, malloc, maybe global allocator?

use crate::util::array::OrderedArray;

#[repr(C)]
struct Header {
    magic: u32,
    is_hole: bool,
    size: u32,
}

#[repr(C)]
struct Footer {
    magic: u32,
    header: *mut Header,
}

const MAGIC_NUMBER: u32 = 0xdeadbeef; // TODO: more interesting magic number lmao

/// initialize heap
pub fn init() {
    /*let mut arr: OrderedArray<u32> = OrderedArray::new(8);
    arr.insert(15);
    arr.insert(37);
    arr.insert(8);
    arr.insert(10);
    arr.insert(3);
    arr.insert(4);
    arr.insert(17);
    arr.insert(58);
    for i in 0..arr.max_size {
        log!("{:?}", arr.get(i));
    }
    arr.insert(621);*/
}
