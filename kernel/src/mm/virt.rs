use super::{ContiguousRegion, PageDirectory};
use crate::{
    arch::{PhysicalAddress, PROPERTIES},
    mm::FrameReference,
    process::Buffer,
};
use alloc::{sync::Arc, vec::Vec};
use bitmask_enum::bitmask;
use common::{Errno, Result};
use log::{debug, error, trace};
use spin::Mutex;

pub type Registers = <crate::arch::InterruptManager as crate::arch::bsp::InterruptManager>::Registers;

pub struct ProcessMap {
    pub page_directory: super::PageDirSync<crate::arch::PageDirectory>,
    pub map: Vec<Mapping>,
}

impl ProcessMap {
    /// creates a new empty memory map
    pub fn new() -> Result<Self> {
        let page_directory = super::PageDirSync::sync_from(crate::get_global_state().page_directory.clone(), PROPERTIES.kernel_region)?;

        Ok(Self { page_directory, map: Vec::new() })
    }

    /// iterates over all mapped regions, resizing and/or combining as needed so that none overlap with the given mapping
    ///
    /// # Arguments
    /// * `arc_self` - a reference counted pointer to this memory map, to allow references to any overlapping memory maps to be freed
    /// * `mapping` - the mapping that every region is adjusted to not overlap
    /// * `is_current` - whether this memory map is the CPU's current memory map
    fn clean_up_overlaps(&mut self, arc_self: &Arc<Mutex<Self>>, mapping: &mut Mapping, is_current: bool) -> Result<()> {
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
                    mapping.region.length += other_mapping.region.length;
                    mapping.region.base = other_mapping.region.base;
                } else {
                    mapping.region.length += other_mapping.region.length;
                }
                to_remove.push(index);
            }
        }

        // remove any zero-length entries
        for index in to_remove {
            self.map.remove(index);
        }

        Ok(())
    }

    /// adds the given mapping to this memory map, modifying its page directory as needed and modifying other mappings so there's no overlap
    ///
    /// # Arguments
    /// * `arc_self` - a reference counted pointer to this memory map, to allow references to any overlapping memory maps to be freed
    /// * `mapping` - the mapping to add
    /// * `is_current` - whether this memory map's page directory is the CPU's current page directory
    /// * `map_exact` - whether the mapping's exact base address should be used, instead of page-aligning it down.
    /// if this is `true` and the mapping's base address isn't page aligned, an error will be returned
    ///
    /// # Returns
    /// on success, the actual base address of the mapping is returned (since it's aligned down to the nearest page boundary)
    pub fn add_mapping(&mut self, arc_self: &Arc<Mutex<Self>>, mut mapping: Mapping, is_current: bool, map_exact: bool) -> Result<usize> {
        if map_exact {
            if mapping.region.base % PROPERTIES.page_size != 0 {
                return Err(Errno::InvalidArgument);
            }
        } else {
            mapping.region.base = ((mapping.region.base) / PROPERTIES.page_size) * PROPERTIES.page_size;
        }

        mapping.region.length = ((mapping.region.length + PROPERTIES.page_size - 1) / PROPERTIES.page_size) * PROPERTIES.page_size;

        assert!(!mapping.region.overlaps(PROPERTIES.kernel_region), "mapping is inside kernel memory");

        self.clean_up_overlaps(arc_self, &mut mapping, is_current)?;

        let base = mapping.region.base;
        self.map.push(mapping);

        Ok(base)
    }

    /// removes the mapping at the given address from the list of mappings and frees any pages allocated for it
    ///
    /// # Arguments
    /// * `arc_self` - a reference counted pointer to this memory map, to allow references to it to be found and removed
    /// * `base` - the base address of the mapping to remove
    /// * `is_current` - whether this memory map's page directory is the CPU's current page directory
    pub fn remove_mapping(&mut self, arc_self: &Arc<Mutex<Self>>, base: usize, is_current: bool) -> Result<()> {
        let (index, _) = self.map.iter().enumerate().find(|(_, m)| m.region.base == base).ok_or(Errno::InvalidArgument)?;
        let mapping = self.map.remove(index);

        for i in (0..mapping.region.length).step_by(PROPERTIES.page_size) {
            let addr = mapping.region.base + i;

            mapping.free(&mut self.page_directory, arc_self, addr, is_current)?;
        }

        Ok(())
    }

    /// removes all the mappings in the memory map, freeing pages allocated for it
    ///
    /// # Arguments
    /// * `arc_self` - a reference counted pointer to this memory map, to allow references to it to be found and removed
    /// * `is_current` - whether this memory map's page directory is the CPU's current page directory
    pub fn remove_all(&mut self, arc_self: &Arc<Mutex<Self>>, is_current: bool) -> Result<()> {
        for mapping in self.map.iter() {
            for i in (0..mapping.region.length).step_by(PROPERTIES.page_size) {
                let addr = mapping.region.base + i;

                mapping.free(&mut self.page_directory, arc_self, addr, is_current)?;
            }
        }
        self.map.clear();

        Ok(())
    }

    /// handles mapping pages in/out on a page fault
    ///
    /// # Arguments
    /// * `arc_self` - a reference counted pointer to this memory map, to allow for proper page referencing
    /// * `addr` - the virtual address that the page fault occurred at
    /// * `access_type` - how the CPU tried to access this region of memory
    ///
    /// # Returns
    /// returns `true` if a page fault was successfully handled, `false` if it wasn't and the process should be killed
    pub async fn page_fault(&mut self, arc_self: &Arc<Mutex<Self>>, addr: usize, access_type: MemoryProtection) -> bool {
        // find the mapping, check its permissions, and try to map it in
        trace!("page fault @ {addr:#x}");
        if let Some(mapping) = self.map.iter().find(|m| m.region.contains(addr)).cloned() && (mapping.protection | !access_type) == !0 && self.fault_in(arc_self, mapping, addr, access_type).await.is_ok() {
            true
        } else {
            false
        }
    }

    /// pages a mapping into memory on a page fault
    ///
    /// # Arguments
    /// * `arc_self` - a reference counted pointer to this memory map, to allow for proper page referencing
    /// * `mapping` - the mapping that needs a page in it faulted into memory
    /// * `addr` - the virtual address that the page fault occurred at
    /// * `access_type` - how the CPU tried to access the faulted page
    async fn fault_in(&mut self, arc_self: &Arc<Mutex<ProcessMap>>, mapping: Mapping, addr: usize, access_type: MemoryProtection) -> Result<()> {
        // align address to page size
        let aligned_addr = (addr / PROPERTIES.page_size) * PROPERTIES.page_size;

        let page = self.page_directory.get_page(aligned_addr);

        // handle copy on write
        if access_type & MemoryProtection::Write != MemoryProtection::None && let Some(page) = page.as_ref() && !page.writable && page.copy_on_write {
            // allocate new page
            let phys_addr = crate::get_global_state().page_manager.lock().alloc_frame(Some(super::FrameReference {
                map: Arc::downgrade(arc_self),
                addr: aligned_addr,
            }))?;
            let old_page = unsafe { core::slice::from_raw_parts(aligned_addr as *const u8, PROPERTIES.page_size) };

            // copy data from old page into new page
            unsafe {
                super::map_memory(&mut self.page_directory, &[phys_addr], |slice| {
                    slice.copy_from_slice(old_page)
                })?;
            }

            // map in new page
            self.page_directory.set_page(None::<&crate::arch::PageDirectory>, aligned_addr, Some(crate::mm::PageFrame {
                addr: phys_addr,
                present: true,
                writable: true,
                executable: mapping.protection & MemoryProtection::Execute != MemoryProtection::None,
                user_mode: true,
                ..Default::default()
            }))?;
            crate::arch::PageDirectory::flush_page(aligned_addr);

            // remove reference to old page, freeing it if applicable
            crate::get_global_state().page_manager.lock().free_frame(page.addr, Some(arc_self));
        }

        if page.is_none() {
            // page needs to be mapped in, map it in
            match &mapping.kind {
                MappingKind::Anonymous => {
                    // allocate and zero out new page
                    let phys_addr = crate::get_global_state().page_manager.lock().alloc_frame(Some(super::FrameReference {
                        map: Arc::downgrade(arc_self),
                        addr: aligned_addr,
                    }))?;
                    Buffer::Page(phys_addr).map_in_immediate(|slice| slice.fill(0))?;

                    self.page_directory.set_page(
                        None::<&crate::arch::PageDirectory>,
                        aligned_addr,
                        Some(crate::mm::PageFrame {
                            addr: phys_addr,
                            present: true,
                            writable: mapping.protection & MemoryProtection::Write != MemoryProtection::None,
                            executable: mapping.protection & MemoryProtection::Execute != MemoryProtection::None,
                            user_mode: true,
                            ..Default::default()
                        }),
                    )?;
                }
                MappingKind::File { file_handle, file_offset } => {
                    debug!("copying in file at {aligned_addr:#x} - {file_offset:#x} ({:?})", mapping.region);

                    // copy in file data
                    let base: i64 = mapping.region.base.try_into().map_err(|_| Errno::ValueOverflow)?;
                    let addr: i64 = aligned_addr.try_into().map_err(|_| Errno::ValueOverflow)?;
                    let region_offset = addr - base;

                    debug!("region_offset is {region_offset:?}");

                    assert!(region_offset >= 0, "region_offset can't be less than zero");

                    let arc_map = arc_self.clone();
                    let protection = mapping.protection;

                    match file_handle.get_page(file_offset + region_offset).await {
                        Some(addr) => {
                            // add a reference to this page, tying it to this map
                            crate::get_global_state().page_manager.lock().add_reference(addr, FrameReference {
                                map: Arc::downgrade(&arc_map),
                                addr: aligned_addr,
                            });

                            self.page_directory.set_page(
                                None::<&crate::arch::PageDirectory>,
                                aligned_addr,
                                Some(crate::mm::PageFrame {
                                    addr,
                                    present: true,
                                    writable: protection & MemoryProtection::Write != MemoryProtection::None,
                                    executable: protection & MemoryProtection::Execute != MemoryProtection::None,
                                    user_mode: true,
                                    ..Default::default()
                                }),
                            )?;
                        }
                        None => error!("TODO: properly kill blocked page faulted process"), // this is fine since it'll just stay blocked so can't continue with invalid state
                    }
                }
            }
        }

        Ok(())
    }

    /// duplicates this memory map, creating a new identical memory map with all private mappings marked as copy on write
    ///
    /// # Arguments
    /// * `is_current` - whether the page directory of this memory map is the CPU's current page directory
    ///
    /// # Returns
    /// this function returns the new memory map on success
    pub fn fork(&mut self, is_current: bool) -> Result<Arc<Mutex<Self>>> {
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

    /// maps in all pages in the given area so they can be read from/written to and checks their permissions
    ///
    /// # Arguments
    /// * `arc_self` - a reference counted pointer to this memory map, so that any mapped in pages will be associated with it
    /// * `base` - the base address of the region of memory to map in
    /// * `length` - the length of the region of memory to map in
    /// * `access_type` - how the region of memory to be mapped in will be accessed
    pub async fn map_in_area(&mut self, arc_self: &Arc<Mutex<Self>>, base: usize, length: usize, access_type: MemoryProtection) -> Result<Vec<PhysicalAddress>> {
        let region = super::ContiguousRegion::new(base, length).align_covering(crate::arch::PROPERTIES.page_size);
        let mut addrs = Vec::new();

        for i in (0..region.length).step_by(crate::arch::PROPERTIES.page_size) {
            let addr = region.base + i;

            if let Some(page) = self.page_directory.get_page(addr) {
                // verify that this page is properly accessible
                // completely inaccessible maps won't be paged in at all so they don't have to be checked for here
                if access_type & MemoryProtection::Write != MemoryProtection::None && !page.writable {
                    return Err(Errno::BadAddress);
                }

                addrs.push(page.addr);
            } else if !self.page_fault(arc_self, addr, access_type).await {
                // page couldn't be mapped in
                return Err(Errno::BadAddress);
            }
        }

        Ok(addrs)
    }

    /// moves and/or resizes the mapping at the given base address to at least the given length
    ///
    /// # Arguments
    /// * `arc_self` - a reference counted pointer to this memory map, to allow for any pages that require freeing to be properly freed
    /// * `base` - the base address of the mapping to resize
    /// * `length` - the new length of the mapping
    /// * `is_current` - whether this memory map is the CPU's current memory map
    ///
    /// # Returns
    /// on success, the actual new region that this map covers is returned since it may have been resized to fit a page boundary
    pub fn resize(&mut self, arc_self: &Arc<Mutex<Self>>, base: usize, length: usize, is_current: bool) -> Result<usize> {
        if base % PROPERTIES.page_size != 0 {
            return Err(Errno::InvalidArgument);
        }

        // since the mapping can't be modified in place due to the borrow checker, just clone it instead
        // the lock held over this memory map will prevent the slightly wacky state from causing problems
        let mut mapping = self.map.iter_mut().find(|mapping| mapping.region.base == base).ok_or(Errno::InvalidArgument)?.clone();

        // align the length up to the nearest page boundary
        let length = ((length + PROPERTIES.page_size - 1) / PROPERTIES.page_size) * PROPERTIES.page_size;
        mapping.region.length = length;

        self.clean_up_overlaps(arc_self, &mut mapping, is_current)?;

        // update the mapping with the new values
        if let Some(old) = self.map.iter_mut().find(|mapping| mapping.region.base == base) {
            *old = mapping;
        }

        Ok(length)
    }
}

