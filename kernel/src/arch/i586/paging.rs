//! x86 non-PAE paging
// warning: this code is terrible. do not do anything like this

use super::{KHEAP_START, LINKED_BASE, MEM_SIZE, PAGE_SIZE};
use crate::{
    mm::KHEAP_INITIAL_SIZE,
    platform::bootloader,
    tasks::{get_current_task_mut, IN_TASK},
    util::array::BitSet,
};
use alloc::alloc::{alloc, Layout};
use bitmask_enum::bitmask;
use core::{arch::asm, default::Default, fmt, mem::size_of};
use x86::tlb::flush;
use log::{error, warn, info, debug, trace};

extern "C" {
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

    /// gets flags of page table entry
    pub fn get_flags(&self) -> u16 {
        (self.0 & 0x00000fff) as u16
    }
}

impl fmt::Debug for PageTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let addr = (self.0 & 0xfffff000) as *const u8;
        let flags = PageTableFlags((self.0 & 0x0fff) as u16);

        f.debug_struct("PageTableEntry")
            .field("address", &addr)
            .field("flags", &flags)
            .finish()
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

    /// gets flags of page directory entry
    pub fn get_flags(&self) -> u16 {
        (self.0 & 0x00000fff) as u16
    }
}

impl fmt::Debug for PageDirEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let addr = (self.0 & 0xfffff000) as *const u8;
        let flags = PageDirFlags((self.0 & 0x0fff) as u16);

        f.debug_struct("PageDirEntry")
            .field("address", &addr)
            .field("flags", &flags)
            .finish()
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

// based on http://www.jamesmolloy.co.uk/tutorial_html/6.-Paging.html

const BUMP_ALLOC_SIZE: usize = 0x100000; // 1mb

static mut PLACEMENT_ADDR_INITIAL: usize = 0; // initial placement addr

static mut PLACEMENT_ADDR: usize = 0; // to be filled in with end of kernel on init

static mut PLACEMENT_AREA: [u8; BUMP_ALLOC_SIZE] = [0; BUMP_ALLOC_SIZE]; // hopefully this will just be located in bss? we can't just allocate memory for it since we need it to allocate memory

/// result of kmalloc calls
pub struct MallocResult<T> {
    pub pointer: *mut T,
    pub phys_addr: usize,
}

/// extremely basic malloc- doesn't support free, only useful for allocating effectively static data
pub unsafe fn kmalloc<T>(size: usize, align: bool) -> MallocResult<T> {
    /*if crate::mm::KERNEL_HEAP.is_some() {
        // can we use the global allocator?
        trace!("kmalloc: using global allocator");

        // get memory layout for type
        let layout = if align {
            Layout::from_size_align(size, PAGE_SIZE).unwrap()
        } else {
            Layout::from_size_align(size, Layout::new::<T>().align()).unwrap() // use recommended alignment for this type
        };

        let pointer = alloc(layout) as *mut T;

        // get physical address of allocated region
        let phys_addr = virt_to_phys(pointer as usize).unwrap();

        MallocResult { pointer, phys_addr }
    } else {
        trace!("kmalloc: using bump allocator");*/

        if align && (PLACEMENT_ADDR & 0xfffff000) > 0 {
            // if alignment is requested and we aren't already aligned
            PLACEMENT_ADDR &= 0xfffff000; // round down to nearest 4k block
            PLACEMENT_ADDR += 0x1000; // increment by 4k- we don't want to overwrite things
        }

        // increment address to make room for area of provided size, return pointer to start of area
        let tmp = PLACEMENT_ADDR;
        PLACEMENT_ADDR += size;

        if PLACEMENT_ADDR >= PLACEMENT_ADDR_INITIAL + BUMP_ALLOC_SIZE {
            // prolly won't happen but might as well
            panic!("out of memory (kmalloc)");
        }

        trace!("kmalloc (bump) allocated virt {:#x}, phys {:#x}", tmp + LINKED_BASE, tmp);

        MallocResult {
            pointer: (tmp + LINKED_BASE) as *mut T,
            phys_addr: tmp,
        }
    //}
}

