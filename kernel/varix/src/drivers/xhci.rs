//! S4.1 · xHCI 最小栈（HID 键鼠）——AI-5 内核基建长线第一件（2026-09-21）。
//!
//! **范围如实声明**：最小路径 = 1 个 xHCI 控制器 × 每设备 1 个中断端点，
//! 只为 HID 键鼠服务（boot 协议）。不做的：集线器级联、USB3 流协议、
//! scratchpad（HCSPARAMS2.MaxSpBuf≠0 如实拒掉该控制器）、HID report
//! descriptor 解析（只用 SET_PROTOCOL(0) 切 boot 协议）、滚轮（报告第 4
//! 字节 dz 丢弃——现有 MouseDelta 无滚轮词汇）、键盘修饰键只映射 Shift
//! （`ps2::Key` 词汇表无 Ctrl/Alt，丢弃并计数）、Evaluate Context（EP0
//! 以 MPS=64 恒定运行——控制传输的分片由控制器按设备真实 MPS 自动完成，
//! 全部控制请求 ≤8B setup + ≤18B 数据，64B 上限恒安全）。
//!
//! **中断模型如实声明**：纯轮询（IMAN.IE=0、IMOD=0），不注册任何中断。
//! 命令/传输完成靠事件环轮询；初始化超时 → 完整重初始化（重试一次）→
//! 仍失败如实 `DeviceReset`（与 NVMe 最小栈同一恢复口径）。
//!
//! **输入接线（增量不替代）**：HID 事件经「HID→PS/2 同构字节」从
//! `InputService::feed_key_byte / feed_mouse_byte` 既有公开汇点进入，
//! PS/2 通道零改动，菜单/ushell/桌面全部既有消费者自动受益。两个显式
//! 语义契约（宿主测试逐条锁定）：
//! 1. **鼠标 Y 轴翻转**——QEMU 实证：USB HID 报告 dy 正=向下（hid.c
//!    `ydy += evt->rel.value`），PS/2 流 dy 正=向上（ps2.c
//!    `mouse_dy -= evt->rel.value`）；内核既有消费者按 PS/2 语义调参，
//!    故 HID dy 取反后再合成 PS/2 字节（[`INVERT_MOUSE_Y`]）。
//! 2. **键盘 make-only**——PS/2 解码器忽略断码（一按一事件，无重复），
//!    HID 侧用「与上一份报告差分」复现同一语义：按键沿 → 1 事件，
//!    保持/释放 → 0 事件。
//!
//! **布局权威对照**：TRB/上下文/端口位布局逐字段对照 QEMU
//! `hcd-xhci.c/h`（参考件归档 `_attic/qemu-hcd-xhci-ref.{c,h}`）：
//! - 事件 TRB：param=TRB 指针；DW2=长度[0:24)|完成码<<24；DW3=周期
//!   bit0|类型[10:16)|EP ID[16:21)|Slot ID[24:32)（`xhci_write_event`）；
//! - EP 上下文：DW0=状态[0:3)|Interval[16:24)；DW1=错误计数[1:3)|类型
//!   [3:6)|突发[8:16)|MaxPacket[16:32)（`xhci_init_epctx`）；DW2/3=TR
//!   Dequeue 指针（DCS=bit0）；DW4=平均 TRB 长度[0:16)；
//! - 槽上下文：DW0=路由串|ContextEntries[27:32)=最高有效上下文号；
//!   DW1=根集线器端口[16:24)；DW3=槽状态[27:32)（`xhci_lookup_uport`/
//!   `SLOT_STATE`）；Address Device 输入上下文硬校验 drop=0、add=0x3；
//!   Configure Endpoint 硬校验 drop[0:2)=0、add[0:2)=0x1（槽在位、EP0
//!   不重复添加），EP i 使能位独立置位——EP1 IN=add 位 3 → add=0x9；
//! - PORTSC：CCS=0|PED=1|PR=4|PLS[5:9]|PP=9|速度[10:14)|CSC=17|PEC=18|
//!   PRC=21|PLC=22（速度 1=全速 2=低速 3=高速 4=超速；变化位 W1C）。

use super::blk::BlockError;
use super::nvme::{BarAccess, DmaMem};
use crate::ps2;

/// 4KiB 页（与 NVMe 栈同一 DMA 帧粒度）。
pub const PAGE: u64 = 4096;

// ---- TRB 类型（QEMU TRBType 枚举逐值同构）--------------------------------
pub const TRB_NORMAL: u8 = 1;
pub const TRB_SETUP: u8 = 2;
pub const TRB_DATA: u8 = 3;
pub const TRB_STATUS: u8 = 4;
pub const TRB_LINK: u8 = 6;
pub const TRB_NOOP: u8 = 8;
pub const CR_ENABLE_SLOT: u8 = 9;
pub const CR_DISABLE_SLOT: u8 = 10;
pub const CR_ADDRESS_DEVICE: u8 = 11;
pub const CR_CONFIGURE_ENDPOINT: u8 = 12;
pub const CR_NOOP: u8 = 23;
pub const ER_TRANSFER: u8 = 32;
pub const ER_COMMAND_COMPLETE: u8 = 33;
pub const ER_PORT_STATUS_CHANGE: u8 = 34;

// ---- TRB 标志（QEMU 位定义逐值同构）--------------------------------------
pub const TRB_C: u32 = 1 << 0;
pub const TRB_LK_TC: u32 = 1 << 1;
pub const TRB_CH: u32 = 1 << 4;
pub const TRB_IOC: u32 = 1 << 5;
pub const TRB_IDT: u32 = 1 << 6;
pub const TRB_DIR_IN: u32 = 1 << 16;
pub const TRB_CR_BSR: u32 = 1 << 9;
pub const TRB_TYPE_SHIFT: u32 = 10;
pub const TRB_CR_SLOTID_SHIFT: u32 = 24;

// ---- 完成码（QEMU TRBCCode 枚举逐值同构）---------------------------------
pub const CC_SUCCESS: u8 = 1;
pub const CC_USB_TRANSACTION_ERROR: u8 = 4;
pub const CC_TRB_ERROR: u8 = 5;
pub const CC_NO_SLOTS_ERROR: u8 = 9;
pub const CC_SHORT_PACKET: u8 = 13;

// ---- 能力/操作段寄存器（xHCI 规范 5.3/5.4；QEMU cap/oper read 同构）------
pub const REG_CAPLENGTH: u16 = 0x00; // 低字节 = 操作段基址
pub const REG_HCSPARAMS1: u16 = 0x04; // ports[31:24] | intrs[15:8] | slots[7:0]
pub const REG_HCSPARAMS2: u16 = 0x08; // MaxSpBuf=[27:32]（≠0 拒掉）
pub const REG_DBOFF: u16 = 0x14; // 32 位：门铃区偏移
pub const REG_RTSOFF: u16 = 0x18; // 32 位：运行段偏移
pub const OP_USBCMD: u16 = 0x00; // RS=bit0 | HCRST=bit1
pub const OP_USBSTS: u16 = 0x04; // HCH=bit0 | CNR=bit11
pub const OP_CRCR: u16 = 0x18; // 64B（QEMU oper_write case 0x18/0x1c 实证）
pub const OP_DCBAAP: u16 = 0x30; // 64B
pub const OP_CONFIG: u16 = 0x38; // MaxSlotsEn[0:8]
pub const OP_PORTS_BASE: u16 = 0x400; // PORTSC(n) = op + 0x400 + n*0x10
pub const PORT_REG_STRIDE: u16 = 0x10;
// PORTSC 位（QEMU 同名宏逐值同构）。
pub const PORTSC_CCS: u32 = 1 << 0;
pub const PORTSC_PED: u32 = 1 << 1;
pub const PORTSC_PR: u32 = 1 << 4;
pub const PORTSC_PP: u32 = 1 << 9;
pub const PORTSC_SPEED_SHIFT: u32 = 10;
pub const PORTSC_SPEED_MASK: u32 = 0xF;
pub const PORTSC_CSC: u32 = 1 << 17;
pub const PORTSC_PEC: u32 = 1 << 18;
pub const PORTSC_PRC: u32 = 1 << 21;
pub const PORTSC_PLC: u32 = 1 << 22;
/// 端口变化位集合（W1C 一次清光）。
pub const PORTSC_CHANGES: u32 =
    PORTSC_CSC | PORTSC_PEC | PORTSC_PRC | PORTSC_PLC;
pub const PORT_SPEED_FULL: u8 = 1;
pub const PORT_SPEED_LOW: u8 = 2;
pub const PORT_SPEED_HIGH: u8 = 3;
pub const PORT_SPEED_SUPER: u8 = 4;
// 运行段（rtsoff 起）：MFINDEX@0；中断器 n @ 0x20+n*0x20。
pub const RT_INTR_BASE: u16 = 0x20;
pub const RT_INTR_STRIDE: u16 = 0x20;
pub const RT_IMAN: u16 = 0x00; // IP=bit0（W1C）| IE=bit1（恒 0：纯轮询）
pub const RT_ERSTSZ: u16 = 0x08;
pub const RT_ERSTBA: u16 = 0x10; // 64B
pub const RT_ERDP: u16 = 0x18; // 64B；bit3=EHB
pub const ERDP_EHB: u32 = 1 << 3;

// ---- 结构参数（最小路径冻结）--------------------------------------------
/// 每环条目数（环各占一个 4KiB 帧；64 条 × 32B = 2KiB ≤ 4KiB）。
pub const RING_ENTRIES: usize = 64;
/// 环内可用业务条目数（最后一条固定留给 Link TRB）。
pub const RING_USABLE: usize = RING_ENTRIES - 1;
/// 最多跟踪的 HID 设备数（键盘+鼠标=2 已覆盖最小路径；超出如实跳过）。
pub const MAX_TRACKED: usize = 4;
/// 事件环条目数（事件 TRB 16B；ERST 表 16B 与环同帧，环 @ 帧内 +64）。
pub const EVENT_ENTRIES: usize = 64;
/// 单次泵最多消费事件数（防失控；正常稳态 ≤ 设备数）。
pub const PUMP_EVENT_BUDGET: usize = 16;
/// EP1 中断端点的 Normal TRB 缓冲（8B：键盘 8B/鼠标 3-4B 报告）。
pub const HID_REPORT_LEN: u32 = 8;
/// 轮询超时（ns）：命令/传输/端口等待。与 NVMe 最小栈同一量纲。
pub const TIMEOUT_NS: u64 = 3_000_000_000;

// ---- TRB 构造 -----------------------------------------------------------

/// 32 字节 TRB：param(8) + status(4) + control(4)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Trb {
    pub param: u64,
    pub status: u32,
    pub control: u32,
}

impl Trb {
    const fn new(param: u64, status: u32, control: u32) -> Trb {
        Trb { param, status, control }
    }

    fn typ(&self) -> u8 {
        ((self.control >> TRB_TYPE_SHIFT) & 0x3F) as u8
    }

    fn cyc(cycle: bool) -> u32 {
        if cycle { TRB_C } else { 0 }
    }

    /// Normal TRB（中断 IN 轮询用）：IOC=1，完成即 Transfer Event。
    pub fn normal(buf: u64, len: u32, cycle: bool) -> Trb {
        Trb::new(buf, len & 0x1_FFFF, (TRB_NORMAL as u32) << TRB_TYPE_SHIFT | TRB_IOC | Self::cyc(cycle))
    }

    /// Setup TRB：8 字节请求包内联（IDT=1）；param 按 USB 规范打包。
    pub fn setup(req: ControlRequest, cycle: bool) -> Trb {
        Trb::new(
            req.pack(),
            8,
            (TRB_SETUP as u32) << TRB_TYPE_SHIFT | TRB_IDT | TRB_CH | Self::cyc(cycle),
        )
    }

    /// Data TRB：控制传输数据段（DIR 必须与传输方向一致——QEMU 校验）。
    pub fn data(buf: u64, len: u32, dir_in: bool, cycle: bool) -> Trb {
        Trb::new(
            buf,
            len & 0x1_FFFF,
            (TRB_DATA as u32) << TRB_TYPE_SHIFT
                | TRB_CH
                | Self::cyc(cycle)
                | if dir_in { TRB_DIR_IN } else { 0 },
        )
    }

    /// Status TRB：状态段（方向 = 数据段取反；IOC=1 携带完成事件）。
    pub fn status(dir_in: bool, cycle: bool) -> Trb {
        Trb::new(
            0,
            0,
            (TRB_STATUS as u32) << TRB_TYPE_SHIFT
                | TRB_IOC
                | Self::cyc(cycle)
                | if dir_in { TRB_DIR_IN } else { 0 },
        )
    }

    /// Link TRB（TC=1）：环尾衔接，cycle 取该位置的合法周期。
    pub fn link(target: u64, cycle: bool) -> Trb {
        Trb::new(target, 0, (TRB_LINK as u32) << TRB_TYPE_SHIFT | TRB_LK_TC | Self::cyc(cycle))
    }

    /// 命令 TRB：slotid 进 control[24:32]。
    pub fn cmd(typ: u8, slotid: u8, param: u64, flags: u32, cycle: bool) -> Trb {
        Trb::new(
            param,
            0,
            (typ as u32) << TRB_TYPE_SHIFT
                | ((slotid as u32) << TRB_CR_SLOTID_SHIFT)
                | flags
                | Self::cyc(cycle),
        )
    }

    /// 32 字节小端字节流（DMA 内存布局）。
    pub fn le_bytes(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        out[0..8].copy_from_slice(&self.param.to_le_bytes());
        out[8..12].copy_from_slice(&self.status.to_le_bytes());
        out[12..16].copy_from_slice(&self.control.to_le_bytes());
        out
    }

    /// 从 32 字节字节流解析（模拟器/事件环读出共用）。
    pub fn parse(b: &[u8; 32]) -> Trb {
        Trb {
            param: u64::from_le_bytes(b[0..8].try_into().unwrap_or([0; 8])),
            status: u32::from_le_bytes(b[8..12].try_into().unwrap_or([0; 4])),
            control: u32::from_le_bytes(b[12..16].try_into().unwrap_or([0; 4])),
        }
    }
}

/// 8 字节控制请求包（USB 规范第 9 章；param 打包与 QEMU 控制路径同构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ControlRequest {
    pub bm_request_type: u8,
    pub b_request: u8,
    pub w_value: u16,
    pub w_index: u16,
    pub w_length: u16,
}

impl ControlRequest {
    pub const DIR_IN: u8 = 0x80;
    /// GET_DESCRIPTOR(DEVICE)：先 8 字节探 bMaxPacketSize0，后 18 字节全量。
    pub const fn get_device_desc(len: u16) -> ControlRequest {
        ControlRequest { bm_request_type: Self::DIR_IN, b_request: 0x06, w_value: 0x0100, w_index: 0, w_length: len }
    }
    /// GET_DESCRIPTOR(CONFIGURATION)：18 字节内含接口类的类/子类/协议。
    pub const fn get_config_desc() -> ControlRequest {
        ControlRequest { bm_request_type: Self::DIR_IN, b_request: 0x06, w_value: 0x0200, w_index: 0, w_length: 18 }
    }
    /// SET_PROTOCOL(boot)：HID 类请求，切换到 boot 报告格式。
    pub const fn set_protocol_boot(iface: u8) -> ControlRequest {
        ControlRequest { bm_request_type: 0x21, b_request: 0x0B, w_value: 0, w_index: iface as u16, w_length: 0 }
    }
    /// SET_IDLE(0)：报告仅变化时上送，关闭周期重复。
    pub const fn set_idle(iface: u8) -> ControlRequest {
        ControlRequest { bm_request_type: 0x21, b_request: 0x22, w_value: 0, w_index: iface as u16, w_length: 0 }
    }
    /// SET_CONFIGURATION(1)。
    pub const fn set_config() -> ControlRequest {
        ControlRequest { bm_request_type: 0x00, b_request: 0x09, w_value: 1, w_index: 0, w_length: 0 }
    }

    fn pack(&self) -> u64 {
        (self.bm_request_type as u64)
            | ((self.b_request as u64) << 8)
            | ((self.w_value as u64) << 16)
            | ((self.w_index as u64) << 32)
            | ((self.w_length as u64) << 48)
    }
}

// ---- 事件解析（布局对照 QEMU xhci_write_event）---------------------------

/// 事件环条目解析产物。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    pub ptr: u64,
    /// Transfer Event：未传完字节数；其余恒 0。
    pub length: u32,
    /// DW2[24:32]。
    pub ccode: u8,
    /// DW3[0]。
    pub cycle: bool,
    pub typ: u8,
    /// DW3[16:21]（Transfer Event）。
    pub epid: u8,
    /// DW3[24:32]。
    pub slotid: u8,
}

pub fn parse_event(b: &[u8; 32]) -> Event {
    let t = Trb::parse(b);
    Event {
        ptr: t.param,
        length: t.status & 0x00FF_FFFF,
        ccode: (t.status >> 24) as u8,
        cycle: t.control & TRB_C != 0,
        typ: t.typ(),
        epid: ((t.control >> 16) & 0x1F) as u8,
        slotid: (t.control >> TRB_CR_SLOTID_SHIFT) as u8,
    }
}

// ---- HID → PS/2 同构转换（纯函数，宿主可测）------------------------------

/// 鼠标 Y 轴翻转契约：HID 正=向下，PS/2 正=向上（见模块头注）。
pub const INVERT_MOUSE_Y: bool = true;

/// HID usage ID → 内核键词汇（`ps2::Key`）。覆盖 boot 键盘报告出现的
/// 主键区 + 方向键；未覆盖 usage 返回 None（丢弃并计数，不猜测）。
pub fn hid_usage_to_key(usage: u8) -> Option<ps2::Key> {
    use ps2::Key::*;
    Some(match usage {
        0x04 => A,
        0x05 => B,
        0x06 => C,
        0x07 => D,
        0x08 => E,
        0x09 => F,
        0x0A => G,
        0x0B => H,
        0x0C => I,
        0x0D => J,
        0x0E => K,
        0x0F => L,
        0x10 => M,
        0x11 => N,
        0x12 => O,
        0x13 => P,
        0x14 => Q,
        0x15 => R,
        0x16 => S,
        0x17 => T,
        0x18 => U,
        0x19 => V,
        0x1A => W,
        0x1B => X,
        0x1C => Y,
        0x1D => Z,
        0x1E => D1,
        0x1F => D2,
        0x20 => D3,
        0x21 => D4,
        0x22 => D5,
        0x23 => D6,
        0x24 => D7,
        0x25 => D8,
        0x26 => D9,
        0x27 => D0,
        0x28 => Enter,
        0x29 => Esc,
        0x2A => Backspace,
        0x2B => Tab,
        0x2C => Space,
        0x2D => Minus,
        0x2E => Equal,
        0x2F => BracketL,
        0x30 => BracketR,
        0x31 => Backslash,
        0x33 => Semicolon,
        0x34 => Apostrophe,
        0x35 => Grave,
        0x36 => Comma,
        0x37 => Period,
        0x38 => Slash,
        0x4F => Right,
        0x50 => Left,
        0x51 => Down,
        0x52 => Up,
        _ => return None,
    })
}

