//! tests

use core::arch::asm;
use crate::mm::heap::{KERNEL_HEAP, alloc, alloc_aligned, free, KHEAP_INITIAL_SIZE, HEAP_MIN_SIZE};
use crate::console::{ColorCode, TextConsole, get_console};
use alloc::vec::Vec;

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
fn int() {
    unsafe {
        asm!("int3");
    }
}

/// test heap alloc/free
#[test_case]
fn heap_alloc_free() {
    unsafe {
        debug!("{:?}", KERNEL_HEAP);
    }

    let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };

    #[cfg(debug_messages)]
    heap.print_holes();

    let heap_start = heap.index.get(0).0 as usize;

    debug!("heap start @ {:#x}", heap_start);

    let a = alloc::<u32>(8);
    let b = alloc::<u32>(8);

    debug!("a (8): {:#x}", a as usize);
    debug!("b (8): {:#x}", b as usize);

    #[cfg(debug_messages)]
    heap.print_holes();

    debug!("free a");

    free(a);

    #[cfg(debug_messages)]
    heap.print_holes();

    debug!("free b");

    free(b);

    #[cfg(debug_messages)]
    heap.print_holes();

    assert!(heap.index.size == 1);

    let c = alloc::<u32>(12);

    debug!("c (12): {:#x}", c as usize);

    assert!(c == a);

    let d = alloc::<u32>(1024);

    debug!("d (1024): {:#x}", d as usize);

    let e = alloc::<u32>(16);

    debug!("e (16): {:#x}", e as usize);

    #[cfg(debug_messages)]
    heap.print_holes();

    debug!("free c");

    free(c);

    #[cfg(debug_messages)]
    heap.print_holes();

    let f = alloc::<u32>(12);

    debug!("f (12): {:#x}", f as usize);

    #[cfg(debug_messages)]
    heap.print_holes();

    assert!(f == c);

    debug!("free e");

    free(e);

    debug!("free d");

    free(d);

    debug!("free f");

    free(f);

    assert!(heap.index.size == 1);

    #[cfg(debug_messages)]
    heap.print_holes();

    let g = alloc::<u32>(8);

    debug!("g (8): {:#x}", g as usize);

    #[cfg(debug_messages)]
    heap.print_holes();

    assert!(g == a);

    debug!("free g");
    
    free(g);

    #[cfg(debug_messages)]
    heap.print_holes();

    assert!(heap.index.size == 1);

    assert!(heap.index.get(0).0 as usize == heap_start);
}

/// test heap expand/contract
#[test_case]
fn heap_expand_contract() {
    let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };

    let heap_start = heap.index.get(0).0 as usize;
    
    let h = alloc::<u32>(2048);

    debug!("h (2048): {:#x}", h as usize);

    #[cfg(debug_messages)]
    heap.print_holes();

    debug!("size: {:#x}", heap.end_address - heap.start_address);

    let i = alloc::<u32>(KHEAP_INITIAL_SIZE);

    debug!("i ({}): {:#x}", KHEAP_INITIAL_SIZE, i as usize);

    #[cfg(debug_messages)]
    heap.print_holes();

    debug!("size: {:#x}", heap.end_address - heap.start_address);

    assert!(heap.end_address - heap.start_address > KHEAP_INITIAL_SIZE);

    debug!("free i");

    free(i);

    debug!("size: {:#x}", heap.end_address - heap.start_address);

    #[cfg(debug_messages)]
    heap.print_holes();

    assert!(heap.end_address - heap.start_address == HEAP_MIN_SIZE);

    debug!("free h");

    free(h);

    assert!(heap.index.get(0).0 as usize == heap_start);
}

/// test heap alloc alignment
#[test_case]
fn heap_alloc_align() {
    let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };
    
    for size in 1..32 {
        for i in 0..16 {
            let before = heap.index.get(0).0 as usize;
            let before_size = (unsafe { &*heap.index.get(0).0 }).size;

            debug!("before: addr @ {:#x}, size {:#x}", before, before_size);

            let alignment = 1 << i;
            let ptr = alloc_aligned::<u8>(size, alignment);
            //let ptr = alloc::<u8>(size);

            debug!("({}): {:#x} % {} == {}", size, ptr as usize, alignment, (ptr as usize) % alignment);

            #[cfg(debug_messages)]
            heap.print_holes();

            debug!("free");

            free(ptr);

            #[cfg(debug_messages)]
            heap.print_holes();

            assert!(heap.index.get(0).0 as usize == before);
            assert!((unsafe { &*heap.index.get(0).0 }).size == before_size);
        }
    }
}

/// test allocating aligned memory with existing allocation
#[test_case]
fn heap_alloc_align_2() {
    let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };

    let heap_start = heap.index.get(0).0 as usize;

    let h = alloc::<u32>(2048);

    debug!("h (2048): {:#x}", h as usize);

    for size in 1..32 {
        for i in 0..16 {
            let before = heap.index.get(0).0 as usize;
            let before_size = (unsafe { &*heap.index.get(0).0 }).size;

            debug!("before: addr @ {:#x}, size {:#x}", before, before_size);

            let alignment = 1 << i;
            let ptr = alloc_aligned::<u8>(size, alignment);

            debug!("({}): {:#x} % {} == {}", size, ptr as usize, alignment, (ptr as usize) % alignment);

            #[cfg(debug_messages)]
            heap.print_holes();

            debug!("free");
            
            free(ptr);

            #[cfg(debug_messages)]
            heap.print_holes();

            assert!(heap.index.get(0).0 as usize == before);
            assert!((unsafe { &*heap.index.get(0).0 }).size == before_size);
        }
    }

    debug!("free h");

    free(h);

    assert!(heap.index.get(0).0 as usize == heap_start);
}

/// make sure writing to vga console doesn't crash
#[test_case]
fn vga_partial() {
    let console = get_console().unwrap();

    for _i in 0..256 {
        for bg in 0..16 {
            for fg in 0..16 {
                console.set_color(ColorCode {
                    foreground: fg.into(),
                    background: bg.into()
                });
                console.puts("OwO ");
            }
        }
    }
}

#[test_case]
fn vec() {
    let mut vec: Vec<u32> = Vec::with_capacity(1);
    vec.push(3);
    vec.push(5);
    vec.push(9);
    vec.push(15);

    debug!("{:?}", vec);

    assert!(vec.len() == 4);
}
