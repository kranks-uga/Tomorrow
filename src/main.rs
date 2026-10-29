// SPDX-License-Identifier: GPL-3.0-or-later
//author: Kirill Repin

#![no_std]
#![no_main]

extern crate alloc;

use crate::console::Console;
use alloc::vec::Vec;
use core::ptr::{addr_of, read_unaligned};

mod boot;
mod console;
mod font;
mod heap;
mod hpet;
mod idt;
mod ioapic;
mod keyboard;
mod lapic;
mod pci;
mod pic;
mod pmm;
mod process;
mod ramfs;
mod scheduler;
mod serial;
mod shell;
mod syscall;
mod tss;
mod vmm;
mod xhci;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // В debug-сборке компилятор вставляет скрытые panic-ветки (unwrap, проверки
    // переполнения, индексация). Молчащий loop {} превращал любую такую панику
    // в «зависание на последней печатной строке». Печатаем место паники, чтобы
    // видеть file:line прямо на экране.
    unsafe {
        if let Some(console) = (&raw mut CONSOLE).as_mut().and_then(|c| c.as_mut()) {
            console.write_str("\nPANIC");
            if let Some(loc) = info.location() {
                console.write_str(" at ");
                console.write_str(loc.file());
                console.write_byte(b':');
                console.write_dec(loc.line() as u64);
            }
            console.write_byte(b'\n');
        }
    }
    // Дублируем в UART: если консоль залипла или экран не виден, паника
    // всё равно останется в serial-логе. try_* — паника не должна встать
    // в дедлок на локе, который сама же и держала.
    match info.location() {
        Some(loc) => {
            crate::serial::try_print(format_args!("\nPANIC at {}:{}\n", loc.file(), loc.line()));
        }
        None => {
            crate::serial::try_print(format_args!("\nPANIC\n"));
        }
    }
    loop {}
}

#[repr(C)]
struct Mbtag {
    typ: u32,
    size: u32,
}

#[repr(C)]
struct MbFramebuffer {
    typ: u32,
    size: u32,
    addr: u64,
    pitch: u32,
    width: u32,
    height: u32,
    bpp: u8,
    fb_type: u8,
    reserved: u16,
}

#[repr(C)]
struct MbModule {
    typ: u32,
    size: u32,
    mod_start: u32,
    mod_end: u32,
}

#[repr(C, packed)]
struct Rsdp {
    signature: [u8; 8],
    checksum: u8,
    oemid: [u8; 6],
    revision: u8,
    rsdt_address: u32,
    length: u32,
    xsdt_address: u64,
    extended_checksum: u8,
    reserved: [u8; 3],
}

#[repr(C, packed)]
struct AcpiHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oemid: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

#[repr(C, packed)]
struct Madt {
    local_apic_address: u32,
    flags: u32,
}

#[repr(C, packed)]
struct MadtLocalApic {
    acpi_processor_id: u8,
    apic_id: u8,
    flags: u32,
}

#[repr(C, packed)]
struct MadtIoApic {
    io_apic_id: u8,
    reserved: u8,
    io_apic_address: u32,
    global_irq_base: u32,
}

#[repr(C, packed)]
struct MadtIso {
    bus: u8,
    source: u8,
    global_system_interrupt: u32,
    flags: u16,
}

static mut CONSOLE: Option<Console> = None;

#[macro_export]
macro_rules! kprint {
    ($s:expr) => {
        unsafe {
            (&raw mut CONSOLE)
                .as_mut()
                .unwrap()
                .as_mut()
                .unwrap()
                .write_str($s);
        }
    };
}

/// Boot-сообщение сразу в две стороны: на экран и в serial. Только для
/// загрузки — в обработчике прерываний serial-лок брать нельзя.
#[macro_export]
macro_rules! bootlog {
    ($s:expr) => {{
        $crate::kprint!($s);
        $crate::sprint!($s);
    }};
}

#[macro_export]
macro_rules! write_hex {
    ($v:expr) => {
        unsafe {
            (&raw mut CONSOLE)
                .as_mut()
                .unwrap()
                .as_mut()
                .unwrap()
                .write_hex($v);
        }
    };
}

