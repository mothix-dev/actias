use super::PageDirectory;
use crate::arch::PROPERTIES;
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use bitmask_enum::bitmask;
use common::Errno;
use log::{debug, warn};
use spin::Mutex;

pub struct ProcessMap {
    pub page_directory: super::PageDirSync<crate::arch::PageDirectory>,
    pub map: Vec<Mapping>,
}

impl ProcessMap {
    /// creates a new empty memory map
    pub fn new() -> common::Result<Self> {
        let split_addr = PROPERTIES.kernel_region.base;
        let global_state = crate::get_global_state();
        let page_directory = super::PageDirSync::sync_from(global_state.page_directory.clone(), split_addr)?;

        Ok(Self { page_directory, map: Vec::new() })
    }

    /// adds the given mapping to this memory map, modifying its page directory as needed and modifying other mappings so there's no overlap
    pub fn add_mapping(&mut self, arc_self: &Arc<Mutex<Self>>, mut mapping: Mapping, is_current: bool, map_exact: bool) -> common::Result<usize> {
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
            // resize overlapping regions and free overlapped pages
            if other_mapping.region.contains(mapping.region.base) {
                for addr in (mapping.region.base..=other_mapping.region.base + (other_mapping.region.length - 1)).step_by(PROPERTIES.page_size) {
                    other_mapping.free(&mut self.page_directory, arc_self, addr, is_current)?;
                }

                other_mapping.region.length = mapping.region.base - other_mapping.region.base;
            } else if mapping.region.contains(other_mapping.region.base) {
                for addr in (other_mapping.region.base..=mapping.region.base + (mapping.region.length - 1)).step_by(PROPERTIES.page_size) {
                    other_mapping.free(&mut self.page_directory, arc_self, addr, is_current)?;
                }

                let new_base = mapping.region.base + mapping.region.length;
                other_mapping.region.length -= new_base - other_mapping.region.base;
                other_mapping.region.base = new_base;
            }

            if other_mapping.region.length == 0 {
                to_remove.push(index);
                continue;
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

    /// removes the mapping at the given index from the list of mappings and frees any pages allocated for it
    pub fn remove_mapping(&mut self, arc_self: &Arc<Mutex<Self>>, base: usize, is_current: bool) -> common::Result<()> {
        let (index, _) = self.map.iter().enumerate().find(|(_, m)| m.region.base == base).ok_or(Errno::InvalidArgument)?;
        let mapping = self.map.remove(index);

        for i in (0..mapping.region.length).step_by(PROPERTIES.page_size) {
            let addr = mapping.region.base + i;

            mapping.free(&mut self.page_directory, arc_self, addr, is_current)?;
        }

        Ok(())
    }

    /// removes all the mappings in the memory map, freeing pages allocated for it
    pub fn remove_all(&mut self, arc_self: &Arc<Mutex<Self>>, is_current: bool) -> common::Result<()> {
        for mapping in self.map.iter() {
            for i in (0..mapping.region.length).step_by(PROPERTIES.page_size) {
                let addr = mapping.region.base + i;

                mapping.free(&mut self.page_directory, arc_self, addr, is_current)?;
            }
        }
        self.map.clear();

        Ok(())
    }

    /// handle mapping pages in/out on a page fault
    ///
    /// returns `true` if a page fault was successfully handled, `false` if it wasn't and the process should be killed
    pub fn page_fault(&mut self, arc_self: &Arc<Mutex<Self>>, addr: usize, access_type: MemoryProtection) -> bool {
        if let Some(mapping) = self.map.iter().find(|m| m.region.contains(addr)) && (mapping.protection | !access_type) == !0 && mapping.fault_in(&mut self.page_directory, arc_self, addr).is_ok() {
            true
        } else {
            false
        }
    }

    /// duplicates this memory map, creating a new identical memory map with all private mappings marked as copy on write
    pub fn fork(&mut self, is_current: bool) -> common::Result<Arc<Mutex<Self>>> {
        let new_map = Arc::new(Mutex::new(Self::new()?));

        {
            let mut new = new_map.lock();

            for mapping in self.map.iter() {
                let new_mapping = mapping.fork(&mut self.page_directory, &new_map, &mut new, is_current)?;
                new.map.push(new_mapping);
            }
        }

        Ok(new_map)
    }
}

pub struct Mapping {
    kind: MappingKind,
    region: super::ContiguousRegion<usize>,
    protection: MemoryProtection,
}

impl Mapping {
    /// creates a new mapping of the specified kind with the specified region and protection
    pub fn new(kind: MappingKind, region: super::ContiguousRegion<usize>, protection: MemoryProtection) -> Self {
        Self { kind, region, protection }
    }

    /// pages this mapping into memory in the given page directory on a page fault
    ///
    /// # Arguments
    ///
    /// * `page_directory` - the page directory that this mapping is mapped into
    /// * `map` - a reference to the map that this mapping is mapped into
    /// * `addr` - the virtual address that the page fault occurred at
    fn fault_in(&self, page_directory: &mut impl PageDirectory, map: &Arc<Mutex<ProcessMap>>, addr: usize) -> common::Result<()> {
        // align address to page size
        let aligned_addr = (addr / PROPERTIES.page_size) * PROPERTIES.page_size;

        match self.kind {
            MappingKind::Anonymous => {
                // allocate and zero out new page
                let phys_addr = crate::get_global_state().page_manager.lock().alloc_frame(Some(super::FrameReference {
                    map: Arc::downgrade(map),
                    addr: aligned_addr,
                }))?;
                let mut page = crate::mm::PageFrame {
                    addr: phys_addr,
                    present: true,
                    writable: true,
                    executable: self.protection & MemoryProtection::Execute != MemoryProtection::None,
                    user_mode: true,
                    ..Default::default()
                };
                page_directory.set_page(None::<&crate::arch::PageDirectory>, aligned_addr, Some(page))?;
                crate::arch::PageDirectory::flush_page(aligned_addr);

                unsafe {
                    core::slice::from_raw_parts_mut(aligned_addr as *mut u8, PROPERTIES.page_size).fill(0);
                }

                // remap page as read-only if required
                if self.protection & MemoryProtection::Write == MemoryProtection::None {
                    page.writable = false;
                    page_directory.set_page(None::<&crate::arch::PageDirectory>, aligned_addr, Some(page))?;
                    crate::arch::PageDirectory::flush_page(aligned_addr);
                }
            }
            MappingKind::FileCopy { ref file_descriptor, file_offset } => {
                // allocate and zero out new page
                let phys_addr = crate::get_global_state().page_manager.lock().alloc_frame(Some(super::FrameReference {
                    map: Arc::downgrade(map),
                    addr: aligned_addr,
                }))?;
                let mut page = crate::mm::PageFrame {
                    addr: phys_addr,
                    present: true,
                    writable: true,
                    executable: self.protection & MemoryProtection::Execute != MemoryProtection::None,
                    user_mode: true,
                    ..Default::default()
                };
                page_directory.set_page(None::<&crate::arch::PageDirectory>, aligned_addr, Some(page))?;
                crate::arch::PageDirectory::flush_page(aligned_addr);

                let slice = unsafe { core::slice::from_raw_parts_mut(aligned_addr as *mut u8, PROPERTIES.page_size) };

                // copy in file data
                let mut region_offset: i64 = (self.region.base - aligned_addr).try_into().map_err(|_| Errno::ValueOverflow)?;
                if region_offset < 0 {
                    warn!("TODO: handle negative region offset");
                    region_offset = 0;
                }

                file_descriptor.seek(file_offset + region_offset, common::SeekKind::Set)?;
                let bytes_read = file_descriptor.read(slice)?;
                slice[bytes_read..].fill(0);

                // remap page as read-only if required
                if self.protection & MemoryProtection::Write == MemoryProtection::None {
                    page.writable = false;
                    page_directory.set_page(None::<&crate::arch::PageDirectory>, aligned_addr, Some(page))?;
                    crate::arch::PageDirectory::flush_page(aligned_addr);
                }
            }
        }

        Ok(())
    }

    /// frees the page at the given address, removing it from the given page directory and allowing it to be allocated elsewhere if applicable
    ///
    /// # Arguments
    ///
    /// * `page_directory` - a reference to the page directory that this mapping is associated with
    /// * `map` - a reference to the map that this mapping is contained in
    /// * `addr` - the virtual address of the page to be freed
    /// * `is_current` - whether the previously specified page directory is the CPU's current page directory
    fn free(&self, page_directory: &mut impl PageDirectory, map: &Mutex<ProcessMap>, addr: usize, is_current: bool) -> common::Result<()> {
        if let Some(page) = page_directory.get_page(addr) {
            crate::get_global_state().page_manager.lock().free_frame(page.addr, Some(map));

            page_directory.set_page(None::<&crate::arch::PageDirectory>, addr, None)?;
            if is_current {
                crate::arch::PageDirectory::flush_page(addr);
            }
        }

        Ok(())
    }

    /// duplicates this mapping into a new memory map, setting all pages it encompasses to copy on write
    ///
    /// # Arguments
    ///
    /// * `page_directory` - the page directory of the map that this mapping is being duplicated from
    /// * `arc_map` - a reference to an Arc storing the new map
    /// * `map` - a reference to the new map itself
    /// * `is_current` - whether the page directory this mapping is being duplicated from is the current page directory
    ///
    /// # Returns
    ///
    /// this function returns the new mapping on success
    fn fork(&self, page_directory: &mut impl PageDirectory, arc_map: &Arc<Mutex<ProcessMap>>, map: &mut ProcessMap, is_current: bool) -> common::Result<Self> {
        match self.kind {
            MappingKind::Anonymous => {
                for i in (0..self.region.length).step_by(PROPERTIES.page_size) {
                    let addr = self.region.base + i;

                    if let Some(mut page) = page_directory.get_page(addr) {
                        crate::get_global_state()
                            .page_manager
                            .lock()
                            .add_reference(page.addr, super::FrameReference { map: Arc::downgrade(arc_map), addr });

                        if page.writable {
                            page.writable = false;
                            page.copy_on_write = true;
                        }

                        page_directory.set_page(None::<&crate::arch::PageDirectory>, addr, Some(page))?;
                        map.page_directory.set_page(None::<&crate::arch::PageDirectory>, addr, Some(page))?;
                        if is_current {
                            crate::arch::PageDirectory::flush_page(addr);
                        }
                    }
                }

                Ok(Mapping {
                    kind: MappingKind::Anonymous,
                    region: self.region,
                    protection: self.protection,
                })
            }
            MappingKind::FileCopy { ref file_descriptor, file_offset } => {
                for i in (0..self.region.length).step_by(PROPERTIES.page_size) {
                    let addr = self.region.base + i;

                    if let Some(page) = page_directory.get_page(addr) {
                        crate::get_global_state()
                            .page_manager
                            .lock()
                            .add_reference(page.addr, super::FrameReference { map: Arc::downgrade(arc_map), addr });

                        map.page_directory.set_page(None::<&crate::arch::PageDirectory>, addr, Some(page))?;
                    }
                }

                Ok(Mapping {
                    kind: MappingKind::FileCopy {
                        file_descriptor: file_descriptor.dup()?,
                        file_offset,
                    },
                    region: self.region,
                    protection: self.protection,
                })
            }
        }
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
