use crate::{kprint, pmm, write_hex, CONSOLE};

// Identity map покрывает всю физическую память (boot.s: pdpt0/pdpt1 × 1GB huge pages)
// phys == virt, KVIRT не нужен

unsafe fn alloc_page() -> u64 {
    let phys = pmm::alloc();
    core::ptr::write_bytes(phys as *mut u8, 0, 4096);
    phys
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
pub const TRB_LINK: u32 = 6 << 10;
pub const TRB_SETUP: u32 = 2 << 10;
pub const TRB_DATA: u32 = 3 << 10;
pub const TRB_STATUS: u32 = 4 << 10;
pub const TRB_NORMAL: u32 = 1 << 10;

// Event TRB types
const EVT_TRANSFER: u32 = 32;
const EVT_CMD_COMPL: u32 = 33;
const EVT_PORT_CHANGE: u32 = 34;

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

// ====================== Event Ring ======================

unsafe fn consume_event() -> Option<(u32, u32, u32)> {
    let trb = (EVT_RING + EVT_DEQ as u64 * 16) as *const Trb;
    let ctrl = (*trb).control;

    if (ctrl & 1) != EVT_CCS {
        return None;
    }

    let trb_type = (ctrl >> 10) & 0x3F;
    let slot_id = (ctrl >> 24) as u32;
    let code = ((*trb).status >> 24) & 0xFF;

    EVT_DEQ += 1;
    if EVT_DEQ == 32 {
        EVT_DEQ = 0;
        EVT_CCS ^= 1;
    }

    // Identity map: EVT_RING уже физический адрес
    let erdp_phys = EVT_RING + EVT_DEQ as u64 * 16;
    *((RT_BASE + 0x20 + 0x18) as *mut u64) = erdp_phys | (1 << 3);

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
            continue;
        }
        timeout -= 1;
        if timeout == 0 {
            kprint!("xhci: event timeout\n");
            return (0xFF, 0);
        }
        core::arch::asm!("pause");
    }
}

// ====================== Command Ring ======================

unsafe fn post_command(param: u64, status: u32, control: u32) {
    let trb = (CMD_RING + CMD_ENQ as u64 * 16) as *mut Trb;
    (*trb).param = param;
    (*trb).status = status;
    (*trb).control = control | CMD_PCS;

    CMD_ENQ += 1;
    if CMD_ENQ == 31 {
        CMD_ENQ = 0;
        CMD_PCS ^= 1;
    }
    *(DB_BASE as *mut u32) = 0;
}

// ====================== Control Transfer (EP0) ======================

unsafe fn ctrl_in(
    slot_id: u32,
    tr_ring: u64,
    tr_enq: &mut usize,
    tr_pcs: &mut u32,
    setup_param: u64,
    buf: u64,
    len: u16,
) -> u32 {
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
    if *tr_enq >= 31 {
        *tr_enq = 0;
        *tr_pcs ^= 1;
    }

    *((DB_BASE + slot_id as u64 * 4) as *mut u32) = 1;

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
    if *tr_enq >= 31 {
        *tr_enq = 0;
        *tr_pcs ^= 1;
    }

    *((DB_BASE + slot_id as u64 * 4) as *mut u32) = 1;

    let (code, _) = wait_event(EVT_TRANSFER);
    code
}

// ====================== Descriptors ======================

