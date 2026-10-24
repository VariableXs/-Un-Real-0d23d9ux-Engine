//! VE-F4342 · 一致性自动化测试（U08 测试与资产组）
//! 职责：一致性自动化测试体系——测试三层（单元/组件/端到端覆盖判定引擎
//! 与工具面）+ 测试资产两件（用例生成器 + 判定预言机——期望违例清单比对）
//! + 覆盖率双门（判定引擎行覆盖 + 规则覆盖双达标）+ 预言机失准双人复核
//! 修订 + 覆盖不达标阻断合入 + flaky 隔离诊断队列 + 无障碍回归层（域本色）。
//! 段位：0x3C60（续表 idx576）。行数预算 360。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3C60;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtCode {
    /// 覆盖不达标（阻断合入）。
    CoverageShort = SEG_BASE,
    /// 预言机失准（双人复核修订）。
    OracleOff = SEG_BASE + 0x01,
    /// flaky 检出（隔离诊断队列）。
    Flaky = SEG_BASE + 0x02,
    /// 层号越界。
    LayerRange = SEG_BASE + 0x03,
}

/// 三层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Unit,
    Component,
    EndToEnd,
}

/// 预言机期望条目：(输入指纹, 期望违例数)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OracleRow {
    pub input_fp: u32,
    pub expect_violations: u32,
}

/// 用例执行记录。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaseRun {
    pub layer: Layer,
    pub passed: bool,
    /// 运行次数（flaky 判定：同用例结果不稳定）。
    pub runs: u32,
    pub flips: u32,
    /// 无障碍回归层覆盖（域色）。
    pub a11y: bool,
}

/// 自动化台。
#[derive(Default)]
pub struct AutoTest {
    pub oracle: Vec<OracleRow>,
    pub runs: Vec<CaseRun>,
    /// 行覆盖 bp / 规则覆盖 bp。
    pub line_cov_bp: u32,
    pub rule_cov_bp: u32,
    pub quarantined: Vec<u32>,
    pub oracle_revisions: u32,
    pub reviewers: Vec<u32>,
}

/// 覆盖率双门达标线。
pub const COV_PASS_BP: u32 = 8000;

impl AutoTest {
    pub fn new() -> Self {
        AutoTest { ..Default::default() }
    }

    /// 预言机装载。
    pub fn load_oracle(&mut self, rows: Vec<OracleRow>) {
        self.oracle = rows;
    }

    /// 预言机比对：实际违例数与期望不符 = 失准（登记复核）。
    pub fn check_oracle(&mut self, input_fp: u32, actual: u32) -> Result<bool, AtCode> {
        let row = self.oracle.iter().find(|r| r.input_fp == input_fp).ok_or(AtCode::OracleOff)?;
        if row.expect_violations != actual {
            self.oracle_revisions += 1;
            return Err(AtCode::OracleOff);
        }
        Ok(true)
    }

    /// 双人复核修订：两个不同复核人签后预言机行修订生效。
    pub fn dual_review(&mut self, r1: u32, r2: u32, input_fp: u32, new_expect: u32) -> Result<(), AtCode> {
        if r1 == 0 || r2 == 0 || r1 == r2 {
            return Err(AtCode::OracleOff);
        }
        for r in [r1, r2] {
            if !self.reviewers.contains(&r) {
                self.reviewers.push(r);
            }
        }
        let row = self.oracle.iter_mut().find(|r| r.input_fp == input_fp).ok_or(AtCode::OracleOff)?;
        row.expect_violations = new_expect;
        Ok(())
    }

    /// 用例执行登记（结果翻转计数——flaky 检测输入）。
    pub fn run_case(&mut self, layer: Layer, passed: bool, a11y: bool) {
        // 同层尾接续跑：翻转检测。
        let (runs, flips) = match self.runs.last_mut() {
            Some(last) if last.layer == layer => {
                if last.passed != passed {
                    last.flips += 1;
                }
                last.runs += 1;
                (last.runs, last.flips)
            }
            _ => (1, 0),
        };
        match self.runs.last_mut() {
            Some(last) if last.layer == layer => {
                last.passed = passed;
                last.a11y = a11y;
            }
            _ => self.runs.push(CaseRun { layer, passed, runs, flips, a11y }),
        }
    }

    /// flaky 隔离：翻转率超阈（≥2 次/5 跑）入隔离诊断队列。
    pub fn quarantine_flaky(&mut self) -> usize {
        let mut n = 0;
        for (i, r) in self.runs.iter().enumerate() {
            if r.runs >= 5 && r.flips * 5 >= r.runs * 2 && !self.quarantined.contains(&(i as u32)) {
                self.quarantined.push(i as u32);
                n += 1;
            }
        }
        n
    }

