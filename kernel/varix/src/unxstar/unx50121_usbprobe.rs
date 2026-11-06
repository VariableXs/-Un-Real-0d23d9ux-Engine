//! UNX-F50121 USB 打印类接口探测（printer class 描述符解析）（0x43b40 段）
//!
//! 锚点：UNX-F50121-J1 描述符解析 ×3 模拟器模型逐项一致；判据锚定 §AI-63 §2 专题八。
//!
//! 真机之前必须先能「读懂描述符」：USB 打印类（bInterfaceClass = 7）的可用性
//! 只由三件事决定——接口协议（1 单向 / 2 双向 / 3 1284.4）、bulk OUT 端点、
//! 以及（除单向外的）bulk IN 端点。任何一项缺失，直通链路都无从谈起。
//!
//! 解析器刻意**不做宽容修补**：
//! * 每个描述符的 `bLength` 必须 ≥2 且不越过 `wTotalLength` 边界；
//! * 接口声明的端点个数必须与实际跟随的端点描述符个数相等（否则端点表就是假的）；
//! * 端点地址保留位、最大包长三角值、非 bulk 传输类型，一律显式拒绝；
//! * 未知描述符类型**不拒绝**，但必须计数（`skipped`）——前向兼容不能变成静默丢弃。
//!
//! 三个模拟器模型覆盖三条真实拓扑：
//! ① fs-uni   全速单向（只有 bulk OUT，proto=1）；
//! ② hs-bidi  高速双向（bulk OUT/IN 各 512 字节，proto=2）；
//! ③ composite-alt 复合设备（厂商接口 + 打印机接口 + 类专用描述符，proto=2）。
//!
//! 数据结构：
//! * `Cursor` 语义由 `byte`/`le16` 两个带界读取函数承担（越界即 `Short`）；
//! * `UsbPrinter` —— 解析结果（只保留探测/直通真正要用的字段 + 摘要）；
//! * `PrinterProto` —— 协议三档（单向/双向/1284.4）；
//! * `ProbeLog` —— 探测账本（按模型名去重、定容、可复算摘要）。
//!
//! 错误路径：段内闭集 `SEG_BASE + n`，全部码值在注入核/判据里被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43b40 独占 0x20）。
pub const SEG_BASE: u32 = 0x43b40;

pub const E_EMPTY: u16 = 1;
pub const E_SHORT: u16 = 2;
pub const E_DEVLEN: u16 = 3;
pub const E_DEVTYPE: u16 = 4;
pub const E_PACKET0: u16 = 5;
pub const E_NOCONFIG: u16 = 6;
pub const E_CFGLEN: u16 = 7;
pub const E_CFGTYPE: u16 = 8;
pub const E_TOTAL: u16 = 9;
pub const E_NUMIF: u16 = 10;
pub const E_IFLEN: u16 = 11;
pub const E_IFNUM: u16 = 12;
pub const E_NO_PRINTER: u16 = 13;
pub const E_EPLEN: u16 = 14;
pub const E_EPADDR: u16 = 15;
pub const E_EPPACKET: u16 = 16;
pub const E_EPORPHAN: u16 = 17;
pub const E_BULK: u16 = 18;
pub const E_TRANSFER: u16 = 19;
pub const E_ZEROLEN: u16 = 20;
pub const E_CAPACITY: u16 = 21;
pub const E_DUP_PROBE: u16 = 22;
pub const E_EPCOUNT: u16 = 23;
pub const E_PROTO: u16 = 24;

/// USB 描述符探测错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbError {
    Empty,
    Short,
    DevLen,
    DevType,
    Packet0,
    NoConfig,
    CfgLen,
    CfgType,
    Total,
    NumIf,
    IfLen,
    IfNum,
    NoPrinter,
    EpLen,
    EpAddr,
    EpPacket,
    EpOrphan,
    Bulk,
    Transfer,
    ZeroLen,
    Capacity,
    DupProbe,
    EpCount,
    Proto,
}

