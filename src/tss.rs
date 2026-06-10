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
//
// GDT занимает СОБСТВЕННУЮ страницу 4 КБ (align 4096 + размер = ровно страница):
// раньше она лежала в .data вплотную к SCHEDULER (0x141000 / 0x141038), и любая
// дикая запись рядом могла испортить дескриптор → #GP 0x18 при загрузке SS.
#[repr(C, align(4096))]
pub struct GdtPage {
    pub entries: [u64; 512],
}

const fn build_gdt() -> [u64; 512] {
    let mut g = [0u64; 512];
    g[1] = 0x00AF9A000000FFFF; // 0x08: kernel code (DPL=0, L=1, D=0)
    g[2] = 0x00CF92000000FFFF; // 0x10: kernel data (DPL=0, L=0, D=1)
    g[3] = 0x00CFF2000000FFFF; // 0x18: user data   (DPL=3, L=0, D=1) ← SS sysret = 0x1B
    g[4] = 0x00AFFA000000FFFF; // 0x20: user code   (DPL=3, L=1, D=0) ← CS sysret = 0x23
    // g[5], g[6] — TSS дескриптор (16 байт), заполняется в init()
    g
}

pub static mut GDT: GdtPage = GdtPage { entries: build_gdt() };

/// Текущее значение дескриптора user-data (GDT[3], phys 0x..18) — для сторожа
/// порчи GDT. Если оно отличается от 0x00CFF2000000FFFF — кто-то затёр GDT.
pub fn gdt_user_data() -> u64 {
    unsafe { core::ptr::addr_of!(GDT.entries).cast::<u64>().add(3).read() }
}

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
    (*gdt).entries[5] = (limit & 0xFFFF)
        | ((base & 0xFFFFFF) << 16)
        | (0x89u64 << 40)        // Present + TSS type
        | (((limit >> 16) & 0xF) << 48)
        | (((base >> 24) & 0xFF) << 56);
    (*gdt).entries[6] = (base >> 32) & 0xFFFFFFFF;

    // загружаем новый GDT (limit покрывает записи 0..6 включительно)
    let gdt_ptr = GdtPtr {
        limit: (7 * 8 - 1) as u16,
        base: core::ptr::addr_of!(GDT) as u64,
    };
    core::arch::asm!("lgdt [{}]", in(reg) &gdt_ptr);

    // загружаем TSS селектор 0x28 в TR
    core::arch::asm!("ltr ax", in("ax") 0x28u16);
}
