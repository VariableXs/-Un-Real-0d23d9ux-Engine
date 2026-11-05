//! UNX-F9248 madvise 错误矩阵（EINVAL/ENOMEM 联 B17） · 0x7AB80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9248 逐字）：
//!   UNX-F9248-J1 未知 advice 全 EINVAL 判据过，越界区间全 ENOMEM 格过，
//!   与 B17 矩阵零重复分层声明格过。
//!
//! 设计要点：
//!   ① advice 走闭集解析：原始 u32 先经 `Advice::from_raw` 映射，表外值一律
//!      EINVAL。未知 advice 的十格注入值故意覆盖 Linux 预留空洞（5/6/7/18/19/
//!      24/25）与越界大值，验证「闭集之外没有隐式别名」。
//!   ② 区间错误全部归 ENOMEM：起点早于区间基址、起点落在区间末尾、长度跨出
//!      上界、长度加法溢出，四类都回 ENOMEM 而不是 EINVAL。Linux 的 madvise
//!      在 VMA 未覆盖时同样回 ENOMEM，语义必须与参数面分开。
//!   ③ 判定次序：未知 advice → 对齐 → 零长度 → 区间越界 → 页洞。未知 advice
//!      即使同时越界也必须回 EINVAL，参数面永远先于区间面。
//!   ④ 状态机：FREE 摘除映射位并清回 Normal，后续任何 advice 命中该页都回
//!      ENOMEM；DONTNEED/WILLNEED 只累计计数，不改变映射位。计数只增不减。
//!   ⑤ 与 B17 分层声明：B17 拥有 mmap/munmap/brk/mremap 映射面，F9248 只拥有
//!      advice 集合面、区间错误面与未知 advice 面；surface 两两互斥。
//!   ⑥ 诊断码段内闭集 12 个（+0x01..=+0x0C），码值唯一，每码带中文建议。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9248 独占 0x7AB80..=0x7ABBF）。
pub const SEG_BASE: u32 = 0x7AB80;
/// 4K 页大小。
pub const PAGE: u64 = 4096;
/// 语料区间基址（16 页，页对齐）。
pub const REGION_BASE: u64 = 0x0000_0000_4000_0000;
/// 语料区间页数。
pub const REGION_PAGES: usize = 16;
/// 语料区间上界（不含）。
pub const REGION_END: u64 = REGION_BASE + PAGE * REGION_PAGES as u64;
/// 单格注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;
/// 未知 advice 注入格数。
pub const UNKNOWN_CELLS: usize = 10;
/// 越界区间注入类别数。
pub const RANGE_CLASSES: u32 = 4;
/// Linux errno 数值（正数口径）。
pub const ENOMEM: i32 = 12;
pub const EINVAL: i32 = 22;
pub const ENOSYS: i32 = 38;

// ── advice 闭集 ───────────────────────────────────────────────────────
/// Linux madvise advice 的闭集模型（只纳入本模块认领的 20 个值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Advice {
    Normal,
    Random,
    Sequential,
    WillNeed,
    DontNeed,
    Free,
    Remove,
    DontFork,
    DoFork,
    Mergeable,
    Unmergeable,
    HugePage,
    NoHugePage,
    DontDump,
    DoDump,
    Cold,
    PageOut,
    PopulateRead,
    PopulateWrite,
    GuardInstall,
}

impl Advice {
    /// 原始值 → 闭集；表外返回 None（调用方转 EINVAL）。
    pub fn from_raw(raw: u32) -> Option<Advice> {
        match raw {
            0 => Some(Advice::Normal),
            1 => Some(Advice::Random),
            2 => Some(Advice::Sequential),
            3 => Some(Advice::WillNeed),
            4 => Some(Advice::DontNeed),
            8 => Some(Advice::Free),
            9 => Some(Advice::Remove),
            10 => Some(Advice::DontFork),
            11 => Some(Advice::DoFork),
            12 => Some(Advice::Mergeable),
            13 => Some(Advice::Unmergeable),
            14 => Some(Advice::HugePage),
            15 => Some(Advice::NoHugePage),
            16 => Some(Advice::DontDump),
            17 => Some(Advice::DoDump),
            20 => Some(Advice::Cold),
            21 => Some(Advice::PageOut),
            22 => Some(Advice::PopulateRead),
            23 => Some(Advice::PopulateWrite),
            102 => Some(Advice::GuardInstall),
            _ => None,
        }
    }

