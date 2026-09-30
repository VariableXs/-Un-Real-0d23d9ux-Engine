//! 深化层二 · F144 「Crafted for VARIX」徽标计划（2026-09-26 深化批次二）。
//!
//! 补深主册【数据与存储】授予册公开面与【设计细节】徽标三形态展示
//! 规范（主册 G-D-19）：审核四查流水线（逐查子状态+汇总裁决）、
//! 复检日历（有效期随大版本+宽限一季）、授予册季报聚合接口、展示
//! 几何规范校验、伪造检测上报链、申诉通道（一级终级）。

use crate::checks::CheckSet;
use crate::stareco::craftbadge::{BadgeOffice, ReviewInput, BADGE_FORMS, GRACE_DAYS, REVIEW_SLA_DAYS};

// ---------------------------------------------------------------------------
// 审核四查流水线：逐查子状态 + 汇总裁决（缺一拒授——逐查可溯）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheckOutcome {
    Pass,
    Fail,
}

/// 四查逐项结果卡。
pub struct FourCheckCard {
    pub charter: CheckOutcome,
    pub a11y: CheckOutcome,
    pub copy: CheckOutcome,
    pub privacy: CheckOutcome,
}

impl FourCheckCard {
    pub fn failed_checks(&self) -> alloc::vec::Vec<&'static str> {
        let mut out = alloc::vec::Vec::new();
        if self.charter == CheckOutcome::Fail {
            out.push("charter");
        }
        if self.a11y == CheckOutcome::Fail {
            out.push("a11y");
        }
        if self.copy == CheckOutcome::Fail {
            out.push("copy");
        }
        if self.privacy == CheckOutcome::Fail {
            out.push("privacy");
        }
        out
    }

    pub fn all_pass(&self) -> bool {
        self.failed_checks().is_empty()
    }

    /// 与基础层 ReviewInput 对齐（同一结论两条路径——一致性互证）。
    pub fn to_review_input(&self) -> ReviewInput {
        ReviewInput {
            charter_compliant: self.charter == CheckOutcome::Pass,
            a11y_green: self.a11y == CheckOutcome::Pass,
            copy_three_parts: self.copy == CheckOutcome::Pass,
            privacy_declared: self.privacy == CheckOutcome::Pass,
        }
    }
}

// ---------------------------------------------------------------------------
// 复检日历：徽标有效期随系统大版本 + 宽限一季复检
// ---------------------------------------------------------------------------

pub struct RecheckCalendar {
    /// (应用, 授予日, 应复检日)。
    entries: alloc::vec::Vec<(&'static str, u32, u32)>,
}

impl RecheckCalendar {
    pub fn new() -> RecheckCalendar {
        RecheckCalendar { entries: alloc::vec::Vec::new() }
    }

    /// 注册：授予后一个宽限季（90 天）为应复检日。
    pub fn register(&mut self, app: &'static str, granted_day: u32) -> Result<(), &'static str> {
        if app.is_empty() {
            return Err("应用名必填");
        }
        if self.entries.iter().any(|(a, _, _)| *a == app) {
            return Err("应用已在复检历");
        }
        self.entries.push((app, granted_day, granted_day + GRACE_DAYS));
        Ok(())
    }

    /// 应复检清单（今日 ≥ 应复检日的应用——宽限未复检即检出）。
    pub fn due(&self, today: u32) -> alloc::vec::Vec<&'static str> {
        self.entries.iter().filter(|(_, _, d)| today >= *d).map(|(a, _, _)| *a).collect()
    }

    /// 复检完成：顺延一个宽限季。
    pub fn done(&mut self, app: &str, today: u32) -> Result<(), &'static str> {
        let e = self.entries.iter_mut().find(|(a, _, _)| *a == app).ok_or("应用不在复检历")?;
        e.1 = today;
        e.2 = today + GRACE_DAYS;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

// ---------------------------------------------------------------------------
// 展示几何规范：三形态 × 最小展示尺寸 × 底色间距（F143 同款纪律）
// ---------------------------------------------------------------------------

/// 三形态的最小展示宽度（px）：横版 96 / 方版 32 / 单色 48。
pub fn min_display_width(form: &str) -> Result<u16, &'static str> {
    match form {
        "horizontal" => Ok(96),
        "square" => Ok(32),
        "mono" => Ok(48),
        _ => Err("未知形态：三形态之外不设展示规范"),
    }
}

/// 展示合法性：形态合法 + 尺寸达标。
pub fn display_ok(form: &str, width: u16) -> bool {
    min_display_width(form).map_or(false, |min| width >= min)
}

/// 全形态规范自洽：BADGE_FORMS 表里的每个形态都有规范值。
pub fn forms_all_specified() -> bool {
    BADGE_FORMS.iter().all(|f| min_display_width(f).is_ok())
}

// ---------------------------------------------------------------------------
// 伪造检测上报链：验真失败 → 撤销令（F142 联动）
// ---------------------------------------------------------------------------

pub struct ForgeryCase {
    pub app: &'static str,
    /// 伪造签名指纹。
    pub forged_fp: u64,
    /// 是否已报 F142 安全通道。
    pub reported: bool,
}

/// 伪造处置：验真失败 → 拦截计数 + 立即上报 F142（零静默丢弃）。
pub fn handle_forgery(office: &mut BadgeOffice, case: &mut ForgeryCase) -> bool {
    if office.verify(case.app, case.forged_fp) {
        return false; // 签名验真通过——不是伪造
    }
    let _ = office.reject_forgery(case.app, case.forged_fp);
    case.reported = true;
    true
}

// ---------------------------------------------------------------------------
// 申诉通道：撤销 → 申诉 → 复检结论（一级终级）
// ---------------------------------------------------------------------------

pub struct AppealCase {
    pub app: &'static str,
    /// 申诉日（必须在撤销后 SLA 内——14 天）。
    pub filed_day: u32,
    pub resolved: bool,
}

pub const APPEAL_WINDOW_DAYS: u32 = REVIEW_SLA_DAYS;

/// 申诉受理：撤销日 + APPEAL_WINDOW_DAYS 内有效；一级终级（每案一诉）。
pub fn appeal_admissible(revoke_day: u32, c: &AppealCase) -> Result<(), &'static str> {
    if c.resolved {
        return Err("已裁决：一级终级，不重复申诉");
    }
    if c.filed_day.saturating_sub(revoke_day) > APPEAL_WINDOW_DAYS {
        return Err("申诉期已过（14 天）：下次复检时重新申请");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F144E_TAG: &str = "stareco-F144-deep2";

pub fn run_f144_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F144E_TAG);

