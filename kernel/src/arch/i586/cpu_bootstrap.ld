OUTPUT_FORMAT(binary)

SECTIONS {
    . = 0x1000;
    .text : {
        *(.text .text.*)
    }
}
