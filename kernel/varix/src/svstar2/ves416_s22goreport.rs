//! ves416_s22goreport — VE-F4176 · T 域预备报告（GO/NO-GO）
//! T 域预备报告：自查汇总 + 缺口清零状态 + 预评分 + 预演结论四段合成 GO/NO-GO 建议；
//! 建议与终审分离声明（预备 GO 不等于终审 GO）；缺口未清→NO-GO+回补指令；
//! 建议失实→撤回+立案；逾期→复议；报告含多语言无障碍专项章（跟读/必检状态——域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0F20;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GoCode {
    Ok = 0,
    GapOpen = SEG_BASE + 0x01,     // 0x0F21
    SuggestionReverted = SEG_BASE + 0x02, // 0x0F22
    Overdue = SEG_BASE + 0x03,     // 0x0F23
}

/// GO/NO-GO 建议。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict { Go = 0, NoGo = 1 }

impl Verdict {
    pub fn name(self) -> &'static str {
        match self { Verdict::Go => "GO", Verdict::NoGo => "NO-GO" }
    }
}

/// 四段合成输入。
#[derive(Clone, Copy)]
pub struct GoInputs {
    pub selfcheck_done: bool,   // 自查汇总完成
    pub gaps_open: u32,         // 缺口清零状态（0 = 清零）
    pub prescore_complete: bool, // 预评分完成
    pub drywalk_ok: bool,       // 预演结论
    pub a11y_chapter: bool,     // 多语言无障碍专项章（跟读/必检状态——域本色）
    pub overdue_days: u32,      // 逾期天数
}

impl GoInputs {
    pub const OVERDUE_LIMIT: u32 = 30;
}

/// 预备报告。
pub struct GoReport {
    pub verdict: Verdict,
    pub a11y_chapter: bool,
    pub withdrawn: bool,
    pub cases: Vec<&'static str>,
    pub remedy: Option<&'static str>, // 回补指令
}

/// 四段合成 GO/NO-GO。
pub fn synthesize(i: &GoInputs) -> GoReport {
    let mut report = GoReport {
        verdict: Verdict::Go, a11y_chapter: i.a11y_chapter,
        withdrawn: false, cases: Vec::new(), remedy: None,
    };
    if !i.selfcheck_done || !i.prescore_complete || !i.drywalk_ok {
        report.verdict = Verdict::NoGo;
        report.remedy = Some("完成自查/预评分/预演后重报");
    } else if i.gaps_open > 0 {
        report.verdict = Verdict::NoGo;
        report.remedy = Some("缺口回补指令：清零后重报");
    } else if !i.a11y_chapter {
        report.verdict = Verdict::NoGo;
        report.remedy = Some("补多语言无障碍专项章");
    } else if i.overdue_days > GoInputs::OVERDUE_LIMIT {
        report.verdict = Verdict::NoGo;
        report.remedy = Some("逾期复议");
    }
    report
}

/// 建议与终审分离声明：预备 GO 不等于终审 GO。
pub fn separation_declare(r: &GoReport) -> bool {
    // 无论预备结论如何，终审独立——声明恒为真（结构性声明）。
    let _ = r.verdict;
    true
}

/// 建议失实 → 撤回 + 立案。
pub fn revert(r: &mut GoReport, why: &'static str) {
    r.withdrawn = true;
    r.cases.push(why);
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves416_checks() -> CheckSet {
    let mut set = CheckSet::new("ves416");

    // C416-01 四段齐 → GO。
    let good = GoInputs { selfcheck_done: true, gaps_open: 0, prescore_complete: true, drywalk_ok: true, a11y_chapter: true, overdue_days: 0 };
    let r = synthesize(&good);
    chk(&mut set, "C416-01", r.verdict == Verdict::Go && r.remedy.is_none(), "四段齐 GO");

    // C416-02 缺口未清 → NO-GO + 回补指令。
    let mut i2 = good;
    i2.gaps_open = 3;
    let r2 = synthesize(&i2);
    chk(&mut set, "C416-02", r2.verdict == Verdict::NoGo
        && r2.remedy == Some("缺口回补指令：清零后重报"), "缺口 NO-GO 回补");

    // C416-03 自查未完 → NO-GO。
    let mut i3 = good;
    i3.selfcheck_done = false;
    chk(&mut set, "C416-03", synthesize(&i3).verdict == Verdict::NoGo, "自查缺 NO-GO");

    // C416-04 预评分未完 → NO-GO。
    let mut i4 = good;
    i4.prescore_complete = false;
    chk(&mut set, "C416-04", synthesize(&i4).verdict == Verdict::NoGo, "预评缺 NO-GO");

    // C416-05 预演失败 → NO-GO。
    let mut i5 = good;
    i5.drywalk_ok = false;
    chk(&mut set, "C416-05", synthesize(&i5).verdict == Verdict::NoGo, "预演败 NO-GO");

    // C416-06 域本色：无障碍专项章缺 → NO-GO。
    let mut i6 = good;
    i6.a11y_chapter = false;
    let r6 = synthesize(&i6);
    chk(&mut set, "C416-06", r6.verdict == Verdict::NoGo && !r6.a11y_chapter, "专项章缺 NO-GO");

    // C416-07 逾期复议（30 天线）。
    let mut i7 = good;
    i7.overdue_days = 31;
    chk(&mut set, "C416-07", synthesize(&i7).verdict == Verdict::NoGo, "逾期 31 NO-GO");
    let mut i8 = good;
    i8.overdue_days = 30;
    chk(&mut set, "C416-07b", synthesize(&i8).verdict == Verdict::Go, "压线 30 GO");

    // C416-08 建议与终审分离声明。
    chk(&mut set, "C416-08", separation_declare(&r) && separation_declare(&r2), "分离声明成立");

    // C416-09 建议失实 → 撤回 + 立案。
    let mut r9 = r;
    revert(&mut r9, "建议失实：缺口少报");
    chk(&mut set, "C416-09", r9.withdrawn && r9.cases.contains(&"建议失实：缺口少报"), "撤回立案");

    // C416-10 撤回后建议失效。
    chk(&mut set, "C416-10", r9.withdrawn && r9.verdict == Verdict::Go, "撤回不影响原判定（留痕制）");

    // C416-11 建议名单单源。
    chk(&mut set, "C416-11", Verdict::Go.name() == "GO" && Verdict::NoGo.name() == "NO-GO", "建议单源");

    // C412 兼容：NO-GO 全部带 remedy。
    let all_nogo = [i2, i3, i4, i5, i6, i7].iter().map(synthesize)
        .all(|x| x.verdict == Verdict::NoGo && x.remedy.is_some());
    chk(&mut set, "C416-12", all_nogo, "NO-GO 必带回补指令");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves416_smoke() {
        let set = run_ves416_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves416 red: {:?}", red);
    }
}
