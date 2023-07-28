//! x86 non-PAE paging

use super::{PAGE_SIZE, SPLIT_ADDR};
use crate::{
    arch::PhysicalAddress,
    mm::{PageDirectory, PageFrame, PagingError, ReservedMemory},
};
use alloc::boxed::Box;
use bitmask_enum::bitmask;
use core::{arch::asm, fmt, pin::Pin};
use log::{error, trace};

/// entry in a page table
#[repr(transparent)]
#[derive(Copy, Clone, Default)]
struct PageTableEntry(u32);

impl PageTableEntry {
    /// create new page table entry
    const fn new(addr: u32, flags: PageTableFlags) -> Self {
        Self((addr & 0xfffff000) | (flags.bits & 0x0fff) as u32)
    }

    /// create an unused page table entry
    const fn new_unused() -> Self {
        Self(0)
    }

    /// set address of page table entry
    /*fn set_address(&mut self, addr: u32) {
        self.0 = (self.0 & 0x00000fff) | (addr & 0xfffff000);
    }*/

    /// set flags of page table entry
    fn set_flags(&mut self, flags: PageTableFlags) {
        self.0 = (self.0 & 0xfffff000) | (flags.bits & 0x00000fff) as u32;
    }

    /// checks if this page table entry is unused
    fn is_unused(&self) -> bool {
        self.0 == 0 // lol. lmao
    }

    /// set page as unused and clear its fields
    /*fn set_unused(&mut self) {
        self.0 = 0;
    }*/

    /// gets address of page table entry
    fn get_address(&self) -> u32 {
        self.0 & 0xfffff000
    }

    /// gets flags of page table entry
    fn get_flags(&self) -> u16 {
        (self.0 & 0x00000fff) as u16
    }
}

impl From<PageTableEntry> for PageFrame {
    fn from(entry: PageTableEntry) -> Self {
        let flags = entry.get_flags();
        Self {
            addr: entry.get_address() as PhysicalAddress,
            present: flags & PageTableFlags::Present.bits > 0,
            user_mode: flags & PageTableFlags::UserSupervisor.bits > 0,
            writable: flags & PageTableFlags::ReadWrite.bits > 0,
            copy_on_write: flags & PageTableFlags::CopyOnWrite.bits > 0,
            executable: true,
            referenced: flags & PageTableFlags::Referenced.bits > 0,
        }
    }
}

impl TryFrom<PageFrame> for PageTableEntry {
    type Error = ();

    fn try_from(frame: PageFrame) -> Result<Self, Self::Error> {
        let mut flags = PageTableFlags::None;

        if frame.present {
            flags |= PageTableFlags::Present;
        }

        if frame.user_mode {
            flags |= PageTableFlags::UserSupervisor;
        }

        if frame.writable {
            flags |= PageTableFlags::ReadWrite;
        }

        if frame.copy_on_write {
            flags |= PageTableFlags::CopyOnWrite;
        }

        if frame.referenced {
            flags |= PageTableFlags::Referenced;
        }

        Ok(PageTableEntry::new(frame.addr, flags))
    }
}

impl fmt::Debug for PageTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let addr = (self.0 & 0xfffff000) as *const u8;
        let flags = PageTableFlags { bits: (self.0 & 0x0fff) as u16 };

        f.debug_struct("PageTableEntry").field("address", &addr).field("flags", &flags).finish()
    }
}

/// page table entry flags
#[bitmask(u16)]
enum PageTableFlags {
    /// no flags?
    None = 0,

    /// page is present in memory and can be accessed
    Present = 1 << 0,

    /// code can read and write to page
    ///
    /// absence of this flag forces read only
    ReadWrite = 1 << 1,

    /// page is accessible in user mode
    ///
    /// absence of this flag only allows supervisor access
    UserSupervisor = 1 << 2,

    /// enables write-through caching instead of write-back
    ///
    /// requires page attribute table
    PageWriteThru = 1 << 3,

    /// disables caching for this page
    ///
    /// requires page attribute table
    PageCacheDisable = 1 << 4,

    /// set if page has been accessed during address translation
    Accessed = 1 << 5,

    /// set if page has been written to
    Dirty = 1 << 6,

    /// can be set if page attribute table is supported, allows setting cache disable and write thru bits
    PageAttributeTable = 1 << 7,

    /// tells cpu to not invalidate this page table entry in cache when page tables are reloaded
    Global = 1 << 8,

