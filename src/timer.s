.intel_syntax noprefix
.global timer_handler_asm

timer_handler_asm:
    # CPU в 64-бит всегда кладёт: rip, cs, rflags, rsp, ss
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
    push rax        # итого 15 * 8 = 120 байт, выше — iretq-фрейм

    mov rdi, rsp
    call timer_do_switch

    test rax, rax
    jz .restore

    # rax → Context нового процесса
    mov rbx, rax

    # переходим на kernel_stack нового процесса
    mov rsp, [rbx + 0xA8]

    # строим iretq-фрейм (CPU ожидает снизу вверх: ss, rsp, rflags, cs, rip)
    mov rax, [rbx + 0x98]
    push rax                        # SS  из контекста
    mov rax, [rbx + 0x38]
    push rax                        # RSP нового процесса
    mov rax, [rbx + 0x88]
    push rax                        # RFLAGS
    mov rax, [rbx + 0x90]
    push rax                        # CS  из контекста
    mov rax, [rbx + 0x80]
    push rax                        # RIP

    # восстанавливаем регистры
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
      # CS находится на [rsp+8], SS на [rsp+32]
      # если возвращаемся в ring-3 (CS.RPL != 0) — фиксим SS.RPL                                                                                                                             
      test WORD PTR [rsp + 8], 3                                                                                                                                                             
      jz 1f                                                                                                                                                                                  
      or WORD PTR [rsp + 32], 3                                                                                                                                                              
  1:                                                                                                                                                                                         
      iretq