/// struct for page table
/// 
/// basically just a wrapper for the array lmao
#[repr(C)]
pub struct PageTable {
    pub entries: [PageTableEntry; 1024],
}

/// an error that can be returned from page directory operations
pub enum PageDirError {
    NoAvailableFrames,
    PageNotUnused,
    FrameInUse,
}

impl fmt::Display for PageDirError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::NoAvailableFrames => "no available frames",
                Self::PageNotUnused => "page not unused",
                Self::FrameInUse => "frame already in use, force not applied",
            }
        )
    }
}

impl fmt::Debug for PageDirError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PageDirError: \"{}\"", self)
    }
}

/// struct for page directory
/// 
/// could be laid out better, but works fine for now
pub struct PageDirectory {
    /// pointers to page tables
    pub tables: [*mut PageTable; 1024], // FIXME: maybe we want references here? too lazy to deal w borrow checking rn

    /// physical addresses of page tables (raw pointer bc references are Annoying and shit breaks without it)
    pub tables_physical: *mut [u32; 1024],

    /// physical address of tables_physical lmao
    pub tables_physical_addr: u32,

    /// bitset to speed up allocation of page frames
    pub frame_set: BitSet,

    /// counter of how many times the page directory has been updated
    /// 
    /// can be used to check if partial copies of this page directory elsewhere are out of date
    pub page_updates: usize,
}

impl PageDirectory {
    /// creates a new page directory, allocating memory for it in the process
    pub fn new() -> Self {
        let num_frames = unsafe { MEM_SIZE >> 12 }; // FIXME: may have to use 4gb instead of MEM_SIZE if we need to access memory mapped things higher up in the address space
        let tables_physical = unsafe { kmalloc::<[u32; 1024]>(1024 * size_of::<u32>(), true) };

        debug!("tables_physical alloc @ {:#x}", tables_physical.pointer as usize);

        for phys in (unsafe { *tables_physical.pointer }).iter_mut() {
            *phys = 0;
        }

        PageDirectory {
            tables: [core::ptr::null_mut(); 1024],
            tables_physical: tables_physical.pointer, // shit breaks without this lmao
            tables_physical_addr: tables_physical.phys_addr as u32,
            frame_set: BitSet::place_at(
                unsafe {
                    kmalloc::<u32>(
                        (num_frames / 32 * size_of::<u32>() as u64)
                            .try_into()
                            .unwrap(),
                        false,
                    )
                    .pointer
                },
                num_frames.try_into().unwrap(),
            ), // BitSet::new uses the global allocator, which isn't initialized yet!
            page_updates: 0,
        }
    }

    /// gets a page from the directory if one exists, makes one if requested
    pub fn get_page(&mut self, mut addr: u32, make: bool) -> Option<*mut PageTableEntry> {
        addr >>= 12;
        let table_idx = (addr / 1024) as usize;
        if !self.tables[table_idx].is_null() {
            // page table already exists
            let table_ref = unsafe { &mut (*self.tables[table_idx]) };

            Some(&mut table_ref.entries[(addr % 1024) as usize])
        } else if make {
            // page table doesn't exist, create it
            unsafe {
                let ptr = kmalloc(1024 * 4, true); // page table entries are 32 bits (4 bytes) wide
                self.tables[table_idx] = ptr.pointer;
                let table_ref = &mut (*self.tables[table_idx]);
                for entry in table_ref.entries.iter_mut() {
                    entry.0 = 0;
                }
                (*self.tables_physical)[table_idx] = (ptr.phys_addr | 0x7) as u32; // present, read/write, user/supervisor

                Some(&mut table_ref.entries[(addr % 1024) as usize])
            }
        } else {
            // page table doesn't exist
            None
        }
    }

    /// allocates a frame for specified page
    pub unsafe fn alloc_frame(&mut self, page: *mut PageTableEntry, is_kernel: bool, is_writeable: bool) -> Result<u32, PageDirError> {
        // TODO: consider passing in flags?
        if let Some(idx) = self.frame_set.first_unset() {
            //log!("got phys {:#x}, idx {:#x}", (idx as u32) << 12, idx);
            self.alloc_frame_at((idx as u32) << 12, page, is_kernel, is_writeable, false)
        } else {
            Err(PageDirError::NoAvailableFrames)
        }
    }

