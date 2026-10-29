// SPDX-License-Identifier: GPL-3.0-or-later
//! Serial-порт COM1 (16550 UART).
//!
//! Зачем: экран показывает только последнюю строку и умирает вместе с ядром.
//! UART даёт независимый канал — boot-лог, паники и «unknown syscall» видны
//! снаружи (QEMU `-serial stdio`, на железе — USB-TTL), даже если консоль
//! залипла или ядро упало.
//!
//! Только передача (TX): приём, прерывания и аппаратные FIFO-события не
//! используются. Порт опрашивается по LSR, ожидание ограничено счётчиком —
//! отсутствующий UART не должен вешать загрузку.

use core::fmt::{self, Write};
use spin::Mutex;

/// Базовый адрес COM1.
const COM1: u16 = 0x3F8;

/// LSR: бит 5 — регистр передатчика пуст, можно писать следующий байт.
const LSR_THR_EMPTY: u8 = 0x20;

/// Сколько раз опросить LSR, прежде чем бросить байт. На реальном железе
/// 115200 бод отдаёт байт за ~87 мкс; запас с большим коэффициентом.
const TX_SPIN_LIMIT: u32 = 1_000_000;

/// Один писатель за раз: boot-лог, шелл и паника не должны перемешивать
/// байты. В обработчиках прерываний serial не используется — иначе лок,
/// взятый в прерванном коде, дал бы дедлок.
static LOCK: Mutex<()> = Mutex::new(());

#[inline]
unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nomem, nostack, preserves_flags),
    );
}

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!(
        "in al, dx",
        in("dx") port,
        out("al") value,
        options(nomem, nostack, preserves_flags),
    );
    value
}

/// Настроить COM1: 115200 8N1, FIFO включён, прерывания выключены.
pub fn init() {
    unsafe {
        outb(COM1 + 1, 0x00); // IER: никаких прерываний
        outb(COM1 + 3, 0x80); // LCR: DLAB=1 — следующие два байта задают делитель
        outb(COM1 + 0, 0x01); // DLL = 1 → 115200 бод
        outb(COM1 + 1, 0x00); // DLM = 0
        outb(COM1 + 3, 0x03); // LCR: DLAB=0, 8 бит, без паритета, 1 стоп-бит
        outb(COM1 + 2, 0xC7); // FCR: FIFO вкл., сброс, порог 14 байт
        outb(COM1 + 4, 0x03); // MCR: DTR | RTS
    }
}

/// Записать один байт, не беря лок (вызывающий уже держит его).
fn emit_byte(b: u8) {
    // Терминалы ожидают CRLF; голый \n уводит курсор в начало строки.
    if b == b'\n' {
        emit_raw(b'\r');
    }
    emit_raw(b);
}

fn emit_raw(b: u8) {
    unsafe {
        for _ in 0..TX_SPIN_LIMIT {
            if inb(COM1 + 5) & LSR_THR_EMPTY != 0 {
                outb(COM1, b);
                return;
            }
        }
        // Порт не отвечает — молча теряем байт, но не загрузку.
    }
}

fn emit_str(s: &str) {
    for b in s.bytes() {
        emit_byte(b);
    }
}

/// Неблокирующий вариант `_print` — для паники.
pub fn try_print(args: fmt::Arguments) -> bool {
    match LOCK.try_lock() {
        Some(_guard) => {
            let _ = Writer.write_fmt(args);
            true
        }
        None => false,
    }
}

struct Writer;

impl Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        emit_str(s);
        Ok(())
    }
}

/// Точка входа для `sprint!`/`sprintln!`. Публична, потому что макросы
/// раскрываются в вызывающем модуле.
pub fn _print(args: fmt::Arguments) {
    let _guard = LOCK.lock();
    let _ = Writer.write_fmt(args);
}

/// Печать в serial без перевода строки.
#[macro_export]
macro_rules! sprint {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*))
    };
}

/// Печать в serial с переводом строки.
#[macro_export]
macro_rules! sprintln {
    () => {
        $crate::sprint!("\n")
    };
    ($($arg:tt)*) => {{
        $crate::sprint!($($arg)*);
        $crate::sprint!("\n");
    }};
}
