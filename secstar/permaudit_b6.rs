//! F179 权限审计页 · 批次六深化（secstar · G-G-09）。
//!
//! 批次六功能面（达成率 94%——小批收尾：时间线分页与活跃度）：
//! - [`TimelinePaging`]：时间线分页——90 天账按页浏览（一页 7 天，
//!   翻页有界不越 90 天账面）；
//! - [`AppActivity`]：应用活跃度——近 N 日事件数分档（活跃/低频/
//!   沉睡三档——权限收缩建议的排序依据）。
//!
//! 零堆纪律：状态字段，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 时间线分页
// ---------------------------------------------------------------------------

/// 每页天数。
pub const PAGE_DAYS: usize = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelinePaging {
    pub page: usize,
    pub total_days: usize, // ≤ 90
}

impl TimelinePaging {
    pub fn new(total_days: usize) -> TimelinePaging {
        TimelinePaging { page: 0, total_days: total_days.min(90) }
    }

    pub fn pages(&self) -> usize {
        (self.total_days + PAGE_DAYS - 1) / PAGE_DAYS.max(1)
    }

    /// 下页：有界（末页不动——到底有边界感）。
    pub fn next(&mut self) -> bool {
        if self.page + 1 < self.pages() {
            self.page += 1;
            true
        } else {
            false
        }
    }

    pub fn prev(&mut self) -> bool {
        if self.page > 0 {
            self.page -= 1;
            true
        } else {
            false
        }
    }

    /// 本页覆盖的日区间 [start, end)。
    pub fn day_range(&self) -> (usize, usize) {
        let start = self.page * PAGE_DAYS;
        let end = (start + PAGE_DAYS).min(self.total_days);
        (start, end)
    }
}

// ---------------------------------------------------------------------------
// 应用活跃度
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// 近 7 日有事件。
    Active,
    /// 近 30 日有事件（7 日内无）。
    LowFreq,
    /// 30 日无事件（沉睡——权限收缩首候选）。
    Dormant,
}

/// 活跃度判定：距最近事件的天数 → 三档。
pub fn activity_of(days_since_last: u32) -> Activity {
    if days_since_last < 7 {
        Activity::Active
    } else if days_since_last < 30 {
        Activity::LowFreq
    } else {
        Activity::Dormant
    }
}

/// 活跃度排序权重（沉睡最先出列——收缩建议排序依据）。
pub fn shrink_weight(a: Activity) -> u8 {
    match a {
        Activity::Dormant => 2,
        Activity::LowFreq => 1,
        Activity::Active => 0,
    }
}

/// 沉睡优先排序（稳定插入排序——同档保持原序）。
pub fn sort_by_shrink(items: &mut [(u32, Activity)]) {
    for i in 1..items.len() {
        let mut j = i;
        while j > 0 && shrink_weight(items[j].1) > shrink_weight(items[j - 1].1) {
            items.swap(j, j - 1);
            j -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// 批次六自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_permaudit_b6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F179-b6");

    // 1) 分页算术：90 天 → 13 页（12×7+6）。
    let mut p = TimelinePaging::new(90);
    cs.add("paging_pages", p.pages() == 13, "");

    // 2) 翻页有界：13 次下页只成功 12 次（到底有边界感）。
    let mut ok = 0;
    for _ in 0..13 {
        if p.next() {
            ok += 1;
        }
    }
    cs.add("paging_next_bounded", ok == 12 && p.page == 12, "");

    // 3) 页区间：第 12 页（末页）覆盖 [84, 90)（尾页短页）。
    let (s, e) = p.day_range();
    cs.add("paging_last_range", s == 84 && e == 90, "");

    // 4) 上页有界：首页再上 = 无效（顶有边界感）。
    let mut p2 = TimelinePaging::new(90);
    let up_at_top = !p2.prev();
    p2.next();
    let up_ok = p2.prev();
    cs.add("paging_prev_bounded", up_at_top && up_ok && p2.page == 0, "");

    // 5) 总天数钳制：请求 200 天账 → 钳 90（账面不越保留期）。
    cs.add("paging_total_clamped", TimelinePaging::new(200).total_days == 90, "");

    // 6) 活跃度三档：3 日活跃 / 15 日低频 / 45 日沉睡（三档逐点）。
    cs.add(
        "activity_tiers",
        activity_of(3) == Activity::Active && activity_of(15) == Activity::LowFreq && activity_of(45) == Activity::Dormant,
        "",
    );

    // 7) 档位边界：6/7 与 29/30 邻域逐点（边界不糊）。
    cs.add(
        "activity_edges",
        activity_of(6) == Activity::Active
            && activity_of(7) == Activity::LowFreq
            && activity_of(29) == Activity::LowFreq
            && activity_of(30) == Activity::Dormant,
        "",
    );

    // 8) 沉睡优先排序：混排 → 沉睡在前活跃在后（收缩建议排序面）。
    let mut items = [(1u32, Activity::Active), (2, Activity::Dormant), (3, Activity::LowFreq), (4, Activity::Dormant)];
    sort_by_shrink(&mut items);
    cs.add(
        "shrink_sort",
        items[0].0 == 2 && items[1].0 == 4 && items[2].0 == 3 && items[3].0 == 1,
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次六）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b6 {
    use super::*;

    #[test]
    fn paging_small_account() {
        // 小账：3 天 → 1 页、下页无效（尾页短账）。
        let mut p = TimelinePaging::new(3);
        assert_eq!(p.pages(), 1);
        assert!(!p.next());
        assert_eq!(p.day_range(), (0, 3));
    }

    #[test]
    fn activity_zero_days_active() {
        // 今天有事件 → 活跃（0 天距today）。
        assert_eq!(activity_of(0), Activity::Active);
    }
}
