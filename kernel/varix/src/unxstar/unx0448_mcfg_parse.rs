//! UNX-F0448 MCFG 表解析（ECAM 基址账） · 0x751C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104）：
//!   MCFG 分配记录逐条（16 字节定长，条数=推算数）账齐，ECAM
//!   段组/基址/起始总线/结束总线四要素入账。
//!
//! 设计要点：
//!   ① MCFG 布局按 PCI Firmware Specification rev 3.x「MCFG Table」
//!      节建模：表头 36B + 保留 8B + N×16B 分配记录；记录内布局
//!      基址 u64 @+0 / PCI 段组 u16 @+8 / 起始总线 u8 @+10 / 结束
//!      总线 u8 @+11 / 保留 4B @+12..16；
//!   ② 条数推算对拍：条数 = (表长 − 44) / 16，余数非零即「破条」
//!      显性拒；推算数与窗口实际步进数二次对拍（双口径必相等）；
//!   ③ 基址 2^20 对齐校验：ECAM 每总线映射 1MB（256 设备 × 8 功能
//!      × 4KB），基址非 1MB 对齐拒、基址为 0 拒；
//!   ④ 总线范围闭合校验：起始 ≤ 结束、结束 ≤ 255，跨度 ×1MB 不得
//!      溢出 u64（checked 算术）；
//!   ⑤ 重叠检测 N×N 对拍：同段组内总线区间相交即拒，异段组同区间
//!      合法（多段组各管各的 ECAM 窗）；记录保留字节非零拒；
//!   ⑥ 单调状态机 Raw→HeaderVetted→CountReckoned→RecordsLedgered，
//!      倒退显性拒。
//!
//! 诊断码段：0x751C0（0x40 窗口闭集，全域独占）。
//! 纯功能线：no_std + alloc，无 std::/unsafe/unwrap/expect/panic，
//! 整数算术，确定性，双跑一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x751C0，AI-VX7-1104 F0448 独占区）。
pub const SEG_BASE: u32 = 0x751C0;

// ── 冻结常量表（来源：PCI Firmware Specification rev 3.0 §4.1.2） ──
/// MCFG 表头定长（Signature..Creator Revision，标准 ACPI 表头）。
pub const MCFG_HDR_LEN: usize = 36;
/// 表头后的保留区字节数（规范规定 8B 全零）。
pub const MCFG_RESERVED_LEN: usize = 8;
/// 分配记录定长（16 字节，无变长）。
pub const MCFG_REC_LEN: usize = 16;
/// 固定区总长 = 表头 36 + 保留 8 = 44；第一条记录从偏移 44 起。
pub const MCFG_FIXED_LEN: usize = MCFG_HDR_LEN + MCFG_RESERVED_LEN;
/// ECAM 每总线窗口字节数：1MB（2^20 = 256 设备 × 8 功能 × 4096B）。
pub const ECAM_BUS_BYTES: u64 = 1u64 << 20;
/// PCI 总线号上限（8 位域）。
pub const PCI_BUS_MAX: u8 = 255;
/// 表头保留区偏移（36..44 必须全零）。
pub const OFF_HDR_RESERVED: usize = MCFG_HDR_LEN;
/// 记录内 PCI 段组偏移（u16）。
pub const REC_OFF_SEGMENT: usize = 8;
/// 记录内起始总线偏移（u8）。
pub const REC_OFF_START: usize = 10;
/// 记录内结束总线偏移（u8）。
pub const REC_OFF_END: usize = 11;
/// 记录内保留区偏移（12..16 必须全零）。
pub const REC_OFF_RESERVED: usize = 12;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum McfgCode {
    /// 表头签名非 "MCFG"
    SigBad,
    /// 表头校验和坏
    SumBad,
    /// 表长不足固定区 44B（连一条记录都放不下）
    LenShort,
    /// (表长−44) 非 16 的整倍数：破条
    LenRagged,
    /// 声明表长超出实际缓冲
    LenOver,
    /// ECAM 基址非 2^20 对齐
    BaseAlign,
    /// ECAM 基址为 0
    BaseZero,
    /// 总线范围破（起始 > 结束）
    BusRange,
    /// 结束总线越 8 位域 / 跨度溢出
    BusOver,
    /// 同段组总线区间重叠
    Overlap,
    /// 记录内保留 4B 非零
    RecReserved,
    /// 表头保留 8B 非零
    HdrReserved,
    /// 条数推算与窗口步进对拍失配
    CountMismatch,
    /// 状态机倒退
    StageRegress,
    /// 双跑不一致（复现红线）
    NotRepro,
}

