# Драйвер xHCI (`src/xhci.rs`)

Документация к USB-стеку Tomorrow OS. Драйвер минимальный и **целевой**: его
единственная задача — пройти по дереву USB, найти **HID-клавиатуру** (в т.ч. за
хабом) и настроить её на прерывания-поллинг. Полноценной подсистемы USB здесь
нет: один Interrupter, синхронные (busy-wait) команды, без обработки отключений.

> Тестовая клавиатура — A4Tech `09da:fa10`, Full-Speed Boot-протокол, висит за
> хабом **RTS5411** на root-порту 7. Именно этот путь (FS-устройство за HS-хабом
> через Transaction Translator) гнал почти все баги ниже.

---

## 1. Карта регистров и базовые адреса

`init(bar0)` получает физический адрес BAR0 контроллера (найден через PCI,
class `0x0C/0x03/0x30`). Контроллер identity-mapped, поэтому физ. адрес = вирт.

Четыре блока регистров, базы вычисляются из Capability-регистров:

| Блок | База | Как считается |
|------|------|---------------|
| **Capability** | `bar0` | RO-параметры контроллера |
| **Operational** | `OP_BASE = bar0 + CAPLENGTH` | `CAPLENGTH = r32(bar0) & 0xFF` |
| **Runtime** | `RT_BASE = bar0 + RTSOFF` | `RTSOFF = r32(bar0 + 0x18)` |
| **Doorbell** | `DB_BASE = bar0 + DBOFF` | `DBOFF = r32(bar0 + 0x14)` |

Ключевые Capability-поля, которые читаем:
- `HCSPARAMS1` (`bar0+4`): `MaxSlots = [7:0]`, `MaxPorts = [31:24]`.
- `HCSPARAMS2` (`bar0+8`): число scratchpad-буферов = `(Hi<<5)|Lo`, где
  `Hi=[25:21]`, `Lo=[31:27]`. **Грабли:** Hi/Lo легко перепутать — см. §9.
- `HCCPARAMS1` (`bar0+0x10`): бит `CSZ=[2]` → размер контекста **64** байта
  (если 1) или **32** (если 0). Хранится в `CTX_SIZE`, используется везде при
  адресации Slot/EP контекстов.

Важные Operational-регистры (смещения от `OP_BASE`):

| Смещение | Имя | Назначение |
|----------|-----|-----------|
| `+0x00` | USBCMD | `R/S=[0]`, `HCRST=[1]` |
| `+0x04` | USBSTS | `HCH=[0]`, `HSE=[2]`, `CNR=[11]`, `HCE=[12]` |
| `+0x18` | CRCR | Command Ring base \| `RCS=[0]`; `CA=[2]`, `CRR=[3]` |
| `+0x30` | DCBAAP | указатель на DCBAA |
| `+0x38` | CONFIG | `MaxSlotsEn=[7:0]` |
| `+0x400 + n*0x10` | PORTSC[n] | статус/управление порта n |

---

## 2. Структуры в памяти

Все структуры — наши страницы из `alloc_page()` (4 KB, выровнены, identity-map),
контроллер читает/пишет их по DMA. Поэтому к ним применимы те же правила, что к
MMIO: **volatile-доступ** и **барьеры** перед doorbell (см. §4).

- **DCBAA** (Device Context Base Address Array) — `DCBAA[slot_id]` указывает на
  Output Device Context слота. `DCBAA[0]` зарезервирован под массив scratchpad.
- **Command Ring** — кольцо команд CPU→контроллер (Enable Slot, Address Device,
  Configure/Evaluate). 1 страница = 32 TRB, последний (индекс 31) — Link TRB.
- **Event Ring** + **ERST** — контроллер→CPU: результаты команд, transfer-события,
  port-change. ERST (Event Ring Segment Table) — таблица сегментов, у нас один
  сегмент: `ERST[0] = {ring_ptr, size}`.
- **Transfer Ring** — по одному на каждый активный endpoint (EP0 у каждого
  устройства, плюс Interrupt EP клавиатуры). Структура как у Command Ring.
