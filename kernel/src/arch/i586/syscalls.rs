//! i586 syscall handlers

use super::{
    ints::SyscallRegisters,
    tasks::{context_switch, idle_until_switch},
    PAGE_SIZE,
};
use crate::{
    arch::tasks::{exit_current_task, fork_task},
    platform::irq::MISSED_SWITCHES,
    tasks::{get_current_task, get_current_task_mut, BlockKind, IN_TASK},
    types::{
        errno::Errno,
        file::{FileDescriptor, FileStatus, OpenFlags, Permissions, SeekKind},
    },
};
use alloc::{string::String, vec, vec::Vec};
use core::{ffi::CStr, mem::size_of};
use log::{error, warn, info, debug, trace};

/// function prototype for individual syscall handlers
type SyscallHandler = fn(&mut SyscallRegisters) -> Result<(), Errno>;

/// amount of syscalls we have
pub const NUM_SYSCALLS: usize = 16;

/// list of function pointers for all available syscalls
pub static SYSCALL_LIST: [SyscallHandler; NUM_SYSCALLS] = [
    is_computer_on_handler,
    test_log_handler,
    fork_handler,
    exit_handler,
    get_pid_handler,
    get_parent_pid_handler,
    exec_handler,
    open_handler,
    close_handler,
    write_handler,
    read_handler,
    seek_handler,
    get_seek_handler,
    truncate_handler,
    stat_handler,
    unlink_handler,
];

/// makes sure a pointer is valid
///
/// wrapper around TaskState.check_ptr to make usage easier in syscall handlers
fn check_ptr(ptr: *const u8) -> Result<(), Errno> {
    debug!("checking ptr @ {:#x}", ptr as usize);
    if get_current_task_mut().unwrap().state.check_ptr(ptr) {
        Ok(())
    } else {
        Err(Errno::BadAddress)
    }
}

/// makes sure a slice is valid, given a pointer to its start and its length
fn check_slice(ptr: *const u8, len: usize) -> Result<(), Errno> {
    check_ptr(ptr)?;

    for i in (ptr as usize..ptr as usize + len).step_by(PAGE_SIZE) {
        check_ptr(i as *const _)?;
    }

    Ok(())
}

/// is computer on?
///
/// sets ebx to 1 (true) if computer is on
///
/// if computer is off, behavior is undefined
pub fn is_computer_on_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    regs.ebx = 1;

    Ok(())
}

/// test syscall- logs a string
pub fn test_log_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    check_ptr(regs.ebx as *const _)?;
    let string = unsafe { CStr::from_ptr(regs.ebx as *const _).to_string_lossy() };

    unsafe {
        IN_TASK = false;
    }

    info!("test_log: {}", string);

    Ok(())
}

/// forks task
///
/// sets ebx to the child pid in parent task, 0 in child task
pub fn fork_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    unsafe {
        IN_TASK = false;
    }

    // save state of current task
    get_current_task_mut().unwrap().state.save(regs);

    let pid = get_current_task().unwrap().id;

    match fork_task(pid) {
        Ok(new_task) => {
            // identify parent and child tasks
            regs.ebx = new_task.id.try_into().unwrap();
            new_task.state.registers.ebx = 0;
        }
        Err(msg) => warn!("could not fork pid {}: {}", pid, msg),
    };

    Ok(())
}

/// exits task
pub fn exit_handler(_regs: &mut SyscallRegisters) -> Result<(), Errno> {
    exit_current_task();
}

/// gets id of current task
///
/// sets ebx to id
pub fn get_pid_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    unsafe {
        IN_TASK = false;
    }

    regs.ebx = get_current_task().unwrap().id.try_into().map_err(|_| Errno::ValueOverflow)?;

    Ok(())
}

/// gets id of parent of current task
///
/// sets ebx to id
///
/// if the current task does not have a parent, ebx will be set to 0
pub fn get_parent_pid_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    unsafe {
        IN_TASK = false;
    }

    regs.ebx = get_current_task().unwrap().parent.unwrap_or(0).try_into().map_err(|_| Errno::ValueOverflow)?;

    Ok(())
}

