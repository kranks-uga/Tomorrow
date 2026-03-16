fn read(base: u64, reg: u32) -> u32 {                                                                                                                                                         
    unsafe {                                                                                                                                                                                  
        core::ptr::write_volatile(base as *mut u32, reg);                                                                                                                                     
        core::ptr::read_volatile((base + 0x10) as *const u32)                                                                                                                                 
    }                                                                                                                                                                                         
}                                                                                                                                                                                             
                                                                                                                                                                                                
fn write(base: u64, reg: u32, val: u32) {                                                                                                                                                     
    unsafe {
        core::ptr::write_volatile(base as *mut u32, reg);                                                                                                                                     
        core::ptr::write_volatile((base + 0x10) as *mut u32, val);
    }                                                                                                                                                                                         
}

pub fn redirect(base: u64, irq: u8, vector: u8, apic_id: u8) {
    let reg = 0x10 + (irq as u32) * 2;                                                                                                                                                        
    let low = vector as u32;                                                                                                           
    let high = (apic_id as u32) << 24;                                                                                                                                
    write(base, reg, low);                                                                                                                                                                    
    write(base, reg + 1, high);
}