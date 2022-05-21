/* enter ring 3 (user mode) from ring 0 (kernel mode) */
.globl enter_user_mode
enter_user_mode:
    cli
    
    /* init function stack frame */
    push %ebp
    mov %esp, %ebp

    /* get pointer to function */
    mov 8(%ebp), %ecx

    /* get stack pointer */
    mov 16(%ebp), %edx

    /* enter ring 3. this is some magic shit im afraid to touch lest it completely break again */
    mov $0x23, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs

    /*mov %esp, %eax*/
    mov %edx, %eax
    pushl $0x23
    pushl %eax
    pushf
    pop %eax 
    or $0x200, %eax
    push %eax
    pushl $0x1b
    pushl %ecx
    iret


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

