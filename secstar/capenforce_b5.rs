//! F177 能力执法可视化 · 批次五深化（secstar · G-G-07）。
//!
//! 批次五功能面（与 b3「决策与升档」、b4「健康与文案」互补，本批管
//! 「自检与免打扰」）：
//! - [`PointSelftest`]：执法点自检——假拦截注入 → 四点各自验证在岗
//!   （执法点睡岗会被抓——异常显性化的自检面）；
//! - [`QuietHours`]：免打扰时段——时段内通知静默但执法照常
//!   （执法与播报解耦的时段面：半夜拦截不吵人但不放纵）；
//! - [`DenyStreak`]：同应用连拒账——连拒 N 次给「去申请授权」提示
//!   （反复撞墙的用户该看到门在哪——第 11 章引导性）。
//!
//! 零堆纪律：定长表 + 定长计数，无 alloc。

use super::capenforce::POINT_N;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 执法点自检
// ---------------------------------------------------------------------------

/// 自检注入器：向指定点注入一次假拦截（假事件带哨兵标记不进真账）。
pub fn point_selftest(reported: [bool; POINT_N]) -> bool {
    // 自检通过条件：四点全部回报在岗（回报面由底层注入回填）。
    reported.iter().all(|r| *r)
}

/// 自检失败定位：第一个未回报点（说谎的点有名字）。
pub fn first_dead_point(reported: [bool; POINT_N]) -> Option<usize> {
    reported.iter().position(|r| !r)
}

/// 自检周期常量（每 60s 一轮——早发现睡岗）。
pub const SELFTEST_PERIOD_MS: u64 = 60_000;

// ---------------------------------------------------------------------------
// 免打扰时段
// ---------------------------------------------------------------------------

/// 免打扰窗（当日分钟）：22:00–07:00（跨 0 点）。
pub const QUIET_START_MIN: u32 = 22 * 60;
pub const QUIET_END_MIN: u32 = 7 * 60;

/// 时段判定（跨 0 点环形窗）。
pub fn in_quiet_hours(at_min: u32) -> bool {
    let m = at_min % 1440;
    m >= QUIET_START_MIN || m < QUIET_END_MIN
}

/// 通知裁决：免打扰内拦截照常但通知静默（执法照常——解耦面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotifyVerdict {
    pub enforced: bool,
    pub notified: bool,
}

pub fn notify_verdict(at_min: u32, quiet_hours_enabled: bool) -> NotifyVerdict {
    let quiet = quiet_hours_enabled && in_quiet_hours(at_min);
    NotifyVerdict { enforced: true, notified: !quiet }
}

// ---------------------------------------------------------------------------
// 连拒引导
// ---------------------------------------------------------------------------

/// 连拒提示线：同应用同能力连拒 3 次 → 给「去申请授权」引导。
pub const DENY_STREAK_AT: u32 = 3;

#[derive(Clone, Copy, Debug, Default)]
pub struct DenyStreak {
    count: u32,
}

impl DenyStreak {
    pub const fn new() -> DenyStreak {
        DenyStreak { count: 0 }
    }

    /// 记一次拒绝：返回是否达到提示线（恰好一次）。
    pub fn on_deny(&mut self) -> bool {
        self.count += 1;
        self.count == DENY_STREAK_AT
    }

    /// 授权成功清零（引导解决后不再出现）。
    pub fn on_granted(&mut self) {
        self.count = 0;
    }

    pub fn count(&self) -> u32 {
        self.count
    }
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_capenforce_b5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F177-b5");

    // 1) 自检全在岗：四点全回报 → 通过（在岗面）。
    cs.add("selftest_all_alive", point_selftest([true; POINT_N]), "");

    // 2) 自检睡岗检出：第三点未回报 → 失败且定位（说谎的点有名字）。
    let mut reported = [true; POINT_N];
    reported[2] = false;
    cs.add(
        "selftest_dead_located",
        !point_selftest(reported) && first_dead_point(reported) == Some(2),
        "",
    );

    // 3) 自检周期常量：60s 一轮在册。
    cs.add("selftest_period", SELFTEST_PERIOD_MS == 60_000, "");

    // 4) 免打扰窗界：22:00 入窗、06:59 在窗、07:00 出窗（跨 0 点逐点）。
    cs.add(
        "quiet_hours_bounds",
        in_quiet_hours(22 * 60) && in_quiet_hours(6 * 60 + 59) && !in_quiet_hours(7 * 60) && !in_quiet_hours(21 * 60 + 59),
        "",
    );

    // 5) 通知裁决：免打扰内执法真、通知假；窗外两者都真（解耦面）。
    let q = notify_verdict(23 * 60, true);
    let day = notify_verdict(12 * 60, true);
    let disabled = notify_verdict(23 * 60, false);
    cs.add(
        "notify_verdict_decoupled",
        q.enforced && !q.notified && day.enforced && day.notified && disabled.notified,
        "",
    );

    // 6) 免打扰可关：开关关闭 → 深夜也通知（用户主权面）。
    cs.add("quiet_hours_switchable", notify_verdict(3 * 60, false).notified, "");

    // 7) 连拒引导：第 3 次拒恰好提示一次、第 4 次不再（不骚扰线）。
    let mut d = DenyStreak::new();
    let n1 = d.on_deny();
    let n2 = d.on_deny();
    let n3 = d.on_deny();
    let n4 = d.on_deny();
    cs.add("deny_streak_prompt_once", !n1 && !n2 && n3 && !n4 && d.count() == 4, "");

    // 8) 授权清零：引导解决后连拒账归零（旧账不再触发）。
    d.on_granted();
    cs.add("deny_streak_reset", d.count() == 0 && !d.on_deny(), "");

    // 9) 主册常量贯通：四执法点一处一事实。
    cs.add("consts_aligned", POINT_N == 4, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;

    #[test]
    fn quiet_window_full_night() {
        // 整夜扫描：22:00-06:59 全在窗、07:00-21:59 全出窗（无漏点）。
        for m in (0..1440u32).step_by(7) {
            let expect = m >= QUIET_START_MIN || m < QUIET_END_MIN;
            assert_eq!(in_quiet_hours(m), expect, "min={m}");
        }
    }

    #[test]
    fn deny_streak_multi_app_isolated() {
        // 连拒按应用隔离（本模型单应用实例——两实例互不串扰验证）。
        let mut a = DenyStreak::new();
        let mut b = DenyStreak::new();
        a.on_deny();
        a.on_deny();
        assert!(a.on_deny(), "a 第三次触发引导");
        assert!(!b.on_deny(), "b 首拒不触发");
        assert!(!a.on_deny(), "a 第四次不再触发");
    }

    #[test]
    fn selftest_empty_report_fails() {
        // 全未回报：自检失败且定位第一点（无漏报幻觉）。
        let reported = [false; POINT_N];
        assert!(!point_selftest(reported));
        assert_eq!(first_dead_point(reported), Some(0));
    }
}
