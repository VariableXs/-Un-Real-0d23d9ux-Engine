//! VE-F3979 · 知识库归档
//!
//! S 域自查知识归档：自查模式/对账口径/补齐案例/演练发现四类归档入家族
//! 知识库。归档格式：家族模板条目化 + 可检索关键词。
//! 错误路径：归档缺类→补档；索引断→重建；条目不合模板→重写。
//! 性能：归档 O(条)；索引 O(关键词)；重建 O(索引)。
//! 跨批对接：家族知识库；F3965/F3972/F3977 案例源；F3997 域级归档汇总。
//! 无障碍与隐私：知识条目含无障碍自查专技（域本色）；案例脱敏后归档。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x36C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x36C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9SCode(pub u16);

impl S9SCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const CLASS_GAP: Self = Self(SEG_BASE + 1);
    pub const INDEX_BROKEN: Self = Self(SEG_BASE + 2);
    pub const TEMPLATE_MISMATCH: Self = Self(SEG_BASE + 3);
    pub const NOT_SANITIZED: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::CLASS_GAP => "归档缺类→补档",
            Self::INDEX_BROKEN => "索引断→重建",
            Self::TEMPLATE_MISMATCH => "条目不合模板→重写",
            Self::NOT_SANITIZED => "未脱敏→拒收",
            _ => "未知",
        }
    }
}

// ── 四类知识条目 ─────────────────────────────────────────────────────

/// 条目四类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KbKind {
    /// 自查模式。
    SelfAuditPattern,
    /// 对账口径。
    ReconCaliber,
    /// 补齐案例。
    FixCase,
    /// 演练发现。
    DrillFinding,
}

pub const KB_KINDS: [KbKind; 4] = [KbKind::SelfAuditPattern, KbKind::ReconCaliber, KbKind::FixCase, KbKind::DrillFinding];

/// 家族模板条目：标题/正文/关键词/脱敏标记。
#[derive(Debug, Clone)]
pub struct KbEntry {
    pub title: &'static str,
    pub kind: KbKind,
    pub body: &'static str,
    /// 检索关键词（可多个）。
    pub keywords: Vec<&'static str>,
    /// 脱敏（案例类必须 true）。
    pub sanitized: bool,
}

impl KbEntry {
    /// 模板合规：标题 + 正文 + 关键词非空。
    pub fn template_ok(&self) -> bool {
        !self.title.is_empty() && !self.body.is_empty() && !self.keywords.is_empty()
    }

    /// 案例脱敏断言。
    pub fn sanitize_ok(&self) -> bool {
        self.kind != KbKind::FixCase && self.kind != KbKind::DrillFinding || self.sanitized
    }
}

/// 知识库。
#[derive(Debug, Clone)]
pub struct KnowledgeBase {
    pub entries: Vec<KbEntry>,
}

impl KnowledgeBase {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// 归档：模板不合或未脱敏拒收。
    pub fn archive(&mut self, e: KbEntry) -> S9SCode {
        if !e.template_ok() {
            return S9SCode::TEMPLATE_MISMATCH;
        }
        if !e.sanitize_ok() {
            return S9SCode::NOT_SANITIZED;
        }
        self.entries.push(e);
        S9SCode::OK
    }

    /// 四类覆盖缺口。
    pub fn kind_gaps(&self) -> Vec<KbKind> {
        KB_KINDS.iter().filter(|k| !self.entries.iter().any(|e| e.kind == **k)).map(|k| *k).collect()
    }

    /// 检索 O(关键词)：按关键词命中条目标题。
    pub fn search(&self, kw: &str) -> Vec<&'static str> {
        self.entries
            .iter()
            .filter(|e| e.keywords.iter().any(|k| *k == kw))
            .map(|e| e.title)
            .collect()
    }

    /// 索引断检出：有关键词在册但检索命中为零。
    pub fn index_broken(&self, kw: &str) -> bool {
        let registered = self.entries.iter().any(|e| e.keywords.iter().any(|k| *k == kw));
        registered && self.search(kw).is_empty()
    }
}

impl Default for KnowledgeBase {
    fn default() -> Self {
        Self::new()
    }
}

// ── 索引重建 ─────────────────────────────────────────────────────────

