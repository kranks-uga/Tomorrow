pub struct BumpAllocator {
    pub start: u64,
    pub end: u64,
    pub next: u64,
}

pub static mut HEAP: BumpAllocator = BumpAllocator {
    start: 0,
    end: 0,
    next: 0,
};

impl BumpAllocator {
    pub fn init(&mut self, addr: u64, size: u64) {
        self.start = addr;
        self.end = addr + size;
        self.next = self.start;
    }

    pub fn alloc(&mut self, size: u64, align: u64) -> *mut u8 {
        let aligned = align_up(self.next, align);
        if aligned + size <= self.end {
            self.next = aligned + size;
            return aligned as *mut u8;
        } else {
            panic!("heap: out of memory");
        }
    }
}

fn align_up(addr: u64, align: u64) -> u64 {
    (addr + align - 1) & !(align - 1)
}
