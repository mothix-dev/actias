//! acpi and acpi accessories

use super::{paging::PageDir, APICToCPU, PAGE_SIZE};
use crate::{
    mm::paging::{get_page_dir, map_memory, PageDirectory},
    task::cpu::{ThreadID, CPU},
    util::debug::{DebugHexArray, FormatHex},
};
use alloc::{format, vec, vec::Vec};
use core::{fmt, mem::size_of, slice, str};
use log::{debug, error, warn};

/// calculates the checksum for a provided type
pub fn calculate_checksum<T>(t: &T) -> usize {
    let bytes = unsafe { slice::from_raw_parts(t as *const _ as *const u8, size_of::<T>()) };

    calculate_checksum_bytes(bytes)
}

/// calculates the checksum for a slice of bytes
pub fn calculate_checksum_bytes(bytes: &[u8]) -> usize {
    let mut sum: usize = 0;

    for &byte in bytes.iter() {
        sum = sum.wrapping_add(byte as usize);
    }

    sum
}

/// gets the header of an acpi table at the provided physical address
fn read_header(phys_addr: u64) -> Option<ACPIHeader> {
    let page = (phys_addr / PAGE_SIZE as u64) * PAGE_SIZE as u64;
    let offset = (phys_addr % PAGE_SIZE as u64) as usize;

    unsafe { map_memory(&mut get_page_dir(), &[page, page + 1], |s| *(&s[offset] as *const u8 as *const ACPIHeader)).ok() }
}

/// gets the data of an acpi table at the provided physical address
fn read_data(phys_addr: u64, len: u32) -> Option<Vec<u8>> {
    let page = (phys_addr / PAGE_SIZE as u64) * PAGE_SIZE as u64;
    let offset = (phys_addr % PAGE_SIZE as u64) as usize + size_of::<ACPIHeader>();
    let len = len as usize - size_of::<ACPIHeader>();

    let mut addresses = Vec::new();

    for addr in (page..page + offset as u64 + len as u64).step_by(PAGE_SIZE) {
        addresses.push(addr);
    }

    unsafe { map_memory(&mut get_page_dir(), &addresses, |s| s[offset..offset + len].to_vec()).ok() }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ACPIHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: u32,
    pub creator_revision: u32,
}

impl fmt::Debug for ACPIHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ACPIHeader")
            .field("signature", &str::from_utf8(&self.signature).unwrap())
            .field("length", &self.length)
            .field("revision", &self.revision)
            .field("checksum", &self.checksum)
            .field("oem_id", &str::from_utf8(&self.oem_id).unwrap())
            .field("oem_table_id", &str::from_utf8(&self.oem_table_id).unwrap())
            .field("oem_revision", &self.oem_revision)
            .field("creator_id", &FormatHex(self.creator_id))
            .field("creator_revision", &self.creator_revision)
            .finish()
    }
}

pub trait SDTPointer {}

impl SDTPointer for u32 {}
impl SDTPointer for u64 {}

pub struct SDT<S: SDTPointer + Clone> {
    pub header: ACPIHeader,
    pub sdt_pointers: Vec<S>,
}

impl<S: SDTPointer + Clone> SDT<S> {
    pub unsafe fn from_raw_pointer(ptr: *const u8) -> Self {
        let header = *(ptr as *const ACPIHeader);
        let num_sdt_pointers = (header.length as usize - size_of::<ACPIHeader>()) / size_of::<S>();
        let sdt_pointers = slice::from_raw_parts(ptr.add(size_of::<ACPIHeader>()) as *const S, num_sdt_pointers).to_vec();

        Self { header, sdt_pointers }
    }

    pub fn verify_checksum(&self) -> bool {
        let mut checksum = calculate_checksum(&self.header);

        for pointer in self.sdt_pointers.iter() {
            checksum += calculate_checksum(pointer);
        }

        (checksum & 0xff) == 0
    }
}

impl<S: SDTPointer + Clone + fmt::Debug> fmt::Debug for SDT<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(&format!("SDT<{}>", core::any::type_name::<S>()))
            .field("header", &self.header)
            .field("sdt_pointers", &DebugHexArray(&self.sdt_pointers))
            .finish()
    }
}