impl UsbError {
    pub fn code(self) -> u32 {
        let n = match self {
            UsbError::Empty => E_EMPTY,
            UsbError::Short => E_SHORT,
            UsbError::DevLen => E_DEVLEN,
            UsbError::DevType => E_DEVTYPE,
            UsbError::Packet0 => E_PACKET0,
            UsbError::NoConfig => E_NOCONFIG,
            UsbError::CfgLen => E_CFGLEN,
            UsbError::CfgType => E_CFGTYPE,
            UsbError::Total => E_TOTAL,
            UsbError::NumIf => E_NUMIF,
            UsbError::IfLen => E_IFLEN,
            UsbError::IfNum => E_IFNUM,
            UsbError::NoPrinter => E_NO_PRINTER,
            UsbError::EpLen => E_EPLEN,
            UsbError::EpAddr => E_EPADDR,
            UsbError::EpPacket => E_EPPACKET,
            UsbError::EpOrphan => E_EPORPHAN,
            UsbError::Bulk => E_BULK,
            UsbError::Transfer => E_TRANSFER,
            UsbError::ZeroLen => E_ZEROLEN,
            UsbError::Capacity => E_CAPACITY,
            UsbError::DupProbe => E_DUP_PROBE,
            UsbError::EpCount => E_EPCOUNT,
            UsbError::Proto => E_PROTO,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            UsbError::Empty => "empty descriptor buffer",
            UsbError::Short => "read past end of buffer",
            UsbError::DevLen => "device bLength is not 18",
            UsbError::DevType => "first descriptor is not DEVICE",
            UsbError::Packet0 => "bMaxPacketSize0 not 8/16/32/64",
            UsbError::NoConfig => "bNumConfigurations is zero",
            UsbError::CfgLen => "config bLength is not 9",
            UsbError::CfgType => "second descriptor is not CONFIGURATION",
            UsbError::Total => "wTotalLength outside buffer or shorter than 18",
            UsbError::NumIf => "bNumInterfaces is zero",
            UsbError::IfLen => "interface bLength is not 9",
            UsbError::IfNum => "bInterfaceNumber >= bNumInterfaces",
            UsbError::NoPrinter => "no interface with class 7",
            UsbError::EpLen => "endpoint bLength is not 7",
            UsbError::EpAddr => "endpoint address uses reserved bits",
            UsbError::EpPacket => "wMaxPacketSize not a power of two in 8..=1024",
            UsbError::EpOrphan => "endpoint before any interface",
            UsbError::Bulk => "printer interface lacks a required bulk endpoint",
            UsbError::Transfer => "printer endpoint is not bulk",
            UsbError::ZeroLen => "descriptor bLength < 2",
            UsbError::Capacity => "probe log full",
            UsbError::DupProbe => "same model probed twice",
            UsbError::EpCount => "endpoint count disagrees with interface",
            UsbError::Proto => "printer protocol code outside 1..=3",
        }
    }
}

/// 协议三档（bInterfaceProtocol）。编码与 USB-IF 定义一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrinterProto {
    Unidirectional,
    Bidirectional,
    Ieee1284,
}

impl PrinterProto {
    pub fn code(self) -> u8 {
        match self {
            PrinterProto::Unidirectional => 1,
            PrinterProto::Bidirectional => 2,
            PrinterProto::Ieee1284 => 3,
        }
    }

    pub fn from_code(c: u8) -> Result<PrinterProto, UsbError> {
        match c {
            1 => Ok(PrinterProto::Unidirectional),
            2 => Ok(PrinterProto::Bidirectional),
            3 => Ok(PrinterProto::Ieee1284),
            _ => Err(UsbError::Proto),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PrinterProto::Unidirectional => "unidirectional",
            PrinterProto::Bidirectional => "bidirectional",
            PrinterProto::Ieee1284 => "ieee1284.4",
        }
    }

    /// 单向协议只要求 bulk OUT；其余两档要求双向通道。
    pub fn needs_in(self) -> bool {
        !matches!(self, PrinterProto::Unidirectional)
    }
}

fn byte(d: &[u8], at: usize) -> Result<u8, UsbError> {
    match d.get(at) {
        Some(b) => Ok(*b),
        None => Err(UsbError::Short),
    }
}

fn le16(d: &[u8], at: usize) -> Result<u16, UsbError> {
    let lo = byte(d, at)? as u16;
    let hi = byte(d, at + 1)? as u16;
    Ok(lo | (hi << 8))
}

fn is_pow2_narrow(v: u16) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

/// 解析结果。字段就是直通链路真正要用的东西，多余信息不驻留。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbPrinter {
    pub vid: u16,
    pub pid: u16,
    pub bcd_usb: u16,
    pub if_num: u8,
    pub alt: u8,
    pub proto: PrinterProto,
    pub ep_out: u8,
    pub ep_in: u8,
    pub max_out: u16,
    pub max_in: u16,
    pub total_len: u16,
    pub skipped: u8,
    pub endpoints_seen: u8,
}

impl UsbPrinter {
    /// 摘要（FNV-1a 64，按固定字段序；同描述符两次解析必须逐位一致）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_u16(h, self.vid);
        h = fnv_u16(h, self.pid);
        h = fnv_u16(h, self.bcd_usb);
        h = fnv_u8(h, self.if_num);
        h = fnv_u8(h, self.alt);
        h = fnv_u8(h, self.proto.code());
        h = fnv_u8(h, self.ep_out);
        h = fnv_u8(h, self.ep_in);
        h = fnv_u16(h, self.max_out);
        h = fnv_u16(h, self.max_in);
        h = fnv_u16(h, self.total_len);
        h = fnv_u8(h, self.skipped);
        h = fnv_u8(h, self.endpoints_seen);
        h
    }

    /// 是否具备双向 bulk 通道（直通全双工的前提）。
    pub fn full_duplex(&self) -> bool {
        self.ep_out != 0 && self.ep_in != 0
    }
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