- **Input Context** — временный буфер для команд Address/Configure/Evaluate:
  `Input Control` (add/drop-флаги) + Slot Context + EP-контексты.
- **Output Device Context** — контроллер сам ведёт его, читаем оттуда актуальный
  Slot Context (например, при `configure_hub_slot` копируем его в Input).
- **Scratchpad** — служебная DMA-память «для контроллера»; на железе обязательна,
  если `max_scratch > 0`.

---

## 3. TRB и кольца (Cycle-бит)

TRB — 16 байт: `param: u64`, `status: u32`, `control: u32` (`struct Trb`). Тип
команды/события — биты `[15:10]` поля `control` (константы `TRB_*`).

Кольцо — массив TRB; контроллер и софт идут по нему по кругу. Согласование «чья
очередь» — через **Cycle-бит** (`control[0]`):
- Софт ставит в новый TRB свой **Producer Cycle State** (`PCS`/`tr_pcs`).
- Контроллер обрабатывает TRB, только если его cycle == ожидаемого; на событиях
  CPU сравнивает cycle с **Consumer Cycle State** (`CCS`/`EVT_CCS`).
- На каждом полном обороте кольца cycle **инвертируется**.

**Link TRB** (индекс 31) заворачивает кольцо в начало. Его cycle тоже обязан
совпадать с текущим состоянием на этом витке — иначе на втором обороте
контроллер встаёт на Link. Поэтому при заворачивании в `post_command` /
`tr_reserve` мы переписываем `control` Link-TRB текущим `PCS` и инвертируем PCS.

`tr_reserve(n)` гарантирует n свободных TRB подряд до Link: если до индекса 31
места мало — добивает хвост `TRB_NOOP` (контроллер съест без события), переключает
cycle Link и заворачивает enqueue в 0. Без этого многоступенчатая транзакция
(Setup+Data+Status) затирала Link TRB.

---

## 4. Volatile и барьеры — почему обязательно

Кольца лежат в обычной RAM, но контроллер пишет их «за спиной» CPU по DMA. Два
правила, нарушение каждого = тихий висяк:

1. **Чтение событий — `read_volatile`.** В spin-цикле `wait_event` компилятор
   иначе докажет, что `(*trb).control` инвариантен, и вынесет чтение из цикла →
   вечный спин на закэшированном cycle-бите (`consume_event`, строки ~91).
2. **Запись TRB видна до doorbell.** Перед `w32(DB_BASE…)` ставим
   `compiler_fence(Release)`, иначе компилятор/CPU могут переставить публикацию
   TRB после звонка → контроллер прочитает мусор (`post_command`, `ctrl_in`).

`ERDP` (Event Ring Dequeue Pointer, `RT_BASE+0x20+0x18`) после каждого
поглощённого события переписываем с битом `EHB=[3]` — иначе контроллер считает,
что мы не успеваем, и тормозит кольцо.

---

## 5. Последовательность `init()`

1. Прочитать Capability (`CAPLENGTH`, `MaxSlots/Ports`, `CTX_SIZE`, scratchpad),
   вычислить `OP/RT/DB` базы.
2. **Stop**: `R/S=0`, ждать `HCH=1`.
3. **Reset**: `HCRST=1`, ждать его снятия (после BIOS-handoff контроллер в
   неизвестном состоянии — чистый сброс единственно надёжен).
4. Ждать `CNR=0` (Controller Not Ready).
5. `MaxSlotsEn = MaxSlots` в CONFIG.
6. Выделить **DCBAA**; если `max_scratch>0` — массив scratchpad → `DCBAA[0]`.
   Записать `DCBAAP`.
7. Выделить **Command Ring**, поставить Link TRB, записать `CRCR | RCS`.
8. Выделить **Event Ring** + **ERST**, заполнить Interrupter 0:
   `ERSTSZ=1`, `ERSTBA`, `ERDP | EHB`.
9. **Start**: `R/S=1`, ждать `HCH=0`.
10. Подать питание на все порты (`PP=PORTSC[9]`), `sleep_ms(200)`, съесть
    стартовые Port-Status-Change события.
