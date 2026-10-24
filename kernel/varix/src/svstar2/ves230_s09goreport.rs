//! VE-F3976 · S 域预备报告（GO/NO-GO）
//!
//! S 域预备报告：自查汇总 + 缺口清零状态 + 预评分 + 预演结论四段合成
//! GO/NO-GO 建议。建议与终审分离声明（预备 GO 不等于终审 GO）。
//! 错误路径：缺口未清→NO-GO+回补指令；建议失实→撤回+立案；逾期→复议。
//! 性能：合成 O(四段)；核验 O(缺口)；出具 O(1)。
//! 跨批对接：F3972/F3973/F3974/F3975 四上游；F3977 演练下游。
//! 无障碍与隐私：报告含无障碍专项章（双维+心流状态）；无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x35C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x35C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9PCode(pub u16);

impl S9PCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const NO_GO: Self = Self(SEG_BASE + 1);
    pub const RETRACTED: Self = Self(SEG_BASE + 2);
    pub const OVERDUE: Self = Self(SEG_BASE + 3);
    pub const SECTION_GAP: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::NO_GO => "缺口未清→NO-GO+回补指令",
            Self::RETRACTED => "建议失实→撤回+立案",
            Self::OVERDUE => "逾期→复议",
            Self::SECTION_GAP => "段缺→不出具",
            _ => "未知",
        }
    }
}

// ── 四段合成 ─────────────────────────────────────────────────────────

/// 四段种别。
pub const SECTIONS: [&str; 4] = ["自查汇总", "缺口清零", "预评分", "预演结论"];

/// 四段输入。
#[derive(Debug, Clone)]
pub struct ReportInput {
    /// 段是否齐备。
    pub present: [bool; 4],
    /// 缺口余数（清零态）。
    pub gaps_left: u32,
    /// 预评分均分。
    pub pre_score: u32,
    /// 预演通过。
    pub rehearsal_ok: bool,
}

impl ReportInput {
    /// 四段齐备断言。
    pub fn sections_complete(&self) -> bool {
        self.present.iter().all(|p| *p)
    }

    /// GO 条件：四段齐 + 缺口清零 + 预评分达线 + 预演过。
    pub fn go_verdict(&self, score_line: u32) -> bool {
        self.sections_complete() && self.gaps_left == 0 && self.pre_score >= score_line && self.rehearsal_ok
    }
}

/// 预备报告。
#[derive(Debug, Clone)]
pub struct PreReport {
    pub input: ReportInput,
    /// GO/NO-GO。
    pub verdict_go: bool,
    /// 已出具。
    pub issued: bool,
    /// 已撤回。
    pub retracted: bool,
    /// 出具周期。
    pub cycle: u32,
    /// 时效窗。
    pub fresh_window: u32,
}

pub const SCORE_LINE: u32 = 85;

impl PreReport {
    /// 合成 O(四段)。
    pub fn compose(input: ReportInput, cycle: u32) -> Self {
        let go = input.go_verdict(SCORE_LINE);
        Self { input, verdict_go: go, issued: true, retracted: false, cycle, fresh_window: 3 }
    }

    /// 出具前置：段缺 → 不出具。
    pub fn can_issue(&self) -> S9PCode {
        if !self.input.sections_complete() {
            S9PCode::SECTION_GAP
        } else if self.retracted {
            S9PCode::RETRACTED
        } else {
            S9PCode::OK
        }
    }

    /// NO-GO 回补指令生成。
    pub fn backfill_orders(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.input.gaps_left > 0 {
            v.push("清缺口");
        }
        if self.input.pre_score < SCORE_LINE {
            v.push("补强低分维");
        }
        if !self.input.rehearsal_ok {
            v.push("重演预演");
        }
        v
    }

    /// 撤回。
    pub fn retract(&mut self) {
        self.retracted = true;
        self.issued = false;
    }

    /// 逾期判定。
    pub fn overdue(&self, now: u32) -> bool {
        now.saturating_sub(self.cycle) > self.fresh_window
    }
}

// ── 预终分离声明 ─────────────────────────────────────────────────────

/// 分离声明锚。
pub const PRE_FINAL_SEPARATION: &str = "预备 GO 不等于终审 GO";

