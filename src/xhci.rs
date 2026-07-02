use crate::{kprint, pmm, write_hex, CONSOLE};

// Identity map покрывает всю физическую память (boot.s: pdpt0/pdpt1 × 1GB huge pages)
// phys == virt, KVIRT не нужен

unsafe fn alloc_page() -> u64 {
    let phys = pmm::alloc();
    core::ptr::write_bytes(phys as *mut u8, 0, 4096);
    phys
}

// MMIO-регистры контроллера ОБЯЗАНЫ читаться/писаться через volatile,
// иначе компилятор вправе переупорядочить, объединить или вовсе выкинуть
// обращение (например, dead-store для doorbell или зацикливание на poll).
#[inline(always)]
unsafe fn r32(addr: u64) -> u32 {
    core::ptr::read_volatile(addr as *const u32)
}
#[inline(always)]
unsafe fn w32(addr: u64, v: u32) {
    core::ptr::write_volatile(addr as *mut u32, v);
}
#[inline(always)]
unsafe fn w64(addr: u64, v: u64) {
    core::ptr::write_volatile(addr as *mut u64, v);
}

#[repr(C)]
pub struct Trb {
    pub param: u64,
    pub status: u32,
    pub control: u32,
}

// TRB Types
pub const TRB_ENABLE_SLOT: u32 = 9 << 10;
pub const TRB_ADDRESS_DEVICE: u32 = 11 << 10;
pub const TRB_CONFIGURE_EP: u32 = 12 << 10;
pub const TRB_EVALUATE_CONTEXT: u32 = 13 << 10;
pub const TRB_LINK: u32 = 6 << 10;
pub const TRB_SETUP: u32 = 2 << 10;
pub const TRB_DATA: u32 = 3 << 10;
pub const TRB_STATUS: u32 = 4 << 10;
pub const TRB_NORMAL: u32 = 1 << 10;
pub const TRB_NOOP: u32 = 8 << 10; // No-Op (Transfer) — для добивки хвоста кольца

// Event TRB types
const EVT_TRANSFER: u32 = 32;
const EVT_CMD_COMPL: u32 = 33;
const EVT_PORT_CHANGE: u32 = 34;

// Event Ring — один сегмент. 256 TRB × 16 байт = ровно страница 4 КБ.
// Раньше было 32: при нескольких подключённых портах поток Port Status
// Change событий за время sleep_ms переполнял кольцо → контроллер
// останавливал командное кольцо (CRR залипал), события не доходили.
const EVT_RING_SIZE: usize = 256;

// ====================== Глобальное состояние ======================

static mut CMD_RING: u64 = 0;
static mut CMD_ENQ: usize = 0;
static mut CMD_PCS: u32 = 1;

static mut EVT_RING: u64 = 0;
static mut EVT_DEQ: usize = 0;
static mut EVT_CCS: u32 = 1;

static mut RT_BASE: u64 = 0;
static mut DB_BASE: u64 = 0;
static mut DCBAA: u64 = 0;
static mut OP_BASE: u64 = 0;
static mut CTX_SIZE: u64 = 32;

// HID polling state
static mut HID_SLOT: u32 = 0;
static mut HID_TR_RING: u64 = 0;
static mut HID_TR_ENQ: usize = 0;
static mut HID_TR_PCS: u32 = 1;
static mut HID_BUF: u64 = 0;
static mut HID_EP_ADDR: u8 = 0;
static mut HID_READY: bool = false;
static mut HID_PREV: [u8; 6] = [0; 6];

// ====================== Event Ring ======================

unsafe fn consume_event() -> Option<(u32, u32, u32)> {
    let trb = (EVT_RING + EVT_DEQ as u64 * 16) as *const Trb;
    // Event Ring — DMA-память, которую контроллер пишет «за спиной» CPU, ровно
    // как MMIO. Без volatile компилятор вправе доказать, что (*trb).control в
    // spin-цикле wait_event инвариантен, и вынести чтение из цикла → вечный
    // спин на закэшированном Cycle-бите, который ни разу не увидит событие.
    let ctrl = core::ptr::read_volatile(&(*trb).control);

    if (ctrl & 1) != EVT_CCS {
        return None;
    }

    let trb_type = (ctrl >> 10) & 0x3F;
    let slot_id = (ctrl >> 24) as u32;
    let code = (core::ptr::read_volatile(&(*trb).status) >> 24) & 0xFF;

    EVT_DEQ += 1;
    if EVT_DEQ == EVT_RING_SIZE {
        EVT_DEQ = 0;
        EVT_CCS ^= 1;
    }

    // Identity map: EVT_RING уже физический адрес
    let erdp_phys = EVT_RING + EVT_DEQ as u64 * 16;
    w64(RT_BASE + 0x20 + 0x18, erdp_phys | (1 << 3));

    Some((trb_type, code, slot_id))
}

unsafe fn wait_event(expected: u32) -> (u32, u32) {
    let mut timeout = 10_000_000u32;
    loop {
        if let Some((typ, code, slot)) = consume_event() {
            if typ == expected {
                if expected == EVT_CMD_COMPL && code != 1 {
                    kprint!("xhci: cmd failed code=");
                    write_hex!(code as u64);
                    kprint!("\n");
                }
                return (code, slot);
            }
            // Чужое событие (например, Port Status Change storm с root-порта)
            // мы поглотили и продвинули ERDP. Бюджет таймаута обязан тикать и
            // здесь — иначе бесконечный поток чужих событий зацикливает нас
            // навсегда, минуя timeout-ветку.
        }
        timeout -= 1;
        if timeout == 0 {
            // USBSTS: HCH=bit0, HSE=bit2, HCE=bit12; CRCR.CRR=bit3 (Command Ring Running)
            kprint!("xhci: event timeout usbsts=");
            write_hex!(r32(OP_BASE + 4) as u64);
            kprint!(" crr=");
            write_hex!((r32(OP_BASE + 0x18) & 0x8) as u64);
            // Что лежит на нашей позиции dequeue и совпал ли cycle:
            // если deq_ctrl&1 != ccs — событие "не готово" (десинк/переполнение),
            // если == ccs — мы по какой-то причине его пропустили.
            let trb = (EVT_RING + EVT_DEQ as u64 * 16) as *const Trb;
            kprint!(" deq_ctrl=");
            write_hex!(core::ptr::read_volatile(&(*trb).control) as u64);
            kprint!(" ccs=");
            write_hex!(EVT_CCS as u64);
            kprint!("\n");
            return (0xFF, 0);
        }
        core::arch::asm!("pause");
    }
}

