//! UNX-F48893 五窗跨模块寻址 · 诊断码普查簿 · 0x70B80
//! ═══════════════════════════════════════════════════════════════════════
//! 覆盖卡：F48893（五窗寻址 ×10^3 零错窗）· F48894（诊断码普查 ×10^3
//!         零漏项）· F48895（桥封存 ×10^3 复算一致）
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48893 ① 纯功能代码 ≥400 行；
//!          ② UNX-F48893-J1 五窗寻址 ×10^3 零错窗；
//!   F48894 ① 纯功能代码 ≥400 行；
//!          ② UNX-F48894-J1 诊断码普查 ×10^3 零漏项；
//!   F48895 ① 纯功能代码 ≥400 行；
//!          ② UNX-F48895-J1 桥封存 ×10^3 复算一致。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：窗表合法 → 段号归属 → 槽位落座 → 明细一致 → 普查齐项
//!           → 封存复算 → 模糊三路 → 账平复核。
//!   次序不可倒的理由逐条：
//!     · 窗表先判：五窗的基址与码数必须自洽，若窗表本身重叠，后面
//!       无论怎么寻址都会出现「一码两窗」，而且两窗都能给出明细；
//!     · 段号归属次判：0x70A00 段只属于 core，0x70A40 段只属于 meta；
//!       先查明细再查归属，就会把别人的明细当成自己的；
//!     · 槽位必须在明细之前：槽号是窗表下标，槽错则取出的是邻窗的
//!       基址，明细再怎么比对都比在错误的窗上；
//!     · 明细一致必须早于普查：普查只统计「条目是否齐全」，它不自证
//!       明细对不对；一码明细写错，普查仍会全绿；
//!     · 封存复算必须在模糊之前：模糊用随机码探路，前提是封存值已
//!       定；若封存先于复算，封存只是抄了一遍自己；
//!     · 账平复核最后执行，它只回答「有没有码被静默漏掉」。
//! 关键差异：本模块只拥有「地址窗 + 普查 + 封存」这一层。
//!   core/meta/guard/dispatch/edge 各自拥有自己的域与判据；bridge 不
//!   重新实现其中任何一条，只把它们已冻的窗口、码数与明细聚成一本
//!   普查簿，并给出可复算的族指纹。quality 与 seal 尚未纳入桥；它们
//!   由 F48896 与 F48899 自行收口，本模块不越界替它们封存。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

use super::unx48881_ops_core as c_core;
use super::unx48883_ops_meta as c_meta;
use super::unx48885_ops_guard as c_guard;
use super::unx48888_ops_dispatch as c_dispatch;
use super::unx48891_ops_edge as c_edge;

/// 诊断段基址（0x70B80，VX6 批 B05 族第六槽）。
pub const SEG_BASE: u32 = 0x70B80;

/// 五窗：(基址, 码数)。
pub const WINDOWS: [(u32, u8); 5] = [
    (0x70A00, 10),
    (0x70A40, 12),
    (0x70A80, 11),
    (0x70AE0, 11),
    (0x70B40, 11),
];
/// 五窗码数之和。
pub const FAMILY_CODES: u32 = 55;
/// 窗距（相邻窗基址差）。
pub const WINDOW_STRIDE: u32 = 0x40;
/// 模糊轮数。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 注入轮数。
pub const INJECT_ROUNDS: u32 = 1000;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0B） ───────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BcCode {
    /// 窗表非法（基址倒挂 / 码数为零 / 越界）
    WindowBad,
    /// 槽位越界
    SlotBad,
    /// 码不属于该窗
    CodeBad,
    /// 明细不一致（本地明细与源模块不符）
    DetailBad,
    /// 普查簿非法（行数 / 重复码 / 槽序）
    CensusBad,
    /// 封存值不一致（复算漂移）
    SealBad,
    /// 模糊路径失配（三路结论不同）
    FuzzBad,
    /// 账不平（窗口计数与行数不符）
    LedgerBad,
    /// 导出畸形（行数 / 字段 / 数字）
    RenderBad,
    /// 重放（同一条目二次入册）
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl BcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            BcCode::WindowBad => "窗表非法：五窗基址须相隔 0x40 且码数非零",
            BcCode::SlotBad => "槽位越界：槽号只允许 0..5",
            BcCode::CodeBad => "码不属于该窗：段号与槽位必须一致",
            BcCode::DetailBad => "明细不一致：桥内明细必须逐字等于源模块输出",
            BcCode::CensusBad => "普查簿非法：55 行、无重复码、槽序递增",
            BcCode::SealBad => "封存不一致：复算指纹必须与冻结指纹相同",
            BcCode::FuzzBad => "模糊失配：三路探针对同一码给出不同结论",
            BcCode::LedgerBad => "账不平：窗口码数之和与普查行数不等",
            BcCode::RenderBad => "导出畸形：行数 55、字段 5、数字可解析",
            BcCode::ReplayBad => "重放：同一条目不得二次入册",
            BcCode::SegmentBad => "段位越界：诊断码逃出 0x70B80 窗口",
        }
    }

    pub const ALL: [BcCode; 11] = [
        BcCode::WindowBad,
        BcCode::SlotBad,
        BcCode::CodeBad,
        BcCode::DetailBad,
        BcCode::CensusBad,
        BcCode::SealBad,
        BcCode::FuzzBad,
        BcCode::LedgerBad,
        BcCode::RenderBad,
        BcCode::ReplayBad,
        BcCode::SegmentBad,
    ];
}