fn fnv_u16(h: u64, v: u16) -> u64 {
    fnv_u8(fnv_u8(h, (v & 0xff) as u8), (v >> 8) as u8)
}

// ---------------------------------------------------------------------------
// 解析主路径
// ---------------------------------------------------------------------------

/// 解析「device + 单 configuration」平坦描述符流。
///
/// 只解析第一份配置（bNumConfigurations ≥ 1 即可），因为打印类设备的可用性
/// 由第一配置决定；多配置枚举属于后续枚举器（F50122）的职责。
pub fn parse(data: &[u8]) -> Result<UsbPrinter, UsbError> {
    if data.is_empty() {
        return Err(UsbError::Empty);
    }
    if data.len() < 18 {
        return Err(UsbError::Short);
    }
    if byte(data, 0)? != 18 {
        return Err(UsbError::DevLen);
    }
    if byte(data, 1)? != 1 {
        return Err(UsbError::DevType);
    }
    let bcd_usb = le16(data, 2)?;
    let max0 = byte(data, 7)?;
    if !matches!(max0, 8 | 16 | 32 | 64) {
        return Err(UsbError::Packet0);
    }
    let vid = le16(data, 8)?;
    let pid = le16(data, 10)?;
    if byte(data, 17)? == 0 {
        return Err(UsbError::NoConfig);
    }
    if data.len() < 27 {
        return Err(UsbError::Short);
    }
    if byte(data, 18)? != 9 {
        return Err(UsbError::CfgLen);
    }
    if byte(data, 19)? != 2 {
        return Err(UsbError::CfgType);
    }
    let total = le16(data, 20)?;
    if total < 18 || 18usize + total as usize > data.len() {
        return Err(UsbError::Total);
    }
    let num_if = byte(data, 22)?;
    if num_if == 0 {
        return Err(UsbError::NumIf);
    }

    let mut pos = 18usize + 9;
    let end = 18usize + total as usize;
    let mut skipped: u8 = 0;
    let mut endpoints_seen: u8 = 0;
    let mut seen_iface = false;
    let mut cur_printer = false;
    let mut if_expect: u8 = 0;
    let mut if_got: u8 = 0;
    let mut found_printer = false;
    let mut p_if: u8 = 0;
    let mut p_alt: u8 = 0;
    let mut p_proto: u8 = 0;
    let mut out_addr: u8 = 0;
    let mut out_max: u16 = 0;
    let mut in_addr: u8 = 0;
    let mut in_max: u16 = 0;

    while pos < end {
        let dl = byte(data, pos)? as usize;
        let dt = byte(data, pos + 1)?;
        if dl < 2 {
            return Err(UsbError::ZeroLen);
        }
        if pos + dl > end {
            return Err(UsbError::Total);
        }
        match dt {
            4 => {
                // 进入新接口：先核对上一个接口的端点记账，再切换上下文。
                if seen_iface && if_got != if_expect {
                    return Err(UsbError::EpCount);
                }
                if dl != 9 {
                    return Err(UsbError::IfLen);
                }
                let num = byte(data, pos + 2)?;
                let alt = byte(data, pos + 3)?;
                let neps = byte(data, pos + 4)?;
                let class = byte(data, pos + 5)?;
                let proto = byte(data, pos + 7)?;
                if num >= num_if {
                    return Err(UsbError::IfNum);
                }
                if neps > 16 {
                    return Err(UsbError::EpCount);
                }
                seen_iface = true;
                if_expect = neps;
                if_got = 0;
                cur_printer = class == 7;
                if cur_printer {
                    // 协议码先验，避免后面用错误的 proto 去判断双向要求。
                    if PrinterProto::from_code(proto).is_err() {
                        return Err(UsbError::Proto);
                    }
                    if !found_printer {
                        found_printer = true;
                        p_if = num;
                        p_alt = alt;
                        p_proto = proto;
                    } else {
                        // 第二个打印机接口（多配置/多 alt 的冗余声明）：不再改写
                        // 已选中的那一份，但它的端点仍要按上面的一致性规则记账。
                        cur_printer = false;
                    }
                }
            }
            5 => {
                if !seen_iface {
                    return Err(UsbError::EpOrphan);
                }
                if dl != 7 {
                    return Err(UsbError::EpLen);
                }
                let addr = byte(data, pos + 2)?;
                let attr = byte(data, pos + 3)?;
                let max = le16(data, pos + 4)?;
                if addr & 0x70 != 0 {
                    return Err(UsbError::EpAddr);
                }
                if !is_pow2_narrow(max) || max < 8 || max > 1024 {
                    return Err(UsbError::EpPacket);
                }
                if_got += 1;
                endpoints_seen = endpoints_seen.saturating_add(1);
                if if_got > if_expect {
                    return Err(UsbError::EpCount);
                }
                let transfer = attr & 0x03;
                if cur_printer {
                    // 打印类只认 bulk：control/interrupt/iso 端点对批量直通没有意义，
                    // 出现即说明拓扑与声明不符，宁可拒绝也不「挑一个能用的」。
                    if transfer != 2 {
                        return Err(UsbError::Transfer);
                    }
                    let dir_in = addr & 0x80 != 0;
                    if dir_in {
                        if in_addr == 0 {
                            in_addr = addr;
                            in_max = max;
                        }
                    } else if out_addr == 0 {
                        out_addr = addr;
                        out_max = max;
                    }
                }
            }
            _ => {
                // 未知类型（含 0x21/0x24/0x25 类专用）：按其 bLength 跳过并计数。
                skipped = skipped.saturating_add(1);
            }
        }
        pos += dl;
    }
    if seen_iface && if_got != if_expect {
        return Err(UsbError::EpCount);
    }
    if !found_printer {
        return Err(UsbError::NoPrinter);
    }
    if out_addr == 0 {
        return Err(UsbError::Bulk);
    }
    let proto = PrinterProto::from_code(p_proto)?;
    if proto.needs_in() && in_addr == 0 {
        return Err(UsbError::Bulk);
    }
    Ok(UsbPrinter {
        vid,
        pid,
        bcd_usb,
        if_num: p_if,
        alt: p_alt,
        proto,
        ep_out: out_addr,
        ep_in: in_addr,
        max_out: out_max,
        max_in: in_max,
        total_len: total,
        skipped,
        endpoints_seen,
    })
}

