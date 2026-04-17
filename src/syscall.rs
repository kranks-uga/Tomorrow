use crate::{pmm, scheduler};

// Linux x86-64 syscall numbers
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_EXIT: u64 = 60;
pub const SYS_YIELD: u64 = 24;

/// Вершина ядерного стека для syscall-обработчика.
/// Экспортируется в syscall_entry.s через #[no_mangle].
#[no_mangle]
pub static mut SYSCALL_KERNEL_RSP: u64 = 0;

const MSR_STAR: u32 = 0xC0000081;
const MSR_LSTAR: u32 = 0xC0000082;
const MSR_SYSCALL_MASK: u32 = 0xC0000084;

unsafe fn write_msr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") low,
        in("edx") high,
    );
}

extern "C" {
    fn syscall_entry();
}

pub fn init() {
    unsafe {
        // Выделяем отдельную страницу для ядерного стека syscall-обработчика
        const KERNEL_VIRT: u64 = 0xFFFF800000000000;
        let page = pmm::alloc();
        SYSCALL_KERNEL_RSP = page + KERNEL_VIRT + 4096; // вершина страницы

        // Включаем SCE бит в EFER
        core::arch::asm!(
            "mov ecx, 0xC0000080",
            "rdmsr",
            "or eax, 1",
            "wrmsr",
            out("eax") _,
            out("edx") _,
            out("ecx") _,
        );

        // LSTAR — адрес обработчика
        write_msr(MSR_LSTAR, syscall_entry as *const () as u64);

        // STAR: [47:32]=0x0008 kernel CS, [63:48]=0x0018 user SS base
        write_msr(MSR_STAR, 0x0010_0008_0000_0000);

        // SYSCALL_MASK — сбрасывает IF при входе
        write_msr(MSR_SYSCALL_MASK, 0x200);
    }
}

#[no_mangle]
pub unsafe extern "C" fn syscall_handler(
    nr: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
) -> u64 {
    match nr {
        SYS_WRITE => sys_write(arg1, arg2 as *const u8, arg3),
        SYS_EXIT => sys_exit(arg1),
        SYS_YIELD => sys_yield(),
        _ => u64::MAX,
    }
}

unsafe fn sys_write(fd: u64, buf: *const u8, len: u64) -> u64 {
    if fd == 1 {
        for i in 0..len {
            let byte = *buf.add(i as usize);
            (&raw mut crate::CONSOLE)
                .as_mut()
                .unwrap()
                .as_mut()
                .unwrap()
                .write_byte(byte);
        }
        return len;
    }
    u64::MAX
}

unsafe fn sys_exit(_code: u64) -> ! {
    loop {}
}

unsafe fn sys_yield() -> u64 {
    scheduler::yield_now();
    0
}
