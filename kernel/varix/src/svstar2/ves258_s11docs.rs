//! VE-F4018 · 国际化文档
//!
//! i18n 文档：开发者册（接入指南/格式规范——义务前置）；用户册（语言
//! 切换指南——可达）；标准册（CLDR 版本/覆盖表公开——透明）；同源纪
//! 律；自举复用。
//! 数据结构：三册结构；义务前置；自举复用；同源清单。
//! 错误路径：漂移→对账；义务章缺→审查；自举不过→修；迷宫→2 跳。
//! 性能：构建 O(文档)；自举 O(审查)；对账 O(清单)；断言增量。
//! 跨批对接：F2879 家族复用；F3356 地图复用；CI 钩子。
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x66C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x66C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T24DCode(pub u16);

impl T24DCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const DRIFT: Self = Self(SEG_BASE + 1);
    pub const DUTY_MISS: Self = Self(SEG_BASE + 2);
    pub const BOOTSTRAP_FAIL: Self = Self(SEG_BASE + 3);
    pub const MAZE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::DRIFT => "漂移→对账",
            Self::DUTY_MISS => "义务章缺→审查",
            Self::BOOTSTRAP_FAIL => "自举不过→修",
            Self::MAZE => "迷宫→2 跳",
            _ => "未知",
        }
    }
}

// ── 三册结构 ─────────────────────────────────────────────────────────

/// 三册。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookKind {
    /// 开发者册。
    Dev,
    /// 用户册。
    User,
    /// 标准册。
    Standard,
}

pub const BOOK_KINDS: [BookKind; 3] = [BookKind::Dev, BookKind::User, BookKind::Standard];

impl BookKind {
    pub fn name(self) -> &'static str {
        match self {
            BookKind::Dev => "开发者册",
            BookKind::User => "用户册",
            BookKind::Standard => "标准册",
        }
    }

    pub fn slot(self) -> usize {
        match self {
            BookKind::Dev => 0,
            BookKind::User => 1,
            BookKind::Standard => 2,
        }
    }
}

/// 单册：章节清单 + 可达标记。
#[derive(Debug, Clone)]
pub struct I18nBook {
    pub kind: BookKind,
    /// 章节。
    pub chapters: Vec<&'static str>,
    /// 义务章（开发者册必含）。
    pub duty_chapter: bool,
    /// 可达（用户册）。
    pub reachable: bool,
}

/// 义务章名。
pub const DUTY_CHAPTER: &str = "接入义务";

/// 三册总册。
#[derive(Debug, Clone)]
pub struct I18nDocSet {
    pub books: [Option<I18nBook>; 3],
}

impl I18nDocSet {
    pub fn new() -> Self {
        Self { books: [None, None, None] }
    }

    /// 建册。
    pub fn build(&mut self, kind: BookKind, chapters: &[&'static str], duty: bool, reachable: bool) {
        self.books[kind.slot()] = Some(I18nBook {
            kind,
            chapters: chapters.iter().map(|c| *c).collect(),
            duty_chapter: duty,
            reachable,
        });
    }

    /// 义务前置审查：开发者册必须含义务章。
    pub fn duty_ok(&self) -> bool {
        match self.books[BookKind::Dev.slot()].as_ref() {
            Some(b) => b.duty_chapter && b.chapters.contains(&DUTY_CHAPTER),
            None => false,
        }
    }

    /// 用户册可达。
    pub fn user_reachable(&self) -> bool {
        self.books[BookKind::User.slot()].as_ref().map(|b| b.reachable).unwrap_or(false)
    }

    /// 标准册透明：CLDR 版本与覆盖表公开。
    pub fn standard_transparent(&self) -> bool {
        match self.books[BookKind::Standard.slot()].as_ref() {
            Some(b) => b.chapters.contains(&"CLDR 版本") && b.chapters.contains(&"覆盖表"),
            None => false,
        }
    }

    /// 三册齐备。
    pub fn complete(&self) -> bool {
        self.books.iter().all(|b| b.is_some())
    }
}

// ── 同源清单与对账 ──────────────────────────────────────────────────

/// 同源条目：文档名 → 真源指针。
#[derive(Debug, Clone)]
pub struct SourceEntry {
    pub doc: String,
    pub source: String,
}

/// 同源清单（可对账）。
#[derive(Debug, Clone)]
pub struct SourceList {
    pub entries: Vec<SourceEntry>,
}

impl SourceList {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn put(&mut self, doc: &str, source: &str) -> bool {
        if self.entries.iter().any(|e| e.doc == doc) {
            return false;
        }
        self.entries.push(SourceEntry {
            doc: String::from(doc),
            source: String::from(source),
        });
        true
    }

