//! VE-F3971 · 文档资产汇总
//!
//! S 域文档资产汇总：八组文档/schema/示例三类归集。汇总规则：每资产挂组/
//! 挂条/挂版本三指针 + 读者画像标注。与 F3956 文档色标对拍。
//! 错误路径：版本漂移→对版修正；画像缺→补标；对拍不符→留痕修正。
//! 性能：归集 O(资产)；对拍 O(台账)；标注 O(1)。
//! 跨批对接：八组文档条目；F3966 上游；F3989 总审下游。
//! 无障碍与隐私：文档资产含无障碍使用指南专册（域本色）；示例不含真实
//! 用户数据（隐私复述）。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x3440）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3440;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9KCode(pub u16);

impl S9KCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const VER_DRIFT: Self = Self(SEG_BASE + 1);
    pub const PORTRAIT_GAP: Self = Self(SEG_BASE + 2);
    pub const RECON_MISMATCH: Self = Self(SEG_BASE + 3);
    pub const POINTER_GAP: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::VER_DRIFT => "版本漂移→对版修正",
            Self::PORTRAIT_GAP => "画像缺→补标",
            Self::RECON_MISMATCH => "对拍不符→留痕修正",
            Self::POINTER_GAP => "三指针缺→补",
            _ => "未知",
        }
    }
}

// ── 三类资产 ─────────────────────────────────────────────────────────

/// 资产三类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    /// 文档。
    Doc,
    /// schema。
    Schema,
    /// 示例。
    Example,
}

pub const DOC_KINDS: [DocKind; 3] = [DocKind::Doc, DocKind::Schema, DocKind::Example];

/// 读者画像标注。
pub const PORTRAITS: [&str; 5] = ["开发者", "测试者", "审计者", "视障读者", "普通读者"];

/// 文档资产行。
#[derive(Debug, Clone)]
pub struct DocRow {
    pub name: &'static str,
    pub kind: DocKind,
    /// 三指针：组 / 条 / 版本。
    pub group: u8,
    pub clause: u32,
    pub ver: u32,
    /// 读者画像（下标对齐 PORTRAITS；空 = 未标）。
    pub portraits: Vec<u8>,
    /// 无障碍使用指南专册标记。
    pub a11y_handbook: bool,
    /// 示例不含真实数据标记（示例类必须 true）。
    pub synthetic_only: bool,
}

impl DocRow {
    pub fn pointers_ok(&self) -> bool {
        self.group >= 1 && self.group <= 8 && self.clause > 0 && self.ver > 0
    }

    /// 画像缺断言。
    pub fn portrait_ok(&self) -> bool {
        !self.portraits.is_empty() && self.portraits.iter().all(|p| (*p as usize) < PORTRAITS.len())
    }

    /// 示例合成数据断言。
    pub fn example_ok(&self) -> bool {
        self.kind != DocKind::Example || self.synthetic_only
    }
}

/// 台账。
#[derive(Debug, Clone)]
pub struct DocLedger {
    pub rows: Vec<DocRow>,
}

impl DocLedger {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn admit(&mut self, r: DocRow) -> S9KCode {
        if !r.pointers_ok() {
            return S9KCode::POINTER_GAP;
        }
        if !r.portrait_ok() {
            return S9KCode::PORTRAIT_GAP;
        }
        if !r.example_ok() {
            return S9KCode::RECON_MISMATCH;
        }
        self.rows.push(r);
        S9KCode::OK
    }

    /// 三类覆盖。
    pub fn kind_gaps(&self) -> Vec<DocKind> {
        DOC_KINDS.iter().filter(|k| !self.rows.iter().any(|r| r.kind == **k)).map(|k| *k).collect()
    }

