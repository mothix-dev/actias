use crate::{arch::bsp::RegisterContext, mm::PageDirectory};
use alloc::{sync::Arc, vec::Vec};
use common::Syscalls;
use log::{error, trace};
use spin::{Mutex, RwLock};

pub type Registers = <crate::arch::InterruptManager as crate::arch::bsp::InterruptManager>::Registers;

/// low-level syscall handler. handles the parsing, execution, and error handling of syscalls
pub fn syscall_handler(registers: &mut Registers, num: u32, arg0: usize, arg1: usize, arg2: usize, arg3: usize) {
    let syscall = Syscalls::try_from(num);
    match syscall {
        Ok(Syscalls::IsComputerOn) => registers.syscall_return(Ok(1)),
        Ok(Syscalls::Exit) => exit_process(registers, arg0),
        Ok(Syscalls::Chmod) => registers.syscall_return(chmod(arg0, arg1).map(|_| 0).map_err(|e| e as usize)),
        Ok(Syscalls::Chown) => registers.syscall_return(chown(arg0, arg1, arg2).map(|_| 0).map_err(|e| e as usize)),
        Ok(Syscalls::Chroot) => registers.syscall_return(chroot(arg0).map(|_| 0).map_err(|e| e as usize)),
        Ok(Syscalls::Close) => registers.syscall_return(close(arg0).map(|_| 0).map_err(|e| e as usize)),
        Ok(Syscalls::Dup) => registers.syscall_return(dup(arg0).map_err(|e| e as usize)),
        Ok(Syscalls::Dup2) => registers.syscall_return(dup2(arg0, arg1).map(|_| 0).map_err(|e| e as usize)),
        Ok(Syscalls::Open) => registers.syscall_return(open(arg0, arg1, arg2, arg3).map_err(|e| e as usize)),
        Ok(Syscalls::Read) => registers.syscall_return(read(arg0, arg1, arg2).map_err(|e| e as usize)),
        Ok(Syscalls::Seek) => registers.syscall_return(seek(arg0, arg1, arg2).map_err(|e| e as usize)),
        Ok(Syscalls::Stat) => registers.syscall_return(stat(arg0, arg1).map(|_| 0).map_err(|e| e as usize)),
        Ok(Syscalls::Truncate) => registers.syscall_return(truncate(arg0, arg1).map(|_| 0).map_err(|e| e as usize)),
        Ok(Syscalls::Unlink) => registers.syscall_return(unlink(arg0, arg1, arg2, arg3).map(|_| 0).map_err(|e| e as usize)),
        Ok(Syscalls::Write) => registers.syscall_return(write(arg0, arg1, arg2).map_err(|e| e as usize)),
        Ok(Syscalls::Fork) => {
            let result = fork(registers).map_err(|e| e as usize);
            registers.syscall_return(result);
        }
        Err(err) => error!("invalid syscall {num} ({err})"),
    }
}

/// syscall handler for `exit`, exits the current process without cleaning up any files, returning the given result code to the parent process
fn exit_process(registers: &mut Registers, code: usize) {
    let _code = code as u8;
    // TODO: pass exit code back to parent process via wait()

    let global_state = crate::get_global_state();

    unsafe {
        global_state.page_directory.lock().switch_to();
    }

    // TODO: detect current CPU
    let scheduler = &global_state.cpus.read()[0].scheduler;

    let current_task = match scheduler.get_current_task() {
        Some(task) => task,
        None => unreachable!(),
    };

    // get pid for current task and mark it as invalid at the same time
    let pid = {
        let mut task = current_task.lock();
        task.exec_mode = crate::sched::ExecMode::Exited;
        task.pid
    };

    if let Some(pid) = pid && let Some(process) = global_state.process_table.read().get(pid) {
        trace!("exiting process {pid}");

        // ensure threads won't be scheduled again
        for thread in process.threads.read().iter() {
            thread.lock().exec_mode = crate::sched::ExecMode::Exited;
        }
    }

    // force a context switch so we don't have to wait for a timer
    scheduler.context_switch(registers);
}

struct ProcessGuard<'a> {
    guard: spin::RwLockReadGuard<'a, crate::process::ProcessTable>,
    pid: usize,
}

