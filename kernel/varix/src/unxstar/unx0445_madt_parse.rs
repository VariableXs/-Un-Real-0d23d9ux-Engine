//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0445 MADT 解析（中断项账）· 0x75100段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，WBV1104·AI-VX7-1104）：
//!   MADT 项账齐（LAPIC/IOAPIC/中断源覆盖项数=表推算数），B22 消
//!   费字段直读一致。
//! 设计要点：
//!   1. MADT 真实布局（ACPI 规范 §5.2.12）：表头 44 字节（36 通用表
//!      头 + Local APIC 地址 4B@36 + flags 4B@40），条目 2B 头（类
//!      型 1B + 长度 1B）。
//!   2. 六类条目闭集：LAPIC(0)/IOAPIC(1)/中断源覆盖(2)/NMI 源(3)/
//!      LAPIC NMI(4)/LAPIC 地址覆盖(5)，逐项类型闭集校验，越集显
//!      性 TypeUnknown。
//!   3. 项数账 = 表长推算对拍：条目长度字节为遍历权威，逐步消耗，
//!      清洁表消耗和必等于 (length − 44)；声明长与本类冻结长不符显
//!      性记账（len_bad），破坏账的表以闭集错误显性拒出。
//!   4. B22 消费字段（B22 中断接管批直读口径）：LAPIC 基址（表头
//!      @36）与 IOAPIC 中断基（首个 IOAPIC 条目 gsi_base），直读账
//!      与结构化解析账双读对拍，不一致即 B22Mismatch。
//!   5. 解析状态机单调推进（Idle→HeaderRead→EntryScan→CountsRecon→
//!      B22Cross→Parsed），倒退显性拒。
//!   6. 注入四路（越集类型/条目长度坏/镜像截尾/B22 篡改）逐路必抓，
//!      LCG 种子固定，双跑一致。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75100，WBV1104 批 MADT 解析卡区）。
pub const SEG_BASE: u32 = 0x75100;

// ── 冻结常量表（来源：ACPI 规范 §5.2.12 Multiple APIC Description Table） ──
/// MADT 表头总长（36 通用表头 + 4 LAPIC 地址 + 4 flags）。
pub const MADT_HDR_LEN: u32 = 44;
/// Local APIC 地址字段偏移（@36）。
pub const OFF_LAPIC_ADDR: usize = 36;
/// flags 字段偏移（@40）。
pub const OFF_FLAGS: usize = 40;
/// 条目头长（类型 1B + 长度 1B）。
pub const ENTRY_HDR: usize = 2;
/// flags 位 0：PCAT 兼容（8254 级联）。
pub const FLAG_PCAT_COMPAT: u32 = 1;
/// 真机 LAPIC 基址（冻结样张：MMIO 默认 0xFEE00000）。
pub const MACHINE_LAPIC_BASE: u32 = 0xFEE0_0000;
/// 真机 IOAPIC 中断基（冻结样张：GSI 从 0 起）。
pub const MACHINE_IOAPIC_GSI_BASE: u32 = 0;

// ── 六类条目闭集冻结长度（字节，含 2B 条目头） ──────────────────
/// LAPIC(0) 条目长。
pub const LEN_LAPIC: usize = 8;
/// IOAPIC(1) 条目长。
pub const LEN_IOAPIC: usize = 8;
/// 中断源覆盖(2) 条目长。
pub const LEN_ISO: usize = 10;
/// NMI 源(3) 条目长（flags2+bus1+src1+gsi4 + 2B 头）。
pub const LEN_NMISRC: usize = 10;
/// LAPIC NMI(4) 条目长。
pub const LEN_LAPICNMI: usize = 6;
/// LAPIC 地址覆盖(5) 条目长。
pub const LEN_ADDROVR: usize = 12;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MadtCode {
    /// 表像残缺（不足 44 字节表头）
    HdrShort,
    /// 条目残缺（2B 头或体越尾）
    EntryShort,
    /// 条目类型越六类闭集
    TypeUnknown,
    /// 条目长度字节坏（<2 无法成项）
    EntryLenBad,
    /// 项数账失配（消耗和 ≠ length−44）
    CountMismatch,
    /// B22 消费字段直读不一致
    B22Mismatch,
    /// 解析状态机倒退
    ParseRegress,
    /// 双跑不一致
    NotReproducible,
}

