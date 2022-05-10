//! x86 non-PAE paging

use core::fmt;
use bitmask_enum::bitmask;

const LINKED_BASE: usize = 0xc0000000;

extern "C" {
    /// page directory, created in boot.S
    static page_directory: [PageDirEntry; 1024];
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

    /// checks if this page table entry is unused
    pub fn is_unused(&self) -> bool {
        self.0 == 0 // lol. lmao
    }
}

impl fmt::Display for PageTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PageTableEntry {{\n")?;
        write!(f, "    address: {:#x},\n", self.0 & 0xfffff000)?;
        write!(f, "    flags: {}\n", PageDirFlags((self.0 & 0x0fff) as u16))?;
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

    /// checks if this page dir entry is unused
    pub fn is_unused(&self) -> bool {
        self.0 == 0 // lol. lmao
    }
}

impl fmt::Display for PageDirEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PageDirEntry {{\n")?;
        write!(f, "    address: {:#x},\n", self.0 & 0xfffff000)?;
        write!(f, "    flags: {}\n", PageDirFlags((self.0 & 0x0fff) as u16))?;
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

/// initializes paging
pub unsafe fn init() {
    for (i, entry) in page_directory.iter().enumerate() {
        if !entry.is_unused() {
            log!("{}: {}", i, entry);
        }
    }
}
