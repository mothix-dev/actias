.section .text
.globl _start
.extern kmain

_start:
    mov $stack_end, %esp
    call kmain

    /* If kmain returns, halt forever */
halt:
    cli
    hlt
    jmp halt

.section .bss
.globl stack_base
.globl stack_end

stack_base:
    .space 0x1000 * 3
stack_end:
