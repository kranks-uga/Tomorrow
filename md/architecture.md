# Tomorrow OS — архитектура

## Оглавление
1. [Загрузка (Boot)](#1-загрузка)
2. [Виртуальная память — раскладка](#2-виртуальная-память--раскладка)
3. [PMM — физическая память](#3-pmm--физическая-память)
4. [VMM — виртуальная память](#4-vmm--виртуальная-память)
5. [Heap](#5-heap)
6. [IDT — прерывания](#6-idt--прерывания)
7. [APIC — таймер](#7-apic--таймер)
8. [TSS и GDT](#8-tss-и-gdt)
9. [Процессы](#9-процессы)
10. [Планировщик](#10-планировщик)
11. [Syscall](#11-syscall)
12. [Поток выполнения от старта до userspace](#12-поток-выполнения)

---

## 1. Загрузка

**Файл:** `src/boot.s`

GRUB загружает ядро по протоколу Multiboot2. Управление передаётся в `start32` (32-бит).

```
start32 (32-bit)
  ├── проверяет magic (0x36D76289)
  ├── настраивает временный стек (stack_top, физ. адрес)
  ├── обнуляет page tables (pml4, pdpt0, pdpt1, pdpt_high, pd_kernel)
  ├── строит page tables:
  │     PML4[0]   → pdpt0      (identity map 0–512 GB, 1GB страницы)
  │     PML4[1]   → pdpt1      (identity map 512GB–1TB, 1GB страницы)
  │     PML4[256] → pdpt_high  (higher half 0xFFFF800000000000)
  │       pdpt_high[0] → pd_kernel
  │         pd_kernel[0] → физ. 0x0, 2MB страница (покрывает ядро)
  ├── включает PAE, CR3 = &pml4, LME, Paging
  ├── lgdt временной GDT (только null + kernel code + kernel data)
  └── far jump → start64 (физ. адрес, 64-bit сегмент 0x08)

start64 (64-bit, физ. адреса)
  ├── загружает сегменты DS/ES/FS/GS/SS = 0x10
  └── abs jump → start64_high (higher half)

start64_high (higher half)
  ├── переключает RSP на higher-half стек
  └── call kernel_main(boot_info)
```

### Что даёт boot.s

| Диапазон виртуальных адресов | Что там |
|---|---|
| `0x0000_0000_0000_0000` – `0x0000_007F_FFFF_FFFF` | Identity map (0–512 GB), 1GB страницы |
| `0x0000_0080_0000_0000` – `0x0000_00FF_FFFF_FFFF` | Identity map (512GB–1TB) |
| `0xFFFF_8000_0000_0000` – `0xFFFF_8000_001F_FFFF` | Higher half ядра (2MB страница → физ. 0x0) |

---

## 2. Виртуальная память — раскладка

```
0xFFFF_FFFF_FFFF_FFFF ┐
                       │  kernel space (недоступно user)
0xFFFF_8000_0000_0000 ─┤  ← KERNEL_VIRT
                       │     ядро, стеки процессов, heap
                       │
       ...             │  non-canonical hole
                       │
0x0000_0200_0001_0000 ─┤
0x0000_0200_0000_0000 ─┤  ← user code (маппится физ. страница process_a)
                       │
0x0000_0100_0001_0000 ─┤
0x0000_0100_0000_0000 ─┤  ← user stack (4KB, PAGE_USER)
                       │
0x0000_0000_0000_0000  ┘
```

Физический адрес ↔ виртуальный:
- kernel: `virt = phys + 0xFFFF_8000_0000_0000`
- identity: `virt = phys` (работает для phys < 1 TB)

---

## 3. PMM — физическая память

**Файл:** `src/pmm.rs`

Менеджер физических страниц (4 KB каждая).

### Структура

```
BITMAP: [u64; 32768]   // 32768 × 64 бит = 2 097 152 страниц = 8 GB адресного пространства
                       // бит = 1 → страница занята
                       // бит = 0 → страница свободна
```

По умолчанию все биты = 1 (всё занято). `init()` размечает свободные страницы из Multiboot2 memory map (тег 6), потом снова помечает занятыми страницы ядра (`_kernel_start`..`_kernel_end`).

### API

```rust
pmm::init(mmap_addr, mmap_size, entry_size)  // вызвать один раз при старте
pmm::alloc() -> u64                          // вернуть физ. адрес свободной страницы
                                             // panic если памяти нет
```

### Алгоритм alloc

```
for idx in 0..32768:
    if BITMAP[idx] != 0xFFFF_FFFF_FFFF_FFFF:  // есть хотя бы один свободный бит
        bit = trailing_ones(BITMAP[idx])        // первый свободный бит
        addr = (idx * 64 + bit) * 4096
        mark_used(addr)
        return addr
```

**Ограничения текущей реализации:**
- Нет `free()` — страницы только выделяются, никогда не освобождаются
- Нет SMP-защиты (один CPU — норм)

---

## 4. VMM — виртуальная память

**Файл:** `src/vmm.rs`

Работает поверх PMM. Управляет x86-64 четырёхуровневыми таблицами страниц (4KB листья).

### Структура

```
PML4 (CR3) → PDPT → PD → PT → физ. страница
 512 записей   512    512   512
```

Каждая запись — 8 байт, биты флагов:
| Бит | Константа | Значение |
|-----|-----------|---------|
| 0 | `PAGE_PRESENT` | страница существует |
| 1 | `PAGE_WRITABLE` | можно писать |
| 2 | `PAGE_USER` | доступна из ring-3 |
| 63 | `PAGE_NO_EXECUTE` | NX-бит |

### API

```rust
pml4.map(virt, phys, flags)  // замапить одну страницу
```

**Важно:** флаг `PAGE_USER` пробрасывается на все промежуточные уровни (PML4→PDPT→PD), иначе CPU заблокирует доступ из ring-3 при обходе таблиц.

### Как получить pml4

```rust
extern "C" { static pml4: vmm::PageTable; }   // линкер-символ из boot.s (физ. адрес)
let ptr = &pml4 as *const _ as *mut vmm::PageTable;
(*ptr).map(virt, phys, flags);
```

---

## 5. Heap

**Файл:** `src/heap.rs`

Простой bump-allocator. Выделяет память линейно — только вперёд, освобождения нет.

```
HEAP.start ──→ [занято][занято][занято]...[свободно...]  ←── HEAP.end
                                           ↑
                                        HEAP.next
```

Инициализация в `kernel_main`:
```rust
let heap_phys = pmm::alloc();
let heap_virt = heap_phys + KERNEL_VIRT;
HEAP.init(heap_virt, 4096 * 16);  // 64 KB
```

**Текущий статус:** heap выделен, но пока нигде не используется (процессы используют pmm::alloc напрямую).

---

## 6. IDT — прерывания

**Файл:** `src/idt.rs`

256 дескрипторов прерываний. Каждый — 16 байт:
```
[offset_low:16][selector:16][ist:8][flags:8][offset_mid:16][offset_high:32][reserved:32]
```

`flags = 0x8E` → Present + Interrupt Gate + DPL=0 (только из кольца 0).

### Инициализация

```
idt::init()
  ├── все 256 векторов → spurious_handler (просто iretq)
  ├── lidt
  └── после: set_handler(0xFF, spurious_handler)  // APIC spurious vector
             set_handler(0x20, timer_handler_asm)  // таймер
```

---

## 7. APIC — таймер

**Файлы:** `src/lapic.rs`, `src/ioapic.rs`, `src/pic.rs`

1. `pic::disable()` — маскирует старый PIC 8259 (иначе будет конфликт с APIC)
2. `lapic::enable(base)` — включает Local APIC (запись 0x1FF в регистр 0xF0)
3. `ioapic::redirect(base, gsi, vector=0x20, apic_id=0)` — IO APIC перенаправляет IRQ таймера на вектор 0x20
4. `sti` — разрешаем прерывания

Адреса APIC и GSI таймера берутся из ACPI MADT таблицы при старте.

### timer_handler_asm (src/timer.s)

```
прерывание 0x20 →
  timer_handler_asm:
    push 15 регистров (r15..rax)
    rdi = rsp           → указатель на SavedRegs
    call timer_do_switch → rax = *Context нового процесса (или null)

    if rax == null:
      .restore:
        pop 15 регистров
        if CS.RPL != 0:              ← возврат в ring-3?
          SS[rsp+32] |= 3            ← фикс SS.RPL=3 (AMD sysretq quirk)
        iretq                        ← возврат в тот же процесс

    else (context switch):
      rbx = rax (→ Context нового процесса)
      rsp = Context.kernel_stack         ← переключаемся на стек нового процесса
      строим iretq-фрейм: ss, rsp, rflags, cs, rip  ← всё из Context
      восстанавливаем регистры из Context
      iretq                              ← прыжок в новый процесс
```

---

## 8. TSS и GDT

**Файл:** `src/tss.rs`

После `tss::init()` GDT выглядит так:

| Selector | Описание | DPL |
|----------|----------|-----|
| `0x00` | null | — |
| `0x08` | kernel code (64-bit) | 0 |
| `0x10` | kernel data | 0 |
| `0x18` | user data | 3 |
| `0x20` | user code (64-bit) | 3 |
| `0x28` | TSS (low 8 байт) | — |
| `0x30` | TSS (high 8 байт) | — |

**TSS** используется в двух случаях:
1. **Аппаратное прерывание в ring-3** — CPU читает `TSS.rsp0` и переключает RSP на ядерный стек
2. **Переключение процесса** — `timer_do_switch` обновляет `TSS.rsp0 = новый_процесс.kernel_stack`

Для SYSCALL/SYSRET CPU НЕ использует TSS — стек переключается вручную в `syscall_entry.s`.

---

## 9. Процессы

**Файл:** `src/process.rs`

### Структура Process

```rust
Process {
    pid, syscall_mask, domain, token,
    state: ProcessState,      // Running / Blocked / Dead
    kernel_stack: u64,        // вершина ядерного стека (для TSS.rsp0)
    user_stack: u64,          // вершина пользовательского стека
    context: Context,         // сохранённые регистры
}
```

### Структура Context (offsets для .s файлов)

```
0x00  rax        0x40  r8
0x08  rbx        0x48  r9
0x10  rcx        0x50  r10
0x18  rdx        0x58  r11
0x20  rsi        0x60  r12
0x28  rdi        0x68  r13
0x30  rbp        0x70  r14
0x38  rsp        0x78  r15
0x80  rip        0x90  cs
0x88  rflags     0x98  ss
0xA0  cr3
0xA8  kernel_stack
```

### Process::new(pid, syscall_mask, domain, entry)

```
1. pmm::alloc() → kernel_stack страница (физ.)
2. pmm::alloc() → user_stack страница (физ.)
3. Маппим user_stack по вирт. 0x0000_0100_0000_0000 с PAGE_USER
4. Вычисляем fn_phys = (entry - KERNEL_VIRT) & !0xFFF
5. Маппим fn_phys по вирт. 0x0000_0200_0000_0000 с PAGE_USER
6. Context.rip    = 0x0200_0000_0000 + (entry & 0xFFF)
7. Context.rsp    = user_stack_top (0x0100_0001_0000)
8. Context.rflags = 0x202  (IF=1)
9. Context.cs     = 0x23   (user code, DPL=3)
10. Context.ss    = 0x1B   (user data, DPL=3)
11. Context.cr3   = текущий CR3
```

---

## 10. Планировщик

**Файл:** `src/scheduler.rs`

Round-robin, максимум 64 процесса.

```rust
SCHEDULER: Scheduler {
    processes: [Option<Process>; 64],
    current: usize,    // индекс текущего процесса
    count: usize,
}
```

### Переключение контекста (timer_do_switch в main.rs)

Вызывается из `timer_handler_asm` каждые 5 тиков (≈ 5ms при 1000Hz таймере):

```
1. TICKS += 1
2. lapic::eoi()           ← сообщаем APIC что обработали
3. if !SCHEDULER_READY или TICKS % 5 != 0 → return null
4. Сохраняем регистры текущего процесса из SavedRegs + iretq-фрейма в Context
5. Ищем следующий Running процесс (round-robin)
6. TSS.rsp0 = новый_процесс.kernel_stack
7. return &новый_процесс.context
```

Если возвращён null — `timer.s` просто восстанавливает регистры и делает iretq в тот же процесс.
Если возвращён Context — `timer.s` переключается на новый процесс через iretq.

### start_first_process_ring3

Запускает первый процесс в ring-3 через `iretq`:
```asm
push 0x1B           // SS  (user data, DPL=3)
push user_rsp       // RSP пользователя
push 0x202          // RFLAGS (IF=1)
push 0x23           // CS  (user code, DPL=3)
push context.rip    // RIP нового процесса
iretq               // → ring-3
```

> **Примечание AMD:** после `sysretq` SS.RPL может не выставляться как 3.
> В `timer.s` применяется фикс: `or WORD PTR [rsp+32], 3` перед `iretq` в `.restore:` —
> принудительно выставляет RPL=3 в SS фрейма при возврате в ring-3.

---

## 11. Syscall

**Файлы:** `src/syscall.rs`, `src/syscall_entry.s`

### Инициализация MSR

```
EFER.SCE = 1             ← разрешить инструкцию SYSCALL
MSR_LSTAR = syscall_entry  ← адрес обработчика
MSR_STAR  = 0x0010_0008_0000_0000
              ├── [47:32] = 0x0008 → SYSCALL: kernel CS=0x08, SS=0x10
              └── [63:48] = 0x0010 → SYSRET:  user CS=0x0010+16|3=0x23
                                               user SS=0x0010+8|3=0x1B
MSR_SYSCALL_MASK = 0x200  ← сбросить IF при входе
```

### syscall_entry.s — путь syscall

```
ring-3 → SYSCALL:
  RCX = return RIP, R11 = RFLAGS, RSP = user RSP, IF = 0

syscall_entry:
  1. user_rsp_tmp = RSP          ← сохраняем user RSP
  2. RSP = SYSCALL_KERNEL_RSP    ← ядерный стек
  3. push user_rsp, R11, RCX     ← для SYSRETQ
  4. push rbx, rbp, r12-r15      ← callee-saved
  5. переупаковываем аргументы:
       Linux ABI:  rax=nr, rdi, rsi, rdx, r10, r8
       C ABI:      rdi=nr, rsi, rdx, rcx, r8,  r9
  6. call syscall_handler
  7. pop r15-r12, rbp, rbx
  8. pop rcx (user RIP), r11 (user RFLAGS), rsp (user RSP)
  9. sysretq → ring-3
```

### Таблица системных вызовов

| Номер | Название | Аргументы |
|-------|----------|-----------|
| 0 | SYS_READ | fd, buf, len |
| 1 | SYS_WRITE | fd, buf, len |
| 2 | SYS_OPEN | — |
| 3 | SYS_CLOSE | — |
| 24 | SYS_YIELD | — |
| 60 | SYS_EXIT | code |

---

## 12. Поток выполнения

### Старт ядра

```
GRUB → start32 → start64 → start64_high → kernel_main
  ├── Console (framebuffer)
  ├── IDT::init
  ├── PIC::disable
  ├── PMM::init (читает memory map из multiboot2)
  ├── TSS::init (новый GDT с дескриптором TSS, ltr 0x28)
  ├── Heap::init
  ├── VMM: маппим доп. страницы (в т.ч. страницу кода process_a с PAGE_USER)
  ├── Syscall::init (MSR)
  ├── ACPI: парсим XSDT → MADT → HPET
  ├── LAPIC::enable, IO APIC redirect, sti
  ├── Process::new(process_a) → SCHEDULER.add_process
  ├── kprint!("Scheduler ok")
  └── start_first_process_ring3 → iretq → process_a в ring-3
```

> `SCHEDULER_READY` пока = false → таймер не переключает процессы, только делает EOI.

### Работа process_a

```
process_a (ring-3, naked fn):
  syscall(SYS_WRITE, fd=1, buf="U3\n", len=3)  ← один раз при старте
    → syscall_entry (ring-0)
    → syscall_handler → sys_write → Console::write_str("U3\n")
    → sysretq → ring-3
  jmp .   ← бесконечный цикл (больше syscall не вызывает)

каждые N тиков таймера:
  прерывание 0x20 → timer_handler_asm
    → timer_do_switch (SCHEDULER_READY=false → EOI, return null)
    → .restore: fix SS.RPL, iretq обратно в process_a
```
