<p align="center">
  <img src="docs/banner.png" alt="Tomorrow OS" width="100%">
</p>

# Tomorrow OS

**English** | [Русский](README.ru.md)

A secure x86-64 operating system written in Rust (no_std), with a mandatory access control model, compartment-based process isolation, and Linux ABI compatibility.

**Original idea by:** Kirill Repin

---

## Concept

Most operating systems check permissions at login, and after that a process keeps its privileges "forever". Tomorrow is built on a different principle: **every action is checked individually**.

### Access rights

Rights are stored in the kernel and are bound to both sides at once:

- **subject** — the user or process making a request
- **object** — the file, device, or resource being accessed

An action is allowed only if the subject's rights and the object's permitted rights overlap. This is similar to mandatory access control (MAC), but implemented at the kernel level with explicit masks for every entity.

### Compartment isolation

Processes are grouped into **compartments** — isolated groups that cannot see or directly affect each other. They interact with the rest of the system only through **brokers**: special intermediary processes that are explicitly defined in the kernel and check every request.

```
 ┌─────────────────┐     ┌─────────────────┐
 │  Compartment A  │     │  Compartment B  │
 │  proc1  proc2   │     │  proc3  proc4   │
 └────────┬────────┘     └─────────┬───────┘
          │    only via a broker   │
          └──────────┬─────────────┘
                ┌────▼─────┐
                │  Broker  │  ← checks rights,
                │          │    passes through only
                └──────────┘    permitted data
```

### Linux ABI compatibility

System call numbers match Linux x86-64. The goal is to run existing Linux binaries without recompilation, while enforcing Tomorrow's own security model in the kernel.

---

## Features

| Component | Status |
|-----------|--------|
| Multiboot2 boot (GRUB2) | done |
| Higher-half kernel (`0xFFFF800000000000`) | done |
| PMM — physical page bitmap (up to 8 GB) | done |
| VMM — 4-level page tables (4 KB pages) | done |
| Heap — free-list allocator (first-fit) | done |
| IDT + APIC (Local + IO APIC via ACPI MADT) | done |
| TSS + GDT (kernel/user segments) | done |
| Round-robin scheduler (up to 64 processes) | done |
| Timer-interrupt context switch | done |
| Ring-3 userspace (iretq) | done |
| SYSCALL/SYSRET (MSR-based) | done |
| PS/2 keyboard | done |
| Framebuffer console + PSF font | done |
| PCI/PCIe enumeration (MCFG) | done |
| xHCI USB (driver + USB keyboard) | done |
| Interactive shell | done |
| Process management from the shell (spawn/kill) | done |
| initrd — modules loaded by GRUB | done |
| RAM FS (ramfs) — `Arc`-based inodes, file creation and writing | done |
| File syscalls — per-process fd table, open/read/close | done |
| ACPI shutdown / reboot | done |
| Serial COM1 — boot log and panics over UART | done |

### Shell commands

| Command | Description |
|---------|-------------|
| `help` | list commands |
| `clear` / `cls` | clear the screen |
| `echo <text>` | print text |
| `ticks` | show the timer tick counter |
| `ps` | list processes |
| `spawn <a\|b\|f>` | start a demo process (`f` reads `hello.txt` via syscalls) |
| `kill <pid>` | terminate a process |
| `mem` | memory statistics |
| `module` | show loaded modules |
| `ls` | list RAM FS contents |
| `cat <file>` | print a file |
| `write <file> <data>` | write data to a file |
| `touch <file>` | create an empty file |
| `rm <file>` | delete a file |
| `reboot` | restart the system |
| `shutdown` / `poweroff` | power off (ACPI) |

### System calls

| # | Name | Arguments / description |
|---|------|-------------------------|
| 0 | `SYS_READ` | fd, buf, len — read from a file, 0 = end of file |
| 1 | `SYS_WRITE` | fd, buf, len — fd 1 = console |
| 2 | `SYS_OPEN` | path, len — open a ramfs file, returns an fd |
| 3 | `SYS_CLOSE` | fd — close a file |
| 24 | `SYS_YIELD` | yield the CPU |
| 60 | `SYS_EXIT` | terminate the process |

Errors are returned Linux-style as a negative `-errno` (`EBADF`, `ENOENT`, `EFAULT`, …).
Each process may only use the syscalls allowed by its mask (`syscall_mask`).

---

## Requirements

- Linux (Ubuntu/Debian, Arch, Fedora)
- Rust nightly
- `grub-mkrescue`, `xorriso`, `gcc`, `nasm`
- `qemu-system-x86_64` + OVMF (UEFI firmware) — to run in an emulator

Automatic setup:
```bash
./setup.sh
```