static mut IOAPIC_BASE: u64 = 0;
pub static mut LAPIC_BASE: u64 = 0;
static mut TICKS: u64 = 0;
static mut TIMER_GSI: u8 = 0;
static mut SCHEDULER_READY: bool = false;
// Сторож GDT[3]: последнее замеченное значение дескриптора user-data.
// Печатаем только при ИЗМЕНЕНИИ — увидим эталон 0x00CFF2000000FFFF один раз
// и затем точный тик, на котором его кто-то затирает (→ #GP 0x18).
static mut GDT3_LAST: u64 = 0;

pub static mut PM1A_CNT: u16 = 0;
pub static mut PM1B_CNT: u16 = 0;
pub static mut DSDT_ADDR: u64 = 0;
// SLP_TYPx из \_S5 в DSDT — значение режима сна для записи в PM1x_CNT.
pub static mut SLP_TYPA: u8 = 0;
pub static mut SLP_TYPB: u8 = 0;

pub static mut MOD_START: u64 = 0;
pub static mut MOD_END: u64 = 0;

#[repr(C)]
struct SavedRegs {
    rax: u64,
    rbx: u64,
    rcx: u64,
    rdx: u64,
    rbp: u64,
    rsi: u64,
    rdi: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
}

#[no_mangle]
unsafe extern "C" fn timer_do_switch(regs: *mut SavedRegs) -> *const scheduler::Context {
    TICKS += 1;
    lapic::eoi(LAPIC_BASE);

    // Опрос USB HID-клавиатуры. Внутри стоит guard HID_READY — если xHCI
    // не инициализирован, вызов мгновенно возвращается.
    xhci::poll_hid();

    // Сторож GDT[3]: проверяем КАЖДЫЙ тик (в т.ч. сразу после poll_hid),
    // печатаем только когда значение поменялось.
    let g3 = tss::gdt_user_data();
    if g3 != GDT3_LAST {
        GDT3_LAST = g3;
        kprint!("GDT3=");
        write_hex!(g3);
        kprint!(" t=");
        write_hex!(TICKS);
        kprint!("\n");
    }

    if !SCHEDULER_READY || TICKS % 5 != 0 {
        return core::ptr::null();
    }

    let current = scheduler::SCHEDULER.current;

    if let Some(proc) = scheduler::SCHEDULER.processes[current].as_mut() {
        proc.context.rax = (*regs).rax;
        proc.context.rbx = (*regs).rbx;
        proc.context.rcx = (*regs).rcx;
        proc.context.rdx = (*regs).rdx;
        proc.context.rbp = (*regs).rbp;
        proc.context.rsi = (*regs).rsi;
        proc.context.rdi = (*regs).rdi;
        proc.context.r8 = (*regs).r8;
        proc.context.r9 = (*regs).r9;
        proc.context.r10 = (*regs).r10;
        proc.context.r11 = (*regs).r11;
        proc.context.r12 = (*regs).r12;
        proc.context.r13 = (*regs).r13;
        proc.context.r14 = (*regs).r14;
        proc.context.r15 = (*regs).r15;
        let iretq = (regs as u64 + 120) as *const u64;
        proc.context.rip = *iretq.add(0);
        proc.context.cs = *iretq.add(1);
        proc.context.rflags = *iretq.add(2);
        proc.context.rsp = *iretq.add(3);
        proc.context.ss = *iretq.add(4);
    }

    if let Some(idx) = (*(&raw const scheduler::SCHEDULER)).next_running() {
        scheduler::SCHEDULER.current = idx;
        let p = scheduler::SCHEDULER.processes[idx].as_ref().unwrap();
        (&raw mut tss::TSS).as_mut().unwrap().rsp0 = p.kernel_stack;
        return &p.context as *const _;
    }

    core::ptr::null()
}

extern "C" {
    static pml4: vmm::PageTable;
}

#[unsafe(naked)]
pub extern "C" fn process_a() -> ! {
    core::arch::naked_asm!(
        "lea rsi, [rip + 1f]",
        "mov rax, 1",
        "mov rdi, 1",
        "mov rdx, 3",
        "syscall",
        "0: jmp 0b",
        "1: .byte 0x55, 0x33, 0x0A",
    )
}

#[unsafe(naked)]
pub extern "C" fn process_b() -> ! {
    core::arch::naked_asm!(
        "lea rsi, [rip + 1f]",
        "mov rax, 1",
        "mov rdi, 1",
        "mov rdx, 2",
        "syscall",
        "0: jmp 0b",
        "1: .byte 0x42, 0x0A",
    )
}