/// parses a null-terminated array of pointers into a vec of strings, watching for invalid pointers
fn parse_ptr_array(ptr: *const *const u8) -> Result<Vec<String>, Errno> {
    let mut items: Vec<String> = vec![];
    let mut ptr = ptr as usize;

    // TODO: maybe specify a limit for number of arguments and argument length?

    unsafe {
        loop {
            check_ptr(ptr as *const _)?;
            if *(ptr as *const usize) == 0 {
                break;
            }
            check_ptr(*(ptr as *const *const _))?;
            items.push(CStr::from_ptr(*(ptr as *const *const _)).to_string_lossy().into_owned());
            ptr += size_of::<usize>();
        }
    }

    Ok(items)
}

/// replaces this process's address space with that of a new process at the path provided
pub fn exec_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    /*check_ptr(regs.ebx as *const _)?;
    let path = unsafe { CStr::from_ptr(regs.ebx as *const _).to_string_lossy() };

    // parse arguments and env
    let args = parse_ptr_array(regs.ecx as *const *const _)?;

    let env = parse_ptr_array(regs.edx as *const *const _)?;

    // leave task mode, anything that breaks now will really fuck things up
    unsafe {
        IN_TASK = false;
    }

    debug!("exec()ing {} as pid {}", path, get_current_task().unwrap().id);

    let mut new_task = get_current_task().unwrap().recreate();

    match crate::exec::exec_as(&mut new_task, &path, &args, &env) {
        Ok(_) => {
            let mut task = get_current_task_mut().unwrap();

            // free pages of old task
            task.state.free_pages();

            // exec_as only touches the state, so we can just replace the state and be fine
            task.state = new_task.state;

            // page table has been replaced, so switch to it
            task.state.pages.switch_to();

            // registers have been changed, update them
            *regs = task.state.registers;

            debug!("done exec()ing");

            Ok(())
        }
        Err(err) => {
            new_task.state.free_pages();
            Err(err)
        }
    }*/
    Err(Errno::NotSupported)
}

/// opens a file, returning a file descriptor
///
/// path of the file to open is a pointer to a null terminated string in ebx, flags are provided in register ecx, permissions if file is to be created are provided in register edx
///
/// new file descriptor is placed in ebx
pub fn open_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    check_ptr(regs.ebx as *const _)?;
    let path = unsafe { CStr::from_ptr(regs.ebx as *const _).to_string_lossy() };
    let flags = OpenFlags::from(regs.ecx as u8);
    let permissions = Permissions::from(regs.edx as u16);

    unsafe {
        IN_TASK = false;
    }

    debug!("open @ {} with flags {:?}", path, flags);

    regs.ebx = get_current_task_mut().unwrap().open(&path, flags, permissions)?.try_into().map_err(|_| Errno::FileDescTooBig)?;

    debug!("opened fd {}", regs.ebx);

    Ok(())
}

/// closes a file descriptor
///
/// file descriptor is given in ebx
pub fn close_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    unsafe {
        IN_TASK = false;
    }

    get_current_task_mut().unwrap().close(regs.ebx as usize)
}

/// writes to a file
///
/// file descriptor is provided in ebx, ecx and edx store the pointer to and length of the slice to write
///
/// number of bytes written is returned in ebx
pub fn write_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    let desc = regs.ebx as FileDescriptor;
    check_slice(regs.ecx as *const _, regs.edx as usize)?;
    let slice = unsafe { core::slice::from_raw_parts(regs.ecx as *mut u8, regs.edx as usize) };

    unsafe {
        IN_TASK = false;
    }

    let file = get_current_task_mut().unwrap().get_open_file(desc)?;

    debug!("writing max {} bytes to fd {}", regs.edx, desc);

    // do we have any space to write?
    if file.can_write(1)? {
        // yes, immediately write to file
        regs.ebx = get_current_task_mut().unwrap().get_open_file(desc)?.write(slice)?.try_into().map_err(|_| Errno::ValueOverflow)?;
        debug!("wrote {} bytes to fd {}", regs.ebx, desc);

        Ok(())
    } else if file.should_block {
        // no, block task until we do
        debug!("can't write, blocking task");

        get_current_task_mut().unwrap().block(BlockKind::Write(desc));

        // force a context switch immediately, as we don't want to be doing anything more with this task
        // and we want to be able to pick back up when data is available by retrying this syscall
        unsafe {
            MISSED_SWITCHES += 1;
        }

        Ok(())
    } else {
        // can't block, fail
        Err(Errno::TryAgain)
    }
}

