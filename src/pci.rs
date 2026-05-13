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

// xHCI OS Handoff — говорим контроллеру передать управление от BIOS к ОС.
// Пока не сделано — BIOS держит xHCI и эмулирует PS/2 через SMI,
// из-за чего IRQ1 никогда не приходит в ядро.
unsafe fn xhci_os_handoff(addr: u64) {
    // Ищем xHCI Extended Capability "USB Legacy Support" (ID=1) в списке
    // расширенных возможностей контроллера.
    // HCCPARAMS1[31:16] = xECP — смещение первого Extended Capability в dwords.
    let hccparams1 = *((addr + 0x10) as *const u32);
    let mut off = ((hccparams1 >> 16) & 0xFFFF) as u64 * 4;

    // Максимум 256 итераций чтобы не зависнуть
    for _ in 0..256 {
        if off == 0 {
            break;
        }

        let cap = *((addr + off) as *const u32);
        let cap_id = cap & 0xFF;
        let cap_next = (cap >> 8) & 0xFF;

        if cap_id == 1 {
            // USBLEGSUP найден
            // Бит 24 = HC OS Owned Semaphore (мы хотим установить)
            // Бит 16 = HC BIOS Owned Semaphore (ждём сброса)
            let legsup = addr + off;

            // Устанавливаем OS Owned бит
            let val = *((legsup) as *const u32);
            *((legsup) as *mut u32) = val | (1 << 24);

            // Ждём пока BIOS сбросит свой бит (таймаут ~1 сек)
            let mut timeout = 1_000_000u32;
            loop {
                let v = *((legsup) as *const u32);
                // BIOS Owned (бит 16) должен стать 0
                if v & (1 << 16) == 0 {
                    break;
                }
                timeout -= 1;
                if timeout == 0 {
                    crate::kprint!("xhci: BIOS handoff timeout\n");
                    break;
                }
                core::arch::asm!("pause");
            }

            crate::kprint!("xhci: OS handoff done\n");

            // Отключаем SMI — биты в USBLEGCTLSTS (offset +4 от USBLEGSUP)
            // Сбрасываем все SMI enable биты (биты 0..12)
            let legctlsts = legsup + 4;
            let ctrl = *((legctlsts) as *const u32);
            *((legctlsts) as *mut u32) = ctrl & !0x1F_FF00; // сбрасываем SMI enable
            break;
        }

        if cap_next == 0 {
            break;
        }
        off += cap_next as u64 * 4;
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
                    let bar0_lo = *((addr + 0x10) as *const u32) as u64;
                    let bar0_hi = *((addr + 0x14) as *const u32) as u64;
                    let bar0 = ((bar0_hi << 32) | bar0_lo) & !0xF;

                    crate::kprint!("xhci: found at bus=");
                    crate::write_hex!(bus);
                    crate::kprint!(" bar0=");
                    crate::write_hex!(bar0);
                    crate::kprint!("\n");

                    // Передаём управление от BIOS к ОС перед возвратом
                    xhci_os_handoff(bar0);

                    return Some(bar0);
                }
            }
        }
    }
    None
}