impl<'a> core::ops::Deref for ProcessGuard<'a> {
    type Target = crate::process::Process;

    fn deref(&self) -> &Self::Target {
        self.guard.get(self.pid).unwrap()
    }
}

fn get_current_process() -> common::Result<ProcessGuard<'static>> {
    let global_state = crate::get_global_state();

    // TODO: detect current CPU
    let scheduler = &global_state.cpus.read()[0].scheduler;

    let current_task = match scheduler.get_current_task() {
        Some(task) => task,
        None => unreachable!(),
    };

    let pid = current_task.lock().pid.ok_or(common::Errno::NoSuchProcess)?;

    if global_state.process_table.read().get(pid).is_some() {
        Ok(ProcessGuard {
            guard: global_state.process_table.read(),
            pid,
        })
    } else {
        Err(common::Errno::NoSuchProcess)
    }
}

/// syscall handler for `chmod`
fn chmod(file_descriptor: usize, permissions: usize) -> common::Result<()> {
    let permissions: u16 = permissions.try_into().map_err(|_| common::Errno::ValueOverflow)?;
    get_current_process()?.environment.chmod(file_descriptor, permissions.into())
}

/// syscall handler for `chown`
fn chown(file_descriptor: usize, owner: usize, group: usize) -> common::Result<()> {
    let owner = owner.try_into().map_err(|_| common::Errno::ValueOverflow)?;
    let group = group.try_into().map_err(|_| common::Errno::ValueOverflow)?;
    get_current_process()?.environment.chown(file_descriptor, Some(owner), Some(group))
}

/// syscall handler for `chroot`
fn chroot(file_descriptor: usize) -> common::Result<()> {
    let global_state = crate::get_global_state();

    // TODO: detect current CPU
    let scheduler = &global_state.cpus.read()[0].scheduler;

    let current_task = match scheduler.get_current_task() {
        Some(task) => task,
        None => unreachable!(),
    };

    let pid = current_task.lock().pid.ok_or(common::Errno::NoSuchProcess)?;

    global_state.process_table.write().get_mut(pid).ok_or(common::Errno::NoSuchProcess)?.environment.chroot(file_descriptor)
}

/// syscall handler for `close`
fn close(file_descriptor: usize) -> common::Result<()> {
    get_current_process()?.environment.close(file_descriptor)
}

/// syscall handler for `dup`
fn dup(file_descriptor: usize) -> common::Result<usize> {
    get_current_process()?.environment.dup(file_descriptor)
}

/// syscall handler for `dup`
fn dup2(file_descriptor: usize, other_fd: usize) -> common::Result<()> {
    get_current_process()?.environment.dup2(file_descriptor, other_fd)
}

/// syscall handler for `open`
fn open(at: usize, path: usize, path_len: usize, flags: usize) -> common::Result<usize> {
    let flags: u32 = flags.try_into().map_err(|_| common::Errno::ValueOverflow)?;
    let current_process = get_current_process()?;

    current_process
        .memory_map
        .lock()
        .map_in_area(&current_process.memory_map, path, path_len, crate::mm::MemoryProtection::Read)?;
    let buf = unsafe { core::slice::from_raw_parts(path as *const u8, path_len) };
    let path = core::str::from_utf8(buf).map_err(|_| common::Errno::InvalidArgument)?;

    current_process.environment.open(at, path, flags.try_into().map_err(|_| common::Errno::InvalidArgument)?)
}

/// syscall handler for `read`
fn read(file_descriptor: usize, buf: usize, buf_len: usize) -> common::Result<usize> {
    let current_process = get_current_process()?;

    current_process
        .memory_map
        .lock()
        .map_in_area(&current_process.memory_map, buf, buf_len, crate::mm::MemoryProtection::Write)?;
    let buf = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, buf_len) };
    current_process.environment.read(file_descriptor, buf)
}

/// syscall handler for `seek`
fn seek(file_descriptor: usize, offset: usize, kind: usize) -> common::Result<usize> {
    let kind: u32 = kind.try_into().map_err(|_| common::Errno::ValueOverflow)?;
    get_current_process()?
        .environment
        .seek(file_descriptor, (offset as isize) as i64, kind.try_into().map_err(|_| common::Errno::InvalidArgument)?)
        .and_then(|ofs| ofs.try_into().map_err(|_| common::Errno::ValueOverflow))
}