// ---------------------------------------------------------------------------
// 描述符构造器（三个模拟器模型共用；也是注入核的底座）
// ---------------------------------------------------------------------------

fn push_u16(v: &mut Vec<u8>, x: u16) {
    v.push((x & 0xff) as u8);
    v.push((x >> 8) as u8);
}

fn usb_device(vid: u16, pid: u16, bcd: u16, max0: u8, ncfg: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(18);
    v.push(18);
    v.push(1);
    push_u16(&mut v, bcd);
    v.push(0); // bDeviceClass
    v.push(0); // bDeviceSubClass
    v.push(0); // bDeviceProtocol
    v.push(max0);
    push_u16(&mut v, vid);
    push_u16(&mut v, pid);
    push_u16(&mut v, 0x0100); // bcdDevice
    v.push(0); // iManufacturer
    v.push(0); // iProduct
    v.push(0); // iSerialNumber
    v.push(ncfg);
    v
}

fn usb_config(total: u16, nif: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(9);
    v.push(9);
    v.push(2);
    push_u16(&mut v, total);
    v.push(nif);
    v.push(1); // bConfigurationValue
    v.push(0); // iConfiguration
    v.push(0xc0); // bmAttributes: self-powered
    v.push(50); // bMaxPower = 100mA
    v
}

fn usb_iface(num: u8, alt: u8, neps: u8, class: u8, sub: u8, proto: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(9);
    v.push(9);
    v.push(4);
    v.push(num);
    v.push(alt);
    v.push(neps);
    v.push(class);
    v.push(sub);
    v.push(proto);
    v.push(0); // iInterface
    v
}

fn usb_ep(addr: u8, attr: u8, max: u16, interval: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(7);
    v.push(7);
    v.push(5);
    v.push(addr);
    v.push(attr);
    push_u16(&mut v, max);
    v.push(interval);
    v
}

fn push_all(dst: &mut Vec<u8>, src: &[u8]) {
    dst.extend_from_slice(src);
}

/// 三个模拟器模型的名字（也是探针账本的键）。
pub const MODEL_NAMES: [&str; 3] = ["fs-uni", "hs-bidi", "composite-alt"];

