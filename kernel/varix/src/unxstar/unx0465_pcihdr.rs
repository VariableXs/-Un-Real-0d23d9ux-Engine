//! unx0465_pcihdr —— UNX-F0465 PCI 头类型解析（00/01/02 三型）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0465 · 头类型解析 · 目标 220 行
//! 判据原文：UNX-F0465-J1 配置空间头解析账：头类型字段
//!           （offset 0x0E 高 7 位）三型合法（0x00 = 端点设
//!           头、0x01 = PCI 桥头、0x02 = CardBus 桥头）；头
//!           长度按型分派（00/01/02 均 64 字节标准头）；账二
//!           多功能位核：低 1 位 = 多功能旗，与头类型正交；
//!           账三 厂商 ID 闸：0xFFFF = 空槽（合法枚举终态，
//!           非立案）；0x0000 非法（规范禁用，立案）；账四
//!           头型未知（≥ 0x03）→ 跳过立案不解析（前瞻兼
//!           容）；账五 头字段投影：vendor/device/revision/
//!           class 三元组逐字段可读出（只读）。
//! 组件：头类型分派器 + 空槽判别器 + 字段投影器 + 未知型跳
//!       过闸。
//! 职责定位：头类型是 PCI 枚举的「分岔口」——0x00 直走端
//!   点 BAR、0x01 转下级总线递归、0x02 转 CardBus；分派错
//!   一格，桥后面的整棵设备树就全丢了。0xFFFF 空槽是枚举
//!   语义的一部分（不是错误），0x0000 厂商号却是规范明文
//!   禁用的坏值——这两个「全 F/全 0」的直觉陷阱必须写死
//!   在判据里。
//! 性能逐项分解：分派 O(1)；投影 O(1)。
//! 诊断码段：0x34480（0x34481~0x34489 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34480 起）。
pub const SEG_BASE: u32 = 0x34480;
pub const XC_VEN_ZERO: u32 = SEG_BASE + 1; // 厂商 ID 0x0000 非法
pub const XC_TYPE_UNK: u32 = SEG_BASE + 2; // 未知头型

/// 标准头 64 字节。
pub const HDR_LEN: usize = 64;

/// 头类型枚举。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HdrKind {
    Endpoint,  // 0x00
    PciBridge, // 0x01
    CardBus,   // 0x02
}

/// 头字段投影（纯数据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciHdr {
    pub vendor: u16,
    pub device: u16,
    pub revision: u8,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    /// 原始头类型字节（低 1 位多功能旗 + 高 7 位型）。
    pub raw_type: u8,
}

impl PciHdr {
    /// 头类型高 7 位。
    pub fn kind_raw(&self) -> u8 {
        self.raw_type & 0x7F
    }

    /// 多功能旗（低 1 位）。
    pub fn multifunc(&self) -> bool {
        self.raw_type & 0x01 != 0
    }

    /// 型分派：0/1/2 合法；其余 Err。
    pub fn kind(&self) -> Result<HdrKind, u32> {
        match self.kind_raw() {
            0x00 => Ok(HdrKind::Endpoint),
            0x01 => Ok(HdrKind::PciBridge),
            0x02 => Ok(HdrKind::CardBus),
            _ => Err(XC_TYPE_UNK),
        }
    }

    /// 厂商 ID 闸：0x0000 非法；0xFFFF = 空槽（由 empty_slot
    /// 判别，不在本核内立案）。
    pub fn vendor_ok(&self) -> Result<(), u32> {
        if self.vendor == 0x0000 {
            return Err(XC_VEN_ZERO);
        }
        Ok(())
    }

    /// 空槽判别。
    pub fn empty_slot(&self) -> bool {
        self.vendor == 0xFFFF
    }
}

/// 从 64 字节配置空间快照解析头（纯内存，无 I/O）。
pub fn parse_hdr(cfg: &[u8; HDR_LEN]) -> PciHdr {
    let rd16 = |o: usize| u16::from_le_bytes([cfg[o], cfg[o + 1]]);
    PciHdr {
        vendor: rd16(0x00),
        device: rd16(0x02),
        revision: cfg[0x08],
        prog_if: cfg[0x09],
        subclass: cfg[0x0A],
        class: cfg[0x0B],
        raw_type: cfg[0x0E],
    }
}

// ─────────────────────────── 判据面 ───────────────────────────

fn mk_cfg(vendor: u16, raw_type: u8) -> [u8; HDR_LEN] {
    let mut c = [0u8; HDR_LEN];
    c[0..2].copy_from_slice(&vendor.to_le_bytes());
    c[2..4].copy_from_slice(&0x1234u16.to_le_bytes());
    c[0x08] = 0x03; // revision
    c[0x0B] = 0x06; // class bridge 系
    c[0x0A] = 0x04;
    c[0x0E] = raw_type;
    c
}

/// 判据入口：UNX-F0465 全族核。
pub fn run_unx0465_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0465");

    // 族一：三型分派。
    for (t, name) in [(0x00u8, "端点头"), (0x01u8, "PCI 桥头"), (0x02u8, "CardBus 头")] {
        let h = parse_hdr(&mk_cfg(0x8086, t));
        cs.add(name, h.kind().is_ok(), "型合法");
    }
    let h0 = parse_hdr(&mk_cfg(0x8086, 0x00));
    cs.add("端点型核", h0.kind() == Ok(HdrKind::Endpoint), "0x00");
    let h1 = parse_hdr(&mk_cfg(0x8086, 0x01));
    cs.add("桥型核", h1.kind() == Ok(HdrKind::PciBridge), "0x01");

    // 族二：多功能旗正交。
    let hm = parse_hdr(&mk_cfg(0x8086, 0x81)); // 型 1 + 多功能位
    cs.add("多功能旗真", hm.multifunc(), "低 1 位");
    cs.add("旗不扰型", hm.kind_raw() == 0x01, "高 7 位");
    cs.add("桥型仍成", hm.kind() == Ok(HdrKind::PciBridge), "正交");

    // 族三：厂商闸。
    let hz = parse_hdr(&mk_cfg(0x0000, 0x00));
    cs.add("厂商 0 立案", hz.vendor_ok() == Err(XC_VEN_ZERO), "禁用值");
    let hok = parse_hdr(&mk_cfg(0x10DE, 0x00));
    cs.add("厂商合法过", hok.vendor_ok().is_ok(), "0x10DE");

    // 族四：空槽语义。
    let he = parse_hdr(&mk_cfg(0xFFFF, 0xFF));
    cs.add("空槽判真", he.empty_slot(), "0xFFFF");
    cs.add("空槽不立案", he.vendor_ok().is_ok(), "枚举终态");
    cs.add("空槽型未知分派", he.kind() == Err(XC_TYPE_UNK), "0xFF 高 7 位");

    // 族五：未知型跳过。
    let hu = parse_hdr(&mk_cfg(0x8086, 0x06));
    cs.add("型 6 拒", hu.kind() == Err(XC_TYPE_UNK), "前瞻兼容");
    let hu2 = parse_hdr(&mk_cfg(0x8086, 0x03));
    cs.add("型 3 拒", hu2.kind() == Err(XC_TYPE_UNK), "边界外一格");

    // 族六：字段投影。
    let hp = parse_hdr(&mk_cfg(0x8086, 0x00));
    cs.add("vendor 投影", hp.vendor == 0x8086, "0x00 偏移");
    cs.add("device 投影", hp.device == 0x1234, "0x02 偏移");
    cs.add("revision 投影", hp.revision == 0x03, "0x08 偏移");
    cs.add("class 投影", hp.class == 0x06 && hp.subclass == 0x04, "桥类");

    cs
}
