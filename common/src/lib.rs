#![no_std]

mod errno;
pub use errno::Errno;

use bitmask_enum::bitmask;
use num_enum::TryFromPrimitive;

#[derive(Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
pub enum Syscalls {
    IsComputerOn,
    Exit,
    Chdir,
    Chmod,
    Chown,
    Chroot,
    Close,
    Dup,
    Dup2,
    Open,
    Read,
    Seek,
    Stat,
    Truncate,
    Unlink,
    Write,
    Fork,
}

/// flags passed to the open() syscall
#[derive(Default)]
#[bitmask(u32)]
pub enum OpenFlags {
    /// O_EXEC
    Exec = 1 << 0,
    Read = 1 << 1,
    Write = 1 << 2,
    /// O_SEARCH
    Search = 1 << 3,

    /// O_RDONLY
    ReadOnly = Self::Read.bits,
    /// O_RDWR
    ReadWrite = Self::Read.bits | Self::Write.bits,
    /// O_WRONLY
    WriteOnly = Self::Write.bits,

    /// O_APPEND
    Append = 1 << 4,
    /// O_CLOEXEC
    CloseOnExec = 1 << 5,
    /// O_CREAT
    Create = 1 << 6,
    /// O_DIRECTORY
    Directory = 1 << 7,
    /// O_DSYNC
    WriteSync = 1 << 8,
    /// O_EXCL
    Exclusive = 1 << 9,
    /// O_NOCTTY (ignored)
    NoCharTTY = 1 << 10,
    /// O_NOFOLLOW
    NoFollow = 1 << 11,
    /// O_NONBLOCK
    NonBlocking = 1 << 12,
    /// O_RSYNC
    ReadSync = 1 << 13,
    /// O_SYNC
    Synchronous = 1 << 14,
    /// O_TRUNC
    Truncate = 1 << 15,
    /// O_TTY_INIT (ignored)
    TTYInit = 1 << 16,
    /// AT_FDCWD
    AtCWD = 1 << 17,

    /// no POSIX equivalent, forces the open file to be a symlink (returning an error if it isn't) or creates a new symlink with `OpenFlags::Create`
    SymLink = 1 << 18,

    IgnoredMask = !(Self::NoCharTTY.bits | Self::TTYInit.bits),

    #[default]
    None = 0,
}

pub type Result<T> = core::result::Result<T, Errno>;

#[derive(Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
pub enum SeekKind {
    /// SEEK_SET
    Set,
    /// SEEK_CUR
    Current,
    /// SEEK_END
    End,
}

#[derive(Debug, Default, Copy, Clone)]
#[repr(C)]
pub struct FileStat {
    /// ID of the device containing this file
    pub device: u32,

    /// file serial number
    pub serial_num: u32,

    /// type and permissions of this file
    pub mode: FileMode,

    /// how many links exist to this file
    pub num_links: u32,

    /// user id of file
    pub user_id: UserId,

    /// group id of file
    pub group_id: GroupId,

    /// size of file
    pub size: i64,

    /// time of last access (in seconds)
    pub access_time: u64,

    /// time of last modification (in seconds)
    pub modification_time: u64,

    /// time of last status change (in seconds)
    pub status_change_time: u64,

    /// recommended block size for this file
    pub block_size: i32,

    /// how many blocks are allocated for this file
    pub num_blocks: i64,
}

pub type UserId = u32;
pub type GroupId = u32;

#[derive(Debug, Default, Copy, Clone)]
#[repr(C)]
pub struct FileMode {
    /// permissions of this file
    pub permissions: Permissions,

    /// what kind of file this is
    pub kind: FileKind,
}

/// standard unix permissions bit field
#[derive(Default)]
#[bitmask(u16)]
pub enum Permissions {
    SetUID = 1 << 11,
    SetGID = 1 << 10,
    Sticky = 1 << 9,
    OwnerRead = 1 << 8,
    OwnerWrite = 1 << 7,
    OwnerExecute = 1 << 6,
    GroupRead = 1 << 5,
    GroupWrite = 1 << 4,
    GroupExecute = 1 << 3,
    OtherRead = 1 << 2,
    OtherWrite = 1 << 1,
    OtherExecute = 1 << 0,
    #[default]
    None = 0,
}

impl core::fmt::Display for Permissions {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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

#[repr(u8)]
#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
pub enum FileKind {
    /// block special file
    BlockSpecial,

    /// character special file
    CharSpecial,

    /// directory
    Directory,

    /// FIFO/pipe
    FIFO,

    /// regular file
    #[default]
    Regular,

    /// symbolic link
    SymLink,

    /// socket
    Socket,

    /// message queue
    MessageQueue,

    /// semaphore
    Semaphore,

    /// shared memory
    SharedMemory,
}

/// flags passed to the unlink() syscall
#[derive(Default)]
#[bitmask(u32)]
pub enum UnlinkFlags {
    /// AT_REMOVEDIR
    RemoveDir = 1 << 0,

    /// AT_FDCWD
    AtCWD = 1 << 1,

    #[default]
    None = 0,
}

#[repr(C)]
#[derive(Debug)]
pub struct FilesystemEvent {
    /// the ID of this event
    pub id: usize,

    /// the file handle associated with this event
    pub handle: usize,

    /// what kind of event this is
    pub kind: EventKind,
}

#[repr(C)]
#[derive(Debug)]
pub enum EventKind {
    /// change the permissions of a file to those provided
    Chmod { permissions: Permissions },

    /// change the owner and group of a file to those provided
    Chown { owner: UserId, group: GroupId },

    /// close this file handle
    Close,

    /// open a new file in the directory pointed to by this file handle
    Open { name_length: usize, flags: OpenFlags },

    /// read from a file at the specified position
    Read { position: i64, length: usize },

    /// get information about a file
    Stat,

    /// truncate a file to the given length
    Truncate { length: i64 },

    /// remove a file from the directory pointed to by this file handle
    Unlink { name_length: usize, flags: UnlinkFlags },

    /// write to a file at the specified position
    Write { length: usize, position: i64 },
}

#[repr(C)]
#[derive(Debug)]
pub struct EventResponse {
    /// the ID of this event
    pub id: usize,

    /// data related to this event
    pub data: ResponseData,
}

#[repr(C)]
#[derive(Debug)]
pub enum ResponseData {
    /// request failed (can be any)
    Error { error: Errno },

    /// request returns a buffer (must be from `read`, `write, `stat`)
    Buffer { addr: usize, len: usize },

    /// request returns a file handle (must be from `open`)
    Handle { handle: usize },

    /// request doesn't return any data, just an acknowledgement that it completed
    None,
}
