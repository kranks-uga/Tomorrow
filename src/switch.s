.intel_syntax noprefix
.global resume_context

# resume_context(ctx: *const Context) -> !
#
# Единственный путь возврата в ring-3: собрать iretq-фрейм из Context и
# прыгнуть в user space. Вызывается из трёх мест:
#   - start_first_process_ring3 — первый запуск процесса;
#   - syscall_reschedule        — yield внутри syscall;
#   - timer_handler_asm         — истёк квант, переключаемся на другой процесс.
#
# Раньше путей было два: таймер строил iretq-фрейм, а context_switch
# возвращался через ret по ядерному адресу. Смешать их — значит прыгнуть
# на user-адрес в ring 0 (или на ядерный адрес в ring 3). Теперь все
# переключения заканчиваются здесь.
resume_context:
    # rdi = *const Context
    mov rbx, rdi

    # Стек процесса: вершина его kernel-стека (TSS.rsp0). Старый стек
    # вызывающего (syscall-стек или прерванный kernel-стек) больше не нужен.
    mov rsp, [rbx + 0xA8]

    # iretq-фрейм, снизу вверх: SS, RSP, RFLAGS, CS, RIP.
    push qword ptr [rbx + 0x98]     # SS
    push qword ptr [rbx + 0x38]     # RSP
    push qword ptr [rbx + 0x88]     # RFLAGS
    push qword ptr [rbx + 0x90]     # CS
    push qword ptr [rbx + 0x80]     # RIP

    # Восстанавливаем регистры; rbx последним, потому что он — база.
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
    mov rbx, [rbx + 0x08]

    iretq
