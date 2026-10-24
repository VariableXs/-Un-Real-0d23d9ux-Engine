//! unx0448_mcfgecam —— UNX-F0448 MCFG 表解析（ECAM 基址账）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0448 · MCFG 表解析（ECAM 基址账） · 目标 240 行
//! 判据原文：UNX-F0448-J1 MCFG（签名 "MCFG"）段清单解析：
//!           条目区自 44 起，每条目 16 字节（基址 u64、段组
//!           号 u16、起始总线 u8、终止总线 u8、保留 4 字节）；
//!           解析产出 = 段清单（段号 → (基址, 总线域)）；核
//!           三则：段号重复立案、总线域倒挂（end < start）
//!           立案、ECAM 基址 4K 不对齐立案；容量换算核：段
//!           内每总线 32 设备 × 8 功能 × 4 KiB = 1 MiB/总线，
//!           段容量 = (end-start+1) MiB；寻址换算核：cfg 地
//!           址 = 基址 + ((总线-start) << 20 | 设备 << 15 |
//!           功能 << 12 | 偏移)，偏移 4K 内（越页立案）。
//! 组件：MCFG 条目迭代器 + 段清单账 + 三则核 + 容量换算 +
//!       ECAM 寻址换算器。
//! 职责定位：MCFG 是 PCIe「配置空间的藏宝图」——MMIO 配置
//!   访问全靠段基址+换算公式；寻址换算错一位（<<15 vs <<20）
//!   读到的就是别的设备的配置头；总线域倒挂会让容量算出负
//!   数（无符号下变天文数字）直接打爆 MMIO 窗口。
//! 性能逐项分解：迭代 O(条目数)；三则 O(条目数)；寻址 O(1)。
//! 诊断码段：0x34140（0x34141~0x34149 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MG_VERSION: &str = "unx0448_mcfgecam-v1";

/// 诊断段基址（0x34140 段 · 本模块独占）。
pub const MG_SEG_BASE: u32 = 0x34140;
pub const XC_SIG: u32 = 0x34141; // 签名非 MCFG
pub const XC_HDR_SHORT: u32 = 0x34142; // 头不足 44
pub const XC_SEG_DUP: u32 = 0x34143; // 段号重复
pub const XC_BUS_REV: u32 = 0x34144; // 总线域倒挂
pub const XC_ALIGN: u32 = 0x34145; // 基址 4K 不对齐
pub const XC_OFF_OOB: u32 = 0x34146; // 寄存器偏移越 4K
pub const XC_OOB: u32 = 0x34147; // 条目越表尾
pub const XC_MAGIC: u32 = 0x34149; // 账魔数破

/// MCFG 公共头长 / 条目长。
pub const MCFG_HDR: usize = 44;
pub const ENT_SZ: usize = 16;
/// 每总线 ECAM 空间（32 设备 × 8 功能 × 4 KiB）。
pub const BUS_ECAM_BYTES: u64 = 32 * 8 * 4096; // 1 MiB

/// 单段清单项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegEntry {
    pub base: u64,
    pub seg: u16,
    pub bus_lo: u8,
    pub bus_hi: u8,
}

impl SegEntry {
    /// 段容量（字节）。
    pub fn span_bytes(&self) -> u64 {
        (self.bus_hi as u64 - self.bus_lo as u64 + 1) * BUS_ECAM_BYTES
    }

    /// 三则核（单条目面）。
    pub fn well_formed(&self) -> Result<(), u32> {
        if self.bus_hi < self.bus_lo {
            return Err(XC_BUS_REV);
        }
        if self.base % 4096 != 0 {
            return Err(XC_ALIGN);
        }
        Ok(())
    }
}

