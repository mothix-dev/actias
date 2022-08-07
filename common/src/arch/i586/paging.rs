//! x86 non-PAE paging

use super::{get_eflags, LINKED_BASE, PAGE_SIZE};
use crate::mm::paging::{PageDirectory, PageError, PageFrame};
use alloc::alloc::{alloc, alloc_zeroed, dealloc, Layout};
use bitmask_enum::bitmask;
use core::{arch::asm, fmt, mem::size_of};
use log::{error, trace};
use x86::bits32::eflags::EFlags;

/// entry in a page table
#[repr(transparent)]
#[derive(Copy, Clone, Default)]
pub struct PageTableEntry(u32);

impl PageTableEntry {
    /// create new page table entry
    pub const fn new(addr: u32, flags: PageTableFlags) -> Self {
        Self((addr & 0xfffff000) | (flags.0 & 0x0fff) as u32)
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
        self.0 = (self.0 & 0xfffff000) | (flags.0 & 0x0fff) as u32;
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
            addr: entry.get_address() as u64,
            present: flags & PageTableFlags::Present.0 > 0,
            user_mode: flags & PageTableFlags::UserSupervisor.0 > 0,
            writable: flags & PageTableFlags::ReadWrite.0 > 0,
            copy_on_write: flags & PageTableFlags::CopyOnWrite.0 > 0,
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

        Ok(PageTableEntry::new(frame.addr.try_into().map_err(|_| ())?, flags))
    }
}

impl fmt::Debug for PageTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let addr = (self.0 & 0xfffff000) as *const u8;
        let flags = PageTableFlags((self.0 & 0x0fff) as u16);

        f.debug_struct("PageTableEntry").field("address", &addr).field("flags", &flags).finish()
    }
}

/// page table entry flags
#[bitmask(u16)]
#[repr(transparent)]
pub enum PageTableFlags {
    /// no flags?
    None = Self(0),

    /// page is present in memory and can be accessed
    Present = Self(1 << 0),

    /// code can read and write to page
    ///
    /// absence of this flag forces read only
    ReadWrite = Self(1 << 1),

    /// page is accessible in user mode
    ///
    /// absence of this flag only allows supervisor access
    UserSupervisor = Self(1 << 2),

    /// enables write-through caching instead of write-back
    ///
    /// requires page attribute table
    PageWriteThru = Self(1 << 3),

    /// disables caching for this page
    ///
    /// requires page attribute table
    PageCacheDisable = Self(1 << 4),

    /// set if page has been accessed during address translation
    Accessed = Self(1 << 5),

    /// set if page has been written to
    Dirty = Self(1 << 6),

    /// can be set if page attribute table is supported, allows setting cache disable and write thru bits
    PageAttributeTable = Self(1 << 7),

    /// tells cpu to not invalidate this page table entry in cache when page tables are reloaded
    Global = Self(1 << 8),

    /// if this bit is set and the present bit is not, the page will be copied into a new page when written to
    CopyOnWrite = Self(1 << 9),
}

impl fmt::Display for PageTableFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PageTableFlags {{")?;

        if self.0 & (1 << 0) > 0 {
            write!(f, " present,")?;
        }

        if self.0 & (1 << 1) > 0 {
            write!(f, " read/write")?;
        } else {
            write!(f, " read only")?;
        }

        if self.0 & (1 << 2) > 0 {
            write!(f, ", user + supervisor mode")?;
        } else {
            write!(f, ", supervisor mode")?;
        }

        if self.0 & (1 << 3) > 0 {
            write!(f, ", write thru")?;
        }

        if self.0 & (1 << 4) > 0 {
            write!(f, ", cache disable")?;
        }

        if self.0 & (1 << 5) > 0 {
            write!(f, ", accessed")?;
        }

        if self.0 & (1 << 6) > 0 {
            write!(f, ", dirty")?;
        }

        if self.0 & (1 << 7) > 0 {
            write!(f, ", page attribute table")?;
        }

        if self.0 & (1 << 8) > 0 {
            write!(f, ", global")?;
        }

        if self.0 & (1 << 9) > 0 {
            write!(f, ", copy on write")?;
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
        Self((addr & 0xfffff000) | (flags.0 & 0x0fff) as u32)
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
        self.0 = (self.0 & 0xfffff000) | (flags.0 & 0x0fff) as u32;
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
        let flags = PageDirFlags((self.0 & 0x0fff) as u16);

        f.debug_struct("PageDirEntry").field("address", &addr).field("flags", &flags).finish()
    }
}

/// page directory entry flags
/// all absent flags override flags of children, i.e. not having the read write bit set prevents
/// all page table entries in the page directory from being writable
#[bitmask(u16)]
#[repr(transparent)]
pub enum PageDirFlags {
    /// no flags?
    None = Self(0),

    /// pages are present in memory and can be accessed
    Present = Self(1 << 0),

    /// code can read/write to pages
    ///
    /// absence of this flag forces read only
    ReadWrite = Self(1 << 1),

