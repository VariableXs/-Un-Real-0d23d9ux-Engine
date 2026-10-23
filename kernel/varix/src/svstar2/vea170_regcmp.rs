//! VE-F0170 · 回归截图自动比对服务（VE-Test · 目标 320 行）
//!
//! 铁律：比对精确，阈值可调，差异可查，历史可存。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8F0;
pub const XC_REGRESSION_DETECTED: u16 = SEG_BASE | 0x01;

pub struct RegressionCompare {
    threshold_pct: u32,
    comparisons: u64,
    regressions: u32,
    last_code: u16,
}

impl RegressionCompare {
    pub fn new(threshold_pct: u32) -> Self { Self { threshold_pct, comparisons: 0, regressions: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    /// 比对两帧（diff_bps = 差异基点，1/10000）。
    pub fn compare(&mut self, diff_bps: u32) -> Result<bool, u16> {
        self.comparisons += 1;
        if diff_bps > self.threshold_pct * 100 {
            self.regressions += 1;
            self.last_code = XC_REGRESSION_DETECTED;
            return Err(XC_REGRESSION_DETECTED);
        }
        self.last_code = 0;
        Ok(true)
    }
    pub fn comparisons(&self) -> u64 { self.comparisons }
    pub fn regressions(&self) -> u32 { self.regressions }
}

pub fn run_vea170_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea170_regcmp");
    { let r = RegressionCompare::new(5); chk(&mut set, "C170-01", r.regressions() == 0, "err"); }
    { let mut r = RegressionCompare::new(5); let res = r.compare(100);
        chk(&mut set, "C170-02", res.is_ok(), "err"); }
    { let mut r = RegressionCompare::new(5); let res = r.compare(1000);
        chk(&mut set, "C170-03", res.err() == Some(XC_REGRESSION_DETECTED), "err"); }
    { let mut r = RegressionCompare::new(5); r.compare(100); r.compare(100);
        chk(&mut set, "C170-04", r.comparisons() == 2, "err"); }
    { let mut r = RegressionCompare::new(0); let res = r.compare(1);
        chk(&mut set, "C170-05", res.err() == Some(XC_REGRESSION_DETECTED), "err"); }
    { let mut r = RegressionCompare::new(5); r.compare(600);
        chk(&mut set, "C170-06", r.regressions() == 1, "err"); }
    { let r = RegressionCompare::new(5); chk(&mut set, "C170-07", r.comparisons() == 0, "err"); }
    { let mut r = RegressionCompare::new(5); r.compare(100);
        chk(&mut set, "C170-08", r.last_code() == 0, "err"); }
    { let mut r = RegressionCompare::new(5); r.compare(1000);
        chk(&mut set, "C170-09", r.last_code() == XC_REGRESSION_DETECTED, "err"); }
    { let mut r = RegressionCompare::new(5); r.compare(500);
        chk(&mut set, "C170-10", r.regressions() == 0, "err"); }
    set
}
