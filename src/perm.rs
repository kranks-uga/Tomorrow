// SPDX-License-Identifier: GPL-3.0-or-later
//! Unix-подобная DAC-проверка прав (rwx).
//!
//! Биты специально совпадают с младшими 9 битами Unix-режима:
//! r = 4, w = 2, x = 1. Те же значения использует access(2):
//! R_OK=4, W_OK=2, X_OK=1, поэтому конвертировать ничего не нужно.

pub const X: u16 = 0b001;
pub const W: u16 = 0b010;
pub const R: u16 = 0b100;

pub const RWX: u16 = R | W | X;
pub const MODE_MASK: u16 = 0o777;

const OWNER_SHIFT: u16 = 6;
const GROUP_SHIFT: u16 = 3;
const OTHER_SHIFT: u16 = 0;

/// Проверить, можно ли субъекту (`uid`, `gid`) выполнить `need`
/// над объектом с режимом `mode` и владельцем `file_uid:file_gid`.
///
/// Правила как в Unix:
/// - `uid == 0` (root) обходит DAC;
/// - иначе выбирается ровно один класс — owner, group или other;
/// - `need` должен целиком содержаться в правах этого класса.
pub fn allowed(mode: u16, file_uid: u32, file_gid: u32, uid: u32, gid: u32, need: u16) -> bool {
    if need & !RWX != 0 {
        return false;
    }
    if need == 0 {
        return true; // F_OK: проверка существования, DAC не нужен
    }
    if uid == 0 {
        return true; // root — доверенный субъект
    }

    let class = if uid == file_uid {
        (mode >> OWNER_SHIFT) & RWX
    } else if gid == file_gid {
        (mode >> GROUP_SHIFT) & RWX
    } else {
        (mode >> OTHER_SHIFT) & RWX
    };

    class & need == need
}

/// Девять символов для `ls -l`: `rwxrwxrwx`, без типа файла.
pub fn mode_string(mode: u16) -> [u8; 9] {
    let mut s = [b'-'; 9];
    for i in 0..9 {
        let bit = 1u16 << (8 - i);
        if mode & bit != 0 {
            s[i] = match i % 3 {
                0 => b'r',
                1 => b'w',
                _ => b'x',
            };
        }
    }
    s
}