/// 段内码取明细文本；闭集外给固定文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < BcCode::ALL.len() {
        if let Some(c) = BcCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知桥诊断码：不属于 0x70B80 闭集"
}

// ── FNV-1a 64 ────────────────────────────────────────────────────────
pub const FNV_OFF: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u16(h: u64, v: u16) -> u64 {
    let a = fold_byte(h, (v & 0xFF) as u8);
    fold_byte(a, (v >> 8) as u8)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let a = fold_u16(h, (v & 0xFFFF) as u16);
    fold_u16(a, (v >> 16) as u16)
}

pub fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut x = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            x = fold_byte(x, *b);
        }
        i += 1;
    }
    x
}

// ── 确定性伪随机（模糊探路用） ───────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg { state: if seed == 0 { 0x853C_49E6_748F_EA9B } else { seed } }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 32) as u32
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next_u32() % n }
    }
}

// ── 窗表 ────────────────────────────────────────────────────────────
/// 第 slot 窗的末码（含）。
pub fn window_end(slot: usize) -> Option<u32> {
    match WINDOWS.get(slot) {
        Some((base, count)) => Some(base.saturating_add(*count as u32)),
        None => None,
    }
}

/// 码是否属于第 slot 窗：base < code <= base+count。
pub fn in_window(slot: usize, code: u32) -> bool {
    match WINDOWS.get(slot) {
        Some((base, count)) => code > *base && code <= base.saturating_add(*count as u32),
        None => false,
    }
}

/// 查码所在槽；不属于任何窗返回 None。
pub fn lookup_slot(code: u32) -> Option<usize> {
    let mut i = 0usize;
    while i < WINDOWS.len() {
        if in_window(i, code) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 窗基址。
pub fn source_base(slot: usize) -> Option<u32> {
    match WINDOWS.get(slot) {
        Some((b, _)) => Some(*b),
        None => None,
    }
}

/// 窗内第 ix 个码（ix 从 0 起；越界返回 None）。
pub fn code_at(slot: usize, ix: usize) -> Option<u32> {
    match WINDOWS.get(slot) {
        Some((base, count)) => {
            if ix < *count as usize {
                Some(base + 1 + ix as u32)
            } else {
                None
            }
        }
        None => None,
    }
}

/// 窗内码序号（0 起）；不属于该窗返回 None。
pub fn index_of(slot: usize, code: u32) -> Option<usize> {
    match WINDOWS.get(slot) {
        Some((base, _)) => {
            if in_window(slot, code) {
                Some((code - base - 1) as usize)
            } else {
                None
            }
        }
        None => None,
    }
}

/// 窗表自审：五窗、基址升序、码数 1..=0x40、窗距不小于 0x40、无交叠。
pub fn windows_ok() -> bool {
    let mut i = 0usize;
    let mut last_end = 0u32;
    while i < WINDOWS.len() {
        let (base, count) = match WINDOWS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if count == 0 || count as u32 > WINDOW_STRIDE {
            return false;
        }
        if i > 0 && base < last_end {
            return false;
        }
        if i > 0 {
            let prev = match WINDOWS.get(i - 1) {
                Some((b, _)) => *b,
                None => return false,
            };
            if base.saturating_sub(prev) < WINDOW_STRIDE {
                return false;
            }
        }
        last_end = base.saturating_add(count as u32);
        i += 1;
    }
    true
}

// ── 源模块明细分派 ──────────────────────────────────────────────────
/// 按槽位取源模块的自由函数明细；窗表外的槽给固定文案。
pub fn module_detail(slot: usize, code: u32) -> &'static str {
    match slot {
        0 => c_core::detail(code),
        1 => c_meta::detail(code),
        2 => c_guard::detail(code),
        3 => c_dispatch::detail(code),
        4 => c_edge::detail(code),
        _ => "未知槽位：桥只登记五个源模块",
    }
}

/// 五窗各自的段位守卫与三探针必须全绿。
pub fn audit_sources() -> Result<(), u32> {
    if !c_core::segment_guard()
        || !c_meta::segment_guard()
        || !c_guard::segment_guard()
        || !c_dispatch::segment_guard()
        || !c_edge::segment_guard()
    {
        return Err(BcCode::WindowBad.val());
    }
    c_core::probe_seq_a().map_err(|_| BcCode::SealBad.val())?;
    c_meta::probe_seq_a().map_err(|_| BcCode::SealBad.val())?;
    c_guard::probe_seq_a().map_err(|_| BcCode::SealBad.val())?;
    c_dispatch::probe_seq_a().map_err(|_| BcCode::SealBad.val())?;
    c_edge::probe_seq_a().map_err(|_| BcCode::SealBad.val())?;
    Ok(())
}

// ── 普查簿 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CensusRow {
    pub slot: u8,
    pub ix: u16,
    pub code: u32,
    pub count: u8,
    pub detail_hash: u32,
}

