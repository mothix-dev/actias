/* low level wrapper for syscalls, so we can access task state */
.extern syscall_handler
.globl syscall_handler_wrapper
syscall_handler_wrapper:
    cli

    pusha

    mov %ds, %ax
    push %eax

    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs

    call syscall_handler

    pop %ebx
    mov %bx, %ds
    mov %bx, %es
    mov %bx, %fs
    mov %bx, %gs

    popa

    sti
    iret

