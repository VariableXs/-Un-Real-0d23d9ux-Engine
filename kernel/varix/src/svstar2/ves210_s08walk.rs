#![allow(dead_code)]
//! VE-F3956 · 无障碍走查汇总
//!
//! 全域走查矩阵：全引擎域 × 五画像 × 三段——全域覆盖断言：全域每域至少
//! 两画像格。全域汇总册（发现分级全域）；全域对照（挫败信号对照全域化，
//! 矛盾→日志优先裁决）；全域阻断清零（未清→不收官）。
//! 错误路径：覆盖全域缺→补走；阻断全域未清→不收官；对照矛盾→日志优先；
//! 汇总漏域→补。
//! 性能：矩阵 O(全域)；汇总 O(全域册)；对照 O(全域信号)；清零 O(全域
//! 断言)。
//! 跨批对接：F2994/F3156 模式全域复用声明；全域走查对端；收官宣告全域
//! 输入。无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x2A80）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x2A80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8PCode(pub u16);

impl S8PCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const COVER_GAP: Self = Self(SEG_BASE + 1);
    pub const BLOCK_UNCLEARED: Self = Self(SEG_BASE + 2);
    pub const CONTRADICT: Self = Self(SEG_BASE + 3);
    pub const SUMMARY_LEAK: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::COVER_GAP => "覆盖全域缺→补走",
            Self::BLOCK_UNCLEARED => "阻断全域未清→不收官",
            Self::CONTRADICT => "对照矛盾→日志优先",
            Self::SUMMARY_LEAK => "汇总漏域→补",
            _ => "未知",
        }
    }
}

// ── 全域走查矩阵 ─────────────────────────────────────────────────────

/// 画像五类。
pub const PORTRAITS: [&str; 5] = ["视障", "听障", "运动", "认知", "老年"];

/// 走查段三段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkPhase {
    /// 执行段。
    Execute,
    /// 观察段。
    Observe,
    /// 复述段。
    Debrief,
}

/// 走查格：域 × 画像 → 三段完成状态。
#[derive(Debug, Clone)]
pub struct WalkCell {
    pub domain: u8,
    pub portrait: usize,
    pub phases: [bool; 3],
    /// 发现的阻断数。
    pub blockers: u32,
}

impl WalkCell {
    pub fn complete(&self) -> bool {
        self.phases.iter().all(|p| *p)
    }
}

/// 全域走查账。
#[derive(Debug, Clone)]
pub struct WalkLedger {
    pub cells: Vec<WalkCell>,
    pub domains: u8,
}

pub const MIN_PORTRAIT_CELLS: usize = 2;

impl WalkLedger {
    pub fn new(domains: u8) -> Self {
        Self { cells: Vec::new(), domains }
    }

    pub fn add(&mut self, c: WalkCell) -> S8PCode {
        if c.domain == 0 || c.domain > self.domains || c.portrait >= PORTRAITS.len() {
            return S8PCode::COVER_GAP;
        }
        self.cells.push(c);
        S8PCode::OK
    }

    /// 域 d 的走查格数。
    pub fn cells_of(&self, domain: u8) -> Vec<usize> {
        self.cells.iter().enumerate().filter(|(_, c)| c.domain == domain).map(|(i, _)| i).collect()
    }

    /// 覆盖全域缺：完成格 < 两画像格的域。
    pub fn cover_gaps(&self) -> Vec<u8> {
        (1..=self.domains)
            .filter(|d| {
                self.cells_of(*d)
                    .iter()
                    .filter(|i| self.cells[**i].complete())
                    .count()
                    < MIN_PORTRAIT_CELLS
            })
            .collect()
    }

    /// 阻断清零断言：全部格阻断数为 0。
    pub fn blockers_clear(&self) -> bool {
        self.cells.iter().all(|c| c.blockers == 0)
    }

    /// 阻断格清单（不收官名单）。
    pub fn blocked_cells(&self) -> Vec<usize> {
        self.cells.iter().enumerate().filter(|(_, c)| c.blockers > 0).map(|(i, _)| i).collect()
    }
}

impl Default for WalkLedger {
    fn default() -> Self {
        Self::new(8)
    }
}

// ── 全域汇总册（漏域检出）────────────────────────────────────────────

/// 汇总册：域 → 发现数（分级：阻断/严重/一般）。
#[derive(Debug, Clone)]
pub struct SummaryItem {
    pub domain: u8,
    pub blocker: u32,
    pub major: u32,
    pub minor: u32,
}