/// ECAM 寻址换算：段基址 + (总线-始)<<20 | 设备<<15 | 功能<<12 | 偏移。
pub fn cfg_addr(seg: &SegEntry, bus: u8, dev: u8, fun: u8, off: u16) -> Result<u64, u32> {
    if bus < seg.bus_lo || bus > seg.bus_hi {
        return Err(XC_BUS_REV); // 越域借用倒挂码（域外=域倒挂的退化面）
    }
    if off >= 4096 {
        return Err(XC_OFF_OOB);
    }
    if dev > 31 || fun > 7 {
        return Err(XC_OFF_OOB); // 设备/功能越位面（同页内不可能合法）
    }
    Ok(seg.base + ((bus - seg.bus_lo) as u64) * BUS_ECAM_BYTES
        + ((dev as u64) << 15)
        + ((fun as u64) << 12)
        + off as u64)
}

/// MCFG 全表解析。
pub fn parse_mcfg(raw: &[u8]) -> Result<Vec<SegEntry>, u32> {
    if raw.len() < 4 || &raw[0..4] != b"MCFG" {
        return Err(XC_SIG);
    }
    if raw.len() < MCFG_HDR {
        return Err(XC_HDR_SHORT);
    }
    let mut out = Vec::new();
    let mut off = MCFG_HDR;
    while off < raw.len() {
        if off + ENT_SZ > raw.len() {
            return Err(XC_OOB);
        }
        let e = SegEntry {
            base: u64::from_le_bytes([
                raw[off], raw[off + 1], raw[off + 2], raw[off + 3],
                raw[off + 4], raw[off + 5], raw[off + 6], raw[off + 7],
            ]),
            seg: u16::from_le_bytes([raw[off + 8], raw[off + 9]]),
            bus_lo: raw[off + 10],
            bus_hi: raw[off + 11],
        };
        e.well_formed()?;
        // 段号重复核（全清单面）。
        if out.iter().any(|x: &SegEntry| x.seg == e.seg) {
            return Err(XC_SEG_DUP);
        }
        out.push(e);
        off += ENT_SZ;
    }
    Ok(out)
}

/// 段寻域（按总线找段）。
pub fn seg_of(list: &[SegEntry], bus: u8) -> Option<&SegEntry> {
    list.iter().find(|e| bus >= e.bus_lo && bus <= e.bus_hi)
}

/// 造 MCFG（夹具）。
pub fn mk_mcfg(ents: &[SegEntry]) -> Vec<u8> {
    let mut raw = vec![0u8; MCFG_HDR];
    raw[0..4].copy_from_slice(b"MCFG");
    let total = (MCFG_HDR + ents.len() * ENT_SZ) as u32;
    raw[4..8].copy_from_slice(&total.to_le_bytes());
    for e in ents.iter() {
        raw.extend_from_slice(&e.base.to_le_bytes());
        raw.extend_from_slice(&e.seg.to_le_bytes());
        raw.push(e.bus_lo);
        raw.push(e.bus_hi);
        raw.extend_from_slice(&[0u8; 4]);
    }
    raw
}