    /// 覆盖率双门：行覆盖与规则覆盖双达标才放行。
    pub fn cov_gate(&self) -> Result<(), AtCode> {
        if self.line_cov_bp < COV_PASS_BP || self.rule_cov_bp < COV_PASS_BP {
            return Err(AtCode::CoverageShort);
        }
        Ok(())
    }

    /// 无障碍回归层断言（域色）：三层至少端到端含 a11y。
    pub fn a11y_layer_ok(&self) -> bool {
        self.runs.iter().any(|r| r.layer == Layer::EndToEnd && r.a11y)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves576_checks() -> CheckSet {
    let mut set = CheckSet::new("ves576");
    let mut at = AutoTest::new();

    // C576-01 预言机装载 + 比对通过。
    at.load_oracle(vec![
        OracleRow { input_fp: 0xA1, expect_violations: 2 },
        OracleRow { input_fp: 0xB2, expect_violations: 0 },
    ]);
    chk(&mut set, "C576-01", at.check_oracle(0xA1, 2) == Ok(true) && at.check_oracle(0xB2, 0) == Ok(true), "预言机比对过");

    // C576-02 预言机失准检出（计数）。
    chk(&mut set, "C576-02", at.check_oracle(0xA1, 5) == Err(AtCode::OracleOff) && at.oracle_revisions == 1, "失准检出");

    // C576-03 双人复核修订（同人拒/零号拒）。
    chk(&mut set, "C576-03", at.dual_review(7, 7, 0xA1, 2).is_err() && at.dual_review(0, 8, 0xA1, 2).is_err(), "同人/零号拒");

    // C576-04 异人复核修订生效。
    chk(&mut set, "C576-04", at.dual_review(7, 8, 0xA1, 3).is_ok()
        && at.check_oracle(0xA1, 3) == Ok(true), "双人修订生效");

    // C576-05 三层用例登记。
    at.run_case(Layer::Unit, true, false);
    at.run_case(Layer::Component, true, false);
    at.run_case(Layer::EndToEnd, true, true);
    chk(&mut set, "C576-05", at.runs.len() == 3 && at.a11y_layer_ok(), "三层登记含无障碍");

    // C576-06 flaky 翻转检出与隔离。
    for p in [true, false, true, false, true] {
        at.run_case(Layer::Unit, p, false);
    }
    let q = at.quarantine_flaky();
    chk(&mut set, "C576-06", q >= 1 && !at.quarantined.is_empty(), "flaky 隔离");

    // C576-07 稳定用例不入隔离。
    let mut at2 = AutoTest::new();
    chk(&mut set, "C576-07", at2.quarantine_flaky() == 0, "稳定不隔离");

    // C576-08 覆盖率双门不达标阻断。
    at.line_cov_bp = 7500;
    at.rule_cov_bp = 9000;
    chk(&mut set, "C576-08", at.cov_gate() == Err(AtCode::CoverageShort), "行覆盖不足阻断");

    // C576-09 双门达标放行（边界恰达）。
    at.line_cov_bp = COV_PASS_BP;
    chk(&mut set, "C576-09", at.cov_gate().is_ok(), "双门边界达标");

    // C576-10 未登记输入预言机拒绝（不臆测）。
    chk(&mut set, "C576-10", at.check_oracle(0xFFFF, 0).is_err(), "未登记不臆测");

    // C576-11 层序登记互不串档（翻转只计同层）。
    let mut at3 = AutoTest::new();
    at3.run_case(Layer::Unit, true, false);
    at3.run_case(Layer::Component, false, false);
    at3.run_case(Layer::Unit, false, false);
    chk(&mut set, "C576-11", at3.runs.len() == 3 && at3.runs[2].flips == 1, "同层翻转计数");

    // C576-12 无障碍层缺失断言（域色）。
    let at4 = AutoTest::new();
    chk(&mut set, "C576-12", !at4.a11y_layer_ok(), "无障碍层缺失检出");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_dual_review() {
        let mut at = AutoTest::new();
        at.load_oracle(vec![OracleRow { input_fp: 1, expect_violations: 1 }]);
        assert_eq!(at.check_oracle(1, 2), Err(AtCode::OracleOff));
        assert!(at.dual_review(1, 2, 1, 2).is_ok());
        assert_eq!(at.check_oracle(1, 2), Ok(true));
    }

    #[test]
    fn flaky_detection() {
        let mut at = AutoTest::new();
        for p in [true, false, true, false, true, false] {
            at.run_case(Layer::Unit, p, false);
        }
        assert_eq!(at.quarantine_flaky(), 1);
        assert_eq!(at.quarantine_flaky(), 0); // 幂等
    }

    #[test]
    fn coverage_dual_gate() {
        let at = AutoTest::new();
        assert_eq!(at.cov_gate(), Err(AtCode::CoverageShort)); // 默认 0
    }
}