// Вычищаем все накопившиеся события (Port Status Change и пр.), продвигая
// ERDP. Зовём перед постингом команд, чтобы backlog от предыдущих портов не
// копился в кольце и не доводил его до переполнения.
unsafe fn drain_events() {
    while consume_event().is_some() {}
}

// ====================== Command Ring ======================

unsafe fn post_command(param: u64, status: u32, control: u32) {
    let trb = (CMD_RING + CMD_ENQ as u64 * 16) as *mut Trb;
    (*trb).param = param;
    (*trb).status = status;
    (*trb).control = control | CMD_PCS;

    CMD_ENQ += 1;
    if CMD_ENQ == 31 {
        // Link TRB заворачивает кольцо: его cycle обязан совпасть с текущим CCS
        // контроллера на этом витке. Иначе на втором обороте (PCS уже 0) Link
        // остаётся с cycle=1 → mismatch → контроллер встаёт на Link.
        let link = (CMD_RING + 31 * 16) as *mut Trb;
        (*link).control = TRB_LINK | (1 << 1) | CMD_PCS;
        CMD_ENQ = 0;
        CMD_PCS ^= 1;
    }
    // TRB команды должен быть виден контроллеру до doorbell.
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
    w32(DB_BASE, 0); // Command Ring doorbell (slot 0, target 0)
}

// Восстановление застрявшего командного кольца.
//
// Если команда (Enable Slot / Address Device) не завершилась за отведённый
// таймаут, CRR залипает в 1: контроллер всё ещё «жуёт» эту команду (например,
// неотвечающее устройство на root-порту), и КАЖДАЯ следующая команда виснет
// за ней. usbsts при этом здоров (нет HSE/HCE) — кольцо просто не двигается.
//
// Спека (4.6.1.2): пишем CRCR.CA=1 (Command Abort, бит 2), ждём CRR=0, забираем
// Command Ring Stopped событие, после чего переинициализируем кольцо с чистого
// листа. Это позволяет одному «плохому» порту не убивать перечисление остальных.
unsafe fn recover_cmd_ring() {
    // CA=бит2. CRCR 64-битный — сохраняем младшие биты указателя, гасим [3:0],
    // ставим Abort. CRR (бит3) RO, запись 0 безвредна.
    w32(OP_BASE + 0x18, (r32(OP_BASE + 0x18) & !0xF) | (1 << 2));

    let mut t = 5_000_000u32;
    while (r32(OP_BASE + 0x18) & 0x8) != 0 && t > 0 {
        t -= 1;
        core::arch::asm!("pause");
    }

    // Забираем Command Ring Stopped и любой накопившийся хвост.
    drain_events();

    // Чистый рестарт кольца: страница в ноль, заново Link TRB, сброс enqueue/PCS.
    core::ptr::write_bytes(CMD_RING as *mut u8, 0, 4096);
    let link = (CMD_RING + 31 * 16) as *mut Trb;
    (*link).param = CMD_RING;
    (*link).status = 0;
    (*link).control = TRB_LINK | (1 << 1) | 1;
    CMD_ENQ = 0;
    CMD_PCS = 1;
    w64(OP_BASE + 0x18, CMD_RING | 1); // CRCR | RCS
}

// ====================== Control Transfer (EP0) ======================

// Резервируем n свободных TRB подряд перед Link TRB (индекс 31).
// Если до Link места не хватает — добиваем хвост No-Op TRB-ами (контроллер их
// съест без событий), переключаем cycle Link TRB и заворачиваем enqueue в 0,
// чтобы контроллер прошёл Link и продолжил с начала. Без этого транзакция при
// *tr_enq=30 затирала Link TRB и писала за пределами логического кольца.
unsafe fn tr_reserve(tr_ring: u64, tr_enq: &mut usize, tr_pcs: &mut u32, n: usize) {
    if *tr_enq + n <= 31 {
        return;
    }
    while *tr_enq < 31 {
        let trb = (tr_ring + *tr_enq as u64 * 16) as *mut Trb;
        (*trb).param = 0;
        (*trb).status = 0;
        (*trb).control = TRB_NOOP | *tr_pcs;
        *tr_enq += 1;
    }
    let link = (tr_ring + 31 * 16) as *mut Trb;
    (*link).control = TRB_LINK | (1 << 1) | *tr_pcs;
    *tr_enq = 0;
    *tr_pcs ^= 1;
}

unsafe fn ctrl_in(
    slot_id: u32,
    tr_ring: u64,
    tr_enq: &mut usize,
    tr_pcs: &mut u32,
    setup_param: u64,
    buf: u64,
    len: u16,
) -> u32 {
    tr_reserve(tr_ring, tr_enq, tr_pcs, 3);
    let base = tr_ring + *tr_enq as u64 * 16;

    let setup = base as *mut Trb;
    (*setup).param = setup_param;
    (*setup).status = 8;
    (*setup).control = TRB_SETUP | (1 << 6) | (3 << 16) | *tr_pcs;

    let data = (base + 16) as *mut Trb;
    (*data).param = buf;
    (*data).status = len as u32;
    (*data).control = TRB_DATA | (1 << 16) | *tr_pcs;

    let status_trb = (base + 32) as *mut Trb;
    (*status_trb).param = 0;
    (*status_trb).status = 0;
    (*status_trb).control = TRB_STATUS | (1 << 5) | *tr_pcs;

    *tr_enq += 3;

    // TRB должны быть видны контроллеру ДО звонка в doorbell.
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
    w32(DB_BASE + slot_id as u64 * 4, 1); // doorbell слота, target EP0 (DCI 1)

    let (code, _) = wait_event(EVT_TRANSFER);
    code
}

unsafe fn ctrl_out_nodata(
    slot_id: u32,
    tr_ring: u64,
    tr_enq: &mut usize,
    tr_pcs: &mut u32,
    setup_param: u64,
) -> u32 {
    tr_reserve(tr_ring, tr_enq, tr_pcs, 2);
    let base = tr_ring + *tr_enq as u64 * 16;

    let setup = base as *mut Trb;
    (*setup).param = setup_param;
    (*setup).status = 8;
    (*setup).control = TRB_SETUP | (1 << 6) | *tr_pcs; // Transfer Type = 0 (No Data)

    let status_trb = (base + 16) as *mut Trb;
    (*status_trb).param = 0;
    (*status_trb).status = 0;
    (*status_trb).control = TRB_STATUS | (1 << 16) | (1 << 5) | *tr_pcs; // IN direction

    *tr_enq += 2;

    // TRB должны быть видны контроллеру ДО звонка в doorbell.
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
    w32(DB_BASE + slot_id as u64 * 4, 1); // doorbell слота, target EP0 (DCI 1)

    let (code, _) = wait_event(EVT_TRANSFER);
    code
}

