//! VE-F3966 · S 域 API 面终版清点
//!
//! S 域 API 面清点：八组对外接口逐组清点 + 与 F3816 总架构声明 API 面
//! 终版对账。清点规则：每 API 挂组/挂条/挂色标三指针 + 版本号 + 稳定性
//! 标注。冗余与缺口双检。
//! 错误路径：声明与清点不符→修正留痕；缺口→立案补齐；冗余→归并评审。
//! 性能：清点 O(API)；对账 O(面)；标注 O(1)。
//! 跨批对接：F3816 架构单源；F3981 接口总账终审上游；移交包。
//! 无障碍与隐私：API 面含无障碍语义接口专章（域本色）；无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x3280）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3280;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9FCode(pub u16);

impl S9FCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const MISMATCH: Self = Self(SEG_BASE + 1);
    pub const SHORTAGE: Self = Self(SEG_BASE + 2);
    pub const REDUNDANT: Self = Self(SEG_BASE + 3);
    pub const ANNOT_GAP: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::MISMATCH => "声明与清点不符→修正留痕",
            Self::SHORTAGE => "缺口→立案补齐",
            Self::REDUNDANT => "冗余→归并评审",
            Self::ANNOT_GAP => "标注缺→补标",
            _ => "未知",
        }
    }
}

// ── API 清点条目（三指针 + 版本 + 稳定性）────────────────────────────

/// 稳定性标注。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stability {
    /// 冻结。
    Frozen,
    /// 稳定。
    Stable,
    /// 实验。
    Experimental,
}

/// 清点条目。
#[derive(Debug, Clone)]
pub struct ApiRow {
    pub name: &'static str,
    /// 指针一：挂组（S01-S08）。
    pub group: u8,
    /// 指针二：挂条（条目号）。
    pub clause: u32,
    /// 指针三：挂色标（证据号，0 = 未挂）。
    pub color_ref: u32,
    /// 版本号。
    pub ver: u32,
    pub stability: Stability,
}

impl ApiRow {
    /// 三指针齐备断言。
    pub fn pointers_ok(&self) -> bool {
        self.group >= 1 && self.group <= 8 && self.clause > 0 && self.color_ref > 0
    }
}

/// 清点册。
#[derive(Debug, Clone)]
pub struct ApiBook {
    pub rows: Vec<ApiRow>,
}

impl ApiBook {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn add(&mut self, r: ApiRow) {
        self.rows.push(r);
    }