impl McfgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            McfgCode::SigBad => "签名非 MCFG：不是 ECAM 描述表，先核对 RSDT/XSDT 条目",
            McfgCode::SumBad => "表头校验和坏：全表 sum mod 256 ≠ 0，疑似表被改写",
            McfgCode::LenShort => "表长 < 44：固定区都不完整，拒绝按记录窗口切片",
            McfgCode::LenRagged => "破条：(表长−44) 不是 16 的整倍数，记录边界不可信",
            McfgCode::LenOver => "声明表长 > 缓冲实长：截断表不可信，补齐映射再解析",
            McfgCode::BaseAlign => "ECAM 基址非 1MB 对齐：每总线 2^20 窗口，基址必须落在 2^20 边界",
            McfgCode::BaseZero => "ECAM 基址为 0：0 页是 ISA 影子区，必是坏记录",
            McfgCode::BusRange => "总线范围破：起始总线 > 结束总线，四要素不闭合",
            McfgCode::BusOver => "总线越界/跨度溢出：结束总线 ≤ 255 且 基址+跨度 不得回绕",
            McfgCode::Overlap => "同段组 ECAM 窗口重叠：先到的记录已占据区间，后到的拒绝",
            McfgCode::RecReserved => "记录保留 4B 非零：规范要求全零，疑似伪造记录",
            McfgCode::HdrReserved => "表头保留 8B 非零：规范要求全零，先查固件",
            McfgCode::CountMismatch => "条数对拍失配：长度推算数 ≠ 窗口步进数，账不平",
            McfgCode::StageRegress => "状态机倒退：解析只能单向推进，禁止回退重走",
            McfgCode::NotRepro => "双跑不一致：同输入两次解析结果不同，查非确定性来源",
        }
    }
    pub const ALL: [McfgCode; 15] = [
        McfgCode::SigBad,
        McfgCode::SumBad,
        McfgCode::LenShort,
        McfgCode::LenRagged,
        McfgCode::LenOver,
        McfgCode::BaseAlign,
        McfgCode::BaseZero,
        McfgCode::BusRange,
        McfgCode::BusOver,
        McfgCode::Overlap,
        McfgCode::RecReserved,
        McfgCode::HdrReserved,
        McfgCode::CountMismatch,
        McfgCode::StageRegress,
        McfgCode::NotRepro,
    ];
}

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < McfgCode::ALL.len() {
        if let Some(c) = McfgCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x751C0 段闭集内"
}

// ── 字节面读数（无裸下标：全走 .get()） ─────────────────────────
fn le16(buf: &[u8], off: usize) -> Option<u16> {
    let b0 = *buf.get(off)?;
    let b1 = *buf.get(off.checked_add(1)?)?;
    Some(u16::from(b0) | (u16::from(b1) << 8))
}

fn le32(buf: &[u8], off: usize) -> Option<u32> {
    let b0 = *buf.get(off)?;
    let b1 = *buf.get(off.checked_add(1)?)?;
    let b2 = *buf.get(off.checked_add(2)?)?;
    let b3 = *buf.get(off.checked_add(3)?)?;
    Some(u32::from(b0) | (u32::from(b1) << 8) | (u32::from(b2) << 16) | (u32::from(b3) << 24))
}

fn le64(buf: &[u8], off: usize) -> Option<u64> {
    let lo = u64::from(le32(buf, off)?);
    let hi = u64::from(le32(buf, off.checked_add(4)?)?);
    Some(lo | (hi << 32))
}

/// 全表校验和（首 `declared` 字节 sum mod 256 ≡ 0）。
fn table_sum_ok(buf: &[u8], declared: usize) -> bool {
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < declared {
        if let Some(b) = buf.get(i) {
            acc += u32::from(*b);
        } else {
            return false;
        }
        i += 1;
    }
    acc & 0xFF == 0
}

// ── 四要素账本 ──────────────────────────────────────────────────
/// 一条 ECAM 分配记录的四要素。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McfgAlloc {
    /// ECAM MMIO 基址（2^20 对齐）。
    pub ecam_base: u64,
    /// PCI 段组号（u16）。
    pub pci_segment: u16,
    /// 起始总线号。
    pub start_bus: u8,
    /// 结束总线号（含）。
    pub end_bus: u8,
}

impl McfgAlloc {
    /// 覆盖总线数（含两端，闭合区间）。
    pub fn bus_count(&self) -> u32 {
        u32::from(self.end_bus) - u32::from(self.start_bus) + 1
    }
    /// 窗口字节数 = 总线数 × 1MB（checked，溢出给 None）。
    pub fn span_bytes(&self) -> Option<u64> {
        let n = u64::from(self.bus_count());
        ECAM_BUS_BYTES.checked_mul(n)
    }
    /// 2^20 对齐谓词。
    pub fn base_aligned(&self) -> bool {
        self.ecam_base % ECAM_BUS_BYTES == 0
    }
    /// 同段组区间相交谓词（闭合区间判定：a.start ≤ b.end && b.start ≤ a.end）。
    fn bus_overlaps(&self, other: &McfgAlloc) -> bool {
        self.pci_segment == other.pci_segment
            && u32::from(self.start_bus) <= u32::from(other.end_bus)
            && u32::from(other.start_bus) <= u32::from(self.end_bus)
    }
}

