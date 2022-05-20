//! aspects of syscalls that we want to be platform independent

/// list of syscalls- we want this to be the same across all platforms
pub enum Syscalls {
    IsComputerOn = 0,
}
