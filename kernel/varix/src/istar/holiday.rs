//! F560 节假日标注 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：红/灰双色规则；悬停名称；离线数据内置；更新通道；
//! 两年覆盖。
//!
//! **设计要点（主册）**：
//! - 日历飞出（F078）与时钟悬停（F549）的节假日标注：法定节假日红色标注
//!   + 名称悬停（「10-01 国庆节」）、调休补班灰色标注；
//! - 离线内置当年与次年数据（U 盘纪律）；数据更新随系统更新（F122）走；
//! - 标注克制（只有法定级，不塞营销节日）。
//!
//! 内置数据：2026 与 2027 两年法定节假日/调休表（国务院办公抽数量口径：
//! 每年 7 个法定假 + 对应调休补班日——内置为静态表，更新通道由 F122
//! 差异表替换整表）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 内置离线数据（两年覆盖——离线判据的唯一源）
// ---------------------------------------------------------------------------

/// 标注类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayMark {
    /// 法定假日（红）。
    Holiday,
    /// 调休补班（灰）。
    Workday,
}

/// 一条内置标注（(年, 月, 日) → 类型 + 名称）。
pub struct Entry {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub mark: DayMark,
    pub name: &'static str,
}

/// 2026 年法定节假日（元旦/春节/清明/劳动/端午/中秋/国庆）与调休补班。
pub const YEAR_2026: &[Entry] = &[
    Entry { year: 2026, month: 1, day: 1, mark: DayMark::Holiday, name: "元旦" },
    Entry { year: 2026, month: 1, day: 2, mark: DayMark::Holiday, name: "元旦" },
    Entry { year: 2026, month: 1, day: 3, mark: DayMark::Holiday, name: "元旦" },
    Entry { year: 2026, month: 2, day: 15, mark: DayMark::Workday, name: "春节调休补班" },
    Entry { year: 2026, month: 2, day: 16, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2026, month: 2, day: 17, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2026, month: 2, day: 18, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2026, month: 2, day: 19, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2026, month: 2, day: 20, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2026, month: 2, day: 21, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2026, month: 2, day: 22, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2026, month: 2, day: 28, mark: DayMark::Workday, name: "春节调休补班" },
    Entry { year: 2026, month: 4, day: 4, mark: DayMark::Holiday, name: "清明节" },
    Entry { year: 2026, month: 4, day: 5, mark: DayMark::Holiday, name: "清明节" },
    Entry { year: 2026, month: 4, day: 6, mark: DayMark::Holiday, name: "清明节" },
    Entry { year: 2026, month: 5, day: 1, mark: DayMark::Holiday, name: "劳动节" },
    Entry { year: 2026, month: 5, day: 2, mark: DayMark::Holiday, name: "劳动节" },
    Entry { year: 2026, month: 5, day: 3, mark: DayMark::Holiday, name: "劳动节" },
    Entry { year: 2026, month: 5, day: 4, mark: DayMark::Holiday, name: "劳动节" },
    Entry { year: 2026, month: 5, day: 5, mark: DayMark::Holiday, name: "劳动节" },
    Entry { year: 2026, month: 5, day: 9, mark: DayMark::Workday, name: "劳动节调休补班" },
    Entry { year: 2026, month: 6, day: 19, mark: DayMark::Holiday, name: "端午节" },
    Entry { year: 2026, month: 6, day: 20, mark: DayMark::Holiday, name: "端午节" },
    Entry { year: 2026, month: 6, day: 21, mark: DayMark::Holiday, name: "端午节" },
    Entry { year: 2026, month: 9, day: 25, mark: DayMark::Holiday, name: "中秋节" },
    Entry { year: 2026, month: 9, day: 26, mark: DayMark::Holiday, name: "中秋节" },
    Entry { year: 2026, month: 9, day: 27, mark: DayMark::Holiday, name: "中秋节" },
    Entry { year: 2026, month: 10, day: 1, mark: DayMark::Holiday, name: "国庆节" },
    Entry { year: 2026, month: 10, day: 2, mark: DayMark::Holiday, name: "国庆节" },
    Entry { year: 2026, month: 10, day: 3, mark: DayMark::Holiday, name: "国庆节" },
    Entry { year: 2026, month: 10, day: 4, mark: DayMark::Holiday, name: "国庆节" },
    Entry { year: 2026, month: 10, day: 5, mark: DayMark::Holiday, name: "国庆节" },
    Entry { year: 2026, month: 10, day: 6, mark: DayMark::Holiday, name: "国庆节" },
    Entry { year: 2026, month: 10, day: 7, mark: DayMark::Holiday, name: "国庆节" },
    Entry { year: 2026, month: 10, day: 10, mark: DayMark::Workday, name: "国庆节调休补班" },
];

