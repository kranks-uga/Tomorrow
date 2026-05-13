pub const PAGE_PRESENT: u64 = 1 << 0;
pub const PAGE_WRITABLE: u64 = 1 << 1;
pub const PAGE_USER: u64 = 1 << 2;
pub const PAGE_NO_EXECUTE: u64 = 1 << 63;

#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [u64; 512],
}

impl PageTable {
    // Выделяет новую таблицу страниц, возвращает физический адрес.
    // Identity map покрывает всю физическую память (boot.s: pdpt0/pdpt1 × 1GB),
    // поэтому phys == virt — пишем напрямую без KVIRT.
    unsafe fn alloc_table() -> u64 {
        let phys = crate::pmm::alloc();
        if phys == 0 {
            panic!("VMM: out of memory");
        }
        core::ptr::write_bytes(phys as *mut u8, 0, 4096);
        phys
    }

    // Получить указатель на таблицу по физическому адресу из записи.
    // Identity map: phys == virt, просто кастуем.
    #[inline]
    unsafe fn entry_to_table(entry: u64) -> *mut PageTable {
        (entry & 0x000F_FFFF_FFFF_F000) as *mut PageTable
    }

    // Маппинг одной 4KB страницы.
    // virt — виртуальный адрес, phys — физический адрес.
    pub unsafe fn map(&mut self, virt: u64, phys: u64, flags: u64) {
        let pml4_idx = (virt >> 39) & 0x1FF;
        let pdpt_idx = (virt >> 30) & 0x1FF;
        let pd_idx = (virt >> 21) & 0x1FF;
        let pt_idx = (virt >> 12) & 0x1FF;

        let user_bit = flags & PAGE_USER;
        const PS: u64 = 1 << 7;

        // PML4 → PDPT
        let pml4_entry = &mut self.entries[pml4_idx as usize];
        if *pml4_entry & PAGE_PRESENT == 0 {
            let t = Self::alloc_table();
            *pml4_entry = t | PAGE_PRESENT | PAGE_WRITABLE | user_bit;
        } else if user_bit != 0 {
            *pml4_entry |= PAGE_USER;
        }

        // PDPT → PD
        let pdpt = Self::entry_to_table(*pml4_entry);
        let pdpt_entry = &mut (*pdpt).entries[pdpt_idx as usize];
        if *pdpt_entry & PAGE_PRESENT == 0 {
            let t = Self::alloc_table();
            *pdpt_entry = t | PAGE_PRESENT | PAGE_WRITABLE | user_bit;
        } else if *pdpt_entry & PS != 0 {
            if user_bit != 0 {
                *pdpt_entry |= PAGE_USER;
            }
            return; // 1GB huge page — не трогаем
        } else if user_bit != 0 {
            *pdpt_entry |= PAGE_USER;
        }

        // PD → PT
        let pd = Self::entry_to_table(*pdpt_entry);
        let pd_entry = &mut (*pd).entries[pd_idx as usize];
        if *pd_entry & PAGE_PRESENT == 0 {
            let t = Self::alloc_table();
            *pd_entry = t | PAGE_PRESENT | PAGE_WRITABLE | user_bit;
        } else if *pd_entry & PS != 0 {
            if user_bit != 0 {
                *pd_entry |= PAGE_USER;
            }
            return; // 2MB huge page — не трогаем
        } else if user_bit != 0 {
            *pd_entry |= PAGE_USER;
        }

        // PT → физическая страница
        let pt = Self::entry_to_table(*pd_entry);
        (*pt).entries[pt_idx as usize] = phys | flags | PAGE_PRESENT;
    }
}

pub unsafe fn load(pml4: *mut PageTable) {
    // CR3 принимает физический адрес PML4.
    // pml4 — это статик из boot.s, его адрес == физический (identity map).
    core::arch::asm!("mov cr3, {}", in(reg) pml4 as u64);
}
