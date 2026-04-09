.intel_syntax noprefix
.global timer_handler_asm

timer_handler_asm:
    # CPU положил на стек: rip, cs, rflags, rsp, ss
    # сохраняем все регистры
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

    mov rdi, rsp
    call timer_do_switch

    test rax, rax
    jz .restore

    # rax = указатель на новый Context
    mov rbx, rax

    # загружаем регистры нового процесса
    mov rax, [rbx + 0x00]
    mov rcx, [rbx + 0x10]
    mov rdx, [rbx + 0x18]
    mov rsi, [rbx + 0x20]
    mov rdi, [rbx + 0x28]
    mov rbp, [rbx + 0x30]
    mov r8,  [rbx + 0x40]
    mov r9,  [rbx + 0x48]
    mov r10, [rbx + 0x50]
    mov r11, [rbx + 0x58]
    mov r12, [rbx + 0x60]
    mov r13, [rbx + 0x68]
    mov r14, [rbx + 0x70]
    mov r15, [rbx + 0x78]

    # переключаем стек на kernel stack нового процесса
    mov rsp, [rbx + 0x98]

    # кладём rip на стек и прыгаем через ret
    mov rcx, [rbx + 0x80]
    push rcx

    # загружаем rcx и rbx последними
    mov rcx, [rbx + 0x10]
    mov rbx, [rbx + 0x08]

    ret

.restore:
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
    iretq