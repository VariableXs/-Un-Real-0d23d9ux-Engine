//! UNX-F50203 副本折算账（copies × 页数折算零重复）（0x43fc0 段）
//!
//! 锚点：UNX-F50203-J1 折算 ×20 与实际页序勾稽；判据锚定 §AI-63 §2 专题十。
//!
//! 「复印 3 份」在账上不是一次加法，而是一次**展开**：3 份 × 10 页 = 30 个需要成像
//! 的页位。展开方式有两种，账必须知道自己是哪一种：
//!
//! * [`FoldMode::Stacked`]（叠份）—— 先整份印完再印下一份：`ordinal = copy*P + page`；
//! * [`FoldMode::Collated`]（配页）—— 每页先配齐所有份再进下一页：`ordinal = page*C + copy`。
//!
//! 两种顺序的**总量相同**（`P*C` 个页位），但页序完全不同；把配页任务按叠份记账，
//! 中途取消时「已印到哪一页」就会算错。因此本模块把折算定义成**双射**并要求：
//!
//! 1. 正向 `ordinal → (copy, page)` 与反向 `(copy, page) → ordinal` 互逆；
//! 2. `0..P*C` 的每个序号恰好出现一次（零重复、零空洞）；
//! 3. 纸张序（`sheet_ordinal`）同样是双射：叠份按份分组，配页按「套」分组；
//! 4. 双面时每份纸张数 = `ceil(P/2)`，单面 = `P`，总量按份数等比放大。
//!
//! 诊断码闭集：`SEG_BASE + 1 ..= SEG_BASE + 11`。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x43fc0 独占 0x40）。
pub const SEG_BASE: u32 = 0x43fc0;

/// 折算用例数（卡面 20 条）。
pub const CASE_COUNT: usize = 20;

/// 单用例页数上限（样本不超过 16 页，保证展开表可穷举）。
pub const MAX_PAGES: u8 = 16;

/// 单用例份数上限。
pub const MAX_COPIES: u8 = 8;

/// 诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldCode {
    CaseCountDrift,
    TotalDrift,
    ForwardGap,
    ReverseMismatch,
    DuplicateOrdinal,
    SheetCountDrift,
    SheetBijection,
    ModeOrderDrift,
    ZeroGuard,
    DigestDrift,
    Unknown,
}

impl FoldCode {
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                FoldCode::CaseCountDrift => 1,
                FoldCode::TotalDrift => 2,
                FoldCode::ForwardGap => 3,
                FoldCode::ReverseMismatch => 4,
                FoldCode::DuplicateOrdinal => 5,
                FoldCode::SheetCountDrift => 6,
                FoldCode::SheetBijection => 7,
                FoldCode::ModeOrderDrift => 8,
                FoldCode::ZeroGuard => 9,
                FoldCode::DigestDrift => 10,
                FoldCode::Unknown => 11,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            FoldCode::CaseCountDrift => "折算用例数与冻结样本数不等",
            FoldCode::TotalDrift => "总页位不等于 页数 × 份数",
            FoldCode::ForwardGap => "正向展开出现空洞或 None",
            FoldCode::ReverseMismatch => "反查序号与正向展开不互逆",
            FoldCode::DuplicateOrdinal => "同页位被展开两次，存在重复计费",
            FoldCode::SheetCountDrift => "纸张总数与份数 × 每份纸张数不等",
            FoldCode::SheetBijection => "纸张序不是双射，存在叠压或空洞",
            FoldCode::ModeOrderDrift => "配页/叠份两种模式的页序被混同",
            FoldCode::ZeroGuard => "零页/零份用例未被拒绝",
            FoldCode::DigestDrift => "折算摘要不稳定",
            FoldCode::Unknown => "未归类的折算异常",
        }
    }

    pub const ALL: [FoldCode; 11] = [
        FoldCode::CaseCountDrift,
        FoldCode::TotalDrift,
        FoldCode::ForwardGap,
        FoldCode::ReverseMismatch,
        FoldCode::DuplicateOrdinal,
        FoldCode::SheetCountDrift,
        FoldCode::SheetBijection,
        FoldCode::ModeOrderDrift,
        FoldCode::ZeroGuard,
        FoldCode::DigestDrift,
        FoldCode::Unknown,
    ];
}

