use crate::pmm;
use crate::scheduler::Context;

#[derive(PartialEq)]
pub enum ProcessState {
    Running,
    Blocked,
    Dead,
}

pub struct Process {
    pub pid: u64,
    pub syscall_mask: u64,
    pub domain: u8,
    pub token: u64,
    pub state: ProcessState,
    pub kernel_stack: u64,
    pub context: Context,
}

impl Process {
    pub fn new(pid: u64, syscall_mask: u64, domain: u8, entry: u64) -> Self {
        let stack = unsafe { pmm::alloc() };

        const KERNEL_VIRT: u64 = 0xFFFF800000000000;
        let stack_virt = stack + KERNEL_VIRT;
        let stack_top = stack_virt + 4096;

        // кладём entry point на вершину стека
        // так что первый ret прыгнет на entry
        unsafe {
            let stack_ptr = (stack_top - 8) as *mut u64;
            *stack_ptr = entry;
        }

        // читаем текущий CR3
        let cr3: u64;
        unsafe {
            core::arch::asm!("mov {}, cr3", out(reg) cr3);
        }

        Process {
            pid,
            syscall_mask,
            domain,
            token: 0,
            state: ProcessState::Running,
            kernel_stack: stack_top,
            context: Context {
                rax: 0,
                rbx: 0,
                rcx: 0,
                rdx: 0,
                rsi: 0,
                rdi: 0,
                rbp: 0,
                r8: 0,
                r9: 0,
                r10: 0,
                r11: 0,
                r12: 0,
                r13: 0,
                r14: 0,
                r15: 0,
                rip: entry,
                rflags: 0x202,
                rsp: stack_top - 8, // указывает на entry
                cr3,
                kernel_stack: stack_top,
            },
        }
    }
}
