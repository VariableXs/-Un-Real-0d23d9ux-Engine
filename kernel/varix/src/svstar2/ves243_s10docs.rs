//! VE-F3989 · S 域文档与 schema 总审
//!
//! 总审：S 域文档总审（八组文档齐备 + 无障碍使用指南专册终版 + F3971
//! 对拍收口）+ schema 总审（配置 / 色标 / 语义树 / 埋点四 schema 终版 +
//! 版本冻结）。
//! 数据结构：文档总审册；schema 终版（四套）；冻结记录。
//! 错误路径：文档缺章→补章；schema 漂移→对版；冻结后发现→F3996 纪律。
//! 性能：总审 O(册)；对版 O(schema)；冻结 O(1)。
//! 跨批对接：F3971 上游；F3995 移交包内容源。
//! 无障碍与隐私：专册终审含替代文本 / 对比度示例规范（域本色）；示例
//! 无真实数据。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x43C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x43C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T9DCode(pub u16);

impl T9DCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const CHAPTER_GAP: Self = Self(SEG_BASE + 1);
    pub const SCHEMA_DRIFT: Self = Self(SEG_BASE + 2);
    pub const POST_FROZEN: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::CHAPTER_GAP => "文档缺章→补章",
            Self::SCHEMA_DRIFT => "schema 漂移→对版",
            Self::POST_FROZEN => "冻结后发现→变更纪律",
            _ => "未知",
        }
    }
}

// ── 八组文档 ─────────────────────────────────────────────────────────

pub const DOC_GROUPS: [&str; 8] = [
    "渲染册", "高对比册", "读屏册", "动效册", "认知册", "输入册", "性能册", "工具册",
];

/// 无障碍使用指南专册（终版要求：示例规范齐）。
#[derive(Debug, Clone)]
pub struct GuideBook {
    /// 替代文本示例规范。
    pub alt_text_spec: bool,
    /// 对比度示例规范。
    pub contrast_spec: bool,
    /// 终版标记。
    pub final_ok: bool,
}

impl GuideBook {
    pub fn new() -> Self {
        Self { alt_text_spec: false, contrast_spec: false, final_ok: false }
    }

    /// 终版断言：两规范齐 + 终版标记。
    pub fn complete(&self) -> bool {
        self.alt_text_spec && self.contrast_spec && self.final_ok
    }
}

/// 文档总审册。
#[derive(Debug, Clone)]
pub struct DocMasterBook {
    /// 各组文档已交。
    pub delivered: Vec<&'static str>,
    /// 专册。
    pub guide: GuideBook,
    /// 对拍收口（F3971 资产对拍）。
    pub peer_reconciled: bool,
}

impl DocMasterBook {
    pub fn new() -> Self {
        Self { delivered: Vec::new(), guide: GuideBook::new(), peer_reconciled: false }
    }

    pub fn deliver(&mut self, group: &'static str) -> bool {
        if !DOC_GROUPS.contains(&group) || self.delivered.contains(&group) {
            return false;
        }
        self.delivered.push(group);
        true
    }

    /// 缺章清单。
    pub fn missing_chapters(&self) -> Vec<&'static str> {
        DOC_GROUPS.iter().filter(|g| !self.delivered.contains(g)).map(|g| *g).collect()
    }

    /// 总审通过：八章齐 + 专册终版 + 对拍收口。
    pub fn pass(&self) -> T9DCode {
        if !self.missing_chapters().is_empty() {
            return T9DCode::CHAPTER_GAP;
        }
        if !self.guide.complete() {
            return T9DCode::CHAPTER_GAP;
        }
        if !self.peer_reconciled {
            return T9DCode::SCHEMA_DRIFT;
        }
        T9DCode::OK
    }
}

// ── 四 schema 终版 ───────────────────────────────────────────────────

/// schema 四套。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaKind {
    /// 配置 schema。
    Config,
    /// 色标 schema。
    ColorMap,
    /// 语义树 schema。
    SemTree,
    /// 埋点 schema。
    Telemetry,
}

pub const SCHEMA_KINDS: [SchemaKind; 4] = [
    SchemaKind::Config, SchemaKind::ColorMap, SchemaKind::SemTree, SchemaKind::Telemetry,
];