/// 展开顺序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldMode {
    /// 叠份：整份印完再下一份。
    Stacked,
    /// 配页：每页先配齐所有份。
    Collated,
}

impl FoldMode {
    pub fn val(self) -> u8 {
        match self {
            FoldMode::Stacked => 0,
            FoldMode::Collated => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FoldMode::Stacked => "stacked",
            FoldMode::Collated => "collated",
        }
    }
}

/// 一个折算用例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldCase {
    pub id: u16,
    pub pages: u8,
    pub copies: u8,
    pub duplex: bool,
    pub mode: FoldMode,
}

impl FoldCase {
    pub const ZERO: FoldCase = FoldCase {
        id: 0,
        pages: 0,
        copies: 0,
        duplex: false,
        mode: FoldMode::Stacked,
    };

    pub fn total(&self) -> u32 {
        (self.pages as u32).saturating_mul(self.copies as u32)
    }

    /// 每份纸张数：双面每两张一面进一，单面一面一张。
    pub fn sheets_per_copy(&self) -> u32 {
        if self.duplex {
            (self.pages as u32).saturating_add(1) / 2
        } else {
            self.pages as u32
        }
    }

    pub fn total_sheets(&self) -> u32 {
        self.sheets_per_copy().saturating_mul(self.copies as u32)
    }

    pub fn admissible(&self) -> bool {
        self.pages > 0
            && self.pages <= MAX_PAGES
            && self.copies > 0
            && self.copies <= MAX_COPIES
    }

    /// 正向展开：页位序号 → (份, 页)。越界返回 `None`。
    pub fn expand(&self, ordinal: u32) -> Option<(u8, u8)> {
        if ordinal >= self.total() {
            return None;
        }
        match self.mode {
            FoldMode::Stacked => {
                let copy = ordinal / self.pages as u32;
                let page = ordinal % self.pages as u32;
                Some((copy as u8, page as u8))
            }
            FoldMode::Collated => {
                let page = ordinal / self.copies as u32;
                let copy = ordinal % self.copies as u32;
                Some((copy as u8, page as u8))
            }
        }
    }

    /// 反向折算：(份, 页) → 页位序号。越界返回 `None`。
    pub fn ordinal_of(&self, copy: u8, page: u8) -> Option<u32> {
        if copy >= self.copies || page >= self.pages {
            return None;
        }
        match self.mode {
            FoldMode::Stacked => Some((copy as u32) * (self.pages as u32) + page as u32),
            FoldMode::Collated => Some((page as u32) * (self.copies as u32) + copy as u32),
        }
    }

    /// 纸张序：把某个页位映射到 (份, 份内第几张纸)。
    ///
    /// 纸张分组与展开顺序解耦：不论页序怎么排，第 `page` 页永远落在第
    /// `page/2` 张纸（双面）或第 `page` 张纸（单面）上，且属于它自己那一份。
    pub fn sheet_of(&self, copy: u8, page: u8) -> Option<(u8, u32)> {
        if copy >= self.copies || page >= self.pages {
            return None;
        }
        let idx = if self.duplex { (page as u32) / 2 } else { page as u32 };
        Some((copy, idx))
    }

    /// 纸张输出序：叠份按份推进；配页按「套」推进（同一张纸号的所有份先出）。
    pub fn sheet_ordinal(&self, copy: u8, sheet: u32) -> Option<u32> {
        if copy >= self.copies || sheet >= self.sheets_per_copy() {
            return None;
        }
        match self.mode {
            FoldMode::Stacked => Some((copy as u32) * self.sheets_per_copy() + sheet),
            FoldMode::Collated => Some(sheet * (self.copies as u32) + copy as u32),
        }
    }

    /// 纸张反向：纸张序 → (份, 份内第几张)。
    pub fn sheet_at(&self, ordinal: u32) -> Option<(u8, u32)> {
        if ordinal >= self.total_sheets() {
            return None;
        }
        let per = self.sheets_per_copy();
        match self.mode {
            FoldMode::Stacked => Some(((ordinal / per) as u8, ordinal % per)),
            FoldMode::Collated => Some(((ordinal % self.copies as u32) as u8, ordinal / self.copies as u32)),
        }
    }
}

/// 20 条冻结用例：页数 1..=16、份数 1..=8、双面交替、两种模式交替。
pub fn sample_cases() -> [FoldCase; CASE_COUNT] {
    let mut out = [FoldCase::ZERO; CASE_COUNT];
    let mut i = 0usize;
    while i < CASE_COUNT {
        out[i] = FoldCase {
            id: i as u16,
            pages: (1 + (i * 3) % MAX_PAGES as usize) as u8,
            copies: (1 + (i * 5) % MAX_COPIES as usize) as u8,
            duplex: i % 2 == 0,
            mode: if i % 3 == 0 {
                FoldMode::Collated
            } else {
                FoldMode::Stacked
            },
        };
        i += 1;
    }
    out
}

fn fnv_mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

/// 双射审计：正向展开覆盖 `0..total` 且互逆。
///
/// 不用集合容器（no_std 固定容量优先），而是用一个位图：`total <= 128`，
/// 每页位一位。任何重复置位或残留零位都会被查出来。
pub fn bijection_ok(c: &FoldCase) -> bool {
    if !c.admissible() {
        return false;
    }
    let total = c.total();
    if total == 0 || total > 128 {
        return false;
    }
    let mut seen: u128 = 0;
    let mut k = 0u32;
    while k < total {
        let (copy, page) = match c.expand(k) {
            Some(p) => p,
            None => return false,
        };
        let back = match c.ordinal_of(copy, page) {
            Some(v) => v,
            None => return false,
        };
        if back != k {
            return false;
        }
        let bit = 1u128 << k;
        if seen & bit != 0 {
            return false;
        }
        seen |= bit;
        k += 1;
    }
    // 全 1 掩码：低 total 位必须全置。
    let mask = if total == 128 {
        u128::MAX
    } else {
        (1u128 << total) - 1
    };
    seen == mask
}

/// 纸张序双射审计。
pub fn sheet_bijection_ok(c: &FoldCase) -> bool {
    if !c.admissible() {
        return false;
    }
    let total = c.total_sheets();
    if total == 0 || total > 128 {
        return false;
    }
    let mut seen: u128 = 0;
    let mut k = 0u32;
    while k < total {
        let (copy, sheet) = match c.sheet_at(k) {
            Some(p) => p,
            None => return false,
        };
        let back = match c.sheet_ordinal(copy, sheet) {
            Some(v) => v,
            None => return false,
        };
        if back != k {
            return false;
        }
        let bit = 1u128 << k;
        if seen & bit != 0 {
            return false;
        }
        seen |= bit;
        k += 1;
    }
    let mask = if total == 128 {
        u128::MAX
    } else {
        (1u128 << total) - 1
    };
    seen == mask
}

/// 页位→纸张的一致性：展开出的每个页位，纸张号必须落在份内范围内。
pub fn page_sheet_consistent(c: &FoldCase) -> bool {
    let mut k = 0u32;
    while k < c.total() {
        let (copy, page) = match c.expand(k) {
            Some(p) => p,
            None => return false,
        };
        match c.sheet_of(copy, page) {
            Some((sc, sid)) => {
                if sc != copy || sid >= c.sheets_per_copy() {
                    return false;
                }
            }
            None => return false,
        }
        k += 1;
    }
    true
}

/// 模式敏感性：份数 >1 且纸张 >1 时，两种模式的页序必须不同。
pub fn mode_sensitive(c: &FoldCase) -> bool {
    if c.copies < 2 || c.sheets_per_copy() < 2 {
        return true;
    }
    let mut twin = *c;
    twin.mode = match c.mode {
        FoldMode::Stacked => FoldMode::Collated,
        FoldMode::Collated => FoldMode::Stacked,
    };
    let mut differ = false;
    let mut k = 0u32;
    while k < c.total() {
        let a = c.expand(k);
        let b = twin.expand(k);
        if a.is_some() && b.is_some() && a != b {
            differ = true;
        }
        k += 1;
    }
    differ
}

