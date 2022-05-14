//! array utilities

use crate::arch::paging::kmalloc;
use core::mem::size_of;
use core::ops::{Index, IndexMut, Drop};
use core::cmp::{Ordering, PartialOrd};
use core::marker::Copy;

/// raw pointer as array of unknown size
pub struct RawPtrArray<T> {
    /// raw pointer to memory
    array: *mut T,

    /// size of array
    pub size: usize,
}

impl<T> RawPtrArray<T> {
    /// create new raw ptr array and allocate memory for it
    pub fn new(size: usize) -> Self {
        // allocate memory
        let array = unsafe { kmalloc::<T>(size * size_of::<T>(), false).pointer };

        Self::place_at(array, size)
    }

    /// create new raw pointer array at provided address
    pub fn place_at(addr: *mut T, size: usize) -> Self {
        // zero out array
        for i in 0..(size as isize) * (size_of::<T>() as isize) {
            unsafe {
                (*(addr as *mut u8).offset(i)) = 0;
            }
        }

        Self {
            array: addr,
            size,
        }
    }
}

impl<T> Index<usize> for RawPtrArray<T> {
    type Output = T;
    fn index(&self, i: usize) -> &T {
        if i >= self.size {
            panic!("attempted to index outside of array");
        }
        unsafe { &*self.array.offset(i as isize) }
    }
}

impl<T> IndexMut<usize> for RawPtrArray<T> {
    fn index_mut(&mut self, i: usize) -> &mut T {
        if i >= self.size {
            panic!("attempted to index outside of array");
        }
        unsafe { &mut *self.array.offset(i as isize) }
    }
}

impl<T> Drop for RawPtrArray<T> {
    fn drop(&mut self) {
        //kfree(self.array);
        log!("dropping raw array, can't free!");
    }
}

/// simple ordered array
pub struct OrderedArray<T> {
    /// array we use internally
    pub array: RawPtrArray<T>,

    /// how many items we have in the array
    pub size: usize,

    /// how many items we can have in the array
    pub max_size: usize,
}

impl<T: PartialOrd + Copy> OrderedArray<T> {
    /// create an ordered array and allocate memory for it
    pub fn new(max_size: usize) -> Self {
        Self {
            array: RawPtrArray::new(max_size),
            max_size,
            size: 0,
        }
    }

    /// place an ordered array at an existing location in memory
    pub fn place_at(addr: *mut T, max_size: usize) -> Self {
        Self {
            array: RawPtrArray::place_at(addr, max_size),
            max_size,
            size: 0,
        }
    }

    /// insert an item into an ordered array
    pub fn insert(&mut self, item: T) {
        if self.size >= self.max_size {
            panic!("attempted to insert into full ordered array"); // should we panic here or return Err?
        } else {
            let mut iterator = 0;
            while iterator < self.size {
                let item2 = self.array[iterator];
                match item2.partial_cmp(&item) {
                    None | Some(Ordering::Greater) => break,
                    _ => (),
                };
                iterator += 1;
            }

            if iterator == self.size {
                self.size += 1;
                self.array[iterator] = item;
            } else {
                let mut tmp = self.array[iterator];
                self.array[iterator] = item;
                while iterator < self.size {
                    iterator += 1;
                    let tmp2 = self.array[iterator];
                    self.array[iterator] = tmp;
                    tmp = tmp2;
                }
                self.size += 1;
            }
        }
    }

    /// get a reference to an item in an ordered array
    pub fn get(&self, index: usize) -> &T {
        if index < self.size {
            self.array.index(index)
        } else {
            panic!("attempted to index outside ordered array");
        }
    }

    /// get a mutable reference to an item in an ordered array
    pub fn get_mut(&mut self, index: usize) -> &mut T {
        if index < self.size {
            self.array.index_mut(index)
        } else {
            panic!("attempted to index outside ordered array");
        }
    }

    /// remove an item from an ordered array
    pub fn remove(&mut self, index: usize) {
        if index < self.size {
            for i in index..self.size {
                self.array[i] = self.array[i + 1];
            }
            self.size -= 1;
        } else {
            panic!("attempted to remove outside ordered array");
        }
    }
}

impl<T: PartialOrd + Copy> Index<usize> for OrderedArray<T> {
    type Output = T;
    fn index(&self, i: usize) -> &T {
        self.get(i)
    }
}

impl<T: PartialOrd + Copy> IndexMut<usize> for OrderedArray<T> {
    fn index_mut(&mut self, i: usize) -> &mut T {
        self.get_mut(i)
    }
}

/// simple bitset, acts sorta like an array but you access single bits
pub struct BitSet {
    /// array of bytes that the bitset uses
    pub array: RawPtrArray<u32>,

    /// amount of bits we can set
    pub size: usize,

    /// amount of bits we have set
    pub bits_used: usize,
}

impl BitSet {
    /// create a bitset and allocate memory for it
    pub fn new(size: usize) -> Self {
        Self {
            array: RawPtrArray::new((size + 31) / 32), // always round up
            size,
            bits_used: 0,
        }
    }

    /// place a bitset at an existing location in memory
    pub fn place_at(addr: *mut u32, size: usize) -> Self {
        Self {
            array: RawPtrArray::place_at(addr, (size + 31) / 32),
            size,
            bits_used: 0,
        }
    }

    /// set a bit in the set
    pub fn set(&mut self, addr: usize) {
        let idx = addr / 32; // TODO: maybe replace with bitwise to improve speed? does it even matter on x86?
        let off = addr % 32;

        if (self.array[idx] & 1 << off) == 0 { // if bit is unset, increment bits_used and set bit
            self.bits_used += 1;
            self.array[idx] |= 1 << off;
        }
    }

    /// clear a bit in the set
    pub fn clear(&mut self, addr: usize) {
        let idx = addr / 32;
        let off = addr % 32;

        if (self.array[idx] & 1 << off) > 0 { // if bit is set, decrement bits_used and clear bit
            self.bits_used -= 1;
            self.array[idx] &= !(1 << off);
        }
    }

    /// check if bit is set
    pub fn test(&self, addr: usize) -> bool {
        let idx = addr / 32;
        let off = addr % 32;
        (self.array[idx] & 1 << off) > 0
    }

    /// gets first unset bit
    pub fn first_unset(&self) -> Option<usize> {
        for i in 0..self.array.size {
            let f = self.array[i];
            if f != 0xffffffff { // only test individual bits if there are bits to be tested
                for j in 0..32 {
                    let bit = 1 << j;
                    if f & bit == 0 {
                        return Some(i * 32 + j);
                    }
                }
            }
        }
        None
    }
}