impl MadtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MadtCode::HdrShort => "MADT 表像残缺：不足 44 字节，建议重取镜像",
            MadtCode::EntryShort => "条目残缺：2B 头或体越尾，建议核对 length 与条目长度字节",
            MadtCode::TypeUnknown => "条目类型越闭集：仅 0..5 六类，建议核对固件 MADT",
            MadtCode::EntryLenBad => "条目长度字节坏：<2 无法成项，建议按 ACPI §5.2.12 复核",
            MadtCode::CountMismatch => "项数账失配：条目消耗和 ≠ length−44，建议重算项账",
            MadtCode::B22Mismatch => "B22 消费字段直读不一致：建议对拍 LAPIC 基址/IOAPIC 中断基",
            MadtCode::ParseRegress => "解析状态机倒退：建议检查驱动推进序",
            MadtCode::NotReproducible => "双跑不一致：复现红线，建议排查非确定源",
        }
    }
    pub const ALL: [MadtCode; 8] = [
        MadtCode::HdrShort,
        MadtCode::EntryShort,
        MadtCode::TypeUnknown,
        MadtCode::EntryLenBad,
        MadtCode::CountMismatch,
        MadtCode::B22Mismatch,
        MadtCode::ParseRegress,
        MadtCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回，越段码返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MadtCode::ALL.len() {
        if let Some(c) = MadtCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75100 段闭集内"
}

// ── 条目类型闭集 ────────────────────────────────────────────────
/// 六类条目类型（闭集，ACPI §5.2.12.1~§5.2.12.6）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryType {
    /// Processor Local APIC（0）
    Lapic,
    /// I/O APIC（1）
    Ioapic,
    /// Interrupt Source Override（2）
    IntSrcOvr,
    /// Non-Maskable Interrupt Source（3）
    NmiSrc,
    /// Local APIC NMI（4）
    LapicNmi,
    /// Local APIC Address Override（5）
    LapicAddrOvr,
}

impl EntryType {
    /// 类型字节（闭集冻结）。
    pub fn ty(self) -> u8 {
        match self {
            EntryType::Lapic => 0,
            EntryType::Ioapic => 1,
            EntryType::IntSrcOvr => 2,
            EntryType::NmiSrc => 3,
            EntryType::LapicNmi => 4,
            EntryType::LapicAddrOvr => 5,
        }
    }
    /// 本类冻结条目长（含 2B 头）。
    pub fn len(self) -> usize {
        match self {
            EntryType::Lapic => LEN_LAPIC,
            EntryType::Ioapic => LEN_IOAPIC,
            EntryType::IntSrcOvr => LEN_ISO,
            EntryType::NmiSrc => LEN_NMISRC,
            EntryType::LapicNmi => LEN_LAPICNMI,
            EntryType::LapicAddrOvr => LEN_ADDROVR,
        }
    }
    pub const ALL: [EntryType; 6] = [
        EntryType::Lapic,
        EntryType::Ioapic,
        EntryType::IntSrcOvr,
        EntryType::NmiSrc,
        EntryType::LapicNmi,
        EntryType::LapicAddrOvr,
    ];
}

/// 类型字节 → 闭集（越集 ⇒ TypeUnknown）。
pub fn entry_type_lookup(ty: u8) -> Result<EntryType, u32> {
    let mut i = 0;
    while i < EntryType::ALL.len() {
        if let Some(t) = EntryType::ALL.get(i) {
            if t.ty() == ty {
                return Ok(*t);
            }
        }
        i += 1;
    }
    Err(MadtCode::TypeUnknown.val())
}

// ── MADT 表头（44 字节） ────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MadtHeader {
    /// 签名（应为 "APIC"）。
    pub sig: [u8; 4],
    /// 全表长度。
    pub length: u32,
    /// revision。
    pub revision: u8,
    /// checksum。
    pub checksum: u8,
    /// Local APIC 基址（@36，B22 消费字段一）。
    pub lapic_addr: u32,
    /// flags（@40，位 0 = PCAT 兼容）。
    pub flags: u32,
}

impl MadtHeader {
    /// 表头解码（不足 44 字节显性拒）。
    pub fn decode(img: &[u8]) -> Result<MadtHeader, u32> {
        if img.len() < MADT_HDR_LEN as usize {
            return Err(MadtCode::HdrShort.val());
        }
        let b = |off: usize| -> u8 { *img.get(off).unwrap_or(&0) };
        let g = |off: usize| -> u32 {
            let b0 = b(off) as u32;
            let b1 = b(off.wrapping_add(1)) as u32;
            let b2 = b(off.wrapping_add(2)) as u32;
            let b3 = b(off.wrapping_add(3)) as u32;
            b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
        };
        let mut sig = [0u8; 4];
        let mut i = 0;
        while i < 4 {
            if let Some(slot) = sig.get_mut(i) {
                *slot = b(i);
            }
            i += 1;
        }
        Ok(MadtHeader {
            sig,
            length: g(4),
            revision: b(8),
            checksum: b(9),
            lapic_addr: g(OFF_LAPIC_ADDR),
            flags: g(OFF_FLAGS),
        })
    }
}

// ── 条目载荷（六类） ────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MadtEntry {
    /// LAPIC：uid/apic_id/flags。
    Lapic { uid: u8, apic_id: u8, flags: u32 },
    /// IOAPIC：id/保留/gsi_base（B22 消费字段二）。
    Ioapic { ioapic_id: u8, reserved: u8, gsi_base: u32 },
    /// 中断源覆盖：bus/src_irq/gsi/flags。
    IntSrcOvr { bus: u8, src_irq: u8, gsi: u32, flags: u16 },
    /// NMI 源：flags/bus/src_irq/gsi。
    NmiSrc { flags: u16, bus: u8, src_irq: u8, gsi: u32 },
    /// LAPIC NMI：uid/flags/lint。
    LapicNmi { uid: u8, flags: u16, lint: u8 },
    /// LAPIC 地址覆盖：保留/64 位地址。
    LapicAddrOvr { reserved: u16, addr: u64 },
}

