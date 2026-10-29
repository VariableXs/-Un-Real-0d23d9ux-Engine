//! VE-F3972 · 缺失与薄弱项补齐
//!
//! 自查缺口与薄弱项补齐：六维资产汇总（测试/性能/安全/无障碍/文档/API）
//! 交叉扫描缺口清单。补齐三级分级：P0 阻断收口必修 / P1 终审前必修 /
//! P2 登记缓办。补齐回执制（回执缺→催办）。
//! 错误路径：P0 未消→阻断收口（硬门复述）；补齐失败→复议或降级声明；
//! 回执缺→催办。
//! 性能：扫描 O(六维×资产)；补齐 O(缺项)；催办 O(1)。
//! 跨批对接：F3966-F3971 六汇总上游；F3976 预备报告前置。
//! 无障碍与隐私：缺口清单含无障碍专项列（双维+心流）；录证补齐走脱敏
//! 通道。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x3480）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3480;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9LCode(pub u16);

impl S9LCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const P0_OPEN: Self = Self(SEG_BASE + 1);
    pub const FIX_FAILED: Self = Self(SEG_BASE + 2);
    pub const RECEIPT_MISSING: Self = Self(SEG_BASE + 3);
    pub const DIM_OVER: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::P0_OPEN => "P0 未消→阻断收口",
            Self::FIX_FAILED => "补齐失败→复议或降级声明",
            Self::RECEIPT_MISSING => "回执缺→催办",
            Self::DIM_OVER => "维度越界→拒收",
            _ => "未知",
        }
    }
}

// ── 六维交叉扫描 ─────────────────────────────────────────────────────

/// 六维。
pub const SIX_DIMS: [&str; 6] = ["测试", "性能", "安全", "无障碍", "文档", "API"];

/// 缺口条目。
#[derive(Debug, Clone)]
pub struct GapRow {
    pub name: &'static str,
    /// 所属维（下标对齐 SIX_DIMS）。
    pub dim: usize,
    /// 分级：0=P0 1=P1 2=P2。
    pub level: u8,
    /// 无障碍专项列标记（双维+心流缺口走专项）。
    pub a11y_special: bool,
}

/// 扫描器：六维资产登记数交叉扫描（登记为 0 = 缺口）。
#[derive(Debug, Clone)]
pub struct CrossScanner {
    /// 每维资产登记数。
    pub counts: [u32; 6],
}

impl CrossScanner {
    pub fn new() -> Self {
        Self { counts: [0; 6] }
    }

    pub fn set(&mut self, dim: usize, n: u32) -> S9LCode {
        if dim >= 6 {
            return S9LCode::DIM_OVER;
        }
        self.counts[dim] = n;
        S9LCode::OK
    }

    /// 扫描缺口：登记为 0 的维。
    pub fn gaps(&self) -> Vec<usize> {
        (0..6).filter(|d| self.counts[*d] == 0).collect()
    }

    /// 无障碍专项列扫描：无障碍维或其专项缺口。
    pub fn a11y_gap(&self) -> bool {
        self.counts[3] == 0
    }
}

impl Default for CrossScanner {
    fn default() -> Self {
        Self::new()
    }
}

// ── 三级分级补齐 ─────────────────────────────────────────────────────

/// 补齐工单。
#[derive(Debug, Clone)]
pub struct FixTicket {
    pub gap: &'static str,
    pub level: u8,
    /// 已补齐。
    pub fixed: bool,
    /// 回执（None = 缺）。
    pub receipt: Option<&'static str>,
    /// 补齐失败标记。
    pub failed: bool,
}

/// 补齐台账：三级执法。
#[derive(Debug, Clone)]
pub struct FixBook {
    pub tickets: Vec<FixTicket>,
}

impl FixBook {
    pub fn new() -> Self {
        Self { tickets: Vec::new() }
    }

    pub fn raise(&mut self, t: FixTicket) -> S9LCode {
        if t.level > 2 {
            return S9LCode::DIM_OVER;
        }
        self.tickets.push(t);
        S9LCode::OK
    }

    /// P0 未消检出（硬门）。
    pub fn p0_open(&self) -> Vec<&'static str> {
        self.tickets.iter().filter(|t| t.level == 0 && !t.fixed).map(|t| t.gap).collect()
    }

    /// 阻断收口判定。
    pub fn blocked(&self) -> bool {
        !self.p0_open().is_empty()
    }

    /// 回执缺清单（催办）。
    pub fn no_receipt(&self) -> Vec<&'static str> {
        self.tickets.iter().filter(|t| t.fixed && t.receipt.is_none()).map(|t| t.gap).collect()
    }

    /// 催办：补回执。
    pub fn ack(&mut self, gap: &str, receipt: &'static str) -> S9LCode {
        match self.tickets.iter_mut().find(|t| t.gap == gap) {
            Some(t) if t.fixed => {
                t.receipt = Some(receipt);
                S9LCode::OK
            }
            Some(_) => S9LCode::RECEIPT_MISSING,
            None => S9LCode::RECEIPT_MISSING,
        }
    }

    /// 补齐失败→降级声明（P0 降 P1 须声明）。
    pub fn demote(&mut self, gap: &str, statement: &'static str) -> S9LCode {
        if statement.is_empty() {
            return S9LCode::FIX_FAILED;
        }
        match self.tickets.iter_mut().find(|t| t.gap == gap) {
            Some(t) if t.level == 0 => {
                t.level = 1;
                S9LCode::OK
            }
            Some(_) => S9LCode::OK,
            None => S9LCode::FIX_FAILED,
        }
    }
}

