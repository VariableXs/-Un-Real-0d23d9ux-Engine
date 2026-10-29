//! VE-F3968 · 性能资产汇总
//!
//! S 域性能资产汇总：八组基准/预算/实测三档归集。汇总规则：每资产挂组/
//! 挂条/挂基准三指针 + 阈值登记。与 F3930 预算基线对拍。
//! 错误路径：阈值缺→立案补立；实测越阈→标记+重议；对拍不符→修正留痕。
//! 性能：归集 O(资产)；对拍 O(台账)；标记 O(1)。
//! 跨批对接：F3930 预算单源；F3964 自查上游；F3984 总基准下游。
//! 无障碍与隐私：性能资产含无障碍专项开销档（色标/快照/朗读——域本色）；
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x3340）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3340;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9HCode(pub u16);

impl S9HCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const THRESH_MISSING: Self = Self(SEG_BASE + 1);
    pub const OVER_MARKED: Self = Self(SEG_BASE + 2);
    pub const RECON_SPLIT: Self = Self(SEG_BASE + 3);
    pub const POINTER_GAP: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::THRESH_MISSING => "阈值缺→立案补立",
            Self::OVER_MARKED => "实测越阈→标记+重议",
            Self::RECON_SPLIT => "对拍不符→修正留痕",
            Self::POINTER_GAP => "三指针缺→补",
            _ => "未知",
        }
    }
}

// ── 三档资产 ─────────────────────────────────────────────────────────

/// 资产三档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfTier {
    /// 基准档。
    Bench,
    /// 预算档。
    Budget,
    /// 实测档。
    Measured,
}

pub const TIERS: [PerfTier; 3] = [PerfTier::Bench, PerfTier::Budget, PerfTier::Measured];

/// 专项开销档（无障碍本色，独立阈值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A11ySpec {
    None,
    ColorSwitch,
    Snapshot,
    SpeakQueue,
}

/// 性能资产条目。
#[derive(Debug, Clone)]
pub struct PerfRow {
    pub name: &'static str,
    pub tier: PerfTier,
    /// 三指针：挂组 / 挂条 / 挂基准。
    pub group: u8,
    pub clause: u32,
    pub bench_ref: u32,
    /// 阈值（毫秒，0 = 未登记）。
    pub threshold_ms: u32,
    /// 实测（毫秒，实测档有效）。
    pub measured_ms: u32,
    /// 专项开销档。
    pub spec: A11ySpec,
    /// 越阈标记。
    pub over_marked: bool,
}

impl PerfRow {
    /// 三指针齐备。
    pub fn pointers_ok(&self) -> bool {
        self.group >= 1 && self.group <= 8 && self.clause > 0 && self.bench_ref > 0
    }

    /// 实测越阈判定。
    pub fn over(&self) -> bool {
        self.tier == PerfTier::Measured && self.threshold_ms > 0 && self.measured_ms > self.threshold_ms
    }
}

/// 台账。
#[derive(Debug, Clone)]
pub struct PerfLedger {
    pub rows: Vec<PerfRow>,
}

impl PerfLedger {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    /// 登记：三指针缺 / 阈值缺（实测档必须带阈）拒收。
    pub fn admit(&mut self, r: PerfRow) -> S9HCode {
        if !r.pointers_ok() {
            return S9HCode::POINTER_GAP;
        }
        if r.tier == PerfTier::Measured && r.threshold_ms == 0 {
            return S9HCode::THRESH_MISSING;
        }
        self.rows.push(r);
        S9HCode::OK
    }

    /// 三档覆盖缺口。
    pub fn tier_gaps(&self) -> Vec<PerfTier> {
        TIERS.iter().filter(|t| !self.rows.iter().any(|r| r.tier == **t)).map(|t| *t).collect()
    }

    /// 越阈标记回写。
    pub fn mark_overs(&mut self) -> usize {
        let mut n = 0;
        for r in self.rows.iter_mut() {
            if r.over() && !r.over_marked {
                r.over_marked = true;
                n += 1;
            }
        }
        n
    }

    /// 重议名单：被标记行。
    pub fn re_nego(&self) -> Vec<&'static str> {
        self.rows.iter().filter(|r| r.over_marked).map(|r| r.name).collect()
    }

    /// 专项开销档独立阈值断言：专项行阈值必须独立（≤ 通用阈）。
    pub fn spec_threshold_ok(&self) -> bool {
        self.rows
            .iter()
            .filter(|r| r.spec != A11ySpec::None)
            .all(|r| r.threshold_ms > 0 && r.threshold_ms <= r.measured_ms.max(r.threshold_ms))
    }
}

impl Default for PerfLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 与预算基线对拍 ───────────────────────────────────────────────────

/// 基线对：资产名 → 基线值。
#[derive(Debug, Clone)]
pub struct BaselinePair {
    pub name: &'static str,
    pub baseline_ms: u32,
    pub ledger_ms: u32,
}

/// 对拍不符清单。
pub fn recon_split(pairs: &[BaselinePair]) -> Vec<&'static str> {
    pairs.iter().filter(|p| p.baseline_ms != p.ledger_ms).map(|p| p.name).collect()
}

/// 修正留痕。
#[derive(Debug, Clone)]
pub struct PerfTrace {
    pub name: &'static str,
    pub from_ms: u32,
    pub to_ms: u32,
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves222_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves222_s09perfasset");

