//! ves305_s15postedit — VE-F4065 · T04 译后编辑工作流
//! MT 输出→编辑→评审→定稿四步 + 编辑留痕（原文/机翻/终稿三栏 diff 可回放）+ 评审驳回循环上限 3 次 + 键盘可达复用声明 + 工作量计量。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x89C0;

pub const REVIEW_LOOP_MAX: u8 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum PeCode {
    Ok = 0,
    LoopOver = SEG_BASE + 0x01,
    TraceMissing = SEG_BASE + 0x02,
    StepSkipped = SEG_BASE + 0x03,
    EditorBroken = SEG_BASE + 0x04,
}

/// 四步。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Step { Machine = 0, Editing = 1, Review = 2, Final = 3 }

impl Step {
    pub const ALL: [Step; 4] = [Step::Machine, Step::Editing, Step::Review, Step::Final];
    pub fn name(self) -> &'static str {
        match self { Step::Machine => "机翻", Step::Editing => "编辑", Step::Review => "评审", Step::Final => "定稿" }
    }
}

/// 三栏留痕。
#[derive(Clone)]
pub struct Trace {
    pub source: String,
    pub mt: String,
    pub final_text: String,
}

impl Trace {
    pub fn complete(&self) -> bool {
        !self.source.is_empty() && !self.mt.is_empty() && !self.final_text.is_empty()
    }
    /// 编辑距离计量（简化：字符差数）。
    pub fn edit_distance(&self) -> u32 {
        let a: Vec<char> = self.mt.chars().collect();
        let b: Vec<char> = self.final_text.chars().collect();
        let mut d = (a.len() as i64 - b.len() as i64).unsigned_abs();
        for x in a.iter() {
            if !b.contains(x) { d += 1; }
        }
        d as u32
    }
}

/// 工作流状态机。
pub struct PeWorkflow {
    pub step: Step,
    pub loop_count: u8,
    pub trace: Option<Trace>,
    pub editor_alive: bool,
    pub history: Vec<Step>,
}

impl PeWorkflow {
    pub fn new() -> Self {
        PeWorkflow { step: Step::Machine, loop_count: 0, trace: None, editor_alive: true, history: Vec::new() }
    }

    /// 推进：必须按步序。
    pub fn advance(&mut self, to: Step) -> Result<(), PeCode> {
        if to as u8 != self.step as u8 + 1 { return Err(PeCode::StepSkipped); }
        self.step = to;
        self.history.push(to);
        Ok(())
    }

    /// 评审驳回：回编辑，循环上限 3 次后升级。
    pub fn reject(&mut self) -> Result<&'static str, PeCode> {
        if self.step != Step::Review { return Err(PeCode::StepSkipped); }
        self.loop_count += 1;
        if self.loop_count > REVIEW_LOOP_MAX {
            return Ok("升级");
        }
        self.step = Step::Editing;
        Ok("回编辑")
    }

    /// 定稿阻断：留痕缺拒。
    pub fn finalize(&mut self, t: Trace) -> Result<(), PeCode> {
        if !t.complete() { return Err(PeCode::TraceMissing); }
        if self.step != Step::Review { return Err(PeCode::StepSkipped); }
        self.trace = Some(t);
        self.step = Step::Final;
        Ok(())
    }

    /// 编辑器故障降级纯文本。
    pub fn editor_down(&mut self) -> &'static str {
        self.editor_alive = false;
        "纯文本模式"
    }
}

pub fn run_ves305_checks() -> CheckSet {
    let mut set = CheckSet::new("ves305_s15postedit");
    let mut wf = PeWorkflow::new();

    // 1) 跳步拒。
    set.add("C4065-01", wf.advance(Step::Review) == Err(PeCode::StepSkipped), "跳步拒");

    // 2) 正常四步流转。
    let _ = wf.advance(Step::Editing);
    let _ = wf.advance(Step::Review);
    set.add("C4065-02", wf.step == Step::Review && wf.history.len() == 2, "四步流转");

    // 3) 驳回回编辑。
    set.add("C4065-03", wf.reject() == Ok("回编辑") && wf.step == Step::Editing && wf.loop_count == 1, "驳回回编辑");

    // 4) 循环上限：3 次后升级。
    let _ = wf.advance(Step::Review);
    let _ = wf.reject();
    let _ = wf.advance(Step::Review);
    let _ = wf.reject();
    let _ = wf.advance(Step::Review);
    set.add("C4065-04", wf.reject() == Ok("升级") && wf.loop_count == REVIEW_LOOP_MAX + 1, "循环上限");

    // 5) 定稿留痕缺拒。
    let mut wf2 = PeWorkflow::new();
    let _ = wf2.advance(Step::Editing);
    let _ = wf2.advance(Step::Review);
    let bad = Trace { source: String::from("原文"), mt: String::new(), final_text: String::from("终") };
    set.add("C4065-05", wf2.finalize(bad) == Err(PeCode::TraceMissing), "留痕缺拒");

    // 6) 正常定稿。
    let good = Trace { source: String::from("原文"), mt: String::from("机翻"), final_text: String::from("终稿") };
    set.add("C4065-06", wf2.finalize(good).is_ok() && wf2.step == Step::Final, "正常定稿");

    // 7) 编辑距离计量。
    let t = Trace { source: String::from("s"), mt: String::from("abcd"), final_text: String::from("abxd") };
    set.add("C4065-07", t.edit_distance() >= 1, "编辑距离");

    // 8) 编辑器故障降级。
    let mut wf3 = PeWorkflow::new();
    set.add("C4065-08", wf3.editor_down() == "纯文本模式" && !wf3.editor_alive, "降级纯文本");

    // 9) 四步名双射。
    let mut names: Vec<&str> = Vec::new();
    for s in Step::ALL.iter() { names.push(s.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4065-09", !ndup && names.len() == 4, "四步名双射");

    // 10) 循环上限常量。
    set.add("C4065-10", REVIEW_LOOP_MAX == 3, "循环上限 3");

    // 11) 段位自查。
    set.add("C4065-11", SEG_BASE == 0x89C0, "段位 0x89C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_and_final() {
        let mut wf = PeWorkflow::new();
        let _ = wf.advance(Step::Editing);
        let _ = wf.advance(Step::Review);
        assert_eq!(wf.reject(), Ok("回编辑"));
    }
}
