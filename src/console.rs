use crate::font::{draw_char, GLYPH_W, GLYPH_H};                                                                                                                                            
                                                                                                                                                                                             
pub struct Console {                                                                                                                                                                       
    pub fb: *mut u32,                                                                                                                                                                      
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub cx: u32,  // текущая колонка в символах
    pub cy: u32,  // текущая строка в символах
}

unsafe impl Send for Console {}

impl Console {
    pub fn write_byte(&mut self, b: u8) {
        match b {
            b'\n' => {
                self.cx = 0;
                self.cy += 1;
            }
            _ => {
                draw_char(b, self.cx * GLYPH_W, self.cy * GLYPH_H, 0xFFFFFF, self.fb, self.pitch);
                self.cx += 1;
                if self.cx * GLYPH_W >= self.width {
                    self.cx = 0;
                    self.cy += 1;
                }
            }
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for b in s.bytes() {
            self.write_byte(b);
        }
    }
    pub fn clear(&mut self) {                                                                                                                                                                  
        let pixels = self.pitch / 4 * self.height;
        for i in 0..pixels {
            unsafe { *self.fb.add(i as usize) = 0x000000; }
        }
    }
    pub fn write_hex(&mut self, val: u64) {
        for i in (0..16).rev() {
            let nibble = (val >> (i * 4)) & 0xF;
            let ch = if nibble < 10 { b'0' + nibble as u8 } else { b'a' + (nibble as u8 - 10) };
            self.write_byte(ch);
        }
    }
}