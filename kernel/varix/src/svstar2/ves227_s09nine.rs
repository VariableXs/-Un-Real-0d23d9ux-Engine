//! VE-F3973 · 九维汇总表与风险登记
//!
//! S 域九维汇总表：API/测试/性能/安全/无障碍/文档/预算/扩展/观测九维各
//! 一页汇总。风险登记册：每风险挂维度/概率/影响/缓解四列。
//! 错误路径：风险未登记→扫描补登；缓解缺→补案；汇总与资产不符→对账
//! 修正。
//! 性能：汇总 O(维)；登记 O(风险)；对账 O(表)。
//! 跨批对接：六汇总上游；F3974 预评分下游；F3993 验收评分终版。
//! 无障碍与隐私：九维含无障碍维主场位（域本色）；风险册不含用户身份
//! 信息。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x34C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x34C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9MCode(pub u16);

impl S9MCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const UNREGISTERED: Self = Self(SEG_BASE + 1);
    pub const MITIGATION_GAP: Self = Self(SEG_BASE + 2);
    pub const RECON_MISMATCH: Self = Self(SEG_BASE + 3);
    pub const DIM_OVER: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::UNREGISTERED => "风险未登记→扫描补登",
            Self::MITIGATION_GAP => "缓解缺→补案",
            Self::RECON_MISMATCH => "汇总与资产不符→对账修正",
            Self::DIM_OVER => "维度越界→拒收",
            _ => "未知",
        }
    }
}

// ── 九维汇总表 ───────────────────────────────────────────────────────

/// 九维。
pub const NINE_DIMS: [&str; 9] = ["API", "测试", "性能", "安全", "无障碍", "文档", "预算", "扩展", "观测"];

/// 无障碍维主场位（九维中的下标）。
pub const A11Y_HOME: usize = 4;

/// 单维汇总页。
#[derive(Debug, Clone)]
pub struct DimPage {
    pub dim: usize,
    /// 页内容指纹（0 = 空页）。
    pub fp: u64,
}

/// 九维汇总表。
#[derive(Debug, Clone)]
pub struct NineTable {
    pub pages: Vec<DimPage>,
}

impl NineTable {
    pub fn new() -> Self {
        Self { pages: Vec::new() }
    }

    pub fn write(&mut self, dim: usize, fp: u64) -> S9MCode {
        if dim >= 9 {
            return S9MCode::DIM_OVER;
        }
        match self.pages.iter_mut().find(|p| p.dim == dim) {
            Some(p) => p.fp = fp,
            None => self.pages.push(DimPage { dim, fp }),
        }
        S9MCode::OK
    }

    /// 空页清单。
    pub fn empty_pages(&self) -> Vec<usize> {
        (0..9).filter(|d| match self.pages.iter().find(|p| p.dim == *d) {
            Some(p) => p.fp == 0,
            None => true,
        }).collect()
    }

    /// 主场位断言：无障碍维必须有实质页。
    pub fn home_set(&self) -> bool {
        self.pages.iter().any(|p| p.dim == A11Y_HOME && p.fp != 0)
    }

    pub fn complete(&self) -> bool {
        self.empty_pages().is_empty()
    }
}

impl Default for NineTable {
    fn default() -> Self {
        Self::new()
    }
}

// ── 风险登记册（四列）────────────────────────────────────────────────

/// 风险条目：维度/概率/影响/缓解四列。
#[derive(Debug, Clone)]
pub struct RiskRow {
    pub name: &'static str,
    /// 挂维度（0-8）。
    pub dim: usize,
    /// 概率（1-5）。
    pub prob: u8,
    /// 影响（1-5）。
    pub impact: u8,
    /// 缓解案（空 = 缺）。
    pub mitigation: &'static str,
}

impl RiskRow {
    /// 风险值 = 概率 × 影响。
    pub fn score(&self) -> u16 {
        self.prob as u16 * self.impact as u16
    }

    pub fn mitigation_ok(&self) -> bool {
        !self.mitigation.is_empty()
    }

    pub fn fields_ok(&self) -> bool {
        self.dim < 9 && self.prob >= 1 && self.prob <= 5 && self.impact >= 1 && self.impact <= 5
    }
}

/// 风险登记册。
#[derive(Debug, Clone)]
pub struct RiskBook {
    pub rows: Vec<RiskRow>,
}

impl RiskBook {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    /// 登记：字段越界拒收。
    pub fn register(&mut self, r: RiskRow) -> S9MCode {
        if !r.fields_ok() {
            return S9MCode::DIM_OVER;
        }
        self.rows.push(r);
        S9MCode::OK
    }

    /// 缓解缺清单。
    pub fn no_mitigation(&self) -> Vec<&'static str> {
        self.rows.iter().filter(|r| !r.mitigation_ok()).map(|r| r.name).collect()
    }

    /// 补案。
    pub fn add_mitigation(&mut self, name: &str, m: &'static str) -> S9MCode {
        match self.rows.iter_mut().find(|r| r.name == name) {
            Some(r) => {
                r.mitigation = m;
                S9MCode::OK
            }
            None => S9MCode::MITIGATION_GAP,
        }
    }

    /// 扫描补登：维 d 无任何风险时补登占位风险。
    pub fn backfill(&mut self, dim: usize) -> S9MCode {
        if dim >= 9 {
            return S9MCode::DIM_OVER;
        }
        if self.rows.iter().any(|r| r.dim == dim) {
            return S9MCode::OK;
        }
        self.rows.push(RiskRow { name: "维级占位", dim, prob: 1, impact: 1, mitigation: "例行巡检" });
        S9MCode::OK
    }

    /// 高危清单：score ≥ 12。
    pub fn high_risks(&self) -> Vec<&'static str> {
        self.rows.iter().filter(|r| r.score() >= 12).map(|r| r.name).collect()
    }
}

