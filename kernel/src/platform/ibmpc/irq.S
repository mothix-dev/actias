/* low level irq handlers */

.extern timer_handler
.globl timer_handler_wrapper
timer_handler_wrapper:
    cli

    pusha

    mov %ds, %ax
    push %eax

    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs

    call timer_handler

    pop %ebx
    mov %bx, %ds
    mov %bx, %es
    mov %bx, %fs
    mov %bx, %gs

    popa

    sti

    iret