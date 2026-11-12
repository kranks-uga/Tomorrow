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

    # rax → Context нового процесса. Дальше — общий путь возврата в ring-3
    # (switch.s): он строит iretq-фрейм из Context и восстанавливает регистры.
    mov rdi, rax
    jmp resume_context

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
