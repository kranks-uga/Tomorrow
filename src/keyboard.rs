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
    let scancode = inb(0x60);

    if let Some(kb) = KEYBOARD.as_mut() {
        if let Ok(Some(key_event)) = kb.add_byte(scancode) {
            if let Some(key) = kb.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode(c) => {
                        // Печатаем только ASCII символы
                        if (c as u32) < 128 {
                            (&raw mut crate::CONSOLE)
                                .as_mut()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .write_byte(c as u8);
                        }
                    }
                    DecodedKey::RawKey(_) => {}
                }
            }
        }
    }

    // EOI
    crate::lapic::eoi(crate::LAPIC_BASE);
}
