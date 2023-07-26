use crate::arch::PhysicalAddress;
use core::fmt;

/// describes a region of physical memory and its use
#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct MemoryRegion {
    /// the base address of this region
    pub base: PhysicalAddress,

    /// the length of this region
    pub length: PhysicalAddress,

    /// how this region should be treated
    pub kind: MemoryKind,
}

/*
impl MemoryRegion {
    /// aligns this region to the specified alignment. alignment must be a power of two and greater than zero, otherwise behavior is undefined
    ///
    /// if the region is marked as available, the resulting region will fit within the boundaries of the given region.
    /// if the region is not, the given region will fit within the boundaries of the resulting region.
    /// this is done to ensure any reserved or bad memory isn't accidentally used
    pub fn align(&self, alignment: u64) -> Self {
        let align_inv = !(alignment - 1);

        match self.kind {
            MemoryKind::Available => Self {
                start: ((self.start - 1) & align_inv) + alignment, // round up
                end: self.end & align_inv,                         // round down
                kind: self.kind,
            },
            MemoryKind::Bad | MemoryKind::Reserved | MemoryKind::ReservedReclaimable => Self {
                start: self.start & align_inv,                 // round down
                end: ((self.end - 1) & align_inv) + alignment, // round up
                kind: self.kind,
            },
        }
    }
}*/

impl fmt::Debug for MemoryRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryRegion")
            .field("base", &crate::FormatHex(self.base))
            .field("length", &crate::FormatHex(self.length))
            .field("kind", &self.kind)
            .finish()
    }
}

/// describes what a region of memory is to be used for
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u32)]
pub enum MemoryKind {
    Bad = 0,
    Reserved,
    ReservedReclaimable,
    Available,
}

/*
/// sorts the given memory regions, aligns them to the specified alignment, and fixes overlapping regions
///
/// *see also: [MemoryRegion::align]*
pub fn sort_regions<T: AsRef<[MemoryRegion]>>(regions: T, alignment: u64) -> Vec<MemoryRegion> {
    let mut regions = regions.as_ref().to_vec();

    // sort the regions so that we can easily slice
    regions.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());

    let mut new_regions = Vec::new();

    // get a peekable iterator over the regions
    let mut regions_iter = regions.iter().peekable();

    // loop over all the regions, looking at the next region if possible for comparisons
    while let Some(region) = regions_iter.next().map(|r| r.align(alignment)) {
        if let Some(next) = regions_iter.peek().cloned().cloned().map(|r| r.align(alignment)) {
            // do these two regions overlap?
            if next.start >= region.start && next.start < region.end {
                regions_iter.next(); // advance the iterator so we don't use this value again and screw things up

                // is the next region located entirely inside this one?
                if next.end >= region.start && next.end < region.end {
                    // are the region kinds different? if they're the same we can just ignore the next value
                    if region.kind != next.kind {
                        // split the region, prioritizing the second region
                        new_regions.push(MemoryRegion {
                            start: region.start,
                            end: next.start,
                            kind: region.kind,
                        });
                        new_regions.push(next);
                        new_regions.push(MemoryRegion {
                            start: next.end,
                            end: region.end,
                            kind: region.kind,
                        });
                    }
                } else if region.kind == next.kind {
                    // no, but both regions are the same kind, so we can just push one region
                    new_regions.push(MemoryRegion {
                        start: region.start,
                        end: next.end,
                        kind: region.kind,
                    });
                } else {
                    // remove the overlapping part, prioritizing the second region
                    new_regions.push(MemoryRegion {
                        start: region.start,
                        end: next.start,
                        kind: region.kind,
                    });
                    new_regions.push(next);
                }
            } else {
                // no, just push the region and move on
                new_regions.push(region);
            }
        } else {
            // push the last region
            new_regions.push(region);
        }
    }

    new_regions
}

/// converts a slice of memory regions into a bitset of pages, where any set bits are unavailable pages
pub fn regions_to_set(regions: &[MemoryRegion], page_size: usize) -> BitSet {
    let page_size = page_size as u64;

    let mut highest_addr: u64 = 0;

    for region in regions.iter() {
        if region.kind == MemoryKind::Available && region.end > highest_addr {
            highest_addr = region.end;
        }
    }

    let mut set = BitSet::new((highest_addr / page_size).try_into().unwrap());

    for num in set.array.to_slice_mut().iter_mut() {
        *num = 0xffffffff;
    }

    set.bits_used = set.size;

    for region in regions.iter() {
        if region.kind == MemoryKind::Available {
            for i in region.start / page_size..region.end / page_size {
                set.clear(i.try_into().unwrap());
            }
        }
    }

    set
}
*/
