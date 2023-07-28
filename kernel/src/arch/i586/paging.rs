//! x86 non-PAE paging

use super::{PAGE_SIZE, SPLIT_ADDR};
use crate::{
    arch::PhysicalAddress,
    mm::{PageDirectory, PageFrame, PagingError, ReservedMemory},
    FormatHex,
};
use alloc::alloc::{alloc, alloc_zeroed, dealloc, Layout};
use bitmask_enum::bitmask;
use core::{arch::asm, fmt, mem::size_of};
use log::{error, trace};
use x86::tlb::flush;

/// entry in a page table
#[repr(transparent)]
#[derive(Copy, Clone, Default)]
pub struct PageTableEntry(u32);

impl PageTableEntry {
    /// create new page table entry
    pub const fn new(addr: u32, flags: PageTableFlags) -> Self {
        Self((addr & 0xfffff000) | (flags.bits & 0x0fff) as u32)
    }

    /// create an unused page table entry
    pub const fn new_unused() -> Self {
        Self(0)
    }

    /// set address of page table entry
    pub fn set_address(&mut self, addr: u32) {
        self.0 = (self.0 & 0x00000fff) | (addr & 0xfffff000);
    }

    /// set flags of page table entry
    pub fn set_flags(&mut self, flags: PageTableFlags) {
        self.0 = (self.0 & 0xfffff000) | (flags.bits & 0x00000fff) as u32;
    }

    /// checks if this page table entry is unused
    pub fn is_unused(&self) -> bool {
        self.0 == 0 // lol. lmao
    }

    /// set page as unused and clear its fields
    pub fn set_unused(&mut self) {
        self.0 = 0;
    }

    /// gets address of page table entry
    pub fn get_address(&self) -> u32 {
        self.0 & 0xfffff000
    }

    /// gets flags of page table entry
    pub fn get_flags(&self) -> u16 {
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

        Ok(PageTableEntry::new(frame.addr.try_into().map_err(|_| ())?, flags))
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
pub enum PageTableFlags {
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
pub struct PageDirEntry(u32);

impl PageDirEntry {
    /// create new page directory entry
    pub const fn new(addr: u32, flags: PageDirFlags) -> Self {
        Self((addr & 0xfffff000) | (flags.bits & 0x0fff) as u32)
    }

    /// create an unused page directory entry
    pub const fn new_unused() -> Self {
        Self(0)
    }

    /// set address of page directory entry
    pub fn set_address(&mut self, addr: u32) {
        self.0 = (self.0 & 0x00000fff) | (addr & 0xfffff000);
    }

    /// set flags of page directory entry
    pub fn set_flags(&mut self, flags: PageDirFlags) {
        self.0 = (self.0 & 0xfffff000) | (flags.bits & 0x0fff) as u32;
    }

    /// checks if this page dir entry is unused
    pub fn is_unused(&self) -> bool {
        self.0 == 0 // lol. lmao
    }

    /// set page dir as unused and clear its fields
    pub fn set_unused(&mut self) {
        self.0 = 0;
    }

    /// gets address of page directory entry
    pub fn get_address(&self) -> u32 {
        self.0 & 0xfffff000
    }

    /// gets flags of page directory entry
    pub fn get_flags(&self) -> u16 {
        (self.0 & 0x00000fff) as u16
    }
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
pub enum PageDirFlags {
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
#[repr(transparent)]
pub struct PageTable {
    pub entries: [PageTableEntry; 1024],
}

impl Default for PageTable {
    fn default() -> Self {
        Self {
            entries: [PageTableEntry::new_unused(); 1024],
        }
    }
}

/// wrapper for a reference to a page table to help us manage allocations
///
/// allows us to store whether this reference was automatically allocated so it can be freed when its page directory is dropped
#[repr(C)]
pub struct TableRef<'a> {
    /// reference to the page table
    pub table: &'a mut PageTable,
}

impl fmt::Debug for TableRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TableRef").field("table", &FormatHex(self.table as *const _ as usize)).finish()
    }
}

/// x86 non-PAE PageDirectory implementation
#[repr(C)]
pub struct PageDir<'a> {
    /// pointers to page tables
    pub tables: &'a mut [Option<TableRef<'a>>; 1024],

    /// physical addresses of page tables
    pub tables_physical: &'a mut [PageDirEntry; 1024],

    /// physical address of tables_physical
    pub tables_physical_addr: u32,
}

impl fmt::Debug for PageDir<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TableRef")
            .field("tables", &FormatHex(&self.tables[0] as *const _ as usize))
            .field("tables_physical", &FormatHex(&self.tables_physical[0] as *const _ as usize))
            .field("tables_physical_addr", &self.tables_physical_addr)
            .finish()
    }
}

pub fn is_page_dir_current(page_dir: &PageDir) -> bool {
    page_dir.tables_physical_addr == unsafe { x86::controlregs::cr3() as u32 }
}

