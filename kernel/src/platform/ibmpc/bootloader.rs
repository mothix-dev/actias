//! bootloader specific code to be run during arch init

use crate::{
    arch::{
        paging::{alloc_pages_at, free_pages, kmalloc},
        LINKED_BASE, MEM_SIZE, PAGE_SIZE,
    },
    util::array::BitSet,
};
use alloc::alloc::{alloc, Layout};
use core::{ffi::CStr, fmt, mem::size_of, slice};
use log::{error, warn, info, debug, trace};

extern "C" {
    /// multiboot signature, provided by the bootloader and set in boot.S
    pub static mboot_sig: u32;

    /// pointer to multiboot info, provided by the bootloader and set in boot.S
    pub static mboot_ptr: *mut MultibootInfo;
}

pub static mut MULTIBOOT_INFO: Option<MultibootInfoCopy> = None;

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

/// contains fields copied from the bootloader provided multiboot info table
/// allows for easier access, and allows for access once the original multiboot info has been overwritten since it's not worth it to reserve the memory it uses
/// certain fields like mmap are not copied, as we won't need them and it's too impractical to do so
#[derive(Debug)]
pub struct MultibootInfoCopy {
    /// flags provided by bootloader
    pub flags: u32,

    /// how much lower and upper memory we have, respectively
    pub mem: Option<(u32, u32)>,

    /// bios disk device the kernel was loaded from
    /// layout is part3, part2, part1, drive
    pub boot_device: Option<[u8; 4]>,

    /// kernel command line arguments
    pub cmdline: Option<&'static str>,

    /// modules provided by bootloader
    pub mods: Option<&'static mut [MultibootModuleCopy]>,

    /// name of bootloader
    pub bootloader_name: Option<&'static str>,

    /// vbe interface info
    pub vbe: Option<VBEInfo>,

    /// framebuffer info
    pub framebuffer: Option<FramebufferInfo>,
}

/// module provided by bootloader
#[repr(C)]
pub struct MultibootModuleCopy {
    /// data of module
    data: Option<&'static [u8]>,

    /// physical starting address of module data
    data_start: u32,

    /// physical ending address of module data
    data_end: u32,

    /// string identifier of module
    string: &'static str,
}

impl MultibootModuleCopy {
    pub fn data(&self) -> &'static [u8] {
        self.data.unwrap()
    }

    pub fn string(&self) -> &str {
        self.string
    }

    pub fn map_data(&mut self) {
        if self.data.is_none() {
            let buf_size = (self.data_end - self.data_start) as usize;

            let num_pages = if buf_size % PAGE_SIZE != 0 {
                (buf_size + PAGE_SIZE) / PAGE_SIZE
            } else {
                buf_size / PAGE_SIZE
            };

            let buf_size_aligned = num_pages * PAGE_SIZE;

            debug!("buf size {:#x}, aligned to {:#x}", buf_size, buf_size_aligned);

            let layout = Layout::from_size_align(buf_size_aligned, PAGE_SIZE).unwrap();
            let ptr = unsafe { alloc(layout) };

            assert!(ptr as usize % PAGE_SIZE == 0); // make absolutely sure pointer is page aligned

            // free memory we're going to remap
            free_pages(ptr as usize, num_pages);

            // remap memory
            alloc_pages_at(
                ptr as usize,
                num_pages,
                self.data_start as u64,
                true,
                true,
                true,
            );

            self.data = Some(unsafe { slice::from_raw_parts(ptr, buf_size) });
        }
    }
}

impl fmt::Debug for MultibootModuleCopy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultibootModuleCopy")
            .field("data", &self.data())
            .field("string", &self.string())
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct FlagOutOfBoundsError;

impl MultibootInfo {
    pub fn copy(&self) -> MultibootInfoCopy {
        let copy_str = |s: &str| {
            trace!("copy str {:?}", s);

            let chars = s.as_bytes();
            let len = chars.len();
            let new = unsafe { slice::from_raw_parts_mut(kmalloc::<u8>(len, false).pointer, len) };

            new.copy_from_slice(chars);

            core::str::from_utf8(new).unwrap()
        };

        let modules_copy = if let Some(modules) = self.get_modules() {
            let len = modules.len();

            if len > 0 {
                let new = unsafe {
                    slice::from_raw_parts_mut(kmalloc::<MultibootModuleCopy>(len * size_of::<MultibootModuleCopy>(), false).pointer, len)
                };

                for i in 0..len {
                    let old_module = &modules[i];
                    let new_module = &mut new[i];

                    new_module.data = None;
                    new_module.data_start = old_module.start;
                    new_module.data_end = old_module.end;
                    new_module.string = copy_str(unsafe {
                        CStr::from_ptr((old_module.string as usize + LINKED_BASE) as *const _)
                            .to_str()
                            .unwrap_or("")
                    });
                }

                Some(new)
            } else {
                None
            }
        } else {
            None
        };

        MultibootInfoCopy {
            flags: self.flags,
            mem: self.get_mem(),
            boot_device: self.get_boot_device(),
            cmdline: self.get_cmdline().map(copy_str),
            mods: modules_copy,
            bootloader_name: self.get_bootloader_name().map(copy_str),
            vbe: self.get_vbe().copied(),
            framebuffer: self.get_framebuffer().copied(),
        }
    }

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
            unsafe {
                CStr::from_ptr((self.cmdline as usize + LINKED_BASE) as *const _)
                    .to_str()
                    .ok()
            }
        } else {
            None
        }
    }

    /// gets modules passed by bootloader if available
    pub fn get_modules(&self) -> Option<&[MultibootModule]> {
        if self.is_flag_set(3).unwrap() {
            Some(unsafe {
                slice::from_raw_parts(
                    (self.mods_addr as usize + LINKED_BASE) as *const MultibootModule,
                    self.mods_count as usize,
                )
            })
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
            unsafe {
                CStr::from_ptr((self.bootloader_name as usize + LINKED_BASE) as *const _)
                    .to_str()
                    .ok()
            }
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
            .field("boot_device", &self.get_boot_device())
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
            slice::from_raw_parts(
                self.start as usize as *const u8,
                self.end as usize - self.start as usize,
            )
        }
    }

    /// gets string associated with this module
    pub fn get_string(&self) -> &str {
        unsafe {
            CStr::from_ptr(self.string as usize as *const _)
                .to_str()
                .unwrap_or("")
        }
    }
}

