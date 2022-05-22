//! virtual filesystems and filesystem interface

use bitmask_enum::bitmask;
use core::fmt;

/// standard unix permissions bit field
#[bitmask(u16)]
pub enum Permissions {
    OwnerRead       = Self(1 << 8),
    OwnerWrite      = Self(1 << 7),
    OwnerExecute    = Self(1 << 6),
    GroupRead       = Self(1 << 5),
    GroupWrite      = Self(1 << 4),
    GroupExecute    = Self(1 << 3),
    OtherRead       = Self(1 << 2),
    OtherWrite      = Self(1 << 1),
    OtherExecute    = Self(1 << 0),
}

impl fmt::Display for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", if *self & Self::OwnerRead != 0 { "r" } else { "-" })?;
        write!(f, "{}", if *self & Self::OwnerWrite != 0 { "w" } else { "-" })?;
        write!(f, "{}", if *self & Self::OwnerExecute != 0 { "x" } else { "-" })?;
        write!(f, "{}", if *self & Self::GroupRead != 0 { "r" } else { "-" })?;
        write!(f, "{}", if *self & Self::GroupWrite != 0 { "w" } else { "-" })?;
        write!(f, "{}", if *self & Self::GroupExecute != 0 { "x" } else { "-" })?;
        write!(f, "{}", if *self & Self::OtherRead != 0 { "r" } else { "-" })?;
        write!(f, "{}", if *self & Self::OtherWrite != 0 { "w" } else { "-" })?;
        write!(f, "{}", if *self & Self::OtherExecute != 0 { "x" } else { "-" })
    }
}

pub trait File {
    fn get_permissions(&self) -> Permissions;
    
}
