#![allow(dead_code)]
//! VE-F3951 · 无障碍文档总纲
//!
//! 全域无障碍文档地图（复用 2 跳地图全域化）：各域文档册 + S 域册 →
//! 全域地图（2 跳全域断言：任意文档到任意文档 ≤ 2 跳）。全域同源清单；
//! 全域自举红线：全域无障碍文档全过无障碍（自举全域破 → P1）；全域检索。
//! 错误路径：迷宫全域检出→重构；自举全域破→P1；同源破→CI；检索败→
//! 修复。
//! 性能：地图 O(全域)；自举 O(全域审查)；同源 O(清单)；检索 O(索引)。
//! 跨批对接：F3356 模式全域复用声明；全域文档册对端；CI 钩子全域。
//! 无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

// ── 诊断码段（0x0EC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x0EC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8KCode(pub u16);

impl S8KCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const MAZE_FOUND: Self = Self(SEG_BASE + 1);
    pub const BOOTSTRAP_BROKEN: Self = Self(SEG_BASE + 2);
    pub const SOURCE_SPLIT: Self = Self(SEG_BASE + 3);
    pub const SEARCH_MISS: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::MAZE_FOUND => "迷宫全域检出→重构",
            Self::BOOTSTRAP_BROKEN => "自举全域破→P1",
            Self::SOURCE_SPLIT => "同源破→CI",
            Self::SEARCH_MISS => "检索败→修复",
            _ => "未知",
        }
    }
}

// ── 全域文档地图（2 跳断言）──────────────────────────────────────────

/// 文档节点：册名 + 出链（到其他册）。
#[derive(Debug, Clone)]
pub struct DocNode {
    pub book: &'static str,
    /// 出链目标册。
    pub links: Vec<&'static str>,
    /// 自举标记：本文档自身通过无障碍审查。
    pub a11y_ok: bool,
}

/// 全域地图：BFS 两跳可达断言。
#[derive(Debug, Clone)]
pub struct DocMap {
    pub nodes: Vec<DocNode>,
}

impl DocMap {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn add(&mut self, n: DocNode) {
        self.nodes.push(n);
    }

    fn find(&self, book: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.book == book)
    }

    /// 两跳可达：from → (直接 or 经一中间册) → to。
    pub fn reachable_2hop(&self, from: &str, to: &str) -> bool {
        let s = match self.find(from) {
            Some(i) => i,
            None => return false,
        };
        // 一跳。
        if self.nodes[s].links.iter().any(|l| *l == to) {
            return true;
        }
        // 两跳。
        for mid in self.nodes[s].links.iter() {
            if let Some(mi) = self.find(mid) {
                if self.nodes[mi].links.iter().any(|l| *l == to) {
                    return true;
                }
            }
        }
        false
    }

    /// 2 跳全域断言：任意册对任意册（含自身）都两跳可达。
    pub fn all_reachable(&self) -> Vec<(&'static str, &'static str)> {
        let mut maze = Vec::new();
        for a in self.nodes.iter() {
            for b in self.nodes.iter() {
                if !self.reachable_2hop(a.book, b.book) {
                    maze.push((a.book, b.book));
                }
            }
        }
        maze
    }

    /// 迷宫检出 → 需重构的对数。
    pub fn maze_pairs(&self) -> usize {
        self.all_reachable().len()
    }

    /// 重构：补一条出链。
    pub fn link(&mut self, from: &str, to: &'static str) -> S8KCode {
        match self.nodes.iter_mut().find(|n| n.book == from) {
            Some(n) if !n.links.contains(&to) => {
                n.links.push(to);
                S8KCode::OK
            }
            Some(_) => S8KCode::SOURCE_SPLIT,
            None => S8KCode::MAZE_FOUND,
        }
    }
}

impl Default for DocMap {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全域自举红线（文档全过无障碍）────────────────────────────────────

impl DocMap {
    /// 自举全域断言：所有文档 a11y_ok。
    pub fn bootstrapped(&self) -> bool {
        !self.nodes.is_empty() && self.nodes.iter().all(|n| n.a11y_ok)
    }

    /// 自举破名单（P1）。
    pub fn bootstrap_broken(&self) -> Vec<&'static str> {
        self.nodes.iter().filter(|n| !n.a11y_ok).map(|n| n.book).collect()
    }
}

// ── 全域同源清单（CI 对账）───────────────────────────────────────────

/// 同源清单：册名 → 内容指纹。同源 = 同一指纹源。
#[derive(Debug, Clone)]
pub struct SourceBook {
    pub book: &'static str,
    pub fp: u64,
}

/// 同源对账：外部指纹表 vs 单源指纹。
pub fn source_split(sources: &[SourceBook], canonical: &[(String, u64)]) -> Vec<&'static str> {
    let mut split = Vec::new();
    for s in sources.iter() {
        match canonical.iter().find(|(n, _)| *n == s.book) {
            Some((_, fp)) if *fp != s.fp => split.push(s.book),
            None => split.push(s.book),
            _ => {}
        }
    }
    split
}

// ── 全域检索 ─────────────────────────────────────────────────────────

/// 检索索引：关键词 → 册名。
#[derive(Debug, Clone)]
pub struct SearchIndex {
    pub entries: Vec<(&'static str, &'static str)>,
}

impl SearchIndex {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn index(&mut self, keyword: &'static str, book: &'static str) {
        self.entries.push((keyword, book));
    }

    /// 检索 O(索引)：返回命中册。
    pub fn search(&self, kw: &str) -> Vec<&'static str> {
        self.entries.iter().filter(|(k, _)| *k == kw).map(|(_, b)| *b).collect()
    }

    /// 检索败检出：期望册未命中。
    pub fn miss(&self, kw: &str, expect: &str) -> bool {
        !self.search(kw).iter().any(|b| *b == expect)
    }
}