#[unsafe(naked)]
pub extern "C" fn process_f() -> ! {
    core::arch::naked_asm!(
        "mov rax, 2",
        "lea rdi, [rip + 1f]",
        "mov rsi, 9",
        "xor rdx, rdx", // O_RDONLY
        "syscall",
        "mov r12, rax",
        "sub rsp, 64",
        "3:",
        "mov rax, 0",
        "mov rdi, r12",
        "mov rsi, rsp",
        "mov rdx, 16",
        "syscall",
        "test rax, rax",
        "jz 4f",
        "js 4f",
        "mov rdx, rax",
        "mov rax, 1",
        "mov rdi, 1",
        "mov rsi, rsp",
        "syscall",
        "jmp 3b",
        "4:",
        "mov rax, 3",
        "mov rdi, r12",
        "syscall",
        "mov rax, 60",
        "xor rdi, rdi",
        "syscall",
        "0: jmp 0b",
        "1: .ascii \"hello.txt\"",
    )
}

#[unsafe(naked)]
pub extern "C" fn process_w() -> ! {
    core::arch::naked_asm!(
        // fd = open("hello.txt", 9, O_WRONLY)
        "mov rax, 2",
        "lea rdi, [rip + 1f]",
        "mov rsi, 9",
        "mov rdx, 1", // O_WRONLY
        "syscall",
        "mov r12, rax",
        // write(fd, "XYZ", 3): перезапись первых 3 байт
        "mov rax, 1",
        "mov rdi, r12",
        "lea rsi, [rip + 2f]",
        "mov rdx, 3",
        "syscall",
        // write(fd, "0123456789abcdefghij", 20): с offset 3 до 23 > 18, файл растёт
        "mov rax, 1",
        "mov rdi, r12",
        "lea rsi, [rip + 5f]",
        "mov rdx, 20",
        "syscall",
        // close(fd); exit(0)
        "mov rax, 3",
        "mov rdi, r12",
        "syscall",
        "mov rax, 60",
        "xor rdi, rdi",
        "syscall",
        "0: jmp 0b",
        "1: .ascii \"hello.txt\"",
        "2: .ascii \"XYZ\"",
        "5: .ascii \"0123456789abcdefghij\"",
    )
}

#[unsafe(naked)]
pub extern "C" fn process_x() -> ! {
    core::arch::naked_asm!(
        // Каждый тест печатает "xN ok -E" если syscall вернул ошибку -E, иначе "xN FAIL N".

        // 1. open("hello.txt", O_WRONLY)
        "mov rax, 2",
        "lea rdi, [rip + 1f]",
        "mov rsi, 9",
        "mov rdx, 1", // O_WRONLY
        "syscall",
        "lea r13, [rip + 11f]",
        "call 9f",
        // 2. fd = open("hello.txt", O_RDONLY)
        "mov rax, 2",
        "lea rdi, [rip + 1f]",
        "mov rsi, 9",
        "xor rdx, rdx", // O_RDONLY
        "syscall",
        "mov r12, rax",
        "test rax, rax",
        "js 5f", // open сам упал — write не делаем, тест провален
        "mov rax, 1",
        "mov rdi, r12",
        "lea rsi, [rip + 3f]",
        "mov rdx, 3",
        "syscall",
        "jmp 6f",
        "5:",
        "xor eax, eax", // rax >= 0 → report напишет FAIL
        "6:",
        "lea r13, [rip + 12f]",
        "call 9f",
        "mov rax, 3", // close(fd); на отрицательном fd просто вернёт EBADF
        "mov rdi, r12",
        "syscall",
        // 3. open("readme.txt", O_RDONLY)
        "mov rax, 2",
        "lea rdi, [rip + 2f]",
        "mov rsi, 10",
        "xor rdx, rdx", // O_RDONLY
        "syscall",
        "lea r13, [rip + 13f]",
        "call 9f",
        // exit(0)
        "mov rax, 60",
        "xor rdi, rdi",
        "syscall",
        "0: jmp 0b",
        // report: rax = результат syscall, r13 = префикс "xN " (3 байта).
        // Ожидаем ошибку, т.е. rax < 0.
        "9:",
        "mov r14, rax",
        "mov rax, 1",
        "mov rdi, 1",
        "mov rsi, r13",
        "mov rdx, 3",
        "syscall",
        "test r14, r14",
        "js 7f",
        "lea rsi, [rip + 21f]",
        "mov rdx, 5",
        "jmp 8f",
        "7:",
        "lea rsi, [rip + 20f]",
        "mov rdx, 3",
        "8:",
        "mov rax, 1",
        "mov rdi, 1",
        "syscall",
        // Печатаем сам результат (r14) знаковым десятичным + '\n'.
        // Буфер на стеке заполняется с конца: цифры идут от младшей.
        "sub rsp, 32",
        "lea rsi, [rsp + 32]",
        "dec rsi",
        "mov byte ptr [rsi], 10", // '\n'
        "mov rax, r14",
        "test rax, rax",
        "jns 15f",
        "neg rax",
        "15:",
        "mov rcx, 10",
        "16:",
        "xor edx, edx",
        "div rcx",    // rax = rax / 10, rdx = остаток
        "add dl, 48", // '0' + цифра
        "dec rsi",
        "mov [rsi], dl",
        "test rax, rax",
        "jnz 16b",
        "test r14, r14",
        "jns 17f",
        "dec rsi",
        "mov byte ptr [rsi], 45", // '-'
        "17:",
        "lea rdx, [rsp + 32]",
        "sub rdx, rsi", // длина = конец буфера - начало числа
        "mov rax, 1",
        "mov rdi, 1",
        "syscall",
        "add rsp, 32",
        "ret",
        "1: .ascii \"hello.txt\"",
        "2: .ascii \"readme.txt\"",
        "3: .ascii \"XYZ\"",
        "11: .ascii \"x1 \"",
        "12: .ascii \"x2 \"",
        "13: .ascii \"x3 \"",
        "20: .ascii \"ok \"",
        "21: .ascii \"FAIL \"",
    )
}

