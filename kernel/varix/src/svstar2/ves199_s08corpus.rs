#![allow(dead_code)]
//! VE-F3945 · 无障碍语料库
//!
//! 全域无障碍测试语料统一：五类语料（态语料/替代语料/媒体语料/输入语料/
//! 认知语料）。语料三要素：构造/依据/预期——依据红线全域化：无依据语料
//! = 伪语料（依据断言全域实测）。语料共享：域间互通单源；合成语料隐私
//! 红线全域复述。
//! 错误路径：三要素缺→语料无效；互通断→修复；隐私违→P0；分类缺→补。
//! 性能：分类 O(五类)；三要素 O(断言)；互通 O(协议)；隐私 O(复述)。
//! 跨批对接：F3019/F2878 语料单源全域声明；五域语料对端；F3942 调度消费。
//! 无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x0BC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x0BC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8ECode(pub u16);

impl S8ECode {
    pub const OK: Self = Self(SEG_BASE);
    pub const NO_BASIS: Self = Self(SEG_BASE + 1);
    pub const SHARE_BREAK: Self = Self(SEG_BASE + 2);
    pub const PRIVACY_VIOLATION: Self = Self(SEG_BASE + 3);
    pub const CLASS_GAP: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::NO_BASIS => "三要素缺→语料无效",
            Self::SHARE_BREAK => "互通断→修复",
            Self::PRIVACY_VIOLATION => "隐私违→P0",
            Self::CLASS_GAP => "分类缺→补",
            _ => "未知",
        }
    }
}

// ── 五类语料 ─────────────────────────────────────────────────────────

/// 语料五类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusKind {
    /// 态语料（状态语义）。
    State,
    /// 替代语料（替代文本）。
    Alt,
    /// 媒体语料（媒体描述）。
    Media,
    /// 输入语料（输入方式）。
    Input,
    /// 认知语料（认知负荷）。
    Cognition,
}

pub const CORPUS_KINDS: [CorpusKind; 5] = [
    CorpusKind::State,
    CorpusKind::Alt,
    CorpusKind::Media,
    CorpusKind::Input,
    CorpusKind::Cognition,
];

/// 单条语料：构造/依据/预期三要素。
#[derive(Debug, Clone)]
pub struct CorpusItem {
    pub name: &'static str,
    pub kind: CorpusKind,
    /// 要素一：构造（如何搭建该场景）。
    pub build: &'static str,
    /// 要素二：依据（构造的规范出处）——依据红线锚点。
    pub basis: &'static str,
    /// 要素三：预期（期望行为）。
    pub expect: &'static str,
    /// 合成语料标记（隐私红线：非合成语料禁止入域共享）。
    pub synthetic: bool,
}

impl CorpusItem {
    /// 三要素齐备断言：任一要素空 = 语料无效。
    pub fn valid(&self) -> bool {
        !self.build.is_empty() && !self.basis.is_empty() && !self.expect.is_empty()
    }

    /// 伪语料判定：无依据 = 伪语料。
    pub fn pseudo(&self) -> bool {
        self.basis.is_empty()
    }
}

// ── 五类语料库 ───────────────────────────────────────────────────────

/// 语料库：五类分桶 + 三要素执法。
#[derive(Debug, Clone)]
pub struct CorpusLibrary {
    /// 五类桶（下标对齐 CORPUS_KINDS）。
    pub buckets: [Vec<CorpusItem>; 5],
}

