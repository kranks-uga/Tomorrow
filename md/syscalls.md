# Системные вызовы (`src/syscall.rs`)

Справочник по ABI системных вызовов Tomorrow OS: как вызывать, что
возвращается, какие ошибки бывают и как устроены права на файлы.
Механика входа в ядро (MSR, `syscall_entry.s`) описана в
[architecture.md, раздел 11](architecture.md#11-syscall), модель прав —
в разделе 5 ниже.

**Конвенция одной строкой:** номера syscall'ов и способ возврата ошибок —
как в Linux x86-64; сигнатуры части вызовов — свои (см. таблицу ниже).

---

## 1. Как вызывать

```
rax = номер syscall
rdi = arg1, rsi = arg2, rdx = arg3, r10 = arg4, r8 = arg5
syscall
rax = результат
```

Регистры после возврата:

| Регистр | Состояние |
|---|---|
| `rax` | результат (см. раздел 2) |
| `rcx`, `r11` | испорчены — их использует сама инструкция `SYSCALL` (RIP и RFLAGS) |
| `rbx`, `rbp`, `r12`–`r15`, `rsp` | сохраняются |
| `rdi`, `rsi`, `rdx`, `r8`–`r10` | **могут быть испорчены** |

> Отличие от Linux: Linux сохраняет все регистры, кроме `rax`, `rcx`, `r11`.
> `syscall_entry.s` сохраняет только callee-saved (по C ABI), а
> `syscall_handler` на Rust волен портить аргументные регистры. Поэтому
> значения, нужные после `syscall`, держи в `rbx`/`r12`–`r15` (так делают
> `process_f`/`process_w`/`process_x` в `main.rs`).

---

## 2. Возврат и ошибки

Результат — одно 64-битное число в `rax`:

- **успех:** неотрицательное значение (fd, число байт, 0);
- **ошибка:** `-errno`, записанный как `u64`. Значения от `-4095` до `-1`,
  т.е. `0xFFFF_FFFF_FFFF_F001` … `0xFFFF_FFFF_FFFF_FFFF`.

Проверка в user-программе: `test rax, rax` / `js ошибка` — ошибка, если
`rax` как знаковое число отрицательно; `-rax` — её код.

В ядре код ошибки собирается хелпером `err(e)` = `(-(e as i64)) as u64`.

### Таблица errno

Номера — из Linux (`include/uapi/asm-generic/errno-base.h`, `errno.h`).

| Код | Имя | Значение | Где возникает |
|---:|---|---|---|
| 1 | `EPERM` | нет прав на операцию | syscall запрещён `syscall_mask` процесса; `chmod` не владельцем и не root |
| 2 | `ENOENT` | нет такого файла | `open`/`access`/`chmod`: grant есть, но файла в ramfs нет |
| 3 | `ESRCH` | нет такого процесса | любой вызов, если слот текущего процесса пуст (внутренняя ошибка ядра, на практике не возникает) |
| 9 | `EBADF` | плохой fd | `read`/`write`/`close`: fd вне таблицы или закрыт; `read`/`write` в fd, открытый без нужного режима |
| 13 | `EACCES` | доступ запрещён | нет grant, в grant не хватает прав, или DAC (`mode`/`uid`/`gid`) не пустил |
| 14 | `EFAULT` | плохой адрес | указатель+длина не лежат целиком в одном user-регионе процесса |
| 22 | `EINVAL` | неверный аргумент | `open`: пустое имя, неизвестные биты во `flags`, режим `3`; `access`: биты вне `0o7`; `chmod`: режим вне `0o777` |
| 24 | `EMFILE` | слишком много открытых файлов | `open`: заняты все `MAX_FDS` (16) слотов |
| 27 | `EFBIG` | файл слишком большой | `write`: файл превысил бы `MAX_FILE_SIZE` (64 КБ) |
| 36 | `ENAMETOOLONG` | слишком длинное имя | `open`/`access`/`chmod`: `path_len > MAX_PATH` (100) |
| 38 | `ENOSYS` | нет такого вызова | неизвестный номер syscall |

---

## 3. Таблица вызовов

| № | Вызов | Аргументы | Успех | Ошибки |
|---:|---|---|---|---|
| 0 | `read` | `fd, buf, len` | число прочитанных байт, `0` = конец файла | `EBADF`, `EFAULT` |
| 1 | `write` | `fd, buf, len` | `len` | `EBADF`, `EFAULT`, `EFBIG` |
| 2 | `open` | `path_ptr, path_len, flags` | fd (≥ 3) | `EINVAL`, `ENAMETOOLONG`, `EFAULT`, `EACCES`, `ENOENT`, `EMFILE` |
| 3 | `close` | `fd` | `0` | `EBADF` |
| 21 | `access` | `path_ptr, path_len, mode` | `0` | `EINVAL`, `ENAMETOOLONG`, `EFAULT`, `EACCES`, `ENOENT` |
| 24 | `yield` | — | `0` | — |
| 60 | `exit` | `code` (пока игнорируется) | не возвращается | — |
| 90 | `chmod` | `path_ptr, path_len, mode` | `0` | `EINVAL`, `ENAMETOOLONG`, `EFAULT`, `EACCES`, `ENOENT`, `EPERM` |

`access`: `mode` — `R_OK=4`, `W_OK=2`, `X_OK=1`, `F_OK=0`; другие биты →
`EINVAL`. `chmod`: `mode` — `0..0o777`. Оба вызова принимают путь парой
«указатель + длина», как `open`.

**Как работает `yield`.** Обработчик не переключает процесс сам: в этот
момент на стеке нет user `rip`/`rsp`, а syscall-стек
(`SYSCALL_KERNEL_RSP`) общий на все процессы. Вместо этого `sys_yield`
выставляет `RESCHED_PENDING`, а `syscall_entry.s` сразу после возврата из
обработчика — пока user-фрейм (`rip`, `rflags`, `rsp`) ещё цел — вызывает
`syscall_reschedule`. Тот сохраняет callee-saved регистры и user-контекст в
`Context` текущего процесса и уходит в ring-3 через `resume_context` —
единственный путь переключения (им же пользуется таймер).

Syscall-стек при этом бросается: процесс продолжает с адреса после
`syscall`, а не внутри ядра, поэтому продолжение в ядре не нужно. Следующий
syscall снова начинает с вершины `SYSCALL_KERNEL_RSP`.

Соглашение о регистрах: callee-saved (`rbx`, `rbp`, `r12`–`r15`)
сохраняются, `rax` = 0; caller-saved (`rcx`, `rdx`, `rsi`, `rdi`,
`r8`–`r11`) могут быть испорчены — как и для любого syscall.

Проверка: `spawn y` — процесс пять раз печатает `Y` и вызывает `yield`,
затем печатает `y done` и делает `exit(0)`. Счётчик живёт в `rbx`, так что
тест заодно ловит потерю callee-saved регистров. Два параллельных `spawn y`
дают 10 `Y` и два `y done`.

**Отличие `open` от Linux:** в Linux `open(path, flags, mode)`, где `path` —
строка с `\0` на конце. Здесь путь передаётся парой «указатель + длина»:
ядру не нужно искать `\0` в user-памяти, и одна проверка диапазона покрывает
весь путь. Поэтому бинарной совместимости с Linux у `open` нет.

### fd 0/1/2

| fd | Что это | `read` | `write` |
|---|---|---|---|
| 0 | stdin | пока `EBADF` | `EBADF` |
| 1 | stdout | `EBADF` | консоль через `shell::program_output` |
| 2 | stderr | `EBADF` | консоль (как stdout) |
| ≥ 3 | файлы | из `inode` с `offset` | в `inode` с `offset` |

---

## 4. Флаги `open`

Значения как в Linux. Режим доступа — это **значение** в двух младших битах,
а не набор битов: `O_RDONLY = 0`, поэтому `flags & O_RDONLY` всегда 0.
Режим извлекается как `flags & O_ACCMODE`.

| Флаг | Значение | Нужные права |
|---|---:|---|
| `O_RDONLY` | 0 | `PERM_READ` |
| `O_WRONLY` | 1 | `PERM_WRITE` |
| `O_RDWR` | 2 | `PERM_READ \| PERM_WRITE` |
| — | 3 | `EINVAL` |

Нужные права проверяются **дважды**: сначала в grant процесса
(`granted & need == need`), затем в DAC inode (`perm::allowed`). Подробнее —
в разделе 5.

Любые биты вне `O_ACCMODE` (`O_CREAT`, `O_TRUNC`, `O_APPEND`, …) пока дают
`EINVAL`: лучше явно отказать, чем молча проигнорировать.

---

## 5. Модель прав на файлы

Модель гибридная: **capability grants** у процесса + **Unix DAC** на inode.
Здесь — сводка.

```rust
pub struct Grant { pub name: String, pub perms: u16 } // PERM_R | PERM_W | PERM_X

pub struct Process { pub uid: u32, pub gid: u32, pub grants: Vec<Grant>, ... }

pub struct Inode { uid: u32, gid: u32, mode: u16, ... } // rwxrwxrwx
```

Процесс видит только файлы из своего списка grants. Но grant — это лишь
разрешение **назвать** файл: конкретное `r`/`w`/`x` дополнительно проверяется
по `uid/gid/mode` inode.

### Где проверяются права

```
open(path, len, flags)
  1. flags → need                        ── EINVAL
  2. длина, указатель, копия имени       ── ENAMETOOLONG / EFAULT
  3. grant по имени: granted & need == need ── EACCES
  4. ramfs::lookup                       ── ENOENT
  5. perm::allowed(mode, uid, gid, ...)  ── EACCES
  6. OpenFile { perms: need, offset: 0 } → fd

access(path, len, mode)
  grant (любой, даже для F_OK) → lookup → DAC   ── EACCES / ENOENT

chmod(path, len, mode)
  grant (любой) → lookup → владелец или root    ── EACCES / ENOENT / EPERM

read(fd)  : OpenFile.perms содержит PERM_READ  иначе EBADF
write(fd) : OpenFile.perms содержит PERM_WRITE иначе EBADF
```

`perm::allowed` выбирает один класс — owner, group или other — и требует,
чтобы `need` целиком содержался в его битах. `uid == 0` обходит DAC, но не
grant.

Принципы:

- **Оба слоя обязательны.** Grant на запись не поможет, если DAC запрещает
  `w` (и наоборот).
- **Права проверяются один раз — в `open`.** Результат запоминается в
  `OpenFile.perms`; `read`/`write` проверяют только этот бит, без поиска
  по grants. Так же делает Unix.
- **В `OpenFile` сохраняется запрошенное (`need`), а не всё разрешённое.**
  Процесс с правом записи, открывший файл `O_RDONLY`, не испортит его по
  ошибке — принцип минимальных прав.
- **Grant проверяется до `lookup`.** Иначе по разнице `ENOENT` / `EACCES`
  процесс без прав мог бы узнать, какие файлы существуют.
- **`access(F_OK)` тоже требует grant.** Существование файла — информация;
  без capability-проверки `access` стал бы оракулом имён.
- **Условие — `granted & need == need`, а не `!= 0`.** Иначе `O_RDWR` при
  grant только на чтение прошёл бы проверку.

### Чего модель (пока) не делает

- **Шелл доверенный.** Команды `cat`, `write`, `touch`, `rm`, `chmod`,
  `chown` работают в ядре и grants не проверяют. Права ограничивают
  процессы, не шелл.
- **Grants по имени.** Если файл удалить и создать заново под тем же именем,
  grant продолжит действовать на новый файл.
- **Нет `setuid`/ACL/дополнительных групп.** Только 9 бит режима и первичный
  `gid`.
- **Лимит размера — на файл, а не на ФС.** 16 файлов по 64 КБ исчерпают
  кучу (1 МБ); `touch` в шелле создаёт файлы без ограничений.

---

## 6. Проверка user-указателей

Ядро работает в CPL0 и через identity map может прочитать любую физическую
память, поэтому каждый указатель из user-программы проверяется
`user_range_ok(p, ptr, len)` **до** создания slice:

- `ptr + len` считается через `checked_add` — переполнение отклоняется;
- диапазон `[ptr, ptr + len)` должен целиком лежать **в одном** регионе
  из `Process::user_regions`: код (`CODE_PAGES` = 2 страницы) и стек
  (1 страница). Раскладка — в [architecture.md, раздел 2](architecture.md#2-виртуальная-память--раскладка).

Проверка по флагу `PAGE_USER` в page tables не подходит: pml4 общий, и
страницы чужих процессов тоже помечены `PAGE_USER`.

Имя файла копируется в буфер ядра хелпером `user_path` (общий для `open`,
`access`, `chmod`), и дальше ядро работает только с копией — защита от
изменения строки между проверкой и использованием (TOCTOU).

**Известное ограничение:** `user_range_ok` не проверяет, можно ли писать в
регион. Сейчас обе страницы процесса writable, но когда код станет
read-only (трек C), `read` в кодовую страницу приведёт к #PF в ядре —
регионам понадобятся флаги.

---

## 7. Тесты (`spawn` в шелле)

| Команда | Grant | Что проверяет |
|---|---|---|
| `spawn p` | `hello.txt`: READ\|WRITE\|EXEC | матрица DAC: `access(R/W/X)`, `open(O_WRONLY)`, `chmod`, обращение без grant |
| `spawn f` | `hello.txt`: READ | `open(O_RDONLY)` → `read` по 16 байт → вывод → `close` |
| `spawn w` | `hello.txt`: WRITE | `open(O_WRONLY)` → две записи: перезапись, сдвиг `offset`, рост файла |
| `spawn x` | `hello.txt`: READ | запреты: `x1` — `open(O_WRONLY)`, `x2` — `write` в fd `O_RDONLY`, `x3` — `open` файла без grant |
| `spawn r` | `hello.txt`: READ | удаление открытого файла: данные доступны через fd после `rm` |

Демо-процессы работают как `uid=1000, gid=1000`; файлы initrd — `root:root`
с режимом `0644` из tar. Поэтому `spawn w` на чистом файле **молча
завершается**: grant на запись есть, но DAC не пускает. Перед ним нужен
`chmod 666 hello.txt` из шелла. После успешного `spawn w` файл содержит
`XYZ0123456789abcdefghij` (23 байта вместо 18).

`spawn p` печатает по одной строке на проверку:

```
p1 0     access(R_OK)     — other r есть
p2 -13   access(W_OK)     — EACCES
p3 -13   access(X_OK)     — EACCES
p4 -13   open(O_WRONLY)   — EACCES (DAC)
p5 -1    chmod(0666)      — EPERM (не владелец)
p6 -13   access(readme.txt) — EACCES (нет grant)
```

После `chmod 666 hello.txt` становятся `p2 0` и `p4 3` (fd). После
`chmod 755 hello.txt` — `p3 0`, а `p2`/`p4` снова `-13`.

`spawn r` открывает `hello.txt` и ждёт, повторяя `open`/`close`, пока
повторный `open` не вернёт `ENOENT` (файл удалили). Затем читает через
первый fd. Сценарий:

    > spawn r
    opened, rm me
    > rm hello.txt
    file deleted
    deleted, reading:
    hello from initrd          ← данные живы: fd держит Arc<Inode>
    > ls                       ← hello.txt в каталоге уже нет

`spawn x` для каждого запрета печатает вердикт и полученный код. Ожидаемый
вывод:

    x1 ok -13      ← EACCES: open(O_WRONLY) при grant только на чтение
    x2 ok -9       ← EBADF:  write в fd, открытый O_RDONLY
    x3 ok -13      ← EACCES: open файла без grant

Вердикт `ok` ставится по знаку (вернулась *какая-то* ошибка), поэтому
сверяй и число: например, `ok -14` (EFAULT) или `ok -22` (EINVAL) значит,
что запрет сработал по неверной причине.