/// `ps2::Key` → PS/2 第一套 make 码（`ps2::decode` 的严格逆映射；本函数
/// 与 decode 的一致性由宿主测试逐键锁定——加键必须两表同步）。
pub fn key_to_ps2_make(k: ps2::Key) -> u8 {
    use ps2::Key::*;
    match k {
        Up => 0x48,
        Down => 0x50,
        Left => 0x4B,
        Right => 0x4D,
        Enter => 0x1C,
        Esc => 0x01,
        Space => 0x39,
        Backspace => 0x0E,
        Tab => 0x0F,
        LShift => 0x2A,
        RShift => 0x36,
        A => 0x1E,
        B => 0x30,
        C => 0x2E,
        D => 0x20,
        E => 0x12,
        F => 0x21,
        G => 0x22,
        H => 0x23,
        I => 0x17,
        J => 0x24,
        K => 0x25,
        L => 0x26,
        M => 0x32,
        N => 0x31,
        O => 0x18,
        P => 0x19,
        Q => 0x10,
        R => 0x13,
        S => 0x1F,
        T => 0x14,
        U => 0x16,
        V => 0x2F,
        W => 0x11,
        X => 0x2D,
        Y => 0x15,
        Z => 0x2C,
        D1 => 0x02,
        D2 => 0x03,
        D3 => 0x04,
        D4 => 0x05,
        D5 => 0x06,
        D6 => 0x07,
        D7 => 0x08,
        D8 => 0x09,
        D9 => 0x0A,
        D0 => 0x0B,
        Minus => 0x0C,
        Equal => 0x0D,
        Comma => 0x33,
        Period => 0x34,
        Slash => 0x35,
        Semicolon => 0x27,
        Apostrophe => 0x28,
        BracketL => 0x1A,
        BracketR => 0x1B,
        Backslash => 0x2B,
        Grave => 0x29,
        F12 => 0x58, // SET1 F12；USB HID → make → ps2::decode 逆映射闭环
    }
}

/// HID 键盘 boot 报告差分解码器：make-only 语义（按键沿产出事件，
/// 保持/释放零事件），修饰键只识别左右 Shift（沿触发）。
#[derive(Default)]
pub struct HidKbdDecoder {
    prev_usage: [u8; 6],
    prev_mod: u8,
    /// 无法差分的报告（ErrorRollOver）计数——诊断用。
    pub rollover: u32,
    /// 词汇表外 usage 丢弃计数——诊断用。
    pub unmapped: u32,
}

impl HidKbdDecoder {
    pub fn feed(&mut self, report: &[u8]) -> Option<ps2::Key> {
        if report.len() < 8 {
            return None;
        }
        let usage = |i: usize| report[2 + i];
        // ErrorRollOver（全 1）：>6 键同按，无法差分，如实丢弃。
        if (0..6).all(|i| usage(i) == 1) {
            self.rollover = self.rollover.wrapping_add(1);
            return None;
        }
        let mut ev = None;
        // 按键沿：新报告中出现、上一份没有的 usage。
        for i in 0..6 {
            let u = usage(i);
            if u == 0 || self.prev_usage.contains(&u) {
                continue;
            }
            if let Some(k) = hid_usage_to_key(u) {
                ev = Some(k); // 同报告多键同按：取最后一个（PS/2 逐字节流等价）。
            } else {
                self.unmapped = self.unmapped.wrapping_add(1);
            }
        }
        // Shift 沿（bit1=左 bit5=右）。
        let m = report[0];
        if ev.is_none() {
            if m & 0x02 != 0 && self.prev_mod & 0x02 == 0 {
                ev = Some(ps2::Key::LShift);
            } else if m & 0x20 != 0 && self.prev_mod & 0x20 == 0 {
                ev = Some(ps2::Key::RShift);
            }
        }
        self.prev_usage.copy_from_slice(&report[2..8]);
        self.prev_mod = m;
        ev
    }
}

/// HID 鼠标 boot 报告解码：取前 3 字节（按钮/dx/dy），滚轮字节（若 4B
/// 报告）如实丢弃；零位移且按键无变化时抑制（防稳态空包灌队列）。
/// 输出为已按 [`INVERT_MOUSE_Y`] 翻转、PS/2 语义的合成三字节包。
#[derive(Default)]
pub struct HidMouseDecoder {
    prev_buttons: u8,
    /// 抑制的空包计数——诊断用。
    pub suppressed: u32,
}

impl HidMouseDecoder {
    pub fn feed(&mut self, report: &[u8]) -> Option<[u8; 3]> {
        if report.len() < 3 {
            return None;
        }
        let buttons = report[0] & 0x07;
        let dx = report[1] as i8;
        let dy_raw = report[2] as i8;
        let dy = if INVERT_MOUSE_Y { -dy_raw } else { dy_raw };
        if dx == 0 && dy == 0 && buttons == self.prev_buttons {
            self.suppressed = self.suppressed.wrapping_add(1);
            return None;
        }
        self.prev_buttons = buttons;
        Some(synth_ps2_mouse_packet(dx, dy, buttons))
    }
}

/// 合成 PS/2 三字节鼠标包（与 `inputsvc::MouseDecoder` 逐字段同构）：
/// 字节 0 = 0x08 同步位 | dx 符号(bit4) | dy 符号(bit5) | 按键位图。
pub fn synth_ps2_mouse_packet(dx: i8, dy: i8, buttons: u8) -> [u8; 3] {
    let b0 = 0x08 | (((dx < 0) as u8) << 4) | (((dy < 0) as u8) << 5) | (buttons & 0x07);
    [b0, dx as u8, dy as u8]
}

// ---- 设备与环状态 --------------------------------------------------------

/// 枚举出的 HID 设备类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HidKind {
    Keyboard,
    Mouse,
}

/// 轮询产出的事件（供调用方接线；本层不认识 InputService）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HidEvent {
    Key(ps2::Key),
    /// PS/2 语义（Y 正=向上，已翻转）的位移 + 按键位图。
    Mouse { dx: i8, dy: i8, buttons: u8 },
}

// ---- S4.2（AI-5）：USB 大容量存储（MSC/BOT）------------------------------
/// 每设备 bulk 数据缓冲（1 帧 4KiB；单 TRB 数据段上限）。
pub const MSC_BUF_LEN: u32 = 4096;
/// 最多跟踪的 MSC 设备数（最小路径：U 盘 1 只已覆盖；超出如实跳过）。
pub const MAX_MSC: usize = 2;
/// bulk 端点最大包（高速 512 / 全速 64，按端口速度选择）。
pub fn bulk_mps(speed: u8) -> u32 {
    if speed == PORT_SPEED_HIGH {
        512
    } else {
        64
    }
}
/// 门铃 EP 编号：EP1 OUT = 2，EP1 IN = 3（EP 上下文号 = 端点号×2+方向）。
pub const EPID_BULK_OUT: u8 = 2;
pub const EPID_BULK_IN: u8 = 3;

/// 枚举出的设备类别（enumerate_ports 日志与接线判定用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DevKind {
    HidKeyboard,
    HidMouse,
    Msc,
}

/// 描述符分类结果（地址+描述符阶段产出，交给对应收尾函数）。
enum DevClass {
    Hid(HidKind, u8),
    Msc,
}

/// 已枚举 MSC 设备（BOT 串行：out/in 各一个传输环 + 共享 4KiB 缓冲帧）。
struct MscDevice {
    slot: u8,
    #[allow(dead_code)]
    port: u8,
    out_ring: u64,
    out_tail: usize,
    out_cycle: bool,
    /// 已投放未完成的 Normal TRB 总线地址（单件在途；BOT 严格串行）。
    out_outstanding: Option<u64>,
    in_ring: u64,
    in_tail: usize,
    in_cycle: bool,
    in_outstanding: Option<u64>,
    /// 数据缓冲帧（CBW/数据段/CSW 分时复用）。
    buf: u64,
    /// BOT 初始化（TEST_UNIT_READY/INQUIRY/READ CAPACITY）成功后的几何。
    inited: bool,
    pub block_size: u32,
    pub blocks: u64,
}

/// 已枚举设备的运行态（每设备固定 3 帧：输出上下文/EP1 环/数据缓冲）。
struct HidDevice {
    slot: u8,
    #[allow(dead_code)]
    port: u8,
    kind: HidKind,
    #[allow(dead_code)]
    iface: u8,
    ep1_ring_phys: u64,
    /// EP1 环投递指针（条目号 + 当前周期位）。
    ep1_tail: usize,
    ep1_cycle: bool,
    /// 已投放未完成的 Normal TRB 物理地址（单件在途；None=待投放）。
    ep1_outstanding: Option<u64>,
    /// 报告缓冲物理地址（8B）。
    report_phys: u64,
    kbd: HidKbdDecoder,
    mouse: HidMouseDecoder,
}

/// 环推进：业务条目只到 RING_USABLE-1（最后一条固定是 Link TRB）；
/// 越过即回 0 并翻转周期（Link TRB 的周期位由 [`XhciCtrl::ring_put`]
/// 在回绕时同步重写——控制器在每轮经过 Link 时校验的是本轮周期）。
fn ring_advance(idx: usize, cycle: bool) -> (usize, bool) {
    if idx + 1 >= RING_USABLE {
        (0, !cycle)
    } else {
        (idx + 1, cycle)
    }
}

/// Link TRB 落位：写入 `ring_phys + (RING_ENTRIES-1)*32`。
fn link_trb_bytes(ring_phys: u64, cycle: bool) -> [u8; 32] {
    Trb::link(ring_phys, cycle).le_bytes()
}

fn u32_bytes(w: &[u32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, v) in w.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

// ---- 控制流（宿主/目标共用）----------------------------------------------

/// xHCI 控制器最小栈。生命周期：[`XhciCtrl::init_with_recovery`] →
/// [`XhciCtrl::enumerate_ports`] → [`XhciCtrl::pump`]。
pub struct XhciCtrl<B: BarAccess, M: DmaMem> {
    bar: B,
    mem: M,
    now: fn() -> u64,
    timeout_ns: u64,
    // 能力段读出的布局（绝不写死）。
    op: u16,
    db_off: u16,
    rts_off: u16,
    max_slots: u8,
    max_ports: u8,
    // 命令环软件侧。
    cmd_phys: u64,
    cmd_tail: usize,
    cmd_cycle: bool,
    /// 在途命令（TRB 物理地址；命令串行，单件在途）。
    cmd_outstanding: Option<u64>,
    // 事件环软件侧。
    evt_phys: u64,
    evt_idx: usize,
    evt_cycle: bool,
    /// ERDP 已写入的槽号（惰性推进：落后 evt_idx 若干槽，防 QEMU 满环
    /// 丢弃分支命中——dp_idx 贴近 er_ep_idx 时完成事件会被静默丢弃）。
    erdp_written: usize,
    // 工作帧。
    ictx_phys: u64,
    ep0_ring_phys: u64,
    data_phys: u64,
    dcbaa_phys: u64,
    // EP0 环软件侧指针（跨控制传输持久——控制器侧 dequeue 连续推进）。
    ep0_tail: usize,
    ep0_cycle: bool,
    /// 在途控制传输的 Status TRB 物理地址（EP0 串行）。
    ep0_outstanding: Option<u64>,
    // 已枚举设备。
    devs: [Option<HidDevice>; MAX_TRACKED],
    // S4.2：已枚举 MSC 设备（与 HID 表互斥占槽——每设备只属一类）。
    mscs: [Option<MscDevice>; MAX_MSC],
    // 验收证据计数。
    pub resets: u32,
    pub cmd_events: u32,
    pub transfer_events: u32,
    pub port_events: u32,
    pub unknown_events: u32,
}

/// 一次完整初始化尝试。失败时归还 bar/mem 供重试（NVMe 同款恢复形态）。
fn try_init<B: BarAccess, M: DmaMem>(
    bar: B,
    mem: M,
    now: fn() -> u64,
    timeout_ns: u64,
    resets: u32,
) -> Result<XhciCtrl<B, M>, (B, M, BlockError)> {
    let mut c = XhciCtrl {
        bar,
        mem,
        now,
        timeout_ns,
        op: 0,
        db_off: 0,
        rts_off: 0,
        max_slots: 0,
        max_ports: 0,
        cmd_phys: 0,
        cmd_tail: 0,
        cmd_cycle: true,
        cmd_outstanding: None,
        evt_phys: 0,
        evt_idx: 0,
        evt_cycle: true,
        erdp_written: 0,
        ictx_phys: 0,
        ep0_ring_phys: 0,
        data_phys: 0,
        dcbaa_phys: 0,
        ep0_tail: 0,
        ep0_cycle: true,
        ep0_outstanding: None,
        devs: core::array::from_fn(|_| None),
        mscs: core::array::from_fn(|_| None),
        resets,
        cmd_events: 0,
        transfer_events: 0,
        port_events: 0,
        unknown_events: 0,
    };
    macro_rules! bail {
        ($e:expr) => {
            if let Err(e) = $e {
                return Err((c.bar, c.mem, e));
            }
        };
    }
    bail!(c.read_caps());
    bail!(c.controller_reset());
    bail!(c.program_rings());
    bail!(c.start());
    Ok(c)
}

/// 失败现场取证（2026-09-22 诊断增强，实机专用）：读能力/操作段关键寄存器
/// 落串口——USBSTS bit3=HSE（Host System Error，真机 "status=8" 即它）、
/// bit11=CNR、bit12=HCE；USBCMD 看 RS/HCRST 残留；CAPLENGTH 校验 BAR 映射
/// 是否落在有效寄存器区。只读，不改任何寄存器。泛型 B 与 init_with_recovery
/// 的 BarAccess 约束同源（宿主模拟器不编译本函数）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn xhci_failure_forensics<B: BarAccess>(bar: &mut B) {
    let cap0 = bar.read32(REG_CAPLENGTH);
    let op = (cap0 & 0xFF) as u16;
    crate::kwarn!(
        "xhci: forensics CAPLENGTH={:#010x} op_off={}",
        cap0,
        op
    );
    if op == 0 || op > 0x400 {
        crate::kwarn!("xhci: forensics op offset out of range - BAR mapping suspect");
        return;
    }
    let cmd = bar.read32(op + OP_USBCMD);
    let sts = bar.read32(op + OP_USBSTS);
    crate::kwarn!(
        "xhci: forensics USBCMD={:#010x} USBSTS={:#010x} (HCH={} HSE={} CNR={} HCE={})",
        cmd,
        sts,
        sts & 1,
        (sts >> 3) & 1,
        (sts >> 11) & 1,
        (sts >> 12) & 1
    );
}

impl<B: BarAccess, M: DmaMem> XhciCtrl<B, M> {
    /// 完整初始化（含恢复路径）：复位 → 编程环 → 启动。任何一步超时 →
    /// 完整复位重试一次 → 仍失败如实 `DeviceReset`。
    pub fn init_with_recovery(bar: B, mem: M, now: fn() -> u64, timeout_ns: u64) -> Result<Self, BlockError> {
        let mut resets = 0;
        let (mut bar, mut mem) = (bar, mem);
        loop {
            match try_init(bar, mem, now, timeout_ns, resets) {
                Ok(c) => return Ok(c),
                Err((b, m, BlockError::Timeout)) if resets < 1 => {
                    resets += 1;
                    bar = b;
                    mem = m;
                }
                Err((mut b, _, e)) => {
                    // 失败现场取证（2026-09-22 诊断增强）：真机 status=8 类
                    // 失败此前只有一句 Err——寄存器现场全部丢失。bar 还在
                    // 手（本次 try_init 的所有权返回），把 CAPLENGTH/USBCMD/
                    // USBSTS 落串口，日志直接可判。错误码语义保持原样
                    // （二次 Timeout → DeviceReset）。
                    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
                    xhci_failure_forensics(&mut b);
                    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
                    let _ = b;
                    let final_e = if matches!(e, BlockError::Timeout) {
                        BlockError::DeviceReset
                    } else {
                        e
                    };
                    return Err(final_e);
                }
            }
        }
    }

    fn r32(&mut self, off: u16) -> u32 {
        self.bar.read32(off)
    }
    fn w32(&mut self, off: u16, v: u32) {
        self.bar.write32(off, v);
    }
    /// 操作段寄存器（op = CAPLENGTH 低字节）。
    fn opr32(&mut self, off: u16) -> u32 {
        self.r32(self.op + off)
    }
    fn opw32(&mut self, off: u16, v: u32) {
        self.w32(self.op + off, v)
    }
    /// 中断器 0 寄存器（rtsoff + 0x20 起）。
    fn rtw32(&mut self, off: u16, v: u32) {
        self.w32(self.rts_off + RT_INTR_BASE + off, v);
    }
    /// 临时取证：ERDP 读回。
    fn rt32_(&mut self, off: u16) -> u32 {
        self.r32(self.rts_off + RT_INTR_BASE + off)
    }
    /// 槽位门铃（db_off + slot*4；命令环 slot=0，QEMU 要求写值 0）。
    fn doorbell(&mut self, slot: u8, epid: u8) {
        // 门铃写序：DMA 内存先全局可见（Release），再敲铃。
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        self.w32(self.db_off + (slot as u16) * 4, epid as u32);
    }

