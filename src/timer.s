.intel_syntax noprefix
.global timer_handler_asm

timer_handler_asm:
    # CPU уже положил на стек: rip, cs, rflags, rsp, ss
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

    # передаём rsp как аргумент — указатель на сохранённые регистры
    mov rdi, rsp
    call timer_do_switch

    # rax = 0 если переключение не нужно
    # rax = указатель на новый Context если нужно
    test rax, rax
    jz .restore

    # переключаемся на новый процесс
    # rax = *const Context
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

    # строим iretq фрейм на kernel stack нового процесса
    mov rsp, [rbx + 0x98]

    # iretq фрейм снизу вверх: rip, cs, rflags, rsp, ss
    xor rcx, rcx
    mov cx, 0x10
    push rcx                    # ss

    mov rcx, [rbx + 0x38]
    push rcx                    # rsp нового процесса

    mov rcx, [rbx + 0x88]
    push rcx                    # rflags

    xor rcx, rcx
    mov cx, 0x08
    push rcx                    # cs

    mov rcx, [rbx + 0x80]
    push rcx                    # rip

    # загружаем rbx и rcx последними
    mov rcx, [rbx + 0x10]
    mov rbx, [rbx + 0x08]

    iretq

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
