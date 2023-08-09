//! array utilities

use alloc::{boxed::Box, vec::Vec};
use core::fmt;

use crate::mm::ContiguousRegion;

/// simple bitset, acts sorta like an array but you access single bits
#[repr(C)]
pub struct BitSet {
    /// array of bytes that the bitset uses
    pub array: Box<[u32]>,

    /// amount of bits we can set
    pub size: usize,

    /// amount of bits we have set
    pub bits_used: usize,
}

impl BitSet {
    /// creates a bitset and allocates memory for it
    pub fn new(size: usize) -> Result<Self, alloc::collections::TryReserveError> {
        let mut array = Vec::new();
        let u32_size = (size + 31) / 32;
        array.try_reserve_exact(u32_size)?;
        array.resize(u32_size, 0);

        Ok(Self {
            array: array.into_boxed_slice(), // always round up
            size,
            bits_used: 0,
        })
    }

    /// sets a bit in the set
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

    /// aligns the given region to the given page size, then sets bits in the set accordingly.
    /// bits will be set so that the bit corresponding to the page containing any given address in the region will be set
    pub fn set_region(&mut self, region: ContiguousRegion<usize>, page_size: usize) {
        let region = region.align_covering(page_size);
        let start = region.base / page_size;
        let end = start + region.length / page_size;

        for i in start..end {
            self.set(i);
        }
    }

    /// clears a bit in the set
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

    /// aligns the given region to the given page size, then clears bits in the set accordingly
    /// bits will be set so that the bit corresponding to the page containing any given address in the region will be set
    pub fn clear_region(&mut self, region: ContiguousRegion<usize>, page_size: usize) {
        let region = region.align_inside(page_size);
        let start = region.base / page_size;
        let end = start + region.length / page_size;

        for i in start..end {
            self.clear(i);
        }
    }

    /// clears all the bits in the set
    pub fn clear_all(&mut self) {
        for i in 0..self.array.len() {
            self.array[i] = 0;
        }
        self.bits_used = 0;
    }

    /// sets all the bits in the set
    pub fn set_all(&mut self) {
        for i in 0..self.array.len() {
            self.array[i] = 0xffffffff;
        }
        self.bits_used = self.size;
    }

    /// checks if bit is set
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
        for i in 0..self.array.len() {
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
            write!(f, "{}", self.test(i) as u8)?;
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

pub struct ConsistentIndexArray<T> {
    array: Vec<Option<T>>,
    bit_set: VecBitSet,
}

impl<T> ConsistentIndexArray<T> {
    pub const fn new() -> Self {
        Self {
            array: Vec::new(),
            bit_set: VecBitSet::new(),
        }
    }

    pub fn add(&mut self, item: T) -> Result<usize, alloc::collections::TryReserveError> {
        let index = self.bit_set.first_unset();
        self.set(index, item)?;
        Ok(index)
    }

    pub fn set(&mut self, index: usize, item: T) -> Result<(), alloc::collections::TryReserveError> {
        if index >= self.array.len() {
            self.array.try_reserve(self.array.len() - index)?;

            while index >= self.array.len() {
                self.array.push(None);
            }
        }

        self.array[index] = Some(item);
        self.bit_set.set(index);

        Ok(())
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.array.get(index).and_then(|i| i.as_ref())
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.array.get_mut(index).and_then(|i| i.as_mut())
    }

    pub fn remove(&mut self, index: usize) -> Option<T> {
        let mut item = None;

        if index < self.array.len() {
            item = self.array[index].take();
            self.bit_set.clear(index);
        }

        while !self.array.is_empty() && self.array[self.array.len() - 1].is_none() {
            self.array.pop();
        }

        item
    }

    pub fn clear(&mut self) {
        self.bit_set.clear_all();
        self.array.clear();
    }

    pub fn num_entries(&self) -> usize {
        self.bit_set.bits_used
    }
}

impl<T> Default for ConsistentIndexArray<T> {
    fn default() -> Self {
        Self::new()
    }
}