    /// 闭集值 → 原始值（与 from_raw 严格互逆）。
    pub fn raw(self) -> u32 {
        match self {
            Advice::Normal => 0,
            Advice::Random => 1,
            Advice::Sequential => 2,
            Advice::WillNeed => 3,
            Advice::DontNeed => 4,
            Advice::Free => 8,
            Advice::Remove => 9,
            Advice::DontFork => 10,
            Advice::DoFork => 11,
            Advice::Mergeable => 12,
            Advice::Unmergeable => 13,
            Advice::HugePage => 14,
            Advice::NoHugePage => 15,
            Advice::DontDump => 16,
            Advice::DoDump => 17,
            Advice::Cold => 20,
            Advice::PageOut => 21,
            Advice::PopulateRead => 22,
            Advice::PopulateWrite => 23,
            Advice::GuardInstall => 102,
        }
    }

    /// 是否销毁映射（FREE 是唯一会摘除映射位的 advice）。
    pub fn destroys(self) -> bool {
        matches!(self, Advice::Free)
    }

    /// 闭集全量（解析互逆性检查使用）。
    pub const ALL: [Advice; 20] = [
        Advice::Normal,
        Advice::Random,
        Advice::Sequential,
        Advice::WillNeed,
        Advice::DontNeed,
        Advice::Free,
        Advice::Remove,
        Advice::DontFork,
        Advice::DoFork,
        Advice::Mergeable,
        Advice::Unmergeable,
        Advice::HugePage,
        Advice::NoHugePage,
        Advice::DontDump,
        Advice::DoDump,
        Advice::Cold,
        Advice::PageOut,
        Advice::PopulateRead,
        Advice::PopulateWrite,
        Advice::GuardInstall,
    ];
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MadvCode {
    UnknownAdvice,
    UnalignedStart,
    ZeroLength,
    RangeBeyond,
    HoleInRange,
    StateIllegal,
    LayeringOverlap,
    LedgerRegress,
    NotReproducible,
    SegmentViolation,
    CapacityExceeded,
    AdviceApplied,
}

impl MadvCode {
    /// 码值（段基址 + 1 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MadvCode::UnknownAdvice => "未知 advice 被拒：原始值不在闭集表内（含预留空洞与越界大值）；请改用 20 个受支持值之一，回 EINVAL",
            MadvCode::UnalignedStart => "起始地址未对齐被拒：madvise 要求 start 按 4K 页对齐；请先 page_align_down 再提交",
            MadvCode::ZeroLength => "零长度被拒：len 必须大于 0；请给出至少一页的建议区间",
            MadvCode::RangeBeyond => "区间越界被拒：start..start+len 超出语料区间或加法溢出；请裁剪到区间内，回 ENOMEM",
            MadvCode::HoleInRange => "区间含未映射页：范围内至少一页已被 FREE 摘除；请按连续映射重新截断区间",
            MadvCode::StateIllegal => "状态迁移非法：advice 与当前页状态冲突；请先恢复映射再重试",
            MadvCode::LayeringOverlap => "分层声明重复：同一 advice 面被两个域认领；请把重复项归还唯一属主",
            MadvCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查回滚路径是否误改累计值",
            MadvCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态或非确定哈希",
            MadvCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7A000..=0x7D1C0 且 0x40 对齐",
            MadvCode::CapacityExceeded => "区间容量超限：页索引越过 16 页语料区；请缩小范围或扩充语料区",
            MadvCode::AdviceApplied => "advice 已生效：页级建议位写回成功，计数按页累加",
        }
    }

    /// 闭集全量（文案与唯一性检查使用）。
    pub const ALL: [MadvCode; 12] = [
        MadvCode::UnknownAdvice,
        MadvCode::UnalignedStart,
        MadvCode::ZeroLength,
        MadvCode::RangeBeyond,
        MadvCode::HoleInRange,
        MadvCode::StateIllegal,
        MadvCode::LayeringOverlap,
        MadvCode::LedgerRegress,
        MadvCode::NotReproducible,
        MadvCode::SegmentViolation,
        MadvCode::CapacityExceeded,
        MadvCode::AdviceApplied,
    ];
}

// ── 页级建议模型 ──────────────────────────────────────────────────────
/// 定容页表：16 个映射位 + 逐页 advice 位 + 四个单调计数。
#[derive(Clone, Copy, Debug)]
pub struct AdviceSpace {
    mapped: u32,
    advice: [Advice; REGION_PAGES],
    willneed: u32,
    dontneed: u32,
    frees: u32,
    applies: u32,
}

impl AdviceSpace {
    /// 全 16 页已映射、advice 全 Normal。
    pub const fn full() -> AdviceSpace {
        AdviceSpace {
            mapped: 0xFFFF,
            advice: [Advice::Normal; REGION_PAGES],
            willneed: 0,
            dontneed: 0,
            frees: 0,
            applies: 0,
        }
    }

    /// 全部摘除（用于空洞语料）。
    pub const fn empty() -> AdviceSpace {
        AdviceSpace {
            mapped: 0,
            advice: [Advice::Normal; REGION_PAGES],
            willneed: 0,
            dontneed: 0,
            frees: 0,
            applies: 0,
        }
    }

    pub fn mapped_mask(&self) -> u32 {
        self.mapped
    }
    pub fn willneed(&self) -> u32 {
        self.willneed
    }
    pub fn dontneed(&self) -> u32 {
        self.dontneed
    }
    pub fn frees(&self) -> u32 {
        self.frees
    }
    pub fn applies(&self) -> u32 {
        self.applies
    }

    /// 页下标 → 该页 advice。
    pub fn advice_at(&self, idx: usize) -> Option<Advice> {
        self.advice.get(idx).copied()
    }

    /// 地址 → 页下标（只在语料区间内成功）。
    pub fn page_of(&self, addr: u64) -> Option<usize> {
        if addr < REGION_BASE || addr >= REGION_END {
            return None;
        }
        let off = addr - REGION_BASE;
        if off % PAGE != 0 {
            return None;
        }
        Some((off / PAGE) as usize)
    }

    /// 区间的页数（len 上取整到页）。
    fn pages_of(len: u64) -> Option<u64> {
        let mask = PAGE - 1;
        len.checked_add(mask).map(|v| v / PAGE)
    }

    /// Linux madvise 判据模型。成功返回生效页数。
    pub fn advise(&mut self, start: u64, len: u64, raw: u32) -> Result<u64, i32> {
        // ① 未知 advice 最先判：即使区间越界也回 EINVAL
        let adv = match Advice::from_raw(raw) {
            Some(a) => a,
            None => return Err(EINVAL),
        };
        // ② 对齐
        if start % PAGE != 0 {
            return Err(EINVAL);
        }
        // ③ 零长度
        if len == 0 {
            return Err(EINVAL);
        }
        // ④ 区间加法与上界
        let pages = match Self::pages_of(len) {
            Some(v) => v,
            None => return Err(ENOMEM),
        };
        let bytes = match pages.checked_mul(PAGE) {
            Some(v) => v,
            None => return Err(ENOMEM),
        };
        let end = match start.checked_add(bytes) {
            Some(v) => v,
            None => return Err(ENOMEM),
        };
        if start < REGION_BASE || end > REGION_END {
            return Err(ENOMEM);
        }
        let first = match self.page_of(start) {
            Some(i) => i,
            None => return Err(ENOMEM),
        };
        let count = match usize::try_from(pages) {
            Ok(v) => v,
            Err(_) => return Err(ENOMEM),
        };
        // ⑤ 范围内不得有页洞
        let mut i = 0usize;
        while i < count {
            let idx = match first.checked_add(i) {
                Some(v) => v,
                None => return Err(ENOMEM),
            };
            let bit = match 1u32.checked_shl(idx as u32) {
                Some(v) => v,
                None => return Err(ENOMEM),
            };
            if idx >= REGION_PAGES || self.mapped & bit == 0 {
                return Err(ENOMEM);
            }
            i += 1;
        }
        // ⑥ 应用 advice
        let mut j = 0usize;
        while j < count {
            let idx = match first.checked_add(j) {
                Some(v) => v,
                None => return Err(ENOMEM),
            };
            if let Some(slot) = self.advice.get_mut(idx) {
                *slot = adv;
            }
            if adv.destroys() {
                if let Some(bit) = 1u32.checked_shl(idx as u32) {
                    self.mapped &= !bit;
                }
            }
            j += 1;
        }
        self.applies = self.applies.saturating_add(pages as u32);
        match adv {
            Advice::WillNeed => self.willneed = self.willneed.saturating_add(pages as u32),
            Advice::DontNeed => self.dontneed = self.dontneed.saturating_add(pages as u32),
            Advice::Free => self.frees = self.frees.saturating_add(pages as u32),
            _ => {}
        }
        Ok(pages)
    }
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账本：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(EINVAL);
        }
        Ok(())
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 未知 advice 注入格：覆盖预留空洞、负值邻域与超大值。
pub const UNKNOWN_RAW: [u32; UNKNOWN_CELLS] =
    [5, 6, 7, 18, 19, 24, 25, 100, 101, 0xFFFF_FFFF];

/// 未知 advice 十格矩阵：每格 10 轮，全部 EINVAL。
pub fn run_unknown_matrix() -> ([u32; UNKNOWN_CELLS], InjectLedger) {
    let mut space = AdviceSpace::full();
    let mut ledger = InjectLedger::zero();
    let mut per_cell = [0u32; UNKNOWN_CELLS];
    let mut ci = 0usize;
    while ci < UNKNOWN_RAW.len() {
        if let Some(raw) = UNKNOWN_RAW.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = space.advise(REGION_BASE, PAGE, raw) == Err(EINVAL);
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_cell.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (per_cell, ledger)
}

/// 越界区间注入类别：起点前置、起点在上界、长度跨界、长度溢出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RangeCase {
    BeforeBase,
    AtEnd,
    CrossEnd,
    LenOverflow,
}

impl RangeCase {
    /// 样本参数 (start, len)。
    pub fn args(self) -> (u64, u64) {
        match self {
            RangeCase::BeforeBase => (REGION_BASE - PAGE, PAGE),
            RangeCase::AtEnd => (REGION_END, PAGE),
            RangeCase::CrossEnd => (REGION_END - PAGE, PAGE * 2),
            RangeCase::LenOverflow => (REGION_BASE, u64::MAX),
        }
    }
    /// 闭集全量。
    pub const ALL: [RangeCase; 4] = [
        RangeCase::BeforeBase,
        RangeCase::AtEnd,
        RangeCase::CrossEnd,
        RangeCase::LenOverflow,
    ];
}

/// 越界区间四类 × 10 轮，全部 ENOMEM。
pub fn run_range_matrix() -> ([u32; 4], InjectLedger) {
    let mut space = AdviceSpace::full();
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < RangeCase::ALL.len() {
        if let Some(case) = RangeCase::ALL.get(ci).copied() {
            let (start, len) = case.args();
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = space.advise(start, len, 0) == Err(ENOMEM);
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (per_class, ledger)
}

// ── 与 B17 的分层声明 ────────────────────────────────────────────────
/// 一条分层声明：属主 + 独占面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayeringClaim {
    pub owner: &'static str,
    pub surface: &'static str,
}

/// 冻结声明表：B17 管映射面，F9248 只管 advice 面。
pub const LAYERING: &[LayeringClaim] = &[
    LayeringClaim { owner: "B17", surface: "mmap-munmap-basic" },
    LayeringClaim { owner: "B17", surface: "brk-grow-shrink" },
    LayeringClaim { owner: "B17", surface: "mremap-move" },
    LayeringClaim { owner: "F9248", surface: "madvise-advice-set" },
    LayeringClaim { owner: "F9248", surface: "madvise-range-error" },
    LayeringClaim { owner: "F9248", surface: "madvise-unknown-advice" },
];

/// 声明表零重复：surface 两两不同，属主只能是 B17/F9248。
pub fn layering_disjoint() -> bool {
    let mut i = 0usize;
    while i < LAYERING.len() {
        if let Some(a) = LAYERING.get(i) {
            if a.owner != "B17" && a.owner != "F9248" {
                return false;
            }
            if a.surface.is_empty() {
                return false;
            }
            let mut j = i + 1;
            while j < LAYERING.len() {
                if let Some(b) = LAYERING.get(j) {
                    if a.surface == b.surface {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    true
}

/// 统计某属主的声明条数。
pub fn claims_owned_by(owner: &str) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < LAYERING.len() {
        if let Some(c) = LAYERING.get(i) {
            if c.owner == owner {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOMEM", -12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；内存不足族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ENOSYS", -38, 1, "Windows ERROR_INVALID_FUNCTION；未实现族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 编码一次 advice 结果。
fn encode(out: &mut Vec<u8>, r: Result<u64, i32>) {
    match r {
        Ok(v) => {
            out.push(1);
            out.push((v & 0xFF) as u8);
        }
        Err(e) => {
            out.push(2);
            out.push((e & 0x7F) as u8);
        }
    }
}

/// 规范语料双跑指纹：正常建议、未知值、越界、页洞、FREE 后重用。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut sp = AdviceSpace::full();
    encode(&mut bytes, sp.advise(REGION_BASE, PAGE, 3));
    encode(&mut bytes, sp.advise(REGION_BASE, PAGE, 4));
    encode(&mut bytes, sp.advise(REGION_BASE, PAGE, 5));
    encode(&mut bytes, sp.advise(REGION_BASE + 1, PAGE, 3));
    encode(&mut bytes, sp.advise(REGION_END - PAGE, PAGE * 2, 3));
    encode(&mut bytes, sp.advise(REGION_BASE, PAGE, 8));
    encode(&mut bytes, sp.advise(REGION_BASE, PAGE, 3));
    encode(&mut bytes, sp.advise(REGION_BASE + PAGE, PAGE, 102));
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9248 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9248_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9248");

    let seg_ok = SEG_BASE % 0x40 == 0
        && (0x7A000..=0x7D1C0).contains(&SEG_BASE)
        && SEG_BASE > 0x7AB40;
    s.add("段位守卫：0x40 对齐且不与 F9247 重叠", seg_ok,
          "段基址唯一且落在本批窗口，段间不得互相覆盖");

    let mut sp = AdviceSpace::full();
    let will = sp.advise(REGION_BASE, PAGE * 3, Advice::WillNeed.raw());
    let will_ok = will == Ok(3) && sp.willneed() == 3 && sp.applies() == 3
        && sp.advice_at(0) == Some(Advice::WillNeed);
    s.add("正常流：WILLNEED 3 页生效并写回建议位", will_ok,
          "成功路径必须按页写回 advice 并累加类型计数");

    let mut dn = AdviceSpace::full();
    let d1 = dn.advise(REGION_BASE + PAGE, PAGE * 2, Advice::DontNeed.raw());
    let d2 = dn.advise(REGION_BASE, PAGE, Advice::DontNeed.raw());
    s.add("正常流：DONTNEED 分段累计单调",
          d1 == Ok(2) && d2 == Ok(1) && dn.dontneed() == 3,
          "同类型建议的分段调用必须累加，不得覆盖旧值");

    let all_parse = Advice::ALL.iter().all(|a| Advice::from_raw(a.raw()) == Some(*a));
    s.add("闭集：20 个 advice 原始值解析互逆", all_parse,
          "raw 与 from_raw 必须严格互逆，闭集之外不得出现别名");

    let (per_cell, u_ledger) = run_unknown_matrix();
    s.add("注入：未知 advice 10 格各 10/10 全 EINVAL",
          per_cell.iter().all(|n| *n == INJECT_ROUNDS),
          "预留空洞与越界大值必须全部回 EINVAL");
    s.add("注入账：未知面 100 次全检出且逃逸为 0",
          u_ledger.conserved() && u_ledger.escaped == 0
              && u_ledger.injected == UNKNOWN_CELLS as u32 * INJECT_ROUNDS,
          "100 次注入必须 100 次检出，不得串到区间面");

    let (per_class, r_ledger) = run_range_matrix();
    s.add("注入：越界区间 4 类各 10/10 全 ENOMEM",
          per_class.iter().all(|n| *n == INJECT_ROUNDS),
          "前置/末尾/跨界/溢出四类必须全部回 ENOMEM");
    s.add("注入账：越界面 40 次全检出且逃逸为 0",
          r_ledger.conserved() && r_ledger.escaped == 0
              && r_ledger.injected == RANGE_CLASSES * INJECT_ROUNDS,
          "40 次注入必须 40 次检出，ENOMEM 不得混入 EINVAL");

    let mut order = AdviceSpace::full();
    let order_ok = order.advise(REGION_END + PAGE, PAGE, 5) == Err(EINVAL)
        && order.advise(REGION_END + PAGE, PAGE, 0) == Err(ENOMEM);
    s.add("判定次序：未知 advice EINVAL 先于区间 ENOMEM", order_ok,
          "参数面错误必须先于区间面错误返回");

    let mut param = AdviceSpace::full();
    let param_ok = param.advise(REGION_BASE + 1, PAGE, 0) == Err(EINVAL)
        && param.advise(REGION_BASE, 0, 0) == Err(EINVAL);
    s.add("参数面：未对齐/零长度全 EINVAL", param_ok,
          "对齐与长度校验先于区间覆盖校验");

    let mut hole = AdviceSpace::full();
    let _ = hole.advise(REGION_BASE + PAGE * 5, PAGE, Advice::Free.raw());
    let hole_ok = hole.advise(REGION_BASE, PAGE * 16, Advice::Normal.raw()) == Err(ENOMEM)
        && AdviceSpace::empty().advise(REGION_BASE, PAGE, 0) == Err(ENOMEM);
    s.add("页洞：FREE 摘页后整段建议回 ENOMEM", hole_ok,
          "范围中任一页被摘除，整段建议都不得部分生效");

    let mut freed = AdviceSpace::full();
    let f1 = freed.advise(REGION_BASE, PAGE, Advice::Free.raw());
    let f2 = freed.advise(REGION_BASE, PAGE, Advice::Normal.raw());
    let f_ok = f1 == Ok(1)
        && f2 == Err(ENOMEM)
        && freed.frees() == 1
        && freed.mapped_mask() & 1 == 0;
    s.add("状态机：FREE 后同页建议 ENOMEM 且映射位清零", f_ok,
          "FREE 必须摘除映射位，后续建议不得命中已释放页");

    let lay_ok = layering_disjoint()
        && claims_owned_by("B17") == 3
        && claims_owned_by("F9248") == 3
        && claims_owned_by("B17") + claims_owned_by("F9248") == LAYERING.len() as u32;
    s.add("对账：B17 分层声明零重复且属主完整", lay_ok,
          "每个 surface 只能有一个属主，两域条数之和等于总表");

    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOMEM)
        && agrees_with_linux(ENOSYS)
        && linux_neg(ENOMEM) == -ENOMEM
        && !agrees_with_linux(0);
    s.add("Linux 逐码一致：EINVAL/ENOMEM 表内可复算", codes_ok,
          "错误码只允许 errno 表内正值，表外码一律拒绝");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    s.add("文案闭集：12 个诊断码均有中文建议",
          MadvCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");

    let mut uniq = true;
    let mut i = 0usize;
    while i < MadvCode::ALL.len() {
        let mut j = i + 1;
        while j < MadvCode::ALL.len() {
            if let (Some(a), Some(b)) = (MadvCode::ALL.get(i), MadvCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    let mut mono = r_ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    let u_text = format!(
        "未知注入 {} / 检出 {} / 逃逸 {}",
        u_ledger.injected, u_ledger.detected, u_ledger.escaped
    );
    s.add_dyn("未知账目", u_ledger.conserved(), &u_text);
    let r_text = format!(
        "越界注入 {} / 检出 {} / 逃逸 {}",
        r_ledger.injected, r_ledger.detected, r_ledger.escaped
    );
    s.add_dyn("越界账目", r_ledger.conserved(), &r_text);
    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn willneed_applies() {
        let mut sp = AdviceSpace::full();
        assert_eq!(sp.advise(REGION_BASE, PAGE * 3, 3), Ok(3));
        assert_eq!(sp.willneed(), 3);
        assert_eq!(sp.advice_at(2), Some(Advice::WillNeed));
    }

    #[test]
    fn unknown_advice_einval() {
        let mut sp = AdviceSpace::full();
        assert_eq!(sp.advise(REGION_BASE, PAGE, 5), Err(EINVAL));
        assert_eq!(sp.advise(REGION_BASE, PAGE, 0xFFFF_FFFF), Err(EINVAL));
    }

    #[test]
    fn range_beyond_enomem() {
        let mut sp = AdviceSpace::full();
        assert_eq!(sp.advise(REGION_END, PAGE, 0), Err(ENOMEM));
        assert_eq!(sp.advise(REGION_BASE - PAGE, PAGE, 0), Err(ENOMEM));
        assert_eq!(sp.advise(REGION_BASE, u64::MAX, 0), Err(ENOMEM));
    }

    #[test]
    fn unaligned_and_zero_einval() {
        let mut sp = AdviceSpace::full();
        assert_eq!(sp.advise(REGION_BASE + 1, PAGE, 0), Err(EINVAL));
        assert_eq!(sp.advise(REGION_BASE, 0, 0), Err(EINVAL));
    }

    #[test]
    fn hole_and_free_state() {
        let mut sp = AdviceSpace::full();
        assert_eq!(sp.advise(REGION_BASE + PAGE * 5, PAGE, 8), Ok(1));
        assert_eq!(sp.advise(REGION_BASE, PAGE * 16, 0), Err(ENOMEM));
        assert_eq!(sp.advise(REGION_BASE + PAGE * 5, PAGE, 0), Err(ENOMEM));
        assert_eq!(sp.mapped_mask() & (1 << 5), 0);
    }

    #[test]
    fn order_unknown_before_range() {
        let mut sp = AdviceSpace::full();
        assert_eq!(sp.advise(REGION_END + PAGE, PAGE, 5), Err(EINVAL));
        assert_eq!(sp.advise(REGION_END + PAGE, PAGE, 0), Err(ENOMEM));
    }

    #[test]
    fn closed_set_roundtrip() {
        assert_eq!(Advice::ALL.len(), 20);
        for a in Advice::ALL.iter() {
            assert_eq!(Advice::from_raw(a.raw()), Some(*a));
        }
        assert_eq!(Advice::from_raw(5), None);
    }

    #[test]
    fn matrices_conserved() {
        let (cells, l1) = run_unknown_matrix();
        assert!(l1.conserved());
        assert_eq!(l1.escaped, 0);
        assert_eq!(l1.injected, 100);
        for n in cells.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
        let (classes, l2) = run_range_matrix();
        assert!(l2.conserved());
        assert_eq!(l2.escaped, 0);
        for n in classes.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9248_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
