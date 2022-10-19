use bitmask_enum::bitmask;
use num_enum::TryFromPrimitive;

#[derive(Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
pub enum Syscalls {
    IsComputerOn,
    ExitProcess,
    ExitThread,
    Fork,
    Mmap,
    Unmap,
    GetProcessID,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct MmapArguments {
    /// the ID of this mapping, if it's shared memory
    pub id: u32,

    /// a hint of where the mapping should start, unless the Fixed or FixedNoReplace flags are set
    pub address: u64,

    /// the length of the mapping. this will be rounded up to the next page boundary
    pub length: u64,

    /// flags describing how pages in this mapping can be accessed
    pub protection: MmapProtection,

    /// flags describing how this mapping should work
    pub flags: MmapFlags,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct UnmapArguments {
    /// the starting address to unmap from
    pub address: u64,

    /// the length of the range to unmap
    pub length: u64,
}

#[bitmask(u8)]
pub enum MmapProtection {
    /// allows for this mapping to be read from. this is set by default because it's silly and completely impractical to disable read permissions :)
    Read = 0b00,

    /// allows for this mapping to be written to
    Write = 0b01,

    /// allows for code to be executed from this mapping. not implemented on all platforms
    Execute = 0b10,
}

#[bitmask(u8)]
pub enum MmapFlags {
    None = 0b0000,

    /// all pages in this mapping will be mapped as copy-on-write, meaning pages in this region will be copied when writes are attempted
    ///
    /// equivalent to MAP_PRIVATE on systems with a more traditional mmap()
    Private = 0b0001,

    /// setting this flag will cause the kernel to place the mapping at the exact address provided, instead of using it as a hint
    ///
    /// equivalent to MAP_FIXED
    Fixed = 0b0010,

    /// this flag acts the same as Fixed, with the difference that if there are already pages mapped at the given address the call to mmap() will fail
    ///
    /// equivalent to MAP_FIXED_NOREPLACE
    FixedNoReplace = 0b0100,

    /// the mapping isn't shared memory of any kind, so its contents will be initialized to zero and the `id` field of the arguments will be ignored.
    /// it's effectively just mapping in new pages
    ///
    /// equivalent to MAP_ANONYMOUS
    Anonymous = 0b1000,
}
