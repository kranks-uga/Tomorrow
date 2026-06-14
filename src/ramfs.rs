use crate::shell::parse_octal;
use alloc::vec::Vec;

pub struct File {
    pub name: [u8; 100],
    pub data: Vec<u8>,
}

static mut FILES: Vec<File> = Vec::new();

pub fn init() {
    let base = unsafe { crate::MOD_START };
    let mut off: u64 = 0;

    loop {
        // внешний: по файлам
        let name0 = unsafe { *((base + off) as *const u8) };
        if name0 == 0 {
            break; // пустое имя = конец архива
        }

        let mut file = File {
            name: [0u8; 100],
            data: Vec::new(),
        };

        // --- печать имени: внутренний цикл ---
        let mut i: u64 = 0;
        loop {
            let c = unsafe { *((base + off + i) as *const u8) };
            if c == 0 || i >= 100 {
                // NUL или предел поля name
                break;
            }
            file.name[i as usize] = c;
            i += 1;
        }

        // --- размер ---
        let size = parse_octal(base, off + 124, 12);
        let data_ptr = unsafe { (base + off + 512) as *const u8 };
        file.data = Vec::with_capacity(size as usize);
        file.data.extend_from_slice(unsafe {
            core::slice::from_raw_parts(data_ptr, size as usize)
        });

        unsafe {
            FILES.push(file);
        }

        // --- переход к следующему header ---
        off += 512 + ((size + 511) & !511);
    }
}

/// Перебрать файлы (для ls)
pub fn list(mut cb: impl FnMut(&[u8], usize)) {
    unsafe {
        for f in FILES.iter() {
            let len = f.name.iter().position(|&b| b == 0).unwrap_or(100);
            cb(&f.name[..len], f.data.len());
        }
    }
}

/// Найти файл по имени (для cat):
pub fn find(name: &[u8]) -> Option<&'static [u8]> {
    unsafe {
        for f in FILES.iter() {
            let len = f.name.iter().position(|&b| b == 0).unwrap_or(100);
            if &f.name[..len] == name {
                return Some(&f.data);
            }
        }
    }
    None
}

pub fn write(name: &[u8], data: &[u8]) -> bool {
    unsafe {
        for i in 0..FILES.len() {
            let file = &mut FILES[i];
            let len = file.name.iter().position(|&b| b == 0).unwrap_or(100);
            if &file.name[..len] == name {
                file.data.clear();
                file.data.extend_from_slice(data);

                return true;
            }
        }
        return false;
    };
}

pub fn create(name: &[u8]) -> bool {
    unsafe {
        for i in 0..FILES.len() {
            let len = FILES[i].name.iter().position(|&b| b == 0).unwrap_or(100);
            if &FILES[i].name[..len] == name {
                return false;
            }
        }
    };
    let mut file: File = File {
        name: [0u8; 100],
        data: Vec::new(),
    };
    let copy_len = name.len().min(99);
    file.name[..copy_len].copy_from_slice(&name[..copy_len]);
    unsafe {
        FILES.push(file);
    };
    return true;
}
