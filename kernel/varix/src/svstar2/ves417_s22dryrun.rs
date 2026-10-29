//! ves417_s22dryrun — VE-F4177 · 预备演练
//! T 域收口前预备演练：终审全流程空跑一遍（联测脚本干跑 + 评分卡空转 + 走查表试填 + 报告模板试出）；
//! 演练发现直通 F4172 补齐；演练失败→发现直通补齐；演练通过→放行收口；脚本缺→补本；
//! 多语种剧本干跑（域本色）；试填表不含真实证据。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0F40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrCode {
    Ok = 0,
    ScriptMissing = SEG_BASE + 0x01, // 0x0F41
    FindOpen = SEG_BASE + 0x02,      // 0x0F42
}

/// 空跑四步。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DryStep { TestScript = 0, ScoreCard = 1, WalkSheet = 2, ReportTemplate = 3 }

impl DryStep {
    pub const ALL: [DryStep; 4] = [
        DryStep::TestScript, DryStep::ScoreCard, DryStep::WalkSheet, DryStep::ReportTemplate];
    pub fn name(self) -> &'static str {
        match self {
            DryStep::TestScript => "test-script", DryStep::ScoreCard => "score-card",
            DryStep::WalkSheet => "walk-sheet", DryStep::ReportTemplate => "report-template",
        }
    }
}

/// 演练发现（直通 F4172 补齐）。
#[derive(Clone, Copy)]
pub struct Finding {
    pub step: DryStep,
    pub desc: &'static str,
    pub straight_to_fill: bool, // 直通补齐单
    pub fixed: bool,
}

/// 演练册。
pub struct DryRunBook {
    pub steps: Vec<(DryStep, bool)>,      // 空跑四步（bool = 完成）
    pub findings: Vec<Finding>,
    pub multilang_dry: bool,              // 多语种剧本干跑（域本色）
    pub sheets_clean: bool,               // 试填表不含真实证据
    pub scripts_written: Vec<&'static str>, // 补本留痕
}

impl DryRunBook {
    pub fn new() -> Self {
        DryRunBook {
            steps: DryStep::ALL.iter().map(|s| (*s, false)).collect(),
            findings: Vec::new(), multilang_dry: false, sheets_clean: true, scripts_written: Vec::new(),
        }
    }

    /// 单步空跑（顺序推进）。
    pub fn run_next(&mut self) -> bool {
        match self.steps.iter_mut().find(|x| !x.1) {
            Some(x) => { x.1 = true; true }
            None => false,
        }
    }

    pub fn full(&self) -> bool { self.steps.iter().all(|x| x.1) }

    pub fn break_at(&self) -> Option<&'static str> {
        self.steps.iter().find(|x| !x.1).map(|x| x.0.name())
    }

    /// 发现登记（直通补齐单）。
    pub fn file_finding(&mut self, step: DryStep, desc: &'static str) {
        self.findings.push(Finding { step, desc, straight_to_fill: true, fixed: false });
    }

    /// 直通补齐执行。
    pub fn fix_finding(&mut self, idx: usize) -> bool {
        match self.findings.get_mut(idx) {
            Some(f) => { f.fixed = true; true }
            None => false,
        }
    }

    pub fn open_findings(&self) -> Vec<usize> {
        self.findings.iter().enumerate().filter(|(_, f)| !f.fixed).map(|(i, _)| i).collect()
    }

    /// 补本留痕。
    pub fn write_script(&mut self, what: &'static str) { self.scripts_written.push(what); }

    /// 放行门：四步全跑 + 发现清零 + 多语种干跑 + 试填洁净 → 放行收口。
    pub fn release(&self) -> bool {
        self.full() && self.open_findings().is_empty() && self.multilang_dry && self.sheets_clean
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves417_checks() -> CheckSet {
    let mut set = CheckSet::new("ves417");
    let mut bk = DryRunBook::new();

    // C417-01 四步空跑顺序完成。
    for _ in 0..4 { bk.run_next(); }
    chk(&mut set, "C417-01", bk.full() && bk.break_at().is_none(), "四步全跑");

    // C417-02 链断定位。
    let bk0 = DryRunBook::new();
    chk(&mut set, "C417-02", bk0.break_at() == Some("test-script"), "空册断于首步");

    // C417-03 演练发现直通补齐。
    bk.file_finding(DryStep::WalkSheet, "走查表缺 RTL 列");
    chk(&mut set, "C417-03", bk.open_findings() == vec![0]
        && bk.findings[0].straight_to_fill, "发现直通单");

    // C417-04 补齐后清零。
    bk.fix_finding(0);
    chk(&mut set, "C417-04", bk.open_findings().is_empty(), "补齐清零");

    // C417-05 多语种剧本干跑（域本色）。
    bk.multilang_dry = true;
    chk(&mut set, "C417-05", bk.multilang_dry, "多语种干跑过");

    // C417-06 试填表洁净。
    chk(&mut set, "C417-06", bk.sheets_clean, "试填无真实证据");

    // C417-07 放行收口门。
    chk(&mut set, "C417-07", bk.release(), "放行收口");

    // C417-08 发现未清不放行。
    let mut bk2 = DryRunBook::new();
    for _ in 0..4 { bk2.run_next(); }
    bk2.multilang_dry = true;
    bk2.file_finding(DryStep::ScoreCard, "评分卡空转卡死");
    chk(&mut set, "C417-08", !bk2.release() && bk2.open_findings().len() == 1, "发现未清不放行");

    // C417-09 脚本缺 → 补本。
    let mut bk3 = DryRunBook::new();
    bk3.write_script("补联测脚本 dry-v2");
    chk(&mut set, "C417-09", bk3.scripts_written == vec!["补联测脚本 dry-v2"], "补本留痕");

    // C417-10 步进封顶（run_next 跑完即停）。
    let mut bk4 = DryRunBook::new();
    for _ in 0..9 { bk4.run_next(); }
    chk(&mut set, "C417-10", bk4.full(), "步进封顶四步");

    // C417-11 试填表污染检出。
    let mut bk5 = DryRunBook::new();
    for _ in 0..4 { bk5.run_next(); }
    bk5.multilang_dry = true;
    bk5.sheets_clean = false;
    chk(&mut set, "C417-11", !bk5.release(), "污染不放行");

    // C417-12 四步名单单源。
    chk(&mut set, "C417-12", DryStep::ALL.len() == 4 && DryStep::ALL[0].name() == "test-script"
        && DryStep::ALL[3].name() == "report-template", "四步单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves417_smoke() {
        let set = run_ves417_checks();
        assert!(set.failed_count() == 0, "ves417 red: {}", set.red_items_text());
    }
}
