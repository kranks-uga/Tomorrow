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
mod lapic;
mod pic;
mod pmm;
mod syscall;
mod vmm;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
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
struct MadtEntryHeader {
    typ: u8,
    length: u8,
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
            CONSOLE.as_mut().unwrap().write_str($s);
        }
    };
}

static mut IOAPIC_BASE: u64 = 0;
static mut LAPIC_BASE: u64 = 0;
static mut TICKS: u64 = 0;
static mut TIMER_GSI: u8 = 0;

#[no_mangle]
extern "C" fn timer_tick() {
    unsafe {
        TICKS += 1;
        CONSOLE.as_mut().unwrap().write_str("T: ");
        CONSOLE.as_mut().unwrap().write_dec(TICKS);
        CONSOLE.as_mut().unwrap().write_str(" ");
        lapic::eoi(LAPIC_BASE);
    }
}

extern "C" {
    static pml4: vmm::PageTable;
}

#[no_mangle]
pub extern "C" fn kernel_main(boot_info: u64) -> ! {
    let mut xsdt_addr: u64 = 0;
    let mut lapic_base: u64;
    let mut mmap_addr: u64 = 0;
    let mut mmap_size: u32 = 0;
    let mut mmap_entry_size: u32 = 0;

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
                    CONSOLE.as_mut().unwrap().clear();
                }
                kprint!("Tomorrow OS\n");
                idt::init();
                kprint!("IDT ok\n");
                idt::set_handler(0xFF, idt::spurious_handler as u64);
                idt::set_handler(0x20, idt::timer_handler as u64);
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

    // PMM
    if mmap_addr != 0 {
        unsafe {
            pmm::init(mmap_addr, mmap_size, mmap_entry_size);
        }
    }
    kprint!("PMM ok\n");

    // HEAP
    let heap_start = unsafe { pmm::alloc() };
    unsafe {
        heap::HEAP.init(heap_start, 4096 * 16);
    }
    kprint!("HEAP ok\n");

    // VMM
    unsafe {
        let pml4_ptr = &pml4 as *const _ as *mut vmm::PageTable;
        (*pml4_ptr).map(0xFFFF_8000_0020_0000, 0x200000, vmm::PAGE_WRITABLE);
    }
    kprint!("VMM ok\n");

    //SYSCALL
    syscall::init();
    kprint!("Syscall ok\n");

    // ACPI
    if xsdt_addr != 0 {
        let header = unsafe { &*(xsdt_addr as *const AcpiHeader) };
        let length = unsafe { read_unaligned(addr_of!(header.length)) };
        let count = (length as usize - 36) / 8;
        let entries_ptr = (xsdt_addr + 36) as *const u64;
        for i in 0..count {
            let entry_addr = unsafe { read_unaligned(entries_ptr.add(i)) };
            let sig = unsafe { &*(entry_addr as *const [u8; 4]) };
            if sig == b"APIC" {
                lapic_base = parse_madt(entry_addr);
                unsafe {
                    LAPIC_BASE = lapic_base;
                }
                lapic::enable(lapic_base);
                ioapic::redirect(unsafe { IOAPIC_BASE }, unsafe { TIMER_GSI }, 0x20, 0);
                unsafe {
                    core::arch::asm!("sti");
                }
            }
            if sig == b"HPET" {
                let hpet_base = parse_hpet(entry_addr);
                unsafe { hpet::init_hpet(hpet_base) };
            }
            for b in sig {
                unsafe {
                    CONSOLE.as_mut().unwrap().write_byte(*b);
                }
            }
            kprint!(" ");
        }
        kprint!("\n");
    }

    loop {}
}

fn parse_madt(addr: u64) -> u64 {
    let header = unsafe { &*(addr as *const AcpiHeader) };
    let length = unsafe { read_unaligned(addr_of!(header.length)) };
    let madt = unsafe { &*((addr + 36) as *const Madt) };
    let local_apic_address = unsafe { read_unaligned(addr_of!(madt.local_apic_address)) };
    kprint!("Local APIC: ");
    unsafe {
        CONSOLE
            .as_mut()
            .unwrap()
            .write_hex(local_apic_address as u64);
    }
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
                unsafe {
                    CONSOLE.as_mut().unwrap().write_hex(apic_id as u64);
                }
                kprint!(" flags=");
                unsafe {
                    CONSOLE.as_mut().unwrap().write_hex(flags as u64);
                }
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
                unsafe {
                    CONSOLE.as_mut().unwrap().write_hex(io_apic_id as u64);
                }
                kprint!(" addr=");
                unsafe {
                    CONSOLE.as_mut().unwrap().write_hex(io_apic_address as u64);
                }
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

fn parse_hpet(entry_base: u64) -> u64 {
    unsafe { read_unaligned((entry_base + 44) as *const u64) }
}