impl<'a> PageDir<'a> {
    /// constructs a new PageDir, allocating memory for it in the process
    pub fn new() -> Self {
        unsafe {
            let tables = {
                // alloc_zeroed prolly doesnt work for this
                let allocated = &mut *(alloc(Layout::new::<[Option<TableRef<'a>>; 1024]>()) as *mut [Option<TableRef<'a>>; 1024]);
                for table_ref in allocated.iter_mut() {
                    *table_ref = None;
                }
                allocated
            };

            let tables_physical = alloc_zeroed(Layout::from_size_align(size_of::<[PageDirEntry; 1024]>(), Self::PAGE_SIZE).unwrap());

            /*let tables_physical_addr = CURRENT_PAGE_DIR
            .as_mut()
            .expect("no current page directory")
            .virt_to_phys(tables_physical as usize)
            .expect("allocated memory not mapped into kernel memory");*/
            panic!("current page table virt_to_phys unimplemented");
            let tables_physical_addr = 0;

            Self {
                tables,
                tables_physical: &mut *(tables_physical as *mut [PageDirEntry; 1024]),
                tables_physical_addr: tables_physical_addr.try_into().unwrap(),
            }
        }
    }

    /// adds an existing top level page table to the page directory
    pub fn add_page_table(&mut self, addr: u32, table: &'a mut PageTable, physical_addr: u32) {
        //assert!(addr & ((1 << 22) - 1) == 0, "address is not page table aligned (22 bits)");

        let idx = (addr >> 22) as usize;

        if self.tables[idx].is_some() {
            error!("overwriting an existing page table at {:#x} ({:#x})", addr, idx);
        }

        trace!("adding a new page table for virt {:#x} @ {:#x} (phys {:#x})", addr, table as *mut _ as usize, physical_addr);

        if idx >= SPLIT_ADDR / PAGE_SIZE / 1024 {
            self.tables_physical[idx] = PageDirEntry::new(physical_addr, PageDirFlags::Present | PageDirFlags::ReadWrite | PageDirFlags::UserSupervisor | PageDirFlags::Global);
        } else {
            self.tables_physical[idx] = PageDirEntry::new(physical_addr, PageDirFlags::Present | PageDirFlags::ReadWrite | PageDirFlags::UserSupervisor);
        }

        trace!("physical entry is {:#x} ({:?})", self.tables_physical[idx].0, self.tables_physical[idx]);

        self.tables[idx] = Some(TableRef { table });
    }

    /// removes a top level page table from the page directory
    pub fn remove_page_table(&mut self, addr: u32) {
        //assert!(addr & ((1 << 22) - 1) == 0, "address is not page table aligned (22 bits)");

        let idx = (addr >> 22) as usize;
        let table = &mut self.tables[idx];

        if let Some(table_ref) = table.as_mut() {
            // get pointer to page table
            let ptr = table_ref.table as *mut PageTable as *mut u8;

            // mark page table as unused
            *table = None;
            self.tables_physical[idx].set_unused();

            // free page table
            unsafe {
                dealloc(ptr, Layout::from_size_align(size_of::<PageTable>(), PAGE_SIZE).unwrap());
            }
        }
    }

    /// checks whether we have a page table for this address already, or whether we have to allocate one
    pub fn has_page_table(&self, addr: u32) -> bool {
        //assert!(addr & ((1 << 22) - 1) == 0, "address is not page table aligned (22 bits)");

        let idx = (addr >> 22) as usize;
        self.tables[idx].is_some()
    }
}

impl<'a> Default for PageDir<'a> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PageDirReserved {}

impl ReservedMemory for PageDirReserved {
    fn allocate() -> Self {
        Self {}
    }
}

impl<'a> PageDirectory for PageDir<'a> {
    const PAGE_SIZE: usize = PAGE_SIZE;
    type Reserved = PageDirReserved;

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

    fn set_page(&mut self, mut addr: usize, page: Option<PageFrame>) -> Result<(), PagingError> {
        addr /= PAGE_SIZE;

        let table_idx = addr / 1024;

        if self.tables[table_idx].is_none() {
            // allocate memory for a new page-aligned page table
            let layout = Layout::from_size_align(size_of::<PageTable>(), PAGE_SIZE).unwrap();
            let ptr = unsafe { alloc_zeroed(layout) };

            if ptr.is_null() {
                Err(PagingError::AllocError)?;
            }

            // make sure this newly allocated page table is located in kernel memory so its reference will be valid as long as our current page directory has an up to date copy of the kernel's page directory
            assert!(ptr as usize >= SPLIT_ADDR, "new page table isn't in kernel memory");

            // get the physical address of our new page table
            /*let phys = unsafe {
                CURRENT_PAGE_DIR
                    .as_ref()
                    .expect("no reference to current page directory")
                    .virt_to_phys(ptr as usize)
                    .expect("new page table isn't mapped into kernel memory")
            };*/
            panic!("current page table virt_to_phys unimplemented");
            let phys = 0;

            self.add_page_table((addr * PAGE_SIZE).try_into().unwrap(), unsafe { &mut *(ptr as *mut PageTable) }, phys.try_into().unwrap());
        }

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

        //trace!("table is now {:?}", self.tables[table_idx].as_mut().unwrap().table.entries[(addr % 1024) as usize]);

        // invalidate this page in the tlb if we're modifying the current page directory
        if is_page_dir_current(self) {
            trace!("flushing {:#x} in tlb", addr * PAGE_SIZE);
            unsafe {
                flush(addr * PAGE_SIZE);
            }
        }

        Ok(())
    }

    fn set_page_no_alloc(&mut self, _addr: usize, _page: Option<PageFrame>, _reserved_memory: Option<Self::Reserved>) -> Result<(), PagingError> {
        todo!();
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

impl<'a> Drop for PageDir<'a> {
    fn drop(&mut self) {
        // free any allocated page tables
        for i in 0..1024 {
            self.remove_page_table(i << 22);
        }

        unsafe {
            dealloc(self.tables as *mut [Option<TableRef<'a>>; 1024] as *mut u8, Layout::new::<[Option<TableRef<'a>>; 1024]>());
            dealloc(self.tables_physical as *mut [PageDirEntry; 1024] as *mut u8, Layout::new::<[PageDirEntry; 1024]>());
        }
    }
}
