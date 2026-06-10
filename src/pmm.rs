unsafe extern "C" {
    static _kernel_start: u8;
    static _kernel_end: u8;
}

#[repr(C)]
struct MbMemoryEntry {
    base_addr: u64,
    length: u64,
    typ: u32,
    reserved: u32,
}

static mut BITMAP: [u64; 32768] = [0xFFFFFFFFFFFFFFFF; 32768]; // все заняты

pub unsafe fn init(mmap_addr: u64, mmap_size: u32, entry_size: u32) {
    let mut offset = 0u32;
    let entries_start = mmap_addr + 16; // пропускаем typ+size+entry_size+entry_version
    let entries_len = mmap_size - 16;

    // Шаг 1: пометить свободные страницы из multiboot2 memory map
    while offset < entries_len {
        let entry = &*((entries_start + offset as u64) as *const MbMemoryEntry);
        if entry.typ == 1 {
            let mut addr = entry.base_addr;
            // пропускаем нулевую страницу — зарезервирована
            if addr == 0 {
                addr = 4096;
            }
            while addr + 4096 <= entry.base_addr + entry.length {
                mark_free(addr);
                addr += 4096;
            }
        }
        offset += entry_size;
    }

    // Шаг 2а: резервируем первые 1MB — там BIOS, IVT, EBDA, видеопамять
    // Без этого PMM выдаёт страницы типа 0x8000 куда пишет BIOS/контроллеры
    let mut addr = 0u64;
    while addr < 0x100000 {
        mark_used(addr);
        addr += 4096;
    }

    // Шаг 2: пометить страницы ядра как занятые
    // _kernel_start и _kernel_end — физические адреса из linker.ld
    let kstart = core::ptr::addr_of!(_kernel_start) as u64;
    let kend = core::ptr::addr_of!(_kernel_end) as u64;
    let page_start = kstart & !0xFFF;
    let page_end = (kend + 0xFFF) & !0xFFF;
    let mut addr = page_start;
    while addr < page_end {
        mark_used(addr);
        addr += 4096;
    }
}

pub unsafe fn alloc() -> u64 {
    for idx in 0..32768 {
        if BITMAP[idx] != u64::MAX {
            let bit = BITMAP[idx].trailing_ones() as usize;
            let page = idx * 64 + bit;
            let addr = page as u64 * 4096;
            mark_used(addr);
            return addr;
        }
    }
    panic!("PMM: out of memory");
}

pub unsafe fn free(addr: u64) {
    let addr = addr & !0xFFF;

    // 1. Не отдовать обратно зарезервированое: 1 Мб и страницы ядра.
    // Иначе alloc положит процес на систему(летально)
    let kstart = core::ptr::addr_of!(_kernel_start) as u64 & !0xFFF;
    let kend = (core::ptr::addr_of!(_kernel_end) as u64 + 0xFFF) & !0xFFF;
    if addr < 0x100000 || (addr >= kstart && addr < kend) {
        return;
    }

    // 2. Защита от double-free: если бит уже 0 (свободна) — выходим.
    // Если будет одна страница на два процеса будет плохо
    let page = (addr / 4096) as usize;
    let idx = page / 64;
    let bit = page % 64;
    if idx >= 32768 {
        return;
    }
    if BITMAP[idx] & (1u64 << bit) == 0 {
        return; // уже свободна
    }

    mark_free(addr);
}

pub fn free_pages() -> u64 {
    let mut free = 0u64;
    unsafe {
        for idx in 0..32768 {
            free += BITMAP[idx].count_zeros() as u64;
        }
    }
    free
}

fn mark_free(addr: u64) {
    let page = (addr / 4096) as usize;
    let idx = page / 64;
    let bit = page % 64;
    if idx >= 32768 {
        return;
    }
    unsafe {
        BITMAP[idx] &= !(1u64 << bit);
    }
}

fn mark_used(addr: u64) {
    let page = (addr / 4096) as usize;
    let idx = page / 64;
    let bit = page % 64;
    if idx >= 32768 {
        return;
    }
    unsafe {
        BITMAP[idx] |= 1u64 << bit;
    }
}