/// 索引表：关键词 → 条目数。
#[derive(Debug, Clone)]
pub struct KwIndex {
    pub table: Vec<(&'static str, usize)>,
}

impl KwIndex {
    pub fn build(kb: &KnowledgeBase) -> Self {
        let mut table: Vec<(&'static str, usize)> = Vec::new();
        for e in kb.entries.iter() {
            for k in e.keywords.iter() {
                match table.iter_mut().find(|(n, _)| n == k) {
                    Some((_, c)) => *c += 1,
                    None => table.push((k, 1)),
                }
            }
        }
        Self { table }
    }

    pub fn count(&self, kw: &str) -> usize {
        self.table.iter().find(|(n, _)| *n == kw).map(|(_, c)| *c).unwrap_or(0)
    }

    /// 重建断言：索引计数与库实态一致。
    pub fn consistent(&self, kb: &KnowledgeBase) -> bool {
        self.table.iter().all(|(k, c)| *c == kb.search(k).len())
    }
}

impl Default for KwIndex {
    fn default() -> Self {
        Self { table: Vec::new() }
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves233_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves233_s09kb");

    fn entry(title: &'static str, kind: KbKind, kws: Vec<&'static str>, sanitized: bool) -> KbEntry {
        KbEntry { title, kind, body: "条目正文", keywords: kws, sanitized }
    }

    // 01 四类清单守恒
    {
        chk(&mut set, "C3980-01", KB_KINDS.len() == 4 && KbKind::SelfAuditPattern as usize == 0 && KbKind::DrillFinding as usize == 3, "err");
    }
    // 02 四类全归档
    {
        let mut kb = KnowledgeBase::new();
        for (i, k) in KB_KINDS.iter().enumerate() {
            kb.archive(entry("条目", *k, vec!["关键词"], true));
        }
        chk(&mut set, "C3980-02", kb.kind_gaps().is_empty() && kb.entries.len() == 4, "err");
    }
    // 03 模板不合拒收
    {
        let mut kb = KnowledgeBase::new();
        let bad = KbEntry { title: "", kind: KbKind::FixCase, body: "b", keywords: vec!["k"], sanitized: true };
        chk(&mut set, "C3980-03", !bad.template_ok() && kb.archive(bad) == S9SCode::TEMPLATE_MISMATCH, "err");
    }
    // 04 关键词缺拒收
    {
        let mut kb = KnowledgeBase::new();
        let bad = KbEntry { title: "t", kind: KbKind::SelfAuditPattern, body: "b", keywords: vec![], sanitized: false };
        chk(&mut set, "C3980-04", kb.archive(bad) == S9SCode::TEMPLATE_MISMATCH, "err");
    }
    // 05 案例未脱敏拒收
    {
        let mut kb = KnowledgeBase::new();
        chk(&mut set, "C3980-05", kb.archive(entry("裸案例", KbKind::FixCase, vec!["k"], false)) == S9SCode::NOT_SANITIZED, "err");
    }
    // 06 非案例免脱敏
    {
        let mut kb = KnowledgeBase::new();
        chk(&mut set, "C3980-06", kb.archive(entry("模式", KbKind::SelfAuditPattern, vec!["k"], false)) == S9SCode::OK, "err");
    }
    // 07 可检索命中
    {
        let mut kb = KnowledgeBase::new();
        kb.archive(entry("对比度口径", KbKind::ReconCaliber, vec!["对比度", "口径"], true));
        kb.archive(entry("焦点环案例", KbKind::FixCase, vec!["焦点", "口径"], true));
        chk(&mut set, "C3980-07", kb.search("口径").len() == 2 && kb.search("焦点").len() == 1 && kb.search("无").is_empty(), "err");
    }
    // 08 索引重建一致
    {
        let mut kb = KnowledgeBase::new();
        kb.archive(entry("甲", KbKind::SelfAuditPattern, vec!["k1", "k2"], true));
        kb.archive(entry("乙", KbKind::ReconCaliber, vec!["k1"], true));
        let ix = KwIndex::build(&kb);
        chk(&mut set, "C3980-08", ix.count("k1") == 2 && ix.count("k2") == 1 && ix.consistent(&kb), "err");
    }
    // 09 空索引一致性
    {
        let kb = KnowledgeBase::new();
        let ix = KwIndex::build(&kb);
        chk(&mut set, "C3980-09", ix.table.is_empty() && ix.consistent(&kb) && kb.kind_gaps().len() == 4, "err");
    }
    // 10 索引断检出（注册词零命中 = 断）
    {
        let mut kb = KnowledgeBase::new();
        // 手工构造：条目关键词在册但被清空关键词后检索失败的场景。
        kb.entries.push(KbEntry { title: "孤条", kind: KbKind::DrillFinding, body: "b", keywords: vec!["丢词"], sanitized: true });
        kb.entries[0].keywords.clear();
        chk(&mut set, "C3980-10", kb.index_broken("丢词"), "err");
    }
    // 11 补档闭环
    {
        let mut kb = KnowledgeBase::new();
        kb.archive(entry("一", KbKind::FixCase, vec!["k"], true));
        let gaps = kb.kind_gaps();
        for k in gaps.iter() {
            kb.archive(entry("补", *k, vec!["k"], true));
        }
        chk(&mut set, "C3980-11", kb.kind_gaps().is_empty(), "err");
    }
    // 12 专技条目在册断言
    {
        let mut kb = KnowledgeBase::new();
        kb.archive(entry("读屏走查专技", KbKind::SelfAuditPattern, vec!["专技"], true));
        chk(&mut set, "C3980-12", !kb.search("专技").is_empty(), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3980-13", S9SCode::NOT_SANITIZED.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9SCode::OK, S9SCode::CLASS_GAP, S9SCode::INDEX_BROKEN, S9SCode::TEMPLATE_MISMATCH, S9SCode::NOT_SANITIZED];
        chk(&mut set, "C3980-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_kb_all_green() {
        let s = run_ves233_checks();
        assert!(s.all_passed(), "ves233 red: {}", s.red_items_text());
    }

    #[test]
    fn test_sanitize_matrix() {
        let mut kb = KnowledgeBase::new();
        let case_dirty = KbEntry { title: "c", kind: KbKind::DrillFinding, body: "b", keywords: vec!["k"], sanitized: false };
        let case_clean = KbEntry { title: "c", kind: KbKind::DrillFinding, body: "b", keywords: vec!["k"], sanitized: true };
        assert_eq!(kb.archive(case_dirty), S9SCode::NOT_SANITIZED);
        assert_eq!(kb.archive(case_clean), S9SCode::OK);
    }

    #[test]
    fn test_index_consistency() {
        let mut kb = KnowledgeBase::new();
        for i in 0..5 {
            kb.archive(KbEntry { title: "t", kind: KB_KINDS[i % 4], body: "b", keywords: vec!["共享词"], sanitized: true }).ok();
        }
        let ix = KwIndex::build(&kb);
        assert_eq!(ix.count("共享词"), 5);
        assert!(ix.consistent(&kb));
    }
}
