//! unx0445_madtirq —— UNX-F0445 MADT 解析（中断项账）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0445 · MADT 解析（中断项账） · 目标 240 行
//! 判据原文：UNX-F0445-J1 MADT（APIC 表，签名 "APIC"）条目
//!           区解析：头 44 字节（Local APIC 地址 u32 @36 +
//!           Flags u32 @40），条目自 44 起，每条目 2 字节头
//!           （类型 u8 + 长度 u8）；账五类：类型 0 = 本地
//!           APIC（含 UID/APIC ID/Flags，长度 8）、类型 1 =
//!           IO APIC（含 IOAPIC ID/地址/GSI 基，长度 12）、
//!           类型 2 = 中断源覆盖 ISO（总线/源/GSI/Flags，长
//!           度 10）、类型 4 = NMI 源（长度 6）、类型 9 = 本
//!           地 x2APIC（长度 16）；未知类型跳过（长度字段保
//!           证步进）；长度 0 条目立案（步进死锁）；CPU 账 =
//!           类型 0/9 计数，ISO 账 = 类型 2 重定向清单（GSI
//!           重复立案）。
//! 组件：MADT 头核 + 条目迭代器（类型分派）+ 五类条目解
//!       析器 + 未知跳过 + CPU/ISO 分账 + GSI 重复哨兵。
//! 职责定位：MADT 是「中断拓扑总图」——几个核、几颗 IOAPIC、
//!   哪些 ISA 线被重定向到哪个 GSI 全在这张表里；解析错一
//!   类就错一路中断路由；未知类型跳过是 ACPI 前向兼容的命
//!   门（新固件加新类型不许炸旧内核）；长度 0 是条目区的
//!   死循环陷阱。
//! 性能逐项分解：头核 O(1)；条目迭代 O(条目数)；GSI 查重
//!   O(ISO²)（小账面）。
//! 诊断码段：0x34080（0x34081~0x34089 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MI_VERSION: &str = "unx0445_madtirq-v1";

/// 诊断段基址（0x34080 段 · 本模块独占）。
pub const MI_SEG_BASE: u32 = 0x34080;
pub const XC_SIG: u32 = 0x34081; // 签名非 APIC
pub const XC_HDR_SHORT: u32 = 0x34082; // 头不足 44
pub const XC_LEN0: u32 = 0x34083; // 条目长度 0
pub const XC_LEN_OOB: u32 = 0x34084; // 条目越表尾
pub const XC_GSI_DUP: u32 = 0x34085; // ISO GSI 重复
pub const XC_MAGIC: u32 = 0x34089; // 账魔数破

/// MADT 公共头长。
pub const MADT_HDR: usize = 44;
/// 条目类型。
pub const T_LAPIC: u8 = 0;
pub const T_IOAPIC: u8 = 1;
pub const T_ISO: u8 = 2;
pub const T_NMI: u8 = 4;
pub const T_X2APIC: u8 = 9;

/// LAPIC 条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LapicEnt {
    pub uid: u8,
    pub apic_id: u8,
    pub enabled: bool,
}

/// IOAPIC 条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IoApicEnt {
    pub id: u8,
    pub addr: u32,
    pub gsi_base: u32,
}

/// ISO 条目（中断源覆盖）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IsoEnt {
    pub bus_src: u8,
    pub irq_src: u8,
    pub gsi: u32,
    /// Flags 位 0~1：极性；位 2~3：触发模式（3 = 电平）。
    pub flags: u16,
}

/// 解析分账。
#[derive(Default, Debug, PartialEq, Eq)]
pub struct MadtBook {
    pub lapics: Vec<LapicEnt>,
    pub ioapics: Vec<IoApicEnt>,
    pub isos: Vec<IsoEnt>,
    pub nmi_srcs: u32,
    pub x2apics: u32,
    pub unknown_skipped: u32,
}

impl MadtBook {
    /// CPU 总数（LAPIC + x2APIC）。
    pub fn cpu_total(&self) -> u32 {
        self.lapics.len() as u32 + self.x2apics
    }

    /// GSI 重复哨兵（ISO 间）。
    pub fn gsi_dup(&self) -> Result<(), u32> {
        for i in 0..self.isos.len() {
            for j in (i + 1)..self.isos.len() {
                if self.isos[i].gsi == self.isos[j].gsi {
                    return Err(XC_GSI_DUP);
                }
            }
        }
        Ok(())
    }
}

/// MADT 全表解析。
pub fn parse_madt(raw: &[u8]) -> Result<MadtBook, u32> {
    if raw.len() < 4 || &raw[0..4] != b"APIC" {
        return Err(XC_SIG);
    }
    if raw.len() < MADT_HDR {
        return Err(XC_HDR_SHORT);
    }
    let mut book = MadtBook::default();
    let mut off = MADT_HDR;
    while off < raw.len() {
        if off + 2 > raw.len() {
            return Err(XC_LEN_OOB);
        }
        let ty = raw[off];
        let ln = raw[off + 1] as usize;
        if ln == 0 {
            return Err(XC_LEN0);
        }
        if off + ln > raw.len() {
            return Err(XC_LEN_OOB);
        }
        let body = &raw[off..off + ln];
        match ty {
            T_LAPIC if ln >= 8 => {
                book.lapics.push(LapicEnt {
                    uid: body[2],
                    apic_id: body[3],
                    enabled: body[4] & 1 != 0,
                });
            }
            T_IOAPIC if ln >= 12 => {
                book.ioapics.push(IoApicEnt {
                    id: body[2],
                    addr: u32::from_le_bytes([body[4], body[5], body[6], body[7]]),
                    gsi_base: u32::from_le_bytes([body[8], body[9], body[10], body[11]]),
                });
            }
            T_ISO if ln >= 10 => {
                book.isos.push(IsoEnt {
                    bus_src: body[2],
                    irq_src: body[3],
                    gsi: u32::from_le_bytes([body[4], body[5], body[6], body[7]]),
                    flags: u16::from_le_bytes([body[8], body[9]]),
                });
            }
            T_NMI => {
                book.nmi_srcs += 1;
            }
            T_X2APIC if ln >= 16 => {
                if body[4] & 1 != 0 {
                    book.x2apics += 1; // 仅计启用
                }
            }
            _ => {
                book.unknown_skipped += 1;
            }
        }
        off += ln;
    }
    Ok(book)
}

/// 造 MADT（夹具）：条目序列编码。
pub fn mk_madt(ents: &[Vec<u8>]) -> Vec<u8> {
    let mut raw = vec![0u8; MADT_HDR];
    raw[0..4].copy_from_slice(b"APIC");
    let total = (MADT_HDR + ents.iter().map(|e| e.len()).sum::<usize>()) as u32;
    raw[4..8].copy_from_slice(&total.to_le_bytes());
    for e in ents.iter() {
        raw.extend_from_slice(e);
    }
    raw
}

/// 编码 LAPIC 条目。
pub fn ent_lapic(uid: u8, id: u8, enabled: bool) -> Vec<u8> {
    vec![T_LAPIC, 8, uid, id, if enabled { 1 } else { 0 }, 0, 0, 0]
}

/// 编码 IOAPIC 条目。
pub fn ent_ioapic(id: u8, addr: u32, gsi: u32) -> Vec<u8> {
    let mut v = vec![T_IOAPIC, 12, id, 0];
    v.extend_from_slice(&addr.to_le_bytes());
    v.extend_from_slice(&gsi.to_le_bytes());
    v
}

/// 编码 ISO 条目。
pub fn ent_iso(irq: u8, gsi: u32, flags: u16) -> Vec<u8> {
    let mut v = vec![T_ISO, 10, 0, irq];
    v.extend_from_slice(&gsi.to_le_bytes());
    v.extend_from_slice(&flags.to_le_bytes());
    v
}

/// 判据入口：UNX-F0445。
pub fn run_unx0445_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0445_madtirq");
    // 族一：头核。
    let m = mk_madt(&[ent_lapic(0, 0, true), ent_lapic(1, 1, true)]);
    cs.add("双核账", parse_madt(&m).map(|b| b.cpu_total()).unwrap_or(0) == 2, "类型 0 ×2");
    cs.add("非 APIC 签名拒", parse_madt(b"HPETxxxx") == Err(XC_SIG), "签名闸");
    cs.add("短头拒", parse_madt(b"APIC") == Err(XC_HDR_SHORT), "44 不足");
    // 族二：五类分派。
    let m2 = mk_madt(&[
        ent_lapic(0, 0, true),
        ent_ioapic(0, 0xFEC0_0000, 24),
        ent_iso(2, 2, 0xF),       // IRQ2→GSI2 电平低有效
        vec![T_NMI, 6, 1, 0xFF, 0, 0], // NMI 源
        vec![T_X2APIC, 16, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], // 启用
    ]);
    let b2 = parse_madt(&m2);
    cs.add("LAPIC 1", b2.as_ref().map(|b| b.lapics.len()).unwrap_or(9) == 1, "类型 0");
    cs.add("IOAPIC 1 @FEC00000", b2.as_ref().map(|b| b.ioapics.first().map(|x| x.addr == 0xFEC0_0000).unwrap_or(false)).unwrap_or(false), "地址");
    cs.add("GSI 基 24", b2.as_ref().map(|b| b.ioapics.first().map(|x| x.gsi_base == 24).unwrap_or(false)).unwrap_or(false), "域基");
    cs.add("ISO 1 IRQ2", b2.as_ref().map(|b| b.isos.first().map(|x| x.irq_src == 2 && x.gsi == 2).unwrap_or(false)).unwrap_or(false), "覆盖");
    cs.add("ISO 电平低有效", b2.as_ref().map(|b| b.isos.first().map(|x| x.flags == 0xF).unwrap_or(false)).unwrap_or(false), "flags");
    cs.add("NMI 1", b2.as_ref().map(|b| b.nmi_srcs).unwrap_or(9) == 1, "类型 4");
    cs.add("x2APIC 启用 1", b2.as_ref().map(|b| b.x2apics).unwrap_or(9) == 1, "类型 9");
    cs.add("CPU 总 2", b2.as_ref().map(|b| b.cpu_total()).unwrap_or(0) == 2, "1+1");
    // 族三：未知类型跳过。
    let m3 = mk_madt(&[ent_lapic(0, 0, true), vec![0x7F, 6, 1, 2, 3, 4], ent_lapic(1, 1, true)]);
    let b3 = parse_madt(&m3);
    cs.add("未知跳 1", b3.as_ref().map(|b| b.unknown_skipped).unwrap_or(9) == 1, "前向兼容");
    cs.add("跳后继读 2", b3.as_ref().map(|b| b.lapics.len()).unwrap_or(0) == 2, "步进不断");
    // 族四：长度死锁。
    let m4 = mk_madt(&[vec![T_LAPIC, 0, 0, 0]]);
    cs.add("长度 0 立案", parse_madt(&m4) == Err(XC_LEN0), "步进防死");
    // 族五：越表尾。
    let mut m5 = mk_madt(&[ent_lapic(0, 0, true)]);
    m5.push(9); // 悬尾字节
    cs.add("悬尾立案", parse_madt(&m5) == Err(XC_LEN_OOB), "头有尾无");
    // 族六：GSI 重复。
    let m6 = mk_madt(&[ent_iso(2, 2, 0), ent_iso(9, 2, 0)]);
    let b6 = parse_madt(&m6);
    cs.add("GSI 重复立案", b6.and_then(|b| b.gsi_dup()) == Err(XC_GSI_DUP), "双 ISO 撞 GSI");
    let m7 = mk_madt(&[ent_iso(2, 2, 0), ent_iso(9, 9, 0)]);
    let b7 = parse_madt(&m7);
    cs.add("GSI 异过", b7.and_then(|b| b.gsi_dup()).is_ok(), "各归各");
    // 族七：禁用核不计。
    let m8 = mk_madt(&[ent_lapic(0, 0, false), vec![T_X2APIC, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]]);
    let b8 = parse_madt(&m8);
    cs.add("禁用不计 CPU", b8.map(|b| b.cpu_total() == 0).unwrap_or(false), "Flags 位 0");
    // 族八：IOAPIC 多颗。
    let m9 = mk_madt(&[ent_ioapic(0, 0xFEC0_0000, 0), ent_ioapic(1, 0xFEC0_1000, 24)]);
    let b9 = parse_madt(&m9);
    cs.add("双 IOAPIC", b9.as_ref().map(|b| b.ioapics.len()).unwrap_or(0) == 2, "分段 GSI");
    cs.add("GSI 基分段", b9.as_ref().map(|b| b.ioapics[1].gsi_base == 24).unwrap_or(false), "第二颗 24 起");
    // 族九：空条目区。
    let m10 = mk_madt(&[]);
    let b10 = parse_madt(&m10);
    cs.add("空条目区合法", b10.map(|b| b.cpu_total() == 0 && b.unknown_skipped == 0).unwrap_or(false), "全空账");
    cs
}
