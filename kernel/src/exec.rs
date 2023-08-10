use alloc::{sync::Arc, vec};
use common::Errno;
use log::debug;
use spin::Mutex;

pub fn exec(descriptor: &dyn crate::fs::FileDescriptor) -> common::Result<(Arc<Mutex<crate::mm::ProcessMap>>, usize)> {
    // read header
    let mut buf = [0; 52];
    if descriptor.read(&mut buf)? != buf.len() {
        return Err(Errno::ExecutableFormatErr);
    }
    let header = goblin::elf32::header::Header::from_bytes(&buf);

    // sanity check
    if header.e_type != goblin::elf::header::ET_EXEC {
        return Err(Errno::ExecutableFormatErr);
    }

    // read program headers
    descriptor.seek(header.e_phoff.try_into().map_err(|_| Errno::ValueOverflow)?, common::SeekKind::Set)?;
    let mut buf = vec![0; header.e_phentsize as usize * header.e_phnum as usize];
    if descriptor.read(&mut buf)? != buf.len() {
        return Err(Errno::ExecutableFormatErr);
    }
    let headers = goblin::elf32::program_header::ProgramHeader::from_bytes(&buf, header.e_phnum as usize);

    let arc_map = Arc::new(Mutex::new(crate::mm::ProcessMap::new()?));

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
                            crate::mm::MappingKind::FileCopy {
                                file_descriptor: descriptor.dup()?,
                                file_offset: file_offset.try_into().map_err(|_| Errno::ValueOverflow)?,
                            }
                        },
                        crate::mm::ContiguousRegion::new(base_addr.try_into().map_err(|_| Errno::ValueOverflow)?, region_len.try_into().map_err(|_| Errno::ValueOverflow)?),
                        protection,
                    );

                    map.add_mapping(&arc_map, mapping, false, true)?;
                }
                goblin::elf::program_header::PT_INTERP => return Err(Errno::ExecutableFormatErr),
                _ => (),
            }
        }
    }

    Ok((arc_map, header.e_entry as usize))
}
