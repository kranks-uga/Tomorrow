//authors: Kirill Repin, Denis Ansuk, Rusina Lilianna, Filipp Razanov

#![no_std]
#![no_main]

use crate::console::Console;
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
mod scheduler;
mod syscall;
mod tss;
mod vmm;
mod xhci;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

const KERNEL_VIRT: u64 = 0xFFFF800000000000;

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
pub unsafe extern "C" fn timer_do_switch(regs: *mut SavedRegs) -> *const scheduler::Context {
    TICKS += 1;
    lapic::eoi(LAPIC_BASE);

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

    for i in 1..64 {
        let idx = (current + i) % 64;
        if let Some(p) = &scheduler::SCHEDULER.processes[idx] {
            if p.state == process::ProcessState::Running {
                scheduler::SCHEDULER.current = idx;
                (&raw mut tss::TSS).as_mut().unwrap().rsp0 = p.kernel_stack;
                return &p.context as *const _;
            }
        }
    }

    core::ptr::null()
}

extern "C" {
    static pml4: vmm::PageTable;
}

#[unsafe(naked)]
extern "C" fn process_a() -> ! {
    core::arch::naked_asm!(
        ".intel_syntax noprefix",
        "lea rsi, [rip + 1f]",
        "mov rax, 1",
        "mov rdi, 1",
        "mov rdx, 3",
        "syscall",
        "0: jmp 0b",
        "1: .byte 0x55, 0x33, 0x0A",
        ".att_syntax prefix",
    )
}

#[unsafe(naked)]
extern "C" fn process_b() -> ! {
    core::arch::naked_asm!(
        ".intel_syntax noprefix",
        "lea rsi, [rip + 1f]",
        "mov rax, 1",
        "mov rdi, 1",
        "mov rdx, 2",
        "syscall",
        "0: jmp 0b",
        "1: .byte 0x42, 0x0A",
        ".att_syntax prefix",
    )
}

#[no_mangle]
pub extern "C" fn kernel_main(boot_info: u64) -> ! {
    let mut xsdt_addr: u64 = 0;
    let mut mmap_addr: u64 = 0;
    let mut mmap_size: u32 = 0;
    let mut mmap_entry_size: u32 = 0;

    // === Парсим Multiboot2 теги ===
    let mut ptr = (boot_info + 8) as *const Mbtag;
    loop {
        let tag = unsafe { &*(ptr as *const Mbtag) };
        match tag.typ {
            0 => break,
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
                kprint!("Tomorrow OS\n");
                idt::init();
                kprint!("IDT ok\n");
                idt::set_handler(0xFF, idt::spurious_handler as *const () as u64);
                idt::set_handler(0x20, idt::timer_handler_asm as *const () as u64);
                idt::set_handler(0x21, idt::keyboard_handler_asm as *const () as u64);
                idt::set_handler(0x0D, idt::gp_handler_asm as *const () as u64);
                idt::set_handler(0x0E, idt::pf_handler_asm as *const () as u64);
                pic::disable();
                kprint!("PIC off\n");
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
        }
    }
    kprint!("PMM ok\n");

    // === TSS ===
    let kernel_stack = unsafe { pmm::alloc() + 4096 }; // +4096 — стек растёт вниз
    unsafe {
        tss::init(kernel_stack);
    }
    kprint!("TSS ok\n");

    // === HEAP ===
    let heap_start = unsafe { pmm::alloc() }; // identity map — физ. адрес доступен напрямую
    heap::HEAP.init(heap_start, 4096 * 16);
    kprint!("HEAP ok\n");

    // === VMM ===
    // Identity map из boot.s покрывает всю физическую память — доп. маппинг не нужен
    kprint!("VMM ok\n");

    // === SYSCALL ===
    syscall::init();
    kprint!("Syscall ok\n");

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
                kprint!("APIC ok\n");
            }
        }

        // Проход 2: MCFG → xHCI
        for i in 0..count {
            let entry_addr = unsafe { read_unaligned(entries_ptr.add(i)) };
            let sig = unsafe { &*(entry_addr as *const [u8; 4]) };

            if sig == b"MCFG" {
                unsafe {
                    let mcfg_base = core::ptr::read_unaligned((entry_addr + 44) as *const u64);
                    kprint!("MCFG: ");
                    write_hex!(mcfg_base);
                    kprint!("\n");
                    xhci_bar_phys = pci::find_xhci(mcfg_base);
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
        // unsafe {
        //     xhci::init(bar_phys);
        // }
    } else {
        kprint!("xhci not found\n");
    }

    // === Scheduler ===
    unsafe {
        let proc_a = process::Process::new(1, 0b11, 0, process_a as *const () as u64);
        let proc_b = process::Process::new(2, 0b11, 0, process_b as *const () as u64);
        (&raw mut scheduler::SCHEDULER)
            .as_mut()
            .unwrap()
            .add_process(proc_a);
        (&raw mut scheduler::SCHEDULER)
            .as_mut()
            .unwrap()
            .add_process(proc_b);
        kprint!("Scheduler ok\n");
        SCHEDULER_READY = true;
        scheduler::start_first_process_ring3();
    }

    loop {}
}

fn parse_madt(addr: u64) -> u64 {
    let header = unsafe { &*(addr as *const AcpiHeader) };
    let length = unsafe { read_unaligned(addr_of!(header.length)) };
    let madt = unsafe { &*((addr + 36) as *const Madt) };
    let local_apic_address = unsafe { read_unaligned(addr_of!(madt.local_apic_address)) };
    kprint!("Local APIC: ");
    write_hex!(local_apic_address as u64);
    kprint!("\n");

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