/// given a physical address, reads the (R|X)SDT located at that address
fn read_sdt<S: SDTPointer + Clone>(phys_addr: u64) -> Option<SDT<S>> {
    let page = (phys_addr / PAGE_SIZE as u64) * PAGE_SIZE as u64;
    let offset = (phys_addr % PAGE_SIZE as u64) as usize;

    unsafe { map_memory(&mut get_page_dir(), &[page, page + 1], |s| SDT::from_raw_pointer(&s[offset] as *const u8)).ok() }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct RSDPOriginal {
    pub signature: [u8; 8],
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub revision: u8,
    pub rsdt_address: u32,
}

impl fmt::Debug for RSDPOriginal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RSDPOriginal")
            .field("signature", &str::from_utf8(&self.signature).unwrap())
            .field("checksum", &self.checksum)
            .field("oem_id", &str::from_utf8(&self.oem_id).unwrap())
            .field("revision", &self.revision)
            .field("rsdt_address", &FormatHex(self.rsdt_address))
            .finish()
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct RSDPExtended {
    pub signature: [u8; 8],
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub revision: u8,
    pub rsdt_address: u32,

    pub length: u32,
    pub xsdt_address: u64,
    pub extended_checksum: u8,
    pub reserved: [u8; 3],
}

impl fmt::Debug for RSDPExtended {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RSDPExtended")
            .field("signature", &str::from_utf8(&self.signature).unwrap())
            .field("checksum", &self.checksum)
            .field("oem_id", &str::from_utf8(&self.oem_id).unwrap())
            .field("revision", &self.revision)
            .field("rsdt_address", &FormatHex(self.rsdt_address))
            .field("length", &self.length)
            .field("xsdt_address", &FormatHex(self.xsdt_address))
            .field("extended_checksum", &self.extended_checksum)
            .field("reserved", &self.reserved)
            .finish()
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union RSDP {
    pub original: RSDPOriginal,
    pub extended: RSDPExtended,
}

/// scans the BIOS reserved data area for a valid RSDP signature
fn find_rsdp() -> Option<u64> {
    // map pages one at a time to avoid exhausting memory on low memory systems
    for page in (0x000e0000..0x00100000).step_by(PAGE_SIZE) {
        unsafe {
            if let Some(addr) = map_memory(&mut get_page_dir(), &[page], |s| {
                // signature is always aligned to 16 bytes
                for i in (0..PAGE_SIZE).step_by(16) {
                    if s[i..i + 8] == (b"RSD PTR ")[0..8] {
                        return Some(page + i as u64);
                    }
                }

                None
            })
            .unwrap()
            {
                return Some(addr);
            }
        }
    }

    None
}

/// given a physical address, reads the RSDP located at that address
fn read_rsdp(phys_addr: u64) -> Option<RSDP> {
    let page = (phys_addr / PAGE_SIZE as u64) * PAGE_SIZE as u64;
    let offset = (phys_addr % PAGE_SIZE as u64) as usize;

    unsafe {
        map_memory(&mut get_page_dir(), &[page, page + 1], |s| {
            // always read extended rsdp regardless of the revision
            RSDP {
                extended: *(&s[offset] as *const _ as *const RSDPExtended),
            }
        })
        .ok()
    }
}

#[derive(Copy, Clone)]
pub struct MADTHeader {
    pub local_apic_addr: u32,
    pub flags: u32,
}

impl fmt::Debug for MADTHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MADTHeader")
            .field("local_apic_addr", &FormatHex(self.local_apic_addr))
            .field("flags", &FormatHex(self.flags))
            .finish()
    }
}

#[derive(Copy, Clone, Debug)]
pub enum MADTRecord {
    LocalAPIC { processor_id: u8, apic_id: u8, flags: u32 },
    IOAPIC { id: u8, addr: u32, global_interrupt_base: u32 },
    InterruptSourceOverride { bus_source: u8, irq_source: u8, global_interrupt: u32, flags: u16 },
    NonMaskableSource { nmi_source: u8, flags: u16, global_interrupt: u32 },
    LocalNonMaskable { processor_id: u8, flags: u16, lint: u8 },
    LocalAddressOverride { apic_addr: u64 },
    LocalX2APIC { processor_id: u32, flags: u32, acpi_id: u32 },
}

