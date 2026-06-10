use crate::CONSOLE;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};

static mut KEYBOARD: Option<Keyboard<layouts::Us104Key, ScancodeSet1>> = None;

unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!("in al, dx", out("al") val, in("dx") port);
    val
}

pub fn init() {
    unsafe {
        KEYBOARD = Some(Keyboard::new(
            ScancodeSet1::new(),
            layouts::Us104Key,
            HandleControl::Ignore,
        ));
    }
    crate::kprint!("keyboard: init ok\n");
}

#[no_mangle]
pub unsafe extern "C" fn keyboard_irq_handler() {
    // i8042 поднимает IRQ только по фронту OBF (0x64 бит0): 0→1. Одно нажатие
    // шлёт минимум 2 байта (make+break), расширенные клавиши — ещё префикс 0xE0.
    // Если вычитать лишь один байт, OBF остаётся в 1, нового фронта нет → IRQ
    // больше не приходят и клавиатура «умирает». Поэтому опустошаем буфер
    // полностью, пока OBF взведён.
    // Предохранитель: если USB-legacy-эмуляция залипнет с OBF=1, безусловный
    // while зациклит обработчик навечно. Поэтому ограничиваем число итераций —
    // но порог должен быть заведомо больше любого нормального всплеска, иначе
    // выход по лимиту оставит OBF=1, фронта 0→1 не будет, и edge-triggered IRQ1
    // умрёт навсегда. 16 было слишком мало: один долгий участок с закрытыми
    // прерываниями копит в буфере эмуляции десятки байт, и первый же IRQ
    // обрезался на 16-м, добивая клавиатуру. Берём большой порог, а при его
    // срабатывании всё равно высасываем буфер всухую, чтобы OBF гарантированно
    // ушёл в 0.
    let mut guard = 0;
    while inb(0x64) & 1 != 0 {
        guard += 1;
        if guard > 256 {
            // Аварийный дренаж залипшей эмуляции: опустошаем буфер досуха.
            while inb(0x64) & 1 != 0 {
                let _ = inb(0x60);
            }
            break;
        }
        let scancode = inb(0x60);

        if let Some(kb) = KEYBOARD.as_mut() {
            if let Ok(Some(key_event)) = kb.add_byte(scancode) {
                if let Some(key) = kb.process_keyevent(key_event) {
                    match key {
                        DecodedKey::Unicode(c) => {
                            // ASCII (включая '\n' и backspace 0x08) отдаём шеллу,
                            // он сам копит строку и эхо-печатает символ.
                            if (c as u32) < 128 {
                                crate::shell::on_char(c as u8);
                            }
                        }
                        DecodedKey::RawKey(_) => {}
                    }
                }
            }
        }
    }

    // EOI
    crate::lapic::eoi(crate::LAPIC_BASE);
}
