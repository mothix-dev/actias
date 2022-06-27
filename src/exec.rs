//! loading executable formats

use alloc::{
    vec, vec::Vec,
};
use core::mem::size_of;
use crate::{
    errno::Errno,
    fs::vfs::read_file,
    tasks::{Task, add_task},
    arch::{
        PAGE_SIZE, LINKED_BASE,
    },
};
use goblin::elf::{
    Elf,
    program_header::{PT_PHDR, PT_LOAD, PT_INTERP},
    section_header::{SHT_NULL, SHT_PROGBITS, SHT_NOBITS},
};

/// spawn a process from the given path
pub fn exec(path: &str) -> Result<(), Errno> {
    let mut task = Task::new();

    match exec_as(&mut task, path) {
        Ok(_) => Ok(add_task(task)),
        Err(err) => Err(err),
    }
}

/// replace a task's address space with that of the program at the given path
pub fn exec_as(task: &mut Task, path: &str) -> Result<(), Errno> {
    let buffer = read_file(path).unwrap();

    let elf = Elf::parse(&buffer).map_err(|_| Errno::ExecutableFormatErr)?;

    if elf.is_64 && size_of::<usize>() != 64 / 8 {
        Err(Errno::ExecutableFormatErr)
    } else {
        //log!("got elf: {:#?}", elf);
        let entry = elf.entry;

        // alloc stack for task
        task.state.alloc_page((LINKED_BASE - PAGE_SIZE) as u32, false, true, false);

        task.state.registers.useresp = (LINKED_BASE - 1) as u32;
        task.state.registers.esp = (LINKED_BASE - 1) as u32;
        task.state.registers.ebp = (LINKED_BASE - 1) as u32;
        task.state.registers.eip = entry as u32;
        task.state.registers.ds = 0x23;
        task.state.registers.ss = 0x23;
        task.state.registers.cs = 0x1b;
        task.state.registers.eflags = 0b0000001000000010; // enable always set flag and interrupt enable flag

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

                    task.state.write_mem(ph.p_vaddr, &data, ph.is_write()).unwrap();
                },
                PT_INTERP => {
                    // TODO: use data given by this header to load interpreter for dynamic linking
                    log!("dynamic linking not supported");
                    return Err(Errno::ExecutableFormatErr);
                },
                _ => debug!("unknown program header {:?}", ph.p_type),
            }
        }

        for sh in elf.section_headers {
            debug!("{:?}", sh);

            match sh.sh_type {
                SHT_NULL => (),
                SHT_PROGBITS => {
                    let start: usize = sh.sh_offset.try_into().map_err(|_| Errno::ExecutableFormatErr)?;
                    let end: usize = (sh.sh_offset + sh.sh_size).try_into().map_err(|_| Errno::ExecutableFormatErr)?;

                    debug!("data @ {:#x} - {:#x}", sh.sh_addr, sh.sh_addr + sh.sh_size);
                    
                    task.state.write_mem(sh.sh_addr, &buffer[start..end], sh.is_writable()).map_err(|_| Errno::ExecutableFormatErr)?;
                },
                SHT_NOBITS => {
                    let data: Vec<u8> = vec![sh.sh_size.try_into().expect("program header size too big"); 0];

                    debug!("data @ {:#x} - {:#x}", sh.sh_addr, sh.sh_addr + sh.sh_size);
                    
                    task.state.write_mem(sh.sh_addr, &data, sh.is_writable()).map_err(|_| Errno::ExecutableFormatErr)?;
                },
                _ => debug!("unknown section header {:?}", sh.sh_type),
            }
        }

        Ok(())
    }
}
