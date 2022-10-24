//! shared memory

use crate::util::array::ConsistentIndexArray;
use alloc::{collections::BTreeMap, vec::Vec};
use common::types::{Errno, Result};
use spin::Mutex;
use log::trace;

pub const MAX_SHARED_IDS: u32 = u32::pow(2, 31) - 2;

pub static SHARED_MEMORY_AREAS: Mutex<ConsistentIndexArray<SharedMemoryArea>> = Mutex::new(ConsistentIndexArray::new());
pub static PHYS_TO_SHARED: Mutex<BTreeMap<u64, u32>> = Mutex::new(BTreeMap::new());

pub struct SharedMemoryArea {
    pub physical_addresses: Vec<u64>,
    pub references: usize,
}

pub fn share_area(addrs: &[u64]) -> Result<u32> {
    let id = SHARED_MEMORY_AREAS
        .lock()
        .add(SharedMemoryArea {
            physical_addresses: addrs.to_vec(),
            references: addrs.len(),
        })
        .map_err(|_| Errno::OutOfMemory)?;

    if id >= MAX_SHARED_IDS as usize {
        Err(Errno::TryAgain)
    } else {
        let id = id as u32;
        let mut mapping = PHYS_TO_SHARED.lock();
        for addr in addrs.iter() {
            mapping.insert(*addr, id);
        }

        Ok(id)
    }
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
