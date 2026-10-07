// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{pmm, process::Process, scheduler};

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

// Номера ошибок
const EFAULT: u64 = 14;
const ENOENT: u64 = 2;
const ESRCH: u64 = 3;
const EINVAL: u64 = 22;
const EMFILE: u64 = 24;
const ENAMETOOLONG: u64 = 36;

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
        // Выделяем отдельную страницу для ядерного стека syscall-обработчика.
        // Адресуем через identity map (phys==virt), а НЕ через higher-half:
        // higher-half покрывает лишь первые 2 MB → стек за этой границей даёт #PF/#DF.
        let page = pmm::alloc();
        SYSCALL_KERNEL_RSP = page + 4096; // вершина страницы

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

        // STAR: [47:32]=0x0008 — SYSCALL грузит kernel CS=0x08, SS=0x10.
        // [63:48]=0x0010 — на SYSRET контроллер берёт user CS=base+16=0x20 (|RPL3=0x23),
        // user SS=base+8=0x18 (|RPL3=0x1B). Раскладка GDT: 0x08 kCS, 0x10 kSS,
        // 0x18 uSS, 0x20 uCS.
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
    _arg4: u64,
    _arg5: u64,
) -> u64 {
    if !syscall_allowed(nr) {
        return u64::MAX;
    }
    match nr {
        SYS_WRITE => sys_write(arg1, arg2 as *const u8, arg3),
        SYS_EXIT => sys_exit(arg1),
        SYS_YIELD => sys_yield(),
        _ => u64::MAX,
    }
}

fn user_range_ok(p: &Process, ptr: u64, len: u64) -> bool {
    let Some(ptr_end) = ptr.checked_add(len) else {
        return false;
    };

    for &(start, end) in p.user_regions.iter() {
        if start <= ptr && ptr_end <= end {
            return true;
        }
    }
    false
}

/// Проверяет `Process::syscall_mask` текущего процесса — бит `nr` должен быть
/// установлен, иначе syscall запрещён. Маска заполняется при создании
/// процесса (см. process.rs), но раньше ничем не проверялась.
unsafe fn syscall_allowed(nr: u64) -> bool {
    match &scheduler::SCHEDULER.processes[scheduler::SCHEDULER.current] {
        Some(p) if nr < 64 => (p.syscall_mask >> nr) & 1 != 0,
        _ => false,
    }
}

unsafe fn sys_write(fd: u64, buf: *const u8, len: u64) -> u64 {
    if fd == 1 {
        // Вывод идёт через шелл, а не напрямую в консоль: иначе он вклинивается
        // в набираемую строку ввода (см. shell::program_output).
        let slice = core::slice::from_raw_parts(buf, len as usize);
        crate::shell::program_output(slice);
        return len;
    }
    u64::MAX
}

fn err(e: u64) -> u64 {
    (-(e as i64)) as u64
}

unsafe fn sys_open(path_ptr: u64, path_len: u64) -> u64 {
    let Some(p) = scheduler::SCHEDULER.processes[scheduler::SCHEDULER.current].as_mut() else {
        return err(ESRCH);
    };
    if path_len == 0 {
        return err(EINVAL);
    }
    if path_len >= crate::ramfs::MAX_PATH as u64 {
        return err(ENAMETOOLONG);
    }

    if !user_range_ok(p, path_ptr, path_len) {
        return err(EFAULT);
    }

    let src = core::slice::from_raw_parts(path_ptr as *const u8, path_len as usize);
    let mut name_buf = [0u8; crate::ramfs::MAX_PATH];
    name_buf[..src.len()].copy_from_slice(src);
    let name = &name_buf[..src.len()];
}

unsafe fn sys_exit(_code: u64) -> ! {
    loop {}
}

unsafe fn sys_yield() -> u64 {
    scheduler::yield_now();
    0
}