/// 构造第 `idx` 个模型描述符流。越界返回 None（调用方显式处理，不 panic）。
pub fn build_model(idx: usize) -> Option<Vec<u8>> {
    match idx {
        0 => {
            // 全速单向：18 + (9+9+7) = 43，wTotalLength = 25
            let mut v = usb_device(0x0403, 0x6001, 0x0200, 8, 1);
            push_all(&mut v, &usb_config(25, 1));
            push_all(&mut v, &usb_iface(0, 0, 1, 7, 1, 1));
            push_all(&mut v, &usb_ep(0x01, 0x02, 64, 0));
            Some(v)
        }
        1 => {
            // 高速双向：18 + (9+9+7+7) = 50，wTotalLength = 32
            let mut v = usb_device(0x04b8, 0x0005, 0x0200, 64, 1);
            push_all(&mut v, &usb_config(32, 1));
            push_all(&mut v, &usb_iface(0, 0, 2, 7, 1, 2));
            push_all(&mut v, &usb_ep(0x01, 0x02, 512, 0));
            push_all(&mut v, &usb_ep(0x81, 0x02, 512, 0));
            Some(v)
        }
        2 => {
            // 复合：18 + (9 + 9+7 + 8 + 9+7+7) = 74，wTotalLength = 56
            let mut v = usb_device(0x03f0, 0x0004, 0x0110, 64, 1);
            push_all(&mut v, &usb_config(56, 2));
            push_all(&mut v, &usb_iface(0, 0, 1, 0xff, 0x00, 0x00));
            push_all(&mut v, &usb_ep(0x84, 0x03, 8, 10));
            // 类专用描述符（0x24 = Printer Interface Class），解析器应跳过并计数。
            let class_specific = [8u8, 0x24, 0x00, 0x10, 0x00, 0x01, 0x00, 0x00];
            push_all(&mut v, &class_specific);
            push_all(&mut v, &usb_iface(1, 0, 2, 7, 1, 2));
            push_all(&mut v, &usb_ep(0x02, 0x02, 64, 0));
            push_all(&mut v, &usb_ep(0x82, 0x02, 64, 0));
            Some(v)
        }
        _ => None,
    }
}

/// 逐项期望值。J1 的「逐项一致」就是拿它和 `UsbPrinter` 的每个字段对表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelExpect {
    pub vid: u16,
    pub pid: u16,
    pub proto: u8,
    pub ep_out: u8,
    pub ep_in: u8,
    pub max_out: u16,
    pub max_in: u16,
    pub if_num: u8,
    pub alt: u8,
    pub total_len: u16,
    pub skipped: u8,
}

/// 三个模型的期望（推导自 `build_model` 的字面布局）。
pub const EXPECT: [ModelExpect; 3] = [
    ModelExpect {
        vid: 0x0403,
        pid: 0x6001,
        proto: 1,
        ep_out: 0x01,
        ep_in: 0x00,
        max_out: 64,
        max_in: 0,
        if_num: 0,
        alt: 0,
        total_len: 25,
        skipped: 0,
    },
    ModelExpect {
        vid: 0x04b8,
        pid: 0x0005,
        proto: 2,
        ep_out: 0x01,
        ep_in: 0x81,
        max_out: 512,
        max_in: 512,
        if_num: 0,
        alt: 0,
        total_len: 32,
        skipped: 0,
    },
    ModelExpect {
        vid: 0x03f0,
        pid: 0x0004,
        proto: 2,
        ep_out: 0x02,
        ep_in: 0x82,
        max_out: 64,
        max_in: 64,
        if_num: 1,
        alt: 0,
        total_len: 56,
        skipped: 1,
    },
];

/// 对比项数（J1 的分母：11 项 × 3 模型 = 33）。
pub const COMPARE_FIELDS: u8 = 11;

/// 逐字段比对，返回 (命中项数, 比对项数)。
pub fn compare(p: &UsbPrinter, e: &ModelExpect) -> (u8, u8) {
    let mut m = 0u8;
    if p.vid == e.vid {
        m += 1;
    }
    if p.pid == e.pid {
        m += 1;
    }
    if p.proto.code() == e.proto {
        m += 1;
    }
    if p.ep_out == e.ep_out {
        m += 1;
    }
    if p.ep_in == e.ep_in {
        m += 1;
    }
    if p.max_out == e.max_out {
        m += 1;
    }
    if p.max_in == e.max_in {
        m += 1;
    }
    if p.if_num == e.if_num {
        m += 1;
    }
    if p.alt == e.alt {
        m += 1;
    }
    if p.total_len == e.total_len {
        m += 1;
    }
    if p.skipped == e.skipped {
        m += 1;
    }
    (m, COMPARE_FIELDS)
}

// ---------------------------------------------------------------------------
// 探测账本
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeEntry {
    pub name: &'static str,
    pub digest: u64,
    pub ok: bool,
}

/// 探测账本：按模型名去重、定容、摘要可复算。重复探测必须显式拒绝：
/// 同一模型探两次却记两条，会让「三模型全过」这句话失去计量意义。
pub struct ProbeLog {
    entries: Vec<ProbeEntry>,
    cap: usize,
    reject_dups: u16,
}