/// 2027 年（次年覆盖——元旦/春节/清明/劳动/端午/中秋/国庆）。
pub const YEAR_2027: &[Entry] = &[
    Entry { year: 2027, month: 1, day: 1, mark: DayMark::Holiday, name: "元旦" },
    Entry { year: 2027, month: 1, day: 2, mark: DayMark::Holiday, name: "元旦" },
    Entry { year: 2027, month: 1, day: 3, mark: DayMark::Holiday, name: "元旦" },
    Entry { year: 2027, month: 2, day: 6, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2027, month: 2, day: 7, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2027, month: 2, day: 8, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2027, month: 2, day: 9, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2027, month: 2, day: 10, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2027, month: 2, day: 11, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2027, month: 2, day: 12, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2027, month: 2, day: 13, mark: DayMark::Holiday, name: "春节" },
    Entry { year: 2027, month: 4, day: 3, mark: DayMark::Holiday, name: "清明节" },
    Entry { year: 2027, month: 4, day: 4, mark: DayMark::Holiday, name: "清明节" },
    Entry { year: 2027, month: 4, day: 5, mark: DayMark::Holiday, name: "清明节" },
    Entry { year: 2027, month: 5, day: 1, mark: DayMark::Holiday, name: "劳动节" },
    Entry { year: 2027, month: 5, day: 2, mark: DayMark::Holiday, name: "劳动节" },
    Entry { year: 2027, month: 5, day: 3, mark: DayMark::Holiday, name: "劳动节" },
    Entry { year: 2027, month: 5, day: 4, mark: DayMark::Holiday, name: "劳动节" },
    Entry { year: 2027, month: 5, day: 5, mark: DayMark::Holiday, name: "劳动节" },
    Entry { year: 2027, month: 6, day: 9, mark: DayMark::Holiday, name: "端午节" },
    Entry { year: 2027, month: 6, day: 10, mark: DayMark::Holiday, name: "端午节" },
    Entry { year: 2027, month: 6, day: 11, mark: DayMark::Holiday, name: "端午节" },
    Entry { year: 2027, month: 9, day: 15, mark: DayMark::Holiday, name: "中秋节" },
    Entry { year: 2027, month: 9, day: 16, mark: DayMark::Holiday, name: "中秋节" },
    Entry { year: 2027, month: 9, day: 17, mark: DayMark::Holiday, name: "中秋节" },
    Entry { year: 2027, month: 10, day: 1, mark: DayMark::Holiday, name: "国庆节" },
    Entry { year: 2027, month: 10, day: 2, mark: DayMark::Holiday, name: "国庆节" },
    Entry { year: 2027, month: 10, day: 3, mark: DayMark::Holiday, name: "国庆节" },
    Entry { year: 2027, month: 10, day: 4, mark: DayMark::Holiday, name: "国庆节" },
    Entry { year: 2027, month: 10, day: 5, mark: DayMark::Holiday, name: "国庆节" },
    Entry { year: 2027, month: 10, day: 6, mark: DayMark::Holiday, name: "国庆节" },
    Entry { year: 2027, month: 10, day: 7, mark: DayMark::Holiday, name: "国庆节" },
];

/// 全量内置表（离线判据唯一源：两年一表）。
pub const BUILTIN: &[&[Entry]] = &[YEAR_2026, YEAR_2027];