impl Default for FixBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 补齐回执制 ───────────────────────────────────────────────────────

/// 回执结构。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub gap: &'static str,
    pub fixer: &'static str,
    /// 回执校验（证据指纹非零）。
    pub fp: u64,
}

impl Receipt {
    pub fn valid(&self) -> bool {
        !self.fixer.is_empty() && self.fp != 0
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves226_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves226_s09fix");

    // 01 六维清单守恒
    {
        chk(&mut set, "C3972-01", SIX_DIMS.len() == 6 && SIX_DIMS[3] == "无障碍" && SIX_DIMS[5] == "API", "err");
    }
    // 02 交叉扫描缺口检出
    {
        let mut s = CrossScanner::new();
        s.set(0, 10);
        s.set(3, 8);
        chk(&mut set, "C3972-02", s.gaps().len() == 4 && !s.gaps().contains(&0) && !s.gaps().contains(&3), "err");
    }
    // 03 无障碍专项列扫描
    {
        let mut s = CrossScanner::new();
        s.set(3, 0);
        chk(&mut set, "C3972-03", s.a11y_gap() && { s.set(3, 5); !s.a11y_gap() }, "err");
    }
    // 04 维度越界拒收
    {
        let mut s = CrossScanner::new();
        chk(&mut set, "C3972-04", s.set(6, 1) == S9LCode::DIM_OVER, "err");
    }
    // 05 三级分级执法
    {
        let mut b = FixBook::new();
        chk(&mut set, "C3972-05", b.raise(FixTicket { gap: "g", level: 3, fixed: false, receipt: None, failed: false }) == S9LCode::DIM_OVER, "err");
    }
    // 06 P0 未消→阻断收口实测
    {
        let mut b = FixBook::new();
        b.raise(FixTicket { gap: "快照缺", level: 0, fixed: false, receipt: None, failed: false });
        chk(&mut set, "C3972-06", b.blocked() && b.p0_open() == vec!["快照缺"], "err");
    }
    // 07 P0 消解后解锁
    {
        let mut b = FixBook::new();
        b.raise(FixTicket { gap: "快照缺", level: 0, fixed: true, receipt: Some("回执甲"), failed: false });
        chk(&mut set, "C3972-07", !b.blocked() && b.p0_open().is_empty(), "err");
    }
    // 08 回执缺→催办实测
    {
        let mut b = FixBook::new();
        b.raise(FixTicket { gap: "语料缺", level: 1, fixed: true, receipt: None, failed: false });
        chk(&mut set, "C3972-08", b.no_receipt() == vec!["语料缺"] && b.ack("语料缺", "回执乙") == S9LCode::OK && b.no_receipt().is_empty(), "err");
    }
    // 09 未补齐不可补回执
    {
        let mut b = FixBook::new();
        b.raise(FixTicket { gap: "未修", level: 2, fixed: false, receipt: None, failed: false });
        chk(&mut set, "C3972-09", b.ack("未修", "r") == S9LCode::RECEIPT_MISSING, "err");
    }
    // 10 补齐失败→降级声明实测
    {
        let mut b = FixBook::new();
        b.raise(FixTicket { gap: "仿真缺", level: 0, fixed: false, receipt: None, failed: true });
        chk(&mut set, "C3972-10", b.blocked() && b.demote("仿真缺", "硬件缺借调中，降级声明") == S9LCode::OK && !b.blocked() && b.tickets[0].level == 1, "err");
    }
    // 11 降级声明空拒收
    {
        let mut b = FixBook::new();
        b.raise(FixTicket { gap: "x", level: 0, fixed: false, receipt: None, failed: false });
        chk(&mut set, "C3972-11", b.demote("x", "") == S9LCode::FIX_FAILED && b.tickets[0].level == 0, "err");
    }
    // 12 回执结构校验
    {
        let r1 = Receipt { gap: "g", fixer: "甲", fp: 42 };
        let r2 = Receipt { gap: "g", fixer: "", fp: 0 };
        chk(&mut set, "C3972-12", r1.valid() && !r2.valid(), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3972-13", S9LCode::DIM_OVER.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9LCode::OK, S9LCode::P0_OPEN, S9LCode::FIX_FAILED, S9LCode::RECEIPT_MISSING, S9LCode::DIM_OVER];
        chk(&mut set, "C3972-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_fix_all_green() {
        let s = run_ves226_checks();
        assert!(s.all_passed(), "ves226 red: {}", s.red_items_text());
    }

    #[test]
    fn test_p0_hard_gate_cycle() {
        let mut b = FixBook::new();
let _ =         b.raise(FixTicket { gap: "a", level: 0, fixed: false, receipt: None, failed: false });
        assert!(b.blocked());
        b.tickets[0].fixed = true;
let _ =         b.ack("a", "r");
        assert!(!b.blocked());
        assert!(b.no_receipt().is_empty());
    }

    #[test]
    fn test_scan_full_six() {
        let mut s = CrossScanner::new();
        for d in 0..6 {
let _ =             s.set(d, 1);
        }
        assert!(s.gaps().is_empty() && !s.a11y_gap());
    }
}