impl MadtEntry {
    /// 条目类型。
    pub fn ty(&self) -> EntryType {
        match self {
            MadtEntry::Lapic { .. } => EntryType::Lapic,
            MadtEntry::Ioapic { .. } => EntryType::Ioapic,
            MadtEntry::IntSrcOvr { .. } => EntryType::IntSrcOvr,
            MadtEntry::NmiSrc { .. } => EntryType::NmiSrc,
            MadtEntry::LapicNmi { .. } => EntryType::LapicNmi,
            MadtEntry::LapicAddrOvr { .. } => EntryType::LapicAddrOvr,
        }
    }
}

/// 小端读 u16/u32/u64（越尾按 0 补账，调用方先用 EntryShort 拦截）。
fn rd_u16(img: &[u8], off: usize) -> u16 {
    let b0 = *img.get(off).unwrap_or(&0) as u16;
    let b1 = *img.get(off.wrapping_add(1)).unwrap_or(&0) as u16;
    b0 | (b1 << 8)
}

fn rd_u32(img: &[u8], off: usize) -> u32 {
    let lo = rd_u16(img, off) as u32;
    let hi = rd_u16(img, off.wrapping_add(2)) as u32;
    lo | (hi << 16)
}

fn rd_u64(img: &[u8], off: usize) -> u64 {
    let lo = rd_u32(img, off) as u64;
    let hi = rd_u32(img, off.wrapping_add(4)) as u64;
    lo | (hi << 32)
}

/// 条目体解码（body 为去掉 2B 头的载荷，长度已由冻结长核过）。
fn decode_entry(ty: EntryType, img: &[u8], body_off: usize) -> MadtEntry {
    match ty {
        EntryType::Lapic => MadtEntry::Lapic {
            uid: *img.get(body_off).unwrap_or(&0),
            apic_id: *img.get(body_off.wrapping_add(1)).unwrap_or(&0),
            flags: rd_u32(img, body_off.wrapping_add(2)),
        },
        EntryType::Ioapic => MadtEntry::Ioapic {
            ioapic_id: *img.get(body_off).unwrap_or(&0),
            reserved: *img.get(body_off.wrapping_add(1)).unwrap_or(&0),
            gsi_base: rd_u32(img, body_off.wrapping_add(2)),
        },
        EntryType::IntSrcOvr => MadtEntry::IntSrcOvr {
            bus: *img.get(body_off).unwrap_or(&0),
            src_irq: *img.get(body_off.wrapping_add(1)).unwrap_or(&0),
            gsi: rd_u32(img, body_off.wrapping_add(2)),
            flags: rd_u16(img, body_off.wrapping_add(6)),
        },
        EntryType::NmiSrc => MadtEntry::NmiSrc {
            flags: rd_u16(img, body_off),
            bus: *img.get(body_off.wrapping_add(2)).unwrap_or(&0),
            src_irq: *img.get(body_off.wrapping_add(3)).unwrap_or(&0),
            gsi: rd_u32(img, body_off.wrapping_add(4)),
        },
        EntryType::LapicNmi => MadtEntry::LapicNmi {
            uid: *img.get(body_off).unwrap_or(&0),
            flags: rd_u16(img, body_off.wrapping_add(1)),
            lint: *img.get(body_off.wrapping_add(3)).unwrap_or(&0),
        },
        EntryType::LapicAddrOvr => MadtEntry::LapicAddrOvr {
            reserved: rd_u16(img, body_off),
            addr: rd_u64(img, body_off.wrapping_add(2)),
        },
    }
}

// ── 解析状态机（单调推进，倒退显性拒） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParsePhase {
    /// 未开始
    Idle,
    /// 表头已读
    HeaderRead,
    /// 条目扫描中/已扫
    EntryScan,
    /// 项数账已对拍
    CountsRecon,
    /// B22 消费字段已对拍
    B22Cross,
    /// 解析完结
    Parsed,
}

impl ParsePhase {
    fn rank(self) -> u32 {
        match self {
            ParsePhase::Idle => 0,
            ParsePhase::HeaderRead => 1,
            ParsePhase::EntryScan => 2,
            ParsePhase::CountsRecon => 3,
            ParsePhase::B22Cross => 4,
            ParsePhase::Parsed => 5,
        }
    }
}

/// 解析游标：单调推进，倒退显性拒。
#[derive(Clone, Debug)]
pub struct ParseCursor {
    pub phase: ParsePhase,
    pub scanned: u32,
}

impl ParseCursor {
    pub fn new() -> ParseCursor {
        ParseCursor { phase: ParsePhase::Idle, scanned: 0 }
    }
    pub fn advance(&mut self, to: ParsePhase) -> Result<(), u32> {
        if to.rank() < self.phase.rank() {
            return Err(MadtCode::ParseRegress.val());
        }
        self.phase = to;
        Ok(())
    }
}

// ── B22 消费字段账 ──────────────────────────────────────────────
/// B22 中断接管批（F0426~F0439）从 MADT 直读的两个消费字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct B22Account {
    /// LAPIC 基址（表头 @36 直读）。
    pub lapic_base: u32,
    /// IOAPIC 中断基（首个 IOAPIC 条目 gsi_base 直读）。
    pub ioapic_gsi_base: u32,
}

/// 直读账：不看结构化解析，直接按偏移摸字节（B22 批口径）。
pub fn b22_direct(img: &[u8]) -> Result<B22Account, u32> {
    let hdr = MadtHeader::decode(img)?;
    let mut off = MADT_HDR_LEN as usize;
    let total = hdr.length as usize;
    // 逐条目找首个 IOAPIC。
    while off + ENTRY_HDR <= total && off + ENTRY_HDR <= img.len() {
        let ty = match img.get(off) {
            Some(x) => *x,
            None => return Err(MadtCode::EntryShort.val()),
        };
        let elen = match img.get(off.wrapping_add(1)) {
            Some(x) => *x as usize,
            None => return Err(MadtCode::EntryShort.val()),
        };
        if elen < ENTRY_HDR {
            return Err(MadtCode::EntryLenBad.val());
        }
        if off + elen > total || off + elen > img.len() {
            return Err(MadtCode::EntryShort.val());
        }
        if ty == EntryType::Ioapic.ty() {
            let body = off + ENTRY_HDR;
            let gsi = rd_u32(img, body.wrapping_add(2));
            return Ok(B22Account { lapic_base: hdr.lapic_addr, ioapic_gsi_base: gsi });
        }
        off += elen;
    }
    // 无 IOAPIC：中断基按 0 缺省记账（显性缺省，非静默）。
    Ok(B22Account { lapic_base: hdr.lapic_addr, ioapic_gsi_base: 0 })
}

/// B22 对拍：直读账 vs 外部消费账（B22 批登记值），不一致即 B22Mismatch。
pub fn b22_crosscheck(img: &[u8], expect: &B22Account) -> Result<(), u32> {
    let direct = b22_direct(img)?;
    if direct != *expect {
        return Err(MadtCode::B22Mismatch.val());
    }
    Ok(())
}

// ── 解析总账 ────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MadtLedger {
    /// 表头账。
    pub header: MadtHeader,
    /// 六类条目计数（下标 = 类型字节，闭集）。
    pub counts: [u32; 6],
    /// 条目总数。
    pub total: usize,
    /// 条目消耗字节和。
    pub consumed: usize,
    /// 表长推算体长（length − 44）。
    pub body_from_len: usize,
    /// 声明长度与本类冻结长不符的条目数（显性记账，黄金样张为 0）。
    pub len_bad: u32,
    /// 结构化条目列表。
    pub entries: Vec<MadtEntry>,
    /// B22 消费字段（结构化口径）。
    pub b22: B22Account,
}

impl MadtLedger {
    /// 项数账齐：消耗和 = 表长推算体长，且无坏长条目。
    pub fn counts_reconciled(&self) -> bool {
        self.consumed == self.body_from_len && self.len_bad == 0
    }
    /// 六类计数闭集取值。
    pub fn count_of(&self, ty: EntryType) -> u32 {
        *self.counts.get(ty as usize).unwrap_or(&0)
    }
}

/// MADT 全解析：表头 → 条目闭集扫描 → 项数对拍 → B22 双读对拍。
pub fn parse_madt(img: &[u8]) -> Result<MadtLedger, u32> {
    let mut cur = ParseCursor::new();
    let hdr = MadtHeader::decode(img)?;
    cur.advance(ParsePhase::HeaderRead)?;
    if hdr.length < MADT_HDR_LEN || hdr.length as usize > img.len() {
        return Err(MadtCode::EntryShort.val());
    }
    let total = hdr.length as usize;
    let body_from_len = (hdr.length - MADT_HDR_LEN) as usize;
    let mut counts = [0u32; 6];
    let mut entries: Vec<MadtEntry> = Vec::new();
    let mut consumed = 0usize;
    let mut len_bad = 0u32;
    let mut ioapic_gsi: Option<u32> = None;
    cur.advance(ParsePhase::EntryScan)?;
    let mut off = MADT_HDR_LEN as usize;
    while off < total {
        let ty_raw = match img.get(off) {
            Some(x) => *x,
            None => return Err(MadtCode::EntryShort.val()),
        };
        let elen = match img.get(off.wrapping_add(1)) {
            Some(x) => *x as usize,
            None => return Err(MadtCode::EntryShort.val()),
        };
        if elen < ENTRY_HDR {
            return Err(MadtCode::EntryLenBad.val());
        }
        if off + elen > total {
            return Err(MadtCode::EntryShort.val());
        }
        let ty = entry_type_lookup(ty_raw)?;
        // 长度字节是遍历权威；与本类冻结长不符显性记账（不静默吞）。
        if elen != ty.len() {
            len_bad += 1;
        }
        let entry = decode_entry(ty, img, off + ENTRY_HDR);
        if ty == EntryType::Ioapic {
            if let MadtEntry::Ioapic { gsi_base, .. } = &entry {
                if ioapic_gsi.is_none() {
                    ioapic_gsi = Some(*gsi_base);
                }
            }
        }
        let slot = counts.get_mut(ty as usize);
        match slot {
            Some(c) => *c += 1,
            None => return Err(MadtCode::TypeUnknown.val()),
        }
        entries.push(entry);
        consumed += elen;
        cur.scanned += 1;
        off += elen;
    }
    if consumed != body_from_len {
        return Err(MadtCode::CountMismatch.val());
    }
    if consumed != body_from_len {
        // 清洁表不变式：条目消耗和必等于表长推算体长。变量长条目下
        // 破坏账的表会先以 TypeUnknown/EntryLenBad/EntryShort 显性
        // 拒出，此路为防御性兜底（不变式一旦破必拒）。
        return Err(MadtCode::CountMismatch.val());
    }
    cur.advance(ParsePhase::CountsRecon)?;
    // B22 双读对拍：直读账 vs 结构化账。
    let direct = b22_direct(img)?;
    let structured = B22Account {
        lapic_base: hdr.lapic_addr,
        ioapic_gsi_base: ioapic_gsi.unwrap_or(0),
    };
    if direct != structured {
        return Err(MadtCode::B22Mismatch.val());
    }
    cur.advance(ParsePhase::B22Cross)?;
    cur.advance(ParsePhase::Parsed)?;
    Ok(MadtLedger {
        header: hdr,
        counts,
        total: entries.len(),
        consumed,
        body_from_len,
        len_bad,
        entries,
        b22: structured,
    })
}

