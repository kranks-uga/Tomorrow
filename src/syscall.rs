unsafe fn write_msr(msr: u32, value: u64) {
    let low = value as u32; // младшие 32 бита → eax
    let high = (value >> 32) as u32; // старшие 32 бита → edx
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") low,
        in("edx") high,
    );
}

const MSR_STAR: u32 = 0xC0000081;
const MSR_LSTAR: u32 = 0xC0000082;
const MSR_SYSCALL_MASK: u32 = 0xC0000084;

unsafe extern "C" fn handler() -> ! {
    loop {}
}

pub fn init() {
    unsafe {
        write_msr(MSR_LSTAR, handler as u64);
        write_msr(MSR_STAR, 0x0013_0008_0000_0000);
        write_msr(MSR_SYSCALL_MASK, 0x200);
    }
}