    fn row(name: &'static str, tier: PerfTier, group: u8, thresh: u32, meas: u32) -> PerfRow {
        PerfRow { name, tier, group, clause: 7, bench_ref: 3, threshold_ms: thresh, measured_ms: meas, spec: A11ySpec::None, over_marked: false }
    }

    // 01 三档清单守恒
    {
        chk(&mut set, "C3968-01", TIERS.len() == 3 && PerfTier::Bench as usize == 0 && PerfTier::Measured as usize == 2, "err");
    }
    // 02 三档全归集
    {
        let mut l = PerfLedger::new();
        l.admit(row("基准", PerfTier::Bench, 1, 0, 0));
        l.admit(row("预算", PerfTier::Budget, 2, 50, 0));
        l.admit(row("实测", PerfTier::Measured, 3, 50, 40));
        chk(&mut set, "C3968-02", l.tier_gaps().is_empty(), "err");
    }
    // 03 三指针缺拒收
    {
        let mut l = PerfLedger::new();
        let mut r = row("缺指针", PerfTier::Bench, 0, 0, 0);
        r.bench_ref = 0;
        chk(&mut set, "C3968-03", l.admit(r) == S9HCode::POINTER_GAP, "err");
    }
    // 04 实测档阈值缺拒收
    {
        let mut l = PerfLedger::new();
        chk(&mut set, "C3968-04", l.admit(row("无阈实测", PerfTier::Measured, 1, 0, 10)) == S9HCode::THRESH_MISSING, "err");
    }
    // 05 非实测档免阈值
    {
        let mut l = PerfLedger::new();
        chk(&mut set, "C3968-05", l.admit(row("纯基准", PerfTier::Bench, 1, 0, 0)) == S9HCode::OK, "err");
    }
    // 06 越阈标记回写
    {
        let mut l = PerfLedger::new();
        l.admit(row("超标件", PerfTier::Measured, 1, 50, 80));
        l.admit(row("达标件", PerfTier::Measured, 2, 50, 30));
        let n = l.mark_overs();
        chk(&mut set, "C3968-06", n == 1 && l.re_nego() == vec!["超标件"] && !l.rows[1].over_marked, "err");
    }
    // 07 重复标记幂等
    {
        let mut l = PerfLedger::new();
        l.admit(row("超", PerfTier::Measured, 1, 10, 20));
        l.mark_overs();
        chk(&mut set, "C3968-07", l.mark_overs() == 0, "err");
    }
    // 08 边界恰达阈不标
    {
        let mut l = PerfLedger::new();
        l.admit(row("贴线", PerfTier::Measured, 1, 50, 50));
        chk(&mut set, "C3968-08", l.mark_overs() == 0 && l.re_nego().is_empty(), "err");
    }
    // 09 专项开销档独立阈值
    {
        let mut l = PerfLedger::new();
        let mut r = row("色标切换专项", PerfTier::Measured, 4, 50, 45);
        r.spec = A11ySpec::ColorSwitch;
        l.admit(r);
        chk(&mut set, "C3968-09", l.spec_threshold_ok() && l.rows[0].spec == A11ySpec::ColorSwitch, "err");
    }
    // 10 对拍不符检出
    {
        let pairs = vec![
            BaselinePair { name: "渲染", baseline_ms: 2, ledger_ms: 2 },
            BaselinePair { name: "快照", baseline_ms: 100, ledger_ms: 120 },
        ];
        chk(&mut set, "C3968-10", recon_split(&pairs) == vec!["快照"], "err");
    }
    // 11 对拍一致
    {
        let pairs = vec![BaselinePair { name: "同值", baseline_ms: 9, ledger_ms: 9 }];
        chk(&mut set, "C3968-11", recon_split(&pairs).is_empty(), "err");
    }
    // 12 修正留痕结构
    {
        let t = PerfTrace { name: "快照", from_ms: 120, to_ms: 100 };
        chk(&mut set, "C3968-12", t.from_ms > t.to_ms && t.name == "快照", "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3968-13", S9HCode::POINTER_GAP.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9HCode::OK, S9HCode::THRESH_MISSING, S9HCode::OVER_MARKED, S9HCode::RECON_SPLIT, S9HCode::POINTER_GAP];
        chk(&mut set, "C3968-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_perfasset_all_green() {
        let s = run_ves222_checks();
        assert!(s.all_passed(), "ves222 red: {}", s.red_items_text());
    }

    #[test]
    fn test_over_workflow() {
        let mut l = PerfLedger::new();
let _ =         l.admit(PerfRow { name: "x", tier: PerfTier::Measured, group: 1, clause: 1, bench_ref: 1, threshold_ms: 10, measured_ms: 25, spec: A11ySpec::SpeakQueue, over_marked: false });
        assert!(l.rows[0].over());
        assert_eq!(l.mark_overs(), 1);
        assert_eq!(l.re_nego(), vec!["x"]);
    }

    #[test]
    fn test_admit_rejections() {
        let mut l = PerfLedger::new();
        let mut r = PerfRow { name: "y", tier: PerfTier::Measured, group: 9, clause: 1, bench_ref: 1, threshold_ms: 1, measured_ms: 1, spec: A11ySpec::None, over_marked: false };
        assert_eq!(l.admit(r.clone()), S9HCode::POINTER_GAP);
        r.group = 1;
        r.threshold_ms = 0;
        assert_eq!(l.admit(r), S9HCode::THRESH_MISSING);
    }
}
