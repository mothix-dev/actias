//! loading executable formats

use alloc::{
    vec, vec::Vec,
    string::String,
};
use byteorder::{ByteOrder, NativeEndian};
use core::mem::size_of;
use crate::{
    arch::{PAGE_SIZE, LINKED_BASE},
    fs::vfs::read_file,
    tasks::{Task, add_task},
    types::errno::Errno,
};
use goblin::elf::{
    Elf,
    program_header::{PT_LOAD, PT_INTERP},
};

/// spawn a process from the given path
pub fn exec(path: &str, args: &[String], env: &[String]) -> Result<usize, Errno> {
    let mut task = Task::new(0, 0);

    match exec_as(&mut task, path, args, env) {
        Ok(_) => {
            debug!("adding task");
            Ok(add_task(task))
        },
        Err(err) => {
            task.state.free_pages();
            Err(err)
        },
    }
}

/// maximum total size for all command line arguments passed to a program
const ARG_MAX: usize = 0x200000;

/// poke a slice of strings into memory of a task. useful for inserting arguments and environment variables into a task
fn poke_str_slice_into_mem(task: &mut Task, slice: &[String]) -> Result<usize, Errno> {
    // convert slice contents from strings to Vec<u8>s, for easy poking into memory
    let mut bytes = Vec::new();

    #[cfg(target_pointer_width = "64")]
    let mut offsets: Vec<u64> = Vec::new(); // used to calculate pointers to individual arguments in list

    #[cfg(target_pointer_width = "32")]
    let mut offsets: Vec<u32> = Vec::new();

    for str in slice {
        let mut str_bytes = str.as_bytes().to_vec();
        str_bytes.push(0); // make sure it's null terminated, otherwise everything breaks
        if bytes.len() + str_bytes.len() > ARG_MAX {
            return Err(Errno::TooBig);
        }
        offsets.push(bytes.len().try_into().map_err(|_| Errno::ValueOverflow)?);
        bytes.append(&mut str_bytes);
    }

    // prepare args for poking into process memory
    let hole = task.state.find_hole(PAGE_SIZE, bytes.len() + (slice.len() + 1) * size_of::<usize>()).ok_or(Errno::TooBig)?;

    let ptr = hole + bytes.len();

    #[cfg(target_pointer_width = "64")]
    for offset in offsets.iter_mut() {
        *offset += hole as u64;
    }

    #[cfg(target_pointer_width = "32")]
    for offset in offsets.iter_mut() {
        *offset += hole as u32;
    }

    offsets.push(0);

    debug!("offsets: {:?}", offsets);

    let mut temp: Vec<u8> = vec![0; offsets.len() * size_of::<usize>()];

    #[cfg(target_pointer_width = "64")]
    NativeEndian::write_u64_into(&offsets, &mut temp);

    #[cfg(target_pointer_width = "32")]
    NativeEndian::write_u32_into(&offsets, &mut temp);

    bytes.append(&mut temp);

    task.state.write_mem(hole as u64, &bytes, true)?;

    Ok(ptr)
}

/// replace a task's address space with that of the program at the given path
#[allow(clippy::vec_init_then_push)]
pub fn exec_as(task: &mut Task, path: &str, args: &[String], env: &[String]) -> Result<(), Errno> {
    debug!("exec_as: {:?} with args {:?}, env {:?}", path, args, env);

    let buffer = read_file(path)?;

    let elf = Elf::parse(&buffer).map_err(|_| Errno::ExecutableFormatErr)?;

    if elf.is_64 && size_of::<usize>() != 64 / 8 {
        Err(Errno::ExecutableFormatErr)
    } else {
        let mut lowest_addr = LINKED_BASE as u64;

        // assemble program in memory
        for ph in elf.program_headers {
            debug!("{:?}", ph);

            match ph.p_type {
                PT_LOAD => {
                    let file_start: usize = ph.p_offset.try_into().map_err(|_| Errno::ExecutableFormatErr)?;
                    let file_end: usize = (ph.p_offset + ph.p_filesz).try_into().map_err(|_| Errno::ExecutableFormatErr)?;

                    let filesz: usize = ph.p_filesz.try_into().map_err(|_| Errno::ExecutableFormatErr)?;
                    let memsz: usize = ph.p_memsz.try_into().map_err(|_| Errno::ExecutableFormatErr)?;

                    let data: Vec<u8> =
                        if filesz > 0 {
                            let mut data = vec![0; filesz];

                            data.clone_from_slice(&buffer[file_start..file_end]);

                            for _i in filesz..memsz {
                                data.push(0);
                            }

                            assert!(data.len() == memsz);

                            data
                        } else {
                            vec![0; memsz]
                        };
                    
                    debug!("data @ {:#x} - {:#x}", ph.p_vaddr, ph.p_vaddr + memsz as u64);

                    task.state.write_mem(ph.p_vaddr, &data, ph.is_write())?;

                    if ph.p_vaddr < lowest_addr {
                        lowest_addr = ph.p_vaddr;
                    }
                },
                PT_INTERP => {
                    // TODO: use data given by this header to load interpreter for dynamic linking
                    log!("dynamic linking not supported");
                    return Err(Errno::ExecutableFormatErr);
                },
                _ => debug!("unknown program header {:?}", ph.p_type),
            }
        }

        debug!("lowest @ {:#x}", lowest_addr);

        debug!("allocating stack");

        // alloc stack for task
        task.state.alloc_page((LINKED_BASE - PAGE_SIZE) as u32, false, true, false);

        debug!("preparing environment");
        let args_ptr = poke_str_slice_into_mem(task, args)?;
        let env_ptr = poke_str_slice_into_mem(task, env)?;

        debug!("preparing stack");

        // this seems to work for setting up a valid cdecl call frame? honestly idk
        #[cfg(target_pointer_width = "64")]
        let mut stack: Vec<u64> = Vec::new();

        #[cfg(target_pointer_width = "32")]
        let mut stack: Vec<u32> = Vec::new();

        stack.push(0);

        stack.push(args.len().try_into().map_err(|_| Errno::ValueOverflow)?); // argc
        stack.push(args_ptr.try_into().map_err(|_| Errno::ValueOverflow)?); // argv
        stack.push(env_ptr.try_into().map_err(|_| Errno::ValueOverflow)?); // envp

        stack.push(0);
        stack.push(0);

        let num_args = 5;

        let mut data_bytes: Vec<u8> = vec![0; stack.len() * size_of::<usize>()];

        #[cfg(target_pointer_width = "64")]
        NativeEndian::write_u64_into(&stack, &mut data_bytes);

        #[cfg(target_pointer_width = "32")]
        NativeEndian::write_u32_into(&stack, &mut data_bytes);

        let stack_addr = (LINKED_BASE - size_of::<usize>() - stack.len() * size_of::<usize>()) & !(16 - 1); // align to 16 byte boundary

        task.state.write_mem(stack_addr as u64, &data_bytes, true)?;

        // set up registers
        task.state.registers.useresp = stack_addr as u32;
        task.state.registers.esp = stack_addr as u32;
        task.state.registers.ebp = (stack_addr + num_args * size_of::<usize>()) as u32;
        task.state.registers.eip = elf.entry as u32;
        task.state.registers.ds = 0x23;
        task.state.registers.ss = 0x23;
        task.state.registers.cs = 0x1b;
        task.state.registers.eflags = 0b0000001000000010; // enable always set flag and interrupt enable flag

        Ok(())
    }
}
