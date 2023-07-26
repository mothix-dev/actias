use crate::mm::{MemoryKind, MemoryRegion};

extern "C" {
    /// multiboot signature, provided by the bootloader and set in boot.S
    pub static mboot_sig: u32;

    /// pointer to multiboot info, provided by the bootloader and set in boot.S
    pub static mboot_ptr: *mut MultibootInfo;
}

/// multiboot info struct
#[repr(C)]
pub struct MultibootInfo {
    /// flags provided by bootloader, describes which fields are present
    pub flags: u32,

    /// amount of lower memory available in kb
    pub mem_lower: u32,

    /// amount of upper memory available - 1 mb
    pub mem_upper: u32,

    /// bios disk device the kernel was loaded from
    /// layout is part3, part2, part1, drive
    pub boot_device: [u8; 4],

    /// pointer to command line arguments, as a c string (physical address)
    pub cmdline: u32,

    /// number of modules loaded
    pub mods_count: u32,

    /// physical address of first module in list
    pub mods_addr: u32,

    /// either the location of an a.out symbol table or the location of elf section headers
    /// we don't use this
    pub syms: [u32; 4],

    /// how many memory mappings exist
    pub mmap_length: u32,

    /// address of the memory mapping list
    pub mmap_addr: u32,

    /// amount of drives
    pub drives_length: u32,

    /// physical address of first drive in list
    pub drives_addr: u32,

    /// address of rom configuration table?
    pub config_table: u32,

    /// name of bootloader, as a c string
    pub bootloader_name: u32,

    /// address of apm table?
    pub apm_table: u32,

    /// vbe interface information
    pub vbe: VBEInfo,

    /// framebuffer information
    pub framebuffer: FramebufferInfo,
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

impl core::fmt::Debug for FramebufferInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
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

/// different types of memory mapping
#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum MappingKind {
    /// unknown memory map
    Unknown = 0,

    /// available (presumably for system use)
    Available,

    /// reserved, not available
    Reserved,

    /// used to initialize ACPI but is reclaimable afterwards
    AcpiReclaimable,

    /// non volatile memory for ACPI settings
    AcpiNVS,

    /// bad memory
    BadRAM,
}

impl From<MappingKind> for MemoryKind {
    fn from(kind: MappingKind) -> Self {
        match kind {
            MappingKind::Unknown | MappingKind::BadRAM => MemoryKind::Bad,
            MappingKind::Reserved | MappingKind::AcpiNVS => MemoryKind::Reserved,
            MappingKind::AcpiReclaimable => MemoryKind::ReservedReclaimable,
            MappingKind::Available => MemoryKind::Available,
        }
    }
}

/// struct that describes a region of memory and what it's used for
#[repr(C)]
#[derive(Debug, Copy, Clone)]
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

impl From<&MemMapEntry> for MemoryRegion {
    fn from(entry: &MemMapEntry) -> Self {
        Self {
            base: entry.base_addr.try_into().unwrap(),
            length: entry.length.try_into().unwrap(),
            kind: entry.kind.into(),
        }
    }
}
