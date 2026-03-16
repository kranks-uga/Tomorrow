// PSF2 header: magic(4) + version(4) + headersize(4) + flags(4) + numglyph(4) + bytesperglyph(4) + height(4) + width(4)
const FONT_DATA: &[u8] = include_bytes!("font.psf");

pub fn glyph(c: u8) -> &'static [u8] {
    // headersize обычно 32, bytesperglyph = 16 для 8x16
    let header_size = u32::from_le_bytes(FONT_DATA[8..12].try_into().unwrap()) as usize;
    let bytes_per_glyph = u32::from_le_bytes(FONT_DATA[20..24].try_into().unwrap()) as usize;
    let offset = header_size + (c as usize) * bytes_per_glyph;
    &FONT_DATA[offset..offset + bytes_per_glyph]
}

pub const GLYPH_W: u32 = 8;                                                                                                                                                                
pub const GLYPH_H: u32 = 16;                                                                                                                                                               
                                                                                                                                                                                             
pub fn draw_char(c: u8, x: u32, y: u32, fg: u32, fb: *mut u32, pitch: u32) {                                                                                                               
    let g = glyph(c);
    for row in 0..GLYPH_H {
        let byte = g[row as usize];
        for col in 0..GLYPH_W {
            if byte & (0x80 >> col) != 0 {
                let px = x + col;
                let py = y + row;
                unsafe { *fb.add((py * (pitch / 4) + px) as usize) = fg; }
            }
        }
    }
}