#[unsafe(naked)]
pub extern "C" fn process_r() -> ! {
    core::arch::naked_asm!(
        // fd = open("hello.txt", O_RDONLY) — держим файл открытым
        "mov rax, 2",
        "lea rdi, [rip + 1f]",
        "mov rsi, 9",
        "xor rdx, rdx", // O_RDONLY
        "syscall",
        "test rax, rax",
        "js 4f",
        "mov r12, rax",
        "mov rax, 1",
        "mov rdi, 1",
        "lea rsi, [rip + 2f]",
        "mov rdx, 14",
        "syscall",
        // Ждём `rm hello.txt` из шелла: пробуем открыть файл заново,
        // пока open не вернёт ошибку (ENOENT).
        "3:",
        "mov rax, 2",
        "lea rdi, [rip + 1f]",
        "mov rsi, 9",
        "xor rdx, rdx",
        "syscall",
        "test rax, rax",
        "js 5f",
        "mov rdi, rax", // close(fd2), иначе EMFILE через 13 итераций
        "mov rax, 3",
        "syscall",
        "jmp 3b",
        "5:",
        "mov rax, 1",
        "mov rdi, 1",
        "lea rsi, [rip + 6f]",
        "mov rdx, 18",
        "syscall",
        // Файла в каталоге нет, но inode жив, пока на него ссылается наш fd.
        "sub rsp, 64",
        "7:",
        "mov rax, 0",
        "mov rdi, r12",
        "mov rsi, rsp",
        "mov rdx, 16",
        "syscall",
        "test rax, rax",
        "jz 8f",
        "js 8f",
        "mov rdx, rax",
        "mov rax, 1",
        "mov rdi, 1",
        "mov rsi, rsp",
        "syscall",
        "jmp 7b",
        "8:",
        "mov rax, 3",
        "mov rdi, r12",
        "syscall",
        "4:",
        "mov rax, 60",
        "xor rdi, rdi",
        "syscall",
        "0: jmp 0b",
        "1: .ascii \"hello.txt\"",
        "2: .ascii \"opened, rm me\\n\"",
        "6: .ascii \"deleted, reading:\\n\"",
    )
}

