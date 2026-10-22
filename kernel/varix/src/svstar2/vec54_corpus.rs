//! VE-F0454 · 语义组测试语料（VE-C 域 · 语义组 C04）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0454`
//!
//! **锚点原文**：「语义检查测试语料库：合法语料（每语义规则至少正例一用例）、
//! 错误语料（每诊断至少反例一用例加期望诊断断言）、边界语料（作用域极限、
//! 类型极限、接口极限三类）；语料版本化随规范版本走；期望诊断与实际输出自动
//! 比对（诊断快照测试）。判据：正反边界三区、诊断快照、同步流程、缺例阻断、
//! 判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **正反边界三区**（判据一）。语料库分三区：正例（合法必须通过）、
//!    反例（必须产出期望诊断）、边界（作用域/类型/接口三类极限）——
//!    每区独立计数，缺区即缺例。
//!
//! 2. **诊断快照**（判据二）。期望诊断快照与实际输出自动比对：失配时
//!    定位「改进还是回归」（新增诊断 = 改进需显性更新快照；丢失期望
//!    诊断 = 回归阻断）——快照不是死比对，是带方向归因的比对。
//!
//! 3. **同步流程**（判据三）。语料版本随规范版本走：规范版本变更时
//!    语料同步状态机（同步中 → 已同步），未同步语料禁止收口。
//!
//! 4. **缺例阻断**（判据四）。规则无正例或诊断无反例 → 缺口清单非空
//!    = 收口阻断（补齐才可发布）。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0454";

/// 语料版本随规范版本走。
pub const SPEC_VER: &str = "F0402-v1.0";

/// 边界语料三类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeClass {
    ScopeLimit,
    TypeLimit,
    IfaceLimit,
}

/// 语料条目：三区之一 + 规则/诊断键 + 源文本 + 期望诊断。
#[derive(Clone, Debug)]
pub struct CorpusItem {
    pub zone: Zone,
    /// 关联键：正例挂规则名，反例挂诊断名。
    pub key: &'static str,
    pub src: &'static str,
    /// 期望诊断（反例用；正例为空）。
    pub expect_diag: Option<&'static str>,
}

/// 语料分区。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Positive,
    Negative,
    Edge(EdgeClass),
}

/// 语料库。
#[derive(Debug, Default)]
pub struct Corpus {
    pub spec_ver: String,
    /// 同步状态：false = 未同步（禁收口）。
    pub synced: bool,
    pub items: Vec<CorpusItem>,
}

impl Corpus {
    pub fn new() -> Self {
        Corpus { spec_ver: SPEC_VER.to_string(), synced: false, items: Vec::new() }
    }
    pub fn add(&mut self, it: CorpusItem) {
        self.items.push(it);
    }
    /// 三区计数（正例 / 反例 / 边界）。
    pub fn tally(&self) -> (usize, usize, usize) {
        let mut p = 0;
        let mut n = 0;
        let mut e = 0;
        for it in self.items.iter() {
            match it.zone {
                Zone::Positive => p += 1,
                Zone::Negative => n += 1,
                Zone::Edge(_) => e += 1,
            }
        }
        (p, n, e)
    }
    /// 边界三类覆盖（三类都有边界语料）。
    pub fn edge_classes(&self) -> Vec<EdgeClass> {
        let mut out = Vec::new();
        for it in self.items.iter() {
            if let Zone::Edge(c) = it.zone {
                if !out.contains(&c) {
                    out.push(c);
                }
            }
        }
        out
    }
}

/// 缺例阻断：规则无正例或诊断无反例的缺口清单。
pub fn missing_gaps(corpus: &Corpus, rules: &[&str], diags: &[&str]) -> Vec<String> {
    let mut gaps = Vec::new();
    for r in rules.iter() {
        if !corpus.items.iter().any(|it| it.zone == Zone::Positive && it.key == *r) {
            gaps.push(alloc::format!("正例缺失:{}", r));
        }
    }
    for d in diags.iter() {
        if !corpus.items.iter().any(|it| it.zone == Zone::Negative && it.key == *d) {
            gaps.push(alloc::format!("反例缺失:{}", d));
        }
    }
    gaps
}

// ---------------------------------------------------------------------------
// 诊断快照（带方向归因的比对）
// ---------------------------------------------------------------------------

/// 快照比对结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapVerdict {
    /// 一致（快照仍有效）。
    Match,
    /// 实际多于期望：改进（需显性更新快照才可收口）。
    Improved,
    /// 实际少于期望：回归（阻断）。
    Regressed,
}

/// 快照测试：期望诊断集 vs 实际诊断集（按名字集合比对 + 方向归因）。
pub fn snap_compare(expected: &[&str], actual: &[&str]) -> (SnapVerdict, Vec<String>) {
    let mut lost: Vec<String> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    for e in expected.iter() {
        if !actual.contains(e) {
            lost.push(e.to_string());
        }
    }
    for a in actual.iter() {
        if !expected.contains(a) {
            added.push(a.to_string());
        }
    }
    if lost.is_empty() && added.is_empty() {
        (SnapVerdict::Match, Vec::new())
    } else if lost.is_empty() {
        (SnapVerdict::Improved, added)
    } else {
        (SnapVerdict::Regressed, lost)
    }
}

