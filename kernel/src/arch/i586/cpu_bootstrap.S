.code16
.section .text, "ax"
.globl _start
_start:
    cli
    cld

    xorw %ax, %ax
    movw %ax, %ds

    /* enable a20 */
.wait1:
    in $0x64, %al
    test $2, %al
    jnz .wait1

    mov $0xad, %al
    outb %al, $0x64

.wait2:
    in $0x64, %al
    test $2, %al
    jnz .wait2

    mov $0xd0, %al
    outb %al, $0x64

.wait3:
    in $0x64, %al
    test $1, %al
    jz .wait3

    in $0x60, %al
    mov %ax, %bx

.wait4:
    in $0x64, %al
    test $2, %al
    jnz .wait4

    mov $0xd1, %al
    outb %al, $0x64

.wait5:
    in $0x64, %al
    test $2, %al
    jnz .wait5

    mov %bx, %ax
    or $2, %al
    outb %al, $0x60

.wait6:
    in $0x64, %al
    test $2, %al
    jnz .wait6

    mov $0xae, %al
    outb %al, $0x64

.wait7:
    in $0x64, %al
    test $2, %al
    jnz .wait7

    /* load initial GDT */
    lgdt GDTPtr

    /* set page directory */
    movl 0x2000 - 12, %eax
    movl %eax, %cr3

    /* enable global pages */
    movl %cr4, %eax
    orl $0x80, %eax
    movl %eax, %cr4

    /* enable protected mode and paging */
    movl %cr0, %eax
    orl $0x80010001, %eax
    movl %eax, %cr0

    /* jump to our 32 bit code */
    ljmp $8, $start_32bit

.align 16
GDT:
    .long 0x00000000, 0x00000000    /* 00 NULL Entry */
    .long 0x0000FFFF, 0x00CF9A00    /* 08 PL0 Code */
    .long 0x0000FFFF, 0x00CF9200    /* 10 PL0 Data */
GDTPtr:
    .word GDTPtr - GDT - 1
    .long GDT

.align 32
.code32
start_32bit:
    /* prep segment registers */
    mov $0x10, %ax
    mov %ax, %ss
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs

    /* set stack pointer */
    mov 0x2000 - 4, %eax
    mov %eax, %esp

    /* jump to actual code */
    mov 0x2000 - 8, %eax
    call *%eax

    /* if we return from this, halt the CPU */
.l:
    cli
    hlt
    jmp .l