// ====================== Descriptors ======================

// Возвращает bDeviceClass (9 = хаб); 0xFF при ошибке чтения.
unsafe fn get_device_descriptor(
    slot_id: u32,
    tr_ring: u64,
    tr_enq: &mut usize,
    tr_pcs: &mut u32,
) -> u8 {
    let buf = alloc_page();
    let code = ctrl_in(
        slot_id,
        tr_ring,
        tr_enq,
        tr_pcs,
        0x0012_0000_0100_0680,
        buf,
        18,
    );

    if code != 1 && code != 13 {
        kprint!("xhci: dev_desc failed code=");
        write_hex!(code as u64);
        kprint!("\n");
        return 0xFF;
    }

    let class = *((buf + 4) as *const u8);
    let vendor = *((buf + 8) as *const u16);
    let product = *((buf + 10) as *const u16);

    kprint!("xhci: class=");
    write_hex!(class as u64);
    kprint!(" vendor=");
    write_hex!(vendor as u64);
    kprint!(" product=");
    write_hex!(product as u64);
    kprint!("\n");

    class
}

unsafe fn get_config_descriptor(
    slot_id: u32,
    tr_ring: u64,
    tr_enq: &mut usize,
    tr_pcs: &mut u32,
) -> (u8, u8, u8, u16, u8) {
    let buf = alloc_page();
    let code = ctrl_in(
        slot_id,
        tr_ring,
        tr_enq,
        tr_pcs,
        0x0200_0000_0200_0680,
        buf,
        512,
    );

    if code != 1 && code != 13 {
        kprint!("xhci: cfg_desc failed code=");
        write_hex!(code as u64);
        kprint!("\n");
        return (0, 0, 0, 0, 0);
    }

    let total_len = *((buf + 2) as *const u16);
    let config_value = *((buf + 5) as *const u8);

    let mut ep_addr = 0u8;
    let mut interval = 0u8;
    let mut max_packet = 0u16;
    let mut iface_class = 0u8;

    let parse_len = (if total_len > 512 { 512 } else { total_len }) as u64;
    let mut offset = 0u64;

    while offset < parse_len {
        let len = *((buf + offset) as *const u8);
        if len == 0 {
            break;
        }
        let kind = *((buf + offset + 1) as *const u8);

        if kind == 4 {
            iface_class = *((buf + offset + 5) as *const u8);
        }
        if kind == 5 {
            let addr = *((buf + offset + 2) as *const u8);
            let attrs = *((buf + offset + 3) as *const u8);
            // wMaxPacketSize лежит по нечётному смещению (endpoint-дескриптор
            // начинается после config+interface+HID), поэтому u16 здесь
            // невыровнен. Прямой *(*const u16) в debug-сборке ловится UB-чеком
            // «misaligned pointer dereference» → паника. Читаем без выравнивания.
            let mp = core::ptr::read_unaligned((buf + offset + 4) as *const u16);
            let iv = *((buf + offset + 6) as *const u8);
            if (attrs & 3) == 3 && (addr & 0x80) != 0 {
                ep_addr = addr;
                max_packet = mp;
                interval = iv;
                break;
            }
        }
        offset += len as u64;
    }

    (config_value, ep_addr, interval, max_packet, iface_class)
}

// ====================== HID Setup ======================

unsafe fn set_configuration(
    slot_id: u32,
    tr_ring: u64,
    tr_enq: &mut usize,
    tr_pcs: &mut u32,
    config_value: u8,
) -> u32 {
    // SET_CONFIGURATION: bmRequestType=0x00, bRequest=9, wValue=config_value
    let setup = (config_value as u64) << 16 | (9u64 << 8) | 0x00;
    ctrl_out_nodata(slot_id, tr_ring, tr_enq, tr_pcs, setup)
}

unsafe fn set_protocol(slot_id: u32, tr_ring: u64, tr_enq: &mut usize, tr_pcs: &mut u32) -> u32 {
    // SET_PROTOCOL: bmRequestType=0x21, bRequest=0x0B, wValue=0 (Boot Protocol)
    // wIndex=0 (interface 0), wLength=0
    // Setup packet: [wLength:16][wIndex:16][wValue:16][bRequest:8][bmRequestType:8]
    let setup: u64 = (0u64 << 48) | (0u64 << 32) | (0u64 << 16) | (0x0Bu64 << 8) | 0x21u64;
    ctrl_out_nodata(slot_id, tr_ring, tr_enq, tr_pcs, setup)
}

