//! i586 global descriptor table (GDT) and task state segment (TSS)

use aligned::{Aligned, A16};
use core::mem::size_of;
use log::debug;
use x86::{
    bits32::task::TaskStateSegment,
    dtables::{lgdt, DescriptorTablePointer},
    segmentation::{BuildDescriptor, CodeSegmentType, DataSegmentType, Descriptor, DescriptorBuilder, SegmentDescriptorBuilder, SegmentSelector},
    task::load_tr,
    Ring,
};

/// how many entries do we want in our GDT
const GDT_ENTRIES: usize = 6;

/// the GDT itself (aligned to 16 bits for performance)
static mut GDT: Aligned<A16, [Descriptor; GDT_ENTRIES]> = Aligned([Descriptor::NULL; GDT_ENTRIES]);

/// the TSS lmao
static mut TSS: Aligned<A16, TaskStateSegment> = Aligned(TaskStateSegment::new());

/// initialize GDT and TSS
pub unsafe fn init(int_stack_end: u32) {
    // populate TSS
    TSS.ss0 = SegmentSelector::new(2, Ring::Ring0).bits(); // stack segment, kernel data segment descriptor index in GDT
    TSS.esp0 = int_stack_end;
    TSS.cs = SegmentSelector::new(1, Ring::Ring3).bits(); // no idea why these reference kernel segments but with ring 3
    TSS.ds = SegmentSelector::new(2, Ring::Ring3).bits();
    TSS.es = SegmentSelector::new(2, Ring::Ring3).bits();
    TSS.fs = SegmentSelector::new(2, Ring::Ring3).bits();
    TSS.gs = SegmentSelector::new(2, Ring::Ring3).bits();
    TSS.iobp_offset = size_of::<TaskStateSegment>() as u16; // size of TSS

    // populate GDT
    GDT[0] = Descriptor::NULL;
    GDT[1] = DescriptorBuilder::code_descriptor(0, 0x000fffff, CodeSegmentType::ExecuteRead)
        .present()
        .dpl(Ring::Ring0)
        .limit_granularity_4kb()
        .db()
        .finish();
    GDT[2] = DescriptorBuilder::data_descriptor(0, 0x000fffff, DataSegmentType::ReadWrite)
        .present()
        .dpl(Ring::Ring0)
        .limit_granularity_4kb()
        .db()
        .finish();
    GDT[3] = DescriptorBuilder::code_descriptor(0, 0x000fffff, CodeSegmentType::ExecuteRead)
        .present()
        .dpl(Ring::Ring3)
        .limit_granularity_4kb()
        .db()
        .finish();
    GDT[4] = DescriptorBuilder::data_descriptor(0, 0x000fffff, DataSegmentType::ReadWrite)
        .present()
        .dpl(Ring::Ring3)
        .limit_granularity_4kb()
        .db()
        .finish();

    let base = (&TSS as *const _) as u32;
    debug!("tss @ {:#x}, size {:#x}", base, size_of::<TaskStateSegment>() as u32);

    // DescriptorBuilder::tss_descriptor would be the proper way to do this If It Fucking Worked
    GDT[5] = DescriptorBuilder::code_descriptor(base, size_of::<TaskStateSegment>() as u32, CodeSegmentType::ExecuteAccessed)
        .present()
        .finish();

    // clear the system bit because rust-x86 completely fucking refuses to
    GDT[5].upper &= 0xffffefff;

    // load GDT
    lgdt(&DescriptorTablePointer::new(&GDT));

    // flush TSS
    load_tr(SegmentSelector::new(5, Ring::Ring0));
}
