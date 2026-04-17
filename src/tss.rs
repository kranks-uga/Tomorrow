#[repr(C, packed)]
pub struct Tss {
    reserved0: u32,
    pub rsp0: u64, // kernel stack для ring 0 ← самое важное
    pub rsp1: u64,
    pub rsp2: u64,
    reserved1: u64,
    pub ist: [u64; 7], // стеки для NMI/double fault
    reserved2: u64,
    reserved3: u16,
    pub iomap_base: u16,
}

pub static mut TSS: Tss = Tss {
    reserved0: 0,
    rsp0: 0,
    rsp1: 0,
    rsp2: 0,
    reserved1: 0,
    ist: [0; 7],
    reserved2: 0,
    reserved3: 0,
    iomap_base: 0,
};

#[repr(C, packed)]
pub struct GdtEntry {
    pub limit_low: u16,
    pub base_low: u16,
    pub base_mid: u8,
    pub access: u8,
    pub flags_limit: u8,
    pub base_high: u8,
}

#[repr(C, packed)]
pub struct TssDescriptor {
    pub low: GdtEntry,
    pub base_upper: u32,
    pub reserved: u32,
}

// GDT с поддержкой TSS
// 0x00: null, 0x08: kernel code, 0x10: kernel data,
// 0x18: user data (DPL=3), 0x20: user code (DPL=3),
// 0x28+0x30: TSS (16 байт)
static mut GDT: [u64; 7] = [
    0x0000000000000000, // 0x00: null
    0x00AF9A000000FFFF, // 0x08: kernel code  (DPL=0, L=1, D=0)
    0x00CF92000000FFFF, // 0x10: kernel data  (DPL=0, L=0, D=1)
    0x00CFF2000000FFFF, // 0x18: user data    (DPL=3, L=0, D=1) ← SS для SYSRET = 0x1B
    0x00AFFA000000FFFF, // 0x20: user code    (DPL=3, L=1, D=0) ← CS для SYSRET = 0x23
    0x0000000000000000, // 0x28: TSS low  (заполним в init)
    0x0000000000000000, // 0x30: TSS high (заполним в init)
];

#[repr(C, packed)]
struct GdtPtr {
    limit: u16,
    base: u64,
}

pub unsafe fn init(kernel_stack: u64) {
    let tss = &raw mut TSS;
    (*tss).rsp0 = kernel_stack;
    (*tss).iomap_base = core::mem::size_of::<Tss>() as u16;

    // вычисляем дескриптор TSS
    let base = &raw const TSS as u64;
    let limit = core::mem::size_of::<Tss>() as u64 - 1;

    // TSS дескриптор — 16 байт (два слота GDT)
    let gdt = &raw mut GDT;
    (*gdt)[5] = (limit & 0xFFFF)
        | ((base & 0xFFFFFF) << 16)
        | (0x89u64 << 40)        // Present + TSS type
        | (((limit >> 16) & 0xF) << 48)
        | (((base >> 24) & 0xFF) << 56);
    (*gdt)[6] = (base >> 32) & 0xFFFFFFFF;

    // загружаем новый GDT
    let gdt_ptr = GdtPtr {
        limit: (core::mem::size_of::<[u64; 7]>() - 1) as u16,
        base: core::ptr::addr_of!(GDT) as u64,
    };
    core::arch::asm!("lgdt [{}]", in(reg) &gdt_ptr);

    // загружаем TSS селектор 0x28 в TR
    core::arch::asm!("ltr ax", in("ax") 0x28u16);
}
