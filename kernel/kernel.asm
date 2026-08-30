[bits 64]
global _start
extern kernel_main

section .text
_start:
    mov rsp, stack_top
    call kernel_main

.hang:
    hlt
    jmp .hang

section .bss
stack_bottom:
    resb 16384
stack_top: