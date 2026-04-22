use crate::{
    pmm::{self, alloc},
    CONSOLE,
};

#[repr(C)]
pub struct Trb {
    pub param: u64,
    pub status: u32,
    pub control: u32,
}

// Trb types
pub const TRB_ENABLE_SLOT: u32 = 9 << 10;
pub const TRB_ADDRESS_DEVICE: u32 = 11 << 10;
pub const TRB_CONFIGURE_EP: u32 = 12 << 10;
pub const TRB_TRANSFER: u32 = 1 << 10; // Normal
pub const TRB_LINK: u32 = 6 << 10;
pub const TRB_COMMAND_COMPL: u32 = 33 << 10; // Event
pub const TRB_PORT_STATUS: u32 = 34 << 10; // Event
pub const TRB_TRANSFER_EVENT: u32 = 32 << 10; // Event
pub const TRB_SETUP: u32 = 2 << 10;
pub const TRB_DATA: u32 = 3 << 10;
pub const TRB_STATUS: u32 = 4 << 10;

// Глобальное состояние xHCI
static mut CMD_RING: u64 = 0;
static mut CMD_ENQ: usize = 0;
static mut CMD_PCS: u32 = 1;

static mut EVT_RING: u64 = 0;
static mut EVT_DEQ: usize = 0;
static mut EVT_CCS: u32 = 1;

static mut RT_BASE: u64 = 0;
static mut DB_BASE: u64 = 0;

static mut TR_ENQ: usize = 0;
static mut TR_PCS: u32 = 1;

pub struct Ring {
    pub trbs: *mut Trb, // физ. адрес (из pmm::alloc())
    pub enq: usize,     // индекс следующего TRB
    pub pcs: u32,       // Producer Cycle State (1 или 0)
}

/// Отправить TRB в Command Ring и позвонить в дверной звонок
unsafe fn post_command(param: u64, status: u32, control: u32) {
    let trb = (CMD_RING + CMD_ENQ as u64 * 16) as *mut Trb;
    (*trb).param = param;
    (*trb).status = status;
    (*trb).control = control | CMD_PCS;

    CMD_ENQ += 1;
    if CMD_ENQ == 31
    /* дошли до Link TRB */
    {
        CMD_ENQ = 0;
        CMD_PCS ^= 1;
    }

    // Doorbell 0 = Host Controller
    *(DB_BASE as *mut u32) = 0;
}

/// Ждать Command Completion Event из Event Ring
unsafe fn wait_command() -> (u32, u32) {
    loop {
        let trb = (EVT_RING + EVT_DEQ as u64 * 16) as *const Trb;
        if (*trb).control & 1 != EVT_CCS {
            continue;
        }

        let trb_type = ((*trb).control >> 10) & 0x3F;
        let slot_id = ((*trb).control >> 24) as u32;
        let code = ((*trb).status >> 24) & 0xFF;

        EVT_DEQ += 1;
        if EVT_DEQ == 32 {
            EVT_DEQ = 0;
            EVT_CCS ^= 1;
        }
        // Bit 3 = EHB: write 1 to clear; without this the controller stops posting events
        *((RT_BASE + 0x38) as *mut u64) = (EVT_RING + EVT_DEQ as u64 * 16) | (1 << 3);

        if trb_type == 33 {
            // Command Completion Event
            return (code, slot_id);
        }
    }
}

unsafe fn wait_transfer() -> u32 {
    loop {
        let trb = (EVT_RING + EVT_DEQ as u64 * 16) as *const Trb;
        if (*trb).control & 1 != EVT_CCS {
            continue;
        }

        let trb_type = ((*trb).control >> 10) & 0x3F;
        let code = ((*trb).status >> 24) & 0xFF;

        EVT_DEQ += 1;
        if EVT_DEQ == 32 {
            EVT_DEQ = 0;
            EVT_CCS ^= 1;
        }
        *((RT_BASE + 0x38) as *mut u64) = (EVT_RING + EVT_DEQ as u64 * 16) | (1 << 3);

        if trb_type == 32 {
            return code;
        }
    }
}

