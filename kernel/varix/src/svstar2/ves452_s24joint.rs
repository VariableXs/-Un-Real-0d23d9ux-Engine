//! ves452_s24joint — VE-F4218 · U01 联调
//! U01 组内联调：20 条目贯通演练（模型→注册→规则→扫描→度量全链跑通）
//! +故障注入（契约悬空/规则冲突/图谱失步）三演练；证据挂度量体系；
//! 链断→定位+回执；演练失败→预案验证；证据缺→重跑；
//! 联调含无障碍规则链路抽演（域本色）；数据合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1560;
pub const STAGE_N: usize = 5; // 模型→注册→规则→扫描→度量

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JtCode {
    Ok = 0,
    ChainBroken = SEG_BASE + 0x01, // 0x1561
    DrillFailed = SEG_BASE + 0x02, // 0x1562
    EvidenceMissing = SEG_BASE + 0x03, // 0x1563
}

/// 五段流水线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage { Model = 0, Register = 1, Rule = 2, Scan = 3, Metric = 4 }

/// 联调册。
pub struct JointBook {
    pub stages: Vec<Stage>,      // 已贯通段
    pub drills: Vec<&'static str>, // 三演练（名称, 通过）
    pub drills_ok: Vec<bool>,
    pub evidence: Vec<&'static str>, // 证据挂度量
    pub a11y_drill: bool,            // 无障碍规则链路抽演（域本色）
    pub reruns: u32,
}

impl JointBook {
    pub fn new() -> Self { JointBook { stages: Vec::new(), drills: Vec::new(), drills_ok: Vec::new(), evidence: Vec::new(), a11y_drill: false, reruns: 0 } }
    /// 顺序贯通：只允许贯通紧邻下一段（链断→定位）。
    pub fn advance(&mut self, s: Stage) -> Result<(), JtCode> {
        if s as usize != self.stages.len() {
            return Err(JtCode::ChainBroken);
        }
        self.stages.push(s);
        Ok(())
    }
    /// 故障注入演练（登记+结果）。
    pub fn drill(&mut self, name: &'static str, passed: bool) {
        self.drills.push(name);
        self.drills_ok.push(passed);
    }
    /// 演练失败→预案验证重跑（幂等计数）。
    pub fn rerun(&mut self, name: &'static str, passed: bool) -> bool {
        self.reruns += 1;
        if let Some(i) = self.drills.iter().position(|n| *n == name) {
            self.drills_ok[i] = passed;
        }
        passed
    }
    /// 证据挂度量。
    pub fn attach(&mut self, ev: &'static str) { if !self.evidence.contains(&ev) { self.evidence.push(ev); } }
    /// 联调终态：全链贯通+三演练全过+a11y 抽演+证据齐。
    pub fn final_ok(&self) -> Result<(), JtCode> {
        if self.stages.len() < STAGE_N { return Err(JtCode::ChainBroken); }
        if self.drills.len() < 3 || self.drills_ok.iter().any(|x| !x) { return Err(JtCode::DrillFailed); }
        if !self.a11y_drill { return Err(JtCode::DrillFailed); }
        if self.evidence.is_empty() { return Err(JtCode::EvidenceMissing); }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves452_checks() -> CheckSet {
    let mut set = CheckSet::new("ves452");
    let mut bk = JointBook::new();

    // C452-01 越段贯通→链断定位。
    chk(&mut set, "C452-01", bk.advance(Stage::Register) == Err(JtCode::ChainBroken), "越段链断");

    // C452-02 顺序贯通五段。
    for s in [Stage::Model, Stage::Register, Stage::Rule, Stage::Scan, Stage::Metric] {
        bk.advance(s).unwrap();
    }
    chk(&mut set, "C452-02", bk.stages.len() == STAGE_N, "全链贯通");

    // C452-03 三故障注入演练（契约悬空/规则冲突/图谱失步）。
    bk.drill("contract-dangling", true);
    bk.drill("rule-conflict", true);
    bk.drill("graph-desync", true);
    chk(&mut set, "C452-03", bk.drills_ok.iter().all(|x| *x), "三演练过");

    // C452-04 演练失败→预案重跑验证。
    bk.drill("rule-conflict", false);
    chk(&mut set, "C452-04", bk.rerun("rule-conflict", true) && bk.reruns == 1
        && bk.drills_ok[1], "预案重跑");

    // C452-05 a11y 规则链路抽演缺→终态拒（域本色）。
    chk(&mut set, "C452-05", bk.final_ok() == Err(JtCode::DrillFailed), "a11y 抽演缺拒");

    // C452-06 a11y 抽演补齐。
    bk.a11y_drill = true;
    chk(&mut set, "C452-06", bk.final_ok() == Err(JtCode::EvidenceMissing), "证据缺拒");

    // C452-07 证据挂度量体系。
    bk.attach("metric-consistency-9466bp");
    chk(&mut set, "C452-07", bk.final_ok().is_ok(), "联调终态过");

    // C452-08 证据幂等。
    bk.attach("metric-consistency-9466bp");
    chk(&mut set, "C452-08", bk.evidence.len() == 1, "证据幂等");

    // C452-09 演练名单单源（三故障）。
    chk(&mut set, "C452-09", bk.drills == vec!["contract-dangling", "rule-conflict", "graph-desync"], "三故障单源");

    // C452-10 五段单源断言。
    chk(&mut set, "C452-10", Stage::Model as u8 == 0 && Stage::Metric as u8 == 4, "五段单源");

    // C452-11 重跑计数累计。
    bk.rerun("graph-desync", true);
    chk(&mut set, "C452-11", bk.reruns == 2, "重跑计数");

    // C452-12 数据合成断言（演练证据均为合成 id）。
    chk(&mut set, "C452-12", bk.evidence[0].starts_with("metric-"), "合成证据");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves452_smoke() {
        let set = run_ves452_checks();
        assert!(set.failed_count() == 0, "ves452 red: {}", set.red_items_text());
    }
}
