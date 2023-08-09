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
                let mapping = crate::mm::Mapping::new(
                    if header.p_filesz == 0 {
                        crate::mm::MappingKind::Anonymous
                    } else {
                        crate::mm::MappingKind::FileCopy {
                            file_descriptor: descriptor.dup()?,
                            offset: header.p_offset as i64,
                            len: header.p_filesz as usize,
                        }
                    },
                    crate::mm::ContiguousRegion::new(header.p_vaddr as usize, header.p_memsz as usize),
                    crate::mm::MemoryProtection::Read | crate::mm::MemoryProtection::Write,
                );
                map.map(mapping, false);
            }
            goblin::elf::program_header::PT_INTERP => return Err(Errno::ExecutableFormatErr),
            _ => (),
        }
    }

    Ok((map, header.e_entry as usize))
}