---

## Building

```bash
./make.sh
```

Builds the ELF kernel with `cargo` and packs it into a bootable ISO image, `tomorrow.iso`.

If Rust is installed from your distribution's packages (stable, no rustup), `make.sh`
sets `RUSTC_BOOTSTRAP=1` automatically to allow the nightly `-Zbuild-std` flags.

### Running in QEMU

```bash
./run.sh
```

The script starts QEMU with UEFI (OVMF) and the q35 chipset:

```bash
qemu-system-x86_64 \
    -machine q35 \
    -bios /usr/share/edk2/x64/OVMF.4m.fd \
    -cdrom tomorrow.iso \
    -m 256M \
    -serial stdio
```

**Why UEFI is required:** the kernel enables the APIC via the XSDT table (ACPI 2.0).
SeaBIOS, which QEMU uses by default, only provides ACPI 1.0 — the APIC
is never enabled, there are no interrupts, and the keyboard and timer don't work.
`q35` is needed for PCIe (the MCFG table) — without it xHCI is not found.

If QEMU was built without GTK, `run.sh` exposes the screen over VNC:
`vncviewer ::1:5900`. For a native window, install `qemu-ui-gtk` (Arch/Fedora)
or `qemu-system-gui` (Debian/Ubuntu).

### Writing to real hardware

```bash
./load.sh
```

The script lists your disks and writes `tomorrow.iso` with `dd`.
**Double-check the disk name** — the selected disk is overwritten completely.

---

## Project layout

```
src/
  boot.s           — 32→64-bit startup, page tables, higher half
  boot.rs          — includes boot.s via global_asm!
  main.rs          — kernel_main, Multiboot2/ACPI parsing, scheduler setup
  pmm.rs           — physical memory manager
  vmm.rs           — virtual memory manager
  heap.rs          — free-list heap allocator
  idt.rs           — interrupt descriptor table
  lapic.rs         — Local APIC
  ioapic.rs        — IO APIC
  pic.rs           — disables the 8259 PIC
  hpet.rs          — HPET timer
  tss.rs           — TSS + GDT
  process.rs       — process and context structures, fd table
  scheduler.rs     — round-robin scheduler
  syscall.rs       — system call handlers
  syscall_entry.s  — SYSCALL entry/exit (MSR)
  timer.s          — timer interrupt handler, context switch
  switch.s         — context_switch (in-kernel switch)
  keyboard.rs      — PS/2 keyboard
  keyboard.s       — keyboard ISR
  console.rs       — framebuffer console + line discipline
  font.rs / font.psf — PSF font
  pci.rs           — finds xHCI via MCFG/PCIe
  xhci.rs          — xHCI USB controller and HID keyboard
  shell.rs         — interactive shell
  serial.rs        — COM1 (16550 UART): boot log, panics, unknown syscall
  ramfs.rs         — RAM file system
boot/grub/grub.cfg — bootloader config (kernel + initrd)
initrd.tar         — initial RAM disk with user modules
md/                — detailed subsystem docs (see "Documentation")
```

---

## Documentation

Detailed write-ups of each subsystem live in [`md/`](md/) (currently in Russian):

| Document | Topic |
|----------|-------|
| [`architecture.md`](md/architecture.md) | the whole kernel: boot, PMM/VMM/heap, IDT, APIC timer, TSS/GDT, processes, scheduler, syscalls |
| [`xhci.md`](md/xhci.md) | USB stack: xHCI controller, device enumeration, hubs/TT, HID keyboard |
| [`ramfs.md`](md/ramfs.md) | file system: initrd (tar) → heap, writable ramfs, `ls/cat/write/touch/rm` commands |
| [`console.md`](md/console.md) | screen output: framebuffer, PSF2 font rendering, the `Console` API |
| [`shell.md`](md/shell.md) | input and shell: keyboard (PS/2 + USB), `on_char`, line discipline, command dispatch |
| [`serial.md`](md/serial.md) | COM1: UART setup, `sprint!`/`sprintln!`, boot log and panics, reading the log |

---

## Architecture

A full description of all subsystems is in [`md/architecture.md`](md/architecture.md).

Virtual memory layout:

```
0xFFFF_FFFF_FFFF_FFFF  ┐
                       │  kernel space
0xFFFF_8000_0000_0000 ─┤  kernel, stacks, heap
       ...             │  non-canonical hole
0x0000_0200_0000_0000 ─┤  user code
0x0000_0100_0000_0000 ─┤  user stack
0x0000_0000_0000_0000  ┘
```

## License

Tomorrow OS is licensed under the [GNU GPL v3.0 or later](LICENSE) (`GPL-3.0-or-later`).