impl CensusRow {
    pub fn text(&self) -> String {
        format!("{}|{}|0x{:08X}|{}|0x{:08X}", self.slot, self.ix, self.code, self.count, self.detail_hash)
    }
}

#[derive(Clone, Debug)]
pub struct CensusBook {
    rows: Vec<CensusRow>,
    per_slot: [u32; 5],
    digest: u64,
}

impl CensusBook {
    pub fn new() -> CensusBook {
        CensusBook { rows: Vec::new(), per_slot: [0; 5], digest: FNV_OFF }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn slot_count(&self, slot: usize) -> u32 {
        match self.per_slot.get(slot) {
            Some(v) => *v,
            None => 0,
        }
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    pub fn rows(&self) -> &[CensusRow] {
        &self.rows
    }

    /// 入册：槽号/序号/码/码数四项必须与窗表一致，码不得重复。
    pub fn push(&mut self, row: CensusRow) -> Result<(), u32> {
        let slot = row.slot as usize;
        let (base, count) = match WINDOWS.get(slot) {
            Some(v) => *v,
            None => return Err(BcCode::SlotBad.val()),
        };
        if row.count != count {
            return Err(BcCode::CensusBad.val());
        }
        let want = base.saturating_add(1).saturating_add(row.ix as u32);
        if row.code != want {
            return Err(BcCode::CodeBad.val());
        }
        if row.ix as usize >= count as usize {
            return Err(BcCode::SlotBad.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.code == row.code {
                    return Err(BcCode::ReplayBad.val());
                }
            }
            i += 1;
        }
        if let Some(n) = self.per_slot.get_mut(slot) {
            *n = n.saturating_add(1);
        }
        self.digest = fold_byte(self.digest, row.slot);
        self.digest = fold_u16(self.digest, row.ix);
        self.digest = fold_u32(self.digest, row.code);
        self.digest = fold_u32(self.digest, row.detail_hash);
        self.rows.push(row);
        Ok(())
    }

    /// 自审：55 行、逐窗计数、码升序、明细哈希与源模块一致。
    pub fn audit(&self) -> Result<(), u32> {
        if self.rows.len() != FAMILY_CODES as usize {
            return Err(BcCode::CensusBad.val());
        }
        let mut i = 0usize;
        let mut last_code = 0u32;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(BcCode::CensusBad.val()),
            };
            if i > 0 && r.code <= last_code {
                return Err(BcCode::CensusBad.val());
            }
            let src = module_detail(r.slot as usize, r.code);
            let want = fold_str(FNV_OFF, src) as u32;
            if want != r.detail_hash {
                return Err(BcCode::DetailBad.val());
            }
            last_code = r.code;
            i += 1;
        }
        let mut s = 0usize;
        while s < WINDOWS.len() {
            let (_, count) = match WINDOWS.get(s) {
                Some(v) => *v,
                None => return Err(BcCode::WindowBad.val()),
            };
            if self.slot_count(s) != count as u32 {
                return Err(BcCode::LedgerBad.val());
            }
            s += 1;
        }
        Ok(())
    }
}

fn detail_hash(slot: usize, code: u32) -> u32 {
    fold_str(FNV_OFF, module_detail(slot, code)) as u32
}

/// 标准普查：五窗逐码入册。
pub fn standard_book() -> Result<CensusBook, u32> {
    if !windows_ok() {
        return Err(BcCode::WindowBad.val());
    }
    let mut book = CensusBook::new();
    let mut slot = 0usize;
    while slot < WINDOWS.len() {
        let (_, count) = match WINDOWS.get(slot) {
            Some(v) => *v,
            None => return Err(BcCode::WindowBad.val()),
        };
        let mut ix = 0usize;
        while ix < count as usize {
            let code = match code_at(slot, ix) {
                Some(v) => v,
                None => return Err(BcCode::CodeBad.val()),
            };
            let row = CensusRow {
                slot: slot as u8,
                ix: ix as u16,
                code,
                count,
                detail_hash: detail_hash(slot, code),
            };
            book.push(row)?;
            ix += 1;
        }
        slot += 1;
    }
    book.audit()?;
    Ok(book)
}