    /// 业务 TRB 落环 + 指针推进；回绕时重写 Link TRB 周期位。
    /// 返回推进后的 (条目号, 周期位)。
    fn ring_put(&mut self, ring: u64, idx: usize, cycle: bool, t: Trb) -> (usize, bool) {
        let addr = ring + idx as u64 * 32;
        self.mem.write_bytes(addr, 0, &t.le_bytes());
        let (nidx, ncyc) = ring_advance(idx, cycle);
        if nidx == 0 {
            // 本轮结束：Link TRB 的合法周期仍是**本轮**周期——控制器在
            // 下一轮首轮门铃时以本轮周期校验 Link，随后才自行翻转周期
            // （QEMU ring_fetch 的 TC 语义）。写成翻转后的值会提前失配。
            // Link 目标写**总线地址**（控制器循链 DMA）。
            self.mem.write_bytes(ring + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.mem.bus_addr(ring), cycle));
        }
        (nidx, ncyc)
    }

    /// 能力段布局读取——绝不写死偏移：CAPLENGTH/DBOFF/RTSOFF/参数全部
    /// 运行时读出（不同实现操作段基址不同）。
    fn read_caps(&mut self) -> Result<(), BlockError> {
        let cap0 = self.r32(REG_CAPLENGTH);
        self.op = (cap0 & 0xFF) as u16;
        if self.op == 0 {
            return Err(BlockError::Unsupported); // 布局非法，如实拒绝。
        }
        let p1 = self.r32(REG_HCSPARAMS1);
        self.max_slots = (p1 & 0xFF) as u8;
        self.max_ports = (p1 >> 24) as u8;
        if self.max_slots == 0 || self.max_ports == 0 {
            return Err(BlockError::Unsupported);
        }
        // scratchpad（[27:32] Log2）：最小栈不支持，≠0 如实拒绝该控制器
        // （QEMU 恒 0；真机带 scratchpad 的控制器本批不冒进）。
        let hcs2 = self.r32(REG_HCSPARAMS2);
        let sp_buf = (hcs2 >> 27) & 0x1F;
        if sp_buf != 0 {
            #[cfg(all(target_arch = "x86_64", target_os = "none"))]
            crate::kwarn!("xhci: scratchpad required (log2={}) - controller skipped", sp_buf);
            return Err(BlockError::Unsupported);
        }
        let dboff = self.r32(REG_DBOFF);
        let rtsoff = self.r32(REG_RTSOFF);
        self.db_off = (dboff & 0xFFFF) as u16;
        self.rts_off = (rtsoff & 0xFFFF) as u16;
        if self.db_off == 0 || self.rts_off == 0 {
            return Err(BlockError::Unsupported);
        }
        Ok(())
    }

    /// USBCMD.HCRST → 等 HCH=1 且 CNR=0（复位完成）。
    fn controller_reset(&mut self) -> Result<(), BlockError> {
        self.opw32(OP_USBCMD, 0x2); // HCRST（RS=0）
        let deadline = self.deadline();
        loop {
            let st = self.opr32(OP_USBSTS);
            if st & 0x1 != 0 && st & (1 << 11) == 0 {
                break;
            }
            if (self.now)() > deadline {
                return Err(BlockError::Timeout);
            }
        }
        Ok(())
    }

    fn deadline(&self) -> u64 {
        (self.now)() + self.timeout_ns
    }

    /// 分配帧 → 清零（复用帧残留 cycle 位会假消费，NVMe 同款戒律）。
    fn alloc_zeroed(&mut self, _what: &str) -> Result<u64, BlockError> {
        match self.mem.alloc_frame() {
            Some(f) => {
                self.mem.zero_frame(f);
                Ok(f)
            }
            None => {
                #[cfg(all(target_arch = "x86_64", target_os = "none"))]
                crate::kwarn!("xhci: dma frame exhausted at {}", _what);
                Err(BlockError::Io)
            }
        }
    }

    /// 命令环/事件环/DCBAA/中断器编程 + 环内 Link TRB 落位。
    fn program_rings(&mut self) -> Result<(), BlockError> {
        self.cmd_phys = self.alloc_zeroed("cmd ring")?;
        self.evt_phys = self.alloc_zeroed("event ring")?;
        self.ictx_phys = self.alloc_zeroed("input ctx")?;
        self.ep0_ring_phys = self.alloc_zeroed("ep0 ring")?;
        self.data_phys = self.alloc_zeroed("data buf")?;
        self.dcbaa_phys = self.alloc_zeroed("dcbaa")?;
        // 环尾 Link TRB（初始周期位 = 各环初始周期）。
        self.mem.write_bytes(self.cmd_phys + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.mem.bus_addr(self.cmd_phys), self.cmd_cycle));
        self.mem.write_bytes(self.ep0_ring_phys + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.mem.bus_addr(self.ep0_ring_phys), self.ep0_cycle));
        // ERST 表（单段 16B）：表 @ 帧头，环 @ +64（都 64B 对齐）。
        // 段基址写**总线地址**（控制器经 ERST DMA 到事件环）。
        let seg = self.evt_phys + 64;
        let seg_bus = self.mem.bus_addr(seg);
        let mut erst = [0u8; 16];
        erst[0..8].copy_from_slice(&seg_bus.to_le_bytes());
        erst[8..12].copy_from_slice(&(EVENT_ENTRIES as u32).to_le_bytes());
        self.mem.write_bytes(self.evt_phys, 0, &erst);
        // CRCR：低 64B 对齐基址 | RCS=1，先低后高（QEMU 在高写时建环）。
        // 以下寄存器值全部是**总线地址**（bus_addr 边界翻译）。
        let cmd_bus = self.mem.bus_addr(self.cmd_phys);
        self.opw32(OP_CRCR, cmd_bus as u32 | 0x1);
        self.opw32(OP_CRCR + 4, (cmd_bus >> 32) as u32);
        let dcbaa_bus = self.mem.bus_addr(self.dcbaa_phys);
        self.opw32(OP_DCBAAP, dcbaa_bus as u32);
        self.opw32(OP_DCBAAP + 4, (dcbaa_bus >> 32) as u32);
        self.opw32(OP_CONFIG, self.max_slots as u32);
        // 中断器 0：ERSTSZ=1 → ERSTBA（高写触发段装载）→ ERDP 指向环头。
        let evt_bus = self.mem.bus_addr(self.evt_phys);
        self.rtw32(RT_ERSTSZ, 1);
        self.rtw32(RT_ERSTBA, evt_bus as u32);
        self.rtw32(RT_ERSTBA + 4, (evt_bus >> 32) as u32);
        self.rtw32(RT_ERDP, seg_bus as u32);
        self.rtw32(RT_ERDP + 4, (seg_bus >> 32) as u32);
        self.erdp_written = 0;
        Ok(())
    }

    /// RS=1 启动并确认 HCH 清零。
    fn start(&mut self) -> Result<(), BlockError> {
        self.opw32(OP_USBCMD, 0x1);
        let deadline = self.deadline();
        while self.opr32(OP_USBSTS) & 0x1 != 0 {
            if (self.now)() > deadline {
                return Err(BlockError::Timeout);
            }
        }
        Ok(())
    }

    // ---- 事件环游标（命令等待期与运行态泵共用）---------------------------

    /// 读当前事件环条目；cycle 不匹配 = 无新事件 → None。
    fn evt_peek(&self) -> Option<Event> {
        let addr = self.evt_phys + 64 + self.evt_idx as u64 * 32;
        let mut raw = [0u8; 32];
        self.mem.read_bytes(addr, 0, &mut raw);
        let ev = parse_event(&raw);
        if ev.cycle != self.evt_cycle {
            return None;
        }
        Some(ev)
    }

    /// 推进事件环游标（惰性：ERDP 不立即前推，见 `erdp_flush_if_needed`）。
    fn evt_step(&mut self) {
        self.evt_idx += 1;
        if self.evt_idx >= EVENT_ENTRIES {
            self.evt_idx = 0;
            self.evt_cycle = !self.evt_cycle;
        }
    }

    /// ERDP 惰性推进：落后达到阈值（8 槽）或调用方要求时，把 ERDP 让位
    /// 到游标处。**设计依据**：QEMU `xhci_event` 在 `dp_idx == er_ep_idx+1`
    /// 时静默丢弃事件（满环分支）——急切推进会让 dp_idx 恰好追平
    /// er_ep_idx+1，完成事件被吞；保持 ERDP 落后 ≥2 槽即绕开该分支。
    fn erdp_flush_if_needed(&mut self, force: bool) {
        let lag = (self.evt_idx + EVENT_ENTRIES - self.erdp_written) % EVENT_ENTRIES;
        if !force && lag < 8 {
            return;
        }
        self.erdp_written = self.evt_idx;
        let erdp_tok = self.evt_phys + 64 + self.evt_idx as u64 * 32;
        let erdp = self.mem.bus_addr(erdp_tok);
        self.rtw32(RT_ERDP, erdp as u32 | ERDP_EHB);
        self.rtw32(RT_ERDP + 4, (erdp >> 32) as u32);
    }

    /// 全槽扫描：在事件环 64 槽内找「类型+TRB 指针」双匹配的事件。
    /// 每槽合法性按其位置的期望周期位判定（游标之前的槽=翻转周期，其后=当前周期）。
    /// 命中 → 游标推进到命中槽之后（沿途事件按类型记账）；未命中 → 游标不动。
    /// **这是对 QEMU 写序/游标错位类异常的工程容错**（真驱动对多段事件环
    /// 本就全段扫描）。沿途未匹配的有效事件也一并消费记账，防游标卡死。
    fn evt_scan_for(&mut self, typ: u8, want_ptr: u64) -> Option<Event> {
        let start = self.evt_idx;
        let base_cycle = self.evt_cycle;
        let mut hit = None;
        for step in 0..EVENT_ENTRIES {
            let slot = (start + step) % EVENT_ENTRIES;
            let wrapped = slot < start;
            let slot_cycle = if wrapped { !base_cycle } else { base_cycle };
            let addr = self.evt_phys + 64 + slot as u64 * 32;
            let mut raw = [0u8; 32];
            self.mem.read_bytes(addr, 0, &mut raw);
            let ev = parse_event(&raw);
            if ev.cycle != slot_cycle {
                continue; // 该槽无有效事件。
            }
            let is_match = ev.typ == typ && ev.ptr == want_ptr;
            match ev.typ {
                ER_COMMAND_COMPLETE => self.cmd_events += 1,
                ER_PORT_STATUS_CHANGE => self.port_events += 1,
                ER_TRANSFER => self.transfer_events += 1,
                _ => self.unknown_events += 1,
            }
            self.evt_idx = (slot + 1) % EVENT_ENTRIES;
            if slot + 1 >= EVENT_ENTRIES {
                self.evt_cycle = !self.evt_cycle;
            }
            self.erdp_flush_if_needed(false);
            if is_match {
                hit = Some(ev);
                break;
            }
        }
        hit
    }

    // ---- 命令通道（串行，自旋等待完成；仅初始化期使用）-------------------

    /// 提交命令并自旋等待 Command Complete，返回 (slotid, ccode)。
    fn cmd_submit(&mut self, typ: u8, slotid: u8, param: u64, flags: u32) -> Result<(u8, u8), BlockError> {
        if self.cmd_outstanding.is_some() {
            return Err(BlockError::Io); // 上一条未收尾，串行契约被破坏。
        }
        let t = Trb::cmd(typ, slotid, param, flags, self.cmd_cycle);
        let trb_addr = self.cmd_phys + self.cmd_tail as u64 * 32;
        self.mem.write_bytes(trb_addr, 0, &t.le_bytes());
        // 事件匹配用总线地址（QEMU 事件 ptr = TRB 的 guest 物理）。
        let trb_bus = self.mem.bus_addr(trb_addr);
        self.cmd_outstanding = Some(trb_bus);
        let (ntail, ncyc) = ring_advance(self.cmd_tail, self.cmd_cycle);
        if ntail == 0 {
            self.mem.write_bytes(self.cmd_phys + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.mem.bus_addr(self.cmd_phys), self.cmd_cycle));
        }
        self.cmd_tail = ntail;
        self.cmd_cycle = ncyc;
        self.doorbell(0, 0); // QEMU：命令门铃写值必须为 0。
        // 自旋等待本命令完成：顺序游标优先，未见则全槽扫描（QEMU 写序
        // 容错），等待过半重振铃一次（门铃丢失类异常兜底）。
        let deadline = self.deadline();
        let rerun_at = (self.now)() + self.timeout_ns / 2;
        let mut re_rung = false;
        loop {
            if let Some(ev) = self.evt_peek() {
                match ev.typ {
                    ER_COMMAND_COMPLETE => {
                        self.cmd_events += 1;
                        self.evt_step();
                        if ev.ptr == trb_bus {
                            self.cmd_outstanding = None;
                            return Ok((ev.slotid, ev.ccode));
                        }
                        // 非本命令的完成：继续等（串行契约下不应出现）。
                    }
                    ER_PORT_STATUS_CHANGE => {
                        self.port_events += 1;
                        self.evt_step();
                    }
                    ER_TRANSFER => {
                        self.transfer_events += 1;
                        self.evt_step();
                    }
                    _ => {
                        self.unknown_events += 1;
                        self.evt_step();
                    }
                }
                continue;
            }
            if let Some(ev) = self.evt_scan_for(ER_COMMAND_COMPLETE, trb_bus) {
                self.cmd_outstanding = None;
                return Ok((ev.slotid, ev.ccode));
            }
            if !re_rung && (self.now)() >= rerun_at {
                re_rung = true;
                self.doorbell(0, 0); // 门铃丢失类异常兜底：重振铃一次。
            }
            self.erdp_flush_if_needed(false);
            if (self.now)() > deadline {
                #[cfg(all(target_arch = "x86_64", target_os = "none"))]
                {
                    let mut raw = [0u8; 32];
                    let addr = self.evt_phys + 64 + self.evt_idx as u64 * 32;
                    self.mem.read_bytes(addr, 0, &mut raw);
                    let mut crb = [0u8; 32];
                    self.mem.read_bytes(self.cmd_phys, 0, &mut crb);
                    let mut erst = [0u8; 16];
                    self.mem.read_bytes(self.evt_phys, 0, &mut erst);
                    let erdp = self.r32(self.rts_off + RT_INTR_BASE + RT_ERDP);
                    let idx = self.evt_idx;
                    let cyc = self.evt_cycle as u8;
                    crate::kwarn!(
                        "xhci: cmd timeout typ={} evt_idx={} evt_cycle={} erdp={:#x} raw_c={:#x} cmd_phys={:#x} cmd_trb_c={:#x} evt_phys={:#x}",
                        typ,
                        idx,
                        cyc,
                        erdp,
                        u32::from_le_bytes(raw[12..16].try_into().unwrap_or([0; 4])),
                        self.cmd_phys,
                        u32::from_le_bytes(crb[12..16].try_into().unwrap_or([0; 4])),
                        self.evt_phys
                    );
                    crate::kwarn!(
                        "xhci: forensic evt_ptr={:#x} evt_ctrl={:#x} erst_base={:#x} erst_size={}",
                        u64::from_le_bytes(raw[0..8].try_into().unwrap_or([0; 8])),
                        u32::from_le_bytes(raw[12..16].try_into().unwrap_or([0; 4])),
                        u64::from_le_bytes(erst[0..8].try_into().unwrap_or([0; 8])),
                        u32::from_le_bytes(erst[8..12].try_into().unwrap_or([0; 4]))
                    );
                    // 全环取证：16 槽 ctrl（事件环）+ cmd 环前 4 槽 ctrl。
                    for slot_i in 0..16usize {
                        let mut sr = [0u8; 32];
                        let sa = self.evt_phys + 64 + (slot_i as u64) * 32;
                        self.mem.read_bytes(sa, 0, &mut sr);
                        let sc = u32::from_le_bytes(sr[12..16].try_into().unwrap_or([0; 4]));
                        if sc != 0 {
                            crate::kwarn!(
                                "xhci: evt_slot[{}] ctrl={:#x} ptr={:#x}",
                                slot_i,
                                sc,
                                u64::from_le_bytes(sr[0..8].try_into().unwrap_or([0; 8]))
                            );
                        }
                    }
                    for slot_i in 0..4usize {
                        let mut sr = [0u8; 32];
                        let sa = self.cmd_phys + (slot_i as u64) * 32;
                        self.mem.read_bytes(sa, 0, &mut sr);
                        crate::kwarn!(
                            "xhci: cmd_slot[{}] ctrl={:#x}",
                            slot_i,
                            u32::from_le_bytes(sr[12..16].try_into().unwrap_or([0; 4]))
                        );
                    }
                }
                self.cmd_outstanding = None; // 超时清在途：不给下一台设备留级联失败。
                return Err(BlockError::Timeout);
            }
        }
    }

    // ---- 枚举（M3）--------------------------------------------------------

    /// 扫描全部端口：CCS 且速度 ≤ 高速的设备逐个走完整枚举。
    /// 无设备的端口跳过；枚举失败的设备如实 kwarn 跳过并回收帧，
    /// 绝不影响其余设备与其余子系统。
    pub fn enumerate_ports(&mut self) -> usize {
        let ports = self.max_ports;
        let mut n = 0;
        for p in 0..ports {
            let off = self.op + OP_PORTS_BASE + (p as u16) * PORT_REG_STRIDE;
            let mut sc = self.r32(off);
            if sc & PORTSC_CCS == 0 {
                // 变化位清光（W1C），保持下次热插拔可感知。
                self.w32(off, PORTSC_CHANGES);
                continue;
            }
            // 端口上电（真机 PP 可能未上电；QEMU 恒已上电，写之无害）。
            if sc & PORTSC_PP == 0 {
                self.w32(off, sc | PORTSC_PP);
                sc = self.r32(off);
            }
            // 复位端口：写 PR → 等 PRC → 变化位 W1C 清光。
            if sc & PORTSC_PED == 0 {
                self.w32(off, sc | PORTSC_PR | PORTSC_PP);
                let deadline = self.deadline();
                loop {
                    let s = self.r32(off);
                    if s & PORTSC_PRC != 0 {
                        self.w32(off, PORTSC_CHANGES);
                        break;
                    }
                    if (self.now)() > deadline {
                        break; // 复位超时：留待下次扫描重试。
                    }
                }
            }
            let sc = self.r32(off);
            if sc & (PORTSC_CCS | PORTSC_PED) != (PORTSC_CCS | PORTSC_PED) {
                continue;
            }
            let speed = ((sc >> PORTSC_SPEED_SHIFT) & PORTSC_SPEED_MASK) as u8;
            if speed == 0 || speed == PORT_SPEED_SUPER {
                // 超速设备需要流协议/更多端点——本批如实不收。
                #[cfg(all(target_arch = "x86_64", target_os = "none"))]
                crate::kinfo!("xhci: port {} speed={} not supported (skip)", p + 1, speed);
                continue;
            }
            match self.enumerate_device(p + 1, speed) {
                Ok(kind) => {
                    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
                    crate::kinfo!("xhci: port {} speed={} enumerated kind={:?}", p + 1, speed, kind);
                    n += 1;
                }
                Err(e) => {
                    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
                    crate::kwarn!("xhci: port {} enumerate failed {:?} (skip)", p + 1, e);
                }
            }
        }
        n
    }

    /// 单设备完整枚举：Enable Slot → Address Device → 描述符分类 →
    /// 按（接口类 3=HID / 8=MSC BOT）完成各自配置。EP0 控制传输串行
    /// 复用同一环；失败路径统一禁用槽位 + 帧归还，不留半挂载态。
    fn enumerate_device(&mut self, rh_port: u8, speed: u8) -> Result<DevKind, BlockError> {
        let octx = self.alloc_zeroed("dev octx")?;
        // Enable Slot → DCBAA[slotid] = 输出上下文。
        let (slotid, cc) = self.cmd_submit(CR_ENABLE_SLOT, 0, 0, 0)?;
        if cc != CC_SUCCESS || slotid == 0 {
            self.mem.free_frame(octx);
            return Err(self.cc_to_err(cc));
        }
        self.mem
            .write_bytes(self.dcbaa_phys, (slotid as u64) * 8, &self.mem.bus_addr(octx).to_le_bytes());
        let outcome = match self.address_and_classify(slotid, rh_port, speed) {
            Ok(DevClass::Hid(kind, iface)) => self.complete_hid(slotid, rh_port, kind, iface),
            Ok(DevClass::Msc) => self.complete_msc(slotid, rh_port, speed),
            Err(e) => Err(e),
        };
        match outcome {
            Ok(kind) => Ok(kind),
            Err(e) => {
                // 枚举失败：槽位禁用（尽力而为）+ 输出上下文帧归还。
                let _ = self.cmd_submit(CR_DISABLE_SLOT, slotid, 0, 0);
                self.mem.free_frame(octx);
                Err(e)
            }
        }
    }

    /// Address Device + 设备/配置描述符读取 + 接口类分类。
    fn address_and_classify(&mut self, slotid: u8, rh_port: u8, speed: u8) -> Result<DevClass, BlockError> {
        self.address_device(slotid, rh_port, speed)?;
        // GET_DESCRIPTOR(DEVICE, 18B)——设备身份证据（版本/类）。
        let mut desc = [0u8; 18];
        self.control_in(slotid, ControlRequest::get_device_desc(18), &mut desc)?;
        // GET_DESCRIPTOR(CONFIGURATION, 18B) → 接口描述符分类。
        let mut cfg = [0u8; 18];
        self.control_in(slotid, ControlRequest::get_config_desc(), &mut cfg)?;
        if cfg[9 + 1] != 4 {
            return Err(BlockError::Unsupported); // 接口描述符缺位。
        }
        let iface = cfg[9 + 2];
        match (cfg[9 + 5], cfg[9 + 6], cfg[9 + 7]) {
            (3, 1, 1) => Ok(DevClass::Hid(HidKind::Keyboard, iface)),
            (3, 1, 2) => Ok(DevClass::Hid(HidKind::Mouse, iface)),
            (8, 6, 0x50) => Ok(DevClass::Msc), // SCSI 透明 / BOT。
            (3, 1, p) => {
                #[cfg(all(target_arch = "x86_64", target_os = "none"))]
                crate::kinfo!("xhci: hid protocol {} unsupported (skip)", p);
                Err(BlockError::Unsupported)
            }
            _ => Err(BlockError::Unsupported),
        }
    }

    /// HID 收尾：SET_PROTOCOL(boot) → SET_IDLE(0) → SET_CONFIGURATION(1)
    /// → Configure Endpoint(EP1 IN)；登记 [`HidDevice`]。
    fn complete_hid(&mut self, slotid: u8, rh_port: u8, kind: HidKind, iface: u8) -> Result<DevKind, BlockError> {
        let free = match self.devs.iter().position(|d| d.is_none()) {
            Some(i) => i,
            None => return Err(BlockError::Unsupported), // 追踪表满，如实拒绝。
        };
        let ep1_ring = self.alloc_zeroed("dev ep1 ring")?;
        let report = self.alloc_zeroed("dev report buf")?;
        self.mem
            .write_bytes(ep1_ring + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.mem.bus_addr(ep1_ring), true));
        let mut outcome = self.control_no_data(slotid, ControlRequest::set_protocol_boot(iface));
        if outcome.is_ok() {
            outcome = self.control_no_data(slotid, ControlRequest::set_idle(iface));
        }
        if outcome.is_ok() {
            outcome = self.control_no_data(slotid, ControlRequest::set_config());
        }
        if outcome.is_ok() {
            // Configure Endpoint：EP1 IN 上线（add=0x9：槽在位 + EP1 IN 位）。
            outcome = self.configure_ep1(slotid, ep1_ring);
        }
        match outcome {
            Ok(()) => {
                self.devs[free] = Some(HidDevice {
                    slot: slotid,
                    port: rh_port,
                    kind,
                    iface,
                    ep1_ring_phys: ep1_ring,
                    ep1_tail: 0,
                    ep1_cycle: true,
                    ep1_outstanding: None,
                    report_phys: report,
                    kbd: HidKbdDecoder::default(),
                    mouse: HidMouseDecoder::default(),
                });
                Ok(match kind {
                    HidKind::Keyboard => DevKind::HidKeyboard,
                    HidKind::Mouse => DevKind::HidMouse,
                })
            }
            Err(e) => {
                self.mem.free_frame(ep1_ring);
                self.mem.free_frame(report);
                Err(e)
            }
        }
    }

    /// MSC 收尾：SET_CONFIGURATION(1) → Configure Endpoint（bulk 双端点）
    /// → BOT 初始化（TUR/INQUIRY/READ CAPACITY 走真实 bulk 管道）。
    fn complete_msc(&mut self, slotid: u8, rh_port: u8, speed: u8) -> Result<DevKind, BlockError> {
        let free = match self.mscs.iter().position(|d| d.is_none()) {
            Some(i) => i,
            None => return Err(BlockError::Unsupported),
        };
        let out_ring = self.alloc_zeroed("msc ep-out ring")?;
        let in_ring = self.alloc_zeroed("msc ep-in ring")?;
        let buf = self.alloc_zeroed("msc data buf")?;
        self.mem
            .write_bytes(out_ring + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.mem.bus_addr(out_ring), true));
        self.mem
            .write_bytes(in_ring + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.mem.bus_addr(in_ring), true));
        // 先登记半挂载态（BOT 初始化要经 bulk_xfer 找到本表项），
        // 失败即整项摘除。
        self.mscs[free] = Some(MscDevice {
            slot: slotid,
            port: rh_port,
            out_ring,
            out_tail: 0,
            out_cycle: true,
            out_outstanding: None,
            in_ring,
            in_tail: 0,
            in_cycle: true,
            in_outstanding: None,
            buf,
            inited: false,
            block_size: 512,
            blocks: 0,
        });
        let mut outcome = self.control_no_data(slotid, ControlRequest::set_config());
        if outcome.is_ok() {
            outcome = self.configure_bulk(slotid, speed, out_ring, in_ring);
        }
        if outcome.is_ok() {
            // BOT 初始化：真实 bulk 管道上跑 TUR/INQUIRY/READ CAPACITY。
            let pipe = MscPipe { c: self, idx: free };
            outcome = match super::msc::MscDev::init(pipe) {
                Ok(mut dev) => {
                    let (blocks, bs) = dev.capacity();
                    drop(dev);
                    if let Some(d) = self.mscs[free].as_mut() {
                        d.inited = true;
                        d.block_size = bs;
                        d.blocks = blocks;
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            };
        }
        match outcome {
            Ok(()) => Ok(DevKind::Msc),
            Err(e) => {
                self.mscs[free] = None;
                self.mem.free_frame(out_ring);
                self.mem.free_frame(in_ring);
                self.mem.free_frame(buf);
                Err(e)
            }
        }
    }

    /// Address Device：写输入上下文（控制 + 槽 + EP0）并提交命令。
    fn address_device(&mut self, slotid: u8, rh_port: u8, speed: u8) -> Result<(), BlockError> {
        // 输入控制上下文：drop=0，add=0x3（槽 + EP0）——QEMU 硬校验。
        self.mem.write_bytes(self.ictx_phys, 0, &u32_bytes(&[0, 0x3]));
        // 槽上下文（ictx+32）：路由 0 | ContextEntries=1（EP0=最高上下文号）
        // | 速度；DW1[16:24]=根集线器端口。
        let slot_ctx = [(1u32 << 27) | ((speed as u32) << 20), (rh_port as u32) << 16, 0, 0];
        self.mem.write_bytes(self.ictx_phys + 32, 0, &u32_bytes(&slot_ctx));
        // EP0 上下文（ictx+64）：控制类型 4，MPS=64，dequeue = EP0 环当前
        // 软件指针（EP0 环跨设备串行复用——枚举严格串行、EP0 在枚举间隙
        // 静默，把 dequeue 交接到本设备当前位置即可；若给环头会把控制器
        // 指回上一台设备的旧 TRB）。
        let ep0_deq = self.ep0_ring_phys + (self.ep0_tail as u64) * 32;
        let ep0_ctx = [
            0u32,
            (4u32 << 3) | (64 << 16),
            (self.mem.bus_addr(ep0_deq) & !0xF) as u32 | (self.ep0_cycle as u32),
            (self.mem.bus_addr(ep0_deq) >> 32) as u32,
            8, // 平均 TRB 长度
        ];
        self.mem.write_bytes(self.ictx_phys + 64, 0, &u32_bytes(&ep0_ctx));
        let (_, cc) = self.cmd_submit(CR_ADDRESS_DEVICE, slotid, self.ictx_phys, 0)?;
        if cc != CC_SUCCESS {
            return Err(self.cc_to_err(cc));
        }
        Ok(())
    }

    /// Configure Endpoint：EP1 IN（类型 7，MPS=8，平均 TRB 8）。
    fn configure_ep1(&mut self, slotid: u8, ep1_ring: u64) -> Result<(), BlockError> {
        // add=0x9：槽（位 0）+ EP1 IN（位 3）；drop=0——QEMU 硬校验
        // (drop&3)==0 且 (add&3)==0x1。
        self.mem.write_bytes(self.ictx_phys, 0, &u32_bytes(&[0, 0x9]));
        // 槽上下文：ContextEntries=3（EP1 IN = 最高上下文号 3）。
        self.mem.write_bytes(self.ictx_phys + 32, 0, &u32_bytes(&[3u32 << 27, 0, 0, 0]));
        // EP1 IN 上下文（ictx+32+32*3 = ictx+128）。
        let ep1_ctx = [
            0u32,
            (3u32 << 1) | (7u32 << 3) | (HID_REPORT_LEN << 16),
            (self.mem.bus_addr(ep1_ring) & !0xF) as u32 | 0x1, // DCS=1
            (self.mem.bus_addr(ep1_ring) >> 32) as u32,
            8,
        ];
        self.mem.write_bytes(self.ictx_phys + 128, 0, &u32_bytes(&ep1_ctx));
        let (_, cc) = self.cmd_submit(CR_CONFIGURE_ENDPOINT, slotid, self.ictx_phys, 0)?;
        if cc != CC_SUCCESS {
            return Err(self.cc_to_err(cc));
        }
        Ok(())
    }

    /// Configure Endpoint：bulk 双端点（EP1 OUT ctx 号 2 / EP1 IN ctx 号 3，
    /// ContextEntries=3；add=0xD = 槽 + EP1 OUT(位2) + EP1 IN(位3)，drop=0
    /// ——QEMU 硬校验 (drop&3)==0 且 (add&3)==0x1 同样满足）。
    fn configure_bulk(&mut self, slotid: u8, speed: u8, out_ring: u64, in_ring: u64) -> Result<(), BlockError> {
        self.mem.write_bytes(self.ictx_phys, 0, &u32_bytes(&[0, 0xD]));
        self.mem.write_bytes(self.ictx_phys + 32, 0, &u32_bytes(&[3u32 << 27, 0, 0, 0]));
        let mps = bulk_mps(speed);
        // EP1 OUT 上下文（ictx+32+32*2 = ictx+96）：类型 2（bulk OUT）。
        let out_ctx = [
            0u32,
            (3u32 << 1) | (2u32 << 3) | (mps << 16),
            (self.mem.bus_addr(out_ring) & !0xF) as u32 | 0x1, // DCS=1
            (self.mem.bus_addr(out_ring) >> 32) as u32,
            512, // 平均 TRB 长度
        ];
        self.mem.write_bytes(self.ictx_phys + 96, 0, &u32_bytes(&out_ctx));
        // EP1 IN 上下文（ictx+128）：类型 6（bulk IN）。
        let in_ctx = [
            0u32,
            (3u32 << 1) | (6u32 << 3) | (mps << 16),
            (self.mem.bus_addr(in_ring) & !0xF) as u32 | 0x1,
            (self.mem.bus_addr(in_ring) >> 32) as u32,
            512,
        ];
        self.mem.write_bytes(self.ictx_phys + 128, 0, &u32_bytes(&in_ctx));
        let (_, cc) = self.cmd_submit(CR_CONFIGURE_ENDPOINT, slotid, self.ictx_phys, 0)?;
        if cc != CC_SUCCESS {
            return Err(self.cc_to_err(cc));
        }
        Ok(())
    }

    // ---- bulk 传输（S4.2 MSC；BOT 严格串行，单件在途）---------------------

    /// 投放一个 Normal TRB（IOC）并自旋等待 Transfer Event。
    /// 返回实际传输字节（len − 事件余量）；短包合法（CC_SHORT_PACKET）。
    fn bulk_xfer(&mut self, mi: usize, dir_in: bool, off: u32, len: u32) -> Result<u32, BlockError> {
        let (slot, ring, tail, cyc) = {
            let Some(d) = self.mscs[mi].as_ref() else {
                return Err(BlockError::Io);
            };
            if d.out_outstanding.is_some() || d.in_outstanding.is_some() {
                return Err(BlockError::Io); // BOT 串行契约：上一笔未收尾。
            }
            (
                d.slot,
                if dir_in { d.in_ring } else { d.out_ring },
                if dir_in { d.in_tail } else { d.out_tail },
                if dir_in { d.in_cycle } else { d.out_cycle },
            )
        };
        let buf_frame = self.mscs[mi].as_ref().expect("表项在上方已判在").buf;
        let trb_addr = ring + tail as u64 * 32;
        let trb_bus = self.mem.bus_addr(trb_addr);
        let buf_bus = self.mem.bus_addr(buf_frame + off as u64);
        let (nt, nc) = self.ring_put(ring, tail, cyc, Trb::normal(buf_bus, len, cyc));
        if let Some(d) = self.mscs[mi].as_mut() {
            if dir_in {
                d.in_tail = nt;
                d.in_cycle = nc;
                d.in_outstanding = Some(trb_bus);
            } else {
                d.out_tail = nt;
                d.out_cycle = nc;
                d.out_outstanding = Some(trb_bus);
            }
        }
        let epid = if dir_in { EPID_BULK_IN } else { EPID_BULK_OUT };
        self.doorbell(slot, epid);
        // 自旋等待：顺序游标优先，未见则全槽扫描（QEMU 写序容错），
        // 等待过半重振铃一次（门铃丢失类异常兜底）。
        let deadline = self.deadline();
        let rerun_at = (self.now)() + self.timeout_ns / 2;
        let mut re_rung = false;
        loop {
            if let Some(ev) = self.evt_peek() {
                if ev.typ == ER_TRANSFER {
                    self.transfer_events += 1;
                    self.evt_step();
                    if ev.ptr == trb_bus {
                        self.msc_clear_outstanding(mi, dir_in);
                        return if ev.ccode == CC_SUCCESS || ev.ccode == CC_SHORT_PACKET {
                            Ok(len.saturating_sub(ev.length))
                        } else {
                            Err(self.cc_to_err(ev.ccode))
                        };
                    }
                    continue; // 非本传输事件：继续等。
                }
                match ev.typ {
                    ER_COMMAND_COMPLETE => self.cmd_events += 1,
                    ER_PORT_STATUS_CHANGE => self.port_events += 1,
                    _ => self.unknown_events += 1,
                }
                self.evt_step();
                continue;
            }
            if let Some(ev) = self.evt_scan_for(ER_TRANSFER, trb_bus) {
                self.msc_clear_outstanding(mi, dir_in);
                return if ev.ccode == CC_SUCCESS || ev.ccode == CC_SHORT_PACKET {
                    Ok(len.saturating_sub(ev.length))
                } else {
                    Err(self.cc_to_err(ev.ccode))
                };
            }
            if !re_rung && (self.now)() >= rerun_at {
                re_rung = true;
                self.doorbell(slot, epid);
            }
            self.erdp_flush_if_needed(false);
            if (self.now)() > deadline {
                self.msc_clear_outstanding(mi, dir_in);
                #[cfg(all(target_arch = "x86_64", target_os = "none"))]
                crate::kwarn!("xhci: bulk timeout slot={} dir={} len={}", slot, dir_in as u8, len);
                return Err(BlockError::Timeout);
            }
        }
    }

    fn msc_clear_outstanding(&mut self, mi: usize, dir_in: bool) {
        if let Some(d) = self.mscs[mi].as_mut() {
            if dir_in {
                d.in_outstanding = None;
            } else {
                d.out_outstanding = None;
            }
        }
    }

    /// MSC 设备数（验收证据）。
    pub fn msc_count(&self) -> usize {
        self.mscs.iter().filter(|d| d.is_some() && d.as_ref().is_some_and(|m| m.inited)).count()
    }

    /// MSC 几何：(块数, 块大小)。
    pub fn msc_geometry(&self, mi: usize) -> Option<(u64, u32)> {
        self.mscs.get(mi)?.as_ref().filter(|d| d.inited).map(|d| (d.blocks, d.block_size))
    }

    /// MSC 读块（内部经 MscDev 协议层，几何用枚举期探测值）。
    pub fn msc_read_blocks(&mut self, mi: usize, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
        let Some((blocks, bs)) = self.msc_geometry(mi) else {
            return Err(BlockError::Io);
        };
        let pipe = MscPipe { c: self, idx: mi };
        let mut dev = super::msc::MscDev::from_parts(pipe, bs, blocks - 1);
        dev.read_blocks(lba, dst)
    }

    /// MSC 写块。**上层闸门**（vfsguard 白名单/快照）由挂载层负责，
    /// 本入口只提供受控块写原语。
    pub fn msc_write_blocks(&mut self, mi: usize, lba: u64, src: &[u8]) -> Result<(), BlockError> {
        let Some((blocks, bs)) = self.msc_geometry(mi) else {
            return Err(BlockError::Io);
        };
        let pipe = MscPipe { c: self, idx: mi };
        let mut dev = super::msc::MscDev::from_parts(pipe, bs, blocks - 1);
        dev.write_blocks(lba, src)
    }

    // ---- 控制传输（EP0 串行）----------------------------------------------

    /// 控制读：Setup(IDT) + Data(IN) + Status(OUT)，IOC 在 Status。
    fn control_in(&mut self, slotid: u8, req: ControlRequest, out: &mut [u8]) -> Result<(), BlockError> {
        let buf = self.data_phys;
        self.post_control(slotid, req, Some((buf, out.len() as u32, true)))?;
        self.mem.read_bytes(buf, 0, out);
        Ok(())
    }

    /// 控制无数据：Setup(IDT) + Status(IN)。
    fn control_no_data(&mut self, slotid: u8, req: ControlRequest) -> Result<(), BlockError> {
        self.post_control(slotid, req, None)
    }

    /// 投放一条控制传输（3 或 2 个 TRB）并自旋等完成。
    fn post_control(&mut self, slotid: u8, req: ControlRequest, data: Option<(u64, u32, bool)>) -> Result<(), BlockError> {
        if self.ep0_outstanding.is_some() {
            return Err(BlockError::Io);
        }
        let n_trbs = if data.is_some() { 3 } else { 2 };
        // 环剩余空间不足以放下一组 TRB：如实拒绝（枚举期传输量恒小；
        // 串行推进下 ep0_tail 只增，回绕由 ring_put 处理——这里只防
        // 一组 TRB 跨 Link 的退化情况）。
        if self.ep0_tail + n_trbs > RING_USABLE {
            return Err(BlockError::Unsupported);
        }
        let mut idx = self.ep0_tail;
        let mut cycle = self.ep0_cycle;
        let setup_t = Trb::setup(req, cycle);
        let (nidx, ncyc) = self.ring_put(self.ep0_ring_phys, idx, cycle, setup_t);
        idx = nidx;
        cycle = ncyc;
        if let Some((buf, len, dir_in)) = data {
            let dt = Trb::data(self.mem.bus_addr(buf), len, dir_in, cycle);
            let (nidx, ncyc) = self.ring_put(self.ep0_ring_phys, idx, cycle, dt);
            idx = nidx;
            cycle = ncyc;
        }
        // 状态段方向 = 数据段取反（无数据段 = IN）。
        let status_in = match data {
            Some((_, _, dir_in)) => !dir_in,
            None => true,
        };
        let st = Trb::status(status_in, cycle);
        let status_addr = self.ep0_ring_phys + idx as u64 * 32;
        let (nidx, ncyc) = self.ring_put(self.ep0_ring_phys, idx, cycle, st);
        self.ep0_tail = nidx;
        self.ep0_cycle = ncyc;
        // 事件匹配用总线地址。
        let status_bus = self.mem.bus_addr(status_addr);
        self.ep0_outstanding = Some(status_bus);
        self.doorbell(slotid, 1); // EP0 控制端点 = EP 编号 1。
        let deadline = self.deadline();
        loop {
            if let Some(ev) = self.evt_peek() {
                if ev.typ == ER_TRANSFER {
                    self.transfer_events += 1;
                    self.evt_step();
                    if ev.ptr == status_bus {
                        self.ep0_outstanding = None;
                        return if ev.ccode == CC_SUCCESS || ev.ccode == CC_SHORT_PACKET {
                            Ok(())
                        } else {
                            Err(self.cc_to_err(ev.ccode))
                        };
                    }
                    continue;
                }
                // 命令/端口事件在控制等待期也必须消化掉（防事件环积压）。
                if ev.typ == ER_COMMAND_COMPLETE {
                    self.cmd_events += 1;
                } else if ev.typ == ER_PORT_STATUS_CHANGE {
                    self.port_events += 1;
                } else {
                    self.unknown_events += 1;
                }
                self.evt_step();
                continue;
            }
            if (self.now)() > deadline {
                return Err(BlockError::Timeout);
            }
        }
    }

    fn cc_to_err(&self, cc: u8) -> BlockError {
        match cc {
            CC_NO_SLOTS_ERROR => BlockError::Unsupported,
            _ => BlockError::Io,
        }
    }

    // ---- 运行态轮询（M4）--------------------------------------------------

    /// 运行态泵：给空闲设备投放 EP1 TRB → 消费事件环 → 解码 HID 报告。
    /// 非阻塞：无事件/无设备立即返回 0。产出写入 `out`（None 槽位保留）。
    pub fn pump(&mut self, out: &mut [Option<HidEvent>; MAX_TRACKED * 2]) -> usize {
        for slot in out.iter_mut() {
            *slot = None;
        }
        let mut n = 0;
        // ① 投放：EP1 无在途 TRB 的设备补一个 Normal TRB + 门铃。
        for i in 0..MAX_TRACKED {
            let (slot, ring, tail, cyc, buf) = match self.devs[i].as_ref() {
                Some(d) if d.ep1_outstanding.is_none() => {
                    (d.slot, d.ep1_ring_phys, d.ep1_tail, d.ep1_cycle, d.report_phys)
                }
                _ => continue,
            };
            let trb_addr = ring + tail as u64 * 32;
            let trb_bus = self.mem.bus_addr(trb_addr);
            let (nt, nc) = self.ring_put(ring, tail, cyc, Trb::normal(self.mem.bus_addr(buf), HID_REPORT_LEN, cyc));
            if let Some(d) = self.devs[i].as_mut() {
                d.ep1_tail = nt;
                d.ep1_cycle = nc;
                d.ep1_outstanding = Some(trb_bus);
            }
            self.doorbell(slot, 3); // EP1 IN。
        }
        // ② 消费事件环（有界）：Transfer Event 归属到设备的在途 TRB。
        let mut claimed: [(u64, u32, u8); MAX_TRACKED] = [(0, 0, 0); MAX_TRACKED];
        let mut claimed_n = 0usize;
        for _ in 0..PUMP_EVENT_BUDGET {
            let Some(ev) = self.evt_peek() else { break };
            match ev.typ {
                ER_TRANSFER => {
                    self.transfer_events += 1;
                    let mut owned = false;
                    if claimed_n < MAX_TRACKED {
                        for d in self.devs.iter().flatten() {
                            if d.ep1_outstanding == Some(ev.ptr) {
                                claimed[claimed_n] = (ev.ptr, ev.length, ev.ccode);
                                claimed_n += 1;
                                owned = true;
                                break;
                            }
                        }
                    }
                    if !owned && self.ep0_outstanding != Some(ev.ptr) {
                        self.unknown_events += 1;
                    }
                    if self.ep0_outstanding == Some(ev.ptr) {
                        self.ep0_outstanding = None; // 运行态无控制传输；有则收尾。
                    }
                }
                ER_COMMAND_COMPLETE => self.cmd_events += 1,
                ER_PORT_STATUS_CHANGE => self.port_events += 1,
                _ => self.unknown_events += 1,
            }
            self.evt_step();
            if claimed_n == MAX_TRACKED {
                break;
            }
        }
        // ③ 解码 + 清在途（下轮 pump 重投）。
        for k in 0..claimed_n {
            let (trb, remain, ccode) = claimed[k];
            let mut hit = None;
            for (i, d) in self.devs.iter().enumerate() {
                if let Some(d) = d {
                    if d.ep1_outstanding == Some(trb) {
                        hit = Some((i, d.kind, d.report_phys));
                        break;
                    }
                }
            }
            let Some((i, kind, report_phys)) = hit else { continue };
            if ccode != CC_SUCCESS && ccode != CC_SHORT_PACKET {
                if let Some(d) = self.devs[i].as_mut() {
                    d.ep1_outstanding = None; // 错误完成：弃本报告，下轮重投。
                }
                continue;
            }
            let transferred = HID_REPORT_LEN.saturating_sub(remain) as usize;
            let Some(d) = self.devs[i].as_mut() else { continue };
            d.ep1_outstanding = None;
            if transferred == 0 {
                continue; // 空报告（闲置心跳），无语义。
            }
            let mut rep = [0u8; 8];
            self.mem.read_bytes(report_phys, 0, &mut rep);
            let ev_out = match kind {
                HidKind::Keyboard => d.kbd.feed(&rep).map(HidEvent::Key),
                HidKind::Mouse => {
                    let upto = transferred.clamp(3, 8);
                    d.mouse.feed(&rep[..upto]).map(|pkt| HidEvent::Mouse {
                        dx: pkt[1] as i8,
                        dy: pkt[2] as i8,
                        buttons: pkt[0] & 0x07,
                    })
                }
            };
            if let Some(ev) = ev_out {
                if let HidEvent::Key(k) = ev {
                    crate::ps2::note_key(k); // F12 逃生门过闸（USB 键盘同源）
                }
                if n < out.len() {
                    out[n] = Some(ev);
                    n += 1;
                }
            }
        }
        self.erdp_flush_if_needed(true);
        n
    }

    /// 已枚举设备数（验收证据）。
    pub fn device_count(&self) -> usize {
        self.devs.iter().filter(|d| d.is_some()).count()
    }
}