11. **Цикл по root-портам** (§6): для каждого с `CCS=1` — reset порта, проверить
    `PED`, погасить RW1C change-биты, определить скорость, `enumerate_device`.
    Нашли клавиатуру — выходим.

---

## 6. Перечисление устройства (`enumerate_device`)

Общий путь для root-порта и порта хаба:

1. **`setup_device`**: `Enable Slot` → получить `slot_id`; собрать Input Context
   (Slot: route string, speed, TT hub/port; EP0: max packet, Transfer Ring);
   `Address Device`. Возвращает `Dev { slot_id, tr_ring, … }`.
2. **FS-фикс EP0**: для Full-Speed (speed==1) прочитать **ровно 8 байт**
   дескриптора устройства, взять `bMaxPacketSize0` (offset 7) и через
   `Evaluate Context` поправить EP0 (см. §9 — иначе Babble).
3. Прочитать **Device Descriptor** (→ `bDeviceClass`) и **Configuration
   Descriptor** (→ config value, endpoint адрес/interval/maxpacket, `bInterfaceClass`).
4. Ветвление по классу:
   - `iface_class == 3` (HID): `Set Configuration` → `Set Protocol(Boot)` →
     `configure_hid_endpoint` → `queue_hid_transfer`. **Клавиатура готова.**
   - `class == 9` (Hub) и `depth < MAX_HUB_DEPTH`: `enumerate_hub` (§7).

Коды завершения (поле `status[31:24]` события): `1` = Success, `13` = Short Packet
(для control-чтений трактуем как ОК), `3` = Babble, `4` = Transaction Error,
`0xFF` = наш внутренний таймаут.

EP0 control-транзакция (`ctrl_in`): три TRB — **Setup** (`param` = 8-байтный USB
setup-пакет, `TRT=3` для IN-with-data), **Data** (`buf`, `len`, dir=IN),
**Status** (`IOC`), затем doorbell слота с target **DCI 1** (EP0).

---

## 7. Хабы, route string и Transaction Translator

Чтобы контроллер маршрутизировал к устройствам за хабом и делал split-транзакции,
хаб нужно явно отметить:

- **`configure_hub_slot`**: копируем актуальный Slot Context из Output Context,
  ставим `Hub=[26]`, `Number of Ports`, `TT Think Time`, шлём `Configure Endpoint`.
- **Hub class requests** (recipient=Other): `SET/CLEAR_FEATURE`, `GET_STATUS` —
  для PORT_POWER(8), PORT_RESET(4), C_PORT_CONNECTION(16), C_PORT_RESET(20).
- Обход портов хаба: подать питание, сбросить каждый порт, прочитать статус,
  определить скорость ребёнка (LS=bit9, HS=bit10, иначе FS) и рекурсивно
  `enumerate_device`.

Два понятия пути:
- **Route String** — 4 бита на ярус хаба, номер downstream-порта пишется в нибл
  `4*depth`. Так контроллер знает физический путь к устройству.
- **Transaction Translator (TT)** — для FS/LS-устройства за HS-хабом контроллер
  шлёт split-транзакции через TT этого хаба. Логика выбора `(tt_slot, tt_port)`:
  - ребёнок HS → TT не нужен `(0,0)`;
  - хаб HS → TT = `(slot этого хаба, номер порта)`;
  - хаб сам FS/LS → наследуем TT, заданный выше по дереву.

`MAX_HUB_DEPTH = 4` (USB-предел 5 ярусов + защита от зацикливания).

---

## 8. HID polling

После настройки клавиатура опрашивается из главного цикла:

- `queue_hid_transfer` ставит `TRB_NORMAL` на Interrupt-EP transfer ring и звонит
  в doorbell нужного EP (`HID_EP_ADDR`/DCI).
- `poll_hid` (зовётся периодически) проверяет Event Ring на transfer-событие от
  HID-EP; если пришёл новый 8-байтный Boot-report (`HID_BUF`) — разбирает
  modifier (offset 0) и до 6 keycode (offsets 2..7), переводит в символы
  (`hid_keycode_to_char`) и переставляет transfer на следующий опрос.