/// 判据入口：UNX-F0448。
pub fn run_unx0448_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0448_mcfgecam");
    let seg0 = SegEntry { base: 0xE000_0000, seg: 0, bus_lo: 0, bus_hi: 0xFF };
    let seg1 = SegEntry { base: 0xC000_0000, seg: 1, bus_lo: 0x80, bus_hi: 0xFF };
    // 族一：基本解析。
    let m = mk_mcfg(&[seg0]);
    let l = parse_mcfg(&m);
    cs.add("单段", l.as_ref().map(|v| v.len()).unwrap_or(0) == 1, "44 起条目");
    cs.add("基址 E0000000", l.as_ref().map(|v| v[0].base == 0xE000_0000).unwrap_or(false), "u64 低位");
    cs.add("总线域 0~FF", l.as_ref().map(|v| v[0].bus_lo == 0 && v[0].bus_hi == 0xFF).unwrap_or(false), "全域");
    // 族二：容量换算。
    cs.add("满域 256MiB", seg0.span_bytes() == 256 * 1024 * 1024, "256 总线");
    cs.add("半域 128MiB", seg1.span_bytes() == 128 * 1024 * 1024, "0x80 总线");
    cs.add("单总线 1MiB", SegEntry { bus_lo: 5, bus_hi: 5, ..seg0 }.span_bytes() == BUS_ECAM_BYTES, "32×8×4K");
    // 族三：三则核。
    let rev = SegEntry { base: 0xE000_0000, seg: 2, bus_lo: 9, bus_hi: 3 };
    cs.add("域倒挂立案", rev.well_formed() == Err(XC_BUS_REV), "end<start");
    let mis = SegEntry { base: 0xE000_0800, seg: 2, bus_lo: 0, bus_hi: 1 };
    cs.add("4K 不对齐立案", mis.well_formed() == Err(XC_ALIGN), "低 11 位脏");
    // 族四：段重复。
    let dup = mk_mcfg(&[seg0, SegEntry { seg: 0, ..seg1 }]);
    cs.add("段号重复拒", parse_mcfg(&dup) == Err(XC_SEG_DUP), "同段双图");
    // 族五：越表尾。
    let mut trunc = mk_mcfg(&[seg0]);
    trunc.truncate(MCFG_HDR + 8); // 条目被截半
    cs.add("截半条目拒", parse_mcfg(&trunc) == Err(XC_OOB), "16 字节不满");
    // 族六：ECAM 寻址换算。
    // 公式：base + (bus-lo)<<20 | dev<<15 | fun<<12 | off
    let a = cfg_addr(&seg0, 0, 0, 0, 0);
    cs.add("段基即 addr", a == Ok(0xE000_0000), "bus0 dev0 fun0");
    let a2 = cfg_addr(&seg0, 2, 4, 1, 0);
    cs.add("bus2 偏 2MiB", a2 == Ok(0xE000_0000 + 2 * 0x10_0000), "<<20");
    let a3 = cfg_addr(&seg0, 0, 1, 0, 0);
    cs.add("dev1 偏 0x8000", a3 == Ok(0xE000_0000 + 0x8000), "<<15");
    let a4 = cfg_addr(&seg0, 0, 0, 1, 0);
    cs.add("fun1 偏 0x1000", a4 == Ok(0xE000_0000 + 0x1000), "<<12");
    let a5 = cfg_addr(&seg0, 0, 0, 0, 0xFC);
    cs.add("页内偏移", a5 == Ok(0xE000_0000 + 0xFC), "直加");
    // 族七：越域。
    cs.add("偏 4K 拒", cfg_addr(&seg0, 0, 0, 0, 4096) == Err(XC_OFF_OOB), "每函数 4K");
    cs.add("dev32 拒", cfg_addr(&seg0, 0, 32, 0, 0) == Err(XC_OFF_OOB), "设备 32 不存在");
    cs.add("fun8 拒", cfg_addr(&seg0, 0, 0, 8, 0) == Err(XC_OFF_OOB), "功能 8 不存在");
    cs.add("域外总线拒", cfg_addr(&seg1, 0x79, 0, 0, 0) == Err(XC_BUS_REV), "0x79<0x80");
    // 族八：按总线找段。
    let l2 = parse_mcfg(&mk_mcfg(&[seg0, seg1]));
    cs.add("bus7F 落段0", l2.as_ref().map(|v| seg_of(v, 0x7F).map(|s| s.seg == 0).unwrap_or(false)).unwrap_or(false), "0..FF");
    cs.add("bus80 落段1", l2.as_ref().map(|v| seg_of(v, 0x80).map(|s| s.seg == 1).unwrap_or(false)).unwrap_or(false), "seg1 首");
    // 族九：签名/短头。
    cs.add("非 MCFG 拒", parse_mcfg(b"HPETxxxx") == Err(XC_SIG), "签名闸");
    cs.add("短头拒", parse_mcfg(b"MCFG") == Err(XC_HDR_SHORT), "44 不足");
    // 族十：空条目区。
    cs.add("空段清单", parse_mcfg(&mk_mcfg(&[])).map(|v| v.is_empty()).unwrap_or(false), "合法空图");
    cs
}
