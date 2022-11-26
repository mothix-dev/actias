//! shared memory

use super::paging::{get_page_manager, PageDirectory, PageFrame};
use crate::{task::get_process, util::array::ConsistentIndexArray};
use alloc::{collections::BTreeMap, vec::Vec};
use common::types::{Errno, MmapAccess, ProcessID, Result};
use log::{error, trace};
use spin::Mutex;

pub const MAX_SHARED_IDS: u32 = u32::pow(2, 31) - 2;

pub static SHARED_MEMORY_AREAS: Mutex<ConsistentIndexArray<SharedMemoryArea>> = Mutex::new(ConsistentIndexArray::new());
pub static PHYS_TO_SHARED: Mutex<BTreeMap<u64, u32>> = Mutex::new(BTreeMap::new());

pub struct SharedMemoryArea {
    pub physical_addresses: Vec<u64>,
    pub references: usize,
    pub access: MmapAccess,
}

pub fn free_shared_reference(addr: u64) -> bool {
    let id = match PHYS_TO_SHARED.lock().get(&addr) {
        Some(id) => *id,
        None => return false,
    };

    let mut shm_lock = SHARED_MEMORY_AREAS.lock();
    let area = match shm_lock.get_mut(id as usize) {
        Some(area) => area,
        None => return false,
    };

    if area.references <= 1 {
        // remove the area and free pages if there aren't any remaining references to it

        trace!("no more references, freeing shared memory area {id}");

        // free pages
        for phys_addr in area.physical_addresses.iter() {
            super::paging::PAGE_REF_COUNTER.lock().remove_reference(*phys_addr);
        }

        // remove area
        shm_lock.remove(id as usize);
    } else {
        //trace!("removing reference to shared memory area {id}");
        area.references -= 1;
    }

    true
}

pub fn release_shared_area(id: u32) -> Result<()> {
    let mut shm_lock = SHARED_MEMORY_AREAS.lock();
    let area = shm_lock.get_mut(id as usize).ok_or(Errno::InvalidArgument)?;

    if area.physical_addresses.len() >= area.references {
        // remove the area and free pages if there aren't any remaining references to it

        trace!("no more references, freeing shared memory area {id}");

        // free pages
        for phys_addr in area.physical_addresses.iter() {
            super::paging::PAGE_REF_COUNTER.lock().remove_reference(*phys_addr);
        }

        // remove area
        shm_lock.remove(id as usize);
    } else {
        area.references -= area.physical_addresses.len();
    }

    Ok(())
}

enum FreeMode {
    RevertToOriginal { addr: usize, page: PageFrame },
    RevertNoFree { addr: usize, page: PageFrame },
    RemoveReference,
    Free,
    None,
}

struct TempMemoryShareEntry {
    phys_addr: u64,
    free_mode: FreeMode,
}

pub struct TempMemoryShare {
    process_id: ProcessID,
    phys_addresses: Vec<TempMemoryShareEntry>,
    finished: bool,
}

impl TempMemoryShare {
    /// end_addr is inclusive, subtract 1 if it's page aligned
    pub fn new(process_id: ProcessID, start_addr: usize, end_addr: usize) -> Result<Self> {
        let page_size = crate::arch::PageDirectory::PAGE_SIZE;

        let mut phys_addresses = Vec::new();
        phys_addresses.try_reserve((end_addr - start_addr + 1) / page_size).map_err(|_| Errno::OutOfMemory)?;

        Ok(Self {
            process_id,
            phys_addresses,
            finished: false,
        })
    }

    /// adds a new entry from an existing page at the given address
    pub fn add_addr(&mut self, addr: usize) -> Result<()> {
        let page = get_process(self.process_id.process)
            .ok_or(Errno::NoSuchProcess)?
            .page_directory
            .get_page(addr)
            .ok_or(Errno::BadAddress)?;

        self.add_page(addr, page)
    }

