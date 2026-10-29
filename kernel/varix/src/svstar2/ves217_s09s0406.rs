//! VE-F3963 · S04-S06 组自查核验
//!
//! 三组逐项核验：S04 动效无障碍减弱动效偏好全链复测（偏好探针 + 降级
//! 路径）；S05 认知无障碍可读性指标复测（词汇分级 + 句长阈值 + 进度外化）；
//! S06 输入无障碍键盘全通路复测（焦点环 + 快捷键冲突表 + 替代输入）——
//! 证据引用制执行。
//! 错误路径：指标越阈→立案+回改；偏好探测失败→保守降级；冲突未消→
//! 回补消解。
//! 性能：复测 O(60 项)；指标 O(文本)；通路 O(键位)。
//! 跨批对接：S04-S06 档案单源；F3965 上游；F3970 资产归集。
//! 无障碍与隐私：三组即无障碍本体（域本色）；偏好不回传远端（隐私复述）。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x2FC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x2FC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9CCode(pub u16);

impl S9CCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const METRIC_OVER: Self = Self(SEG_BASE + 1);
    pub const PROBE_FAIL: Self = Self(SEG_BASE + 2);
    pub const CONFLICT_OPEN: Self = Self(SEG_BASE + 3);
    pub const PATH_GAP: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::METRIC_OVER => "指标越阈→立案+回改",
            Self::PROBE_FAIL => "偏好探测失败→保守降级",
            Self::CONFLICT_OPEN => "冲突未消→回补消解",
            Self::PATH_GAP => "通路缺→补",
            _ => "未知",
        }
    }
}

// ── S04 偏好全链复测 ─────────────────────────────────────────────────

/// 减弱动效偏好探针状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeState {
    /// 明确减弱。
    Reduce,
    /// 明确不减弱。
    NoReduce,
    /// 探测失败（未知）。
    Unknown,
}

/// S04 全链复测器：探针 → 降级路径逐段验证。
#[derive(Debug, Clone)]
pub struct S04Chain {
    pub probe: ProbeState,
    /// 降级路径段（探针后各环节是否闭合）。
    pub degrade_links: [bool; 3],
}

pub const DEGRADE_LINK_NAMES: [&str; 3] = ["偏好读取", "动效降级", "状态外化"];

impl S04Chain {
    pub fn new(probe: ProbeState) -> Self {
        Self { probe, degrade_links: [false; 3] }
    }

    pub fn set_link(&mut self, i: usize, ok: bool) -> S9CCode {
        if i >= 3 {
            return S9CCode::PATH_GAP;
        }
        self.degrade_links[i] = ok;
        S9CCode::OK
    }

    /// 保守降级判定：探测失败时必须走保守路径（当作减弱处理）。
    pub fn conservative_ok(&self) -> bool {
        match self.probe {
            ProbeState::Reduce => self.degrade_links.iter().all(|l| *l),
            ProbeState::NoReduce => true,
            ProbeState::Unknown => self.degrade_links.iter().all(|l| *l),
        }
    }

    /// 全链闭合断言。
    pub fn chain_closed(&self) -> bool {
        self.degrade_links.iter().all(|l| *l)
    }
}

// ── S05 可读性指标复测 ───────────────────────────────────────────────

/// 可读性阈值：句长上限（字）与词汇分级上限（级）。
pub const SENTENCE_LEN_MAX: u32 = 40;
pub const VOCAB_GRADE_MAX: u32 = 6;

/// 单条文本指标。
#[derive(Debug, Clone)]
pub struct ReadMetric {
    pub text_id: &'static str,
    /// 最长句（字）。
    pub max_sentence: u32,
    /// 词汇分级（1-10）。
    pub vocab_grade: u32,
    /// 进度是否外化。
    pub progress_external: bool,
}

impl ReadMetric {
    /// 指标达标断言。
    pub fn pass(&self) -> bool {
        self.max_sentence <= SENTENCE_LEN_MAX && self.vocab_grade <= VOCAB_GRADE_MAX && self.progress_external
    }