unsafe fn get_device_descriptor(slot_id: u32, tr_ring: u64, tr_enq: &mut usize, tr_pcs: &mut u32) {
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
        return;
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
            let mp = *((buf + offset + 4) as *const u16);
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

unsafe fn configure_hid_endpoint(slot_id: u32, ep_addr: u8, max_packet: u16, interval: u8) -> u64 {
    let ep_idx = ((ep_addr & 0xF) * 2 + if ep_addr & 0x80 != 0 { 1 } else { 0 }) as u64;

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
    *ep_ctx.add(0) = ((interval as u32) << 16); // Interval
    *ep_ctx.add(1) = ((max_packet as u32) << 16) | (7 << 3) | (3 << 1); // Type=Interrupt IN
    *(ep_ctx.add(2) as *mut u64) = tr_ring | 1; // TR Dequeue | DCS

    // Configure Endpoint command
    post_command(input_ctx, 0, TRB_CONFIGURE_EP | (slot_id << 24));
    let (code, _) = wait_event(EVT_CMD_COMPL);
    if code != 1 {
        kprint!("xhci: configure ep failed code=");
        write_hex!(code as u64);
        kprint!("\n");
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
        HID_TR_ENQ = 0;
        HID_TR_PCS ^= 1;
    }

    // Doorbell для нужного endpoint
    *((DB_BASE + HID_SLOT as u64 * 4) as *mut u32) = ep_idx as u32;
}

// Опрос HID — вызывается из основного цикла или таймера
pub unsafe fn poll_hid() {
    if !HID_READY {
        return;
    }

    // Проверяем есть ли Transfer Event
    let trb = (EVT_RING + EVT_DEQ as u64 * 16) as *const Trb;
    let ctrl = (*trb).control;
    if (ctrl & 1) != EVT_CCS {
        return;
    }

    let trb_type = (ctrl >> 10) & 0x3F;
    if trb_type != EVT_TRANSFER {
        return;
    }

    // Съедаем событие
    EVT_DEQ += 1;
    if EVT_DEQ == 32 {
        EVT_DEQ = 0;
        EVT_CCS ^= 1;
    }
    let erdp_phys = EVT_RING + EVT_DEQ as u64 * 16;
    *((RT_BASE + 0x20 + 0x18) as *mut u64) = erdp_phys | (1 << 3);

    // Читаем HID Boot Protocol report (8 байт):
    // [0] Modifier, [1] Reserved, [2..7] Keycodes
    let modifier = *(HID_BUF as *const u8);
    let shift = modifier & 0x22 != 0; // Left/Right Shift

    for i in 2..8usize {
        let keycode = *((HID_BUF + i as u64) as *const u8);
        if keycode == 0 {
            continue;
        }
        if let Some(ch) = hid_keycode_to_char(keycode, shift) {
            (&raw mut crate::CONSOLE)
                .as_mut()
                .unwrap()
                .as_mut()
                .unwrap()
                .write_byte(ch);
        }
    }

    // Сразу ставим следующий transfer
    let ep_idx = ((HID_EP_ADDR & 0xF) * 2 + if HID_EP_ADDR & 0x80 != 0 { 1 } else { 0 }) as u64;
    queue_hid_transfer(ep_idx);
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

// ====================== Init ======================

pub unsafe fn init(bar0: u64) {
    kprint!("xhci: init bar=");
    write_hex!(bar0);
    kprint!("\n");

    let cap_length = *(bar0 as *const u8) as u64;
    let hcs_params1 = *((bar0 + 4) as *const u32);
    let max_slots = hcs_params1 & 0xFF;
    let max_ports = (hcs_params1 >> 24) & 0xFF;
    let hcc_params1 = *((bar0 + 0x10) as *const u32);
    let csz = (hcc_params1 >> 2) & 1;
    let ctx_size: u64 = if csz == 1 { 64 } else { 32 };
    CTX_SIZE = ctx_size;

    let op_base = bar0 + cap_length;
    let rt_base = bar0 + *((bar0 + 0x18) as *const u32) as u64;
    let db_base = bar0 + *((bar0 + 0x14) as *const u32) as u64;

    RT_BASE = rt_base;
    DB_BASE = db_base;
    OP_BASE = op_base;

    kprint!("xhci: max_ports=");
    write_hex!(max_ports as u64);
    kprint!(" ctx_size=");
    write_hex!(ctx_size);
    kprint!("\n");

    // --- НЕ сбрасываем контроллер ---
    // BIOS уже инициализировал xHCI и включил питание портов.
    // OS Handoff передал управление. Просто останавливаем и перезапускаем
    // без полного сброса чтобы не потерять питание портов.
    let usbcmd = op_base as *mut u32;
    let usbsts = (op_base + 4) as *const u32;

    // Останавливаем если работает
    if *usbsts & 1 == 0 {
        *usbcmd = *usbcmd & !1;
        let mut t = 1_000_000u32;
        while (*usbsts & 1) == 0 && t > 0 {
            t -= 1;
        }
    }
    kprint!("xhci: reset done\n");

    // --- MaxSlotsEn ---
    let config_reg = (op_base + 0x38) as *mut u32;
    *config_reg = (*config_reg & !0xFF) | (max_slots & 0xFF);

    // --- DCBAA ---
    let dcbaa = alloc_page();
    DCBAA = dcbaa;
    *((op_base + 0x30) as *mut u64) = dcbaa;

    // --- Command Ring ---
    let cmd_ring = alloc_page();
    let link = (cmd_ring + 31 * 16) as *mut Trb;
    (*link).param = cmd_ring;
    (*link).status = 0;
    (*link).control = TRB_LINK | (1 << 1) | 1;
    CMD_RING = cmd_ring;
    CMD_ENQ = 0;
    CMD_PCS = 1;
    *((op_base + 0x18) as *mut u64) = cmd_ring | 1;

    // --- Event Ring ---
    let evt_ring = alloc_page();
    EVT_RING = evt_ring;
    EVT_DEQ = 0;
    EVT_CCS = 1;

    let erst = alloc_page();
    *((erst) as *mut u64) = evt_ring;
    *((erst + 8) as *mut u64) = 32;

    let ir0 = rt_base + 0x20;
    *((ir0 + 0x08) as *mut u32) = 1;
    *((ir0 + 0x10) as *mut u64) = erst;
    *((ir0 + 0x18) as *mut u64) = evt_ring | (1 << 3);

    // --- Запуск ---
    *usbcmd = *usbcmd | 1;
    let mut t = 1_000_000u32;
    while (*usbsts & 1) != 0 && t > 0 {
        t -= 1;
    }

    kprint!("xhci: running\n");

    // Включаем питание на всех портах (PP = бит 9)
    for port in 0..max_ports {
        let portsc = (op_base + 0x400 + port as u64 * 0x10) as *mut u32;
        *portsc = *portsc | (1 << 9);
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
        let portsc = (op_base + 0x400 + port as u64 * 0x10) as *mut u32;

        if *portsc & 1 == 0 {
            continue;
        }

        kprint!("xhci: port ");
        write_hex!(port as u64);
        kprint!(" connected\n");

        // Port Reset
        *portsc = (*portsc & !0x00FF_F1FE) | (1 << 4);
        sleep_ms(50);
        let mut t = 2_000_000u32;
        while (*portsc & (1 << 4)) != 0 && t > 0 {
            t -= 1;
        }

        if (*portsc & 0b10) == 0 {
            kprint!("xhci: port reset failed\n");
            continue;
        }

        *portsc = *portsc | (1 << 17); // сброс CSC

        // Enable Slot
        post_command(0, 0, TRB_ENABLE_SLOT);
        let (code, slot_id) = wait_event(EVT_CMD_COMPL);
        if code != 1 {
            kprint!("xhci: enable slot failed\n");
            continue;
        }

        kprint!("xhci: slot_id=");
        write_hex!(slot_id as u64);
        kprint!("\n");

        let speed = (*portsc >> 10) & 0xF;

        // Input Context
        let input_ctx = alloc_page();
        *((input_ctx + 4) as *mut u32) = 0b11;

        let slot_ctx = input_ctx + ctx_size;
        *((slot_ctx) as *mut u32) = (speed << 20) | (1 << 27);
        *((slot_ctx + 4) as *mut u32) = (port + 1) << 16;

        // Transfer Ring для EP0
        let tr_ring = alloc_page();
        let mut tr_enq: usize = 0;
        let mut tr_pcs: u32 = 1;

        let link = (tr_ring + 31 * 16) as *mut Trb;
        (*link).param = tr_ring;
        (*link).control = TRB_LINK | (1 << 1) | 1;

        // EP0 Context
        let ep0_ctx = input_ctx + ctx_size * 2;
        let max_pkt: u32 = match speed {
            2 => 8,
            3 => 64,
            4 => 512,
            _ => 64,
        };
        *((ep0_ctx + 4) as *mut u32) = (max_pkt << 16) | (4 << 3) | (3 << 1);
        *((ep0_ctx + 8) as *mut u64) = tr_ring | 1;

        // Output Context → DCBAA
        let out_ctx = alloc_page();
        *((DCBAA + slot_id as u64 * 8) as *mut u64) = out_ctx;

        // Address Device
        post_command(input_ctx, 0, TRB_ADDRESS_DEVICE | (slot_id << 24));
        let (code, _) = wait_event(EVT_CMD_COMPL);
        if code != 1 {
            kprint!("xhci: address device failed code=");
            write_hex!(code as u64);
            kprint!("\n");
            continue;
        }

        kprint!("xhci: device addressed\n");

        get_device_descriptor(slot_id, tr_ring, &mut tr_enq, &mut tr_pcs);

        let (cfg, ep_addr, interval, maxp, iface_class) =
            get_config_descriptor(slot_id, tr_ring, &mut tr_enq, &mut tr_pcs);

        kprint!("xhci: cfg=");
        write_hex!(cfg as u64);
        kprint!(" ep=");
        write_hex!(ep_addr as u64);
        kprint!(" iface_class=");
        write_hex!(iface_class as u64);
        kprint!("\n");

        if iface_class != 3 {
            continue; // не HID
        }

        kprint!("xhci: HID device found\n");

        // Set Configuration
        let code = set_configuration(slot_id, tr_ring, &mut tr_enq, &mut tr_pcs, cfg);
        if code != 1 && code != 13 {
            kprint!("xhci: set_config failed code=");
            write_hex!(code as u64);
            kprint!("\n");
            continue;
        }

        // Set Protocol = Boot Protocol (0)
        let code = set_protocol(slot_id, tr_ring, &mut tr_enq, &mut tr_pcs);
        if code != 1 && code != 13 {
            kprint!("xhci: set_protocol failed\n");
            // не критично — продолжаем
        }

        // Configure Endpoint
        let ep_idx = configure_hid_endpoint(slot_id, ep_addr, maxp, interval);
        if ep_idx == 0 {
            continue;
        }

        // Запускаем первый transfer
        queue_hid_transfer(ep_idx);

        kprint!("xhci: keyboard ready\n");
        break; // нашли клавиатуру — достаточно
    }

    kprint!("xhci: init done\n");
}