    /// 对账：真源指针全部非空。
    pub fn reconcile(&self) -> T24DCode {
        if self.entries.iter().any(|e| e.source.is_empty()) {
            T24DCode::DRIFT
        } else {
            T24DCode::OK
        }
    }
}

// ── 自举（文档全过无障碍）───────────────────────────────────────────

/// 自举审查：每文档 a11y_ok。
#[derive(Debug, Clone)]
pub struct BootstrapAudit {
    /// 文档名 → a11y 过。
    pub marks: Vec<(String, bool)>,
}

impl BootstrapAudit {
    pub fn new() -> Self {
        Self { marks: Vec::new() }
    }

    pub fn put(&mut self, doc: &str, ok: bool) {
        match self.marks.iter_mut().find(|(d, _)| d == doc) {
            Some(slot) => slot.1 = ok,
            None => self.marks.push((String::from(doc), ok)),
        }
    }

    /// 自举断言：全过。
    pub fn bootstrapped(&self) -> bool {
        !self.marks.is_empty() && self.marks.iter().all(|(_, ok)| *ok)
    }
}

// ── 2 跳地图（复用 F3356 模式）─────────────────────────────────────

/// 文档地图：边集 + 2 跳可达断言。
#[derive(Debug, Clone)]
pub struct DocMap2 {
    pub edges: Vec<(&'static str, &'static str)>,
}

impl DocMap2 {
    pub fn new() -> Self {
        Self { edges: Vec::new() }
    }

    pub fn link(&mut self, from: &'static str, to: &'static str) {
        self.edges.push((from, to));
    }

    /// 1 跳可达。
    pub fn one_hop(&self, from: &str) -> Vec<&'static str> {
        self.edges.iter().filter(|(f, _)| *f == from).map(|(_, t)| *t).collect()
    }