#[derive(Clone)]
pub struct Mapping {
    kind: MappingKind,
    region: ContiguousRegion<usize>,
    protection: MemoryProtection,
}

impl Mapping {
    /// creates a new mapping of the specified kind with the specified region and protection
    pub fn new(kind: MappingKind, region: ContiguousRegion<usize>, protection: MemoryProtection) -> Self {
        Self { kind, region, protection }
    }

    /// frees the page at the given address, removing it from the given page directory and allowing it to be allocated elsewhere if applicable
    ///
    /// # Arguments
    /// * `page_directory` - a reference to the page directory that this mapping is associated with
    /// * `map` - a reference to the map that this mapping is contained in
    /// * `addr` - the virtual address of the page to be freed
    /// * `is_current` - whether the previously specified page directory is the CPU's current page directory
    fn free(&self, page_directory: &mut impl PageDirectory, map: &Mutex<ProcessMap>, addr: usize, is_current: bool) -> Result<()> {
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
    /// * `page_directory` - the page directory of the map that this mapping is being duplicated from
    /// * `arc_map` - a reference to an Arc storing the new map
    /// * `map` - a reference to the new map itself
    /// * `is_current` - whether the page directory this mapping is being duplicated from is the current page directory
    ///
    /// # Returns
    /// this function returns the new mapping on success
    fn fork(&self, page_directory: &mut impl PageDirectory, arc_map: &Arc<Mutex<ProcessMap>>, map: &mut ProcessMap, is_current: bool) -> Result<Self> {
        match &self.kind {
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
            }
            MappingKind::File { .. } => {
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
            }
        }

        Ok(self.clone())
    }

    /// gets the region of memory that this mapping takes up
    pub fn region(&self) -> &ContiguousRegion<usize> {
        &self.region
    }

    /// gets what kind of mapping this is
    pub fn kind(&self) -> &MappingKind {
        &self.kind
    }
}

#[derive(Clone)]
pub enum MappingKind {
    Anonymous,
    File { file_handle: Arc<crate::fs::FileHandle>, file_offset: i64 },
}

#[bitmask]
pub enum MemoryProtection {
    Read,
    Write,
    Execute,
    None = 0,
}
