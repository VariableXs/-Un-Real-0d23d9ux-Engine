//! F549 时钟悬停完整日期（ustar3 · clockcal）——任务栏时钟悬停 Tooltip
//! （F205 体系）：完整日期+星期+农历，比点开日历飞出（F078）更轻的一层。
//!
//! 主册判据（验收标准第一句）：
//! **三要素内容；500ms 延迟一致；农历准确性（2026-2030 五年抽检）；
//! 离线判据；Tooltip 与 F078 分工。**
//!
//! 【功能定义】任务栏时钟悬停 Tooltip：完整日期+星期+农历（「2026 年
//! 9 月 25 日 星期五 · 八月十五」）——悬停即见不点击；Tooltip 延迟沿用
//! 500ms 全局（F205）；农历数据离线内置（U 盘纪律）。
//! 【无感标准】鼠标搭上去就看见（不用点）；农历给中文用户的贴心默认；
//! 离线也准（万年历内置）；悬停轻、点击重（两层日历体验各司其职）。
//!
//! 【实现与数据源】农历换算采用经典压缩位表算法（月大小位 + 闰月号，
//! 与 1900-2100 通行万年历数据表同源）；本包离线内置 2025-2030 六年
//! 月序数据与 2025-2031 春节（正月初一）锚点，覆盖判据要求 2026-2030
//! 五年抽检（含跨年边界：2026 年初属乙巳年腊月、2030 年末属庚戌年）。
//! 内部一致性不变量：按位表逐月步进必须恰好落在下一年春节锚点上
//! （`year_walk_lands_on_next_cny`）——位表或锚点任一有误即红。
//!
//! 【偏差登记（一处一事实）】主册示例句写作「2026 年 9 月 25 日 …
//! 八月初五」；真实万年历中 2026-09-25 为**八月十五（中秋节）**。
//! 判据唯一源冲突时以事实为准：本实现按真实农历换算（2026-09-25 →
//! 八月十五），主册示例句笔误已登记偏差，待主册修订。
//!
//! 【Tooltip 与 F078 分工】悬停 Tooltip = 轻量（三要素一行、500ms 延迟、
//! 不可交互）；日历飞出 F078 = 重量级（月历网格、可点、弹出 ≤100ms）。
//! 两层各自的触发与内容域互不越界（`tooltip_scope` / `flyout_scope`）。
//!
//! 零堆纪律：定长表 + core 运算，无 String/Vec/format!；输出为结构化
//! 数值字段与 &'static str 命名表。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// Tooltip 悬停延迟：沿用全局 500ms（F205 全局延迟一致——主册规格框架）。
pub const TOOLTIP_DELAY_MS: u32 = 500;
/// F078 日历飞出弹出预算 ≤100ms（对照锚，分工边界依据）。
pub const FLYOUT_OPEN_MS: u32 = 100;
/// 数据覆盖起点（农历年）：2025 乙巳（供 2026 年初跨年边界换算）。
pub const LUNAR_DATA_FIRST_YEAR: i32 = 2025;
/// 数据覆盖终点（农历年）：2030 庚戌（判据五年抽检终点）。
pub const LUNAR_DATA_LAST_YEAR: i32 = 2030;

// ---------------------------------------------------------------------------
// 农历数据（离线内置）
// ---------------------------------------------------------------------------

/// 春节（正月初一）锚点表，索引 0 = 2025-01-29（甲辰→乙巳交接日为
/// 2025-01-29，即乙巳年正月初一），以 (年, 当日公历天数序号) 表达：
/// `days_from_civil(y, m, d)`（见下）保证与换算同一套日历算术。
/// 锚点为通行万年历春节日期：
/// 2025-01-29 / 2026-02-17 / 2027-02-06 / 2028-01-26 / 2029-02-13 /
/// 2030-02-03 / 2031-01-23。
pub const CNY_ANCHORS: [(i32, u32, u32); 7] = [
    (2025, 1, 29),
    (2026, 2, 17),
    (2027, 2, 6),
    (2028, 1, 26),
    (2029, 2, 13),
    (2030, 2, 3),
    (2031, 1, 23),
];

