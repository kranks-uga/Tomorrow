.intel_syntax noprefix
.global keyboard_handler_asm

keyboard_handler_asm:
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rbp
    push rdx
    push rcx
    push rbx
    push rax

    call keyboard_irq_handler

    pop rax
    pop rbx
    pop rcx
    pop rdx
    pop rbp
    pop rsi
    pop rdi
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15

    # если возвращаемся в ring-3 — фиксим SS.RPL (как в таймере)
    test WORD PTR [rsp + 8], 3
    jz 1f
    or WORD PTR [rsp + 32], 3
1:
    iretq