    /// 越阈项清单。
    pub fn overs(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.max_sentence > SENTENCE_LEN_MAX {
            v.push("句长");
        }
        if self.vocab_grade > VOCAB_GRADE_MAX {
            v.push("词汇级");
        }
        if !self.progress_external {
            v.push("进度外化");
        }
        v
    }
}

/// S05 复测账。
#[derive(Debug, Clone)]
pub struct S05Book {
    pub items: Vec<ReadMetric>,
}

impl S05Book {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add(&mut self, m: ReadMetric) {
        self.items.push(m);
    }

    /// 立案名单。
    pub fn over_items(&self) -> Vec<&'static str> {
        self.items.iter().filter(|m| !m.pass()).map(|m| m.text_id).collect()
    }

    pub fn all_pass(&self) -> bool {
        !self.items.is_empty() && self.over_items().is_empty()
    }
}

impl Default for S05Book {
    fn default() -> Self {
        Self::new()
    }
}

// ── S06 键盘全通路复测 ───────────────────────────────────────────────

/// 快捷键冲突表：键位 → 占用者。
#[derive(Debug, Clone)]
pub struct HotkeyRow {
    pub key: &'static str,
    pub owner: &'static str,
    /// 冲突已消解。
    pub resolved: bool,
}

/// S06 复测器。
#[derive(Debug, Clone)]
pub struct S06Path {
    /// 焦点环闭合（首尾相接）。
    pub focus_ring_closed: bool,
    /// 替代输入可用。
    pub alt_input_ok: bool,
    /// 快捷键冲突表。
    pub hotkeys: Vec<HotkeyRow>,
}

impl S06Path {
    pub fn new() -> Self {
        Self { focus_ring_closed: false, alt_input_ok: false, hotkeys: Vec::new() }
    }

    pub fn add_hotkey(&mut self, key: &'static str, owner: &'static str, resolved: bool) {
        self.hotkeys.push(HotkeyRow { key, owner, resolved });
    }

    /// 冲突未消清单（回补消解）。
    pub fn open_conflicts(&self) -> Vec<&'static str> {
        self.hotkeys.iter().filter(|h| !h.resolved).map(|h| h.key).collect()
    }

    /// 回补消解。
    pub fn resolve(&mut self, key: &str) -> S9CCode {
        match self.hotkeys.iter_mut().find(|h| h.key == key) {
            Some(h) => {
                h.resolved = true;
                S9CCode::OK
            }
            None => S9CCode::CONFLICT_OPEN,
        }
    }

    /// 全通路断言。
    pub fn full_path(&self) -> bool {
        self.focus_ring_closed && self.alt_input_ok && self.open_conflicts().is_empty()
    }
}

impl Default for S06Path {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves217_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves217_s09s0406");