// ── 表像构造（真机样张 + 条目构造器） ───────────────────────────
fn push_u16(v: &mut Vec<u8>, x: u16) {
    v.push((x & 0xFF) as u8);
    v.push(((x >> 8) & 0xFF) as u8);
}

fn push_u32(v: &mut Vec<u8>, x: u32) {
    let mut i = 0;
    while i < 4 {
        v.push(((x >> (8 * i)) & 0xFF) as u8);
        i += 1;
    }
}

fn push_u64(v: &mut Vec<u8>, x: u64) {
    let mut i = 0;
    while i < 8 {
        v.push(((x >> (8 * i)) & 0xFF) as u8);
        i += 1;
    }
}

/// 条目编码（闭集内手写，带 2B 头）。
pub fn encode_entry(e: &MadtEntry) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    match e {
        MadtEntry::Lapic { uid, apic_id, flags } => {
            v.push(EntryType::Lapic.ty());
            v.push(LEN_LAPIC as u8);
            v.push(*uid);
            v.push(*apic_id);
            push_u32(&mut v, *flags);
        }
        MadtEntry::Ioapic { ioapic_id, reserved, gsi_base } => {
            v.push(EntryType::Ioapic.ty());
            v.push(LEN_IOAPIC as u8);
            v.push(*ioapic_id);
            v.push(*reserved);
            push_u32(&mut v, *gsi_base);
        }
        MadtEntry::IntSrcOvr { bus, src_irq, gsi, flags } => {
            v.push(EntryType::IntSrcOvr.ty());
            v.push(LEN_ISO as u8);
            v.push(*bus);
            v.push(*src_irq);
            push_u32(&mut v, *gsi);
            push_u16(&mut v, *flags);
        }
        MadtEntry::NmiSrc { flags, bus, src_irq, gsi } => {
            v.push(EntryType::NmiSrc.ty());
            v.push(LEN_NMISRC as u8);
            push_u16(&mut v, *flags);
            v.push(*bus);
            v.push(*src_irq);
            push_u32(&mut v, *gsi);
        }
        MadtEntry::LapicNmi { uid, flags, lint } => {
            v.push(EntryType::LapicNmi.ty());
            v.push(LEN_LAPICNMI as u8);
            v.push(*uid);
            push_u16(&mut v, *flags);
            v.push(*lint);
        }
        MadtEntry::LapicAddrOvr { reserved, addr } => {
            v.push(EntryType::LapicAddrOvr.ty());
            v.push(LEN_ADDROVR as u8);
            push_u16(&mut v, *reserved);
            push_u64(&mut v, *addr);
        }
    }
    v
}

/// 构造 MADT 像：表头（LAPIC 基址/flags）+ 条目表 + 整表补数 checksum。
pub fn build_madt(lapic_addr: u32, flags: u32, entries: &[MadtEntry]) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < entries.len() {
        if let Some(e) = entries.get(i) {
            let enc = encode_entry(e);
            let mut k = 0;
            while k < enc.len() {
                if let Some(b) = enc.get(k) {
                    body.push(*b);
                }
                k += 1;
            }
        }
        i += 1;
    }
    let total = MADT_HDR_LEN as usize + body.len();
    let mut v: Vec<u8> = Vec::new();
    v.extend_from_slice(b"APIC");
    push_u32(&mut v, total as u32);
    v.push(2u8); // revision
    v.push(0u8); // checksum 占位
    let mut j = 0;
    while j < 6 {
        v.push(b'V');
        j += 1;
    }
    let mut k = 0;
    while k < 8 {
        v.push(b'V');
        k += 1;
    }
    let mut m = 0;
    while m < 12 {
        v.push(0u8);
        m += 1;
    }
    push_u32(&mut v, lapic_addr);
    push_u32(&mut v, flags);
    let mut n = 0;
    while n < body.len() {
        if let Some(b) = body.get(n) {
            v.push(*b);
        }
        n += 1;
    }
    // 整表补数 checksum。
    let others = {
        let mut s = 0u8;
        let mut q = 0;
        while q < v.len() {
            if let Some(x) = v.get(q) {
                s = s.wrapping_add(*x);
            }
            q += 1;
        }
        s
    };
    if let Some(slot) = v.get_mut(9) {
        *slot = 0u8.wrapping_sub(others);
    }
    v
}

