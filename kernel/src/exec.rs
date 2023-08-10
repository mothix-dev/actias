use alloc::vec;
use common::Errno;
use log::debug;

pub fn exec(descriptor: &dyn crate::fs::FileDescriptor) -> common::Result<(crate::mm::ProcessMap, usize)> {
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

    let mut map = crate::mm::ProcessMap::new();

    for header in headers.iter() {
        match header.p_type {
            goblin::elf::program_header::PT_LOAD => {
                debug!("{header:?}");

                // align virtual address to specified alignment
                let offset = header.p_vaddr % header.p_align;
                let base_addr = header.p_vaddr - offset;
                let file_offset = header.p_offset - offset;
                let region_len = header.p_memsz + offset;

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
                    crate::mm::MemoryProtection::Read | crate::mm::MemoryProtection::Write,
                );

                map.add_mapping(mapping, false, true)?;
            }
            goblin::elf::program_header::PT_INTERP => return Err(Errno::ExecutableFormatErr),
            _ => (),
        }
    }

    Ok((map, header.e_entry as usize))
}
