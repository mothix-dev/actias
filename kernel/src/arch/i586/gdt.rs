//! i586 global descriptor table (GDT) and task state segment (TSS)

use alloc::{boxed::Box, vec};
use core::{mem::size_of, pin::Pin};
use log::debug;
use x86::{
    bits32::task::TaskStateSegment,
    dtables::{lgdt, DescriptorTablePointer},
    segmentation::{BuildDescriptor, CodeSegmentType, DataSegmentType, Descriptor, DescriptorBuilder, SegmentDescriptorBuilder, SegmentSelector},
    task::load_tr,
    Ring,
};

pub struct GDTState {
    _gdt: Pin<Box<[Descriptor; 6]>>,
    _tss: Pin<Box<TaskStateSegment>>,
    _int_stack: Pin<Box<[u8]>>,
}

/// initialize GDT and TSS
pub fn init(int_stack_size: usize) -> GDTState {
    let int_stack = Box::into_pin(vec![0_u8; int_stack_size].into_boxed_slice());

    let mut tss = Box::pin(TaskStateSegment::new());

    tss.ss0 = SegmentSelector::new(2, Ring::Ring0).bits(); // stack segment, kernel data segment descriptor index in GDT
    tss.esp0 = &int_stack[int_stack.len() - 1] as *const _ as u32;
    tss.cs = SegmentSelector::new(1, Ring::Ring3).bits(); // no idea why these reference kernel segments but with ring 3
    tss.ds = SegmentSelector::new(2, Ring::Ring3).bits();
    tss.es = SegmentSelector::new(2, Ring::Ring3).bits();
    tss.fs = SegmentSelector::new(2, Ring::Ring3).bits();
    tss.gs = SegmentSelector::new(2, Ring::Ring3).bits();
    tss.iobp_offset = size_of::<TaskStateSegment>() as u16; // size of TSS

    let mut gdt = Box::pin([Descriptor::NULL; 6]);

    gdt[0] = Descriptor::NULL;
    gdt[1] = DescriptorBuilder::code_descriptor(0, 0x000fffff, CodeSegmentType::ExecuteRead)
        .present()
        .dpl(Ring::Ring0)
        .limit_granularity_4kb()
        .db()
        .finish();
    gdt[2] = DescriptorBuilder::data_descriptor(0, 0x000fffff, DataSegmentType::ReadWrite)
        .present()
        .dpl(Ring::Ring0)
        .limit_granularity_4kb()
        .db()
        .finish();
    gdt[3] = DescriptorBuilder::code_descriptor(0, 0x000fffff, CodeSegmentType::ExecuteRead)
        .present()
        .dpl(Ring::Ring3)
        .limit_granularity_4kb()
        .db()
        .finish();
    gdt[4] = DescriptorBuilder::data_descriptor(0, 0x000fffff, DataSegmentType::ReadWrite)
        .present()
        .dpl(Ring::Ring3)
        .limit_granularity_4kb()
        .db()
        .finish();

    let base = (&*tss as *const _) as u32;
    debug!("tss @ {:#x}, size {:#x}", base, size_of::<TaskStateSegment>() as u32);

    // DescriptorBuilder::tss_descriptor would be the proper way to do this If It Fucking Worked
    gdt[5] = DescriptorBuilder::code_descriptor(base, size_of::<TaskStateSegment>() as u32, CodeSegmentType::ExecuteAccessed)
        .present()
        .finish();

    // clear the system bit because rust-x86 completely fucking refuses to
    gdt[5].upper &= 0xffffefff;

    unsafe {
        // load GDT
        lgdt(&DescriptorTablePointer::new(&*gdt));

        // flush TSS
        load_tr(SegmentSelector::new(5, Ring::Ring0));
    }

    GDTState {
        _gdt: gdt,
        _tss: tss,
        _int_stack: int_stack,
    }
}
