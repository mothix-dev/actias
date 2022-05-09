// x86 non-PAE paging

use bitmask_enum::bitmask;

const LINKED_BASE: usize = 0xc0000000;

extern "C" {
    /// page directory, created in boot.S
    static page_directory: *mut [u32; 1024];
}

/// entry in a page table
#[repr(transparent)]
pub struct PageTableEntry(u32);

impl PageTableEntry {
    /// create new page table entry
    pub fn new(addr: u32, flags: PageTableFlags) -> Self {
        Self((addr & 0xfffff000) | (flags.0 & 0x0fff) as u32)
    }

    /// set address of page table entry
    pub fn set_address(&mut self, addr: u32) {
        self.0 = (self.0 & 0x00000fff) | (addr & 0xfffff000);
    }

    /// set flags of page table entry
    pub fn set_flags(&mut self, flags: PageTableFlags) {
        self.0 = (self.0 & 0xfffff000) | (flags.0 & 0x0fff) as u32;
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

/// entry in a page directory
#[repr(transparent)]
pub struct PageDirEntry(u32);

impl PageDirEntry {
    /// create new page directory entry
    pub fn new(addr: u32, flags: PageTableFlags) -> Self {
        Self((addr & 0xfffff000) | (flags.0 & 0x0fff) as u32)
    }

    /// set address of page directory entry
    pub fn set_address(&mut self, addr: u32) {
        self.0 = (self.0 & 0x00000fff) | (addr & 0xfffff000);
    }

    /// set flags of page directory entry
    pub fn set_flags(&mut self, flags: PageTableFlags) {
        self.0 = (self.0 & 0xfffff000) | (flags.0 & 0x0fff) as u32;
    }
}

/// page directory entry flags
#[bitmask(u16)]
pub enum PageDirFlags {
    // TODO
}

pub unsafe fn init() {
    
}