impl SummaryItem {
    pub fn total(&self) -> u32 {
        self.blocker + self.major + self.minor
    }
}

/// 汇总册全域对账：汇总必须覆盖全部域（漏域→补）。
#[derive(Debug, Clone)]
pub struct SummaryBook {
    pub items: Vec<SummaryItem>,
    pub domains: u8,
}

impl SummaryBook {
    pub fn new(domains: u8) -> Self {
        Self { items: Vec::new(), domains }
    }

    pub fn upsert(&mut self, it: SummaryItem) {
        match self.items.iter_mut().find(|x| x.domain == it.domain) {
            Some(slot) => *slot = it,
            None => self.items.push(it),
        }
    }

    /// 汇总漏域检出。
    pub fn missing_domains(&self) -> Vec<u8> {
        (1..=self.domains)
            .filter(|d| !self.items.iter().any(|i| i.domain == *d))
            .collect()
    }

    /// 全域汇总完备。
    pub fn complete(&self) -> bool {
        self.missing_domains().is_empty() && self.items.len() == self.domains as usize
    }
}

impl Default for SummaryBook {
    fn default() -> Self {
        Self::new(8)
    }
}

/// 汇总与走查账一致断言。
pub fn summary_matches_walk(book: &SummaryBook, walk: &WalkLedger) -> bool {
    for it in book.items.iter() {
        let actual: u32 = walk
            .cells
            .iter()
            .filter(|c| c.domain == it.domain)
            .map(|c| c.blockers)
            .sum();
        if actual != it.blocker {
            return false;
        }
    }
    book.complete()
}

// ── 挫败信号对照（日志优先）──────────────────────────────────────────

/// 对照对：汇总记录值 vs 日志实测值。
#[derive(Debug, Clone)]
pub struct CrossPair {
    pub domain: u8,
    pub summary_val: u32,
    pub log_val: u32,
}

/// 对照裁决：矛盾时以日志为准，返回需改写的域。
pub fn adjudicate(pairs: &[CrossPair]) -> Vec<u8> {
    pairs.iter().filter(|p| p.summary_val != p.log_val).map(|p| p.domain).collect()
}

