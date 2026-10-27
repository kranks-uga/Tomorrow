// SPDX-License-Identifier: GPL-3.0-or-later
use crate::shell::parse_octal;
use alloc::{string::String, sync::Arc, vec::Vec};
use core::cmp::min;
use spin::Mutex;

pub const MAX_PATH: usize = 100;

/// Предел размера одного файла. Без него программа, пишущая в цикле, растит
/// `Vec` до исчерпания кучи (1 MB) → паника аллокатора роняет всё ядро.
pub const MAX_FILE_SIZE: usize = 64 * 1024;

pub struct Inode {
    pub data: Mutex<Vec<u8>>,
}

impl Inode {
    /// Записать `src` начиная с `offset`: существующие байты перезаписываются,
    /// при выходе за конец файл растёт (дыра между концом и `offset` — нули).
    /// Возвращает новый конец записи или `None`, если файл превысил бы
    /// `MAX_FILE_SIZE` (тогда ничего не пишется).
    pub fn write_at(&self, offset: usize, src: &[u8]) -> Option<usize> {
        let end = offset.checked_add(src.len())?;
        if end > MAX_FILE_SIZE {
            return None;
        }
        let mut data = self.data.lock();
        if data.len() < end {
            data.resize(end, 0);
        }
        data[offset..end].copy_from_slice(src);
        Some(end)
    }

    /// Прочитать в `dst` начиная с `offset`. Возвращает число прочитанных байт:
    /// меньше `dst.len()` у конца файла, 0 — если `offset` за концом.
    pub fn read_at(&self, offset: usize, dst: &mut [u8]) -> usize {
        let data = self.data.lock();
        if offset >= data.len() {
            return 0;
        }
        let n = min(dst.len(), data.len() - offset);
        dst[..n].copy_from_slice(&data[offset..offset + n]);
        n
    }
}

struct DirEntry {
    name: String,
    inode: Arc<Inode>,
}

static ROOT: Mutex<Vec<DirEntry>> = Mutex::new(Vec::new());

pub fn init() {
    let base = unsafe { crate::MOD_START };
    let mut off: u64 = 0;

    loop {
        // внешний: по файлам
        let name0 = unsafe { *((base + off) as *const u8) };
        if name0 == 0 {
            break; // пустое имя = конец архива
        }

        // --- имя: внутренний цикл ---
        let mut name_buf = [0u8; 100];
        let mut i: u64 = 0;
        loop {
            let c = unsafe { *((base + off + i) as *const u8) };
            if c == 0 || i >= 100 {
                // NUL или предел поля name
                break;
            }
            name_buf[i as usize] = c;
            i += 1;
        }
        let name = String::from_utf8_lossy(&name_buf[..i as usize]).into_owned();

        // --- размер + данные ---
        let size = parse_octal(base, off + 124, 12);
        let data_ptr = (base + off + 512) as *const u8;
        let mut data = Vec::with_capacity(size as usize);
        data.extend_from_slice(unsafe { core::slice::from_raw_parts(data_ptr, size as usize) });

        ROOT.lock().push(DirEntry {
            name,
            inode: Arc::new(Inode {
                data: Mutex::new(data),
            }),
        });

        // --- переход к следующему header ---
        off += 512 + ((size + 511) & !511);
    }
}

/// Найти inode по имени. Клонирует `Arc` и отпускает лок `ROOT` перед
/// возвратом — вызывающий работает с `inode.data` уже без него, иначе
/// лочить `inode.data.lock()`, держа `ROOT.lock()`, было бы риском дедлока.
pub fn lookup(name: &[u8]) -> Option<Arc<Inode>> {
    let root = ROOT.lock();
    root.iter()
        .find(|entry| entry.name.as_bytes() == name)
        .map(|entry| entry.inode.clone())
}

/// Перебрать файлы (для ls). Сначала клонируем список (имя + Arc) и
/// отпускаем ROOT, потом уже лочим data каждого inode по отдельности —
/// тот же порядок локов, что и в lookup.
pub fn list(mut cb: impl FnMut(&[u8], usize)) {
    let entries: Vec<(String, Arc<Inode>)> = ROOT
        .lock()
        .iter()
        .map(|entry| (entry.name.clone(), entry.inode.clone()))
        .collect();

    for (name, inode) in &entries {
        let len = inode.data.lock().len();
        cb(name.as_bytes(), len);
    }
}

pub fn write(name: &[u8], data: &[u8]) -> bool {
    match lookup(name) {
        Some(inode) => {
            let mut buf = inode.data.lock();
            buf.clear();
            buf.extend_from_slice(data);
            true
        }
        None => false,
    }
}

pub fn create(name: &[u8]) -> bool {
    let mut root = ROOT.lock();
    if root.iter().any(|entry| entry.name.as_bytes() == name) {
        return false;
    }
    root.push(DirEntry {
        name: String::from_utf8_lossy(name).into_owned(),
        inode: Arc::new(Inode {
            data: Mutex::new(Vec::new()),
        }),
    });
    true
}

pub fn delete(name: &[u8]) -> bool {
    let mut root = ROOT.lock();
    match root.iter().position(|entry| entry.name.as_bytes() == name) {
        Some(i) => {
            root.remove(i);
            true
        }
        None => false,
    }
}
