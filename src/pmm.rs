#[repr(C)]
struct MbMemoryMap { 
    typ: u32,
    size: u32,
    entry_size: u32,
    entry_version: u32 
}

#[repr(C)]
struct MbMemoryEntry { 
    base_addr: u64, 
    length: u64, 
    typ: u32, 
    reserved: u32 
}

static mut BITMAP: [u64; 32768] = [0xFFFFFFFFFFFFFFFF; 32768]; // все заняты

static mut TOTAL_PAGES: usize = 0;

pub unsafe fn init(mmap_addr: u64, mmap_size: u32, entry_size: u32) {
    let mut offset = 0u32;
    let entries_start = mmap_addr + 16; // пропускаем typ+size+entry_size+entry_version
    let entries_len = mmap_size - 16; 
    while offset < entries_len {
        let enteny = &*((entries_start + offset as u64) as *const MbMemoryEntry);
        if enteny.typ == 1 {
          let mut addr = enteny.base_addr;
          while addr + 4096 <= enteny.base_addr + enteny.length {
              mark_free(addr);
              addr += 4096;
          }  
        }
        offset += entry_size;
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
    0 // нет свободных страниц
}

fn mark_free(addr: u64) {
    let page = (addr / 4096) as usize;
    let idx = page / 64;
    let bit = page % 64;
    // Добавь проверку границы!
    if idx >= 32768 {
        return;
    }
    unsafe { BITMAP[idx] &= !(1u64 << bit); }
}

fn mark_used(addr: u64) {
    let page = (addr / 4096) as usize;
    let idx = page / 64;
    let bit = page % 64;
    if idx >= 32768 {
        return;
    }
    unsafe { BITMAP[idx] |= 1u64 << bit; }
}