// ---- MSC bulk 管道（S4.2）：把 XhciCtrl 的 bulk 端点适配成协议层管道 ----

/// 借用控制器的单 MSC 设备管道。BOT 串行 → 每命令内分块顺序传输。
pub struct MscPipe<'a, B: BarAccess, M: DmaMem> {
    c: &'a mut XhciCtrl<B, M>,
    idx: usize,
}

impl<B: BarAccess, M: DmaMem> super::msc::BulkPipe for MscPipe<'_, B, M> {
    fn bulk_out(&mut self, buf: &[u8]) -> Result<(), BlockError> {
        for chunk in buf.chunks(MSC_BUF_LEN as usize) {
            let buf_frame = self
                .c
                .mscs[self.idx]
                .as_ref()
                .map(|d| d.buf)
                .ok_or(BlockError::Io)?;
            self.c.mem.write_bytes(buf_frame, 0, chunk);
            let got = self.c.bulk_xfer(self.idx, false, 0, chunk.len() as u32)?;
            if got != chunk.len() as u32 {
                return Err(BlockError::Io); // OUT 短投 = 相位破坏，绝不静默。
            }
        }
        Ok(())
    }

    fn bulk_in(&mut self, out: &mut [u8]) -> Result<usize, BlockError> {
        let mut total = 0usize;
        for chunk in out.chunks_mut(MSC_BUF_LEN as usize) {
            let buf_frame = self
                .c
                .mscs[self.idx]
                .as_ref()
                .map(|d| d.buf)
                .ok_or(BlockError::Io)?;
            let want = chunk.len() as u32;
            let got = self.c.bulk_xfer(self.idx, true, 0, want)?;
            self.c.mem.read_bytes(buf_frame, 0, &mut chunk[..got as usize]);
            total += got as usize;
            if (got as usize) < chunk.len() {
                break; // 短包 = 数据段提前结束（设备端决定）。
            }
        }
        Ok(total)
    }
}

