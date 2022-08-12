/* small piece of code that's mapped into the page directories of the loader and kernel that switches between them, then calls the kernel */
/* cdecl, (page directory physical addr, stack pointer value, entry point) */
.section .text
.globl _start
_start:
    /* we don't need to bother with using esp, ebp, etc properly since we're not touching the stack and we're not returning */
    mov 4(%esp), %eax /* page directory physical addr */
    mov 8(%esp), %ebx /* stack pointer value */
    mov 12(%esp), %ecx /* entry point */

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