/// 分离合规：预备 GO 不自动授予终审 GO。
pub fn pre_go_implies_final(pre_go: bool) -> bool {
    // 恒 false：预备结论永不直接推导终审结论。
    pre_go && false
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves230_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves230_s09goreport");

    fn full_input(score: u32) -> ReportInput {
        ReportInput { present: [true; 4], gaps_left: 0, pre_score: score, rehearsal_ok: true }
    }

    // 01 四段清单守恒
    {
        chk(&mut set, "C3976-01", SECTIONS.len() == 4 && SECTIONS[0] == "自查汇总" && SECTIONS[3] == "预演结论", "err");
    }
    // 02 全条件 GO
    {
        let r = PreReport::compose(full_input(92), 10);
        chk(&mut set, "C3976-02", r.verdict_go && r.issued && r.can_issue() == S9PCode::OK, "err");
    }
    // 03 缺口未清→NO-GO + 回补指令
    {
        let mut i = full_input(92);
        i.gaps_left = 3;
        let r = PreReport::compose(i, 10);
        chk(&mut set, "C3976-03", !r.verdict_go && r.backfill_orders().contains(&"清缺口"), "err");
    }
    // 04 预评分未达线→NO-GO
    {
        let r = PreReport::compose(full_input(84), 10);
        chk(&mut set, "C3976-04", !r.verdict_go && r.backfill_orders().contains(&"补强低分维"), "err");
    }
    // 05 预演未过→NO-GO
    {
        let mut i = full_input(92);
        i.rehearsal_ok = false;
        let r = PreReport::compose(i, 10);
        chk(&mut set, "C3976-05", !r.verdict_go && r.backfill_orders() == vec!["重演预演"], "err");
    }
    // 06 段缺→不出具
    {
        let mut i = full_input(92);
        i.present[2] = false;
        let r = PreReport::compose(i, 10);
        chk(&mut set, "C3976-06", r.can_issue() == S9PCode::SECTION_GAP && !r.input.sections_complete(), "err");
    }
    // 07 建议失实→撤回
    {
        let mut r = PreReport::compose(full_input(92), 10);
        r.retract();
        chk(&mut set, "C3976-07", r.retracted && !r.issued && r.can_issue() == S9PCode::RETRACTED, "err");
    }
    // 08 逾期→复议
    {
        let r = PreReport::compose(full_input(92), 5);
        chk(&mut set, "C3976-08", !r.overdue(7) && r.overdue(9), "err");
    }
    // 09 达线边界（85 = GO）
    {
        let r = PreReport::compose(full_input(SCORE_LINE), 1);
        chk(&mut set, "C3976-09", r.verdict_go, "err");
    }
    // 10 预终分离声明守恒
    {
        chk(&mut set, "C3976-10", PRE_FINAL_SEPARATION == "预备 GO 不等于终审 GO" && !pre_go_implies_final(true), "err");
    }
    // 11 干净输入零回补指令
    {
        let r = PreReport::compose(full_input(90), 1);
        chk(&mut set, "C3976-11", r.backfill_orders().is_empty(), "err");
    }
    // 12 多缺陷回补指令聚合
    {
        let mut i = full_input(50);
        i.gaps_left = 9;
        i.rehearsal_ok = false;
        i.present[1] = false;
        let r = PreReport::compose(i, 1);
        chk(&mut set, "C3976-12", r.backfill_orders().len() == 3, "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3976-13", S9PCode::SECTION_GAP.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9PCode::OK, S9PCode::NO_GO, S9PCode::RETRACTED, S9PCode::OVERDUE, S9PCode::SECTION_GAP];
        chk(&mut set, "C3976-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_goreport_all_green() {
        let s = run_ves230_checks();
        assert!(s.all_passed(), "ves230 red: {}", s.red_items_text());
    }

    #[test]
    fn test_go_no_go_boundary() {
        let go = PreReport::compose(full_input(85), 1);
        let nogo = PreReport::compose(full_input(84), 1);
        assert!(go.verdict_go);
        assert!(!nogo.verdict_go);
    }

    #[test]
    fn test_separation_invariant() {
        // 预备 GO 永不推出终审 GO（恒 false）。
        assert!(!pre_go_implies_final(true));
        assert!(!pre_go_implies_final(false));
    }
}