    /// allocates a frame for specified page at the specified address
    pub unsafe fn alloc_frame_at(&mut self, address: u32, page: *mut PageTableEntry, is_kernel: bool, is_writeable: bool, force: bool) -> Result<u32, PageDirError> {
        // TODO: consider passing in flags?
        let page2 = &mut *page; // pointer shenanigans to get around the borrow checker lmao
        if page2.is_unused() {
            assert!(
                address % PAGE_SIZE as u32 == 0,
                "frame address is not page aligned"
            );
            let idx = address as usize >> 12;
            trace!("allocating @ phys {:#x}, idx {:#x}", address, idx);

            if force || !self.frame_set.test(idx) {
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
                self.page_updates = self.page_updates.wrapping_add(1); // we want this to be able to overflow

                trace!("allocated frame {:?}", page2);

                Ok((idx << 12) as u32)
            } else {
                Err(PageDirError::FrameInUse)
            }
        } else {
            Err(PageDirError::PageNotUnused)
        }
    }

    /// frees a frame, allowing other things to use it
    pub unsafe fn free_frame(&mut self, page: *mut PageTableEntry) -> Result<u32, PageDirError> {
        let page2 = &mut *page; // pointer shenanigans
        if !page2.is_unused() {
            let addr = page2.get_address();
            //log!("freed phys {:#x}, idx {:#x}", addr, addr >> 12);
            self.frame_set.clear((addr >> 12) as usize);
            page2.set_unused();
            self.page_updates = self.page_updates.wrapping_add(1);

            Ok(addr)
        } else {
            Err(PageDirError::PageNotUnused)
        }
    }

    /// switch global page directory to this page directory
    pub fn switch_to(&self) {
        unsafe {
            //debug!("switching to page table @ phys {:#x}", self.tables_physical_addr);

            asm!(
                "mov cr3, {0}",
                "mov {1}, cr0",
                "or {1}, 0x80000000",
                "mov cr0, {1}",

                in(reg) self.tables_physical_addr,
                out(reg) _,
            );
        }
    }

    /// transform a virtual address to a physical address
    pub fn virt_to_phys(&mut self, addr: u32) -> Option<u32> {
        let page = self.get_page(addr, false)?;

        let page2 = unsafe { &mut *page }; // pointer shenanigans

        if page2.is_unused() {
            None
        } else {
            Some((unsafe { *page }).get_address() | (addr & (PAGE_SIZE as u32 - 1)))
        }
    }
}

impl Default for PageDirectory {
    fn default() -> Self {
        Self::new()
    }
}

/// allocate region of memory
pub unsafe fn alloc_region(dir: &mut PageDirectory, start: usize, size: usize) {
    assert!(size >= PAGE_SIZE, "cannot allocate less than the page size");
    assert!(start % PAGE_SIZE == 0, "start address needs to be page aligned");

    let end = start + size;

    assert!(end % PAGE_SIZE == 0, "start address + size needs to be page aligned");

    for i in (start..end).step_by(PAGE_SIZE) {
        let page = dir.get_page(i.try_into().unwrap(), true).unwrap();
        dir.alloc_frame(page, true, true)
            .expect("couldn't allocate frame"); // FIXME: switch to kernel mode when user tasks don't run in the kernel's address space
    }

    debug!("mapped {:#x} - {:#x}", start, end);
}

/// allocate region of memory at specified address
pub unsafe fn alloc_region_at(
    dir: &mut PageDirectory,
    start: usize,
    size: usize,
    phys_addr: u64,
    force: bool,
) {
    assert!(size >= PAGE_SIZE, "cannot allocate less than the page size");
    assert!(start % PAGE_SIZE == 0, "start address needs to be page aligned");

    let end = start + size;

    assert!(end % PAGE_SIZE == 0, "start address + size needs to be page aligned");

    for i in (start..end).step_by(PAGE_SIZE) {
        let phys = (i - start) as u64 + phys_addr;
        let page = dir.get_page(i.try_into().unwrap(), true).unwrap();
        dir.alloc_frame_at(phys.try_into().unwrap(), page, true, true, force)
            .expect("couldn't allocate frame"); // FIXME: switch to kernel mode when user tasks don't run in the kernel's address space
    }

    debug!("mapped {:#x} - {:#x} @ phys {:#x} - {:#x}{}", start, end, phys_addr, (end - start) as u64 + phys_addr, if force { " (forced)" } else { "" });
}