    /// if this bit is set and the writable bit is not, the page will be copied into a new page when written to
    CopyOnWrite = 1 << 9,

    /// signifies that this page may have more than one reference and should be cleaned up with the reference counter
    Referenced = 1 << 10,
}

impl fmt::Display for PageTableFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PageTableFlags {{")?;

        if (*self & Self::Present).bits() > 0 {
            write!(f, " present,")?;
        }

        if (*self & Self::ReadWrite).bits() > 0 {
            write!(f, " read/write")?;
        } else {
            write!(f, " read only")?;
        }

        if (*self & Self::UserSupervisor).bits() > 0 {
            write!(f, ", user + supervisor mode")?;
        } else {
            write!(f, ", supervisor mode")?;
        }

        if (*self & Self::PageWriteThru).bits() > 0 {
            write!(f, ", write thru")?;
        }

        if (*self & Self::PageCacheDisable).bits() > 0 {
            write!(f, ", cache disable")?;
        }

        if (*self & Self::Accessed).bits() > 0 {
            write!(f, ", accessed")?;
        }

        if (*self & Self::Dirty).bits() > 0 {
            write!(f, ", dirty")?;
        }

        if (*self & Self::PageAttributeTable).bits() > 0 {
            write!(f, ", page attribute table")?;
        }

        if (*self & Self::Global).bits() > 0 {
            write!(f, ", global")?;
        }

        if (*self & Self::CopyOnWrite).bits() > 0 {
            write!(f, ", copy on write")?;
        }

        if (*self & Self::Referenced).bits() > 0 {
            write!(f, ", reference counted")?;
        }

        write!(f, " }}")
    }
}

/// entry in a page directory
#[repr(transparent)]
#[derive(Copy, Clone, Default)]
struct PageDirEntry(u32);

impl PageDirEntry {
    /// create new page directory entry
    const fn new(addr: u32, flags: PageDirFlags) -> Self {
        Self((addr & 0xfffff000) | (flags.bits & 0x0fff) as u32)
    }

    /*/// create an unused page directory entry
    const fn new_unused() -> Self {
        Self(0)
    }

    /// set address of page directory entry
    fn set_address(&mut self, addr: u32) {
        self.0 = (self.0 & 0x00000fff) | (addr & 0xfffff000);
    }

    /// set flags of page directory entry
    fn set_flags(&mut self, flags: PageDirFlags) {
        self.0 = (self.0 & 0xfffff000) | (flags.bits & 0x0fff) as u32;
    }

    /// checks if this page dir entry is unused
    fn is_unused(&self) -> bool {
        self.0 == 0 // lol. lmao
    }

    /// set page dir as unused and clear its fields
    fn set_unused(&mut self) {
        self.0 = 0;
    }

    /// gets address of page directory entry
    fn get_address(&self) -> u32 {
        self.0 & 0xfffff000
    }

    /// gets flags of page directory entry
    fn get_flags(&self) -> u16 {
        (self.0 & 0x00000fff) as u16
    }*/
}

impl fmt::Debug for PageDirEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let addr = (self.0 & 0xfffff000) as *const u8;
        let flags = PageDirFlags { bits: (self.0 & 0x0fff) as u16 };

        f.debug_struct("PageDirEntry").field("address", &addr).field("flags", &flags).finish()
    }
}

/// page directory entry flags
/// all absent flags override flags of children, i.e. not having the read write bit set prevents
/// all page table entries in the page directory from being writable
#[bitmask(u16)]
enum PageDirFlags {
    /// no flags?
    None = 0,

    /// pages are present in memory and can be accessed
    Present = 1 << 0,

    /// code can read/write to pages
    ///
    /// absence of this flag forces read only
    ReadWrite = 1 << 1,

    /// pages are accessible in user mode
    ///
    /// absence of this flag only allows supervisor access
    UserSupervisor = 1 << 2,

    /// enables write-through caching instead of write-back
    ///
    /// requires page attribute table
    PageWriteThru = 1 << 3,

    /// disables caching for this page
    /// requires page attribute table
    PageCacheDisable = 1 << 4,

    /// set if page has been accessed during address translation
    Accessed = 1 << 5,

    /// set if page has been written to
    ///
    /// only available if page is large
    Dirty = 1 << 6,

    /// enables large (4mb) pages
    ///
    /// no support currently
    PageSize = 1 << 7,

    /// tells cpu to not invalidate this page table entry in cache when page tables are reloaded
    Global = 1 << 8,