/// 农历月序压缩位表（2025-2030，与通行 1900-2100 万年历数据表同源）。
/// 编码：bit0-3 = 闰月号（0=无闰月）；bit4-15 = 十二个月大小
/// （bit(16-m)=1 为 30 天大月，m=1..12）；bit16 = 闰月为大月。
pub const LUNAR_INFO: [u32; 6] = [
    0x0a6e6, // 2025 乙巳：闰六月
    0x0a4e0, // 2026 丙午：无闰
    0x0d260, // 2027 丁未：无闰
    0x0ea65, // 2028 戊申：闰五月
    0x0d530, // 2029 己酉：无闰
    0x05aa0, // 2030 庚戌：无闰
];

/// 农历月份名（1-12，含正月/冬月/腊月惯称）。
pub const LUNAR_MONTH_NAMES: [&str; 12] =
    ["正", "二", "三", "四", "五", "六", "七", "八", "九", "十", "冬", "腊"];
/// 农历日名（初一-三十）。
pub const LUNAR_DAY_NAMES: [&str; 30] = [
    "初一", "初二", "初三", "初四", "初五", "初六", "初七", "初八", "初九", "初十",
    "十一", "十二", "十三", "十四", "十五", "十六", "十七", "十八", "十九", "二十",
    "廿一", "廿二", "廿三", "廿四", "廿五", "廿六", "廿七", "廿八", "廿九", "三十",
];
/// 星期名（索引 0=周一 … 6=周日；weekday() 返回 1-7 语义另映射）。
pub const WEEKDAY_NAMES: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];

// ---------------------------------------------------------------------------
// 日历算术（civil days，公历）
// ---------------------------------------------------------------------------

/// 公历 (y,m,d) → 自 0000-03-01 起的天数序号（Howard Anton 式 civil 算法，
/// 纯整数，无浮点）。全包换算统一走这一套算术，杜绝两套历法混算。
pub const fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64; // [0, 399]
    let mp = ((m + 9) % 12) as i64; // 3月=0 … 2月=11
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468 + 305 // 平移到 0000-03-01 基准再补齐
}

/// 星期几（1=周一 … 7=周日）。锚定事实：2026-09-25 为星期五（主册示例
/// 句的星期部分为真，已实测锚定）。
pub fn weekday(y: i32, m: u32, d: u32) -> u32 {
    let days = days_from_civil(y, m, d);
    let anchor = days_from_civil(2026, 9, 25); // 星期五
    // 与锚点同余：((days-anchor)+4) mod 7 → 0=周一
    let diff = days - anchor; // 2026-09-25 → 0
    let idx = (diff + 4).rem_euclid(7) as usize; // 4 = 周五在 WEEKDAY_NAMES 的下标
    (idx + 1) as u32
}

// ---------------------------------------------------------------------------
// 农历换算核心
// ---------------------------------------------------------------------------

/// 某农历年闰月号（0=无闰月）。
pub fn leap_month(lunar_year: i32) -> u32 {
    if !(LUNAR_DATA_FIRST_YEAR..=LUNAR_DATA_LAST_YEAR).contains(&lunar_year) {
        return 0;
    }
    (LUNAR_INFO[(lunar_year - LUNAR_DATA_FIRST_YEAR) as usize] & 0xf) as u32
}

/// 农历年某月天数（lunar_month 1-12；`leap=true` 查闰月本身）。
/// 返回 0 = 该月不存在（如查闰月但当年无闰）。
pub fn lunar_month_days(lunar_year: i32, lunar_month: u32, leap: bool) -> u32 {
    if !(LUNAR_DATA_FIRST_YEAR..=LUNAR_DATA_LAST_YEAR).contains(&lunar_year) {
        return 0;
    }
    let info = LUNAR_INFO[(lunar_year - LUNAR_DATA_FIRST_YEAR) as usize];
    if leap {
        let lm = info & 0xf;
        if lm == 0 || lunar_month != lm {
            return 0;
        }
        return if info & 0x1_0000 != 0 { 30 } else { 29 };
    }
    if lunar_month == 0 || lunar_month > 12 {
        return 0;
    }
    if info & (0x1_0000 >> lunar_month) != 0 {
        30
    } else {
        29
    }
}

