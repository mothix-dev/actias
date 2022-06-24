//! bootloader specific code to be run during arch init

use core::{
    ffi::CStr,
    slice, fmt,
};
use crate::{
    arch::{LINKED_BASE, MEM_SIZE, PAGE_SIZE},
    util::array::BitSet,
};

extern "C" {
    pub static mboot_sig: u32;
    pub static mboot_ptr: *mut MultibootInfo;
}

// TODO: move this into platform along with initial boot code? or maybe have a bootloader interface module
/// multiboot info struct
#[repr(C)]
pub struct MultibootInfo {
    /// flags provided by bootloader, describes which fields are present
    pub flags: u32,


    /// amount of lower memory available in kb
    mem_lower: u32,

    /// amount of upper memory available - 1 mb
    mem_upper: u32,

    /// bios disk device the kernel was loaded from
    /// layout is part3, part2, part1, drive
    boot_device: [u8; 4],

    /// pointer to command line arguments, as a c string (physical address)
    cmdline: u32,

    /// number of modules loaded
    mods_count: u32,

    /// physical address of first module in list
    mods_addr: *mut u8,

    /// either the location of an a.out symbol table or the location of elf section headers
    /// we don't use this
    syms: [u32; 4],

    /// allows us to iterate over memory mappings
    mmap: MemMapList,

    /// amount of drives
    drives_length: u32,
    /// physical address of first drive in list
    drives_addr: u32,

    /// address of rom configuration table?
    config_table: u32,

    /// name of bootloader, as a c string
    bootloader_name: u32,

    /// address of apm table?
    apm_table: u32,

    /// vbe interface information
    vbe: VBEInfo,

    /// framebuffer information
    framebuffer: FramebufferInfo,
}

#[derive(Debug)]
pub struct FlagOutOfBoundsError;

impl MultibootInfo {
    /// check if bit in flags is set
    pub fn is_flag_set(&self, flag: u8) -> Result<bool, FlagOutOfBoundsError> {
        if flag < 32 {
            Ok(self.flags & (1 << flag) != 0)
        } else {
            Err(FlagOutOfBoundsError)
        }
    }

    /// get lower and upper memory amount, if available
    pub fn get_mem(&self) -> Option<(u32, u32)> {
        if self.is_flag_set(0).unwrap() {
            Some((self.mem_lower, self.mem_upper))
        } else {
            None
        }
    }

    /// get bios boot device, if available
    pub fn get_boot_device(&self) -> Option<[u8; 4]> {
        if self.is_flag_set(1).unwrap() {
            Some(self.boot_device)
        } else {
            None
        }
    }

    /// gets command line arguments for kernel if available
    pub fn get_cmdline(&self) -> Option<&str> {
        if self.is_flag_set(2).unwrap() {
            unsafe { CStr::from_ptr((self.cmdline as usize + LINKED_BASE) as *const _).to_str().ok() }
        } else {
            None
        }
    }

    /// gets modules passed by bootloader if available
    pub fn get_modules(&self) -> Option<&[MultibootModule]> {
        if self.is_flag_set(3).unwrap() {
            Some(unsafe { slice::from_raw_parts((self.mods_addr as usize + LINKED_BASE) as *const MultibootModule, self.mods_count as usize) })
        } else {
            None
        }
    }

    /// gets iterator over memory map entries if available
    pub fn get_mmap(&self) -> Option<MemMapIter> {
        if self.is_flag_set(6).unwrap() {
            Some(MemMapIter::new(&self.mmap))
        } else {
            None
        }
    }

    /// gets name of bootloader if available
    pub fn get_bootloader_name(&self) -> Option<&str> {
        if self.is_flag_set(9).unwrap() {
            unsafe { CStr::from_ptr((self.bootloader_name as usize + LINKED_BASE) as *const _).to_str().ok() }
        } else {
            None
        }
    }

    /// gets vbe interface info if available
    pub fn get_vbe(&self) -> Option<&VBEInfo> {
        if self.is_flag_set(11).unwrap() {
            Some(&self.vbe)
        } else {
            None
        }
    }

    /// gets framebuffer info if available
    pub fn get_framebuffer(&self) -> Option<&FramebufferInfo> {
        if self.is_flag_set(12).unwrap() {
            Some(&self.framebuffer)
        } else {
            None
        }
    }
}

impl fmt::Debug for MultibootInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultibootInfo")
         .field("flags", &self.flags)
         .field("mem", &self.get_mem())
         .field("cmdline", &self.get_cmdline())
         .field("modules", &self.get_modules())
         .field("mmap", &self.get_mmap())
         .field("bootloader_name", &self.get_bootloader_name())
         .field("vbe", &self.get_vbe())
         .field("framebuffer", &self.get_framebuffer())
         .finish()
    }
}

/// module provided by bootloader
#[repr(C)]
pub struct MultibootModule {
    /// start of module's contents (physical address)
    start: u32,

    /// end of module's contents (physical address)
    end: u32,
    
    /// string identifier of module
    string: u32,

    reserved: u32,
}

