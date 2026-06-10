use crate::console::Console;
use crate::process::ProcessState;
use crate::{pmm, scheduler, CONSOLE, TICKS};

const PROMPT: &str = "User@Tomorrow> ";
const BUF_LEN: usize = 128;

// Накопитель текущей строки. Ввод приходит из прерываний (PS/2 IRQ1 и
// USB poll_hid в таймерном ISR), оба зовут on_char — отдельной задачи-шелла
// нет, вся обработка идёт в контексте прерывания, поэтому буфер статический.
static mut LINE: [u8; BUF_LEN] = [0; BUF_LEN];
static mut LEN: usize = 0;
static mut READY: bool = false;

// Доступ к консоли тем же паттерном, что и в остальном ядре.
fn console() -> &'static mut Console {
    unsafe { (&raw mut CONSOLE).as_mut().unwrap().as_mut().unwrap() }
}

pub fn init() {
    unsafe {
        LEN = 0;
        READY = true;
    }
    console().write_str("\nType 'help' for a list of commands.\n");
    prompt();
}

fn prompt() {
    console().write_str(PROMPT);
}

/// Единая точка приёма символа от любого драйвера клавиатуры.
/// `ch` — уже декодированный ASCII-байт ('\n' для Enter, 0x08 для Backspace).
pub fn on_char(ch: u8) {
    if unsafe { !READY } {
        return;
    }
    match ch {
        b'\n' => {
            console().write_byte(b'\n');
            execute();
            unsafe {
                LEN = 0;
            }
            prompt();
        }
        0x08 => unsafe {
            if LEN > 0 {
                LEN -= 1;
                console().backspace();
            }
        },
        _ => unsafe {
            if LEN < BUF_LEN {
                LINE[LEN] = ch;
                LEN += 1;
                console().write_byte(ch);
            }
        },
    }
}

fn execute() {
    let line = unsafe { &LINE[..LEN] };
    let line = trim(line);
    if line.is_empty() {
        return;
    }

    // Разбиваем на первое слово (команда) и остаток (аргументы).
    let (cmd, args) = split_first_word(line);

    if eq(cmd, b"help") {
        cmd_help();
    } else if eq(cmd, b"clear") || eq(cmd, b"cls") {
        console().clear();
    } else if eq(cmd, b"echo") {
        cmd_echo(args);
    } else if eq(cmd, b"ticks") {
        cmd_ticks();
    } else if eq(cmd, b"ps") {
        cmd_ps();
    } else if eq(cmd, b"mem") {
        cmd_mem();
    } else if eq(cmd, b"reboot") {
        cmd_reboot();
    } else {
        console().write_str("unknown command: ");
        write_bytes(cmd);
        console().write_str("\n");
    }
}

fn cmd_help() {
    console().write_str(
        "commands:\n\
         \x20 help          show this help\n\
         \x20 clear / cls   clear the screen\n\
         \x20 echo <text>   print text\n\
         \x20 ticks         show timer tick count\n\
         \x20 ps            list processes\n\
         \x20 mem           show free physical memory\n\
         \x20 reboot        restart the machine\n",
    );
}

fn cmd_echo(args: &[u8]) {
    write_bytes(args);
    console().write_str("\n");
}

fn cmd_ticks() {
    console().write_str("ticks: ");
    console().write_dec(unsafe { TICKS });
    console().write_str("\n");
}

fn cmd_ps() {
    let sched = unsafe { (&raw const scheduler::SCHEDULER).as_ref().unwrap() };
    console().write_str("PID  STATE\n");
    for slot in sched.processes.iter() {
        if let Some(p) = slot {
            console().write_dec(p.pid);
            console().write_str("    ");
            console().write_str(match p.state {
                ProcessState::Running => "running",
                ProcessState::Blocked => "blocked",
                ProcessState::Dead => "dead",
            });
            console().write_str("\n");
        }
    }
    console().write_str("total: ");
    console().write_dec(sched.count as u64);
    console().write_str("\n");
}

fn cmd_mem() {
    let free = pmm::free_pages();
    console().write_str("free pages: ");
    console().write_dec(free);
    console().write_str(" (");
    console().write_dec(free * 4096 / 1024);
    console().write_str(" KiB)\n");
}

fn cmd_reboot() {
    console().write_str("rebooting...\n");
    unsafe {
        core::arch::asm!("cli");

        // --- Метод 1: ACPI/PCI reset через порт 0xCF9 ---
        // На современном железе (где i8042 может физически отсутствовать,
        // как на этой машине с USB-клавиатурой за хабом RTS5411) это основной
        // путь. Бит2 = RST_CPU, бит1 = SYS_RST/полный сброс.
        core::arch::asm!(
            "out dx, al",
            in("dx") 0xCF9u16,
            in("al") 0x02u8,
            options(nostack, nomem),
        );
        core::arch::asm!(
            "out dx, al",
            in("dx") 0xCF9u16,
            in("al") 0x06u8,
            options(nostack, nomem),
        );
        io_delay();

        // --- Метод 2: импульс сброса контроллера 8042 (0x64 <- 0xFE) ---
        // Ожидание входного буфера ОГРАНИЧЕНО: если 8042 нет, порт читается
        // как 0xFF (бит1 всегда взведён) и безусловный цикл повис бы навсегда.
        for _ in 0..100_000 {
            let mut status: u8;
            core::arch::asm!("in al, 0x64", out("al") status);
            if status & 0x02 == 0 {
                break;
            }
        }
        core::arch::asm!("mov al, 0xFE", "out 0x64, al", out("al") _);
        io_delay();

        // --- Метод 3: гарантированный тройной фолт через пустой IDT ---
        // Загружаем IDTR нулевой длины и вызываем прерывание: CPU не находит
        // дескриптор → #GP → двойная → тройная ошибка → аппаратный сброс.
        #[repr(C, packed)]
        struct Idtr {
            limit: u16,
            base: u64,
        }
        let null_idt = Idtr { limit: 0, base: 0 };
        core::arch::asm!("lidt [{}]", in(reg) &null_idt, options(nostack));
        core::arch::asm!("int3");

        // Сюда не дойдём.
        loop {
            core::arch::asm!("hlt");
        }
    }
}

/// Короткая задержка ввода-вывода: запись в неиспользуемый порт 0x80.
#[inline]
unsafe fn io_delay() {
    for _ in 0..1000 {
        core::arch::asm!("out 0x80, al", in("al") 0u8, options(nostack, nomem));
    }
}

// === Вспомогательные функции работы с байтовыми срезами (без alloc) ===

fn eq(a: &[u8], b: &[u8]) -> bool {
    a == b
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = s {
        if *first == b' ' || *first == b'\t' {
            s = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = s {
        if *last == b' ' || *last == b'\t' {
            s = rest;
        } else {
            break;
        }
    }
    s
}

fn split_first_word(s: &[u8]) -> (&[u8], &[u8]) {
    match s.iter().position(|&c| c == b' ') {
        Some(i) => (&s[..i], trim(&s[i + 1..])),
        None => (s, &[]),
    }
}

fn write_bytes(s: &[u8]) {
    for &b in s {
        console().write_byte(b);
    }
}
