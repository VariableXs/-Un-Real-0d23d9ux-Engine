//! VE-F3716 · 质量文档
//!
//! 评分规则册（五维算法全公开）、改进指南、集成指南、同源纪律复述、2 跳地图复用。
//! 规则不全 → 补公开；指南失效 → 断言；漂移 → 对账。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xA4xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xA400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CA4Code(pub u16);

impl CA4Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const RULE_INCOMPLETE: Self = Self(SEG_BASE + 1);
    pub const GUIDE_STALE: Self = Self(SEG_BASE + 2);
    pub const DRIFT: Self = Self(SEG_BASE + 3);
    pub const MAZE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::RULE_INCOMPLETE => "规则册不全，需补公开",
            Self::GUIDE_STALE => "指南失效，需更新",
            Self::DRIFT => "文档漂移，需对账",
            Self::MAZE => "文档迷宫：超过 2 跳不可达",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 文档章节
#[derive(Debug, Clone)]
pub struct DocChapter {
    pub id: u32,
    pub title_hash: u64,
    pub published: bool,
    pub hops_from_root: u8,
}

/// 规则册条目
#[derive(Debug, Clone)]
pub struct RuleEntry {
    pub dim: u8,
    pub published: bool,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct QualDocs {
    chapters: Vec<DocChapter>,
    rules: Vec<RuleEntry>,
    integration_guides: u32,
}

impl QualDocs {
    pub fn new() -> Self {
        Self { chapters: Vec::new(), rules: Vec::new(), integration_guides: 0 }
    }

    pub fn add_chapter(&mut self, ch: DocChapter) {
        self.chapters.push(ch);
    }

    pub fn add_rule(&mut self, r: RuleEntry) {
        self.rules.push(r);
    }

    /// 规则全公开断言：五维各至少一条已发布
    pub fn assert_rules_complete(&self) -> Result<(), CA4Code> {
        for dim in 0..5u8 {
            if !self.rules.iter().any(|r| r.dim == dim && r.published) {
                return Err(CA4Code::RULE_INCOMPLETE);
            }
        }
        Ok(())
    }

    /// 2 跳地图断言
    pub fn assert_two_hop(&self) -> Result<(), CA4Code> {
        if self.chapters.iter().any(|c| c.hops_from_root > 2) {
            Err(CA4Code::MAZE)
        } else {
            Ok(())
        }
    }

    /// 指南有效性
    pub fn assert_guides_fresh(&self, required: u32) -> Result<(), CA4Code> {
        if self.integration_guides >= required {
            Ok(())
        } else {
            Err(CA4Code::GUIDE_STALE)
        }
    }

    pub fn add_guide(&mut self) {
        self.integration_guides += 1;
    }

    /// 文档对账：章节发布数与预期一致
    pub fn assert_drift(&self, expected_published: usize) -> Result<(), CA4Code> {
        let actual = self.chapters.iter().filter(|c| c.published).count();
        if actual == expected_published {
            Ok(())
        } else {
            Err(CA4Code::DRIFT)
        }
    }
}

impl Default for QualDocs {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves116_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves116_qualdocs");

    // 01 规则不全
    {
        let mut d = QualDocs::new();
        d.add_rule(RuleEntry { dim: 0, published: true });
        chk(&mut set, "C3716-01", d.assert_rules_complete() == Err(CA4Code::RULE_INCOMPLETE), "err");
    }
    // 02 规则全
    {
        let mut d = QualDocs::new();
        for dim in 0..5u8 {
            d.add_rule(RuleEntry { dim, published: true });
        }
        chk(&mut set, "C3716-02", d.assert_rules_complete().is_ok(), "err");
    }
    // 03 2 跳超限
    {
        let mut d = QualDocs::new();
        d.add_chapter(DocChapter { id: 1, title_hash: 0, published: true, hops_from_root: 3 });
        chk(&mut set, "C3716-03", d.assert_two_hop() == Err(CA4Code::MAZE), "err");
    }
    // 04 2 跳内
    {
        let mut d = QualDocs::new();
        d.add_chapter(DocChapter { id: 1, title_hash: 0, published: true, hops_from_root: 2 });
        chk(&mut set, "C3716-04", d.assert_two_hop().is_ok(), "err");
    }
    // 05 指南不足
    {
        let d = QualDocs::new();
        chk(&mut set, "C3716-05", d.assert_guides_fresh(1) == Err(CA4Code::GUIDE_STALE), "err");
    }
    // 06 指南足够
    {
        let mut d = QualDocs::new();
        d.add_guide();
        chk(&mut set, "C3716-06", d.assert_guides_fresh(1).is_ok(), "err");
    }
    // 07 漂移
    {
        let mut d = QualDocs::new();
        d.add_chapter(DocChapter { id: 1, title_hash: 0, published: true, hops_from_root: 1 });
        chk(&mut set, "C3716-07", d.assert_drift(2) == Err(CA4Code::DRIFT), "err");
    }
    // 08 对账一致
    {
        let mut d = QualDocs::new();
        d.add_chapter(DocChapter { id: 1, title_hash: 0, published: true, hops_from_root: 1 });
        chk(&mut set, "C3716-08", d.assert_drift(1).is_ok(), "err");
    }
    // 09 空规则册
    {
        let d = QualDocs::new();
        chk(&mut set, "C3716-09", d.assert_rules_complete().is_err(), "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3716-10", !CA4Code::RULE_INCOMPLETE.explain().is_empty(), "err");
    }

    set
}
