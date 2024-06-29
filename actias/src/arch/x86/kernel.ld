ENTRY(_start)
OUTPUT_FORMAT(elf32-i386)

SECTIONS {
    . = 0xe0000000;

    kernel_start = .;

    .text : {
        *(.text .text.*)
    }

    /* read-only data, page aligned to allow use of the no-execute feature */
    . = ALIGN(0x1000);
    .rodata : {
        *(.rodata .rodata.*)
    }

    /* read-write data, page aligned for the .padata section */
    . = ALIGN(0x1000);
    .data : {
        *(.padata)
        *(.data .data.*)
    }

    /* zero-initialised data */
    .bss : {
        *(.bss .bss.*)
    }

    . = ALIGN(0x1000);
    kernel_end = .;
}