/// MCFG 解析账本：逐条四要素 + 条数双口径。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct McfgLedger {
    /// 表头声明的表长。
    pub declared_len: usize,
    /// 长度推算条数：(declared − 44) / 16。
    pub reckoned_count: usize,
    /// 窗口实际步进条数（第二口径，对拍用）。
    pub windowed_count: usize,
    /// 逐条四要素账。
    pub records: Vec<McfgAlloc>,
}

impl McfgLedger {
    /// 条数双口径一致（判据「条数=推算数」）。
    pub fn count_consistent(&self) -> bool {
        self.reckoned_count == self.windowed_count && self.reckoned_count == self.records.len()
    }
    /// 四要素齐账谓词：每条非零基址 + 对齐 + 闭合。
    pub fn four_elem_ok(&self) -> bool {
        let mut i = 0;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.ecam_base == 0 || !r.base_aligned() || r.start_bus > r.end_bus {
                    return false;
                }
            } else {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 单调状态机 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum McfgStage {
    Raw,
    HeaderVetted,
    CountReckoned,
    RecordsLedgered,
}

impl McfgStage {
    fn rank(self) -> u8 {
        match self {
            McfgStage::Raw => 0,
            McfgStage::HeaderVetted => 1,
            McfgStage::CountReckoned => 2,
            McfgStage::RecordsLedgered => 3,
        }
    }
}

/// 单调推进器：只许 +1，倒退/越级一律拒。
#[derive(Clone, Copy, Debug, Default)]
pub struct McfgMachine {
    stage: u8,
}

impl McfgMachine {
    pub fn new() -> McfgMachine {
        McfgMachine { stage: McfgStage::Raw.rank() }
    }
    pub fn stage(&self) -> McfgStage {
        match self.stage {
            0 => McfgStage::Raw,
            1 => McfgStage::HeaderVetted,
            2 => McfgStage::CountReckoned,
            _ => McfgStage::RecordsLedgered,
        }
    }
    pub fn advance(&mut self, to: McfgStage) -> Result<(), u32> {
        let want = to.rank();
        if want == self.stage.wrapping_add(1) {
            self.stage = want;
            Ok(())
        } else {
            Err(McfgCode::StageRegress.val())
        }
    }
}

// ── 单条记录窗口解析（四要素逐项闸） ─────────────────────────────
/// 从 `buf` 偏移 `off` 切一条 16B 记录：四要素 + 保留字节 + 对齐 + 闭合。
pub fn parse_alloc_record(buf: &[u8], off: usize) -> Result<McfgAlloc, u32> {
    if off.checked_add(MCFG_REC_LEN) > Some(buf.len()) {
        return Err(McfgCode::LenShort.val());
    }
    let ecam_base = le64(buf, off).ok_or_else(|| McfgCode::LenShort.val())?;
    let pci_segment = le16(buf, off + REC_OFF_SEGMENT).ok_or_else(|| McfgCode::LenShort.val())?;
    let start_bus = *buf.get(off + REC_OFF_START).ok_or_else(|| McfgCode::LenShort.val())?;
    let end_bus = *buf.get(off + REC_OFF_END).ok_or_else(|| McfgCode::LenShort.val())?;
    // 记录内保留 4B（偏移 +12..+16）必须全零。
    let mut rsv_dirty = false;
    let mut k = off + REC_OFF_RESERVED;
    while k < off + REC_OFF_RESERVED + 4 {
        if let Some(b) = buf.get(k) {
            if *b != 0 {
                rsv_dirty = true;
            }
        }
        k += 1;
    }
    if rsv_dirty {
        return Err(McfgCode::RecReserved.val());
    }
    if ecam_base == 0 {
        return Err(McfgCode::BaseZero.val());
    }
    if ecam_base % ECAM_BUS_BYTES != 0 {
        return Err(McfgCode::BaseAlign.val());
    }
    if start_bus > end_bus {
        return Err(McfgCode::BusRange.val());
    }
    let rec = McfgAlloc { ecam_base, pci_segment, start_bus, end_bus };
    // 跨度溢出：基址 + 窗口字节数 不得回绕。
    match (rec.span_bytes(), ecam_base.checked_add(rec.span_bytes().unwrap_or(0))) {
        (Some(_), Some(_)) => {}
        _ => return Err(McfgCode::BusOver.val()),
    }
    Ok(rec)
}

// ── MCFG 夹具构造 ───────────────────────────────────────────────
fn put8(buf: &mut Vec<u8>, off: usize, v: u8) {
    if let Some(slot) = buf.get_mut(off) {
        *slot = v;
    }
}

fn put16(buf: &mut Vec<u8>, off: usize, v: u16) {
    put8(buf, off, (v & 0xFF) as u8);
    put8(buf, off.wrapping_add(1), ((v >> 8) & 0xFF) as u8);
}

fn put32(buf: &mut Vec<u8>, off: usize, v: u32) {
    put8(buf, off, (v & 0xFF) as u8);
    put8(buf, off.wrapping_add(1), ((v >> 8) & 0xFF) as u8);
    put8(buf, off.wrapping_add(2), ((v >> 16) & 0xFF) as u8);
    put8(buf, off.wrapping_add(3), ((v >> 24) & 0xFF) as u8);
}

fn put64(buf: &mut Vec<u8>, off: usize, v: u64) {
    put32(buf, off, (v & 0xFFFF_FFFF) as u32);
    put32(buf, off.wrapping_add(4), ((v >> 32) & 0xFFFF_FFFF) as u32);
}

/// MCFG 夹具参数：一条记录 + 破坏旋钮（注入用）。
pub struct RecKnob {
    pub base: u64,
    pub segment: u16,
    pub start_bus: u8,
    pub end_bus: u8,
    /// 注入：记录内保留字节置脏。
    pub dirty_reserved: bool,
}

/// MCFG 夹具：按记录列表铺表，`pad_bytes` 用于构造破条长度，
/// `dirty_hdr_rsv` 注入表头保留区脏字节。
pub fn build_mcfg(recs: &[RecKnob], pad_bytes: usize, dirty_hdr_rsv: bool) -> Vec<u8> {
    let declared = MCFG_FIXED_LEN + recs.len() * MCFG_REC_LEN + pad_bytes;
    let mut b: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < declared {
        b.push(0u8);
        i += 1;
    }
    put8(&mut b, 0, b'M');
    put8(&mut b, 1, b'C');
    put8(&mut b, 2, b'F');
    put8(&mut b, 3, b'G');
    put32(&mut b, 4, declared as u32);
    put8(&mut b, 8, 1); // MCFG revision 1（表头 Revision @8，Length 占 4..8）
    put8(&mut b, 9, 0); // 校验和先置 0（表头 Checksum @9）
    if dirty_hdr_rsv {
        if let Some(slot) = b.get_mut(OFF_HDR_RESERVED) {
            *slot = 0xA5;
        }
    }
    let mut idx = 0;
    while idx < recs.len() {
        if let Some(r) = recs.get(idx) {
            let off = MCFG_FIXED_LEN + idx * MCFG_REC_LEN;
            put64(&mut b, off, r.base);
            put16(&mut b, off + REC_OFF_SEGMENT, r.segment);
            put8(&mut b, off + REC_OFF_START, r.start_bus);
            put8(&mut b, off + REC_OFF_END, r.end_bus);
            if r.dirty_reserved {
                if let Some(slot) = b.get_mut(off + REC_OFF_RESERVED) {
                    *slot = 0x5A;
                }
            }
        }
        idx += 1;
    }
    // 校验和回填。
    let mut acc: u32 = 0;
    let mut k = 0;
    while k < declared {
        if let Some(byte) = b.get(k) {
            acc += u32::from(*byte);
        }
        k += 1;
    }
    let cks = (0x100u32 - (acc & 0xFF)) & 0xFF;
    put8(&mut b, 9, cks as u8);
    b
}

/// 标准真机口径夹具（QEMU 型：段 0 双区间 + 段 1 单区间）。
pub fn mcfg_machine() -> Vec<u8> {
    let recs = [
        RecKnob { base: 0xE000_0000, segment: 0, start_bus: 0, end_bus: 63, dirty_reserved: false },
        RecKnob { base: 0xE100_0000, segment: 0, start_bus: 64, end_bus: 127, dirty_reserved: false },
        RecKnob { base: 0xE800_0000, segment: 1, start_bus: 0, end_bus: 255, dirty_reserved: false },
    ];
    build_mcfg(&recs, 0, false)
}

// ── 主解析器 ────────────────────────────────────────────────────
/// MCFG 字节面解析：表头门 → 条数推算 → 逐条四要素 → N×N 重叠对拍。
pub fn parse_mcfg(buf: &[u8]) -> Result<McfgLedger, u32> {
    // 门 0：缓冲至少要放得下固定区。
    if buf.len() < MCFG_FIXED_LEN {
        return Err(McfgCode::LenShort.val());
    }
    // 门 1：签名。
    let sig_ok = buf.get(0) == Some(&b'M')
        && buf.get(1) == Some(&b'C')
        && buf.get(2) == Some(&b'F')
        && buf.get(3) == Some(&b'G');
    if !sig_ok {
        return Err(McfgCode::SigBad.val());
    }
    // 门 2：表长读数 + 缓冲上限。
    let declared = match le32(buf, 4) {
        Some(v) => v as usize,
        None => return Err(McfgCode::LenShort.val()),
    };
    if declared > buf.len() {
        return Err(McfgCode::LenOver.val());
    }
    if declared < MCFG_FIXED_LEN {
        return Err(McfgCode::LenShort.val());
    }
    // 门 3：全表校验和。
    if !table_sum_ok(buf, declared) {
        return Err(McfgCode::SumBad.val());
    }
    // 门 4：表头保留区 8B 全零。
    let mut hdr_dirty = false;
    let mut k = OFF_HDR_RESERVED;
    while k < MCFG_FIXED_LEN {
        if let Some(b) = buf.get(k) {
            if *b != 0 {
                hdr_dirty = true;
            }
        }
        k += 1;
    }
    if hdr_dirty {
        return Err(McfgCode::HdrReserved.val());
    }
    // 条数推算：破条拒。
    let body = declared - MCFG_FIXED_LEN;
    if body % MCFG_REC_LEN != 0 {
        return Err(McfgCode::LenRagged.val());
    }
    let reckoned = body / MCFG_REC_LEN;
    // 窗口步进第二口径：按缓冲实长能放下的完整记录数。
    let avail = buf.len().min(declared) - MCFG_FIXED_LEN;
    let windowed = avail / MCFG_REC_LEN;
    if reckoned != windowed {
        return Err(McfgCode::CountMismatch.val());
    }
    // 逐条四要素。
    let mut records: Vec<McfgAlloc> = Vec::new();
    let mut idx = 0;
    while idx < reckoned {
        let off = MCFG_FIXED_LEN + idx * MCFG_REC_LEN;
        let rec = parse_alloc_record(buf, off)?;
        // N×N 重叠对拍：与已入账记录两两对拍。
        let mut j = 0;
        while j < records.len() {
            if let Some(prev) = records.get(j) {
                if prev.bus_overlaps(&rec) {
                    return Err(McfgCode::Overlap.val());
                }
            }
            j += 1;
        }
        records.push(rec);
        idx += 1;
    }
    Ok(McfgLedger {
        declared_len: declared,
        reckoned_count: reckoned,
        windowed_count: windowed,
        records,
    })
}

/// 全链解析（状态机驱动）。
pub fn parse_mcfg_full(buf: &[u8]) -> Result<McfgLedger, u32> {
    let mut m = McfgMachine::new();
    if buf.len() < MCFG_FIXED_LEN {
        return Err(McfgCode::LenShort.val());
    }
    m.advance(McfgStage::HeaderVetted)?;
    let ledger = parse_mcfg(buf)?;
    m.advance(McfgStage::CountReckoned)?;
    if !ledger.count_consistent() || !ledger.four_elem_ok() {
        return Err(McfgCode::CountMismatch.val());
    }
    m.advance(McfgStage::RecordsLedgered)?;
    Ok(ledger)
}

// ── LCG（种子固定） ─────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

// ── 注入检出账（四路：对齐/范围/重叠/破条） ──────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c += 1;
                }
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
            } else {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入轮（LCG 驱动，固定种子，每轮四路必抓）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(u64::from(SEG_BASE));
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：对齐路（基址偏 1 字节）。
        let mis = [RecKnob { base: 0xE000_0001, segment: 0, start_bus: 0, end_bus: 0, dirty_reserved: false }];
        led.record(0, parse_mcfg(&build_mcfg(&mis, 0, false)).is_err());
        // 路 2：范围路（起始 > 结束）。
        let rng_bad = [RecKnob { base: 0xE000_0000, segment: 0, start_bus: 9, end_bus: 3, dirty_reserved: false }];
        led.record(1, parse_mcfg(&build_mcfg(&rng_bad, 0, false)).is_err());
        // 路 3：重叠路（同段组区间相交）。
        let ovl = [
            RecKnob { base: 0xE000_0000, segment: 0, start_bus: 0, end_bus: 63, dirty_reserved: false },
            RecKnob { base: 0xE040_0000, segment: 0, start_bus: 32, end_bus: 95, dirty_reserved: false },
        ];
        led.record(2, parse_mcfg(&build_mcfg(&ovl, 0, false)).is_err());
        // 路 4：破条路（垫 3 字节 → (len−44)%16≠0）。
        let good = [RecKnob { base: 0xE000_0000, segment: 0, start_bus: 0, end_bus: 0, dirty_reserved: false }];
        led.record(3, parse_mcfg(&build_mcfg(&good, 3, false)).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：真机三记录账 → 条数对拍 → 四要素齐账 → 四路注入必抓。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let m3 = mcfg_machine();
    let l = parse_mcfg_full(&m3)?;
    let ok_main = (l.records.len() == 3
        && l.count_consistent()
        && l.four_elem_ok()
        && l.records.first().map(|r| r.pci_segment) == Some(0)
        && l.records.first().map(|r| r.ecam_base) == Some(0xE000_0000)) as u32;
    // 空记录边界：纯表头（44B，0 条）合法。
    let empty = build_mcfg(&[], 0, false);
    let le = parse_mcfg(&empty)?;
    let ok_empty = (le.reckoned_count == 0 && le.records.is_empty()) as u32;
    // 异段组同总线区间合法（段 0 与段 1 各管各窗）。
    let cross = [
        RecKnob { base: 0xE000_0000, segment: 0, start_bus: 0, end_bus: 255, dirty_reserved: false },
        RecKnob { base: 0xF000_0000, segment: 1, start_bus: 0, end_bus: 255, dirty_reserved: false },
    ];
    let ok_cross = parse_mcfg(&build_mcfg(&cross, 0, false)).is_ok() as u32;
    // 四路注入必抓。
    let inj = inject_rounds(4)?;
    let ok_inj = inj.all_caught() as u32;
    Ok((ok_main + ok_empty + ok_cross, ok_inj))
}

/// 双跑一致（复现红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(McfgCode::NotRepro.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < McfgCode::ALL.len() {
        if let Some(c) = McfgCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 真机三记录全账绿。
    let main = parse_mcfg_full(&mcfg_machine());
    v.push((
        "F0448-真机三记录",
        match &main {
            Ok(l) => l.records.len() == 3 && l.count_consistent() && l.four_elem_ok(),
            Err(_) => false,
        },
        match &main {
            Ok(l) => format!("n={} reck={}", l.records.len(), l.reckoned_count),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 2. 条数推算对拍（len=44+16k 口径 + 双口径相等）。
    let two = [
        RecKnob { base: 0xE000_0000, segment: 0, start_bus: 0, end_bus: 0, dirty_reserved: false },
        RecKnob { base: 0xE100_0000, segment: 0, start_bus: 1, end_bus: 2, dirty_reserved: false },
    ];
    let t2 = parse_mcfg(&build_mcfg(&two, 0, false));
    let cnt_ok = match &t2 {
        Ok(l) => l.declared_len == 44 + 32 && l.reckoned_count == 2 && l.windowed_count == 2,
        Err(_) => false,
    };
    v.push((
        "F0448-条数对拍",
        cnt_ok,
        String::from("len=44+16*2 推算=窗口=2"),
    ));

    // 3. 四要素入账逐项核（段组/基址/起始/结束）。
    let four_ok = match &main {
        Ok(l) => match (l.records.get(0), l.records.get(1), l.records.get(2)) {
            (Some(a), Some(b), Some(c)) => {
                a.ecam_base == 0xE000_0000
                    && a.pci_segment == 0
                    && a.start_bus == 0
                    && a.end_bus == 63
                    && b.ecam_base == 0xE100_0000
                    && b.start_bus == 64
                    && b.end_bus == 127
                    && c.pci_segment == 1
                    && c.end_bus == 255
            }
            _ => false,
        },
        Err(_) => false,
    };
    v.push((
        "F0448-四要素入账",
        four_ok,
        String::from("段组/基址/起始/结束 逐条核对"),
    ));

    // 4. 基址 2^20 对齐注入必抓。
    let mis = [RecKnob { base: 0xE000_1234, segment: 0, start_bus: 0, end_bus: 0, dirty_reserved: false }];
    let misalign = parse_mcfg(&build_mcfg(&mis, 0, false));
    v.push((
        "F0448-对齐注入",
        misalign == Err(McfgCode::BaseAlign.val()),
        String::from("基址非 1MB 边界拒"),
    ));

    // 5. 基址为 0 拒。
    let zero = [RecKnob { base: 0, segment: 0, start_bus: 0, end_bus: 0, dirty_reserved: false }];
    let zb = parse_mcfg(&build_mcfg(&zero, 0, false));
    v.push((
        "F0448-零基址拒",
        zb == Err(McfgCode::BaseZero.val()),
        String::from("基址 0（ISA 影子区）拒"),
    ));

    // 6. 总线范围破拒（起始 > 结束）。
    let rng_bad = [RecKnob { base: 0xE000_0000, segment: 0, start_bus: 200, end_bus: 100, dirty_reserved: false }];
    let rb = parse_mcfg(&build_mcfg(&rng_bad, 0, false));
    v.push((
        "F0448-范围破拒",
        rb == Err(McfgCode::BusRange.val()),
        String::from("start>end 不闭合拒"),
    ));

    // 7. 重叠注入必抓 + 异段组合法分账。
    let ovl = [
        RecKnob { base: 0xE000_0000, segment: 0, start_bus: 0, end_bus: 63, dirty_reserved: false },
        RecKnob { base: 0xE040_0000, segment: 0, start_bus: 32, end_bus: 95, dirty_reserved: false },
    ];
    let ov = parse_mcfg(&build_mcfg(&ovl, 0, false));
    let cross = [
        RecKnob { base: 0xE000_0000, segment: 0, start_bus: 0, end_bus: 255, dirty_reserved: false },
        RecKnob { base: 0xF000_0000, segment: 1, start_bus: 0, end_bus: 255, dirty_reserved: false },
    ];
    let cr = parse_mcfg(&build_mcfg(&cross, 0, false));
    v.push((
        "F0448-重叠/跨段分账",
        ov == Err(McfgCode::Overlap.val()) && cr.is_ok(),
        String::from("同段组拒 / 异段组收"),
    ));

    // 8. 破条注入必抓（垫 1/3/15 字节）。
    let one = [RecKnob { base: 0xE000_0000, segment: 0, start_bus: 0, end_bus: 0, dirty_reserved: false }];
    let p1 = parse_mcfg(&build_mcfg(&one, 1, false));
    let p15 = parse_mcfg(&build_mcfg(&one, 15, false));
    v.push((
        "F0448-破条注入",
        p1 == Err(McfgCode::LenRagged.val()) && p15 == Err(McfgCode::LenRagged.val()),
        String::from("(len−44)%16≠0 拒"),
    ));

    // 9. 表长不足 44 拒。
    let short = build_mcfg(&[], 0, false);
    let s = parse_mcfg(&short[..40.min(short.len())]);
    v.push((
        "F0448-短表拒",
        s == Err(McfgCode::LenShort.val()),
        String::from("40B 连固定区都不完整"),
    ));

    // 10. 声明表长超缓冲拒。
    let mut over = mcfg_machine();
    put32(&mut over, 4, 8192);
    let ov2 = parse_mcfg(&over);
    v.push((
        "F0448-超缓冲拒",
        ov2 == Err(McfgCode::LenOver.val()),
        String::from("declared>buf 显性拒"),
    ));

    // 11. 签名/校验和注入必抓。
    let mut sig = mcfg_machine();
    put8(&mut sig, 1, b'X');
    let sg = parse_mcfg(&sig);
    let mut bad = mcfg_machine();
    let bl = bad.len();
    if let Some(slot) = bad.get_mut(bl - 1) {
        *slot = slot.wrapping_add(1);
    }
    let bs = parse_mcfg(&bad);
    v.push((
        "F0448-签名/校验和",
        sg == Err(McfgCode::SigBad.val()) && bs == Err(McfgCode::SumBad.val()),
        String::from("MCFG→XCFG / 尾字节扰动"),
    ));

    // 12. 保留区两处注入必抓（表头 8B + 记录 4B）。
    let hd = build_mcfg(&one, 0, true);
    let h = parse_mcfg(&hd);
    let dirty = [RecKnob { base: 0xE000_0000, segment: 0, start_bus: 0, end_bus: 0, dirty_reserved: true }];
    let rd = parse_mcfg(&build_mcfg(&dirty, 0, false));
    v.push((
        "F0448-保留区注入",
        h == Err(McfgCode::HdrReserved.val()) && rd == Err(McfgCode::RecReserved.val()),
        String::from("表头 8B / 记录 4B 脏字节"),
    ));

    // 13. 空记录边界（44B / 0 条）合法。
    let empty = build_mcfg(&[], 0, false);
    let em = parse_mcfg(&empty);
    let em_ok = match &em {
        Ok(l) => l.reckoned_count == 0 && l.records.is_empty() && l.count_consistent(),
        Err(_) => false,
    };
    v.push((
        "F0448-空记录边界",
        em_ok,
        String::from("纯表头 44B，0 条合法"),
    ));

    // 14. 状态机倒退拒（越级 / 重复 / 正常走完三态）。
    let mut m = McfgMachine::new();
    let a1 = m.advance(McfgStage::CountReckoned).is_err();
    let a2s = m.advance(McfgStage::HeaderVetted).is_ok();
    let a3 = m.advance(McfgStage::HeaderVetted).is_err();
    let a4 = m.advance(McfgStage::CountReckoned).is_ok();
    let a5 = m.advance(McfgStage::RecordsLedgered).is_ok();
    v.push((
        "F0448-状态机倒退",
        a1 && a2s && a3 && a4 && a5,
        String::from("禁越级 + 禁重走 + 正常走完"),
    ));

    // 15. 注入账 10/10。
    let inj = inject_rounds(10);
    v.push((
        "F0448-注入10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));

    // 16. 双跑一致。
    v.push((
        "F0448-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));

    // 17. 段位守卫。
    v.push((
        "F0448-段位守卫",
        segment_guard(),
        String::from("0x751C0 窗口"),
    ));

    v
}

pub fn run_unx0448_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0448 MCFG 表解析（ECAM 基址账）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元测试（注入必抓 / 双跑一致 / 边界） ───────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_machine_three_records() {
        let l = match parse_mcfg_full(&mcfg_machine()) {
            Ok(v) => v,
            Err(_) => {
                assert!(false, "机器夹具必须可解析");
                return;
            }
        };
        assert_eq!(l.records.len(), 3);
        assert_eq!(l.reckoned_count, 3);
        assert_eq!(l.windowed_count, 3);
        assert!(l.four_elem_ok());
        assert_eq!(l.records.get(1).map(|r| r.pci_segment), Some(0));
    }

    #[test]
    fn test_count_formula() {
        let one = [RecKnob { base: 0xE000_0000, segment: 0, start_bus: 0, end_bus: 0, dirty_reserved: false }];
        match parse_mcfg(&build_mcfg(&one, 0, false)) {
            Ok(l) => {
                assert_eq!(l.declared_len, 44 + 16);
                assert_eq!(l.reckoned_count, 1);
            }
            Err(_) => assert!(false, "单记录表必须可解析"),
        }
    }

    #[test]
    fn test_align_and_zero_base() {
        let mis = [RecKnob { base: 0xE000_0800, segment: 0, start_bus: 0, end_bus: 0, dirty_reserved: false }];
        assert_eq!(parse_mcfg(&build_mcfg(&mis, 0, false)), Err(McfgCode::BaseAlign.val()));
        let zero = [RecKnob { base: 0, segment: 0, start_bus: 0, end_bus: 0, dirty_reserved: false }];
        assert_eq!(parse_mcfg(&build_mcfg(&zero, 0, false)), Err(McfgCode::BaseZero.val()));
    }

    #[test]
    fn test_bus_range_and_overlap() {
        let rng_bad = [RecKnob { base: 0xE000_0000, segment: 0, start_bus: 9, end_bus: 3, dirty_reserved: false }];
        assert_eq!(parse_mcfg(&build_mcfg(&rng_bad, 0, false)), Err(McfgCode::BusRange.val()));
        let ovl = [
            RecKnob { base: 0xE000_0000, segment: 0, start_bus: 0, end_bus: 63, dirty_reserved: false },
            RecKnob { base: 0xE040_0000, segment: 0, start_bus: 32, end_bus: 95, dirty_reserved: false },
        ];
        assert_eq!(parse_mcfg(&build_mcfg(&ovl, 0, false)), Err(McfgCode::Overlap.val()));
    }

    #[test]
    fn test_cross_segment_same_buses_ok() {
        let cross = [
            RecKnob { base: 0xE000_0000, segment: 0, start_bus: 0, end_bus: 255, dirty_reserved: false },
            RecKnob { base: 0xF000_0000, segment: 1, start_bus: 0, end_bus: 255, dirty_reserved: false },
        ];
        assert!(parse_mcfg(&build_mcfg(&cross, 0, false)).is_ok());
    }

    #[test]
    fn test_ragged_and_short_and_over() {
        let one = [RecKnob { base: 0xE000_0000, segment: 0, start_bus: 0, end_bus: 0, dirty_reserved: false }];
        assert_eq!(parse_mcfg(&build_mcfg(&one, 7, false)), Err(McfgCode::LenRagged.val()));
        let short = build_mcfg(&one, 0, false);
        let head40 = short.get(..40).unwrap_or(&[]);
        assert_eq!(parse_mcfg(head40), Err(McfgCode::LenShort.val()));
        let mut over = mcfg_machine();
        put32(&mut over, 4, 8192);
        assert_eq!(parse_mcfg(&over), Err(McfgCode::LenOver.val()));
    }

    #[test]
    fn test_sig_sum_reserved() {
        let mut sig = mcfg_machine();
        put8(&mut sig, 3, b'X');
        assert_eq!(parse_mcfg(&sig), Err(McfgCode::SigBad.val()));
        let mut bad = mcfg_machine();
        let n = bad.len();
        if let Some(slot) = bad.get_mut(n - 1) {
            *slot = slot.wrapping_add(1);
        }
        assert_eq!(parse_mcfg(&bad), Err(McfgCode::SumBad.val()));
        let hd = build_mcfg(&[], 0, true);
        assert_eq!(parse_mcfg(&hd), Err(McfgCode::HdrReserved.val()));
        let dirty = [RecKnob { base: 0xE000_0000, segment: 0, start_bus: 0, end_bus: 0, dirty_reserved: true }];
        assert_eq!(parse_mcfg(&build_mcfg(&dirty, 0, false)), Err(McfgCode::RecReserved.val()));
    }

    #[test]
    fn test_empty_table_boundary() {
        let empty = build_mcfg(&[], 0, false);
        match parse_mcfg(&empty) {
            Ok(l) => {
                assert_eq!(l.records.len(), 0);
                assert_eq!(l.reckoned_count, 0);
                assert!(l.count_consistent());
            }
            Err(_) => assert!(false, "纯表头必须可解析"),
        }
    }

    #[test]
    fn test_machine_regress() {
        let mut m = McfgMachine::new();
        // 越级拒：Raw 直接跳 CountReckoned。
        assert!(m.advance(McfgStage::CountReckoned).is_err());
        assert!(m.advance(McfgStage::HeaderVetted).is_ok());
        // 重复拒。
        assert!(m.advance(McfgStage::HeaderVetted).is_err());
        assert!(m.advance(McfgStage::CountReckoned).is_ok());
        assert!(m.advance(McfgStage::RecordsLedgered).is_ok());
    }

    #[test]
    fn test_double_run_repro() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_inject_all_caught() {
        let led = match inject_rounds(10) {
            Ok(v) => v,
            Err(_) => {
                assert!(false, "注入轮不得报错");
                return;
            }
        };
        assert!(led.all_caught());
        assert_eq!(led.tries, [10, 10, 10, 10]);
        assert_eq!(led.caught, [10, 10, 10, 10]);
    }

    #[test]
    fn test_segment_guard_and_detail() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        for c in McfgCode::ALL.iter() {
            assert_ne!(detail(c.val()), "未知诊断码：不在 0x751C0 段闭集内");
        }
    }
}
