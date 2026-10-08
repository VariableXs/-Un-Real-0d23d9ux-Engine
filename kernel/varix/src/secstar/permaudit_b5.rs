//! F179 权限审计页 · 批次五深化（secstar · G-G-09）。
//!
//! 批次五功能面（与 b3「归并与过滤」、b4「异常与摘要」互补，本批管
//! 「快捷视图与影响预览」）：
//! - [`QuickFilter`]：快速过滤预设——今天/本周/仅越权三预设一键切换
//!   （第 11 章可发现性：专家有捷径，新手有默认）；
//! - [`RevokeImpact`]：收回影响预览——收回某能力前先列出受影响应用
//!   与功能数（破坏性操作先看清单——第 12 章信任面）；
//! - [`PermissionCard`]：权限卡摘要行——应用/能力/授予日/状态四字段
//!   （F038 权限卡的审计侧镜像：一处一事实）。
//!
//! 零堆纪律：定长过滤槽 + 定长预览表，无 alloc。

use super::permaudit::{CapKind, Source};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 快速过滤预设
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickFilter {
    Today,
    ThisWeek,
    OverreachOnly,
}

/// 预设 → 时间窗（日）与越权开关（None = 不限）。
pub fn preset_window(q: QuickFilter) -> (u32, bool) {
    match q {
        QuickFilter::Today => (1, false),
        QuickFilter::ThisWeek => (7, false),
        QuickFilter::OverreachOnly => (90, true),
    }
}

/// 预设命中判定：事件日 + 是否越权 → 是否被该预设命中。
pub fn preset_matches(q: QuickFilter, event_day: u32, today: u32, overreach: bool) -> bool {
    let (window_days, only_overreach) = preset_window(q);
    if only_overreach && !overreach {
        return false;
    }
    today.saturating_sub(event_day) < window_days
}

// ---------------------------------------------------------------------------
// 收回影响预览
// ---------------------------------------------------------------------------

/// 受影响应用行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImpactRow {
    pub app_id: u32,
    pub app_name: &'static str,
    /// 该应用基于此能力的功能数（页面数字来源）。
    pub features: u32,
}

/// 影响预览表。
pub struct RevokeImpact {
    rows: [Option<ImpactRow>; 8],
    pub n: usize,
}

impl RevokeImpact {
    pub const fn new() -> RevokeImpact {
        RevokeImpact { rows: [const { None }; 8], n: 0 }
    }

    pub fn add(&mut self, app_id: u32, name: &'static str, features: u32) -> bool {
        if self.n >= 8 || self.rows[..self.n].iter().flatten().any(|r| r.app_id == app_id) {
            return false;
        }
        self.rows[self.n] = Some(ImpactRow { app_id, app_name: name, features });
        self.n += 1;
        true
    }

    /// 受影响功能总数（确认文案「将影响 N 个功能」的数字来源）。
    pub fn total_features(&self) -> u32 {
        self.rows[..self.n].iter().flatten().map(|r| r.features).sum()
    }

    /// 空预览 = 收回无影响（确认文案降级为「无应用受影响」）。
    pub fn empty(&self) -> bool {
        self.n == 0
    }
}

// ---------------------------------------------------------------------------
// 权限卡摘要行
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermissionCard {
    pub app_id: u32,
    pub cap: CapKind,
    pub granted_day: u32,
    pub active: bool,
}

/// 摘要行过滤：活跃卡才有「正在使用」标记（卸载/收回的卡如实变灰）。
pub fn card_active_line(cards: &[PermissionCard], app_id: u32, cap: CapKind) -> Option<PermissionCard> {
    cards.iter().copied().find(|c| c.app_id == app_id && c.cap == cap)
}