    /// can be set if page attribute table is supported, allows setting cache disable and write thru bits
    PageAttributeTable = 1 << 12,
}

impl fmt::Display for PageDirFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PageDirFlags {{")?;

        if self.bits & (1 << 0) > 0 {
            write!(f, " present,")?;
        }

        if self.bits & (1 << 1) > 0 {
            write!(f, " read/write")?;
        } else {
            write!(f, " read only")?;
        }

        if self.bits & (1 << 2) > 0 {
            write!(f, ", user + supervisor mode")?;
        } else {
            write!(f, ", supervisor mode")?;
        }

        if self.bits & (1 << 3) > 0 {
            write!(f, ", write thru")?;
        }

        if self.bits & (1 << 4) > 0 {
            write!(f, ", cache disable")?;
        }

        if self.bits & (1 << 5) > 0 {
            write!(f, ", accessed")?;
        }

        if self.bits & (1 << 6) > 0 {
            write!(f, ", dirty")?;
        }

        if self.bits & (1 << 7) > 0 {
            write!(f, ", large")?;
        }

        if self.bits & (1 << 8) > 0 {
            write!(f, ", global")?;
        }

        if self.bits & (1 << 12) > 0 {
            write!(f, ", page attribute table")?;
        }

        write!(f, " }}")
    }
}

/// struct for page table
///
/// basically just a wrapper for the array lmao
#[derive(Debug)]
#[repr(C)]
#[repr(align(4096))]
struct InternalPageTable {
    entries: [PageTableEntry; 1024],
}

impl Default for InternalPageTable {
    fn default() -> Self {
        Self {
            entries: [PageTableEntry::new_unused(); 1024],
        }
    }
}

/// stores a heap-allocated page table
#[repr(C)]
#[derive(Debug)]
pub struct TableRef {
    table: Pin<Box<InternalPageTable>>,
}

impl ReservedMemory for TableRef {
    fn allocate() -> Result<Self, PagingError> {
        Ok(Self {
            table: unsafe { Box::into_pin(Box::<InternalPageTable>::try_new_zeroed().map_err(|_| PagingError::AllocError)?.assume_init()) },
        })
    }
}

#[derive(Debug)]
#[repr(C)]
#[repr(align(4096))]
struct InternalPageDir {
    tables: [PageDirEntry; 1024],
}

/// x86 non-PAE PageDirectory implementation
#[repr(C)]
#[derive(Debug)]
pub struct PageDir {
    /// pointers to page tables
    tables: Box<[Option<TableRef>; 1024]>,

    /// physical addresses of page tables
    tables_physical: Pin<Box<InternalPageDir>>,

    /// physical address of tables_physical
    tables_physical_addr: u32,
}

impl PageDir {
    /// adds an existing top level page table to the page directory
    fn add_page_table(&mut self, addr: u32, table: Pin<Box<InternalPageTable>>, physical_addr: u32) {
        //assert!(addr & ((1 << 22) - 1) == 0, "address is not page table aligned (22 bits)");

        let idx = (addr >> 22) as usize;

        if self.tables[idx].is_some() {
            error!("overwriting an existing page table at {:#x} ({:#x})", addr, idx);
        }

        trace!("adding a new page table for virt {:#x} @ {:#x} (phys {:#x})", addr, &*table as *const _ as usize, physical_addr);

        if idx >= SPLIT_ADDR / PAGE_SIZE / 1024 {
            self.tables_physical.tables[idx] = PageDirEntry::new(physical_addr, PageDirFlags::Present | PageDirFlags::ReadWrite | PageDirFlags::UserSupervisor | PageDirFlags::Global);
        } else {
            self.tables_physical.tables[idx] = PageDirEntry::new(physical_addr, PageDirFlags::Present | PageDirFlags::ReadWrite | PageDirFlags::UserSupervisor);
        }

        trace!("physical entry is {:#x} ({:?})", self.tables_physical.tables[idx].0, self.tables_physical.tables[idx]);

        self.tables[idx] = Some(TableRef { table });
    }

    fn insert_page(&mut self, page: Option<PageFrame>, addr: usize, table_idx: usize) -> Result<(), PagingError> {
        let mut entry = if let Some(page) = page {
            page.try_into().map_err(|_| PagingError::BadFrame)?
        } else {
            PageTableEntry::new_unused()
        };

        if addr >= SPLIT_ADDR {
            entry.set_flags(PageTableFlags {
                bits: entry.get_flags() | PageTableFlags::Global.bits,
            });
        }

        self.tables[table_idx].as_mut().unwrap().table.entries[addr % 1024] = entry;

        Ok(())
    }
}

