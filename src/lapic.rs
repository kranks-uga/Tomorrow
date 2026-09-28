// SPDX-License-Identifier: GPL-3.0-or-later
fn write(base: u64, reg: u32, val: u32) {
    unsafe{
        core::ptr::write_volatile((base + reg as u64) as *mut u32, val);
    }
}

pub fn enable(base: u64) {
    write(base, 0xF0, 0x1FF); // Enable LAPIC
}

pub fn eoi(lapic_base: u64) {
    write(lapic_base, 0xB0, 0);
}