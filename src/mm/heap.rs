//! heap functions, malloc, maybe global allocator?

use crate::arch::paging::kmalloc;
use core::result::Result;
use core::mem::size_of;

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

// TODO: replace this with something reasonable
struct OrderedArray<T> {
    array: *mut T,
    size: u32,
    max_size: u32,
    //less_than: fn(T, T) -> i8,
}

impl<T: core::cmp::PartialOrd> OrderedArray<T> {
    fn create(max_size: u32) -> Self {
        // allocate mem for array and 
        let array = unsafe { kmalloc::<T>(max_size * (size_of::<T>() as u32), false).pointer };
        Self::place_at(array, max_size)    
    }

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

    fn destroy(&self) {
        //kfree(self.array);
    }

    fn insert(&mut self, item: T) -> Result<(), ()> {
        let mut iterator = 0;
        while iterator < self.size {
            let item2 = unsafe { &*self.array.offset(iterator as isize) };
            if !(item2 < &item) {
                break;
            }
            iterator += 1;
        }

        if iterator == self.size {
            if self.size >= self.max_size {
                Err(())
            } else {
                self.size += 1;
                unsafe { *self.array.offset(iterator as isize) = item; }
                Ok(())
            }
        } else {
            // ...
            Err(())
        }
    }

    fn get(&self, index: u32) -> Option<&T> {
        // ...
        None
    }

    fn get_mut(&mut self, index: u32) -> Option<&mut T> {
        // ...
        None
    }

    fn remove(&mut self, index: u32) -> Result<(), ()> {
        // ...
        Err(())
    }
}