unsafe fn configure_hid_endpoint(
    slot_id: u32,
    ep_addr: u8,
    max_packet: u16,
    interval: u8,
    speed: u32,
) -> u64 {
    let ep_idx = ((ep_addr & 0xF) * 2 + if ep_addr & 0x80 != 0 { 1 } else { 0 }) as u64;

    // bInterval из дескриптора → поле Interval EP Context (единицы 125 мкс, экспонента):
    //  HS/SS (speed 3/4): bInterval уже кодирует 2^(bInterval-1) микрокадров → Interval = bInterval-1
    //  FS/LS (speed 1/2): bInterval в кадрах (мс) → Interval = floor(log2(bInterval)) + 3
    // Раньше сюда клали сырой bInterval → неверная частота опроса / Bandwidth Error.
    let xhci_interval: u32 = match speed {
        3 | 4 => (interval.max(1) - 1).min(15) as u32,
        _ => {
            let mut iv = interval.max(1) as u32;
            let mut log = 0u32;
            while iv > 1 {
                iv >>= 1;
                log += 1;
            }
            (log + 3).min(15)
        }
    };

    let input_ctx = alloc_page();

    // Input Control Context: добавляем EP (ep_idx) + slot (0)
    *((input_ctx + 4) as *mut u32) = (1 << ep_idx) | 1;

    // Slot Context — копируем из Output Context и меняем Context Entries
    let out_ctx = *((DCBAA + slot_id as u64 * 8) as *const u64);
    let slot_src = (out_ctx) as *const u32;
    let slot_dst = (input_ctx + CTX_SIZE) as *mut u32;
    for i in 0..8 {
        *slot_dst.add(i) = *slot_src.add(i);
    }
    // Context Entries = ep_idx
    let slot0 = *slot_dst & !(0x1F << 27);
    *slot_dst = slot0 | ((ep_idx as u32) << 27);

    // Interrupt IN Endpoint Context
    let tr_ring = alloc_page();
    let link = (tr_ring + 31 * 16) as *mut Trb;
    (*link).param = tr_ring;
    (*link).control = TRB_LINK | (1 << 1) | 1;

    let ep_ctx = (input_ctx + CTX_SIZE * (ep_idx + 1)) as *mut u32;
    // EP Type = 7 (Interrupt IN), CErr=3, MaxPacketSize, Interval
    *ep_ctx.add(0) = xhci_interval << 16; // Interval [23:16]
    *ep_ctx.add(1) = ((max_packet as u32) << 16) | (7 << 3) | (3 << 1); // Type=Interrupt IN
    *(ep_ctx.add(2) as *mut u64) = tr_ring | 1; // TR Dequeue | DCS
                                                // Average TRB Length (word4 [15:0]): часть xHCI без него отдаёт Parameter
                                                // Error на Configure Endpoint. Для interrupt EP кладём размер пакета.
    *ep_ctx.add(4) = max_packet as u32;

    // Configure Endpoint command
    post_command(input_ctx, 0, TRB_CONFIGURE_EP | (slot_id << 24));
    let (code, _) = wait_event(EVT_CMD_COMPL);
    if code != 1 {
        kprint!("xhci: configure ep failed code=");
        write_hex!(code as u64);
        kprint!("\n");
        if code == 0xFF {
            recover_cmd_ring();
        }
        return 0;
    }

    kprint!("xhci: endpoint configured\n");

    // Сохраняем состояние для polling
    HID_SLOT = slot_id;
    HID_TR_RING = tr_ring;
    HID_TR_ENQ = 0;
    HID_TR_PCS = 1;
    HID_EP_ADDR = ep_addr;
    HID_BUF = alloc_page();
    HID_READY = true;

    ep_idx
}

// Отправляем один Normal TRB чтобы получить HID репорт
unsafe fn queue_hid_transfer(ep_idx: u64) {
    let trb = (HID_TR_RING + HID_TR_ENQ as u64 * 16) as *mut Trb;
    (*trb).param = HID_BUF;
    (*trb).status = 8; // 8 байт — стандартный HID boot report
    (*trb).control = TRB_NORMAL | (1 << 5) | HID_TR_PCS; // IOC=1

    HID_TR_ENQ += 1;
    if HID_TR_ENQ >= 31 {
        // Обновляем cycle-бит Link TRB под текущий проход. Без этого на втором
        // витке (PCS уже инвертирован) Link остаётся с прежним cycle → mismatch,
        // контроллер встаёт на Link TRB — клавиатура «умирает» после ~62 transfer.
        let link = (HID_TR_RING + 31 * 16) as *mut Trb;
        (*link).control = TRB_LINK | (1 << 1) | HID_TR_PCS;
        HID_TR_ENQ = 0;
        HID_TR_PCS ^= 1;
    }

    // TRB должен быть виден контроллеру до doorbell.
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
    // Doorbell для нужного endpoint
    w32(DB_BASE + HID_SLOT as u64 * 4, ep_idx as u32);
}

// Опрос HID — вызывается из основного цикла или таймера
pub unsafe fn poll_hid() {
    if !HID_READY {
        return;
    }

    // Прокручиваем ВСЕ накопившиеся события, не застревая на чужих
    // (Port Status Change / Command Completion): consume_event сам двигает
    // EVT_DEQ и ERDP. Раньше при не-Transfer событии в голове кольца poll_hid
    // выходил, НЕ сдвинув dequeue — событие застревало навсегда, а Transfer
    // Event с HID-отчётом стоял за ним → клавиатура «мертва».
    let mut got_transfer = false;
    while let Some((typ, _code, _slot)) = consume_event() {
        if typ != EVT_TRANSFER {
            continue;
        }
        got_transfer = true;

        // Читаем HID Boot Protocol report (8 байт):
        // [0] Modifier, [1] Reserved, [2..7] Keycodes
        let modifier = *(HID_BUF as *const u8);
        let shift = modifier & 0x22 != 0; // Left/Right Shift
        let mut cur = [0u8; 6];

        for i in 0..6 {
            cur[i] = *((HID_BUF + 2 + i as u64) as *const u8);
        }

        for &keycode in cur.iter() {
            if keycode == 0 {
                continue;
            }
            if HID_PREV.contains(&keycode) {
                continue;
            } // зажата с прошлого раза — не новое нажатие
            if let Some(ch) = hid_keycode_to_char(keycode, shift) {
                crate::shell::on_char(ch);
            }
        }

        HID_PREV = cur;
    }

    // Перезапускаем transfer один раз после дренажа: дальше контроллер сам
    // будет постить события по interval, а мы — добавлять следующий TRB.
    if got_transfer {
        let ep_idx = ((HID_EP_ADDR & 0xF) * 2 + if HID_EP_ADDR & 0x80 != 0 { 1 } else { 0 }) as u64;
        queue_hid_transfer(ep_idx);
    }
}