/// 裁决修正：汇总值回归日志值。
pub fn apply_log_priority(book: &mut SummaryBook, pairs: &[CrossPair]) -> usize {
    let mut n = 0;
    for p in pairs.iter() {
        if let Some(it) = book.items.iter_mut().find(|i| i.domain == p.domain) {
            if it.blocker != p.log_val {
                it.blocker = p.log_val;
                n += 1;
            }
        }
    }
    n
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves210_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves210_s08walk");

    fn cell(domain: u8, portrait: usize, blockers: u32) -> WalkCell {
        WalkCell { domain, portrait, phases: [true; 3], blockers }
    }

    // 01 画像清单守恒
    {
        chk(&mut set, "C3956-01", PORTRAITS.len() == 5 && PORTRAITS[0] == "视障" && PORTRAITS[4] == "老年", "err");
    }
    // 02 全域覆盖断言：八域各两格完成
    {
        let mut w = WalkLedger::new(8);
        for d in 1..=8u8 {
            w.add(cell(d, 0, 0));
            w.add(cell(d, 1, 0));
        }
        chk(&mut set, "C3956-02", w.cover_gaps().is_empty() && w.blockers_clear(), "err");
    }
    // 03 覆盖全域缺检出
    {
        let mut w = WalkLedger::new(3);
        w.add(cell(1, 0, 0));
        w.add(cell(1, 2, 0));
        w.add(cell(2, 0, 0)); // 域 2 只有一格
        w.add(cell(3, 0, 1)); // 域 3 有阻断
        w.add(cell(3, 4, 0));
        chk(&mut set, "C3956-03", w.cover_gaps() == vec![2], "err");
    }
    // 04 三段未全 = 格未完成
    {
        let mut w = WalkLedger::new(1);
        let mut c = cell(1, 0, 0);
        c.phases[2] = false;
        w.add(c);
        chk(&mut set, "C3956-04", w.cover_gaps() == vec![1], "err");
    }
    // 05 阻断全域未清→不收官名单
    {
        let mut w = WalkLedger::new(2);
        w.add(cell(1, 0, 3));
        w.add(cell(1, 1, 0));
        w.add(cell(2, 0, 0));
        w.add(cell(2, 1, 0));
        chk(&mut set, "C3956-05", !w.blockers_clear() && w.blocked_cells() == vec![0], "err");
    }
    // 06 域号越界拒收
    {
        let mut w = WalkLedger::new(2);
        chk(&mut set, "C3956-06", w.add(cell(3, 0, 0)) == S8PCode::COVER_GAP, "err");
    }
    // 07 画像下标越界拒收
    {
        let mut w = WalkLedger::new(2);
        chk(&mut set, "C3956-07", w.add(cell(1, 5, 0)) == S8PCode::COVER_GAP, "err");
    }
    // 08 汇总漏域检出→补
    {
        let mut b = SummaryBook::new(4);
        b.upsert(SummaryItem { domain: 1, blocker: 0, major: 2, minor: 5 });
        b.upsert(SummaryItem { domain: 2, blocker: 1, major: 0, minor: 3 });
        let miss = b.missing_domains();
        b.upsert(SummaryItem { domain: 3, blocker: 0, major: 0, minor: 0 });
        b.upsert(SummaryItem { domain: 4, blocker: 0, major: 1, minor: 0 });
        chk(&mut set, "C3956-08", miss == vec![3, 4] && b.complete(), "err");
    }
    // 09 汇总与走查对账一致
    {
        let mut w = WalkLedger::new(2);
        w.add(cell(1, 0, 2));
        w.add(cell(1, 1, 0));
        w.add(cell(2, 0, 0));
        w.add(cell(2, 1, 0));
        let mut b = SummaryBook::new(2);
        b.upsert(SummaryItem { domain: 1, blocker: 2, major: 0, minor: 0 });
        b.upsert(SummaryItem { domain: 2, blocker: 0, major: 0, minor: 0 });
        chk(&mut set, "C3956-09", summary_matches_walk(&b, &w), "err");
    }
    // 10 汇总失真检出
    {
        let mut w = WalkLedger::new(1);
        w.add(cell(1, 0, 5));
        w.add(cell(1, 1, 0));
        let mut b = SummaryBook::new(1);
        b.upsert(SummaryItem { domain: 1, blocker: 3, major: 0, minor: 0 });
        chk(&mut set, "C3956-10", !summary_matches_walk(&b, &w), "err");
    }
    // 11 对照矛盾→日志优先裁决
    {
        let pairs = vec![
            CrossPair { domain: 1, summary_val: 2, log_val: 2 },
            CrossPair { domain: 2, summary_val: 5, log_val: 7 },
        ];
        chk(&mut set, "C3956-11", adjudicate(&pairs) == vec![2], "err");
    }
    // 12 裁决修正落账
    {
        let mut b = SummaryBook::new(2);
        b.upsert(SummaryItem { domain: 1, blocker: 2, major: 0, minor: 0 });
        b.upsert(SummaryItem { domain: 2, blocker: 5, major: 0, minor: 0 });
        let pairs = vec![CrossPair { domain: 2, summary_val: 5, log_val: 7 }];
        let n = apply_log_priority(&mut b, &pairs);
        chk(&mut set, "C3956-12", n == 1 && b.items[1].blocker == 7, "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3956-13", S8PCode::SUMMARY_LEAK.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8PCode::OK, S8PCode::COVER_GAP, S8PCode::BLOCK_UNCLEARED, S8PCode::CONTRADICT, S8PCode::SUMMARY_LEAK];
        chk(&mut set, "C3956-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_walk_all_green() {
        let s = run_ves210_checks();
        assert!(s.all_passed(), "ves210 red: {}", s.red_items_text());
    }

    #[test]
    fn test_blocker_clearance_flow() {
        let mut w = WalkLedger::new(1);
        w.add(WalkCell { domain: 1, portrait: 0, phases: [true; 3], blockers: 4 });
        w.add(WalkCell { domain: 1, portrait: 3, phases: [true; 3], blockers: 0 });
        assert!(!w.blockers_clear());
        w.cells[0].blockers = 0;
        assert!(w.blockers_clear());
    }

    #[test]
    fn test_summary_upsert_replaces() {
        let mut b = SummaryBook::new(1);
        b.upsert(SummaryItem { domain: 1, blocker: 1, major: 0, minor: 0 });
        b.upsert(SummaryItem { domain: 1, blocker: 2, major: 1, minor: 0 });
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].total(), 3);
    }
}
