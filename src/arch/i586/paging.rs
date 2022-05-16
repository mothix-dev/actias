//! x86 non-PAE paging
// warning: this code is terrible. do not do anything like this

use core::arch::asm;
use core::fmt;
use bitmask_enum::bitmask;
use core::default::Default;
use core::mem::size_of;
use crate::util::array::BitSet;
use crate::mm::heap::KHEAP_INITIAL_SIZE;
use super::{MEM_SIZE, LINKED_BASE, KHEAP_START, PAGE_SIZE};

extern "C" {
    /// page directory, created in boot.S
    //static page_directory: [PageDirEntry; 1024]; // TODO: consider putting this array in a struct?

    /// located at end of kernel, used for calculating placement address
    static kernel_end: u32;
}

/// entry in a page table
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct PageTableEntry(u32);

impl PageTableEntry {
    /// create new page table entry
    pub fn new(addr: u32, flags: PageTableFlags) -> Self {
        Self((addr & 0xfffff000) | (flags.0 & 0x0fff) as u32)
    }

    /// create an unused page table entry
    pub fn new_unused() -> Self {
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
}

impl fmt::Display for PageTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "PageTableEntry {{")?;
        writeln!(f, "    address: {:#x},", self.0 & 0xfffff000)?;
        writeln!(f, "    flags: {}", PageDirFlags((self.0 & 0x0fff) as u16))?;
        write!(f, "}}")
    }
}

/// page table entry flags
#[bitmask(u16)]
pub enum PageTableFlags {
    /// page is present in memory and can be accessed
    Present             = Self(1 << 0),

    /// code can read and write to page
    /// absence of this flag forces read only
    ReadWrite           = Self(1 << 1),

    /// page is accessible in user mode
    /// absence of this flag only allows supervisor access
    UserSupervisor      = Self(1 << 2),

    /// enables write-through caching instead of write-back
    /// requires page attribute table
    PageWriteThru       = Self(1 << 3),

    /// disables caching for this page
    /// requires page attribute table
    PageCacheDisable    = Self(1 << 4),

    /// set if page has been accessed during address translation
    Accessed            = Self(1 << 5),

    /// set if page has been written to
    Dirty               = Self(1 << 6),

    /// can be set if page attribute table is supported, allows setting cache disable and write thru bits
    PageAttributeTable  = Self(1 << 7),

    /// tells cpu to not invalidate this page table entry in cache when page tables are reloaded
    Global              = Self(1 << 8),
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

        write!(f, " }}")
    }
}

/// entry in a page directory
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct PageDirEntry(u32);

impl PageDirEntry {
    /// create new page directory entry
    pub fn new(addr: u32, flags: PageTableFlags) -> Self {
        Self((addr & 0xfffff000) | (flags.0 & 0x0fff) as u32)
    }

    /// create an unused page directory entry
    pub fn new_unused() -> Self {
        Self(0)
    }

    /// set address of page directory entry
    pub fn set_address(&mut self, addr: u32) {
        self.0 = (self.0 & 0x00000fff) | (addr & 0xfffff000);
    }

    /// set flags of page directory entry
    pub fn set_flags(&mut self, flags: PageTableFlags) {
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
}

impl fmt::Display for PageDirEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "PageDirEntry {{")?;
        writeln!(f, "    address: {:#x},", self.0 & 0xfffff000)?;
        writeln!(f, "    flags: {}", PageDirFlags((self.0 & 0x0fff) as u16))?;
        write!(f, "}}")
    }
}

/// page directory entry flags
/// all absent flags override flags of children, i.e. not having the read write bit set prevents
/// all page table entries in the page directory from being writable
#[bitmask(u16)]
pub enum PageDirFlags {
    /// pages are present in memory and can be accessed
    Present             = Self(1 << 0),

    /// code can read/write to pages
    /// absence of this flag forces read only
    ReadWrite           = Self(1 << 1),

    /// pages are accessible in user mode
    /// absence of this flag only allows supervisor access
    UserSupervisor      = Self(1 << 2),