fn hid_keycode_to_char(code: u8, shift: bool) -> Option<u8> {
    // HID Usage ID → ASCII (Keyboard/Keypad Page 0x07)
    let table: &[(u8, u8, u8)] = &[
        (0x04, b'a', b'A'),
        (0x05, b'b', b'B'),
        (0x06, b'c', b'C'),
        (0x07, b'd', b'D'),
        (0x08, b'e', b'E'),
        (0x09, b'f', b'F'),
        (0x0A, b'g', b'G'),
        (0x0B, b'h', b'H'),
        (0x0C, b'i', b'I'),
        (0x0D, b'j', b'J'),
        (0x0E, b'k', b'K'),
        (0x0F, b'l', b'L'),
        (0x10, b'm', b'M'),
        (0x11, b'n', b'N'),
        (0x12, b'o', b'O'),
        (0x13, b'p', b'P'),
        (0x14, b'q', b'Q'),
        (0x15, b'r', b'R'),
        (0x16, b's', b'S'),
        (0x17, b't', b'T'),
        (0x18, b'u', b'U'),
        (0x19, b'v', b'V'),
        (0x1A, b'w', b'W'),
        (0x1B, b'x', b'X'),
        (0x1C, b'y', b'Y'),
        (0x1D, b'z', b'Z'),
        (0x1E, b'1', b'!'),
        (0x1F, b'2', b'@'),
        (0x20, b'3', b'#'),
        (0x21, b'4', b'$'),
        (0x22, b'5', b'%'),
        (0x23, b'6', b'^'),
        (0x24, b'7', b'&'),
        (0x25, b'8', b'*'),
        (0x26, b'9', b'('),
        (0x27, b'0', b')'),
        (0x28, b'\n', b'\n'),
        (0x2A, 0x08, 0x08), // Backspace
        (0x2C, b' ', b' '),
        (0x2D, b'-', b'_'),
        (0x2E, b'=', b'+'),
        (0x2F, b'[', b'{'),
        (0x30, b']', b'}'),
        (0x33, b';', b':'),
        (0x34, b'\'', b'"'),
        (0x35, b'`', b'~'),
        (0x36, b',', b'<'),
        (0x37, b'.', b'>'),
        (0x38, b'/', b'?'),
    ];

    for &(id, lo, hi) in table {
        if id == code {
            return Some(if shift { hi } else { lo });
        }
    }
    None
}

// ====================== Delay ======================

unsafe fn sleep_ms(ms: u32) {
    for _ in 0..(ms as u64 * 100_000) {
        core::arch::asm!("pause");
    }
}

// ====================== Device setup (root и за хабом) ======================

// Положение устройства в дереве + его EP0 transfer ring.
struct Dev {
    slot_id: u32,
    tr_ring: u64,
    tr_enq: usize,
    tr_pcs: u32,
}

// Глубже этого яруса хабов не лезем (USB-предел 5 + защита от зацикливания).
const MAX_HUB_DEPTH: u32 = 4;

// MaxPacketSize для EP0 по скорости (Default PSI: 1=FS, 2=LS, 3=HS, 4=SS).
fn ep0_max_packet(speed: u32) -> u32 {
    match speed {
        3 => 64,
        4 => 512,
        _ => 8,
    }
}

// Enable Slot + Address Device. route_string/root_port задают путь в дереве,
// tt_hub_slot/tt_port — Transaction Translator для FS/LS-устройства за HS-хабом
// (0 => без TT). Возвращает слот и EP0 transfer ring, либо None при отказе.
unsafe fn setup_device(
    route_string: u32,
    root_port: u32,
    speed: u32,
    tt_hub_slot: u32,
    tt_port: u32,
) -> Option<Dev> {
    drain_events();

    post_command(0, 0, TRB_ENABLE_SLOT);
    let (code, slot_id) = wait_event(EVT_CMD_COMPL);
    if code != 1 {
        kprint!("xhci: enable slot failed\n");
        if code == 0xFF {
            recover_cmd_ring();
        }
        return None;
    }
    kprint!("xhci: slot_id=");
    write_hex!(slot_id as u64);
    kprint!("\n");

    let input_ctx = alloc_page();
    *((input_ctx + 4) as *mut u32) = 0b11; // A0 (slot) + A1 (EP0)

    let slot_ctx = input_ctx + CTX_SIZE;
    // dword0: Route String [19:0], Speed [23:20], Context Entries [31:27]=1
    *((slot_ctx) as *mut u32) = (route_string & 0xF_FFFF) | (speed << 20) | (1 << 27);
    // dword1: Root Hub Port Number [23:16]
    *((slot_ctx + 4) as *mut u32) = root_port << 16;
    // dword2: TT Hub Slot ID [7:0] + TT Port Number [15:8] — split-транзакции
    *((slot_ctx + 8) as *mut u32) = (tt_hub_slot & 0xFF) | ((tt_port & 0xFF) << 8);

    let tr_ring = alloc_page();
    let link = (tr_ring + 31 * 16) as *mut Trb;
    (*link).param = tr_ring;
    (*link).control = TRB_LINK | (1 << 1) | 1;

    let ep0_ctx = input_ctx + CTX_SIZE * 2;
    let max_pkt = ep0_max_packet(speed);
    *((ep0_ctx + 4) as *mut u32) = (max_pkt << 16) | (4 << 3) | (3 << 1);
    *((ep0_ctx + 8) as *mut u64) = tr_ring | 1;

    let out_ctx = alloc_page();
    *((DCBAA + slot_id as u64 * 8) as *mut u64) = out_ctx;

    post_command(input_ctx, 0, TRB_ADDRESS_DEVICE | (slot_id << 24));
    let (code, _) = wait_event(EVT_CMD_COMPL);
    if code != 1 {
        kprint!("xhci: address device failed code=");
        write_hex!(code as u64);
        kprint!("\n");
        if code == 0xFF {
            recover_cmd_ring();
        }
        return None;
    }
    kprint!("xhci: device addressed\n");

    Some(Dev {
        slot_id,
        tr_ring,
        tr_enq: 0,
        tr_pcs: 1,
    })
}

// Обновляет EP0 Max Packet Size в контексте слота через Evaluate Context.
// Full-Speed устройство имеет bMaxPacketSize0 ∈ {8,16,32,64}, но реальное
// значение неизвестно до чтения дескриптора. Address Device программирует EP0
// предположением 8; если устройство больше, оно вернёт весь дескриптор одним
// пакетом > 8 байт → контроллер ловит Babble (code=3) и глушит EP0. Поэтому
// узнав настоящее значение, правим его до полного чтения. A1 (bit1) = EP0.
unsafe fn evaluate_ep0_max_packet(slot_id: u32, max_pkt: u32) -> u32 {
    let input_ctx = alloc_page();
    *((input_ctx + 4) as *mut u32) = 1 << 1; // Add flags: только EP0 (DCI 1)
    let ep0_ctx = input_ctx + CTX_SIZE * 2;
    *((ep0_ctx + 4) as *mut u32) = max_pkt << 16; // Max Packet Size [31:16]

    post_command(input_ctx, 0, TRB_EVALUATE_CONTEXT | (slot_id << 24));
    let (code, _) = wait_event(EVT_CMD_COMPL);
    code
}