    /// pages are accessible in user mode
    ///
    /// absence of this flag only allows supervisor access
    UserSupervisor = Self(1 << 2),

    /// enables write-through caching instead of write-back
    ///
    /// requires page attribute table
    PageWriteThru = Self(1 << 3),

    /// disables caching for this page
    /// requires page attribute table
    PageCacheDisable = Self(1 << 4),

    /// set if page has been accessed during address translation
    Accessed = Self(1 << 5),

    /// set if page has been written to
    ///
    /// only available if page is large
    Dirty = Self(1 << 6),

    /// enables large (4mb) pages
    ///
    /// no support currently
    PageSize = Self(1 << 7),

    /// tells cpu to not invalidate this page table entry in cache when page tables are reloaded
    Global = Self(1 << 8),

    /// can be set if page attribute table is supported, allows setting cache disable and write thru bits
    PageAttributeTable = Self(1 << 12),
}

impl fmt::Display for PageDirFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PageDirFlags {{")?;

        if self.0 & (1 << 0) > 0 {
            write!(f, " present,")?;
        }

        if self.0 & (1 << 1) > 0 {
            write!(f, " read/write")?;
        } else {
            write!(f, " read only")?;
        }

        if self.0 & (1 << 2) > 0 {
            write!(f, ", user + supervisor mode")?;
        } else {
            write!(f, ", supervisor mode")?;
        }

        if self.0 & (1 << 3) > 0 {
            write!(f, ", write thru")?;
        }

        if self.0 & (1 << 4) > 0 {
            write!(f, ", cache disable")?;
        }

        if self.0 & (1 << 5) > 0 {
            write!(f, ", accessed")?;
        }

        if self.0 & (1 << 6) > 0 {
            write!(f, ", dirty")?;
        }

        if self.0 & (1 << 7) > 0 {
            write!(f, ", large")?;
        }

        if self.0 & (1 << 8) > 0 {
            write!(f, ", global")?;
        }

        if self.0 & (1 << 12) > 0 {
            write!(f, ", page attribute table")?;
        }

        write!(f, " }}")
    }
}

/// struct for page table
///
/// basically just a wrapper for the array lmao
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
pub struct TableRef<'a> {
    /// reference to the page table
    table: &'a mut PageTable,

    /// whether we allocated this page table and thus can free it
    can_free: bool,
}

/// x86 non-PAE PageDirectory implementation
pub struct PageDir<'a> {
    /// pointers to page tables
    tables: &'a mut [Option<TableRef<'a>>; 1024],

    /// physical addresses of page tables
    tables_physical: &'a mut [PageDirEntry; 1024],

    /// physical address of tables_physical
    tables_physical_addr: u32,

    /// whether tables and tables_physical were allocated on the heap and thus can be freed
    can_free: bool,
}

