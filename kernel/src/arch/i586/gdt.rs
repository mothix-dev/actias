//! i586 global descriptor table (GDT), task state segment (TSS), kernel stack

use aligned::{Aligned, A16};
use bitmask_enum::bitmask;
use core::{arch::asm, mem::size_of};
use x86::{
    dtables::{lgdt, DescriptorTablePointer},
    bits32::task::TaskStateSegment,
    segmentation::{Descriptor, DescriptorBuilder, SegmentDescriptorBuilder, CodeSegmentType, DataSegmentType},
};

/// how many entries do we want in our GDT
const GDT_ENTRIES: usize = 6;

/// the GDT itself (aligned to 16 bits for performance)
static mut GDT: Aligned<A16, [Descriptor; GDT_ENTRIES]> = Aligned([GDTEntry(0); GDT_ENTRIES]);

/// the TSS lmao
static mut TSS: Aligned<A16, TaskStateSegment> = Aligned(TaskStateSegment::new());

/// size of kernel stack
pub const STACK_SIZE: usize = 768 * 1024;

/// kernel stack
pub static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

#[no_mangle]
pub static STACK_TOP: &u8 = unsafe { &STACK[STACK_SIZE - 1] };

/// kernel stack for interrupt handlers
pub static mut INTERRUPT_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

/// flush TSS
unsafe fn flush_tss() {
    let index = (5 * 8) | 3;
    asm!("ltr ax", in("ax") index);
}

/// initialize GDT and TSS
pub unsafe fn init() {
    // populate TSS
    TSS.ss0 = 0x10; // kernel data segment descriptor
    TSS.esp0 = (&INTERRUPT_STACK[STACK_SIZE - 1] as *const _) as u32;
    TSS.cs = 0x0b;
    TSS.ds = 0x13;
    TSS.es = 0x13;
    TSS.fs = 0x13;
    TSS.gs = 0x13;

    // populate GDT
    GDT[0] = Descriptor::NULL;
    GDT[1] =
        DescriptorBuilder::code_descriptor::<u32>(0, 0x000fffff, CodeSegmentType::ExecuteRead)
            .present()
            .dpl(Ring::Ring0)
            .limit_granularity_4kb()
            .db()
            .finish();
    GDT[2] =
        DescriptorBuilder::data_descriptor::<u32>(0, 0x000fffff, DataSegmentType::ReadWrite)
            .present()
            .dpl(Ring::Ring0)
            .limit_granularity_4kb()
            .db()
            .finish();
    GDT[3] =
        DescriptorBuilder::code_descriptor::<u32>(0, 0x000fffff, CodeSegmentType::ExecuteRead)
            .present()
            .dpl(Ring::Ring3)
            .limit_granularity_4kb()
            .db()
            .finish();
    GDT[4] =
        DescriptorBuilder::data_descriptor::<u32>(0, 0x000fffff, DataSegmentType::ReadWrite)
            .present()
            .dpl(Ring::Ring3)
            .limit_granularity_4kb()
            .db()
            .finish();

    let base = (&TSS as *const _) as u32;
    GDT[5] =
        DescriptorBuilder::code_descriptor::<u32>(base, base + size_of::<TaskStateSegment>() as u32, CodeSegmentType::ExecuteAccessed)
            .present()
            .finish();

    // load GDT
    let gdt_desc = DescriptorTablePointer::new(&GDT);
    lgdt(&gdt_desc);

    flush_tss();
}