unsafe fn ctrl_in(slot_id: u32, transfer_ring: u64, setup_param: u64, buf: u64, len: u16) -> u32 {
    let e0 = TR_ENQ;
    let e1 = TR_ENQ + 1;
    let e2 = TR_ENQ + 2;

    let setup = (transfer_ring + e0 as u64 * 16) as *mut Trb;
    (*setup).param = setup_param;
    (*setup).status = 8;
    (*setup).control = TRB_SETUP | (1 << 6) | (3 << 16) | TR_PCS;

    let data = (transfer_ring + e1 as u64 * 16) as *mut Trb;
    (*data).param = buf;
    (*data).status = len as u32;
    (*data).control = TRB_DATA | (1 << 16) | TR_PCS;

    let status_trb = (transfer_ring + e2 as u64 * 16) as *mut Trb;
    (*status_trb).param = 0;
    (*status_trb).status = 0;
    (*status_trb).control = TRB_STATUS | (1 << 5) | TR_PCS;

    TR_ENQ += 3;
    *((DB_BASE + slot_id as u64 * 4) as *mut u32) = 1;
    wait_transfer()
}

unsafe fn get_config_descriptor(slot_id: u32, transfer_ring: u64) -> (u8, u8, u8, u16, u8) {
    let buf = pmm::alloc();
    core::ptr::write_bytes(buf as *mut u8, 0, 4096);

    let code = ctrl_in(slot_id, transfer_ring, 0x0200_0000_0200_0680, buf, 512);
    crate::kprint!("cfg code=");
    crate::write_hex!(code as u64);
    crate::kprint!("\n");

    let total_len = *((buf + 2) as *const u16);
    let config_value = *((buf + 5) as *const u8);
    crate::kprint!("total_len=");
    crate::write_hex!(total_len as u64);
    crate::kprint!("\n");

    let mut offset = 0u64;
    let mut ep_addr = 0u8;
    let mut interval = 0u8;
    let mut max_packet = 0u16;
    let mut iface_class = 0u8;

    let parse_len = if total_len == 0 || total_len > 512 { 512u64 } else { total_len as u64 };
    while offset < parse_len {
        let len = *((buf + offset) as *const u8);
        if len == 0 { break; }
        let kind = *((buf + offset + 1) as *const u8);

        if kind == 4 /* Interface Descriptor */ {
            iface_class = *((buf + offset + 5) as *const u8);
        }

        if kind == 5 /* Endpoint Descriptor */ {
            let addr  = *((buf + offset + 2) as *const u8);
            let attrs = *((buf + offset + 3) as *const u8);
            let mp    = *((buf + offset + 4) as *const u16);
            let iv    = *((buf + offset + 6) as *const u8);

            if attrs & 3 == 3 && addr & 0x80 != 0 /* Interrupt IN */ {
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

unsafe fn get_descriptor(slot_id: u32, transfer_ring: u64) {
    let buf = pmm::alloc();
    core::ptr::write_bytes(buf as *mut u8, 0, 4096);

    let code = ctrl_in(slot_id, transfer_ring, 0x0012_0000_0100_0680, buf, 18);
    crate::kprint!("transfer code=");
    crate::write_hex!(code as u64);
    crate::kprint!("\n");

    let class = *((buf + 4) as *const u8);
    let vendor = *((buf + 8) as *const u16);
    let product = *((buf + 10) as *const u16);

    crate::kprint!("class=");
    crate::write_hex!(class as u64);
    crate::kprint!(" vendor=");
    crate::write_hex!(vendor as u64);
    crate::kprint!(" product=");
    crate::write_hex!(product as u64);
    crate::kprint!("\n");
}

pub unsafe fn init(bar0: u64) {
    let cap_length = *(bar0 as *const u8) as u64; // размер capability regs
    let hci_version = *((bar0 + 2) as *const u16);
    let hcs_params1 = *((bar0 + 4) as *const u32);
    let max_ports = (hcs_params1 >> 24) & 0xFF;

    let hcc_params1 = *((bar0 + 0x10) as *const u32);
    let csz = (hcc_params1 >> 2) & 1;
    let ctx_size: u64 = if csz == 1 { 64 } else { 32 };
    crate::kprint!("CSZ=");
    crate::write_hex!(csz as u64);
    crate::kprint!("\n");

    let rts_off = *((bar0 + 0x18) as *const u32) as u64 & !0x1F;
    let db_off = *((bar0 + 0x14) as *const u32) as u64 & !0x3;

    let op_base = bar0 + cap_length; // Operational Regs
    let rt_base = bar0 + rts_off; // Runtime Regs
    RT_BASE = rt_base;
    let db_base = bar0 + db_off; // Doorbell Array
    DB_BASE = db_base;

    crate::kprint!("xHCI ver=");
    crate::write_hex!(hci_version as u64);
    crate::kprint!(" ports=");
    crate::write_hex!(max_ports as u64);
    crate::kprint!("\n");

    /*  Offset в Operational Regs:
    0x00 = USBCMD
    0x04 = USBSTS
    */

    let usbcmd = (op_base + 0x00) as *mut u32;
    let usbsts = (op_base + 0x04) as *mut u32;

    // Стоп (если запущен)
    *usbcmd = *usbcmd & !1;

    // Ждём HCHalted (бит 0 в USBSTS)
    while *usbsts & 1 == 0 {}

    //Сброс
    *usbcmd = *usbcmd | (1 << 1); // HCRST
    while (*usbcmd & (1 << 1)) != 0 {}
    // Ждём CNR (Controller Not Ready, USBSTS bit 11)
    while (*usbsts & (1 << 11)) != 0 {}

    crate::kprint!("xHCI reset ok\n");

    //DCBAAP — массив указателей на Device Context (по одному на слот)
    // 64 слота × 8 байт, выровнено на 64 байта
    let dcbaap = pmm::alloc(); // страница 4KB — хватит
                               // обнулить
    core::ptr::write_bytes(dcbaap as *mut u8, 0, 4096);
    // записать в контроллер (offset 0x30 в Op Regs)
    *((op_base + 0x30) as *mut u64) = dcbaap;

    // CONFIG: выставить MaxSlotsEn = 64 (offset 0x38)
    *((op_base + 0x38) as *mut u32) = 64;

    //Command Ring (32 TRB = 512 байт, выровнено на 64)
    let cmd_ring = pmm::alloc();
    core::ptr::write_bytes(cmd_ring as *mut u8, 0, 4096);
    CMD_RING = cmd_ring;
    // последний TRB = Link TRB (замыкает кольцо)
    let link = (cmd_ring + 31 * 16) as *mut Trb;
    (*link).param = cmd_ring; // указывает на начало
    (*link).status = 0;
    (*link).control = TRB_LINK | 1; // TC=1 (Toggle Cycle)
                                    // CRCR (offset 0x18): адрес + CCS=1
    *((op_base + 0x18) as *mut u64) = cmd_ring | 1;

    //Event Ring (32 TRB) + ERST (Event Ring Segment Table, 1 запись)
    let evt_ring = pmm::alloc();
    core::ptr::write_bytes(evt_ring as *mut u8, 0, 4096);
    EVT_RING = evt_ring;

    let erst = pmm::alloc(); // ERST: одна запись = 16 байт
    *((erst + 0) as *mut u64) = evt_ring; // базовый адрес сегмента
    *((erst + 8) as *mut u32) = 32; // размер сегмента (TRB)

    // Runtime Regs, Interrupter 0:
    // ERSTSZ (0x28), ERSTBA (0x30), ERDP (0x38)
    *((rt_base + 0x28) as *mut u32) = 1; // 1 сегмент
    *((rt_base + 0x38) as *mut u64) = evt_ring; // ERDP = начало
    *((rt_base + 0x30) as *mut u64) = erst; // ERSTBA

    //RUN
    *usbcmd = *usbcmd | 1;
    while (*usbsts & 1) != 0 {} // ждём снятия HCHalted
    crate::kprint!("xHCI run\n");

    /*
    Port Status and Control — offset в Op Regs
    0x400 + (port_index * 0x10)
    Бит 0 (CCS) = устройство подключено
    Бит 1 (PED) = порт включён
    Бит 9 (PP)  = питание порта
    */

    for i in 0..max_ports {
        let portsc = (op_base + 0x400 + i as u64 * 0x10) as *mut u32;
        let val = *portsc;
        if val & 1 == 0 {
            continue;
        } // CCS=0, никого нет

        crate::kprint!("Port ");
        crate::write_hex!(i as u64);
        crate::kprint!(" status=");
        crate::write_hex!(val as u64);
        crate::kprint!("\n");

        // Сброс порта (бит 4 = PR)
        *portsc = (val & !0xFFFF_F0E0) | (1 << 4);
        // Ждём снятия PR
        while (*portsc & (1 << 4)) != 0 {}
        crate::kprint!("Port reset ok\n");

        // После while (*portsc & (1 << 4)) != 0 {}
        // Ждём PED (бит 1)
        let mut timeout = 100_000u32;
        while (*portsc & 0b10) == 0 && timeout > 0 {
            timeout -= 1;
        }

        if (*portsc & 0b10) == 0 {
            crate::kprint!("Port timeout!\n");
            continue;
        }
        crate::kprint!("Port enabled\n");

        // Enable Slot Command (тип = 9)
        post_command(0, 0, TRB_ENABLE_SLOT);
        let (code, slot_id) = wait_command();
        crate::kprint!("Slot code=");
        crate::write_hex!(code as u64);
        crate::kprint!(" id=");
        crate::write_hex!(slot_id as u64);
        crate::kprint!("\n");

        if code != 1 {
            continue;
        } // не Success — пропустить

        // Скорость порта из PORTSC bits 13:10
        let speed = (*portsc >> 10) & 0xF;

        //Выделить и обнулить Input Context (96 байт, но alloc даёт 4KB — ок)
        let input_ctx = pmm::alloc();
        core::ptr::write_bytes(input_ctx as *mut u8, 0, 4096);

        //Input Control Context: Add Slot + EP0
        *((input_ctx + 4) as *mut u32) = 0b11;

        //Slot Context DW0
        let slot_dw0 = (speed << 20) | (1 << 27);
        *((input_ctx + ctx_size) as *mut u32) = slot_dw0;
        *((input_ctx + ctx_size + 4) as *mut u32) = (i + 1) << 16;

        //Transfer Ring
        let transfer_ring = pmm::alloc();
        core::ptr::write_bytes(transfer_ring as *mut u8, 0, 4096);
        let transfer_link = (transfer_ring + 31 * 16) as *mut Trb;
        (*transfer_link).param = transfer_ring;
        (*transfer_link).status = 0;
        (*transfer_link).control = TRB_LINK | 1 | (1 << 1); // cycle=1, TC=1

        //EP0 Context
        let max_packet: u32 = match speed {
            2 => 8,   //LS
            3 => 64,  //HS
            4 => 512, //SS
            _ => 64,  //FS
        };

        *((input_ctx + ctx_size * 2 + 4) as *mut u32) = (max_packet << 16) | (4 << 3);
        *((input_ctx + ctx_size * 2 + 8) as *mut u64) = transfer_ring | 1;

        let out_ctx = pmm::alloc();
        core::ptr::write_bytes(out_ctx as *mut u8, 0, 4096);
        // dcbaap — адрес массива, slot_id — индекс
        *((dcbaap + slot_id as u64 * 8) as *mut u64) = out_ctx;

        post_command(input_ctx, 0, TRB_ADDRESS_DEVICE | (slot_id << 24));
        let (code2, _) = wait_command();
        crate::kprint!("AddrDev code=");
        crate::write_hex!(code2 as u64);
        crate::kprint!("\n");

        if code2 == 1 {
            TR_ENQ = 0;
            TR_PCS = 1;
            get_descriptor(slot_id, transfer_ring);
            let (cfg, ep_addr, interval, max_packet, iface_class) =
                get_config_descriptor(slot_id, transfer_ring);
            crate::kprint!("cfg=");
            crate::write_hex!(cfg as u64);
            crate::kprint!(" ep=");
            crate::write_hex!(ep_addr as u64);
            crate::kprint!(" class=");
            crate::write_hex!(iface_class as u64);
            crate::kprint!("\n");

            if iface_class != 3 {
                crate::kprint!("not HID, skip\n");
                continue;
            }
            if ep_addr == 0 {
                crate::kprint!("no interrupt IN ep, skip\n");
                continue;
            }
            crate::kprint!("HID device found\n");
        }
    }
}