impl MultibootModule {
    /// gets contents of this module as a slice
    pub fn get_contents(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts((self.start as usize + LINKED_BASE) as *const u8, self.end as usize - self.start as usize)
        }
    }

    /// gets string associated with this module
    pub fn get_string(&self) -> &str {
        unsafe { CStr::from_ptr((self.string as usize + LINKED_BASE) as *const _).to_str().unwrap_or("") }
    }
}

impl fmt::Debug for MultibootModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultibootModule")
         .field("start", &(self.start as *const i8))
         .field("end", &(self.end as *const i8))
         .field("string", &self.get_string())
         .finish()
    }
}

/// different types of memory mapping
#[repr(u32)]
#[derive(Debug, PartialEq)]
pub enum MappingKind {
    /// unknown memory map
    Unknown = 0,

    /// available (presumably for system use)
    Available,

    /// reserved, not available
    Reserved,

    /// not sure, maybe either used by ACPI but reclaimable by the OS? maybe the other way around
    AcpiReclaimable,

    /// non volatile storage?
    NVS,

    /// bad memory
    BadRAM,
}

/// struct that describes a region of memory and what it's used for
#[repr(C)]
#[derive(Debug)]
pub struct MemMapEntry {
    /// size of struct, can be greater than the default 20 bytes
    pub size: u32,

    /// base address of memory mapping
    pub base_addr: u64,

    /// how many bytes are mapped
    pub length: u64,
    
    /// what kind of mapping is this
    pub kind: MappingKind,
}

/// wrapper struct for list of memory mappings
#[repr(C)]
pub struct MemMapList {
    /// number of memory mappings
    length: u32,

    /// pointer to start of array of memory mappings, stored as a sort of linked list
    addr: u32,
}

impl fmt::Debug for MemMapList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemMapList")
         .field("length", &self.length)
         .field("addr", &(self.addr as *const u8))
         .finish()
    }
}

/// allows us to iterate over a list of memory mappings. we can't create an array since the entries don't have a fixed size
#[derive(Debug)]
pub struct MemMapIter<'a> {
    /// list we're iterating over
    list: &'a MemMapList,

    /// our index in the list
    index: usize,

    /// address of the last entry we've accessed
    current_addr: *const MemMapEntry,

    /// whether we're on the first entry of the list and should just spit it out without moving to the next one
    first_entry: bool,

    /// whether we've finished iterating over the list, set when self.index >= self.list.length or we run into a zero sized entry
    finished: bool,
}

impl<'a> MemMapIter<'a> {
    /// creates a new iterator over a memory map list
    pub fn new(list: &'a MemMapList) -> Self {
        // maybe make unsafe since we can't guarantee that the memory map entries will be there?
        Self {
            list,
            index: 0,
            current_addr: (list.addr as usize + LINKED_BASE) as *const MemMapEntry, // get virtual address, assuming it's in the first 4 mb
            first_entry: true,
            finished: false,
        }
    }
}

impl<'a> Iterator for MemMapIter<'a> {
    type Item = &'a MemMapEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished { // have we finished iterating?
            None
        } else if self.first_entry && self.list.length > 0 { // return the first element of the list before moving on
            self.first_entry = false;

            let entry = unsafe { &*self.current_addr };

            if entry.size > 0 {
                Some(entry)
            } else {
                self.finished = true;

                None
            }
        } else if !self.first_entry && self.index + 1 < self.list.length as usize {
            let size = unsafe { (*self.current_addr).size } + 4; // add 4 to the size to account for the size value in the struct

            self.current_addr = (self.current_addr as u32 + size) as *const MemMapEntry;

            self.index += 1;

            let entry = unsafe { &*self.current_addr };

            if entry.size > 0 {
                Some(entry)
            } else {
                self.finished = true;

                None
            }
        } else {
            self.finished = true;
            
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.finished {
            (0, Some(0)) // no more
        } else {
            let remaining = self.list.length as usize - self.index - if self.first_entry { 0 } else { 1 }; // how many elements we have remaining

            (0, Some(remaining))
        }
    }
}

/// vbe interface info
#[repr(C)]
#[derive(Debug)]
pub struct VBEInfo {
    pub control_info: u32,
    pub mode_info: u32,
    pub mode: u16,
    pub interface_seg: u16,
    pub interface_off: u16,
    pub interface_len: u16,
}

/// framebuffer info
#[repr(C)]
pub struct FramebufferInfo {
    pub addr: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
    pub kind: FramebufferKind,
    color_info: ColorInfo,
}

impl FramebufferInfo {
    pub fn get_indexed_color_info(&self) -> Option<&IndexedColorInfo> {
        if self.kind == FramebufferKind::Indexed {
            Some(unsafe { &self.color_info.indexed })
        } else {
            None
        }
    }

    pub fn get_rgb_color_info(&self) -> Option<&RGBColorInfo> {
        if self.kind == FramebufferKind::RGB {
            Some(unsafe { &self.color_info.rgb })
        } else {
            None
        }
    }
}