// Перечисляет устройство: адресует, читает дескрипторы и либо заводит
// HID-клавиатуру, либо (если это хаб) рекурсивно обходит его порты.
// true => клавиатура найдена и готова.
unsafe fn enumerate_device(
    route_string: u32,
    root_port: u32,
    speed: u32,
    tt_hub_slot: u32,
    tt_port: u32,
    depth: u32,
) -> bool {
    let mut dev = match setup_device(route_string, root_port, speed, tt_hub_slot, tt_port) {
        Some(d) => d,
        None => return false,
    };

    // Full-Speed: до полного чтения дескриптора узнаём настоящий EP0 max packet.
    // Запрос ровно 8 байт умещается в один пакет при любом реальном EP0 (8..64),
    // поэтому Babble не возникает даже при заниженном до 8 контексте. Прочитав
    // bMaxPacketSize0 (offset 7), правим контекст через Evaluate Context.
    if speed == 1 {
        let buf = alloc_page();
        let code = ctrl_in(
            dev.slot_id,
            dev.tr_ring,
            &mut dev.tr_enq,
            &mut dev.tr_pcs,
            0x0008_0000_0100_0680, // GET_DESCRIPTOR(Device), wLength=8
            buf,
            8,
        );
        if code == 1 || code == 13 {
            let mps0 = *((buf + 7) as *const u8) as u32;
            if mps0 != 0 && mps0 != ep0_max_packet(speed) {
                evaluate_ep0_max_packet(dev.slot_id, mps0);
            }
        }
    }

    let dev_class =
        get_device_descriptor(dev.slot_id, dev.tr_ring, &mut dev.tr_enq, &mut dev.tr_pcs);

    let (cfg, ep_addr, interval, maxp, iface_class) =
        get_config_descriptor(dev.slot_id, dev.tr_ring, &mut dev.tr_enq, &mut dev.tr_pcs);

    kprint!("xhci: cfg=");
    write_hex!(cfg as u64);
    kprint!(" ep=");
    write_hex!(ep_addr as u64);
    kprint!(" iface_class=");
    write_hex!(iface_class as u64);
    kprint!("\n");

    // --- HID-устройство ---
    if iface_class == 3 {
        kprint!("xhci: HID device found\n");

        let code = set_configuration(
            dev.slot_id,
            dev.tr_ring,
            &mut dev.tr_enq,
            &mut dev.tr_pcs,
            cfg,
        );
        if code != 1 && code != 13 {
            kprint!("xhci: set_config failed code=");
            write_hex!(code as u64);
            kprint!("\n");
            return false;
        }

        let code = set_protocol(dev.slot_id, dev.tr_ring, &mut dev.tr_enq, &mut dev.tr_pcs);
        if code != 1 && code != 13 {
            kprint!("xhci: set_protocol failed\n");
        }

        let ep_idx = configure_hid_endpoint(dev.slot_id, ep_addr, maxp, interval, speed);
        if ep_idx == 0 {
            return false;
        }
        queue_hid_transfer(ep_idx);
        kprint!("xhci: keyboard ready\n");
        return true;
    }

    // --- Хаб ---
    if (dev_class == 9 || iface_class == 9) && depth < MAX_HUB_DEPTH {
        return enumerate_hub(
            &mut dev,
            cfg,
            route_string,
            root_port,
            speed,
            tt_hub_slot,
            tt_port,
            depth,
        );
    }

    false
}

// Помечает слот хаба как Hub (+ Number of Ports + TT Think Time) через Configure
// Endpoint. Без этого контроллер не маршрутизирует и не делает split-транзакции
// к устройствам за хабом.
unsafe fn configure_hub_slot(slot_id: u32, nbr_ports: u32, ttt: u32) -> bool {
    let input_ctx = alloc_page();
    *((input_ctx + 4) as *mut u32) = 1; // A0 = slot

    let out_ctx = *((DCBAA + slot_id as u64 * 8) as *const u64);
    let slot_src = out_ctx as *const u32;
    let slot_dst = (input_ctx + CTX_SIZE) as *mut u32;
    for i in 0..8 {
        *slot_dst.add(i) = *slot_src.add(i);
    }
    *slot_dst = *slot_dst | (1 << 26); // Hub = 1
    *slot_dst.add(1) = (*slot_dst.add(1) & 0x00FF_FFFF) | (nbr_ports << 24); // Number of Ports
    *slot_dst.add(2) = (*slot_dst.add(2) & !(0x3 << 16)) | ((ttt & 0x3) << 16); // TT Think Time

    post_command(input_ctx, 0, TRB_CONFIGURE_EP | (slot_id << 24));
    let (code, _) = wait_event(EVT_CMD_COMPL);
    if code != 1 {
        if code == 0xFF {
            recover_cmd_ring();
        }
        return false;
    }
    true
}

// --- Hub class requests (recipient = Other, bmRequestType class) ---

unsafe fn hub_set_port_feature(dev: &mut Dev, feature: u16, port: u32) {
    // bmRequestType=0x23, bRequest=SET_FEATURE(3), wValue=feature, wIndex=port
    let setup = ((port as u64) << 32) | ((feature as u64) << 16) | (0x03 << 8) | 0x23;
    ctrl_out_nodata(
        dev.slot_id,
        dev.tr_ring,
        &mut dev.tr_enq,
        &mut dev.tr_pcs,
        setup,
    );
}

unsafe fn hub_clear_port_feature(dev: &mut Dev, feature: u16, port: u32) {
    // bmRequestType=0x23, bRequest=CLEAR_FEATURE(1)
    let setup = ((port as u64) << 32) | ((feature as u64) << 16) | (0x01 << 8) | 0x23;
    ctrl_out_nodata(
        dev.slot_id,
        dev.tr_ring,
        &mut dev.tr_enq,
        &mut dev.tr_pcs,
        setup,
    );
}

unsafe fn hub_get_port_status(dev: &mut Dev, buf: u64, port: u32) -> u32 {
    // bmRequestType=0xA3, bRequest=GET_STATUS(0), wLength=4 → wPortStatus|wPortChange
    let setup = (4u64 << 48) | ((port as u64) << 32) | 0xA3;
    let code = ctrl_in(
        dev.slot_id,
        dev.tr_ring,
        &mut dev.tr_enq,
        &mut dev.tr_pcs,
        setup,
        buf,
        4,
    );
    if code != 1 && code != 13 {
        return 0;
    }
    *(buf as *const u32)
}

