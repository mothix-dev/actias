use super::PageDirectory;
use crate::arch::PROPERTIES;
use alloc::{boxed::Box, vec::Vec};
use bitmask_enum::bitmask;
use common::Errno;
use log::{debug, warn};

pub struct ProcessMap {
    pub page_directory: super::PageDirSync<crate::arch::PageDirectory>,
    pub map: Vec<Mapping>,
}

impl ProcessMap {
    pub fn new() -> Self {
        let split_addr = PROPERTIES.kernel_region.base;
        let global_state = crate::get_global_state();
        let page_directory = super::PageDirSync::sync_from(global_state.page_directory.clone(), split_addr).unwrap();

        Self { page_directory, map: Vec::new() }
    }

    /// adds the given mapping to this memory map, modifying its page directory as needed and modifying other mappings so there's no overlap
    pub fn add_mapping(&mut self, mut mapping: Mapping, is_current: bool, map_exact: bool) -> common::Result<usize> {
        if map_exact {
            if mapping.region.base % PROPERTIES.page_size != 0 {
                return Err(Errno::InvalidArgument);
            }
        } else {
            mapping.region.base = ((mapping.region.base) / PROPERTIES.page_size) * PROPERTIES.page_size;
        }

        mapping.region.length = ((mapping.region.length + PROPERTIES.page_size - 1) / PROPERTIES.page_size) * PROPERTIES.page_size;

        assert!(!mapping.region.overlaps(PROPERTIES.kernel_region), "mapping is inside kernel memory");
        debug!("adding mapping over {:?}, {:?}", mapping.region, mapping.protection);

        let mut should_add = true;
        let mut to_remove = Vec::new();

        // iterate through all mappings looking for overlaps
        for (index, other_mapping) in self.map.iter_mut().enumerate() {
            // resize overlapping regions
            if other_mapping.region.contains(mapping.region.base) {
                for addr in (mapping.region.base..=other_mapping.region.base + (other_mapping.region.length - 1)).step_by(PROPERTIES.page_size) {
                    self.page_directory.set_page(None::<&crate::arch::PageDirectory>, addr, None).unwrap();
                    if is_current {
                        crate::arch::PageDirectory::flush_page(addr);
                    }
                }

                other_mapping.region.length = mapping.region.base - other_mapping.region.base;

                if other_mapping.region.length == 0 {
                    to_remove.push(index);
                }
            }

            // combine adjacent regions
            if (other_mapping.region.base.saturating_add(other_mapping.region.length) == mapping.region.base || mapping.region.base.saturating_add(mapping.region.length) == other_mapping.region.base)
                && (mapping.protection == other_mapping.protection && matches!(mapping.kind, MappingKind::Anonymous) && matches!(other_mapping.kind, MappingKind::Anonymous))
            {
                if mapping.region.base > other_mapping.region.base {
                    other_mapping.region.length += mapping.region.length;
                    should_add = false;
                } else {
                    mapping.region.length += other_mapping.region.length;
                    to_remove.push(index);
                }
            }
        }

        // remove any zero-length entries
        for index in to_remove {
            self.map.remove(index);
        }

        let base = mapping.region.base;
        if should_add {
            self.map.push(mapping);
        }

        Ok(base)
    }

    /// handle mapping pages in/out on a page fault
    ///
    /// returns `true` if a page fault was successfully handled, `false` if it wasn't and the process should be killed
    pub fn page_fault(&mut self, addr: usize, access_type: MemoryProtection) -> bool {
        if let Some(mapping) = self.map.iter().find(|m| m.region.contains(addr)) && (mapping.protection | !access_type) == !0 && mapping.page_in(&mut self.page_directory, addr).is_ok() {
            true
        } else {
            false
        }
    }
}

impl Default for ProcessMap {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Mapping {
    kind: MappingKind,
    region: super::ContiguousRegion<usize>,
    protection: MemoryProtection,
}

impl Mapping {
    pub fn new(kind: MappingKind, region: super::ContiguousRegion<usize>, protection: MemoryProtection) -> Self {
        Self { kind, region, protection }
    }

    /// pages this mapping into memory in the given page directory
    fn page_in(&self, page_directory: &mut impl PageDirectory, addr: usize) -> Result<(), super::PagingError> {
        // align address to page size
        let aligned_addr = (addr / PROPERTIES.page_size) * PROPERTIES.page_size;

        match self.kind {
            MappingKind::Anonymous => {
                // allocate and zero out new page
                let phys_addr = crate::get_global_state().page_manager.lock().alloc_frame()?;
                page_directory.set_page(
                    None::<&crate::arch::PageDirectory>,
                    aligned_addr,
                    Some(crate::mm::PageFrame {
                        addr: phys_addr,
                        present: true,
                        writable: self.protection & MemoryProtection::Write != MemoryProtection::None,
                        executable: self.protection & MemoryProtection::Execute != MemoryProtection::None,
                        user_mode: true,
                        ..Default::default()
                    }),
                )?;
                unsafe {
                    core::slice::from_raw_parts_mut(aligned_addr as *mut u8, PROPERTIES.page_size).fill(0);
                }
                crate::arch::PageDirectory::flush_page(aligned_addr);
            }
            MappingKind::FileCopy { ref file_descriptor, file_offset } => {
                // allocate and zero out new page
                let phys_addr = crate::get_global_state().page_manager.lock().alloc_frame()?;
                page_directory.set_page(
                    None::<&crate::arch::PageDirectory>,
                    aligned_addr,
                    Some(crate::mm::PageFrame {
                        addr: phys_addr,
                        present: true,
                        writable: self.protection & MemoryProtection::Write != MemoryProtection::None,
                        executable: self.protection & MemoryProtection::Execute != MemoryProtection::None,
                        user_mode: true,
                        ..Default::default()
                    }),
                )?;
                let slice = unsafe { core::slice::from_raw_parts_mut(aligned_addr as *mut u8, PROPERTIES.page_size) };
                slice.fill(0);

                // copy in file data
                let mut region_offset: i64 = (self.region.base - aligned_addr).try_into().map_err(|_| super::PagingError::IOError)?;
                if region_offset < 0 {
                    warn!("TODO: handle negative region offset");
                    region_offset = 0;
                }

                file_descriptor.seek(file_offset + region_offset, common::SeekKind::Set).map_err(|_| super::PagingError::IOError)?;
                file_descriptor.read(slice).map_err(|_| super::PagingError::IOError)?;

                // if this is done earlier on x86 everything breaks
                crate::arch::PageDirectory::flush_page(aligned_addr);
            }
        }

        Ok(())
    }

    /// gets the region of memory that this mapping takes up
    pub fn region(&self) -> &super::ContiguousRegion<usize> {
        &self.region
    }
}

pub enum MappingKind {
    Anonymous,
    FileCopy { file_descriptor: Box<dyn crate::fs::FileDescriptor>, file_offset: i64 },
}

#[bitmask]
pub enum MemoryProtection {
    Read,
    Write,
    Execute,
    None = 0,
}