impl fmt::Debug for FramebufferInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug_struct = f.debug_struct("FramebufferInfo");
        debug_struct.field("addr", &(self.addr as *const u8));
        debug_struct.field("pitch", &self.pitch);
        debug_struct.field("width", &self.width);
        debug_struct.field("height", &self.height);
        debug_struct.field("bpp", &self.bpp);
        debug_struct.field("kind", &self.kind);

        if let Some(info) = self.get_indexed_color_info() {
            debug_struct.field("color_info", &info);
        } else if let Some(info) = self.get_rgb_color_info() {
            debug_struct.field("color_info", &info);
        } else {
            let n: Option<()> = None;
            debug_struct.field("color_info", &n);
        }
        
        debug_struct.finish()
    }
}

#[repr(u8)]
#[derive(Debug, PartialEq)]
pub enum FramebufferKind {
    Indexed = 0,
    RGB,
    EGAText,
}

#[repr(C)]
union ColorInfo {
    indexed: IndexedColorInfo,
    rgb: RGBColorInfo,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct IndexedColorInfo {
    pub palette_addr: u32,
    pub num_colors: u16,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct RGBColorInfo {
    pub red_field_pos: u8,
    pub red_mask_size: u8,
    pub green_field_pos: u8,
    pub green_mask_size: u8,
    pub blue_field_pos: u8,
    pub blue_mask_size: u8,
}

/// platform dependent code that's run as early in the boot process as possible
/// won't be needed when boot code is moved into platform
pub unsafe fn pre_init() {
    // check for proper multiboot signature. this is done as early as possible to prevent things from going wrong
    if mboot_sig != 0x2badb002 {
        panic!("invalid multiboot signature!");
    }
}

/// gets reference to multiboot info struct
pub fn get_multiboot_info() -> &'static MultibootInfo {
    unsafe { &*((mboot_ptr as usize + LINKED_BASE) as *const MultibootInfo) }
}

/// saves me from typing a bit
const PAGE_SIZE_U64: u64 = PAGE_SIZE as u64;

/// given a bit set of available pages, set all the bits then clear only the ones that represent memory that is available for us to write to
/// this will prevent memory allocations from using reserved memory regions
pub fn reserve_pages(set: &mut BitSet) {
    let info: &MultibootInfo = get_multiboot_info(); // we can just do this again for now

    // get memory map from bootloader info
    let mut iter = info.get_mmap();

    if let Some(iter) = (&mut iter).as_mut() {
        // set entire bit set, faster to do it this way than to just call set.set()
        for num in set.array.to_slice_mut().iter_mut() {
            *num = 0xffffffff;
        }

        set.bits_used = set.size;

        for region in iter {
            if region.kind == MappingKind::Available {
                debug!("{:?}", region);

                // convert base address of region into page number we can use to index into the bitset
                let start_page =
                    if region.base_addr % PAGE_SIZE_U64 != 0 { // if our base address doesn't align to a page boundary, round up to the nearest page boundary
                        region.base_addr / PAGE_SIZE_U64 + 1
                    } else {
                        region.base_addr / PAGE_SIZE_U64
                    };
                
                debug!("region ends at {:#x}, nearest page {:#x}", region.base_addr + region.length, (region.base_addr + region.length) / PAGE_SIZE_U64);
                
                let end_page = (region.base_addr + region.length) / PAGE_SIZE_U64;
                
                debug!("start page: {:#x}, end page {:#x}", start_page, end_page);
                
                assert!(start_page < end_page);
                assert!(start_page * PAGE_SIZE_U64 >= region.base_addr);
                assert!((end_page * PAGE_SIZE_U64) <= region.base_addr + region.length);

                // free up memory covered by this region, allowing it to be used
                for i in start_page..end_page {
                    if i < set.size as u64 {
                        set.clear(i as usize);
                    }
                }
            }
        }
    } else {
        log!("!!! cannot get memory map from bootloader! things may break !!!");

        // set the 640k-1mb area as reserved
        let start_page = 0xa0000 / PAGE_SIZE_U64;
        let end_page = 0x100000 / PAGE_SIZE_U64;

        for i in start_page..end_page {
            if i < set.size as u64 {
                set.set(i as usize);
            }
        }
    }
}

/// initializes various kernel variables from bootloader info
pub unsafe fn init() {
    debug!("mboot info ptr @ {:#x}", mboot_ptr as usize);

    // get reference to multiboot info struct
    let info: &MultibootInfo = get_multiboot_info();

    debug!("bootloader info: {:?}", info);

    debug!("flags: {:#032b}", info.flags);

    // get amount of available upper memory in kb
    let (_, upper_mem) = info.get_mem().expect("couldn't get memory amount");

    MEM_SIZE = (upper_mem as u64 + 1024) * 1024; // upper memory + 1 mb

    #[cfg(debug_messages)]
    {
        let modules = info.get_modules();

        if let Some(modules) = modules {
            for (index, module) in modules.iter().enumerate() {
                let contents = core::str::from_utf8(module.get_contents()).unwrap_or("Invalid");

                debug!("module {}: {:?}: \"{}\"", index, module, contents);
            }
        }

        let mut mmap_iter = info.get_mmap();

        if let Some(mmap_iter) = (&mut mmap_iter).as_mut() {
            for region in mmap_iter {
                debug!("{:?}", region);
            }
        }
    }
}
