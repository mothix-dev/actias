//! aspects of syscalls that we want to be platform independent

/// list of syscalls- we want this to be the same across all platforms
#[repr(u32)]
pub enum Syscall {
    IsComputerOn = 0,
    TestLog,      // to be removed
    Fork,         // fork()
    Exit,         // _exit()
    GetPID,       // getpid()
    GetParentPID, // getppid()
    Exec,         // execve()
    Open,         // open()
    Close,        // close()
    Write,        // write()
    Read,         // read()
    Seek,         // fseek()
    GetSeek,      // ftell()
    Truncate,     // truncate()
    Stat,         // fstat()
    Unlink,       // unlinkat() ish?
}