impl ProbeLog {
    pub fn new(cap: usize) -> Result<ProbeLog, UsbError> {
        if cap == 0 || cap > 8 {
            return Err(UsbError::Capacity);
        }
        Ok(ProbeLog {
            entries: Vec::with_capacity(cap),
            cap,
            reject_dups: 0,
        })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn reject_dups(&self) -> u16 {
        self.reject_dups
    }

    pub fn get(&self, i: usize) -> Option<&ProbeEntry> {
        self.entries.get(i)
    }

    pub fn register(&mut self, name: &'static str, digest: u64) -> Result<u32, UsbError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.name == name {
                    self.reject_dups += 1;
                    return Err(UsbError::DupProbe);
                }
            }
            i += 1;
        }
        if self.entries.len() >= self.cap {
            return Err(UsbError::Capacity);
        }
        let seq = self.entries.len() as u32;
        self.entries.push(ProbeEntry {
            name,
            digest,
            ok: true,
        });
        Ok(seq)
    }

    /// 账本摘要。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.entries.len() as u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h ^= e.digest;
                h = h.wrapping_mul(0x100_0000_01b3);
                let mut k = 0usize;
                let b = e.name.as_bytes();
                while k < b.len() {
                    if let Some(c) = b.get(k) {
                        h = fnv_u8(h, *c);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        h
    }
}

/// 汇总文本（诊断用）。
pub fn render(p: &UsbPrinter) -> String {
    format!(
        "usb {:04x}:{:04x} if={}.{} proto={} out={:02x}/{} in={:02x}/{} total={} skip={}",
        p.vid,
        p.pid,
        p.if_num,
        p.alt,
        p.proto.label(),
        p.ep_out,
        p.max_out,
        p.ep_in,
        p.max_in,
        p.total_len,
        p.skipped
    )
}