/// our page directory
pub static mut PAGE_DIR: Option<PageDirectory> = None;

/// how many reserved pages we have
pub static mut NUM_RESERVED_PAGES: usize = 0;

/// initializes paging
pub unsafe fn init() {
    // calculate end of kernel in memory
    let kernel_end_pos = (&kernel_end as *const _) as usize;

    // calculate placement addr for initial kmalloc calls
    PLACEMENT_ADDR_INITIAL = (&PLACEMENT_AREA as *const _) as usize - LINKED_BASE;
    PLACEMENT_ADDR = PLACEMENT_ADDR_INITIAL;

    debug!("kernel end @ {:#x}, linked @ {:#x}", kernel_end_pos, LINKED_BASE);
    debug!("placement @ {:#x} - {:#x} (phys @ {:#x})", PLACEMENT_ADDR, PLACEMENT_ADDR + BUMP_ALLOC_SIZE, PLACEMENT_ADDR + LINKED_BASE);

    // set up page directory struct
    let mut dir = PageDirectory::new();

    // get reserved areas of memory from bootloader
    bootloader::reserve_pages(&mut dir.frame_set);

    NUM_RESERVED_PAGES = dir.frame_set.bits_used;

    // TODO: map initial kernel memory allocations as global so they won't be invalidated from TLB flushes

    info!("mapping kernel memory");

    // map kernel up to LINKED_BASE + 1mb
    let kernel_start_pos = LINKED_BASE + 0x100000;
    let kernel_size = kernel_end_pos - kernel_start_pos;
    alloc_region_at(&mut dir, kernel_start_pos, kernel_size, 0x100000, false);

    info!("mapping heap memory");

    // map initial memory for kernel heap
    alloc_region(&mut dir, KHEAP_START, KHEAP_INITIAL_SIZE);

    info!("creating page table");

    // holy fuck we need maybeuninit so bad
    PAGE_DIR = Some(dir);

    info!("switching to page table");

    // switch to our new page directory
    PAGE_DIR.as_ref().unwrap().switch_to();

    print_free();
}

pub fn print_free() {
    if let Some(dir) = unsafe { PAGE_DIR.as_ref() } {
        let reserved = unsafe { NUM_RESERVED_PAGES };
        let mem_size = unsafe { MEM_SIZE };

        let bits_used = dir.frame_set.bits_used - reserved;
        let size = dir.frame_set.size - reserved;
        info!(
            "{}mb total, {}/{} mapped ({}mb, {} pages reserved), {}% usage",
            mem_size / 1024 / 1024,
            bits_used,
            size,
            bits_used / 256,
            reserved,
            (bits_used * 100) / size
        );
    } else {
        error!("no page directory :(");
    }
}

// sync with current task's page table if we haven't fully switched to kernel mode
fn sync_with_task(addr: usize, num: usize) {
    let dir = unsafe { PAGE_DIR.as_mut().unwrap() };

    if unsafe { IN_TASK } {
        let current = get_current_task_mut().unwrap();

        // copy from the kernel's page directory to the task's
        current
            .state
            .copy_pages_from(dir, addr >> 22, ((addr + num * PAGE_SIZE) >> 22) + 1);

        // the task's page directory is now up to date (at least for our purposes)
        current.state.page_updates = dir.page_updates;
    }
}

/// allocate pages and map to given address
///
/// num is in terms of pages, so num * PAGE_SIZE gives the amount of bytes mapped
pub fn alloc_pages(addr: usize, num: usize, is_kernel: bool, is_writeable: bool) {
    assert!(addr % PAGE_SIZE == 0, "address is not page aligned");

    trace!("allocating {} page(s) @ {:#x}", num, addr);

    let dir = unsafe { PAGE_DIR.as_mut().unwrap() };

    for i in (addr..addr + num * PAGE_SIZE).step_by(PAGE_SIZE) {
        let page = dir.get_page(i.try_into().unwrap(), true).unwrap();

        unsafe {
            match dir.alloc_frame(page, is_kernel, is_writeable) {
                Ok(_) => flush(addr), // invalidate this page in the TLB
                Err(msg) => panic!("couldn't allocate page: {}", msg),
            }
        }
    }

    sync_with_task(addr, num);
}

