use alloc::{boxed::Box, sync::Arc, vec};
use common::Errno;
use log::debug;
use spin::Mutex;

pub fn exec(file: crate::fs::OpenFile, callback: Box<dyn crate::fs::RequestCallback<(Arc<Mutex<crate::mm::ProcessMap>>, usize)>>) {
    let handle = file.handle().clone();

    let buffer = Arc::new(Mutex::new(vec![0; 52].into_boxed_slice()));

    handle.clone().read(
        0,
        buffer.clone().into(),
        Box::new(move |res, first_blocked| {
            match res {
                Ok(52) => (),
                Ok(_) => return callback(Err(Errno::TryAgain), first_blocked),
                Err(err) => return callback(Err(err), first_blocked),
            };

            let buffer = buffer.lock();
            let header = goblin::elf32::header::Header::from_bytes(match buffer[..].try_into() {
                Ok(buf) => buf,
                Err(_) => return callback(Err(Errno::ExecutableFormatErr), first_blocked),
            });

            // sanity check
            if header.e_type != goblin::elf::header::ET_EXEC {
                return callback(Err(Errno::ExecutableFormatErr), first_blocked);
            }

            let header = Arc::new(*header);
            let buffer = Arc::new(Mutex::new(vec![0; header.e_phentsize as usize * header.e_phnum as usize].into_boxed_slice()));

            handle.clone().read(
                header.e_phoff.try_into().unwrap(),
                buffer.clone().into(),
                Box::new(move |res, second_blocked| {
                    let blocked = first_blocked || second_blocked;

                    if let Err(err) = res {
                        return callback(Err(err), blocked);
                    };
                    let headers = goblin::elf32::program_header::ProgramHeader::from_bytes(&buffer.lock()[..], header.e_phnum as usize);

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
                                            crate::mm::MappingKind::File {
                                                file_handle: handle.clone(),
                                                file_offset: match file_offset.try_into() {
                                                    Ok(offset) => offset,
                                                    Err(_) => return callback(Err(Errno::ValueOverflow), blocked),
                                                },
                                            }
                                        },
                                        crate::mm::ContiguousRegion::new(
                                            match base_addr.try_into() {
                                                Ok(base) => base,
                                                Err(_) => return callback(Err(Errno::ValueOverflow), blocked),
                                            },
                                            match region_len.try_into() {
                                                Ok(len) => len,
                                                Err(_) => return callback(Err(Errno::ValueOverflow), blocked),
                                            },
                                        ),
                                        protection,
                                    );

                                    if let Err(err) = map.add_mapping(&arc_map, mapping, false, true) {
                                        return callback(Err(err), blocked);
                                    }
                                }
                                goblin::elf::program_header::PT_INTERP => return callback(Err(Errno::ExecutableFormatErr), blocked),
                                _ => (),
                            }
                        }
                    }

                    callback(Ok((arc_map, header.e_entry as usize)), blocked);
                }),
            );
        }),
    );
}