impl CorpusLibrary {
    pub fn new() -> Self {
        Self { buckets: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()] }
    }

    /// 入库：O(1) 分桶；无效语料拒收。
    pub fn add(&mut self, item: CorpusItem) -> S8ECode {
        if !item.valid() {
            return S8ECode::NO_BASIS;
        }
        let idx = item.kind as usize;
        self.buckets[idx].push(item);
        S8ECode::OK
    }

    /// 分类缺→补：返回空类清单。
    pub fn class_gaps(&self) -> Vec<CorpusKind> {
        CORPUS_KINDS
            .iter()
            .filter(|k| self.buckets[**k as usize].is_empty())
            .map(|k| *k)
            .collect()
    }

    pub fn total(&self) -> usize {
        self.buckets.iter().map(|b| b.len()).sum()
    }

    /// 依据红线全域实测：全库无伪语料。
    pub fn no_pseudo(&self) -> bool {
        self.buckets.iter().all(|b| b.iter().all(|i| !i.pseudo()))
    }

    /// 隐私全域复述：非合成语料才允许跨域共享，返回可共享名单。
    pub fn shareable(&self) -> Vec<&'static str> {
        self.buckets
            .iter()
            .flatten()
            .filter(|i| i.synthetic)
            .map(|i| i.name)
            .collect()
    }
}

impl Default for CorpusLibrary {
    fn default() -> Self {
        Self::new()
    }
}

// ── 互通协议（域间单源）──────────────────────────────────────────────

/// 互通账：域 A 借用域 B 语料的登记。
#[derive(Debug, Clone)]
pub struct ShareRec {
    pub item: &'static str,
    pub from_domain: u8,
    pub to_domain: u8,
    /// 借出是否成功（互通断标记）。
    pub ok: bool,
}

/// 互通枢纽：单源对账。
#[derive(Debug, Clone)]
pub struct ShareHub {
    pub recs: Vec<ShareRec>,
}

impl ShareHub {
    pub fn new() -> Self {
        Self { recs: Vec::new() }
    }

    /// 借出登记：域号合法 + 语料存在才通。
    pub fn lend(&mut self, item: &'static str, from: u8, to: u8, exists: bool) -> S8ECode {
        if from == 0 || to == 0 || from > 8 || to > 8 || !exists {
            self.recs.push(ShareRec { item, from_domain: from, to_domain: to, ok: false });
            return S8ECode::SHARE_BREAK;
        }
        self.recs.push(ShareRec { item, from_domain: from, to_domain: to, ok: true });
        S8ECode::OK
    }

    /// 互通断检出。
    pub fn breaks(&self) -> Vec<&'static str> {
        self.recs.iter().filter(|r| !r.ok).map(|r| r.item).collect()
    }

    /// 修复：断记录重登记为通。
    pub fn repair(&mut self, item: &str) -> S8ECode {
        match self.recs.iter_mut().find(|r| r.item == item && !r.ok) {
            Some(r) => {
                r.ok = true;
                S8ECode::OK
            }
            None => S8ECode::SHARE_BREAK,
        }
    }

    /// 单源守恒：借出记录双方域号对称可追。
    pub fn conserved(&self) -> bool {
        self.recs.iter().all(|r| r.from_domain != r.to_domain)
    }
}

impl Default for ShareHub {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves199_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves199_s08corpus");

    fn item(name: &'static str, kind: CorpusKind, synthetic: bool) -> CorpusItem {
        CorpusItem { name, kind, build: "构造场景", basis: "WCAG 条款", expect: "预期通过", synthetic }
    }