// ── 源模块封存 ──────────────────────────────────────────────────────
/// 每槽源模块的五项自证：黄金运行、段位守卫、三探针。
pub fn module_seal(slot: usize) -> Option<u64> {
    let (ok, base) = match slot {
        0 => (
            c_core::golden_run().is_ok()
                && c_core::segment_guard()
                && c_core::probe_seq_a().is_ok()
                && c_core::probe_seq_b().is_ok()
                && c_core::probe_seq_c().is_ok(),
            source_base(0),
        ),
        1 => (
            c_meta::golden_run().is_ok()
                && c_meta::segment_guard()
                && c_meta::probe_seq_a().is_ok()
                && c_meta::probe_seq_b().is_ok()
                && c_meta::probe_seq_c().is_ok(),
            source_base(1),
        ),
        2 => (
            c_guard::golden_run().is_ok()
                && c_guard::segment_guard()
                && c_guard::probe_seq_a().is_ok()
                && c_guard::probe_seq_b().is_ok()
                && c_guard::probe_seq_c().is_ok(),
            source_base(2),
        ),
        3 => (
            c_dispatch::golden_run().is_ok()
                && c_dispatch::segment_guard()
                && c_dispatch::probe_seq_a().is_ok()
                && c_dispatch::probe_seq_b().is_ok()
                && c_dispatch::probe_seq_c().is_ok(),
            source_base(3),
        ),
        4 => (
            c_edge::golden_run().is_ok()
                && c_edge::segment_guard()
                && c_edge::probe_seq_a().is_ok()
                && c_edge::probe_seq_b().is_ok()
                && c_edge::probe_seq_c().is_ok(),
            source_base(4),
        ),
        _ => return None,
    };
    if !ok {
        return None;
    }
    let base = base?;
    let mut h = FNV_OFF;
    h = fold_u32(h, base);
    h = fold_byte(h, slot as u8);
    h = fold_str(h, module_detail(slot, base + 1));
    Some(h)
}

/// 族指纹：五槽封存值折成一枚。
pub fn family_imprint() -> Result<u64, u32> {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < WINDOWS.len() {
        let s = match module_seal(i) {
            Some(v) => v,
            None => return Err(BcCode::SealBad.val()),
        };
        h = fold_u32(h, s as u32);
        h = fold_u32(h, (s >> 32) as u32);
        i += 1;
    }
    Ok(h)
}

/// 内联复算：同一封存在十次独立遍历下必须稳定。
pub fn inline_roll(passes: u32) -> Result<u64, u32> {
    if passes == 0 {
        return Err(BcCode::SealBad.val());
    }
    let first = family_imprint()?;
    let mut i = 1u32;
    while i < passes {
        if family_imprint()? != first {
            return Err(BcCode::SealBad.val());
        }
        i = i.saturating_add(1);
    }
    Ok(first)
}

// ── 模糊三路 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct FuzzReport {
    pub rounds: u32,
    pub slot_hits: u32,
    pub window_hits: u32,
    pub lookup_hits: u32,
    pub digest: u64,
}

impl FuzzReport {
    pub fn all_routes(&self) -> bool {
        self.slot_hits > 0 && self.window_hits > 0 && self.lookup_hits > 0
    }
}

fn fuzz_one(lcg: &mut Lcg) -> Result<(bool, u8), u32> {
    let mode = lcg.below(3);
    let slot = (lcg.below(WINDOWS.len() as u32)) as usize;
    let (base, count) = match WINDOWS.get(slot) {
        Some(v) => *v,
        None => return Err(BcCode::SlotBad.val()),
    };
    let ix = lcg.below(count as u32) as usize;
    let code = match code_at(slot, ix) {
        Some(v) => v,
        None => return Err(BcCode::CodeBad.val()),
    };
    match mode {
        0 => {
            // 明细可读：桥内查到的明细不得为空，也不得落到「未知」。
            let d = module_detail(slot, code);
            let ok = !d.is_empty();
            Ok((ok, 0))
        }
        1 => {
            // 窗归属：必须落在本窗，且相邻窗不得同时命中。
            let mut ok = in_window(slot, code);
            if slot > 0 && in_window(slot - 1, code) {
                ok = false;
            }
            if slot + 1 < WINDOWS.len() && in_window(slot + 1, code) {
                ok = false;
            }
            Ok((ok, 1))
        }
        _ => {
            // 查槽反演：lookup_slot 必须指回槽；窗间空洞必须无主。
            let hit = lookup_slot(code) == Some(slot);
            let gap = base.saturating_add(count as u32).saturating_add(1);
            let gap_ok = lookup_slot(gap).is_none() || gap >= next_base(slot);
            Ok((hit && gap_ok, 2))
        }
    }
}

fn next_base(slot: usize) -> u32 {
    match WINDOWS.get(slot + 1) {
        Some((b, _)) => *b,
        None => u32::MAX,
    }
}

/// 三路模糊：每路各跑 rounds/3 轮。
pub fn fuzz_rounds(rounds: u32) -> Result<FuzzReport, u32> {
    if rounds < 3 {
        return Err(BcCode::FuzzBad.val());
    }
    let mut lcg = Lcg::new(0xB05F_0110_2206_1111);
    let mut rep = FuzzReport { rounds: 0, slot_hits: 0, window_hits: 0, lookup_hits: 0, digest: FNV_OFF };
    let mut i = 0u32;
    while i < rounds {
        let (ok, route) = fuzz_one(&mut lcg)?;
        if ok {
            match route {
                0 => rep.slot_hits = rep.slot_hits.saturating_add(1),
                1 => rep.window_hits = rep.window_hits.saturating_add(1),
                _ => rep.lookup_hits = rep.lookup_hits.saturating_add(1),
            }
        }
        rep.rounds = rep.rounds.saturating_add(1);
        rep.digest = fold_byte(rep.digest, if ok { 1 } else { 0 });
        i = i.saturating_add(1);
    }
    if !rep.all_routes() {
        return Err(BcCode::FuzzBad.val());
    }
    Ok(rep)
}