impl SchemaKind {
    pub fn name(self) -> &'static str {
        match self {
            SchemaKind::Config => "配置",
            SchemaKind::ColorMap => "色标",
            SchemaKind::SemTree => "语义树",
            SchemaKind::Telemetry => "埋点",
        }
    }

    pub fn slot(self) -> usize {
        match self {
            SchemaKind::Config => 0,
            SchemaKind::ColorMap => 1,
            SchemaKind::SemTree => 2,
            SchemaKind::Telemetry => 3,
        }
    }
}

/// schema 条目：版本 + 终版标记。
#[derive(Debug, Clone)]
pub struct SchemaEntry {
    pub ver: u32,
    pub final_ok: bool,
}

pub const SCHEMA_VER: u32 = 3;

/// schema 终版册（可冻结）。
#[derive(Debug, Clone)]
pub struct SchemaBook {
    pub entries: [Option<SchemaEntry>; 4],
    pub frozen: bool,
    /// 冻结版本快照。
    pub frozen_vers: [u32; 4],
}

impl SchemaBook {
    pub fn new() -> Self {
        Self {
            entries: [None, None, None, None],
            frozen: false,
            frozen_vers: [0; 4],
        }
    }

    /// 登记终版 schema（冻结后拒绝——F3996 纪律）。
    pub fn put(&mut self, kind: SchemaKind, ver: u32, final_ok: bool) -> bool {
        if self.frozen {
            return false;
        }
        self.entries[kind.slot()] = Some(SchemaEntry { ver, final_ok });
        true
    }

    /// 漂移检出：缺套或版本不齐或非终版。
    pub fn drift(&self) -> bool {
        self.entries.iter().any(|e| match e {
            Some(s) => s.ver != SCHEMA_VER || !s.final_ok,
            None => true,
        })
    }

    /// 版本冻结（漂移即拒）。
    pub fn freeze(&mut self) -> T9DCode {
        if self.frozen {
            return T9DCode::OK;
        }
        if self.drift() {
            return T9DCode::SCHEMA_DRIFT;
        }
        for (i, e) in self.entries.iter().enumerate() {
            self.frozen_vers[i] = e.as_ref().map(|s| s.ver).unwrap_or(0);
        }
        self.frozen = true;
        T9DCode::OK
    }

