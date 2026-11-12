// SPDX-License-Identifier: GPL-3.0-or-later
use crate::pmm;
use crate::process::{Process, ProcessState};

#[repr(C)]
pub struct Context {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,          // 0x80
    pub rflags: u64,       // 0x88
    pub cs: u64,           // 0x90
    pub ss: u64,           // 0x98
    pub cr3: u64,          // 0xA0
    pub kernel_stack: u64, // 0xA8
}

pub struct Scheduler {
    pub processes: [Option<Process>; 64], // максимум 64 процесса
    pub current: usize,                   // индекс текущего процесса
    pub count: usize,                     // сколько процессов всего
}

pub static mut SCHEDULER: Scheduler = Scheduler {
    processes: [const { None }; 64],
    current: 0,
    count: 0,
};

// Монотонный счётчик pid. Стартовые процессы заняли 1 и 2, поэтому начинаем с 3.
// Только растёт и не переиспользует значения: виртуальные адреса user-стека и
// кода в Process::new зависят от pid (стек + pid*4K, код + pid*CODE_PAGES*4K), reap не размаппивает их —
// повторный pid мог бы наложиться на ещё живущий маппинг.
static mut NEXT_PID: u64 = 3;

/// Выдать следующий свободный pid.
pub unsafe fn next_pid() -> u64 {
    let pid = NEXT_PID;
    NEXT_PID += 1;
    pid
}

impl Scheduler {
    pub fn add_process(&mut self, process: Process) {
        for i in 0..64 {
            if self.processes[i].is_none() {
                self.processes[i] = Some(process);
                self.count += 1;
                return;
            }
        }
        panic!("scheduler: too many processes");
    }

    /// Индекс следующего `Running`-процесса после текущего (round-robin).
    /// Общая логика для `syscall_reschedule` и `timer_do_switch`: раньше
    /// каждый переключатель реализовывал этот обход отдельно, и `yield_now`
    /// забывал обновить `TSS.rsp0` после переключения.
    pub(crate) fn next_running(&self) -> Option<usize> {
        for i in 1..64 {
            let idx = (self.current + i) % 64;
            if let Some(proc) = &self.processes[idx] {
                if proc.state == ProcessState::Running {
                    return Some(idx);
                }
            }
        }
        None
    }

    ///Освободить слоты процессов помеченых Dead (кроме текущего)
    pub unsafe fn reap(&mut self) {
        for i in 0..64 {
            if i == self.current {
                continue;
            }

            if let Some(p) = &self.processes[i] {
                // Жнём только мёртвые процессы — живые трогать нельзя!
                if p.state == ProcessState::Dead {
                    // Возвращаем ФИЗИЧЕСКИЕ страницы стеков (базовые адреса от alloc,
                    // НЕ вершины). Кодовую страницу (fn_phys) не трогаем — это код ядра.
                    pmm::free(p.kernel_stack_phys);
                    pmm::free(p.user_stack_phys);
                    // Освобождаем слот в самом планировщике
                    self.processes[i] = None;
                    self.count -= 1;
                }
            }
        }
    }
}

pub unsafe fn start_first_process_ring3() -> ! {
    let proc = SCHEDULER.processes[0].as_ref().unwrap();
    (&raw mut crate::tss::TSS).as_mut().unwrap().rsp0 = proc.kernel_stack;

    // Тот же путь, что у таймера и yield: никакого отдельного iretq-фрейма.
    resume_context(&proc.context as *const _);
}

extern "C" {
    /// Единственный путь возврата в ring-3: собрать iretq-фрейм из Context
    /// и прыгнуть в user space (см. switch.s).
    pub fn resume_context(ctx: *const Context) -> !;
}