/// reads from a file
///
/// file descriptor is provided in ebx, ecx and edx store the pointer to and length of the slice to read into
///
/// number of bytes read is returned in ebx
pub fn read_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    let desc = regs.ebx as FileDescriptor;
    check_slice(regs.ecx as *const _, regs.edx as usize)?;
    let slice = unsafe { core::slice::from_raw_parts_mut(regs.ecx as *mut u8, regs.edx as usize) };

    unsafe {
        IN_TASK = false;
    }

    let file = get_current_task_mut().unwrap().get_open_file(desc)?;

    debug!("reading max {} bytes from fd {}", regs.edx, desc);

    // do we have any space to read?
    if file.can_read(1)? {
        // yes, immediately read from file
        regs.ebx = file
            .read(slice)?
            .try_into()
            .map_err(|_| Errno::ValueOverflow)?;
        debug!("read {} bytes from fd {}", regs.ebx, desc);

        Ok(())
    } else if file.should_block {
        // no, block task until we do
        debug!("nothing to read, blocking task");

        get_current_task_mut().unwrap().block(BlockKind::Read(desc));

        unsafe {
            MISSED_SWITCHES += 1;
        }

        Ok(())
    } else {
        // can't block, fail
        Err(Errno::TryAgain)
    }
}

/// seek a file descriptor to a specific part of a file
///
/// file descriptor is provided in ebx, offset is provided in ecx, seek mode is provided in edx
///
/// low word of new offset of file descriptor is returned in ebx, high word is returned in ecx (it's 64 bit hehe)
pub fn seek_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    let desc = regs.ebx as FileDescriptor;
    let offset = regs.ecx as isize;

    if regs.edx > 2 {
        Err(Errno::InvalidSeek)?;
    }
    let kind = SeekKind::from(regs.edx as u8);

    unsafe {
        IN_TASK = false;
    }

    let offset = get_current_task_mut().unwrap().get_open_file(desc)?.seek(offset, kind)?;

    regs.ebx = offset as u32;
    regs.ecx = (offset >> 32) as u32;

    Ok(())
}

/// gets the offset from a file descriptor
///
/// file descriptor is provided in ebx, low word of offset is returned in ecx, high word of offset is returned in edx
pub fn get_seek_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    let desc = regs.ebx as FileDescriptor;

    unsafe {
        IN_TASK = false;
    }

    let offset = get_current_task_mut().unwrap().get_open_file(desc)?.offset;

    regs.ecx = offset as u32;
    regs.edx = (offset >> 32) as u32;

    Ok(())
}

/// truncate a file to a certain size
///
/// file descriptor is provided in ebx, new size is provided in ecx
pub fn truncate_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    let desc = regs.ebx as FileDescriptor;
    let size = regs.ecx as usize;

    unsafe {
        IN_TASK = false;
    }

    get_current_task_mut().unwrap().get_open_file(desc)?.truncate(size as u64)
}

/// gets the status of a file pointed to by a file descriptor
///
/// file descriptor is provided in ebx, pointer to stat struct is provided in ecx
pub fn stat_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    let desc = regs.ebx as FileDescriptor;
    check_slice(regs.ecx as *const _, size_of::<FileStatus>())?;
    let stat_struct = unsafe { &mut *(regs.ecx as *mut FileStatus) };

    unsafe {
        IN_TASK = false;
    }

    get_current_task_mut().unwrap().get_open_file(desc)?.file.stat(stat_struct)
}

/// removes a file from the filesystem
pub fn unlink_handler(regs: &mut SyscallRegisters) -> Result<(), Errno> {
    Err(Errno::NotSupported)
}

/// platform-specific syscall handler, will find the specific syscall to run from the eax register and insert the errno returned by the syscall handler
#[no_mangle]
pub unsafe extern "C" fn syscall_handler(mut regs: SyscallRegisters) {
    let syscall_num = regs.eax as usize;
    regs.eax = 0;

    if syscall_num < NUM_SYSCALLS {
        debug!("running syscall {}", syscall_num);
        if let Err(err) = SYSCALL_LIST[syscall_num](&mut regs) {
            warn!("syscall error: {}", err);
            regs.eax = err as u32;
        }

        // if we try and fail to context switch during a syscall (or just need to immediately context switch after regardless),
        // perform a context switch now so we don't spend too much cpu time on one process
        if MISSED_SWITCHES > 0 && !context_switch(&mut regs) {
            idle_until_switch();
        }

        IN_TASK = true;
    } else {
        warn!("bad syscall {}", syscall_num);
        exit_current_task();
    }
}
