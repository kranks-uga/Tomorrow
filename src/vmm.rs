pub const PAGE_PRESENT: u64 = 1 << 0; // страница существует
pub const PAGE_WRITABLE: u64 = 1 << 1; // можно писать
pub const PAGE_USER: u64 = 1 << 2; // доступна из ring 3 (userspace)
pub const PAGE_NO_EXECUTE: u64 = 1 << 63; // нельзя выполнять код (NX бит)

#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [u64; 512],
}

impl PageTable {
    pub unsafe fn new() -> *mut PageTable {
        let addr = crate::pmm::alloc();
        if addr == 0 {
            panic!("VMM: got zero from PMM");
        }
        let ptr = addr as *mut PageTable;
        for i in 0..512 {
            (*ptr).entries[i] = 0;
        }
        ptr
    }

    pub unsafe fn map(&mut self, virt: u64, phys: u64, flags: u64) {
        let pml4_idx = (virt >> 39) & 0x1FF;
        let pdpt_idx = (virt >> 30) & 0x1FF;
        let pd_idx = (virt >> 21) & 0x1FF;
        let pt_idx = (virt >> 12) & 0x1FF;

        let user_bit = flags & PAGE_USER;

        let pml4_entry = &mut self.entries[pml4_idx as usize];
        if *pml4_entry & PAGE_PRESENT == 0 {
            let new_table = PageTable::new();
            *pml4_entry = new_table as u64 | PAGE_PRESENT | PAGE_WRITABLE | user_bit;
        } else if user_bit != 0 {
            *pml4_entry |= PAGE_USER;
        }

        const PS: u64 = 1 << 7; // large page bit

        let pdpt = ((*pml4_entry) & 0x000FFFFF_FFFFF000) as *mut PageTable;
        let pdpt_entry = &mut (*pdpt).entries[pdpt_idx as usize];
        if *pdpt_entry & PAGE_PRESENT == 0 {
            let new_table = PageTable::new();
            *pdpt_entry = new_table as u64 | PAGE_PRESENT | PAGE_WRITABLE | user_bit;
        } else if *pdpt_entry & PS != 0 {
            // 1GB large page — просто добавляем USER, нельзя создать 4KB внутри
            if user_bit != 0 { *pdpt_entry |= PAGE_USER; }
            return;
        } else if user_bit != 0 {
            *pdpt_entry |= PAGE_USER;
        }

        let pd = ((*pdpt_entry) & 0x000FFFFF_FFFFF000) as *mut PageTable;
        let pd_entry = &mut (*pd).entries[pd_idx as usize];
        if *pd_entry & PAGE_PRESENT == 0 {
            let new_table = PageTable::new();
            *pd_entry = new_table as u64 | PAGE_PRESENT | PAGE_WRITABLE | user_bit;
        } else if *pd_entry & PS != 0 {
            // 2MB large page — просто добавляем USER, нельзя создать 4KB внутри
            if user_bit != 0 { *pd_entry |= PAGE_USER; }
            return;
        } else if user_bit != 0 {
            *pd_entry |= PAGE_USER;
        }

        let pt = ((*pd_entry) & 0x000FFFFF_FFFFF000) as *mut PageTable;
        let pt_entry = &mut (*pt).entries[pt_idx as usize];
        (*pt).entries[pt_idx as usize] = phys | flags | PAGE_PRESENT;
    }
}

pub unsafe fn load(pml4: *mut PageTable) {
    let addr = pml4 as u64;
    core::arch::asm!("mov cr3, {}", in(reg) addr);
}
