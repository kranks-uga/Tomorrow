use crate::scheduler::Context;
use crate::vmm::PAGE_USER;
use crate::vmm::PAGE_WRITABLE;
use crate::{pml4, pmm};

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
    pub user_stack: u64,
    pub context: Context,
}

impl Process {
    pub fn new(pid: u64, syscall_mask: u64, domain: u8, entry: u64) -> Self {
        let stack = unsafe { pmm::alloc() };
        let user_stack_phys = unsafe { pmm::alloc() };

        const KERNEL_VIRT: u64 = 0xFFFF800000000000;
        let stack_virt = stack + KERNEL_VIRT;
        let stack_top = (stack_virt + 4096) & !0xF;

        // кладём entry point на вершину kernel stack
        unsafe {
            let stack_ptr = (stack_top - 8) as *mut u64;
            *stack_ptr = entry;
        }

        // маппим user stack с флагом USER
        // user stack — за пределами identity-map (0-1TB покрыта large pages)
        // pml4[2] пустой → vmm::map создаст нормальные 4KB таблицы
        let user_stack_virt: u64 = 0x0000_0100_0000_0000;

        let fn_phys = (entry - KERNEL_VIRT) & !0xFFF;
        let fn_offset = entry & 0xFFF;
        // 0x400000 попадает в 1GB large page boot-таблицы (pml4[0]),
        // vmm не может создать 4KB внутри неё — берём pml4[4] (свободен)
        let user_code_virt: u64 = 0x0000_0200_0000_0000;

        unsafe {
            extern "C" {
                static pml4: crate::vmm::PageTable;
            }
            let pml4_ptr = &pml4 as *const _ as *mut crate::vmm::PageTable;
            (*pml4_ptr).map(
                user_stack_virt,
                user_stack_phys,
                crate::vmm::PAGE_WRITABLE | crate::vmm::PAGE_USER,
            );

            (*pml4_ptr).map(user_code_virt, fn_phys, PAGE_WRITABLE | PAGE_USER);
        }

        // user_stack_top теперь указывает на верхушку этой страницы
        let user_stack_top = user_stack_virt + 4096;

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
            user_stack: user_stack_top,
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
                rip: user_code_virt + fn_offset,
                rflags: 0x202,
                cs: 0x23, // user code  (DPL=3)
                ss: 0x1B, // user data  (DPL=3)
                rsp: user_stack_top,
                cr3,
                kernel_stack: stack_top,
            },
        }
    }
}
