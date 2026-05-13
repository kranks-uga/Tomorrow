use crate::CONSOLE;
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
    ".intel_syntax noprefix",
    ".globl spurious_handler",
    "spurious_handler:",
    "iretq",
    // #GP (13) — pushes error code
    ".globl gp_handler_asm",
    "gp_handler_asm:",
    // stack: [rsp+0]=error_code, [rsp+8]=rip, [rsp+16]=cs, [rsp+24]=rflags, [rsp+32]=rsp_user, [rsp+40]=ss
    "mov rdi, [rsp]",      // arg1 = error_code
    "mov rsi, [rsp + 8]",  // arg2 = fault rip
    "mov rdx, [rsp + 16]", // arg3 = cs
    "add rsp, 8",          // skip error code
    "call on_gp",
    "cli",
    "2: hlt",
    "jmp 2b",
    // #PF (14) — pushes error code
    ".globl pf_handler_asm",
    "pf_handler_asm:",
    "mov rdi, [rsp]",     // error_code
    "mov rsi, [rsp + 8]", // fault rip
    "mov rdx, cr2",       // faulting virtual address
    "add rsp, 8",
    "call on_pf",
    "cli",
    "3: hlt",
    "jmp 3b",
);

extern "C" {
    pub fn spurious_handler();
    pub fn timer_handler_asm();
    pub fn keyboard_handler_asm();
    pub fn gp_handler_asm();
    pub fn pf_handler_asm();
}

#[no_mangle]
pub unsafe extern "C" fn on_gp(err: u64, rip: u64, cs: u64) {
    crate::kprint!("#GP! err=");
    crate::write_hex!(err);
    crate::kprint!(" rip=");
    crate::write_hex!(rip);
    crate::kprint!(" cs=");
    crate::write_hex!(cs);
    crate::kprint!("\n");
}

#[no_mangle]
pub unsafe extern "C" fn on_pf(err: u64, rip: u64, cr2: u64) {
    crate::kprint!("#PF! err=");
    crate::write_hex!(err);
    crate::kprint!(" rip=");
    crate::write_hex!(rip);
    crate::kprint!(" cr2=");
    crate::write_hex!(cr2);
    crate::kprint!("\n");
}