/// 农历年总天数（含闰月；范围外返回 0）。
pub fn lunar_year_days(lunar_year: i32) -> u32 {
    if !(LUNAR_DATA_FIRST_YEAR..=LUNAR_DATA_LAST_YEAR).contains(&lunar_year) {
        return 0;
    }
    let mut total = 0u32;
    for m in 1..=12u32 {
        total += lunar_month_days(lunar_year, m, false);
    }
    let lm = leap_month(lunar_year);
    if lm > 0 {
        total += lunar_month_days(lunar_year, lm, true);
    }
    total
}

/// 春节锚点 → 公历天数序号（年份在表外返回 i64::MIN 哨兵）。
fn cny_day(lunar_year: i32) -> i64 {
    for (y, m, d) in CNY_ANCHORS {
        if y == lunar_year {
            return days_from_civil(y, m, d);
        }
    }
    i64::MIN
}

/// 农历换算结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LunarDate {
    pub lunar_year: i32,
    /// 1-12；`leap=true` 时为闰月的月号。
    pub month: u32,
    pub leap: bool,
    /// 1-30。
    pub day: u32,
}

/// 公历 → 农历（覆盖 [2025 春节, 2031 春节) 的判据范围 2026-2030 及
/// 跨年边界）。超出内置数据范围返回 None——诚实越界，不硬算。
pub fn solar_to_lunar(y: i32, m: u32, d: u32) -> Option<LunarDate> {
    let day = days_from_civil(y, m, d);
    // 定位农历年：最后一个春节锚点 <= day。
    let mut cur: Option<i32> = None;
    for (yy, _, _) in CNY_ANCHORS {
        let cd = cny_day(yy);
        if cd <= day && (LUNAR_DATA_FIRST_YEAR..=LUNAR_DATA_LAST_YEAR).contains(&yy) {
            cur = Some(yy);
        }
    }
    let lunar_year = cur?;
    let base = cny_day(lunar_year);
    if base == i64::MIN {
        return None;
    }
    let offset = (day - base) as u32;
    if offset >= lunar_year_days(lunar_year) {
        // 落在下一年春节之后（正常不会发生——锚点表覆盖到 2031）。
        return None;
    }
    // 逐月步进。
    let lm = leap_month(lunar_year);
    let mut rest = offset;
    let mut m = 1u32;
    while m <= 12 {
        let md = lunar_month_days(lunar_year, m, false);
        if rest < md {
            return Some(LunarDate { lunar_year, month: m, leap: false, day: rest + 1 });
        }
        rest -= md;
        if lm == m {
            let lmd = lunar_month_days(lunar_year, lm, true);
            if rest < lmd {
                return Some(LunarDate { lunar_year, month: m, leap: true, day: rest + 1 });
            }
            rest -= lmd;
        }
        m += 1;
    }
    None
}

/// 农历月名（含「闰」前缀）。
pub fn lunar_month_name(ld: LunarDate) -> &'static str {
    let base = LUNAR_MONTH_NAMES[(ld.month - 1) as usize];
    if ld.leap {
        // &str 拼接需要堆；闰月名走查表（12 个月全表，零堆）。
        LEAP_MONTH_NAMES[(ld.month - 1) as usize]
    } else {
        base
    }
}

/// 闰月名全表（零堆：&'static str 查表替代拼接）。
pub const LEAP_MONTH_NAMES: [&str; 12] =
    ["闰正", "闰二", "闰三", "闰四", "闰五", "闰六", "闰七", "闰八", "闰九", "闰十", "闰冬", "闰腊"];

// ---------------------------------------------------------------------------
// Tooltip 三要素内容模型
// ---------------------------------------------------------------------------

/// Tooltip 内容三要素（判据一：完整日期/星期/农历）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HoverDateCard {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// 1=周一 … 7=周日。
    pub weekday: u32,
    pub lunar: Option<LunarDate>,
}

/// 时钟悬停 → Tooltip 内容（判据核心路径）。
pub fn hover_card(y: i32, m: u32, d: u32) -> Option<HoverDateCard> {
    Some(HoverDateCard {
        year: y,
        month: m,
        day: d,
        weekday: weekday(y, m, d),
        lunar: solar_to_lunar(y, m, d),
    })
}

/// 三要素齐备判定（判据一：三要素内容）。
pub fn card_complete(card: &HoverDateCard) -> bool {
    card.weekday >= 1 && card.weekday <= 7 && card.lunar.is_some()
}