#[no_mangle]
pub extern "C" fn kernel_main(boot_info: u64) -> ! {
    // UART поднимаем до всего остального: дальше каждое веховое сообщение
    // дублируется в serial (bootlog!), и падение на любом этапе будет видно.
    serial::init();
    sprintln!("\n--- Tomorrow OS serial log ---");

    let mut xsdt_addr: u64 = 0;
    let mut mmap_addr: u64 = 0;
    let mut mmap_size: u32 = 0;
    let mut mmap_entry_size: u32 = 0;
    let mut mod_start: u64 = 0;
    let mut mod_end: u64 = 0;

    // === Парсим Multiboot2 теги ===
    let mut ptr = (boot_info + 8) as *const Mbtag;
    loop {
        let tag = unsafe { &*(ptr as *const Mbtag) };
        match tag.typ {
            0 => break,
            3 => {
                let m = unsafe { &*(ptr as *const MbModule) };
                mod_start = m.mod_start as u64;
                mod_end = m.mod_end as u64;
            }
            6 => {
                mmap_addr = ptr as u64;
                mmap_size = tag.size;
                mmap_entry_size = unsafe { *((ptr as *const u8).add(8) as *const u32) };
            }
            8 => {
                let fb = unsafe { &*(ptr as *const MbFramebuffer) };
                unsafe {
                    CONSOLE = Some(Console {
                        fb: fb.addr as *mut u32,
                        pitch: fb.pitch,
                        width: fb.width,
                        height: fb.height,
                        cx: 0,
                        cy: 0,
                    });
                    (&raw mut CONSOLE)
                        .as_mut()
                        .unwrap()
                        .as_mut()
                        .unwrap()
                        .clear();
                }
                bootlog!("Tomorrow OS\n");
                idt::init();
                bootlog!("IDT ok\n");
                idt::set_handler(0xFF, idt::spurious_handler as *const () as u64);
                idt::set_handler(0x20, idt::timer_handler_asm as *const () as u64);
                idt::set_handler(0x21, idt::keyboard_handler_asm as *const () as u64);
                idt::set_handler(0x0D, idt::gp_handler_asm as *const () as u64);
                idt::set_handler(0x0E, idt::pf_handler_asm as *const () as u64);
                pic::disable();
                bootlog!("PIC off\n");
            }
            15 => {
                let rsdp = unsafe { &*((ptr as *const u8).add(8) as *const Rsdp) };
                xsdt_addr = unsafe { read_unaligned(addr_of!(rsdp.xsdt_address)) };
            }
            _ => {}
        }
        let aligned = (tag.size as usize + 7) & !7;
        ptr = unsafe { (ptr as *const u8).add(aligned) as *const Mbtag };
    }

    // === PMM ===
    if mmap_addr != 0 {
        unsafe {
            pmm::init(mmap_addr, mmap_size, mmap_entry_size);
            pmm::reserve(mod_start, mod_end);
            MOD_START = mod_start;
            MOD_END = mod_end;
        }
    }
    bootlog!("PMM ok\n");

    // === TSS ===
    let kernel_stack = unsafe { pmm::alloc() + 4096 }; // +4096 — стек растёт вниз
    unsafe {
        tss::init(kernel_stack);
    }
    bootlog!("TSS ok\n");

    kprint!("module: start=");
    write_hex!(mod_start);
    kprint!(" size=");
    crate::write_hex!(mod_end - mod_start); // или write_dec, если есть макрос
    kprint!("\n");
    sprintln!("module: start={:#x} size={:#x}", mod_start, mod_end - mod_start);

    // === HEAP ===
    let heap_start = unsafe { pmm::alloc_pages(256) }; // 256 страниц = 1 MB
    heap::HEAP.init(heap_start, 256 * 4096);
    heap::self_test();
    bootlog!("HEAP ok\n");
    ramfs::init();

    // === VMM ===
    // Identity map из boot.s покрывает всю физическую память — доп. маппинг не нужен
    bootlog!("VMM ok\n");

    // === SYSCALL ===
    syscall::init();
    bootlog!("Syscall ok\n");

    // === ACPI ===
    let mut xhci_bar_phys: Option<u64> = None;

    if xsdt_addr != 0 {
        let header = unsafe { &*(xsdt_addr as *const AcpiHeader) };
        let length = unsafe { read_unaligned(addr_of!(header.length)) };
        let count = (length as usize - 36) / 8;
        let entries_ptr = (xsdt_addr + 36) as *const u64;

        // Проход 1: APIC
        for i in 0..count {
            let entry_addr = unsafe { read_unaligned(entries_ptr.add(i)) };
            let sig = unsafe { &*(entry_addr as *const [u8; 4]) };
            if sig == b"APIC" {
                let lapic_base = parse_madt(entry_addr);
                unsafe {
                    LAPIC_BASE = lapic_base;
                }
                lapic::enable(lapic_base);
                ioapic::redirect(unsafe { IOAPIC_BASE }, unsafe { TIMER_GSI }, 0x20, 0);
                unsafe {
                    core::arch::asm!("sti");
                }
                bootlog!("APIC ok\n");
            }
        }

        // Проход 2: MCFG → xHCI
        for i in 0..count {
            let entry_addr = unsafe { read_unaligned(entries_ptr.add(i)) };
            let sig = unsafe { &*(entry_addr as *const [u8; 4]) };

            if sig == b"MCFG" {
                let mcfg_base =
                    unsafe { core::ptr::read_unaligned((entry_addr + 44) as *const u64) };
                kprint!("MCFG: ");
                write_hex!(mcfg_base);
                kprint!("\n");
                sprintln!("MCFG: {:#x}", mcfg_base);
                unsafe { pci::enumerate(mcfg_base) };
                xhci_bar_phys = unsafe { pci::find_xhci(mcfg_base) };
            }

            if sig == b"FACP" {
                let pm1a_cnt = unsafe { read_unaligned((entry_addr + 64) as *const u32) } as u16;
                let pm1b_cnt = unsafe { read_unaligned((entry_addr + 68) as *const u32) } as u16;
                let dsdt = unsafe { read_unaligned((entry_addr + 40) as *const u32) } as u64;

                kprint!("PM1a_CNT: ");
                write_hex!(pm1a_cnt as u64);
                kprint!("\n");
                sprintln!("PM1a_CNT: {:#x}", pm1a_cnt);

                // сохранить в статики для команды shutdown
                unsafe {
                    PM1A_CNT = pm1a_cnt;
                    PM1B_CNT = pm1b_cnt;
                    DSDT_ADDR = dsdt;
                }

                // SLP_TYPa/b лежат не в FADT, а в \_S5 внутри DSDT (AML).
                if let Some((a, b)) = parse_s5(dsdt) {
                    unsafe {
                        SLP_TYPA = a;
                        SLP_TYPB = b;
                    }
                    kprint!("SLP_TYPa: ");
                    write_hex!(a as u64);
                    kprint!("\n");
                    sprintln!("SLP_TYPa: {:#x} SLP_TYPb: {:#x}", a, b);
                } else {
                    bootlog!("_S5 not found\n");
                }
            }

            for b in sig {
                unsafe {
                    (&raw mut CONSOLE)
                        .as_mut()
                        .unwrap()
                        .as_mut()
                        .unwrap()
                        .write_byte(*b);
                }
            }
            kprint!(" ");
        }
        kprint!("\n");
    }

    // === Keyboard — после xHCI OS Handoff ===
    // OS Handoff в pci::find_xhci освобождает контроллер от BIOS,
    // только после этого IRQ1 начинает работать
    ioapic::redirect(unsafe { IOAPIC_BASE }, 1, 0x21, 0);
    keyboard::init();

    // === xHCI ===
    if let Some(bar_phys) = xhci_bar_phys {
        // bar_phys < 512GB — уже покрыт identity map из boot.s (pdpt0 × 1GB huge pages)
        // Маппинг не нужен, передаём физический адрес напрямую
        kprint!("xhci bar: ");
        write_hex!(bar_phys);
        kprint!("\n");
        sprintln!("xhci bar: {:#x}", bar_phys);
        unsafe {
            xhci::init(bar_phys);
        }
    } else {
        bootlog!("xhci not found\n");
    }

    // === Scheduler ===
    unsafe {
        let proc_a = process::Process::new(
            1,
            crate::syscall::SYSCALL_MASK,
            0,
            process_a as *const () as u64,
            Vec::new(),
        );
        let proc_b = process::Process::new(
            2,
            crate::syscall::SYSCALL_MASK,
            0,
            process_b as *const () as u64,
            Vec::new(),
        );
        (&raw mut scheduler::SCHEDULER)
            .as_mut()
            .unwrap()
            .add_process(proc_a);
        (&raw mut scheduler::SCHEDULER)
            .as_mut()
            .unwrap()
            .add_process(proc_b);
    }
    bootlog!("Scheduler ok\n");
    unsafe {
        // CLI до SCHEDULER_READY: если таймер сработает между READY=true и
        // первым iretq, он сохранит ядровый RIP в context.rip proc_a и
        // start_first_process_ring3 прыгнет в ring-3 с ядровым адресом → #PF.
        // iretq в start_first_process_ring3 кладёт RFLAGS=0x202 (IF=1),
        // поэтому прерывания включатся сами при входе в user mode.
        core::arch::asm!("cli", options(nomem, nostack));
        SCHEDULER_READY = true;
        shell::init();
        scheduler::start_first_process_ring3();
    }
}

