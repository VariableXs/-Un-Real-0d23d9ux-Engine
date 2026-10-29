//! ves472_s25joint — VE-F4238 · U02 联调
//! U02 组内联调：20 条目贯通演练（DSL→验证→版本→冲突→影响→测试→文档全链跑通）+
//! 故障注入（签名错/沙箱逃逸/谱系环）三演练；证据挂 F4207 度量；
//! 链断→定位+回执；演练失败→预案验证；证据缺→重跑；
//! 含无障碍判据链路抽演（域本色）；数据合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1960;
pub const STAGE_N: usize = 7;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JtCode {
    Ok = 0,
    ChainBroken = SEG_BASE + 0x01, // 0x1961
    DrillFailed = SEG_BASE + 0x02, // 0x1962
    EvidenceMissing = SEG_BASE + 0x03, // 0x1963
}

/// 全链七段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage { Dsl = 0, Validate = 1, Version = 2, Conflict = 3, Impact = 4, Test = 5, Docs = 6 }

/// 联调册。
pub struct JointBook {
    pub stages: Vec<Stage>,
    pub drills: Vec<(&'static str, bool)>,
    pub a11y_drill: bool,
    pub evidence: Vec<&'static str>,
    pub reruns: u32,
}

impl JointBook {
    pub fn new() -> Self { JointBook { stages: Vec::new(), drills: Vec::new(), a11y_drill: false, evidence: Vec::new(), reruns: 0 } }
    pub fn advance(&mut self, s: Stage) -> Result<(), JtCode> {
        if s as usize != self.stages.len() { return Err(JtCode::ChainBroken); }
        self.stages.push(s);
        Ok(())
    }
    pub fn drill(&mut self, name: &'static str, ok: bool) {
        self.drills.retain(|(n, _)| *n != name);
        self.drills.push((name, ok));
    }
    pub fn rerun(&mut self, name: &'static str, ok: bool) -> bool {
        self.reruns += 1;
        for d in self.drills.iter_mut() { if d.0 == name { d.1 = ok; } }
        ok
    }
    pub fn attach(&mut self, ev: &'static str) { if !self.evidence.contains(&ev) { self.evidence.push(ev); } }
    pub fn final_ok(&self) -> Result<(), JtCode> {
        if self.stages.len() < STAGE_N { return Err(JtCode::ChainBroken); }
        if self.drills.len() < 3 || self.drills.iter().any(|(_, ok)| !ok) { return Err(JtCode::DrillFailed); }
        if !self.a11y_drill { return Err(JtCode::DrillFailed); }
        if self.evidence.is_empty() { return Err(JtCode::EvidenceMissing); }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves472_checks() -> CheckSet {
    let mut set = CheckSet::new("ves472");
    let mut bk = JointBook::new();

    // C472-01 越段→链断。
    chk(&mut set, "C472-01", bk.advance(Stage::Validate) == Err(JtCode::ChainBroken), "越段链断");

    // C472-02 七段顺序贯通。
    for s in [Stage::Dsl, Stage::Validate, Stage::Version, Stage::Conflict, Stage::Impact, Stage::Test, Stage::Docs] {
        bk.advance(s).unwrap();
    }
    chk(&mut set, "C472-02", bk.stages.len() == STAGE_N, "全链贯通");

    // C472-03 三故障注入演练（签名错/沙箱逃逸/谱系环）。
    bk.drill("sig-wrong", true);
    bk.drill("sandbox-escape", true);
    bk.drill("lineage-cycle", true);
    chk(&mut set, "C472-03", bk.drills.iter().all(|(_, ok)| *ok), "三演练过");

    // C472-04 演练失败→预案重跑。
    bk.drill("lineage-cycle", false);
    chk(&mut set, "C472-04", bk.rerun("lineage-cycle", true) && bk.reruns == 1, "预案重跑");

    // C472-05 a11y 抽演缺→终态拒（域本色）。
    chk(&mut set, "C472-05", bk.final_ok() == Err(JtCode::DrillFailed), "抽演缺拒");

    // C472-06 a11y 抽演补齐→证据缺拒。
    bk.a11y_drill = true;
    chk(&mut set, "C472-06", bk.final_ok() == Err(JtCode::EvidenceMissing), "证据缺拒");

    // C472-07 证据挂 F4207 度量。
    bk.attach("metric-f4207-consistency");
    chk(&mut set, "C472-07", bk.final_ok().is_ok(), "联调终态");

    // C472-08 三故障名单单源。
    chk(&mut set, "C472-08", bk.drills == vec![
        ("sig-wrong", true), ("sandbox-escape", true), ("lineage-cycle", true)], "三故障单源");

    // C472-09 七段单源断言。
    chk(&mut set, "C472-09", Stage::Dsl as u8 == 0 && Stage::Docs as u8 == 6, "七段单源");

    // C472-10 重跑计数累计。
    bk.rerun("sig-wrong", true);
    chk(&mut set, "C472-10", bk.reruns == 2, "重跑计数");

    // C472-11 证据幂等。
    bk.attach("metric-f4207-consistency");
    chk(&mut set, "C472-11", bk.evidence.len() == 1, "证据幂等");

    // C472-12 错误码段位单源。
    chk(&mut set, "C472-12", JtCode::EvidenceMissing as u16 == SEG_BASE + 3, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves472_smoke() {
        let set = run_ves472_checks();
        assert!(set.failed_count() == 0, "ves472 red: {}", set.red_items_text());
    }
}