    /// enables write-through caching instead of write-back
    /// requires page attribute table
    PageWriteThru       = Self(1 << 3),

    /// disables caching for this page
    /// requires page attribute table
    PageCacheDisable    = Self(1 << 4),

    /// set if page has been accessed during address translation
    Accessed            = Self(1 << 5),

    /// set if page has been written to
    /// only available if page is large
    Dirty               = Self(1 << 6),

    /// enables large (4mb) pages
    /// no support currently
    PageSize            = Self(1 << 7),

    /// tells cpu to not invalidate this page table entry in cache when page tables are reloaded
    Global              = Self(1 << 8),

    /// can be set if page attribute table is supported, allows setting cache disable and write thru bits
    PageAttributeTable  = Self(1 << 12),
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

// based on http://www.jamesmolloy.co.uk/tutorial_html/6.-Paging.html

/// where to allocate memory
static mut PLACEMENT_ADDR: usize = 0; // to be filled in with end of kernel on init

/// result of kmalloc calls
pub struct MallocResult<T> {
    pub pointer: *mut T,
    pub phys_addr: usize,
}

/// extremely basic malloc- doesn't support free, only useful for allocating effectively static data
pub unsafe fn kmalloc<T>(size: usize, align: bool) -> MallocResult<T> {
    //log!("kmalloc {} @ {:#x}", size, PLACEMENT_ADDR);
    if align && (PLACEMENT_ADDR & 0xfffff000) > 0 { // if alignment is requested and we aren't already aligned
        PLACEMENT_ADDR &= 0xfffff000; // round down to nearest 4k block
        PLACEMENT_ADDR += 0x1000; // increment by 4k- we don't want to overwrite things
    }

    // increment address to make room for area of provided size, return pointer to start of area
    let tmp = PLACEMENT_ADDR;
    PLACEMENT_ADDR += size;

    if PLACEMENT_ADDR >= MEM_SIZE { // prolly won't happen but might as well
        panic!("out of memory (kmalloc)");
    }

    MallocResult {
        pointer: (tmp + LINKED_BASE) as *mut T,
        phys_addr: tmp,
    }
}

/// struct for page table
/// basically just a wrapper for the array lmao
#[repr(C)]
pub struct PageTable {
    pub entries: [PageTableEntry; 1024],
}

/// struct for page directory
/// could be laid out better, but works fine for now
#[repr(C)] // im pretty sure this guarantees the order and size of this struct
pub struct PageDirectory {
    /// pointers to page tables
    pub tables: [*mut PageTable; 1024], // FIXME: maybe we want references here? too lazy to deal w borrow checking rn

    /// physical addresses of page tables (raw pointer bc references are Annoying and shit breaks without it)
    pub tables_physical: *mut [u32; 1024],

    /// physical address of this page directory
    pub physical_addr: u32,

    /// bitset to speed up allocation of page frames
    pub frame_set: BitSet,
}

impl PageDirectory {
    /// creates a new page directory, allocating memory for it in the process
    pub fn new() -> Self {
        let num_frames = unsafe { MEM_SIZE >> 12 };
        PageDirectory {
            tables: [core::ptr::null_mut(); 1024],
            tables_physical: unsafe { kmalloc::<[u32; 1024]>(1024 * size_of::<u32>(), false).pointer }, // shit breaks without this lmao
            physical_addr: 0,
            frame_set: BitSet::place_at(unsafe { kmalloc::<u32>(num_frames / 32 * size_of::<u32>(), false).pointer }, num_frames), // BitSet::new uses the global allocator, which isn't initialized yet!
        }
    }

