[bits 64]
global _start
extern kernel_main        ; объявляем C-функцию из kernel.c

section .text
_start:
    lea rsp, [rel stack_top]  ; настраиваем стек (RIP-relative — не зависит от адреса линковки)
    call kernel_main      ; передаем управление в C!

.hang:                    ; страховка, если kernel_main вернется
    hlt
    jmp .hang

section .bss
stack_bottom:
    resb 16384
stack_top: