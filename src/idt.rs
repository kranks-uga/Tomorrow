use core::arch::global_asm;

#[derive(Clone, Copy)]
#[repr(C, packed)]
struct IdtEntry {
    offset_low: u16,  // биты 0-15 адреса обработчика
    selector: u16,    // 0x0008 (code segment)
    ist: u8,          // 0 (interrupt stack table, не используем)
    flags: u8,        // 0x8E (present + interrupt gate + DPL=0)
    offset_mid: u16,  // биты 16-31 адреса
    offset_high: u32, // биты 32-63 адреса
    reserved: u32,    // 0
}

#[repr(C, packed)]
struct IdtDescriptor {
    limit: u16, // размер IDT - 1 = 256*16 - 1 = 4095
    base: u64,  // адрес массива IDT
}

const EMPTY_ENTRY: IdtEntry = IdtEntry {
    offset_low: 0,
    selector: 0,
    ist: 0,
    flags: 0,
    offset_mid: 0,
    offset_high: 0,
    reserved: 0,
};

static mut IDT: [IdtEntry; 256] = [EMPTY_ENTRY; 256];

pub fn init() {
    // заполняем все векторы spurious_handler (iretq) — защита от необработанных IRQ
    let handler = spurious_handler as *const () as u64;
    for v in 0..=255u8 {
        set_handler(v, handler);
    }

    let descriptor = IdtDescriptor {
        limit: (256 * 16 - 1) as u16,
        base: unsafe { core::ptr::addr_of!(IDT) as u64 },
    };
    unsafe {
        core::arch::asm!("lidt [{}]", in(reg) &descriptor);
    }
}

pub fn set_handler(vector: u8, handler: u64) {
    unsafe {
        let entry = &raw mut IDT[vector as usize];
        (*entry).offset_low = (handler & 0xFFFF) as u16;
        (*entry).offset_mid = ((handler >> 16) & 0xFFFF) as u16;
        (*entry).offset_high = (handler >> 32) as u32;
        (*entry).selector = 0x0008;
        (*entry).flags = 0x8E;
        (*entry).ist = 0;
        (*entry).reserved = 0;
    }
}

global_asm!(
    ".globl spurious_handler",
    "spurious_handler:",
    "iretq",
);

extern "C" {
    pub fn spurious_handler();
    pub fn timer_handler_asm();
}