/// Достаёт SLP_TYPa/SLP_TYPb из объекта `\_S5` в DSDT (AML).
/// `Name(_S5, Package(){ SLP_TYPa, SLP_TYPb, ... })` кодируется как
/// `'_S5_' PackageOp PkgLength NumElements <elem0> <elem1> ...`.
fn parse_s5(dsdt: u64) -> Option<(u8, u8)> {
    if dsdt == 0 {
        return None;
    }
    let base = dsdt as *const u8;
    let len = unsafe { read_unaligned((dsdt + 4) as *const u32) } as usize;

    let mut i = 0usize;
    while i + 5 < len {
        let win = unsafe { core::slice::from_raw_parts(base.add(i), 4) };
        if win == b"_S5_" {
            let mut p = i + 4;
            if unsafe { *base.add(p) } != 0x12 {
                // не PackageOp — ложное совпадение, ищем дальше
                i += 1;
                continue;
            }
            p += 1;
            // PkgLength: старшие 2 бита первого байта — число доп. байтов длины.
            let lead = unsafe { *base.add(p) };
            p += 1 + (lead >> 6) as usize;
            // NumElements
            p += 1;
            let a = read_aml_byte(base, &mut p);
            let b = read_aml_byte(base, &mut p);
            return Some((a & 7, b & 7));
        }
        i += 1;
    }
    None
}