/// 内置数据覆盖年份（更新通道判断「当年+次年是否已内置」用）。
pub const COVERED_YEARS: [u16; 2] = [2026, 2027];

// ---------------------------------------------------------------------------
// 查询面（F078 日历飞出 / F549 时钟悬停共用）
// ---------------------------------------------------------------------------

/// 查某天的标注（无标注返回 None——只有法定级，营销节日不进表）。
pub fn lookup(year: u16, month: u8, day: u8) -> Option<(&'static str, DayMark)> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    for table in BUILTIN {
        for e in table.iter() {
            if e.year == year && e.month == month && e.day == day {
                return Some((e.name, e.mark));
            }
        }
    }
    None
}

/// 红灰规则：假日红、补班灰（渲染层取色唯一口——令牌名）。
pub fn mark_token(mark: DayMark) -> &'static str {
    match mark {
        DayMark::Holiday => "holiday-red",
        DayMark::Workday => "workday-gray",
    }
}

/// 更新通道：给定「当前年」，返回内置数据是否覆盖当年与次年（F122 挂载）。
pub fn covers(current_year: u16) -> bool {
    COVERED_YEARS.contains(&current_year) && COVERED_YEARS.contains(&(current_year + 1))
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_holiday_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 红/灰双色规则：假日/补班两型令牌不同且各自在册。
    set.add(
        "red and gray tokens distinct",
        mark_token(DayMark::Holiday) == "holiday-red" && mark_token(DayMark::Workday) == "workday-gray",
        "",
    );

    // 2. 悬停名称：10-01 国庆节（主册点名样例）红名可查。
    let hit = lookup(2026, 10, 1);
    set.add(
        "hover name for national day",
        hit == Some(("国庆节", DayMark::Holiday)),
        "",
    );

    // 3. 离线数据内置：BUILTIN 静态表非空（内核零网络依赖的结构证据——
    //    纯静态切片查询，无 IO 路径）。
    set.add("offline builtin static", !BUILTIN.is_empty() && !YEAR_2026.is_empty(), "");

    // 4. 更新通道：整表随 F122 替换的挂载口径——覆盖判定当年+次年。
    set.add(
        "update channel covers current and next",
        covers(2026) && !covers(2027) && !covers(2025),
        "",
    );

    // 5. 两年覆盖：2026 与 2027 各自可查到元旦。
    let a = lookup(2026, 1, 1).is_some();
    let b = lookup(2027, 1, 1).is_some();
    set.add("two year coverage", a && b, "");

    // 6. 调休补班灰标：2026-02-15 春节调休补班可查且为 Workday。
    let wd = lookup(2026, 2, 15);
    set.add(
        "makeup workday gray",
        wd == Some(("春节调休补班", DayMark::Workday)),
        "",
    );

    // 7. 标注克制：普通工作日无标注（2026-03-05 非法定）。
    set.add("ordinary day unmarked", lookup(2026, 3, 5).is_none(), "");

    // 8. 非法日期拒绝：13 月 / 32 日查无（脏数据不静默命中）。
    set.add(
        "invalid dates rejected",
        lookup(2026, 13, 1).is_none() && lookup(2026, 1, 32).is_none(),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mid_autumn_2026_september() {
        assert_eq!(lookup(2026, 9, 25), Some(("中秋节", DayMark::Holiday)));
    }

    #[test]
    fn spring_festival_2027_exact_range() {
        // 2027 春节 2/6-2/13 八天全红。
        for d in 6..=13u8 {
            assert_eq!(lookup(2027, 2, d), Some(("春节", DayMark::Holiday)));
        }
        assert!(lookup(2027, 2, 5).is_none());
        assert!(lookup(2027, 2, 14).is_none());
    }

    #[test]
    fn entries_sorted_within_year() {
        // 表内同一年条目按 (月, 日) 升序（日历渲染假设，破了会乱标）。
        for table in BUILTIN {
            for w in table.windows(2) {
                let (a, b) = (&w[0], &w[1]);
                if a.year == b.year {
                    assert!((a.month, a.day) < (b.month, b.day), "{}-{} 排序错", a.month, a.day);
                }
            }
        }
    }
}
