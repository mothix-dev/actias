use core::mem::size_of;

use crate::{
    arch::bsp::RegisterContext,
    fs::FsEnvironment,
    mm::PageDirectory,
    sched::{block_until, get_current_process},
};
use alloc::{string::ToString, sync::Arc, vec::Vec};
use common::{Errno, FileStat, Result, Syscalls};
use log::{error, trace};
use spin::{Mutex, RwLock};

pub type Registers = <crate::arch::InterruptManager as crate::arch::bsp::InterruptManager>::Registers;

/// low-level syscall handler. handles the parsing, execution, and error handling of syscalls
pub fn syscall_handler(registers: &mut Registers, num: u32, arg0: usize, arg1: usize, arg2: usize, arg3: usize) {
    let syscall = Syscalls::try_from(num);

    if let Ok(syscall) = &syscall {
        trace!("syscall {syscall:?} with args {arg0:#x}, {arg1:#x}, {arg2:#x}, {arg3:#x}");
    } else {
        trace!("invalid syscall {num} with args {arg0:#x}, {arg1:#x}, {arg2:#x}, {arg3:#x}");
    }

    match syscall {
        Ok(Syscalls::IsComputerOn) => registers.syscall_return(Ok(1)),
        Ok(Syscalls::Exit) => exit_process(registers, arg0),
        Ok(Syscalls::Chdir) => registers.syscall_return(chdir(arg0).map(|_| 0).map_err(|e| e as usize)),
        Ok(Syscalls::Chmod) => chmod(registers, arg0, arg1),
        Ok(Syscalls::Chown) => chown(registers, arg0, arg1, arg2),
        Ok(Syscalls::Chroot) => registers.syscall_return(chroot(arg0).map(|_| 0).map_err(|e| e as usize)),
        Ok(Syscalls::Close) => registers.syscall_return(close(arg0).map(|_| 0).map_err(|e| e as usize)),
        Ok(Syscalls::Dup) => registers.syscall_return(dup(arg0).map_err(|e| e as usize)),
        Ok(Syscalls::Dup2) => registers.syscall_return(dup2(arg0, arg1).map(|_| 0).map_err(|e| e as usize)),
        Ok(Syscalls::Open) => open(registers, arg0, arg1, arg2, arg3),
        Ok(Syscalls::Read) => read(registers, arg0, arg1, arg2),
        Ok(Syscalls::Seek) => seek(registers, arg0, arg1, arg2),
        Ok(Syscalls::Stat) => stat(registers, arg0, arg1),
        Ok(Syscalls::Truncate) => truncate(registers, arg0, arg1),
        Ok(Syscalls::Unlink) => unlink(registers, arg0, arg1, arg2, arg3),
        Ok(Syscalls::Write) => write(registers, arg0, arg1, arg2),
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

/// syscall handler for `chdir`
fn chdir(file_descriptor: usize) -> Result<()> {
    get_current_process()?.environment.chdir(file_descriptor)
}

/// syscall handler for `chmod`
fn chmod(registers: &mut Registers, file_descriptor: usize, permissions: usize) {
    block_until(registers, true, |process, state| async move {
        let permissions: u16 = match permissions.try_into() {
            Ok(val) => val,
            Err(_) => return state.syscall_return(Err(Errno::ValueOverflow)),
        };

        let res = process.environment.chmod(file_descriptor, permissions.into()).await;
        state.syscall_return(res.map(|_| 0));
    });
}

/// syscall handler for `chown`
fn chown(registers: &mut Registers, file_descriptor: usize, owner: usize, group: usize) {
    block_until(registers, true, |process, state| async move {
        let owner = match owner.try_into() {
            Ok(val) => val,
            Err(_) => return state.syscall_return(Err(Errno::ValueOverflow)),
        };
        let group = match group.try_into() {
            Ok(val) => val,
            Err(_) => return state.syscall_return(Err(Errno::ValueOverflow)),
        };

        let res = process.environment.chown(file_descriptor, owner, group).await;
        state.syscall_return(res.map(|_| 0));
    });
}

/// syscall handler for `chroot`
fn chroot(file_descriptor: usize) -> Result<()> {
    get_current_process()?.environment.chroot(file_descriptor)
}

/// syscall handler for `close`
fn close(file_descriptor: usize) -> Result<()> {
    get_current_process()?.environment.close(file_descriptor)
}

/// syscall handler for `dup`
fn dup(file_descriptor: usize) -> Result<usize> {
    get_current_process()?.environment.dup(file_descriptor)
}

/// syscall handler for `dup`
fn dup2(file_descriptor: usize, other_fd: usize) -> Result<()> {
    get_current_process()?.environment.dup2(file_descriptor, other_fd)
}

/// syscall handler for `open`
fn open(registers: &mut Registers, at: usize, path: usize, path_len: usize, flags: usize) {
    let buffer = match crate::process::ProcessBuffer::from_current_process(path, path_len) {
        Ok(buffer) => buffer,
        Err(err) => return registers.syscall_return(Err(err as usize)),
    };

    block_until(registers, true, |process, state| async move {
        let flags: u32 = match flags.try_into() {
            Ok(val) => val,
            Err(_) => return state.syscall_return(Err(Errno::ValueOverflow)),
        };
        let flags = match flags.try_into() {
            Ok(val) => val,
            Err(_) => return state.syscall_return(Err(Errno::InvalidArgument)),
        };

        let path = match buffer
            .map_in(|buf| core::str::from_utf8(buf).map_err(|_| common::Errno::InvalidArgument).map(|string| string.to_string()))
            .await
            .and_then(|res| res)
        {
            Ok(path) => path,
            Err(err) => return state.syscall_return(Err(err)),
        };

        let res = FsEnvironment::open(process.environment.clone(), at, path.to_string(), flags).await;
        state.syscall_return(res);
    });
}

/// syscall handler for `read`
fn read(registers: &mut Registers, file_descriptor: usize, buf: usize, buf_len: usize) {
    if buf_len == 0 {
        return;
    }

    let buffer = match crate::process::ProcessBuffer::from_current_process(buf, buf_len) {
        Ok(buffer) => buffer,
        Err(err) => return registers.syscall_return(Err(err as usize)),
    };

    block_until(registers, true, |process, state| async move {
        let res = process.environment.read(file_descriptor, buffer.into()).await;
        state.syscall_return(res);
    });
}

/// syscall handler for `seek`
fn seek(registers: &mut Registers, file_descriptor: usize, offset: usize, kind: usize) {
    block_until(registers, true, |process, state| async move {
        let kind: u32 = match kind.try_into() {
            Ok(val) => val,
            Err(_) => return state.syscall_return(Err(Errno::ValueOverflow)),
        };
        let kind = match kind.try_into() {
            Ok(val) => val,
            Err(_) => return state.syscall_return(Err(Errno::InvalidArgument)),
        };

        let res = process.environment.seek(file_descriptor, (offset as isize) as i64, kind).await;
        state.syscall_return(res.map(|val| (val as isize) as usize));
    });
}

/// syscall handler for `stat`
fn stat(registers: &mut Registers, file_descriptor: usize, buf: usize) {
    let buf_len = size_of::<FileStat>();
    let buffer = match crate::process::ProcessBuffer::from_current_process(buf, buf_len) {
        Ok(buffer) => buffer,
        Err(err) => return registers.syscall_return(Err(err as usize)),
    };

    block_until(registers, true, |process, state| async move {
        let res = process.environment.stat(file_descriptor).await;
        state.syscall_return(match res {
            Ok(stat) => {
                let to_read = unsafe { core::slice::from_raw_parts(&stat as *const _ as *const u8, buf_len) };
                buffer.copy_from(to_read).await.map_err(Errno::from)
            }
            Err(err) => Err(err),
        });
    });
}

/// syscall handler for `truncate`
fn truncate(registers: &mut Registers, file_descriptor: usize, len: usize) {
    block_until(registers, true, |process, state| async move {
        let len = match len.try_into() {
            Ok(val) => val,
            Err(_) => return state.syscall_return(Err(Errno::ValueOverflow)),
        };
        let res = process.environment.truncate(file_descriptor, len).await;
        state.syscall_return(res.map(|_| 0));
    });
}

/// syscall handler for `unlink`
fn unlink(registers: &mut Registers, at: usize, path: usize, path_len: usize, flags: usize) {
    let buffer = match crate::process::ProcessBuffer::from_current_process(path, path_len) {
        Ok(buffer) => buffer,
        Err(err) => return registers.syscall_return(Err(err as usize)),
    };

    block_until(registers, true, |process, state| async move {
        let flags: u32 = match flags.try_into() {
            Ok(val) => val,
            Err(_) => return state.syscall_return(Err(Errno::ValueOverflow)),
        };
        let flags = match flags.try_into() {
            Ok(val) => val,
            Err(_) => return state.syscall_return(Err(Errno::InvalidArgument)),
        };

        let path = match buffer
            .map_in(|buf| core::str::from_utf8(buf).map_err(|_| common::Errno::InvalidArgument).map(|string| string.to_string()))
            .await
            .and_then(|res| res)
        {
            Ok(path) => path,
            Err(err) => return state.syscall_return(Err(err)),
        };

        let res = FsEnvironment::unlink(process.environment.clone(), at, path.to_string(), flags).await;
        state.syscall_return(res.map(|_| 0));
    });
}

/// syscall handler for `write`
fn write(registers: &mut Registers, file_descriptor: usize, buf: usize, buf_len: usize) {
    if buf_len == 0 {
        return;
    }

    let buffer = match crate::process::ProcessBuffer::from_current_process(buf, buf_len) {
        Ok(buffer) => buffer,
        Err(err) => return registers.syscall_return(Err(err as usize)),
    };

    block_until(registers, true, |process, state| async move {
        let res = process.environment.write(file_descriptor, buffer.into()).await;
        state.syscall_return(res);
    });
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
    let new_pid = process_table
        .insert(crate::process::Process {
            threads,
            memory_map,
            environment: Arc::new(environment),
            filesystem: None.into(),
        })
        .unwrap();

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
