#![allow(dead_code)]
//! VE-F3950 · 无障碍示例库汇总
//!
//! 全域示例归一：S01/S04/S05/S06 各示例库 + 全域模式示例 → 全域示例索引
//! （复用任务索引全域化）。孤岛红线全域化：全域示例无索引 = 示例孤岛
//! （孤岛扫描实测）。示例统计透明（断言绑定覆盖统计）；示例质量：全域
//! 示例等价断言。
//! 错误路径：孤岛检出→入索引；统计缺→补；等价破→P0；漂移→对账。
//! 性能：索引 O(全域)；孤岛 O(扫描)；统计 O(1)；等价 O(全域断言)。
//! 跨批对接：F2879 家族全域复用声明；F3815/F3874/F3892/F3912 四册对端；
//! CI 钩子全域。无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x0E40）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x0E40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8JCode(pub u16);

impl S8JCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const ISLAND: Self = Self(SEG_BASE + 1);
    pub const STAT_GAP: Self = Self(SEG_BASE + 2);
    pub const EQUIV_BROKEN: Self = Self(SEG_BASE + 3);
    pub const DRIFT: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::ISLAND => "孤岛检出→入索引",
            Self::STAT_GAP => "统计缺→补",
            Self::EQUIV_BROKEN => "等价破→P0",
            Self::DRIFT => "漂移→对账",
            _ => "未知",
        }
    }
}

// ── 示例条目与全域索引 ───────────────────────────────────────────────

/// 示例条目：来源册 + 是否已入全域索引 + 等价标记。
#[derive(Debug, Clone)]
pub struct Example {
    pub name: &'static str,
    /// 来源册（S01/S04/S05/S06/全域模式）。
    pub book: &'static str,
    /// 已入全域索引。
    pub indexed: bool,
    /// 等价断言通过（示例演示的效果与无障碍路径等价）。
    pub equiv_pass: bool,
}

pub const EXAMPLE_BOOKS: [&str; 5] = ["S01", "S04", "S05", "S06", "全域模式"];

/// 全域示例索引。
#[derive(Debug, Clone)]
pub struct ExampleIndex {
    pub items: Vec<Example>,
}

impl ExampleIndex {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// 登记示例（未入索引状态）。
    pub fn register(&mut self, name: &'static str, book: &'static str, equiv_pass: bool) -> S8JCode {
        if !EXAMPLE_BOOKS.contains(&book) {
            return S8JCode::STAT_GAP;
        }
        self.items.push(Example { name, book, indexed: false, equiv_pass });
        S8JCode::OK
    }

    /// 孤岛扫描：未入索引的示例名单。
    pub fn islands(&self) -> Vec<&'static str> {
        self.items.iter().filter(|e| !e.indexed).map(|e| e.name).collect()
    }

    /// 入索引：孤岛归位。
    pub fn admit(&mut self, name: &str) -> S8JCode {
        match self.items.iter_mut().find(|e| e.name == name) {
            Some(e) if !e.indexed => {
                e.indexed = true;
                S8JCode::OK
            }
            Some(_) => S8JCode::DRIFT,
            None => S8JCode::ISLAND,
        }
    }

    /// 全域索引完备：无孤岛。
    pub fn no_islands(&self) -> bool {
        self.islands().is_empty() && !self.items.is_empty()
    }

    /// 等价破检出：等价断言不过的示例。
    pub fn equiv_broken(&self) -> Vec<&'static str> {
        self.items.iter().filter(|e| !e.equiv_pass).map(|e| e.name).collect()
    }

    /// 等价全域断言。
    pub fn all_equiv(&self) -> bool {
        self.equiv_broken().is_empty() && !self.items.is_empty()
    }
}

impl Default for ExampleIndex {
    fn default() -> Self {
        Self::new()
    }
}

// ── 统计透明（断言绑定）──────────────────────────────────────────────

/// 全域示例统计：总量 + 分册计数，统计与索引断言绑定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExampleStats {
    pub total: usize,
    /// 分册计数（下标对齐 EXAMPLE_BOOKS）。
    pub per_book: [usize; 5],
    /// 已入索引数。
    pub indexed: usize,
}

impl ExampleStats {
    /// O(1) 可复核：统计必须与索引实态一致（透明断言）。
    pub fn matches(&self, idx: &ExampleIndex) -> bool {
        if self.total != idx.items.len() {
            return false;
        }
        for (i, b) in EXAMPLE_BOOKS.iter().enumerate() {
            let actual = idx.items.iter().filter(|e| e.book == *b).count();
            if self.per_book[i] != actual {
                return false;
            }
        }
        self.indexed == idx.items.iter().filter(|e| e.indexed).count()
    }

    /// 从索引实态生成统计（单源）。
    pub fn from_index(idx: &ExampleIndex) -> Self {
        let mut per_book = [0usize; 5];
        for (i, b) in EXAMPLE_BOOKS.iter().enumerate() {
            per_book[i] = idx.items.iter().filter(|e| e.book == *b).count();
        }
        Self {
            total: idx.items.len(),
            per_book,
            indexed: idx.items.iter().filter(|e| e.indexed).count(),
        }
    }
}

// ── 漂移对账（索引 vs 来源册）────────────────────────────────────────