    /// gets a page from the directory if one exists, makes one if requested
    pub fn get_page(&mut self, mut addr: u32, make: bool) -> Option<*mut PageTableEntry> {
        addr >>= 12;
        let table_idx = (addr / 1024) as usize;
        if !self.tables[table_idx].is_null() { // page table already exists
            unsafe { Some(&mut (*self.tables[table_idx]).entries[(addr % 1024) as usize]) }
        } else if make { // page table doesn't exist, create it
            unsafe {
                let ptr = kmalloc(1024 * 4, true); // page table entries are 32 bits (4 bytes) wide
                self.tables[table_idx] = ptr.pointer;
                for i in 0..1024 {
                    (*self.tables[table_idx]).entries[i].0 = 0;
                }
                (*self.tables_physical)[table_idx] = (ptr.phys_addr | 0x7) as u32; // present, read/write, user/supervisor
                Some(&mut (*self.tables[table_idx]).entries[(addr % 1024) as usize])
            }
        } else { // page table doesn't exist
            None
        }
    }
    
    /// allocates a frame for specified page
    pub unsafe fn alloc_frame(&mut self, page: *mut PageTableEntry, is_kernel: bool, is_writeable: bool) { // TODO: consider passing in flags?
        let page2 = &mut *page; // pointer shenanigans to get around the borrow checker lmao
        if page2.is_unused() {
            if let Some(idx) = self.frame_set.first_unset() {
                let mut flags = PageTableFlags::Present;
                if !is_kernel {
                    flags |= PageTableFlags::UserSupervisor;
                }
                if is_writeable {
                    flags |= PageTableFlags::ReadWrite;
                }

                self.frame_set.set(idx);
                page2.set_flags(flags);
                page2.set_address((idx << 12) as u32);
            } else {
                panic!("out of memory (no free frames)");
            }
        }
    }

    /// frees a frame, allowing other things to use it
    pub unsafe fn free_frame(&mut self, page: *mut PageTableEntry) {
        let page2 = &mut *page; // pointer shenanigans
        if !page2.is_unused() {
            self.frame_set.clear((page2.get_address() >> 12) as usize);
            page2.set_unused();
        }
    }

    /// switch global page directory to this page directory
    pub fn switch_to(&self) {
        unsafe {
            let addr = self.tables_physical as u32 - LINKED_BASE as u32;
            asm!("mov cr3, {0}", in(reg) addr);
            let mut cr0: u32;
            asm!("mov {0}, cr0", out(reg) cr0);
            cr0 |= 0x80000000;
            asm!("mov cr0, {0}", in(reg) cr0);
        }
    }
}

impl Default for PageDirectory {
    fn default() -> Self {
        Self::new()
    }
}

/// our page directory
pub static mut PAGE_DIR: Option<PageDirectory> = None;

/// initializes paging
pub unsafe fn init() {
    // calculate placement addr for kmalloc calls
    PLACEMENT_ADDR = (&kernel_end as *const _) as usize - LINKED_BASE; // we need a physical address for this

    // set up page directory struct
    let mut dir = PageDirectory::new();

    // map first 4mb of memory to LINKED_BASE
    for i in 0..1024 {
        let page = dir.get_page(LINKED_BASE as u32 + i * 0x1000, true).unwrap();
        dir.alloc_frame(page, true, true);
    }

    // map initial memory for kernel heap
    for i in (KHEAP_START..KHEAP_START + KHEAP_INITIAL_SIZE).step_by(PAGE_SIZE) {
        let page = dir.get_page(i.try_into().unwrap(), true).unwrap();
        dir.alloc_frame(page, true, true);
    }

    // holy fuck we need maybeuninit so bad
    PAGE_DIR = Some(dir);
    PAGE_DIR.as_mut().unwrap().physical_addr = (&PAGE_DIR as *const _) as u32 - LINKED_BASE as u32;

    // switch to our new page directory
    PAGE_DIR.as_ref().unwrap().switch_to();

    if let Some(dir) = PAGE_DIR.as_ref() {
        let bits_used = dir.frame_set.bits_used;
        log!("{}mb total, {}/{} mapped ({}mb), {}% usage", MEM_SIZE / 1024 / 1024, bits_used, dir.frame_set.size, bits_used / 256, (bits_used * 100) / dir.frame_set.size);
    }
}