    // 01 降级链名称守恒
    {
        chk(&mut set, "C3963-01", DEGRADE_LINK_NAMES.len() == 3 && DEGRADE_LINK_NAMES[0] == "偏好读取" && DEGRADE_LINK_NAMES[2] == "状态外化", "err");
    }
    // 02 S04 全链闭合（明确减弱）
    {
        let mut c = S04Chain::new(ProbeState::Reduce);
        for i in 0..3 {
            c.set_link(i, true);
        }
        chk(&mut set, "C3963-02", c.chain_closed() && c.conservative_ok(), "err");
    }
    // 03 探测失败→保守降级实测
    {
        let mut c = S04Chain::new(ProbeState::Unknown);
        for i in 0..3 {
            c.set_link(i, true);
        }
        chk(&mut set, "C3963-03", c.conservative_ok() && c.chain_closed(), "err");
    }
    // 04 探测失败且链断裂 → 保守失败检出
    {
        let mut c = S04Chain::new(ProbeState::Unknown);
        c.set_link(0, true);
        chk(&mut set, "C3963-04", !c.conservative_ok() && !c.chain_closed(), "err");
    }
    // 05 降级链段越界拒收
    {
        let mut c = S04Chain::new(ProbeState::NoReduce);
        chk(&mut set, "C3963-05", c.set_link(3, true) == S9CCode::PATH_GAP, "err");
    }
    // 06 S05 指标达标
    {
        let m = ReadMetric { text_id: "教程甲", max_sentence: 30, vocab_grade: 4, progress_external: true };
        chk(&mut set, "C3963-06", m.pass() && m.overs().is_empty(), "err");
    }
    // 07 指标越阈→立案实测（逐项）
    {
        let m1 = ReadMetric { text_id: "长句", max_sentence: 41, vocab_grade: 3, progress_external: true };
        let m2 = ReadMetric { text_id: "高阶词", max_sentence: 10, vocab_grade: 7, progress_external: true };
        let m3 = ReadMetric { text_id: "无外化", max_sentence: 10, vocab_grade: 3, progress_external: false };
        chk(&mut set, "C3963-07", m1.overs() == vec!["句长"] && m2.overs() == vec!["词汇级"] && m3.overs() == vec!["进度外化"], "err");
    }
    // 08 S05 账本立案名单
    {
        let mut b = S05Book::new();
        b.add(ReadMetric { text_id: "好", max_sentence: 20, vocab_grade: 2, progress_external: true });
        b.add(ReadMetric { text_id: "坏", max_sentence: 99, vocab_grade: 2, progress_external: true });
        chk(&mut set, "C3963-08", b.over_items() == vec!["坏"] && !b.all_pass(), "err");
    }
    // 09 S06 全通路闭合
    {
        let mut p = S06Path::new();
        p.focus_ring_closed = true;
        p.alt_input_ok = true;
        p.add_hotkey("Ctrl+K", "命令板", true);
        chk(&mut set, "C3963-09", p.full_path() && p.open_conflicts().is_empty(), "err");
    }
    // 10 冲突未消→回补消解实测
    {
        let mut p = S06Path::new();
        p.focus_ring_closed = true;
        p.alt_input_ok = true;
        p.add_hotkey("Ctrl+S", "保存", false);
        chk(&mut set, "C3963-10", !p.full_path() && p.open_conflicts() == vec!["Ctrl+S"] && p.resolve("Ctrl+S") == S9CCode::OK && p.full_path(), "err");
    }
    // 11 消解不存在冲突拒收
    {
        let mut p = S06Path::new();
        chk(&mut set, "C3963-11", p.resolve("幽灵键") == S9CCode::CONFLICT_OPEN, "err");
    }
    // 12 焦点环断裂阻断全通路
    {
        let mut p = S06Path::new();
        p.alt_input_ok = true;
        p.add_hotkey("Tab", "焦点", true);
        chk(&mut set, "C3963-12", !p.full_path(), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3963-13", S9CCode::PATH_GAP.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9CCode::OK, S9CCode::METRIC_OVER, S9CCode::PROBE_FAIL, S9CCode::CONFLICT_OPEN, S9CCode::PATH_GAP];
        chk(&mut set, "C3963-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_s0406_all_green() {
        let s = run_ves217_checks();
        assert!(s.all_passed(), "ves217 red: {}", s.red_items_text());
    }

    #[test]
    fn test_metric_boundary() {
        let m = ReadMetric { text_id: "边界", max_sentence: SENTENCE_LEN_MAX, vocab_grade: VOCAB_GRADE_MAX, progress_external: true };
        assert!(m.pass());
        let m2 = ReadMetric { text_id: "越界", max_sentence: SENTENCE_LEN_MAX + 1, vocab_grade: VOCAB_GRADE_MAX, progress_external: true };
        assert!(!m2.pass());
    }

    #[test]
    fn test_hotkey_conflict_cycle() {
        let mut p = S06Path::new();
        p.focus_ring_closed = true;
        p.alt_input_ok = true;
        p.add_hotkey("A", "x", false);
        p.add_hotkey("B", "y", false);
let _ =         p.resolve("A");
        assert_eq!(p.open_conflicts(), vec!["B"]);
let _ =         p.resolve("B");
        assert!(p.full_path());
    }
}