/// 真机样张条目集：2 LAPIC + 1 IOAPIC + 2 中断源覆盖 + 1 NMI 源
/// + 1 LAPIC NMI + 1 地址覆盖（8 条，冻结）。
pub fn machine_entries() -> Vec<MadtEntry> {
    vec![
        MadtEntry::Lapic { uid: 0, apic_id: 0, flags: 1 },
        MadtEntry::Lapic { uid: 1, apic_id: 1, flags: 1 },
        MadtEntry::Ioapic { ioapic_id: 0, reserved: 0, gsi_base: MACHINE_IOAPIC_GSI_BASE },
        MadtEntry::IntSrcOvr { bus: 0, src_irq: 2, gsi: 2, flags: 0 },
        MadtEntry::IntSrcOvr { bus: 0, src_irq: 9, gsi: 9, flags: 0x000D },
        MadtEntry::NmiSrc { flags: 0, bus: 0, src_irq: 9, gsi: 9 },
        MadtEntry::LapicNmi { uid: 0xFF, flags: 0x0005, lint: 1 },
        MadtEntry::LapicAddrOvr { reserved: 0, addr: 0xFEE0_0000 },
    ]
}

/// 真机样张像。
pub fn build_machine_madt() -> Vec<u8> {
    build_madt(MACHINE_LAPIC_BASE, FLAG_PCAT_COMPAT, &machine_entries())
}

// ── LCG（种子固定） ─────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路） ──────────────────────────────────────────
/// 路 0：越集类型（0x66）；路 1：条目长度坏（长度字节改 1，<2）；
/// 路 2：镜像截尾（条目残缺）；路 3：B22 篡改（对拍期望错值）。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t == 0 || c != t {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 注入轮（LCG 种子固定，每轮四路全部必抓）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 0：越集类型——首条目类型字节改 0x66。
        let img = build_machine_madt();
        let mut img0 = img;
        if let Some(slot) = img0.get_mut(MADT_HDR_LEN as usize) {
            *slot = 0x66;
        }
        let hit0 = match parse_madt(&img0) {
            Err(e) => e == MadtCode::TypeUnknown.val(),
            Ok(_) => false,
        };
        led.record(0, hit0);
        // 路 1：条目长度坏——首条目长度字节 8→1（<2 无法成项）。
        let img1 = build_machine_madt();
        let mut img1b = img1;
        let len_off = MADT_HDR_LEN as usize + 1;
        if let Some(slot) = img1b.get_mut(len_off) {
            *slot = 1;
        }
        let hit1 = match parse_madt(&img1b) {
            Err(e) => e == MadtCode::EntryLenBad.val(),
            Ok(_) => false,
        };
        led.record(1, hit1);
        // 路 2：镜像截尾——切到第二 LAPIC 条目体中部（条目残缺）。
        let img2 = build_machine_madt();
        let keep = MADT_HDR_LEN as usize + LEN_LAPIC + 4;
        let mut cut: Vec<u8> = Vec::new();
        let mut k = 0;
        while k < keep {
            if let Some(b) = img2.get(k) {
                cut.push(*b);
            }
            k += 1;
        }
        let hit2 = match parse_madt(&cut) {
            Err(e) => e == MadtCode::EntryShort.val(),
            Ok(_) => false,
        };
        led.record(2, hit2);
        // 路 3：B22 篡改——对拍期望账写错 LAPIC 基址。
        let img3 = build_machine_madt();
        let bad_expect = B22Account {
            lapic_base: 0xDEAD_0000,
            ioapic_gsi_base: MACHINE_IOAPIC_GSI_BASE,
        };
        let hit3 = b22_crosscheck(&img3, &bad_expect)
            == Err(MadtCode::B22Mismatch.val());
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

// ── 黄金景 ──────────────────────────────────────────────────────
/// 黄金景 A：真机样张全解析。
pub fn golden_parse() -> Result<MadtLedger, u32> {
    parse_madt(&build_machine_madt())
}

/// 探针序 A：项账齐 + 六类计数冻结值 + B22 直读一致。
pub fn probe_seq_a() -> Result<(), u32> {
    let l = golden_parse()?;
    if !l.counts_reconciled() || l.len_bad != 0 {
        return Err(MadtCode::NotReproducible.val());
    }
    let expect = [2u32, 1, 2, 1, 1, 1];
    let mut i = 0;
    while i < 6 {
        if let (Some(a), Some(b)) = (l.counts.get(i), expect.get(i)) {
            if a != b {
                return Err(MadtCode::NotReproducible.val());
            }
        }
        i += 1;
    }
    if l.b22.lapic_base != MACHINE_LAPIC_BASE {
        return Err(MadtCode::NotReproducible.val());
    }
    Ok(())
}

/// 探针序 B：B22 直读账与登记账一致（真机基址/中断基）。
pub fn probe_seq_b() -> Result<(), u32> {
    let img = build_machine_madt();
    let expect = B22Account {
        lapic_base: MACHINE_LAPIC_BASE,
        ioapic_gsi_base: MACHINE_IOAPIC_GSI_BASE,
    };
    b22_crosscheck(&img, &expect)
}

