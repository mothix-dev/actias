use super::PageDirectory;
use crate::arch::PROPERTIES;
use alloc::{boxed::Box, vec::Vec};
use bitmask_enum::bitmask;

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
    pub fn map(&mut self, mapping: Mapping, is_current: bool) {
        assert!(!mapping.region.overlaps(PROPERTIES.kernel_region), "mapping is inside kernel memory");

        // clear any existing mappings in the area
        // TODO: only do this on overlap
        for i in (0..mapping.aligned_region.length).step_by(PROPERTIES.page_size) {
            let addr = mapping.aligned_region.base + i;
            self.page_directory.set_page(None::<&crate::arch::PageDirectory>, addr, None).unwrap();
            if is_current {
                crate::arch::PageDirectory::flush_page(addr);
            }
        }

        self.map.push(mapping);
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
    aligned_region: super::ContiguousRegion<usize>,
    protection: MemoryProtection,
}

impl Mapping {
    pub fn new(kind: MappingKind, region: super::ContiguousRegion<usize>, protection: MemoryProtection) -> Self {
        Self {
            kind,
            region,
            aligned_region: region.align_covering(PROPERTIES.page_size),
            protection,
        }
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
                crate::arch::PageDirectory::flush_page(aligned_addr);
                unsafe {
                    core::slice::from_raw_parts_mut(aligned_addr as *mut u8, PROPERTIES.page_size).fill(0);
                }
            }
            MappingKind::FileCopy { ref file_descriptor, offset, len } => {
                todo!();
            }
        }

        Ok(())
    }
}

pub enum MappingKind {
    Anonymous,
    FileCopy {
        file_descriptor: Box<dyn crate::fs::FileDescriptor>,
        offset: i64,
        len: usize,
    },
}

#[bitmask]
pub enum MemoryProtection {
    Read,
    Write,
    Execute,
    None = 0,
}
