// SPDX-License-Identifier: GPL-3.0-or-later
use core::arch::global_asm;

global_asm!(include_str!("boot.s"));