// Конфигурирует хаб и обходит его downstream-порты. depth — ярус ЭТОГО хаба
// (0 = хаб воткнут прямо в root-порт). true => нашли клавиатуру за хабом.
unsafe fn enumerate_hub(
    dev: &mut Dev,
    cfg: u8,
    route_string: u32,
    root_port: u32,
    hub_speed: u32,
    parent_tt_hub_slot: u32,
    parent_tt_port: u32,
    depth: u32,
) -> bool {
    kprint!("xhci: hub found\n");

    // [DIAG] A — вошли в enumerate_hub, перед set_configuration
    kprint!("xhci: hubdiag A slot=");
    write_hex!(dev.slot_id as u64);
    kprint!(" enq=");
    write_hex!(dev.tr_enq as u64);
    kprint!(" cfg=");
    write_hex!(cfg as u64);
    kprint!("\n");

    // Хаб надо сконфигурировать, иначе он не подаст питание на порты.
    let code = set_configuration(
        dev.slot_id,
        dev.tr_ring,
        &mut dev.tr_enq,
        &mut dev.tr_pcs,
        cfg,
    );
    // [DIAG] B — set_configuration вернулся
    kprint!("xhci: hubdiag B code=");
    write_hex!(code as u64);
    kprint!("\n");
    if code != 1 && code != 13 {
        kprint!("xhci: hub set_config failed\n");
        return false;
    }

    // Hub class descriptor (0x29): bNbrPorts[2], wHubCharacteristics[3..4].
    let buf = alloc_page();
    // [DIAG] C — буфер выделен, перед чтением hub-дескриптора
    kprint!("xhci: hubdiag C\n");
    let code = ctrl_in(
        dev.slot_id,
        dev.tr_ring,
        &mut dev.tr_enq,
        &mut dev.tr_pcs,
        0x0008_0000_2900_06A0,
        buf,
        8,
    );
    // [DIAG] D — hub-дескриптор прочитан
    kprint!("xhci: hubdiag D code=");
    write_hex!(code as u64);
    kprint!("\n");
    if code != 1 && code != 13 {
        kprint!("xhci: hub desc failed\n");
        return false;
    }
    // [DIAG] E — прошли проверку code, СЫРЫЕ первые 8 байт дескриптора.
    // Если E не печатается — висяк именно на чтении buf (хотя buf валиден);
    // если печатается, но raw=0 — DMA дескриптора не легло, а code соврал.
    kprint!("xhci: hubdiag E raw=");
    write_hex!(*(buf as *const u64));
    kprint!("\n");
    let nbr_ports = *((buf + 2) as *const u8) as u32;
    // buf+3 нечётный → прямой *const u16 = misaligned deref (паника в debug).
    let characteristics = core::ptr::read_unaligned((buf + 3) as *const u16) as u32;
    let ttt = (characteristics >> 5) & 0x3; // TT Think Time [6:5]

    kprint!("xhci: hub ports=");
    write_hex!(nbr_ports as u64);
    kprint!("\n");

    if !configure_hub_slot(dev.slot_id, nbr_ports, ttt) {
        kprint!("xhci: hub slot cfg failed\n");
        return false;
    }

    let stat_buf = alloc_page();

    for p in 1..=nbr_ports {
        hub_set_port_feature(dev, 8, p); // PORT_POWER
    }
    sleep_ms(100); // bPwrOn2PwrGood — с запасом

    for p in 1..=nbr_ports {
        hub_clear_port_feature(dev, 16, p); // C_PORT_CONNECTION

        let st = hub_get_port_status(dev, stat_buf, p);
        if (st & 1) == 0 {
            continue; // нет устройства
        }

        kprint!("xhci: hub port ");
        write_hex!(p as u64);
        kprint!(" connected\n");

        // Reset downstream-порта
        hub_set_port_feature(dev, 4, p); // PORT_RESET
        sleep_ms(50);
        let mut t = 20u32;
        loop {
            let s = hub_get_port_status(dev, stat_buf, p);
            // Reset (bit4) снялся и порт Enabled (bit1)?
            if (s & (1 << 4)) == 0 && (s & (1 << 1)) != 0 {
                break;
            }
            t -= 1;
            if t == 0 {
                break;
            }
            sleep_ms(10);
        }
        hub_clear_port_feature(dev, 20, p); // C_PORT_RESET
        sleep_ms(10); // reset recovery

        let st = hub_get_port_status(dev, stat_buf, p);
        // Скорость: Low Speed bit9, High Speed bit10, иначе Full.
        let child_speed = if (st & (1 << 9)) != 0 {
            2
        } else if (st & (1 << 10)) != 0 {
            3
        } else {
            1
        };

        // Route String: 4 бита на ярус, этот хаб на ярусе depth → порт в нибл depth.
        let nib = if p > 15 { 15 } else { p };
        let child_route = route_string | (nib << (4 * depth));

        // TT для FS/LS-ребёнка: HS-хаб сам несёт TT (его slot/port); если хаб
        // сам FS/LS — наследуем TT, заданный выше по дереву. HS-ребёнку TT не нужен.
        let (c_tt_slot, c_tt_port) = if child_speed == 3 {
            (0, 0)
        } else if hub_speed == 3 {
            (dev.slot_id, p)
        } else {
            (parent_tt_hub_slot, parent_tt_port)
        };

        if enumerate_device(
            child_route,
            root_port,
            child_speed,
            c_tt_slot,
            c_tt_port,
            depth + 1,
        ) {
            return true;
        }
    }

    false
}

// ====================== Init ======================

