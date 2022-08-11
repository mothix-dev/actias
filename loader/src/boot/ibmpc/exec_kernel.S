/* small piece of code that's mapped into the page directories of the loader and kernel that switches between them, then calls the kernel */
/* cdecl, (page directory physical addr, stack pointer value, entry point) */
.section .text
.globl _start
_start:
    push %ebp
    mov %esp, %ebp
    mov 8(%ebp), %eax /* page directory physical addr */
    mov 12(%ebp), %ebx /* stack pointer value */
    mov 16(%ebp), %ecx /* entry point */

    /* switch page directories */
    mov %eax, %cr3
    mov %cr0, %eax
    or $0x80000000, %eax
    mov %eax, %cr0

    /* set stack pointer */
    mov %ebx, %esp
    mov %esp, %ebp

    /* call kernel */
    jmp *%ecx