    /// 2 跳可达（BFS 层级）。
    pub fn two_hop(&self, from: &str) -> Vec<&'static str> {
        let mut out = self.one_hop(from);
        let mut extra = Vec::new();
        for mid in self.one_hop(from) {
            for t in self.one_hop(mid) {
                if t != from && !out.contains(&t) && !extra.contains(&t) {
                    extra.push(t);
                }
            }
        }
        out.extend(extra);
        out
    }

    /// 迷宫检出：任意对超过 2 跳不可达（小图穷举）。
    pub fn maze_found(&self, nodes: &[&'static str]) -> bool {
        for a in nodes.iter() {
            let reach = self.two_hop(a);
            for b in nodes.iter() {
                if a != b && !reach.contains(b) {
                    return true;
                }
            }
        }
        false
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves258_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves258_s11docs");

    // C4018-01 诊断码段不越窗。
    chk(&mut set, "C4018-01",
        T24DCode::MAZE.0 < SEG_BASE + 16 && SEG_BASE == 0x66C0, "err");

    // C4018-02 三册齐备表。
    chk(&mut set, "C4018-02",
        BOOK_KINDS.len() == 3
            && BOOK_KINDS[0].name() == "开发者册"
            && BOOK_KINDS[2].name() == "标准册", "err");

    // C4018-03 义务前置审查：缺义务章 → 拦。
    let mut ds = I18nDocSet::new();
    ds.build(BookKind::Dev, &["接入指南", "格式规范"], false, false);
    chk(&mut set, "C4018-03", !ds.duty_ok(), "err");

    // C4018-04 补义务章 → 过。
    ds.build(BookKind::Dev, &["接入指南", DUTY_CHAPTER, "格式规范"], true, false);
    chk(&mut set, "C4018-04", ds.duty_ok(), "err");

    // C4018-05 用户册可达断言。
    ds.build(BookKind::User, &["语言切换指南"], false, true);
    chk(&mut set, "C4018-05", ds.user_reachable(), "err");

    // C4018-06 用户册不可达 → 拦。
    ds.build(BookKind::User, &["语言切换指南"], false, false);
    chk(&mut set, "C4018-06", !ds.user_reachable(), "err");

    // C4018-07 标准册透明（CLDR + 覆盖表）。
    ds.build(BookKind::Standard, &["CLDR 版本", "覆盖表"], false, false);
    chk(&mut set, "C4018-07", ds.standard_transparent() && ds.complete(), "err");

    // C4018-08 标准册缺覆盖表 → 不透明。
    ds.build(BookKind::Standard, &["CLDR 版本"], false, false);
    chk(&mut set, "C4018-08", !ds.standard_transparent(), "err");

    // C4018-09 同源清单登记 + 重名拒。
    let mut sl = SourceList::new();
    sl.put("i18n 接入指南", "T01 册");
    chk(&mut set, "C4018-09", !sl.put("i18n 接入指南", "另源"), "err");

    // C4018-10 对账：指针空 → 漂移。
    sl.put("孤儿文档", "");
    chk(&mut set, "C4018-10", sl.reconcile() == T24DCode::DRIFT, "err");

    // C4018-11 指针齐 → 过。
    sl.put("孤儿文档", "补源");
    chk(&mut set, "C4018-11", sl.reconcile() == T24DCode::OK, "err");

    // C4018-12 自举：全过。
    let mut bs = BootstrapAudit::new();
    bs.put("开发者册", true);
    bs.put("用户册", true);
    chk(&mut set, "C4018-12", bs.bootstrapped(), "err");

    // C4018-13 自举：一文档不过 → 修。
    bs.put("标准册", false);
    chk(&mut set, "C4018-13", !bs.bootstrapped(), "err");

    // C4018-14 自举修复（重登更新）。
    bs.put("标准册", true);
    chk(&mut set, "C4018-14", bs.bootstrapped() && bs.marks.len() == 3, "err");

    // C4018-15 空自举册不误报。
    chk(&mut set, "C4018-15", !BootstrapAudit::new().bootstrapped(), "err");

    // C4018-16 2 跳地图：一跳查询。
    let mut m = DocMap2::new();
    m.link("总纲", "开发者册");
    m.link("开发者册", "格式规范");
    chk(&mut set, "C4018-16", m.one_hop("总纲") == vec!["开发者册"], "err");

    // C4018-17 2 跳可达：经开发者册到格式规范。
    let two = m.two_hop("总纲");
    chk(&mut set, "C4018-17",
        two.contains(&"开发者册") && two.contains(&"格式规范"), "err");

    // C4018-18 迷宫检出：孤岛节点。
    chk(&mut set, "C4018-18",
        m.maze_found(&["总纲", "开发者册", "格式规范", "孤岛"]), "err");

    // C4018-19 补边后迷宫消。
    m.link("孤岛", "总纲");
    chk(&mut set, "C4018-19",
        !m.maze_found(&["总纲", "开发者册", "格式规范", "孤岛"]), "err");

    // C4018-20 错误码互异。
    let codes = [T24DCode::OK.0, T24DCode::DRIFT.0, T24DCode::DUTY_MISS.0,
        T24DCode::BOOTSTRAP_FAIL.0, T24DCode::MAZE.0];
    let mut uniq = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] { uniq = false; }
        }
    }
    chk(&mut set, "C4018-20", uniq, "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duty_gate() {
        let mut ds = I18nDocSet::new();
        ds.build(BookKind::Dev, &["a"], false, false);
        assert!(!ds.duty_ok());
        ds.build(BookKind::Dev, &["a", DUTY_CHAPTER], true, false);
        assert!(ds.duty_ok());
    }

    #[test]
    fn two_hop_reach() {
        let mut m = DocMap2::new();
        m.link("a", "b");
        m.link("b", "c");
        assert!(m.two_hop("a").contains(&"c"));
    }

    #[test]
    fn bootstrap_gate() {
        let mut bs = BootstrapAudit::new();
        assert!(!bs.bootstrapped());
        bs.put("x", true);
        assert!(bs.bootstrapped());
    }
}