    pub fn add_page(&mut self, addr: usize, mut page: PageFrame) -> Result<()> {
        let old_page = page;

        if page.shared {
            // if this address is shared already, add another reference to it in the reference counter, as that's what's used when freeing shared memory
            super::paging::PAGE_REF_COUNTER.lock().add_reference(page.addr);

            self.phys_addresses.push(TempMemoryShareEntry {
                phys_addr: page.addr,
                free_mode: FreeMode::RemoveReference,
            });

            return Ok(());
        } else if !page.writable && page.copy_on_write && page.referenced {
            page = super::paging::copy_on_write(&mut super::paging::ProcessOrKernelPageDir::Process(self.process_id.process), addr, page)?;

            self.phys_addresses.push(TempMemoryShareEntry {
                phys_addr: page.addr,
                free_mode: FreeMode::RevertToOriginal { addr, page: old_page },
            });
        } else {
            self.phys_addresses.push(TempMemoryShareEntry {
                phys_addr: page.addr,
                free_mode: FreeMode::RevertNoFree { addr, page: old_page },
            });
        }

        page.shared = true;

        get_process(self.process_id.process)
            .ok_or(Errno::NoSuchProcess)?
            .page_directory
            .set_page(addr, Some(page))
            .map_err(|_| Errno::OutOfMemory)?;

        Ok(())
    }

    /// adds a new entry for a newly allocated page at the given address. page must be set to shared in order for this to work properly if it's gonna be mapped into a process's memory map
    pub fn add_new(&mut self, addr: u64) {
        self.phys_addresses.push(TempMemoryShareEntry {
            phys_addr: addr,
            free_mode: FreeMode::Free,
        });
    }

    /// adds a new entry for a reserved page at the given physical address
    pub fn add_reserved(&mut self, addr: u64) {
        self.phys_addresses.push(TempMemoryShareEntry {
            phys_addr: addr,
            free_mode: FreeMode::None,
        });
    }

    /// finishes building this shared memory region and returns its id
    pub fn share(mut self, access: MmapAccess) -> Result<u32> {
        let mut new_phys_addrs = Vec::new();
        new_phys_addrs.try_reserve_exact(self.phys_addresses.len()).map_err(|_| Errno::OutOfMemory)?;
        for entry in self.phys_addresses.iter() {
            new_phys_addrs.push(entry.phys_addr);
        }

        let mut shm_lock = SHARED_MEMORY_AREAS.lock();

        let id = shm_lock
            .add(SharedMemoryArea {
                physical_addresses: new_phys_addrs,
                references: self.phys_addresses.len(),
                access,
            })
            .map_err(|_| Errno::OutOfMemory)?;

        if id >= MAX_SHARED_IDS as usize {
            shm_lock.remove(id);

            return Err(Errno::TryAgain);
        }

        drop(shm_lock);
        let id = id as u32;

        // TODO: if an error occurs here, just remove the shared memory area like above
        let mut mapping = PHYS_TO_SHARED.lock();
        for entry in self.phys_addresses.iter() {
            mapping.insert(entry.phys_addr, id);
        }

        self.finished = true;

        Ok(id)
    }
}

impl Drop for TempMemoryShare {
    fn drop(&mut self) {
        if !self.finished {
            for entry in self.phys_addresses.iter() {
                match entry.free_mode {
                    FreeMode::RevertToOriginal { addr, page: orig_page } => {
                        // fail gracefully if we can't find the process
                        if let Some(mut process) = get_process(self.process_id.process) {
                            let page = match process.page_directory.get_page(addr) {
                                Some(page) => page,
                                None => {
                                    error!("failed to get page after failed memory share attempt (memory may leak!)");
                                    continue;
                                }
                            };

                            if let Err(err) = process.page_directory.set_page(addr, Some(orig_page)) {
                                error!("failed to revert page after failed memory share attempt (memory may leak!): {err:?}");
                                continue;
                            }

                            super::paging::free_page(page);
                        }
                    }
                    FreeMode::RevertNoFree { addr, page } => {
                        // fail gracefully if we can't find the process
                        if let Some(mut process) = get_process(self.process_id.process) {
                            if let Err(err) = process.page_directory.set_page(addr, Some(page)) {
                                error!("failed to revert page after failed memory share attempt (memory may leak!): {err:?}");
                                continue;
                            }
                        }
                    }
                    FreeMode::RemoveReference => super::paging::PAGE_REF_COUNTER.lock().remove_reference_no_free(entry.phys_addr),
                    FreeMode::Free => get_page_manager().set_frame_free(entry.phys_addr),
                    FreeMode::None => (),
                }
            }
        }
    }
}