impl Default for SearchIndex {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves205_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves205_s08docmap");

    fn full_map() -> DocMap {
        let mut m = DocMap::new();
        // 环形出链保证任意两册两跳可达。
        m.add(DocNode { book: "S01册", links: vec!["S02册", "全域册"], a11y_ok: true });
        m.add(DocNode { book: "S02册", links: vec!["S03册", "全域册"], a11y_ok: true });
        m.add(DocNode { book: "S03册", links: vec!["S01册", "全域册"], a11y_ok: true });
        m.add(DocNode { book: "全域册", links: vec!["S01册", "S02册", "S03册"], a11y_ok: true });
        m
    }

    // 01 全域地图：2 跳全域断言成立
    {
        let m = full_map();
        chk(&mut set, "C3951-01", m.maze_pairs() == 0 && m.all_reachable().is_empty(), "err");
    }
    // 02 两跳可达逐例验证
    {
        let m = full_map();
        chk(&mut set, "C3951-02", m.reachable_2hop("S01册", "S03册") && m.reachable_2hop("S01册", "S01册"), "err");
    }
    // 03 迷宫全域检出实测
    {
        let mut m = DocMap::new();
        m.add(DocNode { book: "孤册", links: vec![], a11y_ok: true });
        m.add(DocNode { book: "另册", links: vec![], a11y_ok: true });
        chk(&mut set, "C3951-03", m.maze_pairs() == 4, "err");
    }
    // 04 迷宫重构：补链归零
    {
        let mut m = full_map();
        m.nodes[0].links.clear();
        m.link("S01册", "全域册");
        chk(&mut set, "C3951-04", m.reachable_2hop("S01册", "全域册") && m.reachable_2hop("S01册", "S02册"), "err");
    }
    // 05 补链幂等（重复补 = 同源破码）
    {
        let mut m = full_map();
        chk(&mut set, "C3951-05", m.link("S01册", "全域册") == S8KCode::SOURCE_SPLIT, "err");
    }
    // 06 不存在册补链拒收
    {
        let mut m = full_map();
        chk(&mut set, "C3951-06", m.link("幽灵册", "S01册") == S8KCode::MAZE_FOUND, "err");
    }
    // 07 全域自举断言成立
    {
        let m = full_map();
        chk(&mut set, "C3951-07", m.bootstrapped() && m.bootstrap_broken().is_empty(), "err");
    }
    // 08 自举全域破→P1 名单实测
    {
        let mut m = DocMap::new();
        m.add(DocNode { book: "差册", links: vec![], a11y_ok: false });
        m.add(DocNode { book: "好册", links: vec![], a11y_ok: true });
        chk(&mut set, "C3951-08", !m.bootstrapped() && m.bootstrap_broken() == vec!["差册"], "err");
    }
    // 09 同源对账一致
    {
        let sources = vec![
            SourceBook { book: "S01册", fp: 100 },
            SourceBook { book: "S02册", fp: 200 },
        ];
        let canon = vec![("S01册".to_string(), 100u64), ("S02册".to_string(), 200u64)];
        chk(&mut set, "C3951-09", source_split(&sources, &canon).is_empty(), "err");
    }
    // 10 同源破检出
    {
        let sources = vec![
            SourceBook { book: "S01册", fp: 100 },
            SourceBook { book: "S03册", fp: 999 },
        ];
        let canon = vec![("S01册".to_string(), 100u64), ("S03册".to_string(), 300u64)];
        chk(&mut set, "C3951-10", source_split(&sources, &canon) == vec!["S03册"], "err");
    }
    // 11 全域检索命中
    {
        let mut ix = SearchIndex::new();
        ix.index("对比度", "S01册");
        ix.index("对比度", "全域册");
        ix.index("焦点", "S02册");
        chk(&mut set, "C3951-11", ix.search("对比度").len() == 2 && !ix.miss("对比度", "全域册"), "err");
    }
    // 12 检索败检出→修复（补索引）
    {
        let mut ix = SearchIndex::new();
        ix.index("对比度", "S01册");
        let missed = ix.miss("焦点", "S02册");
        ix.index("焦点", "S02册");
        chk(&mut set, "C3951-12", missed && !ix.miss("焦点", "S02册"), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3951-13", S8KCode::SEARCH_MISS.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8KCode::OK, S8KCode::MAZE_FOUND, S8KCode::BOOTSTRAP_BROKEN, S8KCode::SOURCE_SPLIT, S8KCode::SEARCH_MISS];
        chk(&mut set, "C3951-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_docmap_all_green() {
        let s = run_ves205_checks();
        assert!(s.all_passed(), "ves205 red: {}", s.red_items_text());
    }

    #[test]
    fn test_two_hop_chain() {
        let mut m = DocMap::new();
        m.add(DocNode { book: "A", links: vec!["B"], a11y_ok: true });
        m.add(DocNode { book: "B", links: vec!["C"], a11y_ok: true });
        m.add(DocNode { book: "C", links: vec!["A"], a11y_ok: true });
        assert!(m.reachable_2hop("A", "C"));
        assert!(m.reachable_2hop("C", "B"));
        assert_eq!(m.maze_pairs(), 0);
    }

    #[test]
    fn test_bootstrap_p1() {
        let mut m = DocMap::new();
        for i in 0..4 {
            m.add(DocNode { book: "册", links: vec![], a11y_ok: i != 2 });
        }
        assert_eq!(m.bootstrap_broken().len(), 1);
        assert!(!m.bootstrapped());
    }
}
