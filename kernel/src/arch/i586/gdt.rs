//! i586 global descriptor table (GDT) and task state segment (TSS)

use aligned::{Aligned, A4};
use alloc::alloc::{alloc, Layout};
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

/// the GDT itself (aligned to 32 bits for performance)
static mut GDT: Aligned<A4, [Descriptor; GDT_ENTRIES]> = Aligned([Descriptor::NULL; GDT_ENTRIES]);

/// the TSS lmao
static mut TSS: Aligned<A4, TaskStateSegment> = Aligned(TaskStateSegment::new());

fn populate_tss(tss: &mut TaskStateSegment, int_stack_end: u32) {
    tss.ss0 = SegmentSelector::new(2, Ring::Ring0).bits(); // stack segment, kernel data segment descriptor index in GDT
    tss.esp0 = int_stack_end;
    tss.cs = SegmentSelector::new(1, Ring::Ring3).bits(); // no idea why these reference kernel segments but with ring 3
    tss.ds = SegmentSelector::new(2, Ring::Ring3).bits();
    tss.es = SegmentSelector::new(2, Ring::Ring3).bits();
    tss.fs = SegmentSelector::new(2, Ring::Ring3).bits();
    tss.gs = SegmentSelector::new(2, Ring::Ring3).bits();
    tss.iobp_offset = size_of::<TaskStateSegment>() as u16; // size of TSS
}

fn populate_gdt(gdt: &mut [Descriptor; GDT_ENTRIES], tss: &TaskStateSegment) {
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

    let base = (tss as *const _) as u32;
    debug!("tss @ {:#x}, size {:#x}", base, size_of::<TaskStateSegment>() as u32);

    // DescriptorBuilder::tss_descriptor would be the proper way to do this If It Fucking Worked
    gdt[5] = DescriptorBuilder::code_descriptor(base, size_of::<TaskStateSegment>() as u32, CodeSegmentType::ExecuteAccessed)
        .present()
        .finish();

    // clear the system bit because rust-x86 completely fucking refuses to
    gdt[5].upper &= 0xffffefff;
}

/// initialize GDT and TSS
pub unsafe fn init(int_stack_end: u32) {
    populate_tss(&mut TSS, int_stack_end);
    populate_gdt(&mut GDT, &TSS);

    // load GDT
    lgdt(&DescriptorTablePointer::new(&GDT));

    // flush TSS
    load_tr(SegmentSelector::new(5, Ring::Ring0));
}

pub fn init_other_cpu(int_stack_size: usize) {
    let int_stack_layout = Layout::from_size_align(int_stack_size, 0x1000).unwrap();
    let int_stack = unsafe { alloc(int_stack_layout) };

    // TODO: free lowest page of int stack

    let tss_layout = Layout::from_size_align(size_of::<TaskStateSegment>(), 4).unwrap();
    let tss = unsafe { &mut *(alloc(tss_layout) as *mut TaskStateSegment) };

    populate_tss(tss, (int_stack as usize + int_stack_size - 1).try_into().unwrap());

    let gdt_layout = Layout::from_size_align(size_of::<Descriptor>() * GDT_ENTRIES, 4).unwrap();
    let gdt = unsafe { &mut *(alloc(gdt_layout) as *mut [Descriptor; GDT_ENTRIES]) };

    populate_gdt(gdt, tss);

    unsafe {
        // load GDT
        lgdt(&DescriptorTablePointer::new(gdt));

        // flush TSS
        load_tr(SegmentSelector::new(5, Ring::Ring0));
    }
}