    /// 标注缺清单（三指针不全）。
    pub fn annot_gaps(&self) -> Vec<&'static str> {
        self.rows.iter().filter(|r| !r.pointers_ok()).map(|r| r.name).collect()
    }

    /// 声明对账：声明名单 vs 清点实态（缺口 + 冗余双检）。
    pub fn double_check(&self, declared: &[&'static str]) -> (Vec<&'static str>, Vec<&'static str>) {
        let mut shortage = Vec::new();
        let mut redundant = Vec::new();
        for d in declared.iter() {
            if !self.rows.iter().any(|r| r.name == *d) {
                shortage.push(*d);
            }
        }
        for r in self.rows.iter() {
            if !declared.contains(&r.name) {
                redundant.push(r.name);
            }
        }
        (shortage, redundant)
    }

    /// 版本对账：声明版本 vs 清点版本不符（修正留痕锚）。
    pub fn ver_mismatch(&self, declared: &[(&'static str, u32)]) -> Vec<&'static str> {
        declared
            .iter()
            .filter(|(n, v)| match self.rows.iter().find(|r| r.name == *n) {
                Some(r) => r.ver != *v,
                None => false,
            })
            .map(|(n, _)| *n)
            .collect()
    }

    /// 专章断言：无障碍语义接口章（组 = 无障碍语义域号 9）齐备。
    pub fn semantic_chapter(&self) -> bool {
        self.rows.iter().any(|r| r.group == 9 && r.pointers_ok())
    }
}

// ── 修正留痕 ─────────────────────────────────────────────────────────

/// 留痕记录：API 名 → 修正前值 → 修正后值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixTrace {
    pub name: &'static str,
    pub before: u32,
    pub after: u32,
}

/// 留痕账。
#[derive(Debug, Clone)]
pub struct TraceBook {
    pub items: Vec<FixTrace>,
}

impl TraceBook {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn record(&mut self, name: &'static str, before: u32, after: u32) {
        self.items.push(FixTrace { name, before, after });
    }

    pub fn has(&self, name: &str) -> bool {
        self.items.iter().any(|t| t.name == name)
    }
}

impl Default for TraceBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 归并评审（冗余处置）──────────────────────────────────────────────

/// 归并：冗余 API 并入主条目，从清点册移除并留痕。
pub fn merge_redundant(book: &mut ApiBook, trace: &mut TraceBook, name: &str, into: &'static str) -> S9FCode {
    match book.rows.iter().position(|r| r.name == name) {
        Some(i) => {
            let v = book.rows[i].ver;
            book.rows.remove(i);
            trace.record(into, v, v);
            S9FCode::OK
        }
        None => S9FCode::REDUNDANT,
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves220_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves220_s09api");

    fn row(name: &'static str, group: u8, clause: u32, color: u32, ver: u32) -> ApiRow {
        ApiRow { name, group, clause, color_ref: color, ver, stability: Stability::Frozen }
    }

    // 01 三指针齐备断言
    {
        let r = row("a11y_focus_next", 3, 101, 7, 2);
        chk(&mut set, "C3966-01", r.pointers_ok(), "err");
    }
    // 02 标注缺检出（逐指针）
    {
        let r1 = row("a", 0, 1, 1, 1);
        let r2 = row("b", 1, 0, 1, 1);
        let r3 = row("c", 1, 1, 0, 1);
        let mut b = ApiBook::new();
        b.add(r1);
        b.add(r2);
        b.add(r3);
        chk(&mut set, "C3966-02", b.annot_gaps().len() == 3, "err");
    }
    // 03 缺口+冗余双检
    {
        let mut b = ApiBook::new();
        b.add(row("已清点", 2, 10, 5, 1));
        b.add(row("多出来", 2, 11, 6, 1));
        let declared = vec!["已清点", "漏清点"];
        let (short, red) = b.double_check(&declared);
        chk(&mut set, "C3966-03", short == vec!["漏清点"] && red == vec!["多出来"], "err");
    }
    // 04 双检干净态
    {
        let mut b = ApiBook::new();
        b.add(row("甲", 1, 1, 1, 3));
        let declared = vec!["甲"];
        let (short, red) = b.double_check(&declared);
        chk(&mut set, "C3966-04", short.is_empty() && red.is_empty(), "err");
    }
    // 05 版本不符→修正留痕实测
    {
        let mut b = ApiBook::new();
        b.add(row("焦点环", 3, 20, 9, 2));
        let declared = vec![("焦点环", 3u32)];
        let mism = b.ver_mismatch(&declared);
        let mut t = TraceBook::new();
        t.record("焦点环", 2, 3);
        chk(&mut set, "C3966-05", mism == vec!["焦点环"] && t.has("焦点环") && t.items[0] == FixTrace { name: "焦点环", before: 2, after: 3 }, "err");
    }
    // 06 版本一致零留痕需求
    {
        let mut b = ApiBook::new();
        b.add(row("同版", 4, 30, 2, 5));
        let declared = vec![("同版", 5u32)];
        chk(&mut set, "C3966-06", b.ver_mismatch(&declared).is_empty(), "err");
    }
    // 07 语义专章断言
    {
        let mut b = ApiBook::new();
        b.add(row("语义读屏序", 9, 40, 11, 1));
        chk(&mut set, "C3966-07", b.semantic_chapter(), "err");
    }
    // 08 语义专章缺失
    {
        let mut b = ApiBook::new();
        b.add(row("普通接口", 5, 41, 12, 1));
        chk(&mut set, "C3966-08", !b.semantic_chapter(), "err");
    }
    // 09 归并评审：冗余移除留痕
    {
        let mut b = ApiBook::new();
        b.add(row("旧名", 6, 50, 3, 4));
        b.add(row("新名", 6, 50, 3, 4));
        let mut t = TraceBook::new();
        chk(&mut set, "C3966-09", merge_redundant(&mut b, &mut t, "旧名", "新名") == S9FCode::OK && b.rows.len() == 1 && t.has("新名"), "err");
    }
    // 10 归并不存在条目拒收
    {
        let mut b = ApiBook::new();
        let mut t = TraceBook::new();
        chk(&mut set, "C3966-10", merge_redundant(&mut b, &mut t, "幽灵", "X") == S9FCode::REDUNDANT, "err");
    }
    // 11 稳定性标注守恒
    {
        chk(&mut set, "C3966-11", Stability::Frozen as usize == 0 && Stability::Experimental as usize == 2, "err");
    }
    // 12 组号域守恒（1-8 + 语义专章 9）
    {
        let r_ok = row("x", 8, 1, 1, 1);
        let r_over = row("y", 10, 1, 1, 1);
        chk(&mut set, "C3966-12", r_ok.pointers_ok() && !r_over.pointers_ok(), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3966-13", S9FCode::ANNOT_GAP.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9FCode::OK, S9FCode::MISMATCH, S9FCode::SHORTAGE, S9FCode::REDUNDANT, S9FCode::ANNOT_GAP];
        chk(&mut set, "C3966-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_api_all_green() {
        let s = run_ves220_checks();
        assert!(s.all_passed(), "ves220 red: {}", s.red_items_text());
    }

    #[test]
    fn test_double_check_symmetry() {
        let mut b = ApiBook::new();
        b.add(ApiRow { name: "a", group: 1, clause: 1, color_ref: 1, ver: 1, stability: Stability::Stable });
        let (s, r) = b.double_check(&["a"]);
        assert!(s.is_empty() && r.is_empty());
        let (s2, r2) = b.double_check(&["a", "b"]);
        assert_eq!(s2, vec!["b"]);
        assert!(r2.is_empty());
    }

    #[test]
    fn test_trace_immutable_records() {
        let mut t = TraceBook::new();
        t.record("x", 1, 2);
        t.record("y", 2, 3);
        assert_eq!(t.items.len(), 2);
        assert!(t.has("x") && t.has("y") && !t.has("z"));
    }
}