/// 三源账目对账：授权源事件数 = 活跃卡数 + 已失效卡数（守恒）。
pub fn grant_conservation(grants: usize, active: usize, inactive: usize) -> bool {
    grants == active + inactive
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_permaudit_b5_checks() -> CheckSet {
    use CapKind as CK;
    let mut cs = CheckSet::new("F179-b5");

    // 1) 预设窗口：今天 1 日 / 本周 7 日 / 越权 90 日（三预设在册）。
    cs.add(
        "preset_windows",
        preset_window(QuickFilter::Today) == (1, false)
            && preset_window(QuickFilter::ThisWeek) == (7, false)
            && preset_window(QuickFilter::OverreachOnly) == (90, true),
        "",
    );

    // 2) 预设命中：今天预设不吃昨天的卡、越权预设滤掉正常事件（三面）。
    let today_hit = preset_matches(QuickFilter::Today, 100, 100, false);
    let today_miss = !preset_matches(QuickFilter::Today, 99, 100, false);
    let overreach_hit = preset_matches(QuickFilter::OverreachOnly, 50, 100, true);
    let overreach_miss = !preset_matches(QuickFilter::OverreachOnly, 50, 100, false);
    let week_hit = preset_matches(QuickFilter::ThisWeek, 95, 100, false);
    cs.add(
        "preset_matching",
        today_hit && today_miss && overreach_hit && overreach_miss && week_hit,
        "",
    );

    // 3) 影响预览：登记/查重/功能总数（确认文案数字来源）。
    let mut imp = RevokeImpact::new();
    let a = imp.add(1, "相机应用", 3);
    let dup = imp.add(1, "重复", 9);
    let b = imp.add(2, "会议软件", 2);
    cs.add(
        "impact_rows",
        a && !dup && b && imp.total_features() == 5 && !imp.empty(),
        "",
    );

    // 4) 空预览：无受影响应用 → empty（确认文案降级路径）。
    cs.add("impact_empty", RevokeImpact::new().empty(), "");

    // 5) 权限卡查询：命中返回四字段、未授权组合 None（键查两面）。
    let cards = [
        PermissionCard { app_id: 7, cap: CK::Camera, granted_day: 10, active: true },
        PermissionCard { app_id: 7, cap: CK::Microphone, granted_day: 12, active: false },
    ];
    let hit = card_active_line(&cards, 7, CK::Camera).unwrap();
    let miss = card_active_line(&cards, 7, CK::Location);
    cs.add(
        "card_lookup",
        hit.active && hit.granted_day == 10 && miss.is_none(),
        "",
    );

    // 6) 授权守恒：授权数 = 活跃 + 失效（账目守恒两面）。
    cs.add(
        "grant_conservation",
        grant_conservation(5, 3, 2) && !grant_conservation(5, 3, 1),
        "",
    );

    // 7) 源枚举贯通：三源 ord 序（主层一处一事实复用）。
    cs.add(
        "source_ord_reuse",
        Source::Grant.ord() < Source::Intercept.ord() && Source::Intercept.ord() < Source::Network.ord(),
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;
    use CapKind as CK;

    #[test]
    fn impact_cap_eight() {
        // 8 应用满容拒（预览表容量边界）。
        let mut imp = RevokeImpact::new();
        for i in 0..8u32 {
            assert!(imp.add(i, "app", 1));
        }
        assert!(!imp.add(99, "over", 1));
        assert_eq!(imp.total_features(), 8);
    }

    #[test]
    fn preset_boundary_day() {
        // 窗口边界：今天预设对 event_day=today-0 命中、-1 不命中
        // （saturating_sub < window 语义的边界逐点）。
        assert!(preset_matches(QuickFilter::Today, 7, 7, false));
        assert!(!preset_matches(QuickFilter::Today, 6, 7, false));
        // 本周 7 日窗：第 6 天命中、第 7 天出窗。
        assert!(preset_matches(QuickFilter::ThisWeek, 1, 7, false));
        assert!(!preset_matches(QuickFilter::ThisWeek, 0, 7, false));
    }

    #[test]
    fn card_lifecycle_lines() {
        // 卡生命周期：收回后 active=false 但行仍在（历史不抹）。
        let cards = [PermissionCard { app_id: 3, cap: CK::Location, granted_day: 1, active: false }];
        let line = card_active_line(&cards, 3, CK::Location).unwrap();
        assert!(!line.active, "收回后行保留但标记失效");
    }
}
