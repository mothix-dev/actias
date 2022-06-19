pub mod ints;
pub mod gdt;
pub mod paging;
pub mod syscalls;
pub mod tasks;

use core::{
    arch::asm,
    ffi::CStr,
    slice,
    fmt,
};

// various useful constants
pub const MEM_TOP: usize = 0xffffffff;
pub const LINKED_BASE: usize = 0xc0000000;
pub const KHEAP_START: usize = LINKED_BASE + 0x10000000;

pub const PAGE_SIZE: usize = 0x1000;
pub const INV_PAGE_SIZE: usize = !(PAGE_SIZE - 1);

pub const MAX_STACK_FRAMES: usize = 1024;

pub static mut MEM_SIZE: usize = 128 * 1024 * 1024; // TODO: get actual RAM size from BIOS

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
    cmdline: *const i8,

    /// number of modules loaded
    mods_count: u32,

    /// physical address of first module in list
    mods_addr: *mut u8,

    /// either the location of an a.out symbol table or the location of elf section headers
    /// we don't use this
    syms: [u32; 4],

    /// allows us to iterate over memory mappings
    mmap: MemMapList,

    drives_length: u32,
    drives_addr: *mut u8,

    config_table: u32,

    boot_loader_name: u32,

    apm_table: u32,

    vbe_control_info: u32,
    vbe_mode_info: u32,
    vbe_mode: u32,
    vbe_interface_seg: u32,
    vbe_interface_off: u32,
    vbe_interface_len: u32,

    framebuffer_addr: u32,
    framebuffer_pitch: u32,
    framebuffer_width: u32,
    framebuffer_height: u32,
    framebuffer_bpp: u8,
    framebuffer_type: u8,
    color_info: [u8; 6],
}

impl MultibootInfo {
    /// check if bit in flags is set
    pub fn is_flag_set(&self, flag: u8) -> Result<bool, ()> {
        if flag < 32 {
            Ok(self.flags & (1 << flag) != 0)
        } else {
            Err(())
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
}

/// module provided by bootloader
#[repr(C)]
pub struct MultibootModule {
    /// start of module's contents (physical address)
    start: *const u8,

    /// end of module's contents (physical address)
    end: *const u8,
    
    /// string identifier of module
    string: *const i8,

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
         .field("start", &self.start)
         .field("end", &self.end)
         .field("string", &self.get_string())
         .finish()
    }
}

/// different types of memory mapping
#[repr(u32)]
#[derive(Debug)]
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
#[derive(Debug)]
pub struct MemMapList {
    /// number of memory mappings
    length: u32,

    /// pointer to start of array of memory mappings, stored as a sort of linked list
    addr: *mut u8,
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
            let size = unsafe { (&*self.current_addr).size } + 4; // add 4 to the size to account for the size value in the struct

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

/// initialize paging, just cleanly map our kernel to 3gb
#[no_mangle]
pub extern fn x86_prep_page_table(buf: &mut [u32; 1024]) {
    for i in 0u32 .. 1024 {
        buf[i as usize] = i * 0x1000 + 3;
    }
}

/// halt system
pub fn halt() -> ! {
    log!("halting");

    unsafe {
        loop {
            asm!("cli; hlt"); // clear interrupts, halt
        }
    }
}

/// initialize sub-modules
pub fn init() {
    // check for proper multiboot signature. this is done as early as possible to prevent things from going wrong
    unsafe {
        if mboot_sig != 0x2badb002 {
            log!("invalid multiboot signature!");
            loop {
                asm!("cli; hlt"); // clear interrupts, halt
            }
        }
    }

    debug!("initializing GDT");
    unsafe { gdt::init(); }
    debug!("initializing interrupts");
    unsafe { ints::init(); }

    // get reference to multiboot info struct
    let info: &MultibootInfo = unsafe { &*((mboot_ptr as usize + LINKED_BASE) as *const MultibootInfo) };

    // get amount of available memory
    let (_, upper_mem) = info.get_mem().expect("couldn't get memory amount");

    unsafe {
        MEM_SIZE = (upper_mem as usize + 1024) * 1024;
    };

    let cmdline = info.get_cmdline();

    debug!("cmdline: {:?}", cmdline);

    let modules = info.get_modules();

    debug!("modules: {:?}", modules);

    if let Some(modules) = modules {
        for (index, module) in modules.iter().enumerate() {
            let contents = match core::str::from_utf8(module.get_contents()) {
                Ok(string) => string,
                Err(_) => "Invalid",
            };

            debug!("module {}: {:?}: \"{}\"", index, module, contents);
        }
    }

    let mut mmap_iter = info.get_mmap();

    debug!("mmap: {:?}", mmap_iter);

    if let Some(mmap_iter) = (&mut mmap_iter).as_mut() {
        for region in mmap_iter {
            debug!("{:?}", region);
        }
    }

    debug!("initializing paging");
    unsafe { paging::init(); }
}