    /// 画像缺清单。
    pub fn portrait_gaps(&self) -> Vec<&'static str> {
        self.rows.iter().filter(|r| !r.portrait_ok()).map(|r| r.name).collect()
    }

    /// 专册在册断言。
    pub fn handbook_present(&self) -> bool {
        self.rows.iter().any(|r| r.a11y_handbook && r.kind == DocKind::Doc)
    }

    /// 版本对拍：外部版本表 vs 台账。
    pub fn ver_drift(&self, external: &[(&'static str, u32)]) -> Vec<&'static str> {
        external
            .iter()
            .filter(|(n, v)| match self.rows.iter().find(|r| r.name == *n) {
                Some(r) => r.ver != *v,
                None => false,
            })
            .map(|(n, _)| *n)
            .collect()
    }

    /// 对版修正。
    pub fn reversion(&mut self, name: &str, ver: u32) -> S9KCode {
        match self.rows.iter_mut().find(|r| r.name == name) {
            Some(r) => {
                r.ver = ver;
                S9KCode::OK
            }
            None => S9KCode::VER_DRIFT,
        }
    }
}

impl Default for DocLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 与 F3956 文档色标对拍 ────────────────────────────────────────────

/// 色标对：文档名 → 色标状态。
#[derive(Debug, Clone)]
pub struct ColorPair {
    pub name: &'static str,
    /// 色标挂账（true = 已挂色标）。
    pub marked: bool,
}

/// 对拍不符清单：台账在册但色标未挂。
pub fn color_mismatch(ledger: &DocLedger, marks: &[ColorPair]) -> Vec<&'static str> {
    ledger
        .rows
        .iter()
        .filter(|r| !marks.iter().any(|m| m.name == r.name && m.marked))
        .map(|r| r.name)
        .collect()
}

/// 留痕修正记录。
#[derive(Debug, Clone)]
pub struct DocTrace {
    pub name: &'static str,
    pub action: &'static str,
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves225_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves225_s09docasset");

    fn row(name: &'static str, kind: DocKind, group: u8, ver: u32) -> DocRow {
        DocRow { name, kind, group, clause: 6, ver, portraits: vec![0, 4], a11y_handbook: false, synthetic_only: kind == DocKind::Example }
    }

    // 01 三类清单守恒
    {
        chk(&mut set, "C3971-01", DOC_KINDS.len() == 3 && DocKind::Doc as usize == 0 && DocKind::Example as usize == 2, "err");
    }
    // 02 三类全覆盖
    {
        let mut l = DocLedger::new();
        for (i, k) in DOC_KINDS.iter().enumerate() {
            l.admit(row("资产", *k, (i + 1) as u8, 1));
        }
        chk(&mut set, "C3971-02", l.kind_gaps().is_empty(), "err");
    }
    // 03 三指针缺拒收
    {
        let mut l = DocLedger::new();
        let mut r = row("缺版", DocKind::Doc, 1, 0);
        r.ver = 0;
        chk(&mut set, "C3971-03", l.admit(r) == S9KCode::POINTER_GAP, "err");
    }
    // 04 画像缺拒收 + 补标闭环
    {
        let mut l = DocLedger::new();
        let mut r = row("无画像", DocKind::Doc, 2, 1);
        r.portraits.clear();
        chk(&mut set, "C3971-04", l.admit(r.clone()) == S9KCode::PORTRAIT_GAP && { r.portraits = vec![2]; l.admit(r) == S9KCode::OK }, "err");
    }
    // 05 画像下标越界拒收
    {
        let mut l = DocLedger::new();
        let mut r = row("越界画像", DocKind::Doc, 2, 1);
        r.portraits = vec![9];
        chk(&mut set, "C3971-05", !r.portrait_ok() && l.admit(r) == S9KCode::PORTRAIT_GAP, "err");
    }
    // 06 示例未标合成数据拒收
    {
        let mut l = DocLedger::new();
        let mut r = row("真实示例", DocKind::Example, 3, 1);
        r.synthetic_only = false;
        chk(&mut set, "C3971-06", !r.example_ok() && l.admit(r) == S9KCode::RECON_MISMATCH, "err");
    }
    // 07 专册在册断言
    {
        let mut l = DocLedger::new();
        let mut r = row("无障碍指南", DocKind::Doc, 8, 2);
        r.a11y_handbook = true;
        l.admit(r);
        chk(&mut set, "C3971-07", l.handbook_present(), "err");
    }
    // 08 专册缺失
    {
        let mut l = DocLedger::new();
        l.admit(row("普通册", DocKind::Doc, 1, 1));
        chk(&mut set, "C3971-08", !l.handbook_present(), "err");
    }
    // 09 版本漂移检出→对版修正
    {
        let mut l = DocLedger::new();
        l.admit(row("配置册", DocKind::Schema, 4, 3));
        let ext = vec![("配置册", 5u32)];
        chk(&mut set, "C3971-09", l.ver_drift(&ext) == vec!["配置册"] && l.reversion("配置册", 5) == S9KCode::OK && l.ver_drift(&ext).is_empty(), "err");
    }
    // 10 对版不存在条目拒收
    {
        let mut l = DocLedger::new();
        chk(&mut set, "C3971-10", l.reversion("幽灵", 1) == S9KCode::VER_DRIFT, "err");
    }
    // 11 色标对拍不符检出
    {
        let mut l = DocLedger::new();
        l.admit(row("甲册", DocKind::Doc, 1, 1));
        l.admit(row("乙册", DocKind::Doc, 2, 1));
        let marks = vec![ColorPair { name: "甲册", marked: true }];
        chk(&mut set, "C3971-11", color_mismatch(&l, &marks) == vec!["乙册"], "err");
    }
    // 12 留痕修正结构
    {
        let t = DocTrace { name: "乙册", action: "补挂色标" };
        chk(&mut set, "C3971-12", !t.action.is_empty() && t.name == "乙册", "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3971-13", S9KCode::POINTER_GAP.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9KCode::OK, S9KCode::VER_DRIFT, S9KCode::PORTRAIT_GAP, S9KCode::RECON_MISMATCH, S9KCode::POINTER_GAP];
        chk(&mut set, "C3971-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_docasset_all_green() {
        let s = run_ves225_checks();
        assert!(s.all_passed(), "ves225 red: {}", s.red_items_text());
    }

    #[test]
    fn test_portrait_boundary() {
        let mut r = DocRow { name: "x", kind: DocKind::Doc, group: 1, clause: 1, ver: 1, portraits: vec![4], a11y_handbook: false, synthetic_only: false };
        assert!(r.portrait_ok()); // 下标 4 = 最后画像
        r.portraits = vec![5];
        assert!(!r.portrait_ok());
    }

    #[test]
    fn test_color_recon_clean() {
        let mut l = DocLedger::new();
let _ =         l.admit(DocRow { name: "甲", kind: DocKind::Doc, group: 1, clause: 1, ver: 1, portraits: vec![0], a11y_handbook: false, synthetic_only: false });
        let marks = vec![ColorPair { name: "甲", marked: true }];
        assert!(color_mismatch(&l, &marks).is_empty());
    }
}
