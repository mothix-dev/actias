//! our terrible, and i cannot stress this enough, TERRIBLE filesystem implementation
//! abandon all hope, ye who enter here

pub mod vfs;
pub mod tree;
pub mod ops;

use alloc::{
    string::String,
    vec::Vec,
};

/// maximum amount of files allowed to be opened at once on the system
pub const MAX_FILES: usize = 8192;

pub fn dirname(path: &str) -> String {
    let mut elements = path.split('/').collect::<Vec<_>>();
    elements.pop();
    elements.join("/")
}

pub fn basename(path: &str) -> Option<&str> {
    path.split('/').last()
}

pub fn init() {
    debug!("initializing vfs");
    vfs::init();
}