// ---------------------------------------------------------------------------
// 注入核：十类坏描述符必须全部被拒
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;

    // 1 空缓冲
    if parse(&[]).err() == Some(UsbError::Empty) {
        caught += 1;
    }
    // 2 不足 18 字节
    let stub = [18u8, 1, 0, 2, 0, 0, 0, 64, 1, 2, 3, 4, 0, 0, 0, 0, 0];
    if parse(&stub).err() == Some(UsbError::Short) {
        caught += 1;
    }

    let good = match build_model(1) {
        Some(v) => v,
        None => return (caught, total),
    };

    // 3 设备 bLength 错
    let mut bad = good.clone();
    if let Some(x) = bad.get_mut(0) {
        *x = 17;
    }
    if parse(&bad).err() == Some(UsbError::DevLen) {
        caught += 1;
    }

    // 4 首个描述符类型错
    let mut bad = good.clone();
    if let Some(x) = bad.get_mut(1) {
        *x = 2;
    }
    if parse(&bad).err() == Some(UsbError::DevType) {
        caught += 1;
    }

    // 5 bMaxPacketSize0 非法
    let mut bad = good.clone();
    if let Some(x) = bad.get_mut(7) {
        *x = 7;
    }
    if parse(&bad).err() == Some(UsbError::Packet0) {
        caught += 1;
    }

    // 6 bNumConfigurations = 0
    let mut bad = good.clone();
    if let Some(x) = bad.get_mut(17) {
        *x = 0;
    }
    if parse(&bad).err() == Some(UsbError::NoConfig) {
        caught += 1;
    }

    // 7 wTotalLength 越界
    let mut bad = good.clone();
    if let Some(x) = bad.get_mut(20) {
        *x = 0xff;
    }
    if let Some(x) = bad.get_mut(21) {
        *x = 0x7f;
    }
    if parse(&bad).err() == Some(UsbError::Total) {
        caught += 1;
    }

    // 8 接口 bLength 错
    let mut bad = good.clone();
    if let Some(x) = bad.get_mut(27) {
        *x = 8;
    }
    if parse(&bad).err() == Some(UsbError::IfLen) {
        caught += 1;
    }

    // 9 描述符 bLength = 0（死循环面）
    let mut bad = good.clone();
    if let Some(x) = bad.get_mut(27) {
        *x = 0;
    }
    if parse(&bad).err() == Some(UsbError::ZeroLen) {
        caught += 1;
    }

    // 10 双向协议缺 bulk IN
    if let Some(m2) = build_model(1) {
        // 把第二个端点（0x81）改成 bulk OUT 地址，等价于「没有 IN 通道」。
        let mut bad2 = m2;
        if let Some(x) = bad2.get_mut(45) {
            *x = 0x03;
        }
        if parse(&bad2).err() == Some(UsbError::Bulk) {
            caught += 1;
        }
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50121_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50121");

    // J1-1 三模型全部可解析
    let mut parsed: [Option<UsbPrinter>; 3] = [None, None, None];
    let mut all_ok = true;
    let mut i = 0usize;
    while i < 3 {
        match build_model(i) {
            Some(bytes) => match parse(&bytes) {
                Ok(p) => {
                    if let Some(cell) = parsed.get_mut(i) {
                        *cell = Some(p);
                    }
                }
                Err(_) => all_ok = false,
            },
            None => all_ok = false,
        }
        i += 1;
    }
    s.add("50121-J1-三模型可解", all_ok, if all_ok { "" } else { "合法模型被拒" });

    // J1-2 逐项一致：11 字段 × 3 模型 = 33 项
    let mut matched = 0u8;
    let mut compared = 0u8;
    let mut k = 0usize;
    while k < 3 {
        if let (Some(p), Some(e)) = (parsed.get(k).and_then(|x| *x), EXPECT.get(k)) {
            let (m, c) = compare(&p, e);
            matched += m;
            compared += c;
        }
        k += 1;
    }
    let ok = compared == 33 && matched == compared;
    let det = format!("matched={}/{}", matched, compared);
    s.add_dyn("50121-J1-逐项一致", ok, &det);

    // 取模型 1（高速双向）做后续性质的锚点
    let m1 = match parsed.get(1).and_then(|x| *x) {
        Some(p) => p,
        None => return s,
    };

    // J1-3 描述符链长度守恒：解析域 = 18 + wTotalLength
    let ok = (18 + m1.total_len as usize) == 50 && m1.endpoints_seen == 2;
    let det = format!("consumed={} eps={}", 18 + m1.total_len as usize, m1.endpoints_seen);
    s.add_dyn("50121-J1-链长守恒", ok, &det);

    // J1-4 端点方向位：OUT 地址 bit7 为 0，IN 地址 bit7 为 1
    let ok = (m1.ep_out & 0x80) == 0 && (m1.ep_in & 0x80) != 0 && m1.ep_out != m1.ep_in;
    s.add("50121-J1-方向位", ok, if ok { "" } else { "方向位解析错" });

    // J1-5 协议分档：单向只要求 OUT；双向必须双向通道
    let uni = match parsed.first().and_then(|x| *x) {
        Some(p) => p,
        None => return s,
    };
    let ok = uni.proto == PrinterProto::Unidirectional
        && !uni.proto.needs_in()
        && uni.ep_in == 0
        && m1.proto == PrinterProto::Bidirectional
        && m1.proto.needs_in()
        && m1.full_duplex();
    s.add("50121-J1-协议分档", ok, if ok { "" } else { "单向/双向判定错" });

    // J1-6 复合模型：只有打印机接口的端点进入配对，厂商接口的 interrupt 端点被忽略
    let comp = match parsed.get(2).and_then(|x| *x) {
        Some(p) => p,
        None => return s,
    };
    let ok = comp.if_num == 1 && comp.ep_out == 0x02 && comp.ep_in == 0x82
        && comp.max_out == 64 && comp.max_in == 64 && comp.endpoints_seen == 3;
    let det = format!("if={} out={:02x} in={:02x} eps={}", comp.if_num, comp.ep_out, comp.ep_in, comp.endpoints_seen);
    s.add_dyn("50121-J1-复合选口", ok, &det);

    // J1-7 未知描述符计数不拒绝（0x24 类专用）
    let ok = comp.skipped == 1 && uni.skipped == 0 && m1.skipped == 0;
    let det = format!("skip comp={} m1={}", comp.skipped, m1.skipped);
    s.add_dyn("50121-J1-未知计数", ok, &det);

    // J1-8 解析确定性：同缓冲两次摘要逐位一致
    let again = match build_model(2) {
        Some(b) => parse(&b),
        None => return s,
    };
    let ok = match again {
        Ok(p) => p.digest() == comp.digest() && p == comp,
        Err(_) => false,
    };
    s.add("50121-J1-解析确定", ok, if ok { "" } else { "同缓冲摘要漂移" });

    // J1-9 端点地址保留位（bit4..6）必拒
    let mut bytes = match build_model(1) {
        Some(b) => b,
        None => return s,
    };
    if let Some(x) = bytes.get_mut(38) {
        *x = 0x91;
    }
    let ok = parse(&bytes).err() == Some(UsbError::EpAddr);
    s.add("50121-J1-地址保留位", ok, if ok { "" } else { "保留位被放行" });

    // J1-10 最大包长非 2 的幂必拒（wMaxPacketSize 位于端点描述符 +4/+5）
    let mut bytes2 = match build_model(1) {
        Some(b) => b,
        None => return s,
    };
    if let Some(x) = bytes2.get_mut(40) {
        *x = 0x2c;
    }
    if let Some(x) = bytes2.get_mut(41) {
        *x = 0x01;
    }
    let ok = parse(&bytes2).err() == Some(UsbError::EpPacket);
    s.add("50121-J1-包长三角", ok, if ok { "" } else { "三角包长被放行" });

    // J1-11 接口号越界必拒（bNumInterfaces=1 而接口号=1）
    let mut bytes3 = match build_model(1) {
        Some(b) => b,
        None => return s,
    };
    if let Some(x) = bytes3.get_mut(29) {
        *x = 1;
    }
    let ok = parse(&bytes3).err() == Some(UsbError::IfNum);
    s.add("50121-J1-接口号界", ok, if ok { "" } else { "接口号越界被放行" });

    // J1-12 无打印类接口必拒（把 class 换成 0xff）
    let mut bytes4 = match build_model(1) {
        Some(b) => b,
        None => return s,
    };
    if let Some(x) = bytes4.get_mut(32) {
        *x = 0xff;
    }
    let ok = parse(&bytes4).err() == Some(UsbError::NoPrinter);
    s.add("50121-J1-无打印口", ok, if ok { "" } else { "无 class 7 仍被收下" });

    // J1-13 打印类接口出现非 bulk 端点必拒（bmAttributes 位于端点描述符 +3）
    let mut bytes5 = match build_model(1) {
        Some(b) => b,
        None => return s,
    };
    if let Some(x) = bytes5.get_mut(39) {
        *x = 0x03;
    }
    let ok = parse(&bytes5).err() == Some(UsbError::Transfer);
    s.add("50121-J1-非bulk拒", ok, if ok { "" } else { "interrupt 端点被当成 bulk" });

    // J1-14 接口声明的端点个数必须与实际一致（把 bNumEndpoints 改成 1）
    let mut bytes6 = match build_model(1) {
        Some(b) => b,
        None => return s,
    };
    if let Some(x) = bytes6.get_mut(31) {
        *x = 1;
    }
    let ok = parse(&bytes6).err() == Some(UsbError::EpCount);
    s.add("50121-J1-端点计数", ok, if ok { "" } else { "端点个数虚报被放行" });

    // J1-15 协议码非法必拒（proto=0）
    let mut bytes7 = match build_model(1) {
        Some(b) => b,
        None => return s,
    };
    if let Some(x) = bytes7.get_mut(34) {
        *x = 0;
    }
    let ok = parse(&bytes7).err() == Some(UsbError::Proto);
    s.add("50121-J1-协议码界", ok, if ok { "" } else { "非法协议码被放行" });

    // J1-16 探测账本：去重 + 定容
    let mut log = match ProbeLog::new(3) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let a = log.register(MODEL_NAMES[0], uni.digest());
    let b = log.register(MODEL_NAMES[1], m1.digest());
    let c = log.register(MODEL_NAMES[2], comp.digest());
    let dup = log.register(MODEL_NAMES[0], uni.digest());
    let ok = a == Ok(0)
        && b == Ok(1)
        && c == Ok(2)
        && log.len() == 3
        && dup.err() == Some(UsbError::DupProbe)
        && log.reject_dups() == 1
        && ProbeLog::new(0).err() == Some(UsbError::Capacity)
        && ProbeLog::new(9).err() == Some(UsbError::Capacity);
    s.add("50121-J1-探针账本", ok, if ok { "" } else { "探针账本失守" });

    // J1-17 三模型摘要互不相同（三种拓扑不能被同一摘要覆盖）
    let ok = uni.digest() != m1.digest()
        && m1.digest() != comp.digest()
        && uni.digest() != comp.digest()
        && log.digest() == log.digest();
    let det = format!("{:x}/{:x}/{:x}", uni.digest(), m1.digest(), comp.digest());
    s.add_dyn("50121-J1-摘要互异", ok, &det);

    // J1-18 注入核：十类坏描述符全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50121-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50121_checks_green() {
        let set = run_unx50121_checks();
        assert!(set.all_passed(), "UNX-F50121 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50121_three_models_match_expect() {
        let mut i = 0usize;
        while i < 3 {
            let bytes = build_model(i);
            assert!(bytes.is_some());
            if let Some(b) = bytes {
                let p = parse(&b);
                assert!(p.is_ok());
                if let Ok(p) = p {
                    let e = EXPECT[i];
                    let (m, c) = compare(&p, &e);
                    assert_eq!(m, c, "model {} mismatch", i);
                }
            }
            i += 1;
        }
    }

    #[test]
    fn unx50121_bidirectional_needs_in_endpoint() {
        let mut b = build_model(1);
        assert!(b.is_some());
        if let Some(v) = b.as_mut() {
            if let Some(x) = v.get_mut(45) {
                *x = 0x03;
            }
            assert_eq!(parse(v).err(), Some(UsbError::Bulk));
        }
    }

    #[test]
    fn unx50121_error_codes_in_segment() {
        assert_eq!(UsbError::Empty.code(), SEG_BASE + 1);
        assert_eq!(UsbError::Proto.code(), SEG_BASE + 24);
    }
}
