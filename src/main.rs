#![no_std]
#![no_main]

use core::{ptr::{addr_of, read_unaligned}};

use crate::console::Console;

mod boot;
mod font;
mod console;
mod idt;
mod pic;
mod lapic;
mod ioapic;
mod hpet;

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
    // header уже прочитан отдельно, offset 2:
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
        CONSOLE.as_mut().unwrap().write_str("T");
        lapic::eoi(LAPIC_BASE);
    }
}  

#[no_mangle]
pub extern "C" fn kernel_main(boot_info: u64) -> ! {
    let mut xsdt_addr: u64 = 0;
    let mut lapic_base: u64;
    
    let mut ptr = (boot_info + 8) as *const Mbtag;
    loop {
        let tag = unsafe { &*(ptr as *const Mbtag) };

        match tag.typ {
            0 => break,
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
                }
                unsafe { CONSOLE.as_mut().unwrap().clear() };
                kprint!("Tomorow OS\n");
                idt::init();
                kprint!("IDT init\n");
                idt::set_handler(0xFF, idt::spurious_handler as u64);
                idt::set_handler(0x20, idt::timer_handler as u64);
                kprint!("LAPIC enabled\n");
                pic::disable();
                kprint!("PIC Off\n");
                
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

    kprint!("XSDT: ");
    unsafe { CONSOLE.as_mut().unwrap().write_hex(xsdt_addr); }
    kprint!("\n");

    // парсим XSDT
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
                unsafe{ LAPIC_BASE = lapic_base; }
                lapic::enable(lapic_base);
                ioapic::redirect(unsafe { IOAPIC_BASE }, unsafe { TIMER_GSI }, 0x20, 0);
                unsafe { core::arch::asm!("sti"); }
            }
            if sig == b"HPET" {
                unsafe { hpet::init_hpet(entry_addr); };
            }
            for b in sig {
                unsafe { CONSOLE.as_mut().unwrap().write_byte(*b); }
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
    kprint!("Local APIC Address: ");
    unsafe { CONSOLE.as_mut().unwrap().write_hex(local_apic_address as u64); }
    kprint!("\n");
    let mut offset: u64 = 36 + 8; // пропускаем заголовок MADT
    while offset < length as u64 {
        let entry_ptr = (addr + offset) as *const u8;
        let typ = unsafe { *entry_ptr }; 
        let entry_length = unsafe { *entry_ptr.add(1) };

        match typ {
            0 => {
                let e = unsafe { &*(entry_ptr.add(2) as *const MadtLocalApic) };
                let apic_id = unsafe { *addr_of!(e.apic_id) };
                let flags   = unsafe { read_unaligned(addr_of!(e.flags)) };
                kprint!("CPU apic_id=");
                unsafe { CONSOLE.as_mut().unwrap().write_hex(apic_id as u64); }
                kprint!(" flags=");
                unsafe { CONSOLE.as_mut().unwrap().write_hex(flags as u64); } 
                kprint!("\n");
            }

            1 => {
                let e = unsafe { &*(entry_ptr.add(2) as *const MadtIoApic) };
                let io_apic_id= unsafe { *addr_of!(e.io_apic_id) };                                                                                                                                  
                let io_apic_address = unsafe { read_unaligned(addr_of!(e.io_apic_address)) };                                                                                                              
                let global_irq_base = unsafe { read_unaligned(addr_of!(e.global_irq_base)) };

                if global_irq_base == 0 {
                    unsafe { IOAPIC_BASE = io_apic_address as u64; }
                }

                kprint!("IO APIC id=");
                unsafe { CONSOLE.as_mut().unwrap().write_hex(io_apic_id as u64);}
                kprint!(" address=");
                unsafe { CONSOLE.as_mut().unwrap().write_hex(io_apic_address as u64); }
                kprint!(" global_irq_base=");
                unsafe { CONSOLE.as_mut().unwrap().write_hex(global_irq_base as u64); }
                kprint!("\n");
            }

            2 => {
                let e = unsafe{ &*(entry_ptr.add(2) as *const MadtIso) };
                let bus = unsafe { *addr_of!(e.bus) };
                let source = unsafe { *addr_of!(e.source) };
                let gsi = unsafe { read_unaligned(addr_of!(e.global_system_interrupt)) };
                let flags = unsafe { read_unaligned(addr_of!(e.flags)) };

                if source == 0 {
                    unsafe { TIMER_GSI = gsi as u8; }
                }
                kprint!("Interrupt Source Override bus=");
                unsafe { CONSOLE.as_mut().unwrap().write_hex(bus as u64); }
                kprint!(" source=");
                unsafe { CONSOLE.as_mut().unwrap().write_hex(source as u64); }
                kprint!(" gsi=");
                unsafe { CONSOLE.as_mut().unwrap().write_hex(gsi as u64); }
                kprint!(" flags=");
                unsafe { CONSOLE.as_mut().unwrap().write_hex(flags as u64); }
                kprint!("\n");
            }

            _ => {}

        }

        offset += entry_length as u64;
        
    }
    local_apic_address as u64
}
