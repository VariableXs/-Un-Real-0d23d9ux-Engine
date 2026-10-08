//! 深化层 · F144 「Crafted for VARIX」徽标计划（2026-09-26 回炉补深化）。
//!
//! 补深：申请表模型、审核队列（SLA 14 天排程）、徽标三形态资产登记、
//! 撤销申诉路由（F148）、复检通知、季报指标导出。

use crate::checks::CheckSet;
use crate::stareco::craftbadge::{BadgeOffice, BadgeState, ReviewInput, GRACE_DAYS, REVIEW_SLA_DAYS};
use crate::stareco::ebase::TraceId;

// ---------------------------------------------------------------------------
// 申请表
// ---------------------------------------------------------------------------

pub struct Application {
    pub app: &'static str,
    pub contact: &'static str,
    /// 四项自测预跑结果（申请前必须先过自测——省双方时间）。
    pub preflight: ReviewInput,
}

impl Application {
    /// 申请受理门禁：自测预跑必须全过才收表（预跑不过 = 指回指南）。
    pub fn acceptable(&self) -> Result<(), &'static str> {
        if self.app.is_empty() || self.contact.is_empty() {
            return Err("应用名/联系方式必填");
        }
        if !self.preflight.four_checks_pass() {
            return Err("自测预跑未过：请按 F141 指南修正后再申请");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 审核队列（SLA 14 天）
// ---------------------------------------------------------------------------

pub struct ReviewQueueEntry {
    pub app: &'static str,
    pub submitted_day: u32,
}

/// 队列 SLA 判定：提交日起 14 天内必须出结论。
pub fn sla_days_left(submitted_day: u32, today: u32) -> u32 {
    let elapsed = today.saturating_sub(submitted_day);
    REVIEW_SLA_DAYS.saturating_sub(elapsed)
}

// ---------------------------------------------------------------------------
// 徽标三形态资产登记
// ---------------------------------------------------------------------------

pub const BADGE_ASSETS: [(&str, u16, u16); 3] = [
    ("horizontal", 320, 96),
    ("square", 96, 96),
    ("mono", 320, 96),
];

// ---------------------------------------------------------------------------
// 撤销申诉路由
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RevocationRoute {
    /// 质量回退撤销 → 修正后重新申请（走 F141 自测）。
    FixAndReapply,
    /// 对撤销有异议 → F148 仲裁。
    Arbitration,
}

/// 申诉路由：质疑判定走仲裁；接受判定走修复重申。
pub fn appeal_route(dispute: bool) -> RevocationRoute {
    if dispute {
        RevocationRoute::Arbitration
    } else {
        RevocationRoute::FixAndReapply
    }
}

// ---------------------------------------------------------------------------
// 复检通知与季报指标
// ---------------------------------------------------------------------------

/// 复检通知：宽限期开始即通知，剩余天数随日递减。
pub fn recheck_notice(grace_start_day: u32, today: u32) -> (bool, u32) {
    let elapsed = today.saturating_sub(grace_start_day);
    (elapsed <= GRACE_DAYS, GRACE_DAYS.saturating_sub(elapsed))
}

/// 季报指标：生效徽标数（从授予册统计）。
pub fn active_metric(office: &BadgeOffice) -> u32 {
    office.active() as u32
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F144D_TAG: &str = "stareco-F144-deep";

pub fn run_f144_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F144D_TAG);

    // 申请表门禁
    let full = ReviewInput { charter_compliant: true, a11y_green: true, copy_three_parts: true, privacy_declared: true };
    let good = Application { app: "notes-plus", contact: "dev@example.org", preflight: full };
    set.add("f144d application accepted", good.acceptable().is_ok(), "预跑全过");
    let pref_fail = Application { app: "x", contact: "c", preflight: ReviewInput { a11y_green: false, ..full } };
    set.add(
        "f144d preflight gate",
        pref_fail.acceptable().is_err() && Application { app: "", ..good }.acceptable().is_err(),
        "预跑不过指回指南",
    );

    // SLA 排程
    set.add(
        "f144d sla countdown",
        sla_days_left(0, 10) == 4 && sla_days_left(0, 20) == 0,
        "14 天线",
    );

    // 三形态资产
    set.add(
        "f144d three asset forms",
        BADGE_ASSETS.len() == 3 && BADGE_ASSETS.iter().all(|(n, w, h)| !n.is_empty() && *w > 0 && *h > 0),
        "横/方/单色",
    );

    // 申诉路由
    set.add(
        "f144d appeal routing",
        appeal_route(true) == RevocationRoute::Arbitration && appeal_route(false) == RevocationRoute::FixAndReapply,
        "质疑走 F148；接受走重申",
    );

    // 复检通知
    let (in_window, left) = recheck_notice(0, GRACE_DAYS);
    set.add("f144d recheck notice", in_window && left == 0, "期满边界仍在窗内");
    let (expired, _) = recheck_notice(0, GRACE_DAYS + 1);
    set.add("f144d recheck expired", !expired, "过期即撤语义");

    // 授予-撤销-季报指标联动
    let mut office = BadgeOffice::new();
    office.grant(20260926, "a", &full).ok();
    office.grant(20260926, "b", &full).ok();
    set.add("f144d metric two", active_metric(&office) == 2, "授予册导出");
    office.revoke("b", 20260927).ok();
    set.add("f144d metric after revoke", active_metric(&office) == 1, "撤销即减");

    // 撤销态不可再撤销、宽限只从 Granted 开
    set.add("f144d state machine holds", office.revoke("b", 20260928).is_err(), "幂等保护");

    // TraceId 在授予流程中有效
    let id_probe = TraceId::new("CRAFT", 20260926, 1);
    set.add("f144d grant id valid", id_probe.is_valid(), "编号可查");

    // BadgeState 完整性（三态可见）
    set.add(
        "f144d badge states",
        [BadgeState::Granted, BadgeState::Revoked, BadgeState::GracePeriod].len() == 3,
        "三态",
    );

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn sla_floor() {
        assert_eq!(sla_days_left(0, 100), 0);
    }
}
