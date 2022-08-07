use super::{GroupID, UserID};
use bitmask_enum::bitmask;
use core::fmt;
use num_enum::FromPrimitive;

/// numerical file descriptor
pub type FileDescriptor = usize;

/// describes how a file will be opened
#[bitmask(u8)]
#[derive(PartialOrd, FromPrimitive)]
pub enum OpenFlags {
    #[num_enum(default)]
    None = Self(0),
    Read = Self(1 << 0),
    Write = Self(1 << 1),
    Append = Self(1 << 2),
    Create = Self(1 << 3),
    Truncate = Self(1 << 4),
    NonBlocking = Self(1 << 5),
}

/// controls how FileDescriptor::seek() seeks
#[repr(u8)]
#[derive(FromPrimitive)]
pub enum SeekKind {
    /// set file writing offset to provided offset
    #[num_enum(default)]
    Set = 0,

    /// add the provided offset to the current file offset
    Current,

    /// set the file offset to the end of the file plus the provided offset
    End,
}

/// posix stat
#[repr(C)]
#[derive(Default, Debug)]
pub struct FileStatus {
    pub device: u32,
    pub serial: u32,
    pub kind: FileKind,
    pub num_links: u32,
    pub user_id: UserID,
    pub group_id: GroupID,
    pub size: u64,
    pub access_time: TimeSpec,
    pub mod_time: TimeSpec,
    pub stat_time: TimeSpec,
    pub block_size: u32,
    pub num_blocks: u32,
}

/// what kind of file this is
#[repr(u8)]
#[derive(Default, Debug)]
pub enum FileKind {
    #[default]
    Regular = 0,
    SymLink,
    CharSpecial,
    BlockSpecial,
    Directory,
    FIFO,
    Socket,
}

/// posix timespec
#[repr(C)]
#[derive(Default, Debug)]
pub struct TimeSpec {
    pub seconds: u64,
    pub nanoseconds: u32,
}

/// standard unix permissions bit field
#[bitmask(u16)]
#[derive(FromPrimitive)]
pub enum Permissions {
    SetUID = Self(1 << 11),
    SetGID = Self(1 << 10),
    Sticky = Self(1 << 9),
    OwnerRead = Self(1 << 8),
    OwnerWrite = Self(1 << 7),
    OwnerExecute = Self(1 << 6),
    GroupRead = Self(1 << 5),
    GroupWrite = Self(1 << 4),
    GroupExecute = Self(1 << 3),
    OtherRead = Self(1 << 2),
    OtherWrite = Self(1 << 1),
    OtherExecute = Self(1 << 0),
    None = Self(0),
}

impl fmt::Display for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", if *self & Self::OwnerRead != 0 { "r" } else { "-" })?;
        write!(f, "{}", if *self & Self::OwnerWrite != 0 { "w" } else { "-" })?;
        write!(
            f,
            "{}",
            if *self & Self::OwnerExecute != 0 && *self & Self::SetUID != 0 {
                "s"
            } else if *self & Self::SetUID != 0 {
                "S"
            } else if *self & Self::OwnerExecute != 0 {
                "x"
            } else {
                "-"
            }
        )?;
        write!(f, "{}", if *self & Self::GroupRead != 0 { "r" } else { "-" })?;
        write!(f, "{}", if *self & Self::GroupWrite != 0 { "w" } else { "-" })?;
        write!(
            f,
            "{}",
            if *self & Self::GroupExecute != 0 && *self & Self::SetGID != 0 {
                "s"
            } else if *self & Self::SetGID != 0 {
                "S"
            } else if *self & Self::GroupExecute != 0 {
                "x"
            } else {
                "-"
            }
        )?;
        write!(f, "{}", if *self & Self::OtherRead != 0 { "r" } else { "-" })?;
        write!(f, "{}", if *self & Self::OtherWrite != 0 { "w" } else { "-" })?;
        write!(
            f,
            "{}",
            if *self & Self::Sticky != 0 {
                "t"
            } else if *self & Self::OtherExecute != 0 {
                "x"
            } else {
                "-"
            }
        )
    }
}

#[bitmask(u16)]
#[derive(FromPrimitive)]
pub enum UnlinkFlags {
    RemoveDir = Self(1 << 0),
    None = Self(0),
}

/*/// controls how File::lock() locks
pub enum LockKind {
    /// unlock a locked section
    Unlock,

    /// lock a section
    Lock,

    /// test if a section is locked and lock it
    TestLock,

    /// test if a section is locked
    Test
}*/
