// Эту функцию добавь в main.rs вместо timer_tick_switch

// Структура сохранённых регистров на стеке — должна совпадать с порядком push в timer.s
#[repr(C)]
struct SavedRegs {
    rax: u64, rbx: u64, rcx: u64, rdx: u64,
    rbp: u64, rsi: u64, rdi: u64,
    r8: u64,  r9: u64,  r10: u64, r11: u64,
    r12: u64, r13: u64, r14: u64, r15: u64,
}

#[no_mangle]
pub unsafe extern "C" fn timer_do_switch(regs: *mut SavedRegs) -> *const scheduler::Context {
    TICKS += 1;
    lapic::eoi(LAPIC_BASE);

    if !SCHEDULER_READY || TICKS % 50 != 0 {
        return core::ptr::null();
    }

    // сохраняем контекст текущего процесса
    let current = scheduler::SCHEDULER.current;
    if let Some(proc) = scheduler::SCHEDULER.processes[current].as_mut() {
        proc.context.rax = (*regs).rax;
        proc.context.rbx = (*regs).rbx;
        proc.context.rcx = (*regs).rcx;
        proc.context.rdx = (*regs).rdx;
        proc.context.rbp = (*regs).rbp;
        proc.context.rsi = (*regs).rsi;
        proc.context.rdi = (*regs).rdi;
        proc.context.r8  = (*regs).r8;
        proc.context.r9  = (*regs).r9;
        proc.context.r10 = (*regs).r10;
        proc.context.r11 = (*regs).r11;
        proc.context.r12 = (*regs).r12;
        proc.context.r13 = (*regs).r13;
        proc.context.r14 = (*regs).r14;
        proc.context.r15 = (*regs).r15;
        // rip/rsp/rflags берём из iretq фрейма который CPU положил выше наших push'ей
        // наши push'и: 15 регистров * 8 = 120 байт
        let iretq_frame = (regs as u64 + 120) as *const u64;
        proc.context.rip    = *iretq_frame;
        proc.context.rsp    = *iretq_frame.add(3);
        proc.context.rflags = *iretq_frame.add(2);
    }

    // выбираем следующий процесс
    for i in 0..64 {
        let idx = (current + 1 + i) % 64;
        if let Some(p) = &scheduler::SCHEDULER.processes[idx] {
            if p.state == process::ProcessState::Running {
                scheduler::SCHEDULER.current = idx;
                tss::TSS.rsp0 = p.kernel_stack;
                return &p.context as *const _;
            }
        }
    }

    core::ptr::null()
}
