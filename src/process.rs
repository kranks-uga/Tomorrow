use crate::pmm;

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
}

impl Process {
    pub fn new(pid: u64, syscall_mask: u64, domain: u8) -> Self {
        Process {
            pid, // сокращение от pid: pid
            syscall_mask,
            domain,
            token: 0,
            state: ProcessState::Running,
            kernel_stack: unsafe { pmm::alloc() },
        }
    }
}