/// allocate pages at given physical address and map to given virtual address
///
/// num is in terms of pages, so num * PAGE_SIZE gives the amount of bytes mapped
pub fn alloc_pages_at(virt_addr: usize, num: usize, phys_addr: u64, is_kernel: bool, is_writeable: bool, force: bool) {
    assert!(virt_addr % PAGE_SIZE == 0, "virtual address is not page aligned");
    assert!(phys_addr % PAGE_SIZE as u64 == 0, "physical address is not page aligned");

    trace!("allocating {} page(s) @ {:#x}, phys {:#x}", num, virt_addr, phys_addr);

    let dir = unsafe { PAGE_DIR.as_mut().unwrap() };

    for i in (virt_addr..virt_addr + num * PAGE_SIZE).step_by(PAGE_SIZE) {
        let phys = (i - virt_addr) as u64 + phys_addr;
        let page = dir.get_page(i.try_into().unwrap(), true).unwrap();

        if force {
            unsafe {
                (&mut *page).set_unused();
            }
        }

        unsafe {
            match dir.alloc_frame_at(phys.try_into().unwrap(), page, is_kernel, is_writeable, force) {
                Ok(_) => flush(virt_addr), // invalidate this page in the TLB
                Err(msg) => panic!("couldn't allocate page: {}", msg),
            }
        }
    }

    sync_with_task(virt_addr, num);
}

/// free pages at given address
///
/// num is in terms of pages, so num * PAGE_SIZE gives the amount of bytes mapped
pub fn free_pages(addr: usize, num: usize) {
    assert!(addr % PAGE_SIZE == 0, "address is not page aligned");

    let dir = unsafe { PAGE_DIR.as_mut().unwrap() };

    for i in (addr..addr + num * PAGE_SIZE).step_by(PAGE_SIZE) {
        if let Some(page) = dir.get_page(i.try_into().unwrap(), false) {
            unsafe {
                match dir.free_frame(page) {
                    Ok(_) => asm!("invlpg [{0}]", in(reg) addr), // invalidate this page in the TLB
                    Err(msg) => panic!("couldn't free page: {}", msg),
                }
            }
        }
    }

    sync_with_task(addr, num);
}

/// given the physical address of a page, set it as unused
pub fn free_page_phys(phys: u64) {
    assert!(phys % PAGE_SIZE as u64 == 0, "address is not page aligned");

    let dir = unsafe { PAGE_DIR.as_mut().unwrap() };

    dir.frame_set.clear((phys >> 12).try_into().unwrap());
}

/// convert virtual to physical address
pub fn virt_to_phys(addr: usize) -> Option<usize> {
    let dir = unsafe { PAGE_DIR.as_mut()? };

    let addr = if let Ok(res) = addr.try_into() {
        res
    } else {
        return None;
    };

    match dir.virt_to_phys(addr) {
        Some(res) => match res.try_into() {
            Ok(ult) => Some(ult),
            Err(..) => None,
        },
        None => None,
    }
}

/// bump allocate some memory
pub unsafe fn bump_alloc<T>(size: usize, alignment: usize) -> *mut T {
    trace!("bump alloc");
    let offset: usize = if PLACEMENT_ADDR % alignment != 0 {
        alignment - (PLACEMENT_ADDR % alignment)
    } else {
        0
    };

    PLACEMENT_ADDR += offset;

    let tmp = PLACEMENT_ADDR;
    PLACEMENT_ADDR += size;

    if PLACEMENT_ADDR >= PLACEMENT_ADDR_INITIAL + BUMP_ALLOC_SIZE {
        // prolly won't happen but might as well
        panic!("out of memory (kmalloc)");
    }

    (tmp + LINKED_BASE) as *mut T
}