/// contains a reference to the current page table if one has been set, used for virtual to physical address translations when allocating memory for new page tables
///
/// since we have checks to ensure the current page table must be in kernel memory, and all of kernel memory should be the same across all page tables,
/// having this reference be invalid is the least of our concerns
///
/// additionally, there are checks in place to prevent freeing the current page table to prevent potential use-after-free bugs
static mut CURRENT_PAGE_DIR: Option<&'static PageDir> = None;

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

            let tables_physical = alloc_zeroed(Layout::new::<[PageDirEntry; 1024]>());

            let tables_physical_addr = CURRENT_PAGE_DIR
                .as_mut()
                .expect("no current page directory")
                .virt_to_phys(tables_physical as usize)
                .expect("allocated memory not mapped into kernel memory");

            Self {
                tables,
                tables_physical: &mut *(tables_physical as *mut [PageDirEntry; 1024]),
                tables_physical_addr: tables_physical_addr.try_into().unwrap(),
                can_free: true,
            }
        }
    }

    /// constructs a new PageDir with a given physical page table array and the physical address of said array
    pub fn from_allocated(tables: &'a mut [Option<TableRef<'a>>; 1024], tables_physical: &'a mut [PageDirEntry; 1024], tables_physical_addr: u32) -> Self {
        trace!(
            "creating new PageDir from tables @ {:#x}, tables_physical @ {:#x}, tables_physical_addr @ {:#x}",
            tables as *mut _ as usize,
            tables_physical as *mut _ as usize,
            tables_physical_addr
        );

        Self {
            tables,
            tables_physical,
            tables_physical_addr,
            can_free: false,
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

        self.tables_physical[idx] = PageDirEntry::new(physical_addr, PageDirFlags::Present | PageDirFlags::ReadWrite | PageDirFlags::UserSupervisor);

        trace!("physical entry is {:#x} ({:?})", self.tables_physical[idx].0, self.tables_physical[idx]);

        self.tables[idx] = Some(TableRef { table, can_free: false });
    }

    /// removes a top level page table from the page directory
    pub fn remove_page_table(&mut self, addr: u32) {
        //assert!(addr & ((1 << 22) - 1) == 0, "address is not page table aligned (22 bits)");

        let idx = (addr >> 22) as usize;
        let table = &mut self.tables[idx];

        if let Some(table_ref) = table.as_mut() {
            if table_ref.can_free {
                // get pointer to page table
                let ptr = table_ref.table as *mut PageTable as *mut u8;

                // mark page table as unused
                *table = None;
                self.tables_physical[idx].set_unused();

                // free page table
                unsafe {
                    dealloc(ptr, Layout::from_size_align(size_of::<PageTable>(), PAGE_SIZE).unwrap());
                }
            } else {
                // just mark page table as unused since we can't free it
                *table = None;
                self.tables_physical[idx].set_unused();
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

impl<'a> PageDirectory for PageDir<'a> {
    fn get_page(&self, mut addr: usize) -> Option<PageFrame> {
        addr /= PAGE_SIZE;

        let table_idx = (addr / 1024) as usize;

        if let Some(table) = self.tables[table_idx].as_ref() {
            let entry = table.table.entries[(addr % 1024) as usize];

            if entry.is_unused() {
                None
            } else {
                Some(entry.into())
            }
        } else {
            None
        }
    }

    fn set_page(&mut self, mut addr: usize, page: Option<PageFrame>) -> Result<(), PageError> {
        addr /= PAGE_SIZE;

        let table_idx = (addr / 1024) as usize;

        if self.tables[table_idx].is_none() {
            // allocate memory for a new page-aligned page table
            let layout = Layout::from_size_align(size_of::<PageTable>(), PAGE_SIZE).unwrap();
            let ptr = unsafe { alloc_zeroed(layout) };

            if ptr.is_null() {
                Err(PageError::AllocError)?;
            }

            // make sure this newly allocated page table is located in kernel memory so its reference will be valid as long as our current page directory has an up to date copy of the kernel's page directory
            assert!(ptr as usize >= LINKED_BASE, "new page table isn't in kernel memory");

            // get the physical address of our new page table
            let phys = unsafe {
                CURRENT_PAGE_DIR
                    .as_ref()
                    .expect("no reference to current page directory")
                    .virt_to_phys(ptr as usize)
                    .expect("new page table isn't mapped into kernel memory")
            };

            self.tables_physical[table_idx] = PageDirEntry::new(phys as u32, PageDirFlags::Present | PageDirFlags::ReadWrite | PageDirFlags::UserSupervisor);

            self.tables[table_idx] = unsafe {
                Some(TableRef {
                    table: &mut *(ptr as *mut PageTable),
                    can_free: true,
                })
            };
        }

        self.tables[table_idx].as_mut().unwrap().table.entries[(addr % 1024) as usize] = if let Some(page) = page {
            page.try_into().unwrap() // FIXME: maybe come up with some way to handle this error so we can't crash here?
        } else {
            PageTableEntry::new_unused()
        };

        Ok(())
    }

    unsafe fn switch_to(&self) {
        // check if the reference to this page directory is in kernel memory, and will be valid across *up to date* page directories
        assert!(self as *const _ as usize >= LINKED_BASE, "current page directory reference isn't in kernel memory");

        trace!("switching to page table @ {:#x}", self.tables_physical_addr);

        // FIXME: add global state for if interrupts are enabled to avoid parsing eflags (which is slow)
        let enable_interrupts = get_eflags().contains(EFlags::FLAGS_IF);

        asm!(
            "cli", // we CANNOT afford for this code to be interrupted
            "mov cr3, {0}",
            "mov {1}, cr0",
            "or {1}, 0x80000000",
            "mov cr0, {1}",

            in(reg) self.tables_physical_addr,
            out(reg) _,
        );

        // effectively clone the reference to this page directory and put it in CURRENT_PAGE_DIR
        // this is horribly unsafe, however we do have checks in place to make sure this reference stays valid
        CURRENT_PAGE_DIR = Some(core::mem::transmute(self));

        if enable_interrupts {
            asm!("sti");
        }
    }

    fn page_size(&self) -> usize {
        PAGE_SIZE
    }
}

impl<'a> Drop for PageDir<'a> {
    fn drop(&mut self) {
        // sanity check, makes sure we're not freeing the current page directory
        if let Some(current) = unsafe { CURRENT_PAGE_DIR.as_ref() } {
            assert!(self as *const _ as usize != current as *const _ as usize, "attempted to free current page directory");
        }

        // free any allocated page tables
        for i in 0..1024 {
            self.remove_page_table(i << 22);
        }

        // only free this if we allocated it in the first place
        if self.can_free {
            unsafe {
                dealloc(self.tables as *mut [Option<TableRef<'a>>; 1024] as *mut u8, Layout::new::<[Option<TableRef<'a>>; 1024]>());
                dealloc(self.tables_physical as *mut [PageDirEntry; 1024] as *mut u8, Layout::new::<[PageDirEntry; 1024]>());
            }
        }
    }
}
