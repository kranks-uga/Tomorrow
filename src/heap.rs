use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::null_mut;

// ====================== BumpAllocator ======================

#[repr(C)]
pub struct BumpAllocator {
    inner: UnsafeCell<BumpAllocatorInner>,
}

#[repr(C)]
struct BumpAllocatorInner {
    start: u64,
    end: u64,
    next: u64,
}

unsafe impl Sync for BumpAllocator {}

impl BumpAllocator {
    pub const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(BumpAllocatorInner {
                start: 0,
                end: 0,
                next: 0,
            }),
        }
    }

    pub fn init(&self, addr: u64, size: u64) {
        let inner = unsafe { &mut *self.inner.get() };
        inner.start = addr;
        inner.end = addr + size;
        inner.next = addr;
    }

    pub fn alloc(&self, size: u64, align: u64) -> *mut u8 {
        if size == 0 {
            return null_mut();
        }
        let inner = unsafe { &mut *self.inner.get() };
        let aligned = align_up(inner.next, align);
        if aligned
            .checked_add(size)
            .map_or(true, |end| end > inner.end)
        {
            return null_mut(); // OOM
        }
        inner.next = aligned + size;
        aligned as *mut u8
    }
}

fn align_up(addr: u64, align: u64) -> u64 {
    if align <= 1 {
        return addr;
    }
    (addr + align - 1) & !(align - 1)
}

// ====================== Global Allocator ======================

#[global_allocator]
pub static HEAP: BumpAllocator = BumpAllocator::new();

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.alloc(layout.size() as u64, layout.align() as u64)
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

// ====================== Удобные функции ======================

pub fn kmalloc(size: usize) -> *mut u8 {
    HEAP.alloc(size as u64, 8)
}

pub fn kmalloc_aligned(size: usize, align: usize) -> *mut u8 {
    HEAP.alloc(size as u64, align as u64)
}