    /// 冻结哈希（四版本合成 FNV）。
    pub fn freeze_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for v in self.frozen_vers.iter() {
            h ^= *v as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves243_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves243_s10docs");

    // C3989-01 诊断码段不越窗。
    chk(&mut set, "C3989-01",
        T9DCode::POST_FROZEN.0 < SEG_BASE + 16 && SEG_BASE == 0x43C0, "err");

    // C3989-02 八组文档齐备表。
    chk(&mut set, "C3989-02",
        DOC_GROUPS.len() == 8 && DOC_GROUPS[0] == "渲染册" && DOC_GROUPS[7] == "工具册", "err");

    // C3989-03 缺章检出。
    let mut d = DocMasterBook::new();
    for g in DOC_GROUPS.iter().take(6) {
        d.deliver(g);
    }
    chk(&mut set, "C3989-03",
        d.missing_chapters() == vec!["性能册", "工具册"], "err");

    // C3989-04 八章齐 + 专册未终版 → 仍缺。
    d.deliver("性能册");
    d.deliver("工具册");
    chk(&mut set, "C3989-04",
        d.missing_chapters().is_empty() && d.pass() == T9DCode::CHAPTER_GAP, "err");

    // C3989-05 专册两规范齐 + 终版 → 过此关。
    d.guide.alt_text_spec = true;
    d.guide.contrast_spec = true;
    d.guide.final_ok = true;
    chk(&mut set, "C3989-05", d.guide.complete(), "err");

    // C3989-06 对拍未收口 → SCHEMA_DRIFT 路径。
    chk(&mut set, "C3989-06", d.pass() == T9DCode::SCHEMA_DRIFT, "err");

    // C3989-07 对拍收口 → 总审过。
    d.peer_reconciled = true;
    chk(&mut set, "C3989-07", d.pass() == T9DCode::OK, "err");

    // C3989-08 专册缺对比度规范 → 拒。
    let mut d2 = DocMasterBook::new();
    for g in DOC_GROUPS.iter() {
        d2.deliver(g);
    }
    d2.guide.alt_text_spec = true;
    d2.guide.final_ok = true;
    chk(&mut set, "C3989-08", !d2.guide.complete() && d2.pass() == T9DCode::CHAPTER_GAP, "err");

    // C3989-09 越名文档组拒绝 + 重复交档拒绝。
    let mut d3 = DocMasterBook::new();
    d3.deliver("渲染册");
    chk(&mut set, "C3989-09", !d3.deliver("伪册") && !d3.deliver("渲染册"), "err");

    // C3989-10 四 schema 套齐备。
    chk(&mut set, "C3989-10",
        SCHEMA_KINDS.len() == 4
            && SCHEMA_KINDS[0].name() == "配置"
            && SCHEMA_KINDS[3].name() == "埋点", "err");

    // C3989-11 schema 漂移检出（缺套）。
    let mut s = SchemaBook::new();
    for k in SCHEMA_KINDS.iter().take(3) {
        s.put(*k, SCHEMA_VER, true);
    }
    chk(&mut set, "C3989-11", s.drift() && s.freeze() == T9DCode::SCHEMA_DRIFT, "err");

    // C3989-12 四套齐 → 冻结过 + 版本快照。
    s.put(SchemaKind::Telemetry, SCHEMA_VER, true);
    chk(&mut set, "C3989-12",
        s.freeze() == T9DCode::OK
            && s.frozen_vers == [SCHEMA_VER; 4]
            && s.freeze_hash() != 0, "err");

    // C3989-13 版本不齐 → 漂移。
    let mut s2 = SchemaBook::new();
    for k in SCHEMA_KINDS.iter() {
        let v = if *k == SchemaKind::SemTree { 2 } else { SCHEMA_VER };
        s2.put(*k, v, true);
    }
    chk(&mut set, "C3989-13", s2.drift() && s2.freeze() == T9DCode::SCHEMA_DRIFT, "err");

    // C3989-14 非终版条目 → 漂移。
    let mut s3 = SchemaBook::new();
    for k in SCHEMA_KINDS.iter() {
        s3.put(*k, SCHEMA_VER, *k != SchemaKind::ColorMap);
    }
    chk(&mut set, "C3989-14", s3.drift(), "err");

    // C3989-15 冻结后写入拒绝（F3996 纪律衔接）。
    chk(&mut set, "C3989-15", !s.put(SchemaKind::Config, 9, true), "err");

    // C3989-16 冻结哈希随版本变。
    let h1 = s.freeze_hash();
    let mut s4 = SchemaBook::new();
    for k in SCHEMA_KINDS.iter() {
        s4.put(*k, SCHEMA_VER + 1, true);
    }
    s4.freeze();
    chk(&mut set, "C3989-16", s4.freeze_hash() != h1, "err");

    // C3989-17 schema 槽双射。
    let slots: Vec<usize> = SCHEMA_KINDS.iter().map(|k| k.slot()).collect();
    let mut uniq = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if slots[i] == slots[j] { uniq = false; }
        }
    }
    chk(&mut set, "C3989-17", uniq, "err");

    // C3989-18 冻结幂等。
    chk(&mut set, "C3989-18", s.freeze() == T9DCode::OK, "err");

    // C3989-19 文档组名互异。
    let mut uniq2 = true;
    for i in 0..8 {
        for j in (i + 1)..8 {
            if DOC_GROUPS[i] == DOC_GROUPS[j] { uniq2 = false; }
        }
    }
    chk(&mut set, "C3989-19", uniq2, "err");

    // C3989-20 错误码互异。
    let codes = [T9DCode::OK.0, T9DCode::CHAPTER_GAP.0, T9DCode::SCHEMA_DRIFT.0, T9DCode::POST_FROZEN.0];
    chk(&mut set, "C3989-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_pass_flow() {
        let mut d = DocMasterBook::new();
        assert_eq!(d.pass(), T9DCode::CHAPTER_GAP);
        for g in DOC_GROUPS.iter() {
            d.deliver(g);
        }
        d.guide.alt_text_spec = true;
        d.guide.contrast_spec = true;
        d.guide.final_ok = true;
        d.peer_reconciled = true;
        assert_eq!(d.pass(), T9DCode::OK);
    }

    #[test]
    fn schema_freeze_gate() {
        let mut s = SchemaBook::new();
        assert_eq!(s.freeze(), T9DCode::SCHEMA_DRIFT);
        for k in SCHEMA_KINDS.iter() {
            s.put(*k, SCHEMA_VER, true);
        }
        assert_eq!(s.freeze(), T9DCode::OK);
        assert!(!s.put(SchemaKind::Config, 9, true));
    }
}
