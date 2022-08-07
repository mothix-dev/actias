//! array utilities

use core::{
    alloc::Layout,
    fmt,
    mem::size_of,
    ops::{Drop, Index, IndexMut},
    slice,
};

use alloc::{
    alloc::{alloc, dealloc},
    vec::Vec,
};

/// raw pointer as array of unknown size
#[derive(Debug)]
pub struct RawPtrArray<T> {
    /// raw pointer to memory
    array: *mut T,

    /// layout used when allocating our array, optional since we may not have allocated
    layout: Option<Layout>,

    /// size of array
    pub size: usize,
}

impl<T> RawPtrArray<T> {
    /// create new raw ptr array and allocate memory for it
    pub fn new(size: usize) -> Self {
        // get alignment for type
        let align = Layout::new::<T>().align();

        // create layout to allocate with
        let layout = Layout::from_size_align(size * size_of::<T>(), align).unwrap();

        // allocate memory
        let array = unsafe { alloc(layout) };

        // zero out array
        for i in 0..size * size_of::<T>() {
            unsafe {
                (*array.add(i)) = 0;
            }
        }

        Self {
            array: array as *mut T,
            layout: Some(layout),
            size,
        }
    }

    /// create new raw pointer array at provided address
    pub fn place_at(addr: *mut T, size: usize) -> Self {
        // zero out array
        for i in 0..size * size_of::<T>() {
            unsafe {
                (*(addr as *mut u8).add(i)) = 0;
            }
        }

        Self { array: addr, layout: None, size }
    }

    /// returns a slice representation of this array
    pub fn to_slice(&self) -> &[T] {
        unsafe { slice::from_raw_parts(self.array, self.size) }
    }

    /// returns a mutable slice representation of this array
    pub fn to_slice_mut(&mut self) -> &mut [T] {
        unsafe { slice::from_raw_parts_mut(self.array, self.size) }
    }
}

impl<T> Index<usize> for RawPtrArray<T> {
    type Output = T;
    fn index(&self, i: usize) -> &T {
        if i >= self.size {
            panic!("attempted to index outside of array");
        }
        unsafe { &*self.array.add(i) }
    }
}

impl<T> IndexMut<usize> for RawPtrArray<T> {
    fn index_mut(&mut self, i: usize) -> &mut T {
        if i >= self.size {
            panic!("attempted to index outside of array");
        }
        unsafe { &mut *self.array.add(i) }
    }
}

impl<T> Drop for RawPtrArray<T> {
    fn drop(&mut self) {
        // deallocate our memory if necessary
        if let Some(layout) = self.layout {
            unsafe {
                dealloc(self.array as *mut u8, layout);
            }
        }
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
        if addr >= self.size {
            return;
        }

        let idx = addr / 32;
        let off = addr % 32;

        if (self.array[idx] & 1 << off) == 0 {
            // if bit is unset, increment bits_used and set bit
            self.bits_used += 1;
            self.array[idx] |= 1 << off;
        }
    }

    /// clear a bit in the set
    pub fn clear(&mut self, addr: usize) {
        if addr >= self.size {
            return;
        }

        let idx = addr / 32;
        let off = addr % 32;

        if (self.array[idx] & 1 << off) > 0 {
            // if bit is set, decrement bits_used and clear bit
            self.bits_used -= 1;
            self.array[idx] &= !(1 << off);
        }
    }

    /// clear all the bits in the set
    pub fn clear_all(&mut self) {
        for i in 0..self.array.size {
            self.array[i] = 0;
        }
        self.bits_used = 0;
    }

    /// check if bit is set
    pub fn test(&self, addr: usize) -> bool {
        if addr < self.size {
            let idx = addr / 32;
            let off = addr % 32;
            (self.array[idx] & 1 << off) > 0
        } else {
            false
        }
    }

    /// gets first unset bit
    pub fn first_unset(&self) -> Option<usize> {
        for i in 0..self.array.size {
            let f = self.array[i];
            if f != 0xffffffff {
                // only test individual bits if there are bits to be tested
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

impl fmt::Debug for BitSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        for i in 0..self.size {
            write!(f, "{}", if self.test(i) { 1 } else { 0 })?;
        }

        Ok(())
    }
}

/// simple bitset that uses vec internally, dynamic size
#[derive(Clone)]
pub struct VecBitSet {
    /// array of bytes that the bitset uses
    pub array: Vec<u32>,

    /// amount of bits we have set
    pub bits_used: usize,
}

impl VecBitSet {
    /// create a bitset and allocate memory for it
    pub const fn new() -> Self {
        Self {
            array: Vec::new(), // always round up
            bits_used: 0,
        }
    }

    /// set a bit in the set
    pub fn set(&mut self, addr: usize) {
        let idx = addr / 32;
        let off = addr % 32;

        if idx >= self.array.len() {
            // grow vec if necessary
            for _i in 0..=self.array.len() - idx {
                self.array.push(0);
            }
        }

        if (self.array[idx] & 1 << off) == 0 {
            // if bit is unset, increment bits_used and set bit
            self.bits_used += 1;
            self.array[idx] |= 1 << off;
        }
    }

    /// clear a bit in the set
    pub fn clear(&mut self, addr: usize) {
        let idx = addr / 32;
        let off = addr % 32;

        if idx < self.array.len() && (self.array[idx] & 1 << off) > 0 {
            // if bit is set, decrement bits_used and clear bit
            self.bits_used -= 1;
            self.array[idx] &= !(1 << off);
        }
    }

    /// clear all the bits in the set
    pub fn clear_all(&mut self) {
        self.array.clear();
        self.bits_used = 0;
    }

    /// check if bit is set
    pub fn test(&self, addr: usize) -> bool {
        let idx = addr / 32;
        let off = addr % 32;

        if idx >= self.array.len() {
            false
        } else {
            (self.array[idx] & 1 << off) > 0
        }
    }

    /// gets first unset bit
    pub fn first_unset(&self) -> usize {
        for i in 0..self.array.len() {
            let f = self.array[i];
            if f != 0xffffffff {
                // only test individual bits if there are bits to be tested
                for j in 0..32 {
                    let bit = 1 << j;
                    if f & bit == 0 {
                        return i * 32 + j;
                    }
                }
            }
        }
        self.array.len() * 32
    }
}
