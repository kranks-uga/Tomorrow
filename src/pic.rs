pub fn disable() {
    unsafe {                                                                                                                                                                               
        // ICW1: инициализация                                                                                                                                                             
        outb(0x20, 0x11);
        outb(0xA0, 0x11);                                                                                                                                                                  
                  
        // ICW2: remapping (мастер → 32, слейв → 40)                                                                                                                                       
        outb(0x21, 32);
        outb(0xA1, 40);                                                                                                                                                                    
                                                                                                                                                                                             
        // ICW3
        outb(0x21, 4);   // мастер: слейв на IRQ2                                                                                                                                          
        outb(0xA1, 2);   // слейв: я на IRQ2                                                                                                                                               
   
        // ICW4: режим 8086                                                                                                                                                                
        outb(0x21, 0x01);
        outb(0xA1, 0x01);                                                                                                                                                                  
                  
        // Маскировать все IRQ                                                                                                                                                             
        outb(0x21, 0xFF);
        outb(0xA1, 0xFF);                                                                                                                                                                  
    }     
}

unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack));

}