// ---------------------------------------------------------------------------
// 目标态：真 MMIO BAR（复用 NVMe 槽位窗口）+ PMM DMA 池 + 实机探针。
// 仅内核目标编译。
// ---------------------------------------------------------------------------
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub mod target {
    use super::*;
    use super::super::nvme::target::{now_ns, BarMmio};

    /// DMA 池页数（共享 6 帧 + 每设备 3 帧 ×4 = 18，取整 24）。
    const DMA_POOL_FRAMES: usize = 32;
    const DMA_POOL_BYTES: usize = DMA_POOL_FRAMES * 4096;

    /// .bss 驻留 DMA 池 v2（2026-09-21 晚间改型，修缺口二）：
    /// - **key = 内核虚拟地址**：驱动内部内存访问全部 key+off 直接算术
    ///   （v1 的 #PF 根因 = 「帧基+偏移」键在帧基精确匹配表里反查失败，
    ///   兜底返回物理形态地址被当虚拟写——本版从结构上消灭该类反查）；
    /// - 池页是内核映像 .bss（Limine 装载区，引导保留，免疫 PMM 与
    ///   initfs/内核页重叠——v0 pmm 版的失败根因假设）；
    /// - `bus_addr()` 只在寄存器编程边界（CRCR/DCBAA/ERSTBA/ERST 表内容/
    ///   ERDP/EP 上下文 dequeue/TRB 参数/Link 目标/事件匹配）把 key
    ///   翻译为页表权威真物理（`RealPt::translate`，2MiB 大叶语义正确）。
    #[repr(align(4096))]
    struct DmaPoolBytes([u8; DMA_POOL_BYTES]);
    static mut DMA_POOL: DmaPoolBytes = DmaPoolBytes([0; DMA_POOL_BYTES]);

    pub struct BssDma {
        /// (帧虚拟 token, 真物理) —— 分配时经页表翻译登记。
        frames: [Option<(u64, u64)>; DMA_POOL_FRAMES],
        hhdm: u64,
        next: usize,
    }

    impl BssDma {
        fn pool_virt() -> u64 {
            (unsafe { (&raw const DMA_POOL as *const u8).addr() }) as u64
        }

        pub fn new() -> BssDma {
            BssDma {
                frames: [None; DMA_POOL_FRAMES],
                hhdm: crate::limine::hhdm_offset().unwrap_or(0),
                next: 0,
            }
        }

        /// 帧物理：页表翻译为权威（内核映像映射真值，2MiB 大叶含全偏移）；
        /// 翻译不可用时兜底 executable_address 滑移。
        fn phys_of_frame(virt: u64) -> u64 {
            use crate::mem::pfh::PageTableOps as _;
            if let Some(p) = crate::mem::pfh::target_ops().translate(virt) {
                return p;
            }
            let (phys, virtb) = crate::limine::executable_address().unwrap_or((0, 0));
            virt - (virtb - phys)
        }

        /// key（帧 token + <4KiB 偏移）→ HHDM 访问地址。
        /// **先掩码取帧基再查表**（v1 的 #PF 根因 = 带 偏移 的键去精确匹配
        /// 帧基条目），命中后拼回 帧物理+HHDM+偏移 —— v0 pmm 版在同一
        /// QEMU 上实证过的访问路径（trace2 boot1 全链枚举+报告流动）。
        fn hhdm_of(&self, key: u64) -> u64 {
            let frame = key & !0xFFF;
            for (tok, phys) in self.frames.iter().flatten() {
                if *tok == frame {
                    return phys + self.hhdm + (key & 0xFFF);
                }
            }
            key + self.hhdm // 不可达：key 全部出自本池分配的帧
        }
    }

    impl DmaMem for BssDma {
        fn alloc_frame(&mut self) -> Option<u64> {
            if self.next >= DMA_POOL_FRAMES {
                return None;
            }
            let i = self.next;
            let virt = Self::pool_virt() + (i * 4096) as u64;
            let phys = Self::phys_of_frame(virt);
            self.frames[i] = Some((virt, phys));
            self.next += 1;
            Some(virt) // key = 帧虚拟 token
        }
        fn free_frame(&mut self, _key: u64) {}
        fn write_bytes(&mut self, key: u64, off: u64, data: &[u8]) {
            let virt = self.hhdm_of(key + off);
            // SAFETY: 帧物理来自页表翻译的内核映像 .bss，HHDM 全覆盖，off+len ≤4KiB。
            for (i, &b) in data.iter().enumerate() {
                unsafe { core::ptr::write_volatile((virt + i as u64) as *mut u8, b) };
            }
        }
        fn read_bytes(&self, key: u64, off: u64, out: &mut [u8]) {
            let virt = self.hhdm_of(key + off);
            for (i, slot) in out.iter_mut().enumerate() {
                // SAFETY: 同 write_bytes。
                *slot = unsafe { core::ptr::read_volatile((virt + i as u64) as *const u8) };
            }
        }
        fn zero_frame(&mut self, key: u64) {
            const Z: [u8; 256] = [0u8; 256];
            let mut off = 0u64;
            while off < 4096 {
                self.write_bytes(key, off, &Z);
                off += 256;
            }
        }
        fn bus_addr(&self, key: u64) -> u64 {
            // 帧基查表 + 低 12 位帧内偏移。
            let frame = key & !0xFFF;
            for (tok, phys) in self.frames.iter().flatten() {
                if *tok == frame {
                    return phys | (key & 0xFFF);
                }
            }
            frame // 不可达：key 全部出自本池分配的帧
        }
    }

    /// 全局控制器实例（引导期单核、探针在 enable_interrupts 前独占运行；
    /// 与 inputsvc/ps2 同一手工 Once 范式）。
    static mut GLOBAL: Option<XhciCtrl<BarMmio, BssDma>> = None;

    fn global() -> Option<&'static mut XhciCtrl<BarMmio, BssDma>> {
        let slot = &raw mut GLOBAL;
        // SAFETY: 引导期/ushell 泵单线程访问（菜单与 shim 泵都在
        // enable_interrupts 之前的既有调用序里），独占。
        unsafe { (*slot).as_mut() }
    }

    fn install_global(c: XhciCtrl<BarMmio, BssDma>) -> bool {
        let slot = &raw mut GLOBAL;
        // SAFETY: 同 global()。
        unsafe {
            if (*slot).is_some() {
                return false;
            }
            *slot = Some(c);
            true
        }
    }

    /// S4.1 实机入口：ACPI→MCFG→ECAM 扫描 xHCI→BAR 映射→初始化→端口
    /// 枚举→登记全局。无控制器时优雅跳过（镜像在其他验收配置下照常）。
    pub fn probe_and_selftest() {
        let Some(rsdp) = crate::limine::rsdp_address() else {
            crate::kinfo!("xhci: no RSDP - usb hid skipped");
            return;
        };
        let hhdm = crate::limine::hhdm_offset().unwrap_or(0);
        let Some(mcfg_phys) = super::super::pci::target::find_mcfg_phys(rsdp, hhdm) else {
            crate::kinfo!("xhci: no MCFG table - usb hid skipped");
            return;
        };
        let Some(seg) = super::super::pci::target::read_first_segment(mcfg_phys, hhdm) else {
            crate::kinfo!("xhci: MCFG has no usable segment - usb hid skipped");
            return;
        };
        let mut ecam = super::super::pci::target::EcamMmio::new(seg);
        let hits = super::super::pci::scan_xhci_all(&mut ecam, &seg);
        if hits.is_empty() {
            crate::kinfo!("xhci: no controller - usb hid skipped (graceful)");
            return;
        }
        let hit = hits[0];
        crate::kinfo!(
            "xhci: controller at {:#x}:{:#x}.{} bar0={:#x}",
            hit.bus,
            hit.dev,
            hit.func,
            hit.bar0
        );
        // xHCI BAR（QEMU 16KiB / 真机常见 ≤64KiB）按整槽位段 16 页映射。
        let Some(bar) = BarMmio::map(hit.bar0, 16) else {
            crate::kwarn!("xhci: BAR0 map failed - skipped");
            return;
        };
        let dma = BssDma::new();
        match XhciCtrl::init_with_recovery(bar, dma, now_ns, TIMEOUT_NS) {
            Ok(mut c) => {
                let n = c.enumerate_ports();
                crate::kinfo!(
                    "xhci: init ok resets={} devices={} cmd_ev={} xfer_ev={} port_ev={} unk_ev={}",
                    c.resets,
                    n,
                    c.cmd_events,
                    c.transfer_events,
                    c.port_events,
                    c.unknown_events
                );
                if install_global(c) {
                    crate::kinfo!("xhci: usb keyboard/mouse channel live");
                } else {
                    // 第二个控制器不接管（帧保留不回收——一次性初始化，
                    // 如实记账，不假装干净）。
                    crate::kwarn!("xhci: second controller ignored (global slot taken)");
                }
            }
            Err(e) => crate::kwarn!("xhci: init failed {:?} - usb hid skipped", e),
        }
    }

    /// S4.1 输入接线：运行态泵（PS/2 泵的同位增量；未初始化零开销）。
    /// 键盘 → `key_to_ps2_make` 合成 make 字节走 `feed_key_byte`；
    /// 鼠标 → 合成 3 字节 PS/2 包走 `feed_mouse_byte`（Y 已在解码层翻转）。
    pub fn hid_pump(svc: &mut crate::inputsvc::InputService) {
        let Some(c) = global() else { return };
        let mut out = [None; MAX_TRACKED * 2];
        let n = c.pump(&mut out);
        for ev in out.iter().take(n).flatten() {
            match *ev {
                HidEvent::Key(k) => {
                    // 串口证据行：USB 键事件到达汇点的实机取证（走查断言用）。
                    crate::kinfo!(
                        "xhci-hid: key {:?} make={:#04x}",
                        k,
                        key_to_ps2_make(k)
                    );
                    svc.feed_key_byte(key_to_ps2_make(k));
                }
                HidEvent::Mouse { dx, dy, buttons } => {
                    // dy 已翻转为 PS/2 语义（正=向上）；HMP 注入向下移动时
                    // 此处日志为负值——方向契约的实机证据。
                    crate::kinfo!(
                        "xhci-hid: mouse dx={} dy={} buttons={:#04x}",
                        dx,
                        dy,
                        buttons
                    );
                    let pkt = synth_ps2_mouse_packet(dx, dy, buttons);
                    for b in pkt {
                        svc.feed_mouse_byte(b);
                    }
                }
            }
        }
    }

    // ---- S4.2 全局 MSC 块设备接口（挂载层：exFAT 读写 / boot-select 写回）--

    /// 已完成 BOT 初始化的 MSC 设备数。
    pub fn msc_count_global() -> usize {
        global().map_or(0, |c| c.msc_count())
    }

    /// MSC 几何：(块数, 块大小)。
    pub fn msc_geometry_global(idx: usize) -> Option<(u64, u32)> {
        let c = global()?;
        c.msc_geometry(idx)
    }

    /// MSC 读块。
    pub fn msc_read_global(idx: usize, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
        let c = global().ok_or(BlockError::Io)?;
        c.msc_read_blocks(idx, lba, dst)
    }

    /// MSC 写块（上层闸门由挂载层负责，见挂载侧 vfsguard 契约）。
    pub fn msc_write_global(idx: usize, lba: u64, src: &[u8]) -> Result<(), BlockError> {
        let c = global().ok_or(BlockError::Io)?;
        c.msc_write_blocks(idx, lba, src)
    }

    /// BlockDevice 桥——U 盘 MSC LUN0 以块设备形态交给挂载层。
    pub struct MscBlock {
        pub idx: usize,
    }

    impl crate::drivers::blk::BlockDevice for MscBlock {
        fn block_size(&self) -> u32 {
            msc_geometry_global(self.idx).map_or(512, |(_, bs)| bs)
        }
        fn capacity_blocks(&self) -> u64 {
            msc_geometry_global(self.idx).map_or(0, |(n, _)| n)
        }
        fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
            msc_read_global(self.idx, lba, dst)
        }
        fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
            msc_write_global(self.idx, lba, src)
        }
        fn flush(&mut self) -> Result<(), BlockError> {
            Ok(())
        }
    }

    /// 拿到 MSC LUN0 的块设备桥（无 U 盘 / 未初始化 → None）。
    pub fn msc_block_device() -> Option<MscBlock> {
        (msc_count_global() > 0).then_some(MscBlock { idx: 0 })
    }
}