/// Читает один маленький целочисленный элемент AML и двигает курсор.
fn read_aml_byte(base: *const u8, p: &mut usize) -> u8 {
    let op = unsafe { *base.add(*p) };
    *p += 1;
    match op {
        0x00 => 0, // ZeroOp
        0x01 => 1, // OneOp
        0x0A => {
            // BytePrefix — значение в следующем байте
            let v = unsafe { *base.add(*p) };
            *p += 1;
            v
        }
        _ => op, // редкий случай: значение закодировано самим опкодом
    }
}

fn parse_madt(addr: u64) -> u64 {
    let header = unsafe { &*(addr as *const AcpiHeader) };
    let length = unsafe { read_unaligned(addr_of!(header.length)) };
    let madt = unsafe { &*((addr + 36) as *const Madt) };
    let local_apic_address = unsafe { read_unaligned(addr_of!(madt.local_apic_address)) };
    kprint!("Local APIC: ");
    write_hex!(local_apic_address as u64);
    kprint!("\n");
    sprintln!("Local APIC: {:#x}", local_apic_address);

    let mut offset: u64 = 36 + 8;
    while offset < length as u64 {
        let entry_ptr = (addr + offset) as *const u8;
        let typ = unsafe { *entry_ptr };
        let entry_length = unsafe { *entry_ptr.add(1) };
        match typ {
            0 => {
                let e = unsafe { &*(entry_ptr.add(2) as *const MadtLocalApic) };
                let apic_id = unsafe { *addr_of!(e.apic_id) };
                let flags = unsafe { read_unaligned(addr_of!(e.flags)) };
                kprint!("CPU apic_id=");
                write_hex!(apic_id as u64);
                kprint!(" flags=");
                write_hex!(flags as u64);
                kprint!("\n");
                sprintln!("CPU apic_id={:#x} flags={:#x}", apic_id, flags);
            }
            1 => {
                let e = unsafe { &*(entry_ptr.add(2) as *const MadtIoApic) };
                let io_apic_id = unsafe { *addr_of!(e.io_apic_id) };
                let io_apic_address = unsafe { read_unaligned(addr_of!(e.io_apic_address)) };
                let global_irq_base = unsafe { read_unaligned(addr_of!(e.global_irq_base)) };
                if global_irq_base == 0 {
                    unsafe {
                        IOAPIC_BASE = io_apic_address as u64;
                    }
                }
                kprint!("IO APIC id=");
                write_hex!(io_apic_id as u64);
                kprint!(" addr=");
                write_hex!(io_apic_address as u64);
                kprint!("\n");
            }
            2 => {
                let e = unsafe { &*(entry_ptr.add(2) as *const MadtIso) };
                let source = unsafe { *addr_of!(e.source) };
                let gsi = unsafe { read_unaligned(addr_of!(e.global_system_interrupt)) };
                if source == 0 {
                    unsafe {
                        TIMER_GSI = gsi as u8;
                    }
                }
            }
            _ => {}
        }
        offset += entry_length as u64;
    }
    local_apic_address as u64
}