pub unsafe fn init(bar0: u64) {
    kprint!("xhci: init bar=");
    write_hex!(bar0);
    kprint!("\n");

    let cap_length = (r32(bar0) & 0xFF) as u64;
    let hcs_params1 = r32(bar0 + 4);
    let hcs_params2 = r32(bar0 + 8);
    let max_slots = hcs_params1 & 0xFF;
    let max_ports = (hcs_params1 >> 24) & 0xFF;
    let hcc_params1 = r32(bar0 + 0x10);
    let csz = (hcc_params1 >> 2) & 1;
    let ctx_size: u64 = if csz == 1 { 64 } else { 32 };
    CTX_SIZE = ctx_size;

    // Max Scratchpad Buffers (HCSPARAMS2): Hi = биты [25:21], Lo = биты [31:27],
    // итог = (Hi << 5) | Lo. Раньше Hi/Lo были перепутаны → неверное число
    // буферов: либо лишние аллокации (исчерпание PMM), либо нехватка →
    // контроллер DMA-ит в невалидный scratchpad → порча памяти.
    let max_scratch = ((((hcs_params2 >> 21) & 0x1F) << 5) | ((hcs_params2 >> 27) & 0x1F)) as u64;

    let op_base = bar0 + cap_length;
    let rt_base = bar0 + r32(bar0 + 0x18) as u64;
    let db_base = bar0 + r32(bar0 + 0x14) as u64;

    RT_BASE = rt_base;
    DB_BASE = db_base;
    OP_BASE = op_base;

    kprint!("xhci: max_ports=");
    write_hex!(max_ports as u64);
    kprint!(" ctx_size=");
    write_hex!(ctx_size);
    kprint!(" scratch=");
    write_hex!(max_scratch);
    kprint!("\n");

    let usbcmd = op_base; // +0x00 USBCMD
    let usbsts = op_base + 4; // +0x04 USBSTS

    // --- 1. Останавливаем контроллер (R/S=0) и ждём HCHalted=1 ---
    if r32(usbsts) & 1 == 0 {
        w32(usbcmd, r32(usbcmd) & !1);
        let mut t = 1_000_000u32;
        while (r32(usbsts) & 1) == 0 && t > 0 {
            t -= 1;
            core::arch::asm!("pause");
        }
    }

    // --- 2. Host Controller Reset (HCRST = бит 1) ---
    // После OS Handoff контроллер в неизвестном состоянии от BIOS.
    // Чистый сброс — единственный надёжный способ привести его в default.
    w32(usbcmd, r32(usbcmd) | (1 << 1));
    let mut t = 5_000_000u32;
    while (r32(usbcmd) & (1 << 1)) != 0 && t > 0 {
        t -= 1;
        core::arch::asm!("pause");
    }

    // --- 3. Ждём CNR (Controller Not Ready, USBSTS бит 11) = 0 ---
    let mut t = 5_000_000u32;
    while (r32(usbsts) & (1 << 11)) != 0 && t > 0 {
        t -= 1;
        core::arch::asm!("pause");
    }
    kprint!("xhci: reset done\n");

    // --- MaxSlotsEn ---
    let config_reg = op_base + 0x38;
    w32(config_reg, (r32(config_reg) & !0xFF) | (max_slots & 0xFF));

    // --- DCBAA ---
    let dcbaa = alloc_page();
    DCBAA = dcbaa;

    // --- Scratchpad Buffer Array → DCBAA[0] ---
    // Обязательно на реальном железе если max_scratch > 0, иначе контроллер
    // не может обслуживать транзакции → Transaction Error (code=4) на портах.
    if max_scratch > 0 {
        let scratch_arr = alloc_page();
        for i in 0..max_scratch {
            let buf = alloc_page(); // страница 4 KB, выровнена
            w64(scratch_arr + i * 8, buf);
        }
        w64(dcbaa, scratch_arr); // DCBAA[0] = указатель на массив scratchpad
    }

    w64(op_base + 0x30, dcbaa);

    // --- Command Ring ---
    let cmd_ring = alloc_page();
    let link = (cmd_ring + 31 * 16) as *mut Trb;
    (*link).param = cmd_ring;
    (*link).status = 0;
    (*link).control = TRB_LINK | (1 << 1) | 1;
    CMD_RING = cmd_ring;
    CMD_ENQ = 0;
    CMD_PCS = 1;
    w64(op_base + 0x18, cmd_ring | 1); // CRCR | RCS

    // --- Event Ring ---
    let evt_ring = alloc_page();
    EVT_RING = evt_ring;
    EVT_DEQ = 0;
    EVT_CCS = 1;

    // ERST живёт в нашей RAM (контроллер читает её по DMA) — обычная запись
    let erst = alloc_page();
    *((erst) as *mut u64) = evt_ring;
    *((erst + 8) as *mut u64) = EVT_RING_SIZE as u64;

    // Interrupter 0 — это MMIO рантайм-регистры → volatile
    let ir0 = rt_base + 0x20;
    w32(ir0 + 0x08, 1); // ERSTSZ = 1
    w64(ir0 + 0x10, erst); // ERSTBA
    w64(ir0 + 0x18, evt_ring | (1 << 3)); // ERDP | EHB

    // --- Запуск (R/S = 1) ---
    w32(usbcmd, r32(usbcmd) | 1);
    let mut t = 1_000_000u32;
    while (r32(usbsts) & 1) != 0 && t > 0 {
        t -= 1;
        core::arch::asm!("pause");
    }

    kprint!("xhci: running\n");

    // Включаем питание на всех портах (PP = бит 9)
    for port in 0..max_ports {
        let portsc = op_base + 0x400 + port as u64 * 0x10;
        w32(portsc, r32(portsc) | (1 << 9));
    }
    sleep_ms(200);

    // Съедаем Port Status Change события при старте
    for _ in 0..64 {
        if let Some((typ, _, _)) = consume_event() {
            if typ != EVT_PORT_CHANGE {
                break;
            }
        } else {
            break;
        }
    }

    // === Цикл по портам ===
    for port in 0..max_ports {
        let portsc = op_base + 0x400 + port as u64 * 0x10;

        if r32(portsc) & 1 == 0 {
            continue;
        }

        kprint!("xhci: port ");
        write_hex!(port as u64);
        kprint!(" connected\n");

        // Port Reset (PR = бит 4). RW1C-биты статуса (CSC/PEC/...) маскируем,
        // чтобы случайно их не сбросить при записи.
        w32(portsc, (r32(portsc) & !0x00FF_F1FE) | (1 << 4));
        sleep_ms(50);
        let mut t = 2_000_000u32;
        while (r32(portsc) & (1 << 4)) != 0 && t > 0 {
            t -= 1;
            core::arch::asm!("pause");
        }

        // После сброса порт должен стать Enabled (PED = бит 1)
        if (r32(portsc) & 0b10) == 0 {
            kprint!("xhci: port reset failed\n");
            continue;
        }

        // Сбрасываем ВСЕ RW1C change-биты (17..23): CSC/PEC/WRC/OCC/PRC/PLC/CEC.
        // Раньше гасили только CSC+PRC — PLC на USB3-портах сыпался пачками и
        // забивал Event Ring.
        w32(portsc, (r32(portsc) & !0x00FF_F1FE) | 0x00FE_0000);

        let speed = (r32(portsc) >> 10) & 0xF;

        // Root-порт: route string 0, root hub port = port+1, без TT, ярус 0.
        // Внутри: Enable Slot → Address Device → дескрипторы → HID или обход хаба.
        if enumerate_device(0, port + 1, speed, 0, 0, 0) {
            break; // клавиатура найдена — достаточно
        }
    }

    kprint!("xhci: init done\n");
}
