#[repr(C)]
struct Hpet {
    capabilities: u64,
    reserved_0: u64,
    general_configuration: u64,
    reserved_1: u64,
    general_interrupt_status: u64,
    reserved_2: [u64; 25],
    main_counter: u64,
}


pub unsafe fn init_hpet(hpet_base: u64) -> u64 {
    let hpet = &mut *(hpet_base as *mut Hpet);                                                                                                                                                
    let gcap = core::ptr::read_volatile(&hpet.capabilities);
    let period_fs = gcap >> 32;                                                                                                                                                               
    let conf = core::ptr::read_volatile(&hpet.general_configuration);
    core::ptr::write_volatile(&mut hpet.general_configuration, conf | 1);                                                                                                                     
    period_fs   
}



pub unsafe fn read_counter(hpet_base: u64) -> u64 {                                                                                                                                           
    let hpet = &*(hpet_base as *const Hpet);                                                                                                                                                  
    core::ptr::read_volatile(&hpet.main_counter)                                                                                                                                              
} 