/// 用例摘要：覆盖页数、份数、模式与全部页位序。
pub fn case_digest(c: &FoldCase) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = fnv_mix(h, c.pages as u64);
    h = fnv_mix(h, c.copies as u64);
    h = fnv_mix(h, if c.duplex { 1 } else { 0 });
    h = fnv_mix(h, c.mode.val() as u64);
    let mut k = 0u32;
    while k < c.total() {
        if let Some((copy, page)) = c.expand(k) {
            h = fnv_mix(h, ((copy as u64) << 8) | page as u64);
        }
        k += 1;
    }
    h
}

/// 全样本聚合摘要。
pub fn set_digest(cases: &[FoldCase]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(c) = cases.get(i) {
            h = fnv_mix(h, case_digest(c));
        }
        i += 1;
    }
    h
}

/// 判据。
pub fn run_unx50203_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50203");
    let cases = sample_cases();

    // J1-1 诊断码闭集
    let mut code_ok = FoldCode::ALL.len() == 11;
    let mut i = 0usize;
    while i < FoldCode::ALL.len() {
        if let Some(c) = FoldCode::ALL.get(i) {
            if c.val() != SEG_BASE + 1 + i as u32 || c.detail().is_empty() {
                code_ok = false;
            }
        }
        i += 1;
    }
    s.add("50203-J1-诊断码闭集", code_ok, "11 条码连续且文案非空");

    // J1-2 用例基数
    let mut count_ok = cases.len() == CASE_COUNT;
    let mut i = 0usize;
    while i < CASE_COUNT {
        if let Some(c) = cases.get(i) {
            if c.id as usize != i || !c.admissible() {
                count_ok = false;
            }
        } else {
            count_ok = false;
        }
        i += 1;
    }
    s.add("50203-J1-用例基数", count_ok, "20 条用例均在其界内");

    // J1-3 总量 = 页数 × 份数
    let mut total_ok = true;
    let mut i = 0usize;
    while i < CASE_COUNT {
        if let Some(c) = cases.get(i) {
            let want = (c.pages as u32) * (c.copies as u32);
            if c.total() != want {
                total_ok = false;
            }
        }
        i += 1;
    }
    s.add("50203-J1-总量折算", total_ok, "逐例 total == pages × copies");

    // J1-4 页位双射（零重复）
    let mut bij_ok = true;
    let mut i = 0usize;
    while i < CASE_COUNT {
        if let Some(c) = cases.get(i) {
            if !bijection_ok(c) {
                bij_ok = false;
            }
        }
        i += 1;
    }
    s.add("50203-J1-页位双射", bij_ok, "0..P*C 全覆盖且正反互逆");

    // J1-5 越界拒绝
    let c0 = match cases.first() {
        Some(c) => *c,
        None => FoldCase::ZERO,
    };
    let ok = c0.expand(c0.total()).is_none()
        && c0.ordinal_of(c0.copies, 0).is_none()
        && c0.ordinal_of(0, c0.pages).is_none();
    s.add("50203-J1-越界拒绝", ok, "序号/份/页越界一律 None");

    // J1-6 纸张总数
    let mut sheet_total_ok = true;
    let mut i = 0usize;
    while i < CASE_COUNT {
        if let Some(c) = cases.get(i) {
            let per = if c.duplex {
                (c.pages as u32 + 1) / 2
            } else {
                c.pages as u32
            };
            if c.sheets_per_copy() != per || c.total_sheets() != per * c.copies as u32 {
                sheet_total_ok = false;
            }
        }
        i += 1;
    }
    s.add("50203-J1-纸张总数", sheet_total_ok, "双面进一、按份等比");

    // J1-7 纸张序双射
    let mut sheet_bij = true;
    let mut i = 0usize;
    while i < CASE_COUNT {
        if let Some(c) = cases.get(i) {
            if !sheet_bijection_ok(c) {
                sheet_bij = false;
            }
        }
        i += 1;
    }
    s.add("50203-J1-纸张序双射", sheet_bij, "叠份/配页两模式纸张序均无叠压");

    // J1-8 页位与纸张一致
    let mut ps_ok = true;
    let mut i = 0usize;
    while i < CASE_COUNT {
        if let Some(c) = cases.get(i) {
            if !page_sheet_consistent(c) {
                ps_ok = false;
            }
        }
        i += 1;
    }
    s.add("50203-J1-纸页一致", ps_ok, "每页位落在本份纸张范围内");

    // J1-9 模式敏感：叠份与配页页序不可混同
    let mut mode_ok = true;
    let mut i = 0usize;
    while i < CASE_COUNT {
        if let Some(c) = cases.get(i) {
            if !mode_sensitive(c) {
                mode_ok = false;
            }
        }
        i += 1;
    }
    s.add("50203-J1-模式敏感", mode_ok, "多份多页时两模式页序不同");

    // J1-10 零值防护
    let zero_pages = FoldCase {
        pages: 0,
        copies: 1,
        ..FoldCase::ZERO
    };
    let zero_copies = FoldCase {
        pages: 4,
        copies: 0,
        ..FoldCase::ZERO
    };
    s.add(
        "50203-J1-零值防护",
        !zero_pages.admissible() && !zero_copies.admissible() && !bijection_ok(&zero_pages),
        "零页/零份不可展开",
    );

    // J1-11 摘要稳定
    let d1 = set_digest(&cases);
    let d2 = set_digest(&sample_cases());
    let mut alt = cases;
    if let Some(first) = alt.first_mut() {
        first.mode = match first.mode {
            FoldMode::Stacked => FoldMode::Collated,
            FoldMode::Collated => FoldMode::Stacked,
        };
    }
    let changed = set_digest(&alt) != d1;
    s.add("50203-J1-摘要敏感", d1 == d2 && d1 != 0 && changed, "双跑同摘要且随模式变化");

    // J1-12 容量与段位
    s.add(
        "50203-J1-容量段位",
        SEG_BASE % 0x40 == 0 && MAX_PAGES as u32 * MAX_COPIES as u32 <= 128,
        "段对齐且单例展开不超过位图容量",
    );

    // 动态明细
    let mut pages_sum = 0u64;
    let mut sheets_sum = 0u64;
    let mut i = 0usize;
    while i < CASE_COUNT {
        if let Some(c) = cases.get(i) {
            pages_sum = pages_sum.saturating_add(c.total() as u64);
            sheets_sum = sheets_sum.saturating_add(c.total_sheets() as u64);
        }
        i += 1;
    }
    s.add_dyn(
        "50203-J1-折算明细",
        pages_sum > 0 && sheets_sum > 0,
        &format!("页位合计={} 纸张合计={} 用例={}", pages_sum, sheets_sum, CASE_COUNT),
    );
    s.add_dyn(
        "50203-J1-双跑明细",
        d1 == d2,
        &format!("集合摘要={:016x}", d1),
    );
    s.add_dyn(
        "50203-J1-段位明细",
        SEG_BASE % 0x40 == 0,
        &format!("段基址={:#x} 页上限={} 份上限={}", SEG_BASE, MAX_PAGES, MAX_COPIES),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50203_checks_green() {
        let set = run_unx50203_checks();
        assert!(set.all_passed(), "UNX-F50203 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50203_expansion_is_bijective() {
        for c in sample_cases().iter() {
            assert!(bijection_ok(c), "case {} not bijective", c.id);
            assert!(sheet_bijection_ok(c), "case {} sheet not bijective", c.id);
            assert_eq!(c.total(), c.pages as u32 * c.copies as u32);
        }
    }

    #[test]
    fn unx50203_modes_differ() {
        let stacked = FoldCase {
            id: 0,
            pages: 3,
            copies: 2,
            duplex: false,
            mode: FoldMode::Stacked,
        };
        let collated = FoldCase {
            mode: FoldMode::Collated,
            ..stacked
        };
        assert_eq!(stacked.expand(0), Some((0, 0)));
        assert_eq!(stacked.expand(1), Some((0, 1)));
        assert_eq!(stacked.expand(3), Some((1, 0)));
        assert_eq!(collated.expand(0), Some((0, 0)));
        assert_eq!(collated.expand(1), Some((1, 0)));
        assert_eq!(collated.expand(2), Some((0, 1)));
        assert!(mode_sensitive(&stacked));
    }

    #[test]
    fn unx50203_duplex_sheets() {
        let c = FoldCase {
            id: 1,
            pages: 5,
            copies: 3,
            duplex: true,
            mode: FoldMode::Stacked,
        };
        assert_eq!(c.sheets_per_copy(), 3);
        assert_eq!(c.total_sheets(), 9);
        assert!(sheet_bijection_ok(&c));
    }
}
