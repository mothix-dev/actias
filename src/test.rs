//! tests

use core::arch::asm;
use crate::mm::heap::{KERNEL_HEAP, alloc, alloc_aligned, free, KHEAP_INITIAL_SIZE, HEAP_MIN_SIZE};
use crate::platform::vga::*;
use crate::console::*;

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
    #[cfg(debug_messages)]
    unsafe {
        log!("{:?}", KERNEL_HEAP);
    }

    let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };

    #[cfg(debug_messages)]
    heap.print_holes();

    let heap_start = heap.index.get(0).0 as usize;

    #[cfg(debug_messages)]
    log!("heap start @ {:#x}", heap_start);

    let a = alloc::<u32>(8);
    let b = alloc::<u32>(8);

    #[cfg(debug_messages)]
    {
        log!("a (8): {:#x}", a as usize);
        log!("b (8): {:#x}", b as usize);
    }

    #[cfg(debug_messages)]
    {
        heap.print_holes();

        log!("free a");
    }

    free(a);

    #[cfg(debug_messages)]
    {
        heap.print_holes();

        log!("free b");
    }

    free(b);

    #[cfg(debug_messages)]
    heap.print_holes();

    assert!(heap.index.size == 1);

    let c = alloc::<u32>(12);

    #[cfg(debug_messages)]
    log!("c (12): {:#x}", c as usize);

    assert!(c == a);

    let d = alloc::<u32>(1024);

    #[cfg(debug_messages)]
    log!("d (1024): {:#x}", d as usize);

    let e = alloc::<u32>(16);

    #[cfg(debug_messages)]
    log!("e (16): {:#x}", e as usize);

    #[cfg(debug_messages)]
    {
        heap.print_holes();

        log!("free c");
    }

    free(c);

    #[cfg(debug_messages)]
    heap.print_holes();

    let f = alloc::<u32>(12);

    #[cfg(debug_messages)]
    {
        log!("f (12): {:#x}", f as usize);

        heap.print_holes();
    }

    assert!(f == c);

    #[cfg(debug_messages)]
    log!("free e");

    free(e);

    #[cfg(debug_messages)]
    log!("free d");

    free(d);

    #[cfg(debug_messages)]
    log!("free f");

    free(f);

    assert!(heap.index.size == 1);

    #[cfg(debug_messages)]
    heap.print_holes();

    let g = alloc::<u32>(8);

    #[cfg(debug_messages)]
    {
        log!("g (8): {:#x}", g as usize);

        heap.print_holes();
    }

    assert!(g == a);

    #[cfg(debug_messages)]
    log!("free g");
    
    free(g);

    #[cfg(debug_messages)]
    heap.print_holes();

    assert!(heap.index.size == 1);

    assert!(heap.index.get(0).0 as usize == heap_start);
}

/// test heap expand/contract
#[test_case]
fn test_heap_expand_contract() {
    let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };

    let heap_start = heap.index.get(0).0 as usize;
    
    let h = alloc::<u32>(2048);

    #[cfg(debug_messages)]
    log!("h (2048): {:#x}", h as usize);

    #[cfg(debug_messages)]
    heap.print_holes();

    #[cfg(debug_messages)]
    log!("size: {:#x}", heap.end_address - heap.start_address);

    let i = alloc::<u32>(KHEAP_INITIAL_SIZE);

    #[cfg(debug_messages)]
    log!("i ({}): {:#x}", KHEAP_INITIAL_SIZE, i as usize);

    #[cfg(debug_messages)]
    heap.print_holes();

    #[cfg(debug_messages)]
    log!("size: {:#x}", heap.end_address - heap.start_address);

    assert!(heap.end_address - heap.start_address > KHEAP_INITIAL_SIZE);

    #[cfg(debug_messages)]
    log!("free i");

    free(i);

    #[cfg(debug_messages)]
    log!("size: {:#x}", heap.end_address - heap.start_address);

    #[cfg(debug_messages)]
    heap.print_holes();

    assert!(heap.end_address - heap.start_address == HEAP_MIN_SIZE);

    #[cfg(debug_messages)]
    log!("free h");

    free(h);

    assert!(heap.index.get(0).0 as usize == heap_start);
}

/// test heap alloc alignment
#[test_case]
fn test_heap_alloc_align() {
    #[cfg(debug_messages)]
    let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };
    
    for size in 1..32 {
        for i in 0..16 {
            let alignment = 1 << i;
            let ptr = alloc_aligned::<u8>(size, alignment);
            //let ptr = alloc::<u8>(size);

            #[cfg(debug_messages)]
            {
                log!("({}): {:#x} % {} == {}", size, ptr as usize, alignment, (ptr as usize) % alignment);

                heap.print_holes();

                log!("free");
            }

            free(ptr);

            #[cfg(debug_messages)]
            heap.print_holes();
        }
    }
}

#[test_case]
fn test_heap_alloc_align_2() {
    let heap = unsafe { KERNEL_HEAP.as_mut().unwrap() };

    let heap_start = heap.index.get(0).0 as usize;

    let h = alloc::<u32>(2048);

    #[cfg(debug_messages)]
    log!("h (2048): {:#x}", h as usize);

    for size in 1..32 {
        for i in 0..16 {
            let before = heap.index.get(0).0 as usize;
            let before_size = (unsafe { &*heap.index.get(0).0 }).size;

            #[cfg(debug_messages)]
            log!("before: addr @ {:#x}, size {:#x}", before, before_size);

            let alignment = 1 << i;
            let ptr = alloc_aligned::<u8>(size, alignment);
            //let ptr = alloc::<u8>(size);

            #[cfg(debug_messages)]
            log!("({}): {:#x} % {} == {}", size, ptr as usize, alignment, (ptr as usize) % alignment);

            #[cfg(debug_messages)]
            heap.print_holes();

            #[cfg(debug_messages)]
            log!("free");
            
            free(ptr);

            #[cfg(debug_messages)]
            heap.print_holes();

            assert!(heap.index.get(0).0 as usize == before);
            assert!((unsafe { &*heap.index.get(0).0 }).size == before_size);
        }
    }

    #[cfg(debug_messages)]
    log!("free h");

    free(h);

    assert!(heap.index.get(0).0 as usize == heap_start);
}

/// make sure writing to vga console doesn't crash
#[test_case]
fn test_vga_partial() {
    let mut raw = create_console();
    let mut console = SimpleConsole::new(&mut raw, 80, 25);

    for _i in 0..256 {
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


