.intel_syntax noprefix
.global context_switch
context_switch:
    # rdi = old: *mut Context
    # rsi = new: *const Context
    
    # сохраняем old
    mov [rdi + 0x00], rax
    mov [rdi + 0x08], rbx
    mov [rdi + 0x10], rcx
    mov [rdi + 0x18], rdx
    mov [rdi + 0x20], rsi  # осторожно — rsi это указатель на new!
    mov [rdi + 0x28], rdi  # и rdi это указатель на old!
    mov [rdi + 0x30], rbp
    mov [rdi + 0x38], rsp
    mov [rdi + 0x40], r8
    mov [rdi + 0x48], r9
    mov [rdi + 0x50], r10
    mov [rdi + 0x58], r11
    mov [rdi + 0x60], r12
    mov [rdi + 0x68], r13
    mov [rdi + 0x70], r14
    mov [rdi + 0x78], r15
    
    # переключаем CR3
    mov rax, [rsi + 0x90]
    mov cr3, rax
    
    # загружаем new — порядок важен!
    mov rbx, [rsi + 0x08]
    mov rcx, [rsi + 0x10]
    mov rdx, [rsi + 0x18]
    mov rbp, [rsi + 0x30]
    mov r8,  [rsi + 0x40]
    mov r9,  [rsi + 0x48]
    mov r10, [rsi + 0x50]
    mov r11, [rsi + 0x58]
    mov r12, [rsi + 0x60]
    mov r13, [rsi + 0x68]
    mov r14, [rsi + 0x70]
    mov r15, [rsi + 0x78]
    mov rsp, [rsi + 0x38]
    
    # rip кладём на новый стек
    push [rsi + 0x80]
    
    # теперь загружаем rdi, rsi, rax — последними
    mov rdi, [rsi + 0x28]
    mov rax, [rsi + 0x00]
    mov rsi, [rsi + 0x20]  # rsi последним — теряем указатель на new
    
    # прыгаем через ret — rip уже на стеке
    ret