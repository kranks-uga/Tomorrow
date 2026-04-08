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
// 0x00: null, 0x08: code, 0x10: data, 0x18+0x20: TSS (16 байт)
static mut GDT: [u64; 5] = [
    0x0000000000000000, // null
    0x00AF9A000000FFFF, // 0x08: kernel code
    0x00AF92000000FFFF, // 0x10: kernel data
    0x0000000000000000, // 0x18: TSS low  (заполним в init)
    0x0000000000000000, // 0x20: TSS high (заполним в init)
];

#[repr(C, packed)]
struct GdtPtr {
    limit: u16,
    base: u64,
}

pub unsafe fn init(kernel_stack: u64) {
    TSS.rsp0 = kernel_stack;
    TSS.iomap_base = core::mem::size_of::<Tss>() as u16;

    // вычисляем дескриптор TSS
    let base = &TSS as *const Tss as u64;
    let limit = core::mem::size_of::<Tss>() as u64 - 1;

    // TSS дескриптор — 16 байт (два слота GDT)
    GDT[3] = (limit & 0xFFFF)
        | ((base & 0xFFFFFF) << 16)
        | (0x89u64 << 40)        // Present + TSS type
        | (((limit >> 16) & 0xF) << 48)
        | (((base >> 24) & 0xFF) << 56);
    GDT[4] = (base >> 32) & 0xFFFFFFFF;

    // загружаем новый GDT
    let gdt_ptr = GdtPtr {
        limit: (core::mem::size_of::<[u64; 5]>() - 1) as u16,
        base: GDT.as_ptr() as u64,
    };
    core::arch::asm!("lgdt [{}]", in(reg) &gdt_ptr);

    // загружаем TSS селектор 0x18 в TR
    core::arch::asm!("ltr ax", in("ax") 0x18u16);
}