    // 四查流水线
    let full = FourCheckCard {
        charter: CheckOutcome::Pass,
        a11y: CheckOutcome::Pass,
        copy: CheckOutcome::Pass,
        privacy: CheckOutcome::Pass,
    };
    set.add("f144e four pass", full.all_pass() && full.failed_checks().is_empty(), "四查全绿");
    let partial = FourCheckCard {
        charter: CheckOutcome::Pass,
        a11y: CheckOutcome::Fail,
        copy: CheckOutcome::Pass,
        privacy: CheckOutcome::Fail,
    };
    set.add(
        "f144e failed list",
        partial.failed_checks() == alloc::vec!["a11y", "privacy"],
        "红项逐一点名",
    );
    let input = partial.to_review_input();
    set.add(
        "f144e cross agree",
        input.four_checks_pass() == partial.all_pass(),
        "与基础层四查结论互证",
    );

    // 复检日历
    let mut cal = RecheckCalendar::new();
    let _ = cal.register("app-a", 100);
    set.add("f144e not due", cal.due(189).is_empty(), "宽限期内不复检");
    set.add("f144e due", cal.due(190) == alloc::vec!["app-a"], "期满点名");
    let _ = cal.done("app-a", 195);
    set.add("f144e done defers", cal.due(280).is_empty(), "复检后顺延");
    set.add("f144e cal dup", cal.register("app-a", 1).is_err(), "重复注册拒绝");

    // 展示规范
    set.add("f144e forms specified", forms_all_specified(), "三形态全有规范");
    set.add("f144e display ok", display_ok("square", 32), "方版 32px 达标");
    set.add("f144e display small", !display_ok("horizontal", 95), "横版 95px 差 1px");
    set.add("f144e display unknown", !display_ok("sticker", 999), "未知形态拒绝");

    // 伪造上报
    let mut office = BadgeOffice::new();
    let review = ReviewInput { charter_compliant: true, a11y_green: true, copy_three_parts: true, privacy_declared: true };
    let grant_id = office.grant(100, "app-x", &review).expect("grant");
    set.add("f144e grant ok", office.len() == 1, "授予成功（演练 1/3）");
    let mut case = ForgeryCase { app: "app-x", forged_fp: 0xDEAD, reported: false };
    set.add("f144e forgery caught", handle_forgery(&mut office, &mut case), "伪造指纹拦截");
    set.add("f144e forgery reported", case.reported, "上报链触发");
    let _ = grant_id; // 授予编号供徽标文件引用（本自检不持有私钥面）

    // 申诉
    let appeal = AppealCase { app: "app-y", filed_day: 100 + 10, resolved: false };
    set.add("f144e appeal in window", appeal_admissible(100, &appeal).is_ok(), "10 天内申诉受理");
    let late = AppealCase { app: "app-y", filed_day: 100 + 15, resolved: false };
    set.add("f144e appeal late", appeal_admissible(100, &late).is_err(), "超 14 天拒收");
    let twice = AppealCase { app: "app-y", filed_day: 105, resolved: true };
    set.add("f144e appeal once", appeal_admissible(100, &twice).is_err(), "一级终级");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn recheck_full_cycle() {
        let mut c = RecheckCalendar::new();
        for (app, day) in [("a", 1u32), ("b", 50)] {
            let _ = c.register(app, day);
        }
        assert_eq!(c.due(91), alloc::vec!["a"]);
        let _ = c.done("a", 95);
        assert!(c.due(95).is_empty());
        assert_eq!(c.due(145), alloc::vec!["b"]);
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn review_input_roundtrip() {
        let fail_all = FourCheckCard {
            charter: CheckOutcome::Fail,
            a11y: CheckOutcome::Fail,
            copy: CheckOutcome::Fail,
            privacy: CheckOutcome::Fail,
        };
        assert_eq!(fail_all.failed_checks().len(), 4);
        assert!(!fail_all.to_review_input().four_checks_pass());
    }
}