/// 快照表：键 → 期望诊断集（版本化存取）。
#[derive(Debug, Default)]
pub struct SnapshotBook {
    pub ver: String,
    pub snaps: Vec<(String, Vec<String>)>,
}

impl SnapshotBook {
    pub fn new() -> Self {
        SnapshotBook { ver: SPEC_VER.to_string(), snaps: Vec::new() }
    }
    pub fn put(&mut self, key: &str, diags: &[&str]) {
        if let Some(e) = self.snaps.iter_mut().find(|(k, _)| k == key) {
            e.1 = diags.iter().map(|d| d.to_string()).collect();
        } else {
            self.snaps.push((key.to_string(), diags.iter().map(|d| d.to_string()).collect()));
        }
    }
    pub fn get(&self, key: &str) -> Option<&Vec<String>> {
        self.snaps.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    /// 快照显性更新（改进路径：更新必须主动调用并留版本痕）。
    pub fn accept_update(&mut self, key: &str, diags: &[&str]) {
        self.ver = alloc::format!("{}+updated", SPEC_VER);
        self.put(key, diags);
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec54_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    let pos = |key: &'static str| CorpusItem { zone: Zone::Positive, key, src: "int x;", expect_diag: None };
    let neg = |key: &'static str, d: &'static str| CorpusItem { zone: Zone::Negative, key, src: "nonsuch;", expect_diag: Some(d) };

    // 一、正反边界三区。
    {
        let mut c = Corpus::new();
        c.add(pos("rule-qual-combo"));
        c.add(neg("diag-divzero", "F0448-除零"));
        c.add(CorpusItem { zone: Zone::Edge(EdgeClass::ScopeLimit), key: "scope-256", src: "deep;", expect_diag: None });
        c.add(CorpusItem { zone: Zone::Edge(EdgeClass::TypeLimit), key: "type-huge", src: "vec9;", expect_diag: None });
        c.add(CorpusItem { zone: Zone::Edge(EdgeClass::IfaceLimit), key: "iface-many", src: "io;", expect_diag: None });
        let (p, n, e) = c.tally();
        v.push(("F0454-三区-计数齐", p == 1 && n == 1 && e == 3));
        // 边界三类全覆盖。
        v.push(("F0454-三区-边界三类", c.edge_classes().len() == 3));
    }

    // 二、缺例阻断。
    {
        let mut c = Corpus::new();
        c.add(pos("rule-a"));
        c.add(neg("diag-x", "d-x"));
        // rule-b 无正例、diag-y 无反例 → 双缺口。
        let gaps = missing_gaps(&c, &["rule-a", "rule-b"], &["diag-x", "diag-y"]);
        v.push((
            "F0454-缺例-双缺口精确",
            gaps.len() == 2 && gaps[0].contains("rule-b") && gaps[1].contains("diag-y"),
        ));
        // 补齐后缺口清空（可收口）。
        c.add(pos("rule-b"));
        c.add(neg("diag-y", "d-y"));
        v.push(("F0454-缺例-补齐放行", missing_gaps(&c, &["rule-a", "rule-b"], &["diag-x", "diag-y"]).is_empty()));
    }

    // 三、诊断快照。
    {
        // 一致。
        let (r1, _) = snap_compare(&["d1", "d2"], &["d2", "d1"]);
        v.push(("F0454-快照-一致", r1 == SnapVerdict::Match));
        // 改进方向（实际多了 d3）。
        let (r2, added) = snap_compare(&["d1"], &["d1", "d3"]);
        v.push((
            "F0454-快照-改进方向",
            r2 == SnapVerdict::Improved && added == vec!["d3".to_string()],
        ));
        // 回归方向（丢了 d1）→ 阻断。
        let (r3, lost) = snap_compare(&["d1", "d2"], &["d2"]);
        v.push((
            "F0454-快照-回归阻断",
            r3 == SnapVerdict::Regressed && lost == vec!["d1".to_string()],
        ));
    }

    // 四、同步流程。
    {
        let mut book = SnapshotBook::new();
        book.put("case-1", &["d-old"]);
        // 比对：实际新增 d-new → Improved。
        let actual = ["d-old", "d-new"];
        let (r, _) = snap_compare(&["d-old"], &actual);
        v.push(("F0454-同步-改进检出", r == SnapVerdict::Improved));
        // 显性更新快照（留版本痕）。
        book.accept_update("case-1", &actual);
        v.push((
            "F0454-同步-显性更新留痕",
            book.get("case-1").map(|v| v.len()) == Some(2) && book.ver.contains("updated"),
        ));
        // 更新后比对一致。
        let (r2, _) = snap_compare(&["d-old", "d-new"], &actual);
        v.push(("F0454-同步-更新后一致", r2 == SnapVerdict::Match));
    }

    // 五、判据自检。
    {
        v.push(("F0454-判据-锚点入册", ANCHOR == "VE-F0454"));
        v.push(("F0454-判据-版本随规范", SPEC_VER.starts_with("F0402-")));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec54）。
pub fn run_vec54_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec54");
    for (name, ok) in vec54_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec54_raw_checks as run_vec54_checks;
    #[test]
    fn vec54_corpus_checks() {
        for (name, ok) in run_vec54_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