impl fmt::Debug for MultibootModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultibootModule")
            .field("start", &(self.start as *const i8))
            .field("end", &(self.end as *const i8))
            //.field("string", &self.get_string())
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
        if self.finished {
            // have we finished iterating?
            None
        } else if self.first_entry && self.list.length > 0 {
            // return the first element of the list before moving on
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
            let remaining =
                self.list.length as usize - self.index - if self.first_entry { 0 } else { 1 }; // how many elements we have remaining

            (0, Some(remaining))
        }
    }
}

/// vbe interface info
#[repr(C)]
#[derive(Debug, Copy, Clone)]
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
#[derive(Copy, Clone)]
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
#[derive(Debug, PartialEq, Copy, Clone)]
pub enum FramebufferKind {
    Indexed = 0,
    RGB,
    EGAText,
}

#[repr(C)]
#[derive(Copy, Clone)]
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
fn get_orig_multiboot_info() -> &'static MultibootInfo {
    unsafe { &*((mboot_ptr as usize + LINKED_BASE) as *const MultibootInfo) }
}

/// gets copy of multiboot info
pub fn get_multiboot_info() -> &'static MultibootInfoCopy {
    unsafe { MULTIBOOT_INFO.as_ref().unwrap() }
}

/// gets mutable copy of multiboot info. only used here since having other parts of the kernel able to modify it could be bad?
fn get_multiboot_info_mut() -> &'static mut MultibootInfoCopy {
    unsafe { MULTIBOOT_INFO.as_mut().unwrap() }
}

/// saves me from typing a bit
const PAGE_SIZE_U64: u64 = PAGE_SIZE as u64;

/// given a bit set of available pages, set all the bits then clear only the ones that represent memory that is available for us to write to
/// this will prevent memory allocations from using reserved memory regions
pub fn reserve_pages(set: &mut BitSet) {
    let info: &MultibootInfo = get_orig_multiboot_info(); // we can just do this again for now

    // set or clear a region of memory in the bitset
    fn set_region(set: &mut BitSet, start: u64, end: u64, state: bool) {
        // convert base address of region into page number we can use to index into the bitset
        let start_page = if start % PAGE_SIZE_U64 != 0 {
            // if our base address doesn't align to a page boundary, round up to the nearest page boundary
            start / PAGE_SIZE_U64 + 1
        } else {
            start / PAGE_SIZE_U64
        };

        let end_page = (end) / PAGE_SIZE_U64;

        debug!("start page: {:#x}, end page {:#x}, state {}", start_page, end_page, state);

        assert!(start_page < end_page);
        assert!(start_page * PAGE_SIZE_U64 >= start);
        assert!((end_page * PAGE_SIZE_U64) <= end);

        // free up memory covered by this region, allowing it to be used
        for i in start_page..end_page {
            if i < set.size as u64 {
                if state {
                    set.set(i as usize);
                } else {
                    set.clear(i as usize);
                }
            }
        }
    }

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

                set_region(
                    set,
                    region.base_addr,
                    region.base_addr + region.length,
                    false,
                );
            }
        }
    } else {
        error!("!!! cannot get memory map from bootloader! things may break !!!");

        // set the 640k-1mb area as reserved
        set_region(set, 0xa0000, 0x100000, true);
    }

    // mark modules provided by bootloader as reserved, so we don't trample on them later
    let modules = info.get_modules();

    if let Some(modules) = modules {
        for module in modules.iter() {
            debug!("{:?}", module);

            set_region(set, module.start as u64, module.end as u64, true);
        }
    }

    // copy multiboot info since bump allocator is initialized here and we still have access to the old struct
    info!("copying multiboot info");

    unsafe {
        MULTIBOOT_INFO = Some(info.copy());
    }
}

/// initializes various kernel variables from bootloader info
pub unsafe fn init() {
    debug!("mboot info ptr @ {:#x}", mboot_ptr as usize);

    // get reference to multiboot info struct
    let info: &MultibootInfo = get_orig_multiboot_info();

    debug!("bootloader info: {:?}", info);

    debug!("flags: {:#032b}", info.flags);

    // get amount of available upper memory in kb
    let (_, upper_mem) = info.get_mem().expect("couldn't get memory amount");

    MEM_SIZE = (upper_mem as u64 + 1024) * 1024; // upper memory + 1 mb

    /*#[cfg(debug_messages)]
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
    }*/
}

/// initialization to be run after the heap is initialized
pub fn init_after_heap() {
    if let Some(mods) = get_multiboot_info_mut().mods.as_mut() {
        for module in mods.iter_mut() {
            module.map_data();
        }
    }
}
