.intel_syntax noprefix
.global syscall_entry

# Временное хранение RSP пользователя (не SMP-safe — пока норм)
.bss
.align 8
user_rsp_tmp: .quad 0

.text
syscall_entry:
    # На входе (от ring-3 через SYSCALL):
    #   RCX = адрес возврата (user RIP)
    #   R11 = user RFLAGS
    #   RSP = user stack
    #   IF  = 0 (сброшен через SYSCALL_MASK)

    # 1. Сохраняем user RSP и переключаемся на ядерный стек
    mov [rip + user_rsp_tmp], rsp
    mov rsp, [rip + SYSCALL_KERNEL_RSP]

    # 2. Кладём на стек то, что нужно для SYSRETQ (в порядке pop)
    push qword ptr [rip + user_rsp_tmp]  # user RSP
    push r11                              # user RFLAGS
    push rcx                              # user RIP

    # 3. Сохраняем callee-saved регистры (ABI C)
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    # 4. Перекладываем аргументы: Linux syscall ABI → C calling convention
    #   Linux: rax=nr, rdi=arg1, rsi=arg2, rdx=arg3, r10=arg4, r8=arg5
    #   C ABI: rdi=nr, rsi=arg1, rdx=arg2, rcx=arg3, r8=arg4,  r9=arg5
    mov r9,  r8        # arg5 (до перезаписи r8)
    mov r8,  r10       # arg4
    mov rcx, rdx       # arg3 (до перезаписи rdx)
    mov rdx, rsi       # arg2
    mov rsi, rdi       # arg1
    mov rdi, rax       # nr

    call syscall_handler
    # rax = возвращаемое значение (для пользователя)

    # 5. Восстанавливаем callee-saved
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx

    # 6. Восстанавливаем контекст для SYSRETQ
    pop rcx            # user RIP → RCX
    pop r11            # user RFLAGS → R11
    pop rsp            # user RSP (pop rsp корректен на x86-64)

    # 7. Возврат в ring-3
    sysretq
