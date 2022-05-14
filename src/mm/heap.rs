//! heap functions, malloc, maybe global allocator?

use crate::arch::paging::kmalloc;
use core::result::Result;
use core::mem::size_of;
use core::cmp::Ordering;

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

// TODO: create wrapper for unsafe raw pointer array, will clean up this code a lot
struct OrderedArray<T> {
    array: *mut T,
    size: u32,
    max_size: u32,
    //less_than: fn(T, T) -> i8,
}

impl<T: core::cmp::PartialOrd + core::marker::Copy> OrderedArray<T> {
    /// create an ordered array and allocate memory for it
    fn new(max_size: u32) -> Self {
        // allocate mem for array
        let array = unsafe { kmalloc::<T>(max_size * (size_of::<T>() as u32), false).pointer };
        Self::place_at(array, max_size)    
    }

    /// place an ordered array at an existing location in memory
    fn place_at(addr: *mut T, max_size: u32) -> Self {
        // zero out array
        for i in 0..(max_size as isize) * (size_of::<T>() as isize) {
            unsafe {
                (*(addr as *mut u8).offset(i)) = 0;
            }
        }

        Self {
            array: addr,
            max_size,
            size: 0,
        }
    }

    /// insert an item into an ordered array
    fn insert(&mut self, item: T) -> Result<(), ()> {
        if self.size >= self.max_size {
            Err(())
        } else {
            let mut iterator = 0;
            while iterator < self.size {
                let item2 = unsafe { *self.array.offset(iterator as isize) };
                match item2.partial_cmp(&item) {
                    None | Some(Ordering::Greater) => break,
                    _ => (),
                };
                iterator += 1;
            }

            if iterator == self.size {
                self.size += 1;
                unsafe { *self.array.offset(iterator as isize) = item; }
                Ok(())
            } else {
                let mut tmp = unsafe { *self.array.offset(iterator as isize) };
                unsafe { *self.array.offset(iterator as isize) = item; }
                while iterator < self.size {
                    iterator += 1;
                    let tmp2 = unsafe { *self.array.offset(iterator as isize) };
                    unsafe { *self.array.offset(iterator as isize) = tmp; }
                    tmp = tmp2;
                }
                self.size += 1;
                Ok(())
            }
        }
    }

    /// get a reference to an item in an ordered array
    fn get(&self, index: u32) -> Option<&T> {
        if index < self.size {
            Some(unsafe { &*self.array.offset(index as isize) })
        } else {
            None
        }
    }

    /// get a mutable reference to an item in an ordered array
    fn get_mut(&mut self, index: u32) -> Option<&mut T> {
        if index < self.size {
            Some(unsafe { &mut *self.array.offset(index as isize) })
        } else {
            None
        }
    }

    /// remove an item from an ordered array
    fn remove(&mut self, index: u32) -> Result<(), ()> {
        if index < self.size {
            for i in index..self.size {
                unsafe {
                    *self.array.offset(i as isize) = *self.array.offset(i as isize + 1);
                }
            }
            self.size -= 1;
            Ok(())
        } else {
            Err(())
        }
    }
}

impl<T> core::ops::Drop for OrderedArray<T> {
    fn drop(&mut self) {
        //kfree(self.array);
        log!("dropping ordered array, can't free!");
    }
}

/// initialize heap
pub fn init() {
    let mut arr: OrderedArray<u32> = OrderedArray::new(8);
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
    log!("{:?}", arr.insert(621));
}