/// Tooltip 轻量层（悬停触发，500ms 延迟，不可交互）。
pub const fn tooltip_scope() -> (&'static str, u32, bool) {
    ("hover-tooltip", TOOLTIP_DELAY_MS, false)
}

/// 日历飞出重量层（F078：点击触发，≤100ms 弹出，月历网格可交互）。
pub const fn flyout_scope() -> (&'static str, u32, bool) {
    ("click-flyout-F078", FLYOUT_OPEN_MS, true)
}

/// 两层分工不打架：同一时钟上悬停只出 Tooltip、点击才出飞出
/// （触发分离 + 内容域分离 + 交互级分离——判据五）。
pub fn layers_disjoint() -> bool {
    let (t_name, t_delay, t_interactive) = tooltip_scope();
    let (f_name, f_delay, f_interactive) = flyout_scope();
    t_name != f_name && t_delay != f_delay && t_interactive != f_interactive
}

/// 离线判据：数据全部内置（静态表），无任何外部取数路径——
/// 结构自证：本模块无 I/O 符号；对账函数核对表覆盖年份数。
pub fn offline_data_intact() -> bool {
    LUNAR_INFO.len() == (LUNAR_DATA_LAST_YEAR - LUNAR_DATA_FIRST_YEAR + 1) as usize
        && CNY_ANCHORS.len() == 7
        && LUNAR_DAY_NAMES.len() == 30
        && LUNAR_MONTH_NAMES.len() == 12
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_f549_checks() -> CheckSet {
    let mut cs = CheckSet::new("F549-clock-hover");
    // 判据一：三要素内容（完整日期+星期+农历）。
    let card = hover_card(2026, 9, 25).unwrap();
    cs.add("hover_card_three_elements", card_complete(&card), "");
    // 星期锚定：2026-09-25 = 星期五（主册示例句星期部分为真）。
    cs.add("weekday_anchor_friday", card.weekday == 5, "");
    // 判据二：500ms 延迟一致（F205 全局）。
    cs.add("tooltip_delay_500ms", tooltip_scope().1 == 500 && tooltip_scope().0 != flyout_scope().0, "");
    // 判据五：Tooltip 与 F078 分工（触发/内容/交互三层分离）。
    cs.add("tooltip_flyout_layers_disjoint", layers_disjoint(), "");
    // 判据四：离线判据（数据内置齐备）。
    cs.add("offline_data_intact", offline_data_intact(), "");
    // 判据三：农历准确性——春节锚点五年抽检（正月初一必须落在春节）。
    let cny_ok = [
        solar_to_lunar(2026, 2, 17),
        solar_to_lunar(2027, 2, 6),
        solar_to_lunar(2028, 1, 26),
        solar_to_lunar(2029, 2, 13),
        solar_to_lunar(2030, 2, 3),
    ]
    .iter()
    .enumerate()
    .all(|(i, ld)| match ld {
        Some(l) => l.month == 1 && l.day == 1 && !l.leap,
        None => i == usize::MAX, // None 必红
    });
    cs.add("cny_anchors_2026_2030", cny_ok, "");
    // 中秋抽检：2026-09-25 = 八月十五（真实万年历；主册示例「初五」为笔误，
    // 偏差已登记）。
    let mid_autumn = solar_to_lunar(2026, 9, 25).unwrap();
    cs.add(
        "mid_autumn_2026_8_15",
        mid_autumn.month == 8 && mid_autumn.day == 15 && !mid_autumn.leap,
        "",
    );
    // 内部一致性不变量：位表逐月步进恰好落在下一年春节（数据自洽）。
    let mut walk_ok = true;
    for y in LUNAR_DATA_FIRST_YEAR..LUNAR_DATA_LAST_YEAR {
        let span = cny_day(y + 1) - cny_day(y);
        if span != lunar_year_days(y) as i64 {
            walk_ok = false;
        }
    }
    cs.add("year_walk_lands_on_next_cny", walk_ok, "");
    // 闰月抽检：2025 闰六月、2028 闰五月（通行万年历事实）。
    cs.add("leap_2025_month6_2028_month5", leap_month(2025) == 6 && leap_month(2028) == 5, "");
    // 跨年边界：2026-01-01 属乙巳年冬月十三（下一个春节 2026-02-17 之前）。
    let edge = solar_to_lunar(2026, 1, 1).unwrap();
    cs.add(
        "year_edge_2026_01_01_is_yisi_11m13",
        edge.lunar_year == 2025 && edge.month == 11 && edge.day == 13 && !edge.leap,
        "",
    );
    // 范围外诚实越界（2024 不在内置数据内 → None，不硬算）。
    cs.add("out_of_range_honest_none", solar_to_lunar(2024, 2, 10).is_none(), "");
    // 月名/日名表可索引（八月初五→「八」、三十日名在表）。
    cs.add(
        "naming_tables_indexable",
        LUNAR_MONTH_NAMES[7] == "八" && LUNAR_DAY_NAMES[14] == "十五" && LUNAR_DAY_NAMES[29] == "三十",
        "",
    );
    cs
}

#[cfg(test)]
mod f549_tests {
    use super::*;

    #[test]
    fn cny_anchors_all_land_on_lunar_1_1() {
        // 农历准确性（2026-2030 五年抽检）：春节必须=正月初一。
        for (y, m, d) in [
            (2026, 2, 17),
            (2027, 2, 6),
            (2028, 1, 26),
            (2029, 2, 13),
            (2030, 2, 3),
        ] {
            let ld = solar_to_lunar(y, m, d).expect("春节必须在内置数据范围内");
            assert!((ld.month, ld.day, ld.leap) == (1, 1, false), "{y} 年春节应为正月初一");
        }
    }

    #[test]
    fn mid_autumn_2026_is_aug_15() {
        // 主册示例句「八月初五」与真实万年历不符：2026-09-25 = 八月十五（中秋）。
        let ld = solar_to_lunar(2026, 9, 25).unwrap();
        assert_eq!(ld.month, 8, "八月");
        assert_eq!(ld.day, 15, "十五——偏差登记：主册笔误");
        assert!(!ld.leap);
    }

    #[test]
    fn year_walk_consistency_invariant() {
        // 位表月长合计 = 相邻春节锚点差（六年逐年核对）。
        for y in LUNAR_DATA_FIRST_YEAR..=LUNAR_DATA_LAST_YEAR {
            let span = cny_day(y + 1) - cny_day(y);
            assert_eq!(span, lunar_year_days(y) as i64, "{y} 农历年天数与春节锚点差必须一致");
        }
    }

    #[test]
    fn leap_months_match_reality() {
        assert_eq!(leap_month(2025), 6, "2025 闰六月");
        assert_eq!(leap_month(2028), 5, "2028 闰五月");
        assert_eq!(leap_month(2026), 0, "2026 无闰月");
        let leap_day = solar_to_lunar(2025, 7, 25); // 闰六月初一=2025-07-25
        let ld = leap_day.unwrap();
        assert!(ld.leap && ld.month == 6 && ld.day == 1, "2025-07-25 应为闰六月初一");
    }

    #[test]
    fn weekday_math_across_years() {
        // 锚点 2026-09-25=周五；次周六、前日周四（civil 算术自洽）。
        assert_eq!(weekday(2026, 9, 26), 6);
        assert_eq!(weekday(2026, 9, 24), 4);
        assert_eq!(weekday(2026, 1, 1), 4); // 2026 元旦周四（通行事实）
        assert_eq!(weekday(2030, 1, 1), 2); // 2030 元旦周二（通行事实）
    }

    #[test]
    fn hover_card_is_lightweight_and_complete() {
        let card = hover_card(2030, 6, 1).unwrap();
        assert!(card_complete(&card), "三要素齐备");
        assert_eq!(card.lunar.unwrap().lunar_year, 2030);
        assert_eq!(TOOLTIP_DELAY_MS, 500, "延迟沿用 500ms 全局");
        assert!(layers_disjoint(), "Tooltip 与 F078 分工不打架");
    }

    #[test]
    fn out_of_range_is_honest_none() {
        assert!(solar_to_lunar(2024, 1, 1).is_none(), "数据范围外诚实返回 None");
        assert!(solar_to_lunar(2031, 6, 1).is_none(), "2031（超出判据范围）不硬算");
    }
}
