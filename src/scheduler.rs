use crate::process::{self, Process, ProcessState};

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
    pub rip: u64, // адрес следующей инструкции
    pub rflags: u64,
    pub cr3: u64,
    pub kernel_stack: u64,
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

impl Scheduler {
    pub fn new() -> Self {
        Scheduler {
            processes: [const { None }; 64],
            current: 0,
            count: 0,
        }
    }

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

    pub fn schedule(&mut self) -> Option<&mut Process> {
        let mut found_idx = None;

        for i in 0..64 {
            let idx = (self.current + 1 + i) % 64;
            if let Some(proc) = &self.processes[idx] {
                if proc.state == ProcessState::Running {
                    found_idx = Some(idx);
                    break;
                }
            }
        }

        if let Some(idx) = found_idx {
            self.current = idx;
            return self.processes[idx].as_mut();
        }

        None
    }
}

pub unsafe fn yield_now() {
    let current = SCHEDULER.current;

    for i in 1..64 {
        let idx = (current + i) % 64;
        if let Some(p) = &SCHEDULER.processes[idx] {
            if p.state == ProcessState::Running {
                let old =
                    &mut SCHEDULER.processes[current].as_mut().unwrap().context as *mut Context;
                let new = &SCHEDULER.processes[idx].as_ref().unwrap().context as *const Context;
                SCHEDULER.current = idx;
                context_switch(old, new);
                return;
            }
        }
    }
}

pub unsafe fn start_first_process() -> ! {
    let ctx = &SCHEDULER.processes[0].as_ref().unwrap().context as *const Context;
    core::arch::asm!(
        "mov rsp, [{0} + 0x38]",
        "jmp [{0} + 0x80]",
        in(reg) ctx,
        options(noreturn)
    );
}

extern "C" {
    pub fn context_switch(old: *mut Context, new: *const Context);
}