/// 来源册登记：册名 → 条目数。
#[derive(Debug, Clone)]
pub struct SourceBook {
    pub book: &'static str,
    pub count: usize,
}

/// 漂移对账：索引实态 vs 来源册登记。
pub fn reconcile(idx: &ExampleIndex, sources: &[SourceBook]) -> Vec<&'static str> {
    let mut drifted = Vec::new();
    for s in sources.iter() {
        let actual = idx.items.iter().filter(|e| e.book == s.book).count();
        if actual != s.count {
            drifted.push(s.book);
        }
    }
    drifted
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves204_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves204_s08examples");

    // 01 全域示例归一：五册登记
    {
        let mut ix = ExampleIndex::new();
        for b in EXAMPLE_BOOKS.iter() {
            chk(&mut set, "C3950-01", ix.register("样例", b, true) == S8JCode::OK, "err");
        }
    }
    // 02 孤岛扫描实测
    {
        let mut ix = ExampleIndex::new();
        ix.register("e1", "S01", true);
        ix.register("e2", "S04", true);
        chk(&mut set, "C3950-02", ix.islands() == vec!["e1", "e2"] && !ix.no_islands(), "err");
    }
    // 03 孤岛入索引闭环
    {
        let mut ix = ExampleIndex::new();
        ix.register("e1", "S01", true);
        ix.admit("e1");
        chk(&mut set, "C3950-03", ix.islands().is_empty() && ix.no_islands(), "err");
    }
    // 04 未知册拒收
    {
        let mut ix = ExampleIndex::new();
        chk(&mut set, "C3950-04", ix.register("坏", "S09", true) == S8JCode::STAT_GAP, "err");
    }
    // 05 重复入索引 = 漂移
    {
        let mut ix = ExampleIndex::new();
        ix.register("e1", "S05", true);
        ix.admit("e1");
        chk(&mut set, "C3950-05", ix.admit("e1") == S8JCode::DRIFT, "err");
    }
    // 06 不存在示例入索引 = 孤岛码
    {
        let mut ix = ExampleIndex::new();
        chk(&mut set, "C3950-06", ix.admit("幽灵") == S8JCode::ISLAND, "err");
    }
    // 07 等价全域断言
    {
        let mut ix = ExampleIndex::new();
        ix.register("好", "S01", true);
        ix.register("坏", "S04", false);
        chk(&mut set, "C3950-07", ix.equiv_broken() == vec!["坏"] && !ix.all_equiv(), "err");
    }
    // 08 等价破→P0 处置后归零
    {
        let mut ix = ExampleIndex::new();
        ix.register("坏", "S06", false);
        ix.items[0].equiv_pass = true;
        chk(&mut set, "C3950-08", ix.all_equiv(), "err");
    }
    // 09 统计透明：实态生成
    {
        let mut ix = ExampleIndex::new();
        ix.register("a", "S01", true);
        ix.register("b", "S01", true);
        ix.register("c", "全域模式", true);
        let st = ExampleStats::from_index(&ix);
        chk(&mut set, "C3950-09", st.total == 3 && st.per_book[0] == 2 && st.per_book[4] == 1 && st.matches(&ix), "err");
    }
    // 10 统计伪造检出（透明断言实测）
    {
        let mut ix = ExampleIndex::new();
        ix.register("a", "S04", true);
        let mut st = ExampleStats::from_index(&ix);
        st.total = 99;
        chk(&mut set, "C3950-10", !st.matches(&ix), "err");
    }
    // 11 漂移对账：来源册失真检出
    {
        let mut ix = ExampleIndex::new();
        ix.register("a", "S01", true);
        ix.register("b", "S01", true);
        let sources = vec![SourceBook { book: "S01", count: 3 }];
        chk(&mut set, "C3950-11", reconcile(&ix, &sources) == vec!["S01"], "err");
    }
    // 12 漂移对账一致零漂
    {
        let mut ix = ExampleIndex::new();
        ix.register("a", "S05", true);
        let sources = vec![SourceBook { book: "S05", count: 1 }];
        chk(&mut set, "C3950-12", reconcile(&ix, &sources).is_empty(), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3950-13", S8JCode::DRIFT.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8JCode::OK, S8JCode::ISLAND, S8JCode::STAT_GAP, S8JCode::EQUIV_BROKEN, S8JCode::DRIFT];
        chk(&mut set, "C3950-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_examples_all_green() {
        let s = run_ves204_checks();
        assert!(s.all_passed(), "ves204 red: {}", s.red_items_text());
    }

    #[test]
    fn test_island_scan_full_cycle() {
        let mut ix = ExampleIndex::new();
        ix.register("x", "S01", true);
        ix.register("y", "S06", true);
        assert_eq!(ix.islands().len(), 2);
        ix.admit("x");
        ix.admit("y");
        assert!(ix.no_islands());
    }

    #[test]
    fn test_stats_transparency() {
        let mut ix = ExampleIndex::new();
        for i in 0..5 {
            ix.register("e", EXAMPLE_BOOKS[i], true);
        }
        let st = ExampleStats::from_index(&ix);
        assert!(st.matches(&ix));
        assert_eq!(st.total, 5);
    }
}
