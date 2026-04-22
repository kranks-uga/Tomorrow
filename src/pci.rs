use crate::CONSOLE;

pub unsafe fn enumerate(base: u64) {
    for bus in 0u64..256 {
        for dev in 0u64..32 {
            for fun in 0u64..8 {
                let addr = base + ((bus << 20) | (dev << 15) | (fun << 12));
                let vendor = *(addr as *const u16);
                if vendor == 0xFFFF {
                    continue;
                }
                let class = *((addr + 0x0B) as *const u8);
                let subclass = *((addr + 0x0A) as *const u8);
                let id = *(addr as *const u32);
                let device = (id >> 16) as u16;

                crate::kprint!("PCI ");
                crate::write_hex!(bus);
                crate::kprint!(":");
                crate::write_hex!(dev);
                crate::kprint!(" ");
                crate::write_hex!(vendor as u64);
                crate::kprint!(":");
                crate::write_hex!(device as u64);
                crate::kprint!(" cl=");
                crate::write_hex!(class as u64);
                crate::write_hex!(subclass as u64);
                crate::kprint!("\n");
            }
        }
    }
}

pub unsafe fn find_xhci(base: u64) -> Option<u64> {
    for bus in 0u64..256 {
        for dev in 0u64..32 {
            for fun in 0u64..8 {
                let addr = base + ((bus << 20) | (dev << 15) | (fun << 12));
                let vendor = *(addr as *const u16);
                if vendor == 0xFFFF {
                    continue;
                }

                let class = *((addr + 0x0B) as *const u8);
                let subclass = *((addr + 0x0A) as *const u8);
                let progif = *((addr + 0x09) as *const u8);

                if class == 0x0C && subclass == 0x03 && progif == 0x30 {
                    // BAR0: биты [63:4] — базовый адрес (64-bit BAR)
                    let bar0_lo = *((addr + 0x10) as *const u32) as u64;
                    let bar0_hi = *((addr + 0x14) as *const u32) as u64;
                    let bar0 = ((bar0_hi << 32) | bar0_lo) & !0xF;
                    return Some(bar0);
                }
            }
        }
    }
    None
}
