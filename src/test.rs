//! tests

use core::arch::asm;
use crate::mm::heap::{KERNEL_HEAP, alloc, alloc_aligned, free, KHEAP_INITIAL_SIZE, HEAP_MIN_SIZE};
use crate::platform::vga::*;
use crate::console::*;
use crate::arch::paging::virt_to_phys;
use crate::arch::{KHEAP_START, PAGE_SIZE};

/// custom test runner to run all tests
pub fn test_runner(tests: &[&dyn Testable]) {
    log!("=== Running {} tests", tests.len());
    for test in tests {
        test.run();
    }
    log!("=== Done");
}

/// custom testable trait
pub trait Testable {
    fn run(&self) -> ();
}

impl<T> Testable for T where T: Fn() {
    fn run(&self) {
        log!("--- {}...", core::any::type_name::<T>());
        self();
        log!("--- ok");
    }
}

/// test breakpoint interrupt
#[test_case]
fn test_int() {
    unsafe {
        asm!("int3");
    }
}

/// test heap alloc/free
#[test_case]
fn test_heap_alloc_free() {
    unsafe {
        log!("{:?}", KERNEL_HEAP);
    }

    let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };

    heap.print_holes();

    let a = alloc::<u32>(8);
    let b = alloc::<u32>(8);
    log!("a (8): {:#x}", a as usize);
    log!("b (8): {:#x}", b as usize);

    heap.print_holes();

    log!("free a");
    free(a);

    heap.print_holes();

    log!("free b");
    free(b);

    heap.print_holes();

    assert!(heap.index.size == 1);

    let c = alloc::<u32>(12);
    log!("c (12): {:#x}", c as usize);

    assert!(c == a);

    let d = alloc::<u32>(1024);
    log!("d (1024): {:#x}", d as usize);

    let e = alloc::<u32>(16);
    log!("e (16): {:#x}", e as usize);

    heap.print_holes();

    log!("free c");
    free(c);

    heap.print_holes();

    let f = alloc::<u32>(12);
    log!("f (12): {:#x}", f as usize);

    heap.print_holes();

    assert!(f == c);

    log!("free e");
    free(e);

    log!("free d");
    free(d);

    log!("free f");
    free(f);

    assert!(heap.index.size == 1);

    heap.print_holes();

    let g = alloc::<u32>(8);
    log!("g (8): {:#x}", g as usize);

    heap.print_holes();

    assert!(g == a);

    log!("free g");
    free(g);

    heap.print_holes();

    assert!(heap.index.size == 1);
}

/// test heap expand/contract
#[test_case]
fn test_heap_expand_contract() {
    let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };
    
    let h = alloc::<u32>(2048);
    log!("h (2048): {:#x}", h as usize);

    heap.print_holes();

    log!("size: {:#x}", heap.end_address - heap.start_address);

    let i = alloc::<u32>(KHEAP_INITIAL_SIZE);

    log!("i ({}): {:#x}", KHEAP_INITIAL_SIZE, i as usize);

    heap.print_holes();

    log!("size: {:#x}", heap.end_address - heap.start_address);

    assert!(heap.end_address - heap.start_address > KHEAP_INITIAL_SIZE);

    log!("free i");
    free(i);

    log!("size: {:#x}", heap.end_address - heap.start_address);

    heap.print_holes();

    assert!(heap.end_address - heap.start_address == HEAP_MIN_SIZE);
}

/// test heap alloc alignment
#[test_case]
fn test_heap_alloc_align() {
    for size in 1..32 {
        for i in 0..16 {
            let alignment = 1 << i;
            let ptr = alloc_aligned::<u8>(size, alignment);

            log!("({}): {:#x} % {} == {}", size, ptr as usize, alignment, (ptr as usize) % alignment);

            free(ptr);
        }
    }
}

/// make sure writing to vga console doesn't crash
#[test_case]
fn test_vga_partial() {
    let mut raw = create_console();
    let mut console = SimpleConsole::new(&mut raw, 80, 25);

    for i in 0..256 {
        for bg in 0..16 {
            for fg in 0..16 {
                console.color = ColorCode {
                    foreground: fg.into(),
                    background: bg.into()
                };
                console.puts("OwO ");
            }
        }
    }
}


