use alloc::{boxed::Box, sync::Arc, vec};
use common::Errno;
use log::debug;
use spin::Mutex;

pub fn exec(file: crate::fs::OpenFile, mut callback: Box<dyn crate::fs::RequestCallback<(Arc<Mutex<crate::mm::ProcessMap>>, usize)>>) {
    let callback = Arc::new(Mutex::new(callback));
    let handle = file.handle().clone();

    handle.clone().make_request(crate::fs::Request::Read {
        position: 0,
        length: 52,
        callback: Box::new(move |res, first_blocked| {
            let buf = match res {
                Ok(buf) => buf,
                Err(err) => return (callback.lock())(Err(err), first_blocked),
            };

            let header = goblin::elf32::header::Header::from_bytes(match buf.try_into() {
                Ok(buf) => buf,
                Err(_) => return (callback.lock())(Err(Errno::ExecutableFormatErr), first_blocked),
            });

            // sanity check
            if header.e_type != goblin::elf::header::ET_EXEC {
                return (callback.lock())(Err(Errno::ExecutableFormatErr), first_blocked);
            }

            let header = Arc::new(header.clone());
            let callback = callback.clone();

            handle.make_request(crate::fs::Request::Read {
                position: header.e_phoff.try_into().unwrap(),
                length: header.e_phentsize as usize * header.e_phnum as usize,
                callback: Box::new(move |res, second_blocked| {
                    let blocked = first_blocked || second_blocked;

                    let buf = match res {
                        Ok(buf) => buf,
                        Err(err) => return (callback.lock())(Err(err), blocked),
                    };
                    let headers = goblin::elf32::program_header::ProgramHeader::from_bytes(buf, header.e_phnum as usize);

                    let arc_map = Arc::new(Mutex::new(crate::mm::ProcessMap::new().unwrap()));

                    {
                        let mut map = arc_map.lock();

                        for header in headers.iter() {
                            match header.p_type {
                                goblin::elf::program_header::PT_LOAD => {
                                    debug!("{header:?}");

                                    // align virtual address to specified alignment
                                    let offset = header.p_vaddr % header.p_align;
                                    let base_addr = header.p_vaddr - offset;
                                    let file_offset = header.p_offset - offset;
                                    let region_len = header.p_memsz + offset;

                                    let mut protection = crate::mm::MemoryProtection::None;
                                    if header.p_flags & goblin::elf::program_header::PF_R != 0 {
                                        protection |= crate::mm::MemoryProtection::Read;
                                    }
                                    if header.p_flags & goblin::elf::program_header::PF_W != 0 {
                                        protection |= crate::mm::MemoryProtection::Write;
                                    }
                                    if header.p_flags & goblin::elf::program_header::PF_X != 0 {
                                        protection |= crate::mm::MemoryProtection::Execute;
                                    }

                                    // create mapping
                                    let mapping = crate::mm::Mapping::new(
                                        if header.p_filesz == 0 {
                                            crate::mm::MappingKind::Anonymous
                                        } else {
                                            /*crate::mm::MappingKind::FileCopy {
                                                file_descriptor: descriptor.dup()?,
                                                file_offset: file_offset.try_into().map_err(|_| Errno::ValueOverflow)?,
                                            }*/
                                            todo!();
                                        },
                                        crate::mm::ContiguousRegion::new(
                                            match base_addr.try_into() {
                                                Ok(base) => base,
                                                Err(_) => return (callback.lock())(Err(Errno::ValueOverflow), blocked),
                                            },
                                            match region_len.try_into() {
                                                Ok(len) => len,
                                                Err(_) => return (callback.lock())(Err(Errno::ValueOverflow), blocked),
                                            },
                                        ),
                                        protection,
                                    );

                                    if let Err(err) = map.add_mapping(&arc_map, mapping, false, true) {
                                        return (callback.lock())(Err(err), blocked);
                                    }
                                }
                                goblin::elf::program_header::PT_INTERP => return (callback.lock())(Err(Errno::ExecutableFormatErr), blocked),
                                _ => (),
                            }
                        }
                    }

                    (callback.lock())(Ok((arc_map, header.e_entry as usize)), blocked);
                }),
            });
        }),
    });
}