impl MADTRecord {
    pub fn from_raw_data(raw: &[u8]) -> Option<Self> {
        if raw.len() < 2 {
            None
        } else {
            let entry_kind = raw[0];
            //let record_length = raw[1];

            debug!("entry kind: {entry_kind:?}");

            match entry_kind {
                0 => {
                    if raw.len() >= 8 {
                        Some(Self::LocalAPIC {
                            processor_id: raw[2],
                            apic_id: raw[3],
                            flags: unsafe { *(&raw[4] as *const _ as *const u32) },
                        })
                    } else {
                        None
                    }
                }
                1 => {
                    if raw.len() >= 12 {
                        Some(Self::IOAPIC {
                            id: raw[2],
                            addr: unsafe { *(&raw[4] as *const _ as *const u32) },
                            global_interrupt_base: unsafe { *(&raw[8] as *const _ as *const u32) },
                        })
                    } else {
                        None
                    }
                }
                2 => {
                    if raw.len() >= 10 {
                        Some(Self::InterruptSourceOverride {
                            bus_source: raw[2],
                            irq_source: raw[3],
                            global_interrupt: unsafe { *(&raw[4] as *const _ as *const u32) },
                            flags: unsafe { *(&raw[8] as *const _ as *const u16) },
                        })
                    } else {
                        None
                    }
                }
                3 => {
                    if raw.len() >= 10 {
                        Some(Self::NonMaskableSource {
                            nmi_source: raw[2],
                            flags: unsafe { *(&raw[4] as *const _ as *const u16) },
                            global_interrupt: unsafe { *(&raw[6] as *const _ as *const u32) },
                        })
                    } else {
                        None
                    }
                }
                4 => {
                    if raw.len() >= 6 {
                        Some(Self::LocalNonMaskable {
                            processor_id: raw[2],
                            flags: unsafe { *(&raw[3] as *const _ as *const u16) },
                            lint: raw[5],
                        })
                    } else {
                        None
                    }
                }
                5 => {
                    if raw.len() >= 12 {
                        Some(Self::LocalAddressOverride {
                            apic_addr: unsafe { *(&raw[4] as *const _ as *const u64) },
                        })
                    } else {
                        None
                    }
                }
                9 => {
                    if raw.len() >= 12 {
                        Some(Self::LocalX2APIC {
                            processor_id: unsafe { *(&raw[4] as *const _ as *const u32) },
                            flags: unsafe { *(&raw[8] as *const _ as *const u32) },
                            acpi_id: unsafe { *(&raw[12] as *const _ as *const u32) },
                        })
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct MADT {
    pub header: MADTHeader,
    pub records: Vec<MADTRecord>,
}

impl MADT {
    pub fn from_raw_data(raw: &[u8]) -> Option<Self> {
        if raw.len() >= 8 {
            let header = unsafe { *(&raw[0] as *const _ as *const MADTHeader) };
            let mut records = Vec::new();
            let mut offset = size_of::<MADTHeader>();

            while offset <= (raw.len() - 1) {
                let record = MADTRecord::from_raw_data(&raw[offset..]);
                debug!("found record {record:?} @ offset {offset:#x}");

                if let Some(record) = record {
                    records.push(record);
                }

                let size = raw[offset + 1] as usize;

                if size == 0 {
                    break;
                } else {
                    offset += size;
                }
            }

            Some(Self { header, records })
        } else {
            None
        }
    }
}

/// finds ACPI system descriptor table pointers from the global RSDP and (R|X)SDT
pub fn find_sdts() -> Option<Vec<u64>> {
    if let Some(addr) = find_rsdp() {
        debug!("rsdp @ {addr:#x}");

        let rsdp = read_rsdp(addr).expect("failed to read RSDP");

        // accessing these union fields is perfectly safe
        if unsafe { rsdp.original.revision } > 0 {
            // acpi 2.0+ uses extended fields in the RSDP
            let rsdp = unsafe { rsdp.extended };

            debug!("assuming ACPI revision 2.0+");

            if calculate_checksum(&rsdp) & 0xff != 0 {
                error!("RSDP checksum invalid");
                return None;
            }

            debug!("rsdp is {rsdp:#?}");

            let sdt = read_sdt::<u64>(rsdp.xsdt_address).expect("failed to read XSDT");

            if !sdt.verify_checksum() {
                error!("XSDT checksum invalid");
                return None;
            }

            Some(sdt.sdt_pointers)
        } else {
            // acpi 1.0 doesn't use extended fields in the RSDP
            let rsdp = unsafe { rsdp.original };

            debug!("assuming ACPI revision 1.0");

            if calculate_checksum(&rsdp) & 0xff != 0 {
                error!("RSDP checksum invalid");
                return None;
            }

            debug!("rsdp is {rsdp:#?}");

            let sdt = read_sdt::<u32>(rsdp.rsdt_address as u64).expect("failed to read RSDT");

            if !sdt.verify_checksum() {
                error!("RSDT checksum invalid");
                return None;
            }

            // convert all pointers to u64
            Some(sdt.sdt_pointers.iter().map(|&p| p as u64).collect())
        }
    } else {
        debug!("couldn't find RSDP");

        None
    }
}

pub fn find_madt(sdt_pointers: &[u64]) -> Option<MADT> {
    // find the MADT
    for &ptr in sdt_pointers.iter() {
        if let Some(header) = read_header(ptr) {
            debug!("found header {:?}", header);

            // check for MADT signature ("APIC")
            if header.signature == [b'A', b'P', b'I', b'C'] {
                // read MADT data
                if let Some(data) = read_data(ptr, header.length) {
                    if (calculate_checksum(&header) + calculate_checksum_bytes(&data)) & 0xff != 0 {
                        error!("MADT checksum invalid");
                    } else {
                        return MADT::from_raw_data(&data);
                    }
                } else {
                    error!("failed to read MADT data");
                }
            }
        } else {
            warn!("ACPI SDT @ {ptr:#x} is invalid");
        }
    }

    None
}

pub fn detect_cpus(topology: Option<&super::CPUTopology>, mapping: Option<&APICToCPU>) -> Option<(super::APICToCPU, CPU)> {
    let mut mapping = mapping.cloned();

    let sdt_pointers = find_sdts().unwrap();
    let madt = find_madt(&sdt_pointers).unwrap();

    let mut local_apic_addr: u64 = madt.header.local_apic_addr as u64;
    let mut local_apics: Vec<(usize, usize, ThreadID)> = Vec::new();

    // parse MADT records
    for record in madt.records.iter() {
        match record {
            MADTRecord::LocalAPIC { processor_id, apic_id, flags } => {
                if (flags & 0x3) > 0 {
                    let thread_id = if let Some(m) = mapping.as_ref() {
                        m.apic_to_cpu(*apic_id as usize)
                    } else {
                        ThreadID {
                            core: *processor_id as usize,
                            thread: 0,
                        }
                    };
                    local_apics.push((*apic_id as usize, *processor_id as usize, thread_id));
                }
            }
            MADTRecord::LocalX2APIC { processor_id, flags, acpi_id } => {
                if (flags & 0x3) > 0 {
                    let thread_id = if let Some(m) = mapping.as_ref() {
                        m.apic_to_cpu(*processor_id as usize)
                    } else {
                        ThreadID { core: *acpi_id as usize, thread: 0 }
                    };
                    local_apics.push((*processor_id as usize, *acpi_id as usize, thread_id));
                }
            }
            MADTRecord::LocalAddressOverride { apic_addr } => local_apic_addr = *apic_addr,
            _ => (),
        }
    }

    debug!("local APIC is mapped @ {local_apic_addr:#x}");

    // sanity check
    assert!((local_apic_addr % PageDir::PAGE_SIZE as u64) == 0, "local APIC address isn't page aligned");

    super::apic::map_local_apic(local_apic_addr);

    // set up CPUs
    let mut cpus = CPU::new();

    let num_cores = match topology.as_ref() {
        Some(topology) => topology.num_cores,
        None => local_apics.len(),
    };

    let threads_per_core = match topology.as_ref() {
        Some(topology) => topology.threads_per_core,
        None => 1,
    };

    // populate cores
    for core_idx in 0..num_cores {
        cpus.add_core();

        for thread_idx in 0..threads_per_core {
            cpus.cores.get_mut(core_idx).unwrap().add_thread(
                super::ThreadInfo {
                    apic_id: None,
                    processor_id: 0,
                    stack: None,
                },
                crate::timer::register_timer(Some(ThreadID { core: core_idx, thread: thread_idx }), 0)
                    .unwrap_or_else(|err| panic!("failed to register timer for CPU {core_idx}:{thread_idx}: {err:?}")),
            );
        }
    }

    // populate threads
    for (apic_id, processor_id, thread_id) in local_apics.iter() {
        let thread = cpus
            .get_thread_mut(*thread_id)
            .unwrap_or_else(|| panic!("couldn't get CPU {thread_id} for local APIC {apic_id}, address is probably invalid"));

        thread.info.apic_id = Some(*apic_id);
        thread.info.processor_id = *processor_id;
    }

    if mapping.is_none() {
        // check if all APIC IDs are contiguous, if not assemble an arbitrary mapping
        let mut is_contiguous = true;
        let mut last_id = None;
        let mut highest_id = 0;

        for (apic_id, _, _) in local_apics.iter() {
            let apic_id = *apic_id;

            if is_contiguous {
                if let Some(last) = last_id {
                    if apic_id != last + 1 {
                        is_contiguous = false;
                    }
                }

                last_id = Some(apic_id);
            }

            if apic_id > highest_id {
                highest_id = apic_id;
            }
        }

        if is_contiguous {
            mapping = Some(APICToCPU::OneToOne);
        } else {
            let mut translation = vec![ThreadID { core: 0, thread: 0 }; highest_id + 1];

            for (apic_id, _, thread_id) in local_apics.iter() {
                translation[*apic_id] = *thread_id;
            }

            mapping = Some(APICToCPU::Arbitrary { translation })
        }
    }

    Some((mapping.unwrap_or_default(), cpus))
}