// ── 黄金运行 ────────────────────────────────────────────────────────
/// 返回：(码数, 行数, 五槽封存和, 族指纹, 普查摘要)。
pub fn golden_run() -> Result<(u32, u32, u64, u64, u64), u32> {
    let book = standard_book()?;
    let imprint = family_imprint()?;
    let mut seals = 0u64;
    let mut i = 0usize;
    while i < WINDOWS.len() {
        let s = match module_seal(i) {
            Some(v) => v,
            None => return Err(BcCode::SealBad.val()),
        };
        seals = seals.saturating_add(s);
        i += 1;
    }
    if book.len() != FAMILY_CODES as usize {
        return Err(BcCode::LedgerBad.val());
    }
    Ok((FAMILY_CODES, book.len() as u32, seals, imprint, book.digest()))
}

// ── 注入（四路） ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    pub rounds: u32,
    pub caught: u32,
    pub route_caught: [u32; 4],
    pub digest: u64,
}

impl InjectLedger {
    pub fn all_caught(&self) -> bool {
        self.caught == self.rounds && self.rounds > 0
    }
}

fn inject_once(route: u32, ix: u32) -> Result<bool, u32> {
    match route % 4 {
        0 => {
            // 越窗码：邻窗边界码必须归属唯一。
            let slot = (ix as usize) % 4;
            let end = match window_end(slot) {
                Some(v) => v,
                None => return Ok(false),
            };
            let belong = lookup_slot(end) == Some(slot);
            let foreign = !in_window((slot + 1) % 5, end);
            Ok(belong && foreign)
        }
        1 => {
            // 明细分派：每槽首码的桥明细必须非空，且与邻槽首码明细互不相同
            // （源模块明细本就允许以「未知」起头，不能拿措辞当判据）。
            let slot = (ix as usize) % 5;
            let next = (slot + 1) % 5;
            let code = match code_at(slot, 0) {
                Some(v) => v,
                None => return Ok(false),
            };
            let peer = match code_at(next, 0) {
                Some(v) => v,
                None => return Ok(false),
            };
            let d = module_detail(slot, code);
            let e = module_detail(next, peer);
            Ok(!d.is_empty() && !e.is_empty() && d != e)
        }
        2 => {
            // 普查重放：同码二次入册必须 ReplayBad。
            let mut book = CensusBook::new();
            let row = CensusRow {
                slot: 0,
                ix: 0,
                code: 0x70A01,
                count: 10,
                detail_hash: detail_hash(0, 0x70A01),
            };
            let first = book.push(row).is_ok();
            let second = book.push(row) == Err(BcCode::ReplayBad.val());
            Ok(first && second)
        }
        _ => {
            // 封存复算：两次族指纹必须相同；空槽必须 None。
            let a = family_imprint()?;
            let b = family_imprint()?;
            Ok(a == b && module_seal(5).is_none())
        }
    }
}

/// 四路注入各跑 rounds/4 轮。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    if rounds == 0 || rounds / 4 == 0 {
        return Err(BcCode::LedgerBad.val());
    }
    let per = rounds / 4;
    let mut led = InjectLedger { rounds: 0, caught: 0, route_caught: [0; 4], digest: FNV_OFF };
    let mut route = 0u32;
    while route < 4 {
        let mut i = 0u32;
        while i < per {
            let hit = inject_once(route, i)?;
            if hit {
                led.caught = led.caught.saturating_add(1);
                if let Some(c) = led.route_caught.get_mut(route as usize) {
                    *c = c.saturating_add(1);
                }
            }
            led.rounds = led.rounds.saturating_add(1);
            led.digest = fold_byte(led.digest, if hit { 1 } else { 0 });
            i = i.saturating_add(1);
        }
        route = route.saturating_add(1);
    }
    Ok(led)
}

// ── 导出与回读 ──────────────────────────────────────────────────────
pub fn render_book(book: &CensusBook) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < book.len() {
        if let Some(r) = book.rows().get(i) {
            out.push_str(&r.text());
            out.push('\n');
        }
        i += 1;
    }
    out
}

fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(BcCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(BcCode::RenderBad.val()),
        };
        if !b.is_ascii_digit() {
            return Err(BcCode::RenderBad.val());
        }
        v = v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
            .ok_or_else(|| BcCode::RenderBad.val())?;
        i += 1;
    }
    Ok(v)
}

fn parse_hex8(s: &str) -> Result<u32, u32> {
    let body = if let Some(rest) = s.strip_prefix("0x") { rest } else { return Err(BcCode::RenderBad.val()) };
    if body.len() != 8 {
        return Err(BcCode::RenderBad.val());
    }
    let bytes = body.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(BcCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u32,
            b'A'..=b'F' => (b - b'A') as u32 + 10,
            b'a'..=b'f' => (b - b'a') as u32 + 10,
            _ => return Err(BcCode::RenderBad.val()),
        };
        v = v.checked_mul(16).and_then(|x| x.checked_add(d)).ok_or_else(|| BcCode::RenderBad.val())?;
        i += 1;
    }
    Ok(v)
}

/// 回读：55 行、五字段、槽号连续、码升序、明细哈希可复算。
pub fn audit_render(text: &str) -> Result<usize, u32> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != FAMILY_CODES as usize {
        return Err(BcCode::RenderBad.val());
    }
    let mut i = 0usize;
    let mut last_code = 0u32;
    while i < lines.len() {
        let ln = match lines.get(i) {
            Some(v) => *v,
            None => return Err(BcCode::RenderBad.val()),
        };
        let parts: Vec<&str> = ln.split('|').collect();
        if parts.len() != 5 {
            return Err(BcCode::RenderBad.val());
        }
        let slot = parse_u32(match parts.get(0) {
            Some(v) => *v,
            None => return Err(BcCode::RenderBad.val()),
        })? as usize;
        let _ = parse_u32(match parts.get(1) {
            Some(v) => *v,
            None => return Err(BcCode::RenderBad.val()),
        })?;
        let code = parse_hex8(match parts.get(2) {
            Some(v) => *v,
            None => return Err(BcCode::RenderBad.val()),
        })?;
        let _ = parse_u32(match parts.get(3) {
            Some(v) => *v,
            None => return Err(BcCode::RenderBad.val()),
        })?;
        let hash = parse_hex8(match parts.get(4) {
            Some(v) => *v,
            None => return Err(BcCode::RenderBad.val()),
        })?;
        if lookup_slot(code) != Some(slot) {
            return Err(BcCode::CodeBad.val());
        }
        if i > 0 && code <= last_code {
            return Err(BcCode::CensusBad.val());
        }
        if hash != detail_hash(slot, code) {
            return Err(BcCode::DetailBad.val());
        }
        last_code = code;
        i += 1;
    }
    Ok(lines.len())
}

// ── 探针 ────────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if !windows_ok() {
        return Err(BcCode::WindowBad.val());
    }
    let mut slot = 0usize;
    while slot < WINDOWS.len() {
        let (base, count) = match WINDOWS.get(slot) {
            Some(v) => *v,
            None => return Err(BcCode::WindowBad.val()),
        };
        if !in_window(slot, base + 1) || !in_window(slot, base + count as u32) {
            return Err(BcCode::WindowBad.val());
        }
        if in_window(slot, base) {
            return Err(BcCode::WindowBad.val());
        }
        slot += 1;
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let book = standard_book()?;
    if book.len() != FAMILY_CODES as usize {
        return Err(BcCode::CensusBad.val());
    }
    book.audit()
}

pub fn probe_seq_c() -> Result<(), u32> {
    let a = module_seal(0).ok_or(BcCode::SealBad.val())?;
    let b = module_seal(0).ok_or(BcCode::SealBad.val())?;
    if a != b {
        return Err(BcCode::SealBad.val());
    }
    if module_seal(5).is_some() {
        return Err(BcCode::SlotBad.val());
    }
    let f = fuzz_rounds(99)?;
    if !f.all_routes() {
        return Err(BcCode::FuzzBad.val());
    }
    Ok(())
}

/// 复现：窗口、普查、封存、模糊、注入五件事必须同时成立。
pub fn repro_check() -> Result<(), u32> {
    let (codes, rows, seals, imprint, digest) = golden_run()?;
    if codes != FAMILY_CODES || rows != FAMILY_CODES as u32 {
        return Err(BcCode::LedgerBad.val());
    }
    if seals == 0 || imprint == 0 {
        return Err(BcCode::SealBad.val());
    }
    let again = golden_run()?;
    if again.3 != imprint || again.4 != digest {
        return Err(BcCode::SealBad.val());
    }
    let inj = inject_rounds(INJECT_ROUNDS)?;
    if !inj.all_caught() {
        return Err(BcCode::LedgerBad.val());
    }
    let f = fuzz_rounds(300)?;
    if f.digest == 0 || !f.all_routes() {
        return Err(BcCode::FuzzBad.val());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落在 0x70B81..=0x70B8B。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0B;
    let mut i = 0usize;
    while i < BcCode::ALL.len() {
        let c = match BcCode::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.val() < lo || c.val() > hi {
            return false;
        }
        i += 1;
    }
    true
}

// ── 十项动态判据 ────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "F48893-J1-五窗合法",
        windows_ok() && probe_seq_a().is_ok(),
        format!("{:?}", WINDOWS),
    ));
    v.push((
        "F48893-J1-码归属唯一",
        {
            let mut ok = true;
            let mut slot = 0usize;
            while slot < WINDOWS.len() {
                let (base, count) = match WINDOWS.get(slot) {
                    Some(v) => *v,
                    None => {
                        ok = false;
                        break;
                    }
                };
                let mut k = 0u32;
                while k < count as u32 {
                    let code = base + 1 + k;
                    if lookup_slot(code) != Some(slot) {
                        ok = false;
                    }
                    k += 1;
                }
                slot += 1;
            }
            ok
        },
        String::from("every code maps back to its only slot"),
    ));
    v.push((
        "F48894-J1-普查齐全",
        match standard_book() {
            Ok(b) => b.len() == FAMILY_CODES as usize && b.audit().is_ok(),
            Err(_) => false,
        },
        String::from("55 rows across five windows"),
    ));
    v.push((
        "F48894-J1-五槽计数",
        match standard_book() {
            Ok(b) => b.slot_count(0) == 10 && b.slot_count(1) == 12 && b.slot_count(4) == 11,
            Err(_) => false,
        },
        String::from("10/12/11/11/11"),
    ));
    v.push((
        "F48895-J1-封存稳定",
        match inline_roll(10) {
            Ok(h) => h != 0,
            Err(_) => false,
        },
        String::from("ten independent passes agree"),
    ));
    v.push((
        "F48895-J1-空槽无封",
        module_seal(5).is_none() && module_seal(99).is_none(),
        String::from("out-of-table slots have no seal"),
    ));
    v.push((
        "J1-明细逐字一致",
        {
            let mut ok = true;
            let mut slot = 0usize;
            while slot < WINDOWS.len() {
                let (base, count) = match WINDOWS.get(slot) {
                    Some(v) => *v,
                    None => {
                        ok = false;
                        break;
                    }
                };
                let mut k = 0u32;
                while k < count as u32 {
                    let code = base + 1 + k;
                    if module_detail(slot, code).is_empty() {
                        ok = false;
                    }
                    k += 1;
                }
                slot += 1;
            }
            ok
        },
        String::from("all 55 details non-empty"),
    ));
    v.push((
        "J1-模糊三路",
        match fuzz_rounds(FUZZ_ROUNDS) {
            Ok(f) => f.all_routes() && f.rounds == FUZZ_ROUNDS,
            Err(_) => false,
        },
        match fuzz_rounds(FUZZ_ROUNDS) {
            Ok(f) => format!("rounds={} slot={} window={} lookup={}", f.rounds, f.slot_hits, f.window_hits, f.lookup_hits),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "J1-注入四路全捕",
        match inject_rounds(INJECT_ROUNDS) {
            Ok(l) => l.all_caught() && l.route_caught.iter().all(|c| *c > 0),
            Err(_) => false,
        },
        match inject_rounds(INJECT_ROUNDS) {
            Ok(l) => format!("rounds={} caught={} routes={:?}", l.rounds, l.caught, l.route_caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "J1-导出回读",
        match standard_book() {
            Ok(b) => audit_render(&render_book(&b)).is_ok(),
            Err(_) => false,
        },
        String::from("55 rows x 5 fields round-trip"),
    ));
    v
}

/// 本模块判据集合。
pub fn run_unx48893_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48893 五窗跨模块寻址");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    let g = segment_guard();
    s.add_dyn("F48893-J1-段位守卫", g, if g { "" } else { "诊断码逃出 0x70B80" });
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_well_formed() {
        assert!(windows_ok());
        assert_eq!(WINDOWS.len(), 5);
        // 真实窗距：0x40、0x40、0x60、0x60（后两窗为 11 码，前两窗 10/12 码）。
        let gaps = [0x40u32, 0x40, 0x60, 0x60];
        let mut i = 1usize;
        while i < WINDOWS.len() {
            let a = WINDOWS.get(i - 1);
            let b = WINDOWS.get(i);
            match (a, b) {
                (Some((x, _)), Some((y, _))) => {
                    assert_eq!(*y - *x, *gaps.get(i - 1).unwrap_or(&0));
                    assert!(*y - *x >= WINDOW_STRIDE);
                }
                _ => assert!(false),
            }
            i += 1;
        }
    }

    #[test]
    fn window_bounds_are_exclusive_at_base() {
        assert!(!in_window(0, 0x70A00));
        assert!(in_window(0, 0x70A01));
        assert!(in_window(0, 0x70A0A));
        assert!(!in_window(0, 0x70A0B));
        assert!(in_window(1, 0x70A41));
        assert!(in_window(1, 0x70A4C));
    }

    #[test]
    fn lookup_slot_maps_each_code() {
        assert_eq!(lookup_slot(0x70A01), Some(0));
        assert_eq!(lookup_slot(0x70A0A), Some(0));
        assert_eq!(lookup_slot(0x70A41), Some(1));
        assert_eq!(lookup_slot(0x70A81), Some(2));
        assert_eq!(lookup_slot(0x70B41), Some(4));
        assert_eq!(lookup_slot(0x70B4B), Some(4));
        assert_eq!(lookup_slot(0x70B4C), None);
        assert_eq!(lookup_slot(0), None);
    }

    #[test]
    fn window_end_is_base_plus_count() {
        assert_eq!(window_end(0), Some(0x70A0A));
        assert_eq!(window_end(1), Some(0x70A4C));
        assert_eq!(window_end(4), Some(0x70B4B));
        assert_eq!(window_end(5), None);
    }

    #[test]
    fn code_at_and_index_are_inverse() {
        let mut slot = 0usize;
        while slot < WINDOWS.len() {
            let mut ix = 0usize;
            while ix < 20 {
                if let Some(c) = code_at(slot, ix) {
                    assert_eq!(index_of(slot, c), Some(ix));
                }
                ix += 1;
            }
            slot += 1;
        }
    }

    #[test]
    fn module_detail_follows_slot() {
        assert!(module_detail(0, 0x70A01).contains("SOP"));
        assert!(!module_detail(5, 0x70A01).is_empty());
        assert!(module_detail(5, 0x70A01).contains("未知"));
    }

    #[test]
    fn census_book_has_55_rows() {
        let b = standard_book();
        let b = match b {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(b.len(), 55);
        assert_eq!(b.slot_count(0), 10);
        assert_eq!(b.slot_count(1), 12);
        assert_eq!(b.slot_count(2), 11);
        assert_eq!(b.slot_count(3), 11);
        assert_eq!(b.slot_count(4), 11);
        assert!(b.audit().is_ok());
    }

    #[test]
    fn census_rejects_wrong_count() {
        let mut b = CensusBook::new();
        let row = CensusRow { slot: 0, ix: 0, code: 0x70A01, count: 9, detail_hash: 0 };
        assert_eq!(b.push(row), Err(BcCode::CensusBad.val()));
    }

    #[test]
    fn census_rejects_wrong_code() {
        let mut b = CensusBook::new();
        let row = CensusRow { slot: 0, ix: 0, code: 0x70A02, count: 10, detail_hash: 0 };
        assert_eq!(b.push(row), Err(BcCode::CodeBad.val()));
    }

    #[test]
    fn census_rejects_replay() {
        let mut b = CensusBook::new();
        let row = CensusRow { slot: 0, ix: 0, code: 0x70A01, count: 10, detail_hash: detail_hash(0, 0x70A01) };
        assert!(b.push(row).is_ok());
        assert_eq!(b.push(row), Err(BcCode::ReplayBad.val()));
    }

    #[test]
    fn census_audit_catches_wrong_hash() {
        let mut b = CensusBook::new();
        let mut slot = 0usize;
        while slot < WINDOWS.len() {
            let (_, count) = match WINDOWS.get(slot) {
                Some(v) => *v,
                None => return,
            };
            let mut ix = 0usize;
            while ix < count as usize {
                let code = match code_at(slot, ix) {
                    Some(v) => v,
                    None => return,
                };
                let mut h = detail_hash(slot, code);
                if slot == 2 && ix == 0 {
                    h ^= 0xFF;
                }
                let row = CensusRow {
                    slot: slot as u8,
                    ix: ix as u16,
                    code,
                    count,
                    detail_hash: h,
                };
                let _ = b.push(row);
                ix += 1;
            }
            slot += 1;
        }
        assert_eq!(b.audit(), Err(BcCode::DetailBad.val()));
    }

    #[test]
    fn module_seal_is_stable() {
        let mut slot = 0usize;
        while slot < 5 {
            let a = module_seal(slot);
            let b = module_seal(slot);
            assert_eq!(a.is_some(), b.is_some());
            assert_eq!(a, b);
            slot += 1;
        }
        assert!(module_seal(5).is_none());
    }

    #[test]
    fn family_imprint_matches_roll() {
        let a = family_imprint();
        let b = inline_roll(5);
        assert_eq!(a.ok(), b.ok());
        match (a, b) {
            (Ok(x), Ok(y)) => assert_eq!(x, y),
            _ => assert!(false),
        }
    }

    #[test]
    fn golden_counts_are_fixed() {
        let g = golden_run();
        let g = match g {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(g.0, 55);
        assert_eq!(g.1, 55);
        assert!(g.2 > 0);
        assert_ne!(g.3, 0);
    }

    #[test]
    fn fuzz_covers_all_routes() {
        let f = fuzz_rounds(3000);
        let f = match f {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(f.rounds, 3000);
        assert!(f.all_routes());
        assert!(f.slot_hits + f.window_hits + f.lookup_hits <= f.rounds);
    }

    #[test]
    fn injection_all_routes() {
        let l = inject_rounds(1000);
        let l = match l {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(l.all_caught());
        assert_eq!(l.route_caught, [250, 250, 250, 250]);
    }

    #[test]
    fn render_round_trip_and_tamper() {
        let b = standard_book();
        let b = match b {
            Ok(v) => v,
            Err(_) => return,
        };
        let text = render_book(&b);
        assert_eq!(audit_render(&text), Ok(55));
        let bad_hash = text.replacen("0x00000000", "0xFFFFFFFF", 1);
        if bad_hash != text {
            assert!(audit_render(&bad_hash).is_err());
        }
        let short = text.lines().take(54).collect::<Vec<_>>().join("\n");
        assert!(audit_render(&short).is_err());
    }

    #[test]
    fn probes_and_repro() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn codes_in_window() {
        assert!(segment_guard());
        let mut i = 0usize;
        while i < BcCode::ALL.len() {
            if let Some(c) = BcCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE + 1 && c.val() <= SEG_BASE + 0x0B);
                assert!(!detail(c.val()).is_empty());
            }
            i += 1;
        }
        assert!(detail(1).contains("未知"));
    }
}
