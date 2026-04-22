use crate::font::{draw_char, GLYPH_H, GLYPH_W};

pub struct Console {
    pub fb: *mut u32,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub cx: u32, // текущая колонка в символах
    pub cy: u32, // текущая строка в символах
}

unsafe impl Send for Console {}

impl Console {
    pub fn write_byte(&mut self, b: u8) {
        let max_rows = self.height / GLYPH_H;
        match b {
            b'\n' => {
                self.cx = 0;
                self.cy += 1;
            }
            _ => {
                draw_char(
                    b,
                    self.cx * GLYPH_W,
                    self.cy * GLYPH_H,
                    0xFFFFFF,
                    self.fb,
                    self.pitch,
                );
                self.cx += 1;
                if self.cx * GLYPH_W >= self.width {
                    self.cx = 0;
                    self.cy += 1;
                }
            }
        }
        if self.cy >= max_rows {
            self.clear();
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
            unsafe {
                *self.fb.add(i as usize) = 0x000000;
            }
        }
        self.cx = 0;
        self.cy = 0;
    }

    pub fn write_hex(&mut self, val: u64) {
        for i in (0..16).rev() {
            let nibble = (val >> (i * 4)) & 0xF;
            let ch = if nibble < 10 {
                b'0' + nibble as u8
            } else {
                b'a' + (nibble as u8 - 10)
            };
            self.write_byte(ch);
        }
    }

    pub fn write_dec(&mut self, val: u64) {
        if val == 0 {
            self.write_byte(b'0');
            return;
        }
        let mut buf = [0u8; 20];
        let mut len = 0;
        let mut n = val;
        while n > 0 {
            buf[len] = b'0' + (n % 10) as u8;
            len += 1;
            n /= 10;
        }
        for i in (0..len).rev() {
            self.write_byte(buf[i]);
        }
    }
}