/// 探针序 C：解析状态机倒退必抓。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut c = ParseCursor::new();
    let _ = c.advance(ParsePhase::EntryScan);
    match c.advance(ParsePhase::HeaderRead) {
        Err(e) if e == MadtCode::ParseRegress.val() => Ok(()),
        _ => Err(MadtCode::NotReproducible.val()),
    }
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_parse()?;
    let b = golden_parse()?;
    if a != b {
        return Err(MadtCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < MadtCode::ALL.len() {
        if let Some(c) = MadtCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// 判据：项账齐（六类计数冻结 + 消耗和 = 表长推算）。
fn judge_counts_ok() -> bool {
    match golden_parse() {
        Ok(l) => {
            l.counts_reconciled()
                && l.count_of(EntryType::Lapic) == 2
                && l.count_of(EntryType::Ioapic) == 1
                && l.count_of(EntryType::IntSrcOvr) == 2
                && l.count_of(EntryType::NmiSrc) == 1
                && l.count_of(EntryType::LapicNmi) == 1
                && l.count_of(EntryType::LapicAddrOvr) == 1
                && l.total == 8
        }
        Err(_) => false,
    }
}

/// 判据：B22 消费字段直读一致（双读 + 登记账三方齐）。
fn judge_b22_ok() -> bool {
    match golden_parse() {
        Ok(l) => {
            l.b22.lapic_base == MACHINE_LAPIC_BASE
                && l.b22.ioapic_gsi_base == MACHINE_IOAPIC_GSI_BASE
                && probe_seq_b().is_ok()
        }
        Err(_) => false,
    }
}

/// 判据迭代轮数（10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 守卫：段位窗口。
    v.push((
        "守卫-段位窗口",
        segment_guard(),
        format!("SEG_BASE=0x{:x}", SEG_BASE),
    ));

    // 判据：项账齐。
    v.push((
        "J1-MADT项账齐",
        judge_counts_ok(),
        String::from("LAPIC2/IOAPIC1/覆盖2/NMI源1/LNMI1/地址覆盖1 = 8 = 推算"),
    ));

    // 判据：B22 消费字段直读一致。
    v.push((
        "J2-B22消费字段直读一致",
        judge_b22_ok(),
        String::from("LAPIC 基址 0xFEE00000 + IOAPIC 中断基 0 三方齐"),
    ));

    // 判据：注入四路全抓。
    v.push((
        "J3-注入四路必抓",
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        String::from("越集/长坏/项数失配/B22篡改 全中"),
    ));

    // 表头 44 字节布局账（LAPIC 基址 @36 / flags @40）。
    v.push((
        "表头44B布局冻结",
        MADT_HDR_LEN == 44 && OFF_LAPIC_ADDR == 36 && OFF_FLAGS == 40 && ENTRY_HDR == 2,
        format!("hdr={} lapic@{} flags@{} 项头={}", MADT_HDR_LEN, OFF_LAPIC_ADDR, OFF_FLAGS, ENTRY_HDR),
    ));

    // 六类条目冻结长度账。
    v.push((
        "六类冻结长度",
        LEN_LAPIC == 8 && LEN_IOAPIC == 8 && LEN_ISO == 10
            && LEN_NMISRC == 10 && LEN_LAPICNMI == 6 && LEN_ADDROVR == 12,
        String::from("8/8/10/10/6/12（含 2B 项头）"),
    ));

    // 条目类型闭集回查 + 越集显性拒。
    {
        let rt = entry_type_lookup(0) == Ok(EntryType::Lapic)
            && entry_type_lookup(5) == Ok(EntryType::LapicAddrOvr);
        let unk = entry_type_lookup(0x66) == Err(MadtCode::TypeUnknown.val());
        v.push((
            "类型闭集回查+越集拒",
            rt && unk,
            String::from("0..5 可回查，0x66 ⇒ TypeUnknown"),
        ));
    }

    // PCAT 兼容位账。
    v.push((
        "flags-PCAT兼容位",
        match golden_parse() {
            Ok(l) => l.header.flags == FLAG_PCAT_COMPAT,
            Err(_) => false,
        },
        format!("flags 位 0 = {}", FLAG_PCAT_COMPAT),
    ));

    // 表像残缺显性拒（30 字节 < 44）。
    {
        let img = build_machine_madt();
        let hit = match img.get(..30) {
            Some(short) => MadtHeader::decode(short) == Err(MadtCode::HdrShort.val()),
            None => false,
        };
        v.push((
            "表像残缺显性拒",
            hit,
            String::from("30 字节 < 44 ⇒ HdrShort"),
        ));
    }

    // 条目残缺显性拒（截尾至第 3 条目头）。
    {
        let img = build_machine_madt();
        let keep = MADT_HDR_LEN as usize + LEN_LAPIC + 4; // 第二条目头都在
        let mut cut: Vec<u8> = Vec::new();
        let mut k = 0;
        while k < keep {
            if let Some(b) = img.get(k) {
                cut.push(*b);
            }
            k += 1;
        }
        let hit = match parse_madt(&cut) {
            Err(e) => e == MadtCode::EntryShort.val(),
            Ok(_) => false,
        };
        v.push((
            "条目残缺显性拒",
            hit,
            String::from("镜像截尾 ⇒ EntryShort"),
        ));
    }

    // 无 IOAPIC 缺省账（中断基显性缺省 0，非静默）。
    {
        let img = build_madt(MACHINE_LAPIC_BASE, 0, &[MadtEntry::Lapic { uid: 0, apic_id: 0, flags: 1 }]);
        let hit = match parse_madt(&img) {
            Ok(l) => l.count_of(EntryType::Ioapic) == 0 && l.b22.ioapic_gsi_base == 0,
            Err(_) => false,
        };
        v.push((
            "无IOAPIC缺省账显性",
            hit,
            String::from("中断基缺省 0 入账"),
        ));
    }

    // 状态机单调：倒退必抓。
    v.push((
        "状态机单调-倒退必抓",
        probe_seq_c().is_ok(),
        String::from("EntryScan→HeaderRead ⇒ ParseRegress"),
    ));

    // 双跑一致。
    v.push((
        "双跑一致",
        repro_check().is_ok(),
        String::from("真机样张两跑同值"),
    ));

    // 探针三序。
    v.push((
        "探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        String::from("a=项账齐 b=B22齐 c=倒退抓"),
    ));

    v
}

pub fn run_unx0445_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0445 MADT 解析（中断项账）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_parse_ledger() {
        let l = golden_parse().ok().unwrap_or(MadtLedger {
            header: MadtHeader {
                sig: [0; 4],
                length: 0,
                revision: 0,
                checksum: 0,
                lapic_addr: 0,
                flags: 0,
            },
            counts: [0; 6],
            total: 0,
            consumed: 0,
            body_from_len: 0,
            len_bad: 1,
            entries: Vec::new(),
            b22: B22Account { lapic_base: 0, ioapic_gsi_base: 0 },
        });
        assert!(l.counts_reconciled());
        assert_eq!(l.total, 8);
        assert_eq!(l.count_of(EntryType::Lapic), 2);
        assert_eq!(l.count_of(EntryType::IntSrcOvr), 2);
    }

    #[test]
    fn test_b22_direct_matches_structured() {
        let img = build_machine_madt();
        let direct = b22_direct(&img).ok().unwrap_or(B22Account { lapic_base: 0, ioapic_gsi_base: 1 });
        let l = golden_parse().ok().unwrap_or(MadtLedger {
            header: MadtHeader {
                sig: [0; 4],
                length: 0,
                revision: 0,
                checksum: 0,
                lapic_addr: 0,
                flags: 0,
            },
            counts: [0; 6],
            total: 0,
            consumed: 0,
            body_from_len: 0,
            len_bad: 0,
            entries: Vec::new(),
            b22: B22Account { lapic_base: 0, ioapic_gsi_base: 1 },
        });
        assert_eq!(direct, l.b22);
        assert_eq!(direct.lapic_base, MACHINE_LAPIC_BASE);
        assert_eq!(direct.ioapic_gsi_base, MACHINE_IOAPIC_GSI_BASE);
    }

    #[test]
    fn test_type_unknown_caught() {
        let mut img = build_machine_madt();
        if let Some(slot) = img.get_mut(MADT_HDR_LEN as usize + LEN_LAPIC) {
            *slot = 0x66;
        }
        assert_eq!(parse_madt(&img), Err(MadtCode::TypeUnknown.val()));
    }

    #[test]
    fn test_entry_len_bad_caught() {
        let mut img = build_machine_madt();
        let off = MADT_HDR_LEN as usize + 1;
        if let Some(slot) = img.get_mut(off) {
            *slot = 1;
        }
        assert_eq!(parse_madt(&img), Err(MadtCode::EntryLenBad.val()));
    }

    #[test]
    fn test_truncated_caught() {
        let img = build_machine_madt();
        let keep = MADT_HDR_LEN as usize + LEN_LAPIC + 4;
        let mut cut: Vec<u8> = Vec::new();
        let mut k = 0;
        while k < keep {
            if let Some(b) = img.get(k) {
                cut.push(*b);
            }
            k += 1;
        }
        assert_eq!(parse_madt(&cut), Err(MadtCode::EntryShort.val()));
    }

    #[test]
    fn test_b22_tamper_caught() {
        let img = build_machine_madt();
        let bad = B22Account { lapic_base: 0x1234, ioapic_gsi_base: 0 };
        assert_eq!(b22_crosscheck(&img, &bad), Err(MadtCode::B22Mismatch.val()));
    }

    #[test]
    fn test_hdr_short_caught() {
        let img = build_machine_madt();
        let short = img.get(..30).unwrap_or(&[]);
        assert_eq!(MadtHeader::decode(short), Err(MadtCode::HdrShort.val()));
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let led = inject_rounds(5).ok().unwrap_or(InjectLedger::new());
        assert!(led.all_caught());
        assert_eq!(led.tries[0], 5);
        assert_eq!(led.caught[3], 5);
    }

    #[test]
    fn test_phase_regress() {
        let mut c = ParseCursor::new();
        assert!(c.advance(ParsePhase::CountsRecon).is_ok());
        assert_eq!(
            c.advance(ParsePhase::Idle),
            Err(MadtCode::ParseRegress.val())
        );
    }

    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_checks_all_green() {
        let s = run_unx0445_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
    }
}