impl PageDirectory for PageDir {
    const PAGE_SIZE: usize = PAGE_SIZE;
    type Reserved = TableRef;

    fn new(current_dir: &impl PageDirectory) -> Result<Self, PagingError> {
        unsafe {
            let tables = {
                // assume_init() is required here since otherwise there's no way to initialize the array without an expensive stack copy
                let mut allocated: Box<[Option<TableRef>; 1024]> = Box::try_new_uninit().map_err(|_| PagingError::AllocError)?.assume_init();

                for table_ref in allocated.iter_mut() {
                    *table_ref = None;
                }

                allocated
            };

            // assume_init() is ok here because PageDirEntry is transparent and needs to be zeroed out
            let tables_physical = Box::into_pin(Box::<InternalPageDir>::try_new_zeroed().map_err(|_| PagingError::AllocError)?.assume_init());

            let tables_physical_addr = current_dir
                .virt_to_phys(&*tables_physical as *const _ as usize)
                .expect("allocated memory not mapped into kernel memory");

            Ok(Self {
                tables,
                tables_physical,
                tables_physical_addr,
            })
        }
    }

    fn get_page(&self, mut addr: usize) -> Option<PageFrame> {
        addr /= PAGE_SIZE;

        let table_idx = addr / 1024;

        if let Some(table) = self.tables[table_idx].as_ref() {
            let entry = table.table.entries[addr % 1024];

            if entry.is_unused() {
                None
            } else {
                Some(entry.into())
            }
        } else {
            None
        }
    }

    fn is_unused(&self, mut addr: usize) -> bool {
        addr /= PAGE_SIZE;

        let table_idx = addr / 1024;

        if let Some(table) = self.tables[table_idx].as_ref() {
            table.table.entries[addr % 1024].is_unused()
        } else {
            true
        }
    }

    fn virt_to_phys(&self, mut virt: usize) -> Option<PhysicalAddress> {
        virt /= PAGE_SIZE;

        let table_idx = virt / 1024;

        if let Some(table) = self.tables[table_idx].as_ref() {
            let entry = table.table.entries[virt % 1024];

            if entry.is_unused() {
                None
            } else {
                Some(entry.get_address() as PhysicalAddress)
            }
        } else {
            None
        }
    }

    fn set_page(&mut self, current_dir: &impl PageDirectory, mut addr: usize, page: Option<PageFrame>) -> Result<(), PagingError> {
        addr /= PAGE_SIZE;

        let table_idx = addr / 1024;

        if self.tables[table_idx].is_none() {
            if page.is_none() {
                return Ok(());
            }

            // allocate memory for a new page-aligned page table
            let table = unsafe { Box::into_pin(Box::<InternalPageTable>::try_new_zeroed().map_err(|_| PagingError::AllocError)?.assume_init()) };

            // get the physical address of our new page table
            let phys = current_dir.virt_to_phys(&*table as *const _ as usize).expect("new page table isn't mapped into kernel memory");

            self.add_page_table((addr * PAGE_SIZE).try_into().unwrap(), table, phys);
        }

        self.insert_page(page, addr, table_idx)
    }

    fn set_page_no_alloc(&mut self, current_dir: &impl PageDirectory, mut addr: usize, page: Option<PageFrame>, reserved_memory: Option<Self::Reserved>) -> Result<(), PagingError> {
        addr /= PAGE_SIZE;

        let table_idx = addr / 1024;

        if self.tables[table_idx].is_none() {
            if page.is_none() {
                return Ok(());
            }

            let table = match reserved_memory {
                Some(reserved) => reserved.table,
                None => return Err(PagingError::AllocError),
            };

            // get the physical address of our new page table
            let phys = current_dir.virt_to_phys(&*table as *const _ as usize).expect("new page table isn't mapped into kernel memory");

            self.add_page_table((addr * PAGE_SIZE).try_into().unwrap(), table, phys);
        }

        self.insert_page(page, addr, table_idx)
    }

    unsafe fn switch_to(&self) {
        // check if the reference to this page directory is in kernel memory, and will be valid across *up to date* page directories
        assert!(self as *const _ as usize >= SPLIT_ADDR, "current page directory reference isn't in kernel memory");

        trace!("switching to page table @ {:#x}", self.tables_physical_addr);

        asm!(
            "mov cr3, {0}",
            in(reg) self.tables_physical_addr,
        );
    }
}