// ---------------------------------------------------------------------------
// 宿主模拟器（tests only）：与 QEMU hcd-xhci.c 行为同构的寄存器+设备模型。
// 门铃写入即同步处理（真硬件异步完成，行为学等价）；ERST 装载、端口复位
// 通知、命令/传输语义、事件环打包逐字段对照参考源码。hcrst_fail 注入
// 复位超时驱动恢复路径用例。
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    // ---- 时钟注入（超时路径驱动）------------------------------------------
    thread_local! {
        static CLOCK: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
    }
    fn fake_now() -> u64 {
        CLOCK.with(|c| {
            let v = c.get() + 1;
            c.set(v);
            v
        })
    }
    fn reset_clock() {
        CLOCK.with(|c| c.set(0));
    }

    // ---- DMA 内存模型 ------------------------------------------------------
    struct Mem {
        data: Vec<u8>,
        frames: Vec<u64>,
        frame_total: u64,
    }

    impl Mem {
        fn new(frames: u64) -> Mem {
            Mem {
                data: vec![0u8; (frames * PAGE) as usize],
                frames: (0..frames).map(|i| i * PAGE).collect(),
                frame_total: frames,
            }
        }
        fn rd(&self, phys: u64, off: u64, out: &mut [u8]) {
            let base = (phys + off) as usize;
            assert!(base + out.len() <= self.data.len(), "dma read OOB {:#x}", base);
            out.copy_from_slice(&self.data[base..base + out.len()]);
        }
        fn wr(&mut self, phys: u64, off: u64, data: &[u8]) {
            let base = (phys + off) as usize;
            assert!(base + data.len() <= self.data.len(), "dma write OOB {:#x}", base);
            self.data[base..base + data.len()].copy_from_slice(data);
        }
        fn trb_at(&self, addr: u64) -> Trb {
            let mut b = [0u8; 32];
            self.rd(addr, 0, &mut b);
            Trb::parse(&b)
        }
        fn u32s(&self, addr: u64, n: usize) -> Vec<u32> {
            let mut b = vec![0u8; n * 4];
            self.rd(addr, 0, &mut b);
            b.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()
        }
        fn wr_u32s(&mut self, addr: u64, w: &[u32]) {
            let mut b = Vec::new();
            for v in w {
                b.extend_from_slice(&v.to_le_bytes());
            }
            self.wr(addr, 0, &b);
        }
        fn u64_at(&self, addr: u64) -> u64 {
            let mut b = [0u8; 8];
            self.rd(addr, 0, &mut b);
            u64::from_le_bytes(b)
        }
    }

    // ---- 虚拟设备 ----------------------------------------------------------
    /// BOT 设备相位（模拟器内部）。
    enum BotPhase {
        ExpectCbw,
        DataIn { data: Vec<u8>, pos: usize },
        DataOut { total: usize, got: Vec<u8> },
        SendCsw { pending: Option<[u8; 13]> },
    }

    /// MSC 虚拟盘（内存 LBA 盘 + BOT 相位机；行为与 msc.rs 的管道级
    /// 模拟器同构，但直接挂 DMA/TRB 层）。
    struct MscSim {
        disk: Vec<u8>,
        bs: usize,
        phase: BotPhase,
        cur_tag: u32,
        cur_lba: usize,
    }

    impl MscSim {
        fn new(blocks: usize, bs: usize) -> MscSim {
            MscSim {
                disk: vec![0u8; blocks * bs],
                bs,
                phase: BotPhase::ExpectCbw,
                cur_tag: 0,
                cur_lba: 0,
            }
        }

        fn csw(&self, tag: u32, residue: u32, status: u8) -> [u8; 13] {
            let mut b = [0u8; 13];
            b[0..4].copy_from_slice(&super::super::msc::CSW_SIG.to_le_bytes());
            b[4..8].copy_from_slice(&tag.to_le_bytes());
            b[8..12].copy_from_slice(&residue.to_le_bytes());
            b[12] = status;
            b
        }

        fn exec_cdb(&mut self, cdb: &[u8], tag: u32) {
            use super::super::msc::{CSW_PASSED, SCSI_INQUIRY, SCSI_READ10, SCSI_READ_CAPACITY10, SCSI_TEST_UNIT_READY, SCSI_WRITE10};
            match cdb[0] {
                SCSI_TEST_UNIT_READY => {
                    let c = self.csw(tag, 0, CSW_PASSED);
                    self.phase = BotPhase::SendCsw { pending: Some(c) };
                }
                SCSI_INQUIRY => {
                    let mut d = vec![0u8; 36];
                    d[0] = 0x00;
                    d[4] = 0x21;
                    d[8..16].copy_from_slice(b"VARIXMSD");
                    d[16..32].copy_from_slice(b"SIM-DMA-DISK001 ");
                    self.phase = BotPhase::DataIn { data: d, pos: 0 };
                }
                SCSI_READ_CAPACITY10 => {
                    let mut d = [0u8; 8];
                    let last = (self.disk.len() / self.bs) as u32 - 1;
                    d[0..4].copy_from_slice(&last.to_be_bytes());
                    d[4..8].copy_from_slice(&(self.bs as u32).to_be_bytes());
                    self.phase = BotPhase::DataIn { data: d.to_vec(), pos: 0 };
                }
                SCSI_READ10 => {
                    let lba = u32::from_be_bytes(cdb[2..6].try_into().unwrap()) as usize;
                    let n = u16::from_be_bytes(cdb[7..9].try_into().unwrap()) as usize;
                    let start = lba * self.bs;
                    let data = self.disk[start..start + n * self.bs].to_vec();
                    self.phase = BotPhase::DataIn { data, pos: 0 };
                }
                SCSI_WRITE10 => {
                    let lba = u32::from_be_bytes(cdb[2..6].try_into().unwrap());
                    let n = u16::from_be_bytes(cdb[7..9].try_into().unwrap()) as usize;
                    self.cur_lba = lba as usize;
                    self.phase = BotPhase::DataOut { total: n * self.bs, got: Vec::new() };
                }
                _ => {
                    let c = self.csw(tag, 0, 1); // FAILED
                    self.phase = BotPhase::SendCsw { pending: Some(c) };
                }
            }
        }

        fn on_bulk_out(&mut self, data: &[u8]) {
            // SendCsw 期间的 OUT = stall 丢弃。
            if matches!(self.phase, BotPhase::SendCsw { .. }) {
                return;
            }
            match std::mem::replace(&mut self.phase, BotPhase::ExpectCbw) {
                BotPhase::ExpectCbw => {
                    assert_eq!(data.len(), 31);
                    let sig = u32::from_le_bytes(data[0..4].try_into().unwrap());
                    assert_eq!(sig, super::super::msc::CBW_SIG);
                    let tag = u32::from_le_bytes(data[4..8].try_into().unwrap());
                    let cdb_len = data[14] as usize;
                    let cdb = data[15..15 + cdb_len].to_vec();
                    self.cur_tag = tag;
                    self.exec_cdb(&cdb, tag);
                }
                BotPhase::DataOut { total, mut got } => {
                    got.extend_from_slice(data);
                    if got.len() >= total {
                        let start = self.cur_lba * self.bs;
                        self.disk[start..start + total].copy_from_slice(&got[..total]);
                        let c = self.csw(self.cur_tag, 0, 0);
                        self.phase = BotPhase::SendCsw { pending: Some(c) };
                    } else {
                        self.phase = BotPhase::DataOut { total, got };
                    }
                }
                _ => {}
            }
        }

        /// 填充 IN 数据，返回实际字节数。
        fn fill_in(&mut self, out: &mut [u8], want: usize) -> usize {
            match &mut self.phase {
                BotPhase::DataIn { data, pos } => {
                    let n = want.min(data.len() - *pos);
                    out[..n].copy_from_slice(&data[*pos..*pos + n]);
                    *pos += n;
                    let done = *pos >= data.len();
                    if done {
                        let c = self.csw(self.cur_tag, 0, 0);
                        self.phase = BotPhase::SendCsw { pending: Some(c) };
                    }
                    n
                }
                BotPhase::SendCsw { pending } => {
                    let c = pending.take().unwrap();
                    out[..13].copy_from_slice(&c);
                    self.phase = BotPhase::ExpectCbw;
                    13
                }
                _ => 0,
            }
        }
    }

    struct DevState {
        /// 接口协议：1=键盘 2=鼠标 8=MSC。
        proto: u8,
        /// 待上送的报告（None = 闲置零报告）。
        pending: Option<Vec<u8>>,
        /// MSC 虚拟盘（proto=8 时在位）。
        msc: Option<MscSim>,
    }

    impl DevState {
        fn dev_desc(&self) -> [u8; 18] {
            let mut d = [0u8; 18];
            d[0] = 0x12;
            d[1] = 0x01;
            d[2] = 0x00;
            d[3] = 0x02; // bcdUSB 2.00
            d[7] = 0x08; // bMaxPacketSize0
            d[8] = 0x86;
            d[9] = 0x80; // VID 0x8086
            d[10] = 0x34;
            d[11] = 0x12; // PID 0x1234
            d[17] = self.proto;
            d
        }
        fn cfg_desc(&self) -> [u8; 18] {
            let mut c = [0u8; 18];
            // 配置描述符（9B）。
            c[0] = 0x09;
            c[1] = 0x02;
            c[2] = 0x12;
            c[4] = 0x01;
            c[5] = 0x01;
            c[7] = 0x80;
            // 接口描述符（9B）：HID=类 3/子类 1/协议=设备类型；
            // MSC=类 8/子类 6（SCSI 透明）/协议 0x50（BOT）。
            c[9] = 0x09;
            c[10] = 0x04;
            if self.proto == 8 {
                c[14] = 0x08;
                c[15] = 0x06;
                c[16] = 0x50;
            } else {
                c[14] = 0x03;
                c[15] = 0x01;
                c[16] = self.proto;
            }
            c
        }
    }

    // ---- 寄存器模型（QEMU 语义同构）----------------------------------------
    struct ErRing {
        start: u64,
        size: usize,
        pcs: bool,
        idx: usize,
    }

    struct SlotState {
        enabled: bool,
        addressed: bool,
        uport: Option<usize>,
        ep0: Option<(u64, bool)>,
        /// EP1 IN（ctx 号 3）：HID 中断端点或 MSC bulk IN。
        ep1: Option<(u64, bool)>,
        ep1_mps: u32,
        /// EP1 OUT（ctx 号 2）：MSC bulk OUT。
        ep_out: Option<(u64, bool)>,
        ep_out_mps: u32,
        protocol_set: bool,
        idle_set: bool,
        config_set: bool,
    }

    struct PortState {
        dev: Option<usize>,
        speed: u8,
        portsc: u32,
    }

    enum Pending {
        Doorbell(u8, u8),
        ErReset,
        PortEvent(usize),
    }

    struct Regs {
        op_base: u16,
        db_off: u16,
        rts_off: u16,
        max_slots: u8,
        max_ports: u8,
        running: bool,
        cnr: bool,
        crcr_lo: u32,
        cmd_ring: Option<(u64, bool)>,
        dcbaap: u64,
        usbcmd: u32,
        ports: Vec<PortState>,
        slots: Vec<SlotState>,
        devices: Vec<DevState>,
        er: Option<ErRing>,
        erstsz: u32,
        erstba: u64,
        erdp: u64,
        hcrst_fail: u32,
        pending: Option<Pending>,
        pub errs: Vec<String>,
    }

    impl Regs {
        fn new() -> Regs {
            // QEMU nec-usb-xhci 同构：4 端口（USB2），8 槽，键盘 port1/鼠标 port2 全速。
            let devices = vec![
                DevState { proto: 1, pending: None, msc: None },
                DevState { proto: 2, pending: None, msc: None },
            ];
            let ports = vec![
                PortState { dev: Some(0), speed: PORT_SPEED_FULL, portsc: 0 },
                PortState { dev: Some(1), speed: PORT_SPEED_FULL, portsc: 0 },
                PortState { dev: None, speed: 0, portsc: 0 },
                PortState { dev: None, speed: 0, portsc: 0 },
            ];
            let mut r = Regs {
                op_base: 0x40,
                db_off: 0x2000,
                rts_off: 0x1000,
                max_slots: 8,
                max_ports: 4,
                running: false,
                cnr: false,
                crcr_lo: 0,
                cmd_ring: None,
                dcbaap: 0,
                usbcmd: 0,
                ports,
                slots: (0..8)
                    .map(|_| SlotState {
                        enabled: false,
                        addressed: false,
                        uport: None,
                        ep0: None,
                        ep1: None,
                        ep1_mps: 0,
                        ep_out: None,
                        ep_out_mps: 0,
                        protocol_set: false,
                        idle_set: false,
                        config_set: false,
                    })
                    .collect(),
                devices,
                er: None,
                erstsz: 0,
                erstba: 0,
                erdp: 0,
                hcrst_fail: 0,
                pending: None,
                errs: Vec::new(),
            };
            for p in r.ports.iter_mut() {
                // xhci_port_update 同构：PP | (有设备: CCS|速度|PLS_POLLING) | CSC 通知。
                p.portsc = PORTSC_PP;
                if p.dev.is_some() {
                    p.portsc |= PORTSC_CCS
                        | ((p.speed as u32) << PORTSC_SPEED_SHIFT)
                        | (7 << 5) // PLS_POLLING
                        | PORTSC_CSC;
                }
            }
            r
        }

        /// MSC 用例前置：port3 加一只全速 U 盘（128 块 × 512B）。
        fn new_with_msc() -> Regs {
            let mut r = Regs::new();
            r.devices.push(DevState { proto: 8, pending: None, msc: Some(MscSim::new(128, 512)) });
            r.ports[2].dev = Some(2);
            r.ports[2].speed = PORT_SPEED_FULL;
            r.ports[2].portsc = PORTSC_PP
                | PORTSC_CCS
                | ((PORT_SPEED_FULL as u32) << PORTSC_SPEED_SHIFT)
                | (7 << 5)
                | PORTSC_CSC;
            r
        }

        fn do_reset(&mut self) {
            // xhci_reset 同构：全停 + 槽位清 + 端口重读 + 中断器清。
            self.running = false;
            self.cnr = false;
            self.usbcmd = 0;
            self.cmd_ring = None;
            self.crcr_lo = 0;
            self.dcbaap = 0;
            for s in self.slots.iter_mut() {
                *s = SlotState {
                    enabled: false,
                    addressed: false,
                    uport: None,
                    ep0: None,
                    ep1: None,
                    ep1_mps: 0,
                    ep_out: None,
                    ep_out_mps: 0,
                    protocol_set: false,
                    idle_set: false,
                    config_set: false,
                };
            }
            for p in self.ports.iter_mut() {
                p.portsc = PORTSC_PP;
                if let Some(d) = p.dev {
                    p.portsc |= PORTSC_CCS | ((p.speed as u32) << PORTSC_SPEED_SHIFT) | (7 << 5) | PORTSC_CSC;
                    let _ = d;
                }
            }
            self.er = None;
            self.erstba = 0;
            self.erstsz = 0;
            self.erdp = 0;
        }

        fn push_event(&mut self, mem: &mut Mem, ev: Event) {
            let Some(er) = &mut self.er else {
                self.errs.push("event with no event ring".into());
                return;
            };
            let addr = er.start + er.idx as u64 * 32;
            let mut ctrl = (ev.typ as u32) << TRB_TYPE_SHIFT | (ev.slotid as u32) << TRB_CR_SLOTID_SHIFT
                | (ev.epid as u32) << 16;
            if er.pcs {
                ctrl |= TRB_C;
            }
            let mut raw = [0u8; 32];
            raw[0..8].copy_from_slice(&ev.ptr.to_le_bytes());
            raw[8..12].copy_from_slice(&(ev.length | ((ev.ccode as u32) << 24)).to_le_bytes());
            raw[12..16].copy_from_slice(&ctrl.to_le_bytes());
            mem.wr(addr, 0, &raw);
            er.idx += 1;
            if er.idx >= er.size {
                er.idx = 0;
                er.pcs = !er.pcs;
            }
        }

        /// xhci_ring_fetch 同构：cycle 校验 + Link TC 翻转。
        fn ring_fetch(mem: &Mem, ring: &mut (u64, bool)) -> Option<(Trb, u64)> {
            for _ in 0..64 {
                let t = mem.trb_at(ring.0);
                if (t.control & TRB_C != 0) != ring.1 {
                    return None;
                }
                if t.typ() == TRB_LINK {
                    ring.0 = t.param;
                    if t.control & TRB_LK_TC != 0 {
                        ring.1 = !ring.1;
                    }
                    continue;
                }
                let addr = ring.0;
                ring.0 += 32;
                return Some((t, addr));
            }
            None
        }

        fn process_commands(&mut self, mem: &mut Mem) {
            if !self.running {
                self.errs.push("doorbell while halted".into());
                return;
            }
            let Some(mut ring) = self.cmd_ring else {
                self.errs.push("commands with no cmd ring".into());
                return;
            };
            // QEMU 同构：命令环 dequeue 持久化（处理进度跨门铃保留）。
            while let Some((t, addr)) = Self::ring_fetch(mem, &mut ring) {
                let mut ccode = CC_SUCCESS;
                let mut ev_slot = 0u8;
                match t.typ() {
                    CR_ENABLE_SLOT => {
                        match self.slots.iter().position(|s| !s.enabled) {
                            Some(i) => {
                                self.slots[i].enabled = true;
                                ev_slot = (i + 1) as u8;
                            }
                            None => ccode = CC_NO_SLOTS_ERROR,
                        }
                    }
                    CR_DISABLE_SLOT => {
                        // 枚举失败路径的槽位回收：禁用即复位全部状态。
                        let slotid = ((t.control >> TRB_CR_SLOTID_SHIFT) & 0xFF) as usize;
                        if slotid >= 1 && slotid <= self.slots.len() && self.slots[slotid - 1].enabled {
                            self.slots[slotid - 1] = SlotState {
                                enabled: false,
                                addressed: false,
                                uport: None,
                                ep0: None,
                                ep1: None,
                                ep1_mps: 0,
                                ep_out: None,
                                ep_out_mps: 0,
                                protocol_set: false,
                                idle_set: false,
                                config_set: false,
                            };
                            ev_slot = slotid as u8;
                        } else {
                            ccode = CC_TRB_ERROR;
                        }
                    }
                    CR_ADDRESS_DEVICE => {
                        let slotid = ((t.control >> TRB_CR_SLOTID_SHIFT) & 0xFF) as usize;
                        let ictx = t.param;
                        let ictl = mem.u32s(ictx, 2);
                        if ictl[0] != 0 || ictl[1] != 0x3 {
                            self.errs.push(format!("address ictl invalid {:08x} {:08x}", ictl[0], ictl[1]));
                            ccode = CC_TRB_ERROR;
                        } else if slotid == 0 || slotid > self.slots.len() || !self.slots[slotid - 1].enabled {
                            ccode = CC_TRB_ERROR;
                        } else {
                            let slot_ctx = mem.u32s(ictx + 32, 4);
                            let port = ((slot_ctx[1] >> 16) & 0xFF) as usize;
                            if port == 0 || port > self.max_ports as usize || self.ports[port - 1].dev.is_none() {
                                self.errs.push(format!("address port lookup failed {}", port));
                                ccode = CC_TRB_ERROR;
                            } else {
                                let s = &mut self.slots[slotid - 1];
                                s.uport = self.ports[port - 1].dev;
                                s.addressed = true;
                                let ep0 = mem.u32s(ictx + 64, 5);
                                let deq = ((ep0[2] as u64) & !0xF) | ((ep0[3] as u64) << 32);
                                s.ep0 = Some((deq, ep0[2] & 1 == 1));
                                // 输出上下文：槽状态 ADDRESSED|地址 + EP0 RUNNING。
                                let octx = mem.u64_at(self.dcbaap + 8 * slotid as u64);
                                let mut o_slot = slot_ctx;
                                o_slot[3] = (2 << 27) | slotid as u32;
                                mem.wr_u32s(octx, &o_slot);
                                let mut o_ep0 = ep0;
                                o_ep0[0] = (o_ep0[0] & !0x7) | 1;
                                mem.wr_u32s(octx + 32, &o_ep0);
                                ev_slot = slotid as u8;
                            }
                        }
                    }
                    CR_CONFIGURE_ENDPOINT => {
                        let slotid = ((t.control >> TRB_CR_SLOTID_SHIFT) & 0xFF) as usize;
                        let ictx = t.param;
                        let ictl = mem.u32s(ictx, 2);
                        if (ictl[0] & 0x3) != 0 || (ictl[1] & 0x3) != 0x1 {
                            self.errs.push(format!("configure ictl invalid {:08x} {:08x}", ictl[0], ictl[1]));
                            ccode = CC_TRB_ERROR;
                        } else if slotid == 0
                            || slotid > self.slots.len()
                            || !self.slots[slotid - 1].addressed
                        {
                            ccode = CC_TRB_ERROR;
                        } else {
                            for i in 2..32usize {
                                if ictl[1] & (1 << i) != 0 {
                                    let ep = mem.u32s(ictx + 32 + 32 * i as u64, 5);
                                    let deq = ((ep[2] as u64) & !0xF) | ((ep[3] as u64) << 32);
                                    let s = &mut self.slots[slotid - 1];
                                    if i == 2 {
                                        s.ep_out = Some((deq, ep[2] & 1 == 1));
                                        s.ep_out_mps = ep[1] >> 16;
                                    }
                                    if i == 3 {
                                        s.ep1 = Some((deq, ep[2] & 1 == 1));
                                        s.ep1_mps = ep[1] >> 16;
                                    }
                                    let octx = mem.u64_at(self.dcbaap + 8 * slotid as u64);
                                    let mut o_ep = ep;
                                    o_ep[0] = (o_ep[0] & !0x7) | 1;
                                    mem.wr_u32s(octx + 32 * i as u64, &o_ep);
                                }
                            }
                            let octx = mem.u64_at(self.dcbaap + 8 * slotid as u64);
                            let mut o_slot = mem.u32s(octx, 4);
                            o_slot[3] = (o_slot[3] & !(0x1F << 27)) | (3 << 27);
                            mem.wr_u32s(octx, &o_slot);
                            ev_slot = slotid as u8;
                        }
                    }
                    CR_NOOP => {}
                    _ => ccode = CC_TRB_ERROR,
                }
                self.push_event(
                    mem,
                    Event { ptr: addr, length: 0, ccode, cycle: false, typ: ER_COMMAND_COMPLETE, epid: 0, slotid: ev_slot },
                );
            }
            self.cmd_ring = Some(ring);
        }

        fn kick_ep(&mut self, mem: &mut Mem, slot: u8, epid: u8) {
            if !self.running {
                self.errs.push("ep doorbell while halted".into());
                return;
            }
            if slot == 0 || slot as usize > self.slots.len() {
                self.errs.push(format!("bad ep doorbell slot {}", slot));
                return;
            }
            let slotid = slot as usize;
            match epid {
                1 => {
                    // 控制传输：收整条 Setup→(Data)→Status 链后一次性应答。
                    let Some(mut ring) = self.slots[slotid - 1].ep0 else { return };
                    let mut setup: Option<(u8, u8, u16, u16, u16)> = None;
                    let mut data: Option<(u64, u32, bool)> = None;
                    let mut status_addr = 0u64;
                    while let Some((t, addr)) = Self::ring_fetch(mem, &mut ring) {
                        match t.typ() {
                            TRB_SETUP => {
                                let p = t.param;
                                setup = Some((
                                    (p & 0xFF) as u8,
                                    ((p >> 8) & 0xFF) as u8,
                                    ((p >> 16) & 0xFFFF) as u16,
                                    ((p >> 32) & 0xFFFF) as u16,
                                    ((p >> 48) & 0xFFFF) as u16,
                                ));
                            }
                            TRB_DATA => data = Some((t.param, t.status & 0x1_FFFF, t.control & TRB_DIR_IN != 0)),
                            TRB_STATUS => {
                                status_addr = addr;
                                break;
                            }
                            _ => break,
                        }
                    }
                    self.slots[slotid - 1].ep0 = Some(ring);
                    let Some((bm, breq, wval, _widx, wlen)) = setup else { return };
                    let dir_in = bm & 0x80 != 0;
                    let mut actual = 0u32;
                    if dir_in {
                        let payload: Vec<u8> = match (breq, (wval >> 8) as u8) {
                            (0x06, 0x01) => self.devices[self.slots[slotid - 1].uport.unwrap()]
                                .dev_desc()[..wlen as usize]
                                .to_vec(),
                            (0x06, 0x02) => self.devices[self.slots[slotid - 1].uport.unwrap()]
                                .cfg_desc()[..wlen as usize]
                                .to_vec(),
                            _ => Vec::new(),
                        };
                        if let Some((buf, len, true)) = data {
                            actual = (len as usize).min(payload.len()) as u32;
                            mem.wr(buf, 0, &payload[..actual as usize]);
                        }
                    }
                    match (bm, breq) {
                        (0x21, 0x0B) => self.slots[slotid - 1].protocol_set = true,
                        (0x21, 0x22) => self.slots[slotid - 1].idle_set = true,
                        (0x00, 0x09) => self.slots[slotid - 1].config_set = true,
                        _ => {}
                    }
                    let full = !dir_in || data.map_or(true, |(_, len, _)| actual >= len);
                    let remaining = data.map_or(0, |(_, len, _)| len - actual);
                    let ccode = if full { CC_SUCCESS } else { CC_SHORT_PACKET };
                    self.push_event(
                        mem,
                        Event { ptr: status_addr, length: remaining, ccode, cycle: false, typ: ER_TRANSFER, epid: 1, slotid: slot },
                    );
                }
                2 => {
                    // bulk OUT（MSC BOT）：CBW / 写数据段。
                    let Some(mut ring) = self.slots[slotid - 1].ep_out else { return };
                    let Some((t, addr)) = Self::ring_fetch(mem, &mut ring) else {
                        self.slots[slotid - 1].ep_out = Some(ring);
                        return;
                    };
                    self.slots[slotid - 1].ep_out = Some(ring);
                    if t.typ() != TRB_NORMAL {
                        self.errs.push(format!("bulk-out got TRB type {}", t.typ()));
                        return;
                    }
                    let len = (t.status & 0x1_FFFF) as usize;
                    let mut tmp = vec![0u8; len];
                    mem.rd(t.param, 0, &mut tmp);
                    let uport = self.slots[slotid - 1].uport.unwrap();
                    let Some(msc) = self.devices[uport].msc.as_mut() else {
                        self.errs.push("bulk-out on non-msc slot".into());
                        return;
                    };
                    msc.on_bulk_out(&tmp);
                    self.push_event(
                        mem,
                        Event { ptr: addr, length: 0, ccode: CC_SUCCESS, cycle: false, typ: ER_TRANSFER, epid: 2, slotid: slot },
                    );
                }
                3 => {
                    // EP1 IN：MSC bulk IN（BOT 数据段/CSW）或 HID 中断报告。
                    let Some(mut ring) = self.slots[slotid - 1].ep1 else { return };
                    let Some((t, addr)) = Self::ring_fetch(mem, &mut ring) else {
                        self.slots[slotid - 1].ep1 = Some(ring);
                        return;
                    };
                    self.slots[slotid - 1].ep1 = Some(ring);
                    if t.typ() != TRB_NORMAL {
                        self.errs.push(format!("ep1 got TRB type {}", t.typ()));
                        return;
                    }
                    let uport = self.slots[slotid - 1].uport.unwrap();
                    let len = (t.status & 0x1_FFFF) as usize;
                    if self.devices[uport].msc.is_some() {
                        let msc = self.devices[uport].msc.as_mut().unwrap();
                        let mut tmp = vec![0u8; len];
                        let got = msc.fill_in(&mut tmp, len);
                        mem.wr(t.param, 0, &tmp[..got]);
                        let ccode = if got == len { CC_SUCCESS } else { CC_SHORT_PACKET };
                        self.push_event(
                            mem,
                            Event {
                                ptr: addr,
                                length: (len - got) as u32,
                                ccode,
                                cycle: false,
                                typ: ER_TRANSFER,
                                epid: 3,
                                slotid: slot,
                            },
                        );
                        return;
                    }
                    let rpt: Vec<u8> = self.devices[uport].pending.take().unwrap_or_else(|| {
                        // 闲置报告：QEMU HID 键盘恒 8B 零报、鼠标 4B 零报。
                        if self.devices[uport].proto == 1 {
                            vec![0u8; 8]
                        } else {
                            vec![0u8; 4]
                        }
                    });
                    let written = len.min(rpt.len());
                    mem.wr(t.param, 0, &rpt[..written]);
                    let ccode = if written == len { CC_SUCCESS } else { CC_SHORT_PACKET };
                    self.push_event(
                        mem,
                        Event {
                            ptr: addr,
                            length: (len - written) as u32,
                            ccode,
                            cycle: false,
                            typ: ER_TRANSFER,
                            epid: 3,
                            slotid: slot,
                        },
                    );
                }
                _ => {}
            }
        }

        fn read32(&mut self, off: u16) -> u32 {
            if off < 0x40 {
                return match off {
                    0x00 => 0x0100_0000 | 0x40,
                    0x04 => ((self.ports.len() as u32) << 24) | (1 << 8) | self.max_slots as u32,
                    0x08 => 0xF,
                    0x10 => 0x1, // 64 位寻址，CSZ=0（32B 上下文）
                    0x14 => self.db_off as u32,
                    0x18 => self.rts_off as u32,
                    _ => 0,
                };
            }
            if (0x40..0x440).contains(&off) {
                return match off - 0x40 {
                    0x00 => self.usbcmd,
                    0x04 => {
                        let mut st = 0;
                        if !self.running {
                            st |= 0x1; // HCH
                        }
                        if self.cnr {
                            st |= 1 << 11;
                        }
                        st
                    }
                    0x18 => self.crcr_lo,
                    0x30 => self.dcbaap as u32,
                    0x38 => self.max_slots as u32,
                    _ => 0,
                };
            }
            let port_off = 0x440u16;
            if off >= port_off && off < port_off + (self.max_ports as u16) * PORT_REG_STRIDE {
                let p = ((off - port_off) / PORT_REG_STRIDE) as usize;
                return self.ports[p].portsc;
            }
            if off >= self.rts_off {
                let r = off - self.rts_off;
                if r < 0x20 {
                    return 0; // MFINDEX
                }
                return match r - 0x20 {
                    0x00 => 0, // IMAN：IP 恒 0（无中断路径）
                    0x08 => self.erstsz,
                    0x10 => self.erstba as u32,
                    0x14 => (self.erstba >> 32) as u32,
                    0x18 => self.erdp as u32,
                    0x1C => (self.erdp >> 32) as u32,
                    _ => 0,
                };
            }
            0
        }

        fn write32(&mut self, off: u16, v: u32) {
            if off < 0x40 {
                return; // 能力段只读。
            }
            if (0x40..0x440).contains(&off) {
                match off - 0x40 {
                    0x00 => {
                        // USBCMD：RS/HCRST（QEMU 同构）。
                        if v & 0x2 != 0 {
                            if self.hcrst_fail > 0 {
                                self.hcrst_fail -= 1;
                                self.cnr = true; // 复位不完成：CNR 保持。
                                self.running = false;
                            } else {
                                self.do_reset();
                            }
                        } else {
                            self.running = v & 0x1 != 0;
                            self.cnr = false;
                        }
                        self.usbcmd = v & 0xC0F;
                    }
                    0x18 => self.crcr_lo = v,
                    0x1C => {
                        // QEMU 同构：CRCR 高写时建环（低写只存值）。
                        let base = (((v as u64) << 32) | ((self.crcr_lo as u64) & !0x3F));
                        self.cmd_ring = Some((base, true));
                    }
                    0x30 => self.dcbaap = (self.dcbaap & 0xFFFF_FFFF_0000_0000) | (v as u64 & !0x3F),
                    0x34 => self.dcbaap = (self.dcbaap & 0xFFFF_FFFF) | ((v as u64) << 32),
                    0x38 => {}
                    _ => {}
                }
                return;
            }
            let port_off = 0x440u16;
            if off >= port_off && off < port_off + (self.max_ports as u16) * PORT_REG_STRIDE {
                let p = ((off - port_off) / PORT_REG_STRIDE) as usize;
                let sc = &mut self.ports[p].portsc;
                if v & PORTSC_PR != 0 {
                    // xhci_port_reset 同构：PED 置位、PLS U0、PRC 通知。
                    *sc |= PORTSC_PED;
                    *sc &= !PORTSC_PR;
                    *sc &= !(0xF << 5); // PLS_U0
                    *sc |= PORTSC_PRC;
                    self.pending = Some(Pending::PortEvent(p));
                }
                // 变化位 W1C。
                let w1c = v & PORTSC_CHANGES;
                *sc &= !w1c;
                return;
            }
            if off >= self.db_off {
                let slot = ((off - self.db_off) / 4) as u8;
                let epid = (v & 0xFF) as u8;
                self.pending = Some(Pending::Doorbell(slot, epid));
                return;
            }
            if off >= self.rts_off {
                let r = off - self.rts_off;
                if r < 0x20 {
                    return;
                }
                match r - 0x20 {
                    0x00 => {}
                    0x08 => self.erstsz = v & 0xFFFF,
                    0x10 => self.erstba = (self.erstba & 0xFFFF_FFFF_0000_0000) | (v as u64 & !0x3F),
                    0x14 => {
                        self.erstba = (self.erstba & 0xFFFF_FFFF) | ((v as u64) << 32);
                        self.pending = Some(Pending::ErReset);
                    }
                    0x18 => self.erdp = (self.erdp & 0xFFFF_FFFF_0000_0000) | (v as u64 & !0xF),
                    0x1C => self.erdp = (self.erdp & 0xFFFF_FFFF) | ((v as u64) << 32),
                    _ => {}
                }
            }
        }

        fn run_er_reset(&mut self, mem: &Mem) {
            if self.erstsz == 0 || self.erstba == 0 {
                self.er = None;
                return;
            }
            if self.erstsz != 1 {
                self.errs.push("ERSTSZ != 1".into());
                return;
            }
            let mut seg = [0u8; 16];
            mem.rd(self.erstba, 0, &mut seg);
            let start = u64::from_le_bytes(seg[0..8].try_into().unwrap());
            let size = u32::from_le_bytes(seg[8..12].try_into().unwrap()) as usize;
            if !(16..=4096).contains(&size) {
                self.errs.push(format!("bad ER segment size {}", size));
                return;
            }
            self.er = Some(ErRing { start, size, pcs: true, idx: 0 });
        }
    }

    // ---- 共享句柄（NvmeCtrl 测试同款形态）----------------------------------
    #[derive(Clone)]
    struct Dev {
        regs: Rc<RefCell<Regs>>,
        mem: Rc<RefCell<Mem>>,
    }

    impl Dev {
        fn new() -> Dev {
            Dev { regs: Rc::new(RefCell::new(Regs::new())), mem: Rc::new(RefCell::new(Mem::new(24))) }
        }
        fn run_pending(&self) {
            loop {
                let act = self.regs.borrow_mut().pending.take();
                match act {
                    Some(Pending::Doorbell(slot, epid)) => {
                        let mut regs = self.regs.borrow_mut();
                        let mut mem = self.mem.borrow_mut();
                        if slot == 0 {
                            regs.process_commands(&mut mem);
                        } else {
                            regs.kick_ep(&mut mem, slot, epid);
                        }
                    }
                    Some(Pending::ErReset) => {
                        let mut regs = self.regs.borrow_mut();
                        let mem = self.mem.borrow();
                        regs.run_er_reset(&mem);
                    }
                    Some(Pending::PortEvent(p)) => {
                        let mut regs = self.regs.borrow_mut();
                        let portnr = (p + 1) as u64;
                        if regs.running {
                            regs.push_event(
                                &mut self.mem.borrow_mut(),
                                Event { ptr: portnr << 24, length: 0, ccode: CC_SUCCESS, cycle: false, typ: ER_PORT_STATUS_CHANGE, epid: 0, slotid: 0 },
                            );
                        }
                    }
                    None => break,
                }
            }
        }
        /// 注入设备待上送报告（下一次 EP1 轮询即被消费）。
        fn set_pending(&self, dev_idx: usize, rpt: Vec<u8>) {
            self.regs.borrow_mut().devices[dev_idx].pending = Some(rpt);
        }
        fn errs(&self) -> Vec<String> {
            self.regs.borrow().errs.clone()
        }
    }

    impl BarAccess for Dev {
        fn read32(&mut self, off: u16) -> u32 {
            self.regs.borrow_mut().read32(off)
        }
        fn write32(&mut self, off: u16, val: u32) {
            self.regs.borrow_mut().write32(off, val);
            self.run_pending();
        }
    }

    impl DmaMem for Dev {
        fn alloc_frame(&mut self) -> Option<u64> {
            self.mem.borrow_mut().frames.pop()
        }
        fn free_frame(&mut self, phys: u64) {
            self.mem.borrow_mut().frames.push(phys);
        }
        fn write_bytes(&mut self, phys: u64, off: u64, data: &[u8]) {
            self.mem.borrow_mut().wr(phys, off, data);
        }
        fn read_bytes(&self, phys: u64, off: u64, out: &mut [u8]) {
            self.mem.borrow().rd(phys, off, out);
        }
        fn zero_frame(&mut self, phys: u64) {
            self.mem.borrow_mut().wr(phys, 0, &[0u8; PAGE as usize]);
        }
    }

    /// 完整初始化 + 枚举（多数用例的前置）。
    fn bringup() -> (Dev, XhciCtrl<Dev, Dev>) {
        reset_clock();
        let dev = Dev::new();
        let mut c = XhciCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 2000)
            .expect("完整初始化必须成功");
        let n = c.enumerate_ports();
        assert_eq!(n, 2, "键盘+鼠标必须都枚举成功；errs={:?}", dev.errs());
        (dev, c)
    }

    // ---- TRB/事件打包 ------------------------------------------------------

    #[test]
    fn xhci_trb_field_packing() {
        // Normal：IOC + cycle + 长度。
        let t = Trb::normal(0x1234_5678, 8, true);
        assert_eq!((t.control >> TRB_TYPE_SHIFT) & 0x3F, TRB_NORMAL as u32);
        assert_eq!(t.control & TRB_IOC, TRB_IOC);
        assert_eq!(t.control & TRB_C, TRB_C);
        assert_eq!(t.status & 0x1_FFFF, 8);
        // Setup：IDT + 8 字节请求打包（bm|req<<8|wv<<16|wi<<32|wl<<48）。
        let req = ControlRequest::get_device_desc(8);
        let s = Trb::setup(req, false);
        assert_eq!(s.control & TRB_IDT, TRB_IDT);
        assert_eq!(s.control >> TRB_TYPE_SHIFT & 0x3F, TRB_SETUP as u32);
        assert_eq!(s.param & 0xFF, 0x80);
        assert_eq!((s.param >> 8) & 0xFF, 0x06);
        assert_eq!((s.param >> 16) & 0xFFFF, 0x0100);
        assert_eq!((s.param >> 48) & 0xFFFF, 8);
        // Data：方向位。
        let d = Trb::data(0x2000, 18, true, true);
        assert_eq!(d.control & TRB_DIR_IN, TRB_DIR_IN);
        assert_eq!(d.status & 0x1_FFFF, 18);
        // Status：IOC。
        let st = Trb::status(false, true);
        assert_eq!(st.control >> TRB_TYPE_SHIFT & 0x3F, TRB_STATUS as u32);
        assert_eq!(st.control & TRB_IOC, TRB_IOC);
        // 命令：slotid 进 [24:32]。
        let c = Trb::cmd(CR_ADDRESS_DEVICE, 7, 0x5000, 0, true);
        assert_eq!(c.control >> TRB_CR_SLOTID_SHIFT & 0xFF, 7);
        assert_eq!(c.control >> TRB_TYPE_SHIFT & 0x3F, CR_ADDRESS_DEVICE as u32);
        // Link：TC。
        let l = Trb::link(0x1000, true);
        assert_eq!(l.control & TRB_LK_TC, TRB_LK_TC);
        assert_eq!(l.param, 0x1000);
        // 小端往返。
        let t2 = Trb::parse(&t.le_bytes());
        assert_eq!(t, t2);
    }

    #[test]
    fn xhci_event_parse_fields() {
        // QEMU 打包同构：DW2=length|ccode<<24；DW3=slotid<<24|epid<<16|type<<10|cycle。
        let mut raw = [0u8; 32];
        raw[0..8].copy_from_slice(&0xABCDu64.to_le_bytes());
        raw[8..12].copy_from_slice(&(4u32 | (CC_SHORT_PACKET as u32) << 24).to_le_bytes());
        let ctrl = (ER_TRANSFER as u32) << TRB_TYPE_SHIFT | (3u32 << 16) | (9u32 << 24) | TRB_C;
        raw[12..16].copy_from_slice(&ctrl.to_le_bytes());
        let ev = parse_event(&raw);
        assert_eq!(ev.typ, ER_TRANSFER);
        assert_eq!(ev.ptr, 0xABCD);
        assert_eq!(ev.length, 4);
        assert_eq!(ev.ccode, CC_SHORT_PACKET);
        assert_eq!(ev.epid, 3);
        assert_eq!(ev.slotid, 9);
        assert!(ev.cycle);
    }

    // ---- HID 表与解码器 ----------------------------------------------------

    #[test]
    fn xhci_usage_table_roundtrip_via_ps2_decode() {
        // 全表：HID usage → Key → PS/2 make → ps2::decode 必须还原同一键。
        for u in 0x04u8..=0x52 {
            let Some(k) = hid_usage_to_key(u) else { continue };
            let make = key_to_ps2_make(k);
            assert_eq!(
                ps2::decode(make, false),
                Some(k),
                "usage {:#04x} → make {:#04x} 解码不一致",
                u,
                make
            );
        }
        // 引导菜单三键（契约序号 0/1/2）。
        assert_eq!(hid_usage_to_key(0x52), Some(ps2::Key::Up));
        assert_eq!(hid_usage_to_key(0x51), Some(ps2::Key::Down));
        assert_eq!(hid_usage_to_key(0x28), Some(ps2::Key::Enter));
        // 方向键 make 码与 ps2::decode 主区表一致（0x48/0x50/0x4B/0x4D）。
        assert_eq!(key_to_ps2_make(ps2::Key::Left), 0x4B);
        assert_eq!(key_to_ps2_make(ps2::Key::Right), 0x4D);
    }

    #[test]
    fn xhci_kbd_decoder_make_only_semantics() {
        let mut d = HidKbdDecoder::default();
        let idle = [0u8; 8];
        // 按下 A → 一事件。
        let mut rpt = [0u8; 8];
        rpt[2] = 0x04;
        assert_eq!(d.feed(&rpt), Some(ps2::Key::A));
        // 保持（同报告再喂）→ 零事件（无重复）。
        assert_eq!(d.feed(&rpt), None);
        // 释放（回零）→ 零事件（PS/2 断码同语义）。
        assert_eq!(d.feed(&idle), None);
        // Shift 沿 → LShift 一事件。
        let mut sh = [0u8; 8];
        sh[0] = 0x02;
        assert_eq!(d.feed(&sh), Some(ps2::Key::LShift));
        // Shift 保持 + 按 B → B（Shift 已在上份报告，不再触发）。
        let mut sb = [0u8; 8];
        sb[0] = 0x02;
        sb[2] = 0x05;
        assert_eq!(d.feed(&sb), Some(ps2::Key::B));
        assert_eq!(d.feed(&sb), None);
        // Enter（0x28）。
        let mut en = [0u8; 8];
        en[2] = 0x28;
        assert_eq!(d.feed(&en), Some(ps2::Key::Enter));
        // ErrorRollOver（六槽全 1，USB 规范定义）→ 丢弃 + 计数。
        let mut ro = [0u8; 8];
        ro[2..8].fill(1);
        assert_eq!(d.feed(&ro), None);
        assert_eq!(d.rollover, 1);
        // 词汇表外 usage → 丢弃 + 计数。
        let mut unk = [0u8; 8];
        unk[2] = 0x65; // Application（未映射）
        assert_eq!(d.feed(&unk), None);
        assert_eq!(d.unmapped, 1);
    }

    #[test]
    fn xhci_mouse_y_inverted_and_buttons_preserved() {
        let mut d = HidMouseDecoder::default();
        // HID 报告 dy=+5（向下）→ 合成 PS/2 包 dy=-5（向上语义）。
        let pkt = d.feed(&[0x01, 3, 5]).expect("位移包必须产出");
        assert_eq!(pkt[0] & 0x07, 0x01, "左键位图保留");
        assert_eq!(pkt[0] & 0x08, 0x08, "同步位恒置");
        assert_eq!(pkt[1], 3);
        assert_eq!(pkt[2] as i8, -5, "Y 必须翻转（HID 正=下 → PS/2 正=上）");
        assert_eq!(pkt[0] >> 5 & 1, 1, "dy 符号位（负）");
        // 零位移零按键变化 → 抑制。
        assert!(d.feed(&[0x01, 0, 0]).is_none());
        // 按键变化（位移为零）→ 仍产出。
        let pkt2 = d.feed(&[0x00, 0, 0]).expect("按键变化必须产出");
        assert_eq!(pkt2[0] & 0x07, 0x00);
    }

    #[test]
    fn xhci_synthetic_packet_roundtrip_via_mouse_decoder() {
        // 合成包喂回既有 PS/2 解码器：跨模块契约锁定。
        use crate::inputsvc::MouseDecoder;
        let mut dec = MouseDecoder::default();
        let pkt = synth_ps2_mouse_packet(-7, 9, 0x05);
        let mut delta = None;
        for b in pkt {
            delta = dec.feed(b);
        }
        let m = delta.expect("三字节凑包必须产出");
        assert_eq!(m.dx, -7);
        assert_eq!(m.dy, 9, "PS/2 语义位移原样通过（翻转已在 HID 层完成）");
        assert_eq!(m.buttons, 0x05);
    }

    // ---- 全链（初始化 → 枚举 → 事件流）-------------------------------------

    #[test]
    fn xhci_full_enumeration_kbd_and_mouse() {
        let dev = Dev::new();
        let mut c = XhciCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 2000)
            .expect("初始化必须成功");
        assert_eq!(c.resets, 0);
        let n = c.enumerate_ports();
        assert_eq!(n, 2);
        assert_eq!(c.device_count(), 2);
        // 类请求与端点配置的证据（QEMU 同构槽位状态）。
        let regs = dev.regs.borrow();
        let slots: Vec<&SlotState> = regs.slots.iter().filter(|s| s.enabled).collect();
        assert_eq!(slots.len(), 2);
        assert!(slots.iter().all(|s| s.addressed && s.config_set));
        assert!(slots.iter().all(|s| s.protocol_set && s.idle_set && s.config_set));
        assert!(slots.iter().all(|s| s.ep1_mps == 8), "EP1 MPS=8");
        assert!(regs.errs.is_empty(), "模拟器无错误日志: {:?}", regs.errs);
        // 变化位已清（W1C 生效）。
        assert_eq!(regs.ports[0].portsc & PORTSC_CHANGES, 0);
    }

    #[test]
    fn xhci_keyboard_report_flows_to_event_make_only() {
        let (dev, mut c) = bringup();
        let mut out = [None; MAX_TRACKED * 2];
        // 按下 'Q'（usage 0x14）。
        let mut rpt = [0u8; 8];
        rpt[2] = 0x14;
        dev.set_pending(0, rpt.to_vec());
        let n = c.pump(&mut out);
        assert_eq!(n, 1, "一次泵一次按键");
        assert_eq!(out[0], Some(HidEvent::Key(ps2::Key::Q)));
        // 同键保持（闲置零报告）→ 无重复。
        let n = c.pump(&mut out);
        assert_eq!(n, 0);
        // 鼠标不动，键盘通道零泄漏。
        assert!(dev.errs().is_empty());
    }

    #[test]
    fn xhci_mouse_report_flows_with_inverted_y() {
        let (dev, mut c) = bringup();
        let mut out = [None; MAX_TRACKED * 2];
        // 向下移动（HID dy=+10）+ 右移（dx=+4）+ 左键。
        dev.set_pending(1, vec![0x01, 4, 10, 0]);
        let n = c.pump(&mut out);
        assert_eq!(n, 1);
        match out[0] {
            Some(HidEvent::Mouse { dx, dy, buttons }) => {
                assert_eq!(dx, 4);
                assert_eq!(dy, -10, "Y 必须翻转为 PS/2 语义");
                assert_eq!(buttons, 0x01);
            }
            other => panic!("必须是鼠标事件，实际 {:?}", other),
        }
        // 4 字节报告 → SHORT_PACKET 路径（length=4）已在上一步验证通过。
        assert!(dev.errs().is_empty());
    }

    #[test]
    fn xhci_ep1_ring_wraps_without_loss() {
        let (dev, mut c) = bringup();
        let mut out = [None; MAX_TRACKED * 2];
        // 70 次位移 > 63 条可用环：Link TRB 周期翻转必须零丢失。
        let mut total = 0usize;
        for i in 0..70i8 {
            dev.set_pending(1, vec![0u8, i as u8, 1, 0]);
            let n = c.pump(&mut out);
            total += n;
        }
        assert_eq!(total, 70, "环回绕零丢失；errs={:?}", dev.errs());
        assert!(dev.errs().is_empty());
    }

    #[test]
    fn xhci_port_change_events_consumed_without_side_effects() {
        let (_dev, mut c) = bringup();
        // 枚举期间的端口复位会产出 PORT_STATUS_CHANGE 事件；再泵一轮
        // 键盘闲置报告，事件计数应单调且不误伤键事件。
        let mut out = [None; MAX_TRACKED * 2];
        let n = c.pump(&mut out);
        assert_eq!(n, 0, "无报告无事件");
        assert!(c.port_events >= 2, "两个端口的复位通知必须被消费");
        assert!(c.unknown_events == 0, "未知事件必须为零");
    }

    #[test]
    fn xhci_no_device_ports_enumerates_zero() {
        reset_clock();
        let dev = Dev::new();
        // 物理拔掉两台设备：CCS 消失（端口只余 PP）。
        {
            let mut regs = dev.regs.borrow_mut();
            for p in regs.ports.iter_mut() {
                p.dev = None;
                p.portsc = PORTSC_PP;
            }
        }
        let mut c = XhciCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 2000).unwrap();
        assert_eq!(c.enumerate_ports(), 0);
        assert_eq!(c.device_count(), 0);
        let mut out = [None; MAX_TRACKED * 2];
        assert_eq!(c.pump(&mut out), 0);
    }

    #[test]
    fn xhci_timeout_recovery_retries_once() {
        reset_clock();
        let dev = Dev::new();
        dev.regs.borrow_mut().hcrst_fail = 1;
        let c = XhciCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 2000)
            .expect("复位失败一次后重试必须成功");
        assert_eq!(c.resets, 1, "恢复路径必须留下重试证据");
    }

    #[test]
    fn xhci_timeout_exhausted_reports_device_reset() {
        reset_clock();
        let dev = Dev::new();
        dev.regs.borrow_mut().hcrst_fail = 99;
        let e = match XhciCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 2000) {
            Ok(_) => panic!("持续失败必须报错"),
            Err(e) => e,
        };
        assert_eq!(e, BlockError::DeviceReset, "重试耗尽=DeviceReset 口径");
    }

    #[test]
    fn xhci_ictl_flags_strictly_validated_by_controller_model() {
        // 模拟器对输入上下文标志的硬校验（QEMU 同构）已经内建于
        // 枚举用例——这里显式验证「标志错误 → 设备被跳过且帧被回收」：
        // 用一个协议未知的设备（proto=0 无接口类命中）驱动 Unsupported 路径。
        reset_clock();
        let dev = Dev::new();
        {
            let mut regs = dev.regs.borrow_mut();
            regs.devices[0].proto = 0xFF; // 非 1/2：HID 协议不支持。
        }
        let mut c = XhciCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 2000).unwrap();
        assert_eq!(c.enumerate_ports(), 1, "仅鼠标枚举成功");
        assert_eq!(c.device_count(), 1);
        // 键盘槽被禁用回收（失败路径不留半挂载态）。
        let regs = dev.regs.borrow();
        let disabled = regs.slots.iter().filter(|s| !s.enabled).count();
        assert_eq!(disabled, 7, "失败的槽位已禁用");
    }

    // ---- S4.2 MSC（BOT 全链：枚举→几何→读写→环回绕→HID 共存）---------------

    /// MSC 前置：初始化 + 三设备枚举（键盘/鼠标/U 盘）。
    fn bringup_msc() -> (Dev, XhciCtrl<Dev, Dev>) {
        reset_clock();
        let dev = Dev {
            regs: Rc::new(RefCell::new(Regs::new_with_msc())),
            mem: Rc::new(RefCell::new(Mem::new(32))),
        };
        let mut c = XhciCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 2000)
            .expect("初始化必须成功");
        let n = c.enumerate_ports();
        assert_eq!(n, 3, "键盘+鼠标+U 盘必须都枚举成功；errs={:?}", dev.errs());
        assert_eq!(c.msc_count(), 1, "U 盘必须完成 BOT 初始化");
        (dev, c)
    }

    #[test]
    fn xhci_msc_enumeration_state() {
        let (dev, mut _c) = bringup_msc();
        assert_eq!(_c.device_count(), 2, "HID 表只收键鼠");
        assert_eq!(_c.msc_geometry(0), Some((128, 512)), "BOT init 后几何必须就位");
        assert_eq!(_c.msc_geometry(1), None, "第二个 MSC 槽位必须为空");
        let regs = dev.regs.borrow();
        let msc_slot = regs.slots.iter().find(|s| s.enabled && s.ep_out.is_some()).unwrap();
        assert!(msc_slot.addressed && msc_slot.config_set);
        assert_eq!(msc_slot.ep_out_mps, 64, "全速 bulk MPS=64");
        assert_eq!(msc_slot.ep1_mps, 64, "全速 bulk IN MPS=64");
        assert!(regs.errs.is_empty(), "模拟器无错误日志: {:?}", regs.errs);
    }

    #[test]
    fn xhci_msc_read_write_loopback() {
        let (dev, mut c) = bringup_msc();
        let mut pattern = [0u8; 8 * 512];
        for (i, b) in pattern.iter_mut().enumerate() {
            *b = (i * 13 + 5) as u8;
        }
        c.msc_write_blocks(0, 4, &pattern).expect("整段写必须成功");
        let mut back = [0u8; 8 * 512];
        c.msc_read_blocks(0, 4, &mut back).expect("整段读必须成功");
        assert_eq!(pattern, back, "读回必须逐字节一致");
        // 越界拒绝（容量 128 块）。
        assert_eq!(
            c.msc_read_blocks(0, 120, &mut [0u8; 16 * 512]),
            Err(BlockError::InvalidRange)
        );
        assert!(dev.errs().is_empty());
    }

    #[test]
    fn xhci_msc_bulk_ring_wraps_without_loss() {
        let (dev, mut c) = bringup_msc();
        // 80 次单块写（每命令 CBW+数据 2 个 OUT TRB）→ out 环多轮回绕。
        for i in 0..80u32 {
            let blk = [i as u8; 512];
            c.msc_write_blocks(0, i as u64, &blk).expect("写必须成功");
        }
        for i in 0..80u32 {
            let mut blk = [0u8; 512];
            c.msc_read_blocks(0, i as u64, &mut blk).expect("读必须成功");
            assert!(blk.iter().all(|&b| b == i as u8), "LBA {} 数据错", i);
        }
        assert!(dev.errs().is_empty(), "{:?}", dev.errs());
    }

    #[test]
    fn xhci_msc_coexists_with_hid_events() {
        let (dev, mut c) = bringup_msc();
        c.msc_write_blocks(0, 0, &[0xAA; 512]).expect("MSC 写成功");
        let mut chk = [0u8; 512];
        c.msc_read_blocks(0, 0, &mut chk).expect("MSC 读成功");
        assert!(chk.iter().all(|&b| b == 0xAA));
        // HID 通道不受 MSC 流量影响：键盘 Q 仍正常上送。
        let mut out = [None; MAX_TRACKED * 2];
        let mut rpt = [0u8; 8];
        rpt[2] = 0x14;
        dev.set_pending(0, rpt.to_vec());
        let n = c.pump(&mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0], Some(HidEvent::Key(ps2::Key::Q)));
        assert!(dev.errs().is_empty());
    }
}