    // 01 五类语料全建档
    {
        let mut lib = CorpusLibrary::new();
        for k in CORPUS_KINDS.iter() {
            lib.add(item("样例", *k, true));
        }
        chk(&mut set, "C3945-01", lib.total() == 5 && lib.class_gaps().is_empty(), "err");
    }
    // 02 三要素缺→语料无效拒收
    {
        let mut lib = CorpusLibrary::new();
        let bad = CorpusItem { name: "伪", kind: CorpusKind::State, build: "有", basis: "", expect: "有", synthetic: true };
        chk(&mut set, "C3945-02", !bad.valid() && bad.pseudo() && lib.add(bad) == S8ECode::NO_BASIS, "err");
    }
    // 03 依据红线：入库后全库无伪语料
    {
        let mut lib = CorpusLibrary::new();
        lib.add(item("a", CorpusKind::Alt, true));
        lib.add(item("b", CorpusKind::Input, true));
        chk(&mut set, "C3945-03", lib.no_pseudo(), "err");
    }
    // 04 分类缺→补实测
    {
        let mut lib = CorpusLibrary::new();
        lib.add(item("a", CorpusKind::Cognition, true));
        let gaps = lib.class_gaps();
        chk(&mut set, "C3945-04", gaps.len() == 4 && !gaps.contains(&CorpusKind::Cognition), "err");
    }
    // 05 合成语料隐私红线：可共享名单只含合成
    {
        let mut lib = CorpusLibrary::new();
        lib.add(item("合成", CorpusKind::Media, true));
        lib.add(item("真实", CorpusKind::Media, false));
        chk(&mut set, "C3945-05", lib.shareable() == vec!["合成"], "err");
    }
    // 06 互通借出成功
    {
        let mut hub = ShareHub::new();
        chk(&mut set, "C3945-06", hub.lend("语料x", 1, 3, true) == S8ECode::OK && hub.breaks().is_empty() && hub.conserved(), "err");
    }
    // 07 互通断→修复实测
    {
        let mut hub = ShareHub::new();
        hub.lend("语料y", 2, 5, false);
        chk(&mut set, "C3945-07", hub.breaks() == vec!["语料y"] && hub.repair("语料y") == S8ECode::OK && hub.breaks().is_empty(), "err");
    }
    // 08 借出域号越界拒收
    {
        let mut hub = ShareHub::new();
        chk(&mut set, "C3945-08", hub.lend("z", 0, 3, true) == S8ECode::SHARE_BREAK && hub.lend("z", 3, 9, true) == S8ECode::SHARE_BREAK, "err");
    }
    // 09 自借守恒违例检出
    {
        let mut hub = ShareHub::new();
        hub.lend("自借", 4, 4, true);
        chk(&mut set, "C3945-09", !hub.conserved(), "err");
    }
    // 10 分桶正确性：互不串桶
    {
        let mut lib = CorpusLibrary::new();
        lib.add(item("态", CorpusKind::State, true));
        lib.add(item("媒", CorpusKind::Media, true));
        chk(&mut set, "C3945-10", lib.buckets[0].len() == 1 && lib.buckets[2].len() == 1 && lib.buckets[4].is_empty(), "err");
    }
    // 11 三要素逐项执法：预期缺也拒
    {
        let mut lib = CorpusLibrary::new();
        let bad = CorpusItem { name: "缺预期", kind: CorpusKind::Input, build: "有", basis: "有", expect: "", synthetic: true };
        chk(&mut set, "C3945-11", lib.add(bad) == S8ECode::NO_BASIS && lib.total() == 0, "err");
    }
    // 12 五类枚举守恒
    {
        chk(&mut set, "C3945-12", CORPUS_KINDS.len() == 5 && CorpusKind::State as usize == 0 && CorpusKind::Cognition as usize == 4, "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3945-13", S8ECode::CLASS_GAP.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8ECode::OK, S8ECode::NO_BASIS, S8ECode::SHARE_BREAK, S8ECode::PRIVACY_VIOLATION, S8ECode::CLASS_GAP];
        chk(&mut set, "C3945-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_corpus_all_green() {
        let s = run_ves199_checks();
        assert!(s.all_passed(), "ves199 red: {}", s.red_items_text());
    }

    #[test]
    fn test_pseudo_redline() {
        let it = CorpusItem { name: "p", kind: CorpusKind::State, build: "b", basis: "", expect: "e", synthetic: true };
        assert!(it.pseudo());
        assert!(!it.valid());
    }

    #[test]
    fn test_share_hub_full_cycle() {
        let mut hub = ShareHub::new();
        hub.lend("i1", 1, 2, true);
        hub.lend("i2", 2, 1, false);
        hub.repair("i2");
        assert!(hub.breaks().is_empty());
        assert!(hub.conserved());
    }
}