/// syscall handler for `stat`
fn stat(file_descriptor: usize, buf: usize) -> common::Result<()> {
    let current_process = get_current_process()?;

    current_process
        .memory_map
        .lock()
        .map_in_area(&current_process.memory_map, buf, core::mem::size_of::<common::FileStat>(), crate::mm::MemoryProtection::Write)?;
    let buf = buf as *mut common::FileStat;

    unsafe {
        *buf = current_process.environment.stat(file_descriptor)?;
    }

    Ok(())
}

/// syscall handler for `truncate`
fn truncate(file_descriptor: usize, len: usize) -> common::Result<()> {
    get_current_process()?.environment.truncate(file_descriptor, len.try_into().map_err(|_| common::Errno::ValueOverflow)?)
}

/// syscall handler for `unlink`
fn unlink(at: usize, path: usize, path_len: usize, flags: usize) -> common::Result<()> {
    let flags: u32 = flags.try_into().map_err(|_| common::Errno::ValueOverflow)?;
    let current_process = get_current_process()?;

    current_process
        .memory_map
        .lock()
        .map_in_area(&current_process.memory_map, path, path_len, crate::mm::MemoryProtection::Read)?;
    let buf = unsafe { core::slice::from_raw_parts(path as *const u8, path_len) };
    let path = core::str::from_utf8(buf).map_err(|_| common::Errno::InvalidArgument)?;

    current_process.environment.unlink(at, path, flags.try_into().map_err(|_| common::Errno::InvalidArgument)?)
}

/// syscall handler for `write`
fn write(file_descriptor: usize, buf: usize, buf_len: usize) -> common::Result<usize> {
    let current_process = get_current_process()?;

    current_process
        .memory_map
        .lock()
        .map_in_area(&current_process.memory_map, buf, buf_len, crate::mm::MemoryProtection::Read)?;
    let buf = unsafe { core::slice::from_raw_parts(buf as *mut u8, buf_len) };

    current_process.environment.write(file_descriptor, buf)
}

/// syscall handler for `fork`
fn fork(registers: &Registers) -> common::Result<usize> {
    let global_state = crate::get_global_state();

    // TODO: detect current CPU
    let scheduler = &global_state.cpus.read()[0].scheduler;

    let current_task = match scheduler.get_current_task() {
        Some(task) => task,
        None => unreachable!(),
    };

    // get the current task's pid and save its registers
    #[allow(clippy::clone_on_copy)]
    let pid = {
        let mut current_task = current_task.lock();

        current_task.registers = registers.clone();
        // set the child's return value here since there's no way of knowing which task this is in the list
        current_task.registers.syscall_return(Ok(0));

        current_task.pid.ok_or(common::Errno::NoSuchProcess)?
    };

    let mut process_table = global_state.process_table.write();
    let process = process_table.get_mut(pid).ok_or(common::Errno::NoSuchProcess)?;

    // clone the memory map and filesystem environment
    let memory_map = process.memory_map.lock().fork(true)?;
    let environment = process.environment.fork()?;

    // clone the threads
    let mut threads = Vec::with_capacity(process.threads.read().len());
    #[allow(clippy::clone_on_copy)]
    for task in process.threads.read().iter() {
        let task = task.lock();

        threads.push(Arc::new(Mutex::new(crate::sched::Task {
            registers: task.registers.clone(),
            exec_mode: task.exec_mode,
            niceness: task.niceness,
            cpu_time: task.cpu_time,
            memory_map: memory_map.clone(),
            pid: None,
        })));
    }

    // add new process to process table
    let threads = RwLock::new(threads);
    let new_pid = process_table.insert(crate::process::Process { threads, memory_map, environment }).unwrap();

    // update PIDs of all threads in the new process
    for task in process_table.get(new_pid).unwrap().threads.read().iter() {
        {
            let mut task = task.lock();
            task.pid = Some(new_pid);
        }

        scheduler.push_task(task.clone());
    }

    Ok(new_pid)
}