Состояние HID-эндпоинта — в статиках `HID_*` (`HID_SLOT`, `HID_TR_RING`,
`HID_BUF`, `HID_EP_ADDR`, `HID_READY`).

---

## 9. Грабли (уже исправленные — не наступать снова)

Эти баги ловились **только на железе** (QEMU до xHCI не доходит — другой тег MB2),
и каждый давал тихий висяк/порчу памяти, а не явную ошибку:

1. **Scratchpad Hi/Lo перепутаны.** `max_scratch` собирается из `Hi=[25:21]`,
   `Lo=[31:27]` как `(Hi<<5)|Lo`. Перепутаешь — либо лишние аллокации (исчерпание
   PMM), либо нехватка → контроллер DMA-ит в невалидный scratchpad → порча памяти.

2. **EP0 Max Packet → Babble (FS).** Full-Speed EP0 может быть 8/16/32/64, но до
   чтения дескриптора неизвестен. `Address Device` ставит 8; если устройство
   больше — оно отдаёт весь дескриптор одним пакетом >8 байт → `Babble (code=3)`,
   EP0 глохнет. Фикс: прочитать **8 байт**, взять `bMaxPacketSize0`, поправить
   через `Evaluate Context` **до** полного чтения.

3. **RW1C change-биты / PLC-шторм.** Биты статуса порта (`CSC/PEC/WRC/OCC/PRC/
   PLC/CEC`, `[17:23]`) — write-1-to-clear. При записи в PORTSC их надо
   **маскировать** (`& !0x00FFF1FE`), иначе случайно сбросишь. И гасить **все**
   change-биты после reset: раньше гасили только CSC+PRC, а `PLC` на USB3-портах
   сыпался пачками и забивал Event Ring.

4. **Зависший Command Ring.** Неотвечающее устройство держит `CRR=1`, и каждая
   следующая команда виснет за ним (usbsts при этом здоров). Лечится
   `recover_cmd_ring`: `CA=1` (Command Abort) → ждать `CRR=0` → забрать
   `Command Ring Stopped` → переинициализировать кольцо. Так один «плохой» порт
   не убивает перечисление остальных.

5. **Таймаут должен тикать на чужих событиях.** `wait_event` поглощает не-целевые
   события (Port-Status-Change шторм), но бюджет таймаута обязан уменьшаться и в
   этой ветке — иначе бесконечный поток чужих событий зацикливает навсегда.

6. **Невыровненные `*const u16`.** Поля дескрипторов (`wMaxPacketSize` на нечётном
   offset, `wHubCharacteristics` на `buf+3`) читаем через `read_unaligned` —
   прямой `*(*const u16)` в debug-сборке ловится UB-чеком и паникует.

> Связанные заметки памяти: тест на железе (не QEMU), клавиатура за RTS5411,
> FS-устройство EP0 babble, «замёрзший экран = скрытая паника».

---

## 10. Карта функций

| Функция | Что делает |
|---------|-----------|
| `init(bar0)` | полная инициализация контроллера + цикл по root-портам |
| `consume_event` / `wait_event` / `drain_events` | чтение Event Ring |
| `post_command` / `recover_cmd_ring` | Command Ring |
| `tr_reserve` / `ctrl_in` / `ctrl_out_nodata` | EP0 control-транзакции |
| `get_device_descriptor` / `get_config_descriptor` | дескрипторы |
| `setup_device` | Enable Slot + Address Device |
| `evaluate_ep0_max_packet` | фикс EP0 для FS (Babble) |
| `enumerate_device` | адресация → дескрипторы → HID или хаб |
| `configure_hub_slot` / `enumerate_hub` / `hub_*` | хабы и их порты |
| `set_configuration` / `set_protocol` / `configure_hid_endpoint` | настройка HID |
| `queue_hid_transfer` / `poll_hid` / `hid_keycode_to_char` | поллинг клавиатуры |