impl Default for RiskBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 汇总与资产对账 ───────────────────────────────────────────────────

/// 对账对：维 → 表指纹 vs 资产指纹。
#[derive(Debug, Clone)]
pub struct ReconCell {
    pub dim: usize,
    pub table_fp: u64,
    pub asset_fp: u64,
}

/// 对账不符清单。
pub fn recon_mismatch(cells: &[ReconCell]) -> Vec<usize> {
    cells.iter().filter(|c| c.table_fp != c.asset_fp).map(|c| c.dim).collect()
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves227_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves227_s09nine");

    // 01 九维清单守恒 + 主场位
    {
        chk(&mut set, "C3973-01", NINE_DIMS.len() == 9 && NINE_DIMS[A11Y_HOME] == "无障碍" && NINE_DIMS[8] == "观测", "err");
    }
    // 02 九页全写完备
    {
        let mut t = NineTable::new();
        for d in 0..9 {
            t.write(d, 100 + d as u64);
        }
        chk(&mut set, "C3973-02", t.complete() && t.empty_pages().is_empty() && t.home_set(), "err");
    }
    // 03 空页检出
    {
        let mut t = NineTable::new();
        t.write(0, 5);
        t.write(4, 7);
        chk(&mut set, "C3973-03", t.empty_pages().len() == 7 && !t.complete(), "err");
    }
    // 04 主场位缺席
    {
        let mut t = NineTable::new();
        for d in 0..9 {
            if d != 4 {
                t.write(d, 1);
            }
        }
        chk(&mut set, "C3973-04", !t.home_set(), "err");
    }
    // 05 维度越界拒收
    {
        let mut t = NineTable::new();
        chk(&mut set, "C3973-05", t.write(9, 1) == S9MCode::DIM_OVER, "err");
    }
    // 06 四列字段执法
    {
        let mut b = RiskBook::new();
        chk(&mut set, "C3973-06", b.register(RiskRow { name: "坏", dim: 9, prob: 3, impact: 3, mitigation: "m" }) == S9MCode::DIM_OVER
            && b.register(RiskRow { name: "坏", dim: 1, prob: 6, impact: 3, mitigation: "m" }) == S9MCode::DIM_OVER, "err");
    }
    // 07 缓解缺→补案实测
    {
        let mut b = RiskBook::new();
        b.register(RiskRow { name: "色标漂移", dim: 4, prob: 2, impact: 4, mitigation: "" });
        chk(&mut set, "C3973-07", b.no_mitigation() == vec!["色标漂移"] && b.add_mitigation("色标漂移", "季度矩阵刷新") == S9MCode::OK && b.no_mitigation().is_empty(), "err");
    }
    // 08 补案不存在项拒收
    {
        let mut b = RiskBook::new();
        chk(&mut set, "C3973-08", b.add_mitigation("幽灵", "x") == S9MCode::MITIGATION_GAP, "err");
    }
    // 09 扫描补登
    {
        let mut b = RiskBook::new();
        b.backfill(6);
        chk(&mut set, "C3973-09", b.rows.len() == 1 && b.rows[0].dim == 6 && { b.backfill(6); b.rows.len() == 1 }, "err");
    }
    // 10 风险值计算 + 高危线
    {
        let mut b = RiskBook::new();
        b.register(RiskRow { name: "高危", dim: 3, prob: 4, impact: 4, mitigation: "m" });
        b.register(RiskRow { name: "低危", dim: 1, prob: 1, impact: 2, mitigation: "m" });
        chk(&mut set, "C3973-10", b.high_risks() == vec!["高危"] && b.rows[1].score() == 2, "err");
    }
    // 11 汇总与资产对账不符检出
    {
        let cells = vec![
            ReconCell { dim: 0, table_fp: 10, asset_fp: 10 },
            ReconCell { dim: 2, table_fp: 11, asset_fp: 99 },
        ];
        chk(&mut set, "C3973-11", recon_mismatch(&cells) == vec![2], "err");
    }
    // 12 对账一致
    {
        let cells = vec![ReconCell { dim: 5, table_fp: 7, asset_fp: 7 }];
        chk(&mut set, "C3973-12", recon_mismatch(&cells).is_empty(), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3973-13", S9MCode::DIM_OVER.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9MCode::OK, S9MCode::UNREGISTERED, S9MCode::MITIGATION_GAP, S9MCode::RECON_MISMATCH, S9MCode::DIM_OVER];
        chk(&mut set, "C3973-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_nine_all_green() {
        let s = run_ves227_checks();
        assert!(s.all_passed(), "ves227 red: {}", s.red_items_text());
    }

    #[test]
    fn test_risk_score_matrix() {
        let mut b = RiskBook::new();
        b.register(RiskRow { name: "a", dim: 0, prob: 5, impact: 5, mitigation: "m" }).ok();
        b.register(RiskRow { name: "b", dim: 8, prob: 2, impact: 3, mitigation: "" }).ok();
        assert_eq!(b.rows[0].score(), 25);
        assert_eq!(b.no_mitigation(), vec!["b"]);
        b.add_mitigation("b", "m2").ok();
        assert!(b.no_mitigation().is_empty());
    }

    #[test]
    fn test_page_upsert() {
        let mut t = NineTable::new();
        t.write(2, 1).ok();
        t.write(2, 2).ok();
        assert_eq!(t.pages.len(), 1);
        assert_eq!(t.pages[0].fp, 2);
    }
}
