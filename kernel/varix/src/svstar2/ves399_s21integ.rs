//! ves399_s21integ — VE-F4159 · T08 联调
//! 组内联调：20 条目贯通演练（审查→仿真→语料→回归→走查工具链全链跑通）
//! + 故障注入三演练（审查器误报飙升/仿真泄漏/语料分级错）；证据挂 F4148 看板；
//! 链断→定位+回执；演练失败→预案验证；证据缺→重跑；读屏仿真抽演（域本色）；语料合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0080;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IgCode {
    Ok = 0,
    ChainBroken = SEG_BASE + 0x01,   // 0x0081
    DrillFailed = SEG_BASE + 0x02,   // 0x0082
    EvidenceMissing = SEG_BASE + 0x03, // 0x0083
}

/// 工具链五站（审查→仿真→语料→回归→走查）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Station { Linter = 0, Sim = 1, Corpus = 2, Regress = 3, Walkthrough = 4 }

impl Station {
    pub const CHAIN: [Station; 5] = [
        Station::Linter, Station::Sim, Station::Corpus, Station::Regress, Station::Walkthrough];
    pub fn name(self) -> &'static str {
        match self {
            Station::Linter => "linter", Station::Sim => "sim", Station::Corpus => "corpus",
            Station::Regress => "regress", Station::Walkthrough => "walkthrough",
        }
    }
}

/// 故障注入三类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault { LinterFpSurge = 0, SimLeak = 1, CorpusGradeWrong = 2 }

impl Fault {
    pub const ALL: [Fault; 3] = [Fault::LinterFpSurge, Fault::SimLeak, Fault::CorpusGradeWrong];
    pub fn name(self) -> &'static str {
        match self {
            Fault::LinterFpSurge => "linter-fp-surge", Fault::SimLeak => "sim-leak",
            Fault::CorpusGradeWrong => "corpus-grade-wrong",
        }
    }
}

/// 贯通演练：五站链 + 每站产出证据。
pub struct ChainDrill {
    pub passed: Vec<Station>,      // 已贯通站
    pub evidence: Vec<&'static str>, // 证据（挂看板）
    pub drills: Vec<(Fault, bool)>,  // 三演练（故障, 预案验证过）
    pub screen_reader_drill: bool,   // 读屏仿真抽演（域本色）
}

impl ChainDrill {
    pub fn new() -> Self {
        ChainDrill { passed: Vec::new(), evidence: Vec::new(), drills: Vec::new(), screen_reader_drill: false }
    }

    /// 单站贯通（顺序推进：只能贯通下一站）。
    pub fn pass_next(&mut self) -> bool {
        let next = Station::CHAIN.iter().find(|s| !self.passed.contains(s));
        match next {
            Some(s) => { self.passed.push(*s); true }
            None => false,
        }
    }

    /// 链断定位：第一个未贯通站。
    pub fn break_at(&self) -> Option<&'static str> {
        Station::CHAIN.iter().find(|s| !self.passed.contains(s)).map(|s| s.name())
    }

    pub fn full_chain(&self) -> bool { self.passed.len() == 5 }

    /// 挂证据（幂等）。
    pub fn attach_evidence(&mut self, e: &'static str) -> bool {
        if e.is_empty() || self.evidence.contains(&e) { return false; }
        self.evidence.push(e);
        true
    }

    /// 故障注入演练：预案验证过 → 记 (fault, true)。
    pub fn drill(&mut self, f: Fault, plan_ok: bool) {
        if !self.drills.iter().any(|x| x.0 == f) { self.drills.push((f, plan_ok)); }
    }

    pub fn drills_ok(&self) -> bool {
        Fault::ALL.iter().all(|f| self.drills.iter().any(|x| x.0 == *f && x.1))
    }

    pub fn failed_drills(&self) -> Vec<&'static str> {
        Fault::ALL.iter()
            .filter(|f| self.drills.iter().any(|x| x.0 == **f && !x.1))
            .map(|f| f.name()).collect()
    }

    /// 联调放行：全链 + 三演练过 + 读屏抽演 + 证据齐。
    pub fn released(&self) -> bool {
        self.full_chain() && self.drills_ok() && self.screen_reader_drill
            && self.evidence.len() >= 2
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves399_checks() -> CheckSet {
    let mut set = CheckSet::new("ves399");
    let mut d = ChainDrill::new();

    // C399-01 五站顺序贯通。
    for _ in 0..5 { d.pass_next(); }
    chk(&mut set, "C399-01", d.full_chain() && d.break_at().is_none(), "五站全通");

    // C399-02 链断定位：空链断在 linter。
    let d0 = ChainDrill::new();
    chk(&mut set, "C399-02", d0.break_at() == Some("linter"), "空链断于首站");

    // C399-03 贯通两站后断在 corpus。
    let mut d2 = ChainDrill::new();
    d2.pass_next(); d2.pass_next();
    chk(&mut set, "C399-03", d2.break_at() == Some("corpus"), "断于第三站");

    // C399-04 证据挂链（幂等）。
    chk(&mut set, "C399-04", d.attach_evidence("chain-run-001")
        && d.attach_evidence("drill-report-002") && !d.attach_evidence("chain-run-001")
        && d.evidence.len() == 2, "证据二条幂等");

    // C399-05 三故障注入演练（预案验证过）。
    d.drill(Fault::LinterFpSurge, true);
    d.drill(Fault::SimLeak, true);
    d.drill(Fault::CorpusGradeWrong, true);
    chk(&mut set, "C399-05", d.drills_ok() && d.failed_drills().is_empty(), "三演练全过");

    // C399-06 演练失败 → 预案验证失败登记。
    let mut d3 = ChainDrill::new();
    d3.drill(Fault::LinterFpSurge, true);
    d3.drill(Fault::SimLeak, false);
    d3.drill(Fault::CorpusGradeWrong, true);
    chk(&mut set, "C399-06", d3.failed_drills() == vec!["sim-leak"] && !d3.drills_ok(), "演练失败定位");

    // C399-07 复演通过后翻绿（幂等登记不重复）。
    d3.drill(Fault::SimLeak, true);
    chk(&mut set, "C399-07", d3.drills_ok() && d3.drills.len() == 3, "复演翻绿");

    // C399-08 读屏仿真抽演（域本色）。
    d.screen_reader_drill = true;
    chk(&mut set, "C399-08", d.screen_reader_drill, "读屏抽演过");

    // C399-09 联调放行判定。
    chk(&mut set, "C399-09", d.released(), "全件齐放行");

    // C399-10 缺证据不放行。
    let mut d4 = ChainDrill::new();
    for _ in 0..5 { d4.pass_next(); }
    d4.drill(Fault::LinterFpSurge, true);
    d4.drill(Fault::SimLeak, true);
    d4.drill(Fault::CorpusGradeWrong, true);
    d4.screen_reader_drill = true;
    chk(&mut set, "C399-10", !d4.released(), "证据缺不放行");

    // C399-11 空证据拒挂。
    chk(&mut set, "C399-11", !d4.attach_evidence(""), "空证据拒");

    // C399-12 五站名单单源。
    chk(&mut set, "C399-12", Station::CHAIN.len() == 5 && Station::CHAIN[0].name() == "linter"
        && Station::CHAIN[4].name() == "walkthrough" && Fault::ALL.len() == 3, "五站三fault单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves399_smoke() {
        let set = run_ves399_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves399 red: {:?}", red);
    }

    #[test]
    fn ves399_pass_next_capped() {
        let mut d = ChainDrill::new();
        for _ in 0..8 { d.pass_next(); }
        assert_eq!(d.passed.len(), 5);
    }
}
