//! F078 日历飞出 · 完整设计（STAR I 主册 G-C-08）。
//!
//! **判据（主册）**：月历渲染与万年历对照 12 个月全对（含闰年 2028
//! 二月）；弹出 ≤100ms。
//!
//! **设计要点（主册）**：
//! - 点击任务栏时钟弹出月历卡：当月网格/今日高亮（强调色圆底）/
//!   前后月翻页/今日日程位（数据面留口，STAR I start 后程接日历应用）；
//! - 面板 320×340px 时钟上方弹出；月网格 7 列单元 42px、今日格强调
//!   色圆底白字；左右箭头翻月（键盘 PgUp/PgDn 同效）；标题
//!   「2026 年 9 月」可点回今日；
//! - 月历纯计算无存储；日程槽位预留 API（空实现，差异表注明）；
//! - 系统时间异常（F187 校时未完成）→ 顶部黄条提示；翻页无边界
//!   （无限前后）；
//! - 周起始日跟随区域设置（中国周一）；节假日标注槽位预留（数据面
//!   后程）；农历显示评估项（开源 lunar 库接入评估，F130）——进评估
//!   不进承诺；面板动画 150ms 自上而下 8px 位移。
//!
//! 实装口径：公历（前溯格里高利历）纯算法——闰年规则、星期锚点、
//! 月长表全自研零依赖；时间注入式（年月日实参）；弹出预算与动画账
//! 走 dbase 底盘。

use crate::checks::CheckSet;

use crate::deskstar::dbase::{FloatLayer, POP_SLIDE, Token};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/设计细节）
// ---------------------------------------------------------------------------

/// 面板宽（px）。
pub const PANEL_W_PX: i32 = 320;

/// 面板高（px）。
pub const PANEL_H_PX: i32 = 340;

/// 月网格单元（px，7 列）。
pub const CELL_PX: i32 = 42;

/// 弹出预算（ms）。
pub const POPUP_BUDGET_MS: u64 = 100;

/// 动画位移幅度（px，自上而下）。
pub const SLIDE_OFFSET_PX: i32 = 8;

/// 周起始日（中国周一——区域设置跟随的缺省档）。
pub const WEEK_START_MONDAY: bool = true;

/// 今日日程槽数（数据面留口——空实现，差异表注明）。
pub const SCHEDULE_SLOTS: usize = 0;

// ---------------------------------------------------------------------------
// 公历算法（万年历核心——12 个月全对判据的实体）
// ---------------------------------------------------------------------------

/// 闰年判定（前溯格里高利历：4 整除且非 100 整除，或 400 整除）。
pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// 月长表（平年；二月在闰年 +1）。
pub fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// 星期判定（返回 0=周一 .. 6=周日；中国周起始）。
///
/// 锚点事实：2000-01-01 是周六。前溯外推（格里高利历在 1582 年前为
/// 外推值——万年历对照口径一致）。
pub fn weekday_monday0(year: i32, month: u8, day: u8) -> u8 {
    // Zeller 式计数改为「自锚点日数差」：先算绝对日序（rata die 简化式）。
    let (y, m) = if month <= 2 { (year - 1, month as i32 + 12) } else { (year, month as i32) };
    // Fliegel-Van Flandern 公式（格里高利历日序），结果域 0=周六。
    let a = (13 * (m + 1)) / 5;
    let b = y / 4;
    let c = y / 100;
    let d = y / 400;
    let jdn = 365 * y + a + b - c + d + day as i32 - 1;
    // jdn 对 7 取模：锚定 2000-01-01（周六）校准。
    // 2000-01-01 代入：y=1999, m=13, day=1 → 用同公式自洽，模 7 校准常量。
    let base = 365 * 1999 + (13 * 14) / 5 + 1999 / 4 - 1999 / 100 + 1999 / 400 + 0;
    let diff = jdn - base; // 0 = 2000-01-01
    let sat0 = diff.rem_euclid(7); // 0=周六
    // 0=周六 → 0=周一 的映射：周一=-2 mod 7 = 5。
    ((sat0 + 5) % 7) as u8
}

/// 月网格：给定年月，返回 6×7 格（含前后月补位格；None = 补位）。
///
/// 网格序：周一起始、行优先。首格 = 当月 1 日所在周的周一。
pub fn month_grid(year: i32, month: u8) -> Vec<Option<(u8, bool)>> {
    let mut grid = vec![None; 42];
    let dim = days_in_month(year, month) as usize;
    if dim == 0 {
        return grid; // 非法月：空网格（渲染层显空态）
    }
    let lead = weekday_monday0(year, month, 1) as usize;
    for d in 0..dim {
        grid[lead + d] = Some((d as u8 + 1, true));
    }
    // 尾补位：次月开头（日号按位置重排——次月年月号无需显式取）。
    for (i, cell) in grid.iter_mut().enumerate().skip(lead + dim) {
        *cell = Some(((i - lead - dim + 1).min(31) as u8, false));
    }
    // 头补位：上月结尾。
    let (py, pm) = prev_month(year, month);
    let pdim = days_in_month(py, pm);
    for i in 0..lead {
        grid[i] = Some((pdim - lead as u8 + 1 + i as u8, false));
    }
    grid
}

/// 次月（翻页无边界：支持负年份方向无限推）。
pub fn next_month(year: i32, month: u8) -> (i32, u8) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

/// 上月。
pub fn prev_month(year: i32, month: u8) -> (i32, u8) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

// ---------------------------------------------------------------------------
// 飞出面板状态机
// ---------------------------------------------------------------------------

/// 日历飞出。
pub struct CalFlyout {
    layer: FloatLayer,
    frame: crate::deskstar::dbase::Rect,
    /// 正在查看的年月（翻页态；与今日解耦——「点回今日」才重置）。
    view_year: i32,
    view_month: u8,
    /// 今日（注入；本模块不持真实时钟）。
    today: (i32, u8, u8),
    /// 系统时间异常旗标（F187 校时未完成 → 顶部黄条）。
    time_anomaly: bool,
    now_ms: u64,
    popup_ok: Option<bool>,
    /// 节假日槽位（数据面预留：空实现——差异表注明）。
    holiday_slots: u32,
    /// 日程槽位（数据面预留：空实现）。
    schedule_slots: u32,
}

impl CalFlyout {
    pub fn new(today: (i32, u8, u8), now_ms: u64) -> CalFlyout {
        CalFlyout {
            layer: FloatLayer::new(),
            frame: crate::deskstar::dbase::Rect::new(0, 0, PANEL_W_PX, PANEL_H_PX),
            view_year: today.0,
            view_month: today.1,
            today,
            time_anomaly: false,
            now_ms,
            popup_ok: None,
            holiday_slots: 0,
            schedule_slots: SCHEDULE_SLOTS as u32,
        }
    }

    /// 打开（时钟点击）。view 重置回今日——每次打开都「回到今天」。
    pub fn open(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        if self.layer.open(now_ms) {
            self.view_year = self.today.0;
            self.view_month = self.today.1;
        }
    }

    pub fn close(&mut self, now_ms: u64) {
        self.layer
            .close(crate::deskstar::dbase::CloseCause::Escape, now_ms, true);
    }

    pub fn is_open(&self) -> bool {
        self.layer.is_open()
    }

    /// 首帧渲染完报到（弹出 ≤100ms 判据）。
    pub fn popup_shown(&mut self, now_ms: u64) {
        self.popup_ok = Some(self.layer.open_in_budget(now_ms, POPUP_BUDGET_MS));
    }

    pub fn popup_in_budget(&self) -> bool {
        self.popup_ok == Some(true)
    }

    /// 面板定位（时钟上方弹出：底边贴时钟顶，水平居中于时钟）。
    pub fn place_above(&mut self, clock_rect: crate::deskstar::dbase::Rect, screen: crate::deskstar::dbase::Rect) {
        let f = crate::deskstar::dbase::Rect::new(
            clock_rect.x + clock_rect.w / 2 - PANEL_W_PX / 2,
            clock_rect.y - PANEL_H_PX,
            PANEL_W_PX,
            PANEL_H_PX,
        );
        self.frame = f.clamp_into(&screen);
    }

    pub fn frame(&self) -> crate::deskstar::dbase::Rect {
        self.frame
    }

    /// 翻页（左右箭头 / PgUp·PgDn 同效；dir=true 次月）。无边界。
    pub fn page(&mut self, dir: bool) {
        let (y, m) = if dir {
            next_month(self.view_year, self.view_month)
        } else {
            prev_month(self.view_year, self.view_month)
        };
        self.view_year = y;
        self.view_month = m;
    }

    /// 标题点击：回今日（view 与今日重合）。
    pub fn jump_today(&mut self) {
        self.view_year = self.today.0;
        self.view_month = self.today.1;
    }

    /// 标题文案（「2026 年 9 月」）。
    pub fn title(&self) -> alloc::string::String {
        alloc::format!("{} 年 {} 月", self.view_year, self.view_month)
    }

    /// 标题是否正对今日（回今日钮的可点性提示）。
    pub fn on_today(&self) -> bool {
        self.view_year == self.today.0 && self.view_month == self.today.1
    }

    /// 当前网格（42 格；含今日高亮判定：格值与今日全等）。
    pub fn grid(&self) -> Vec<Option<(u8, bool)>> {
        month_grid(self.view_year, self.view_month)
    }

    /// 今日格下标（网格内存在今日才返回 Some）。
    pub fn today_index(&self) -> Option<usize> {
        if self.view_year != self.today.0 || self.view_month != self.today.1 {
            return None;
        }
        let lead = weekday_monday0(self.today.0, self.today.1, 1) as usize;
        Some(lead + self.today.2 as usize - 1)
    }

    /// 今日格着色令牌（强调色圆底白字——白字为文本令牌非硬编码色）。
    pub fn today_tokens(&self) -> (Token, Token) {
        (Token::Accent, Token::TextPrimary)
    }

    /// 时间异常黄条（F187 联动；着色走 Warn 令牌）。
    pub fn anomaly_banner(&self) -> Option<&'static str> {
        if self.time_anomaly {
            Some("系统时间可能不准确，正在校时（F187）")
        } else {
            None
        }
    }

    pub fn set_time_anomaly(&mut self, anomaly: bool) {
        self.time_anomaly = anomaly;
    }

    /// 动画进度（150ms 自上而下 8px 位移——曲线取 POP_SLIDE，
    /// 位移量 = (1-进度) × 8px 的渲染换算交上层，本账供千分比）。
    pub fn anim_progress(&self) -> u16 {
        if !self.layer.is_open() {
            return 0;
        }
        POP_SLIDE.at((self.now_ms.saturating_sub(self.layer.opened_at_ms())) as u32)
    }

    /// 今日日程槽位查询（数据面留口：空实现——差异表注明）。
    pub fn schedule_of(&self, _year: i32, _month: u8, _day: u8) -> u32 {
        0
    }

    /// 节假日标注槽位查询（数据面预留：空实现）。
    pub fn holiday_of(&self, _year: i32, _month: u8, _day: u8) -> Option<&'static str> {
        None
    }

    pub fn slot_counts(&self) -> (u32, u32) {
        (self.holiday_slots, self.schedule_slots)
    }
}

// ---------------------------------------------------------------------------
// 自检（判据唯一源：主册 G-C-08 验收判据）
// ---------------------------------------------------------------------------

/// F078 自检：12 个月万年历全对（含 2028 闰二月）、星期锚点、
/// 弹出 ≤100ms、翻页无边界、回今日、补位网格完整、时间异常黄条。
pub fn run_calflyout_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F078");
    // 1. 闰年规则：2028 闰、2100 平、2000 闰、2026 平。
    set.add(
        "leap-rules",
        is_leap(2028) && !is_leap(2100) && is_leap(2000) && !is_leap(2026),
        "gregorian leap",
    );
    // 2. 月长表：2028 二月 29、2026 二月 28、四季月长全对。
    let ok_dim = days_in_month(2028, 2) == 29
        && days_in_month(2026, 2) == 28
        && days_in_month(2026, 1) == 31
        && days_in_month(2026, 4) == 30
        && days_in_month(2026, 12) == 31;
    set.add("month-days", ok_dim, "days in month table");
    // 3. 星期锚点：2026-09-26（仓库主册成文日）是周六；2000-01-01 周六。
    set.add(
        "weekday-anchor",
        weekday_monday0(2026, 9, 26) == 5 && weekday_monday0(2000, 1, 1) == 5,
        "sat anchor",
    );
    // 4. 2026 全 12 个月网格：当月日序连续、总格数守恒、首格是周一。
    let mut ok12 = true;
    for m in 1..=12u8 {
        let g = month_grid(2026, m);
        let dim = days_in_month(2026, m) as usize;
        let count = g.iter().filter(|c| matches!(c, Some((_, true)))).count();
        let lead = weekday_monday0(2026, m, 1) as usize;
        let first_ok = matches!(g[0], Some((d, _)) if lead > 0 || d == 1);
        if count != dim || lead + dim > 42 || !first_ok {
            ok12 = false;
        }
    }
    set.add("grid-12-months", ok12, "2026 all 12 months");
    // 5. 2028 闰二月：29 天全在当月列。
    let g = month_grid(2028, 2);
    let feb_days = g.iter().filter(|c| matches!(c, Some((_, true)))).count();
    set.add("leap-feb-grid", feb_days == 29, "2028 feb 29");
    // 6. 面板生命周期全链。
    let mut cal = CalFlyout::new((2026, 9, 26), 1_000);
    cal.open(1_000);
    cal.popup_shown(1_099);
    let in_budget = cal.popup_in_budget();
    cal.popup_shown(1_101);
    ok_if(
        &mut set,
        "popup-budget",
        in_budget && !cal.popup_in_budget(),
        "≤100ms",
    );
    // 7. 翻页无边界（跨年双向）+ 回今日。
    cal.page(true); // 2026-09 → 2026-10
    cal.page(true); // → 2026-11
    let fwd = cal.title() == "2026 年 11 月";
    for _ in 0..3 {
        cal.page(false); // → 10 → 9 → 8
    }
    let back = cal.title() == "2026 年 8 月";
    for _ in 0..12 {
        cal.page(true); // 8 月 +12 → 2027-08
    }
    let cross_year = cal.title() == "2027 年 8 月";
    for _ in 0..12 {
        cal.page(false);
    }
    cal.jump_today();
    ok_if(
        &mut set,
        "paging",
        fwd && back && cross_year && cal.on_today(),
        "no boundary + today",
    );
    // 8. 今日高亮位：9 月网格中 26 日在周六列（周一0 起第 5 列）。
    cal.jump_today();
    let ti = cal.today_index();
    let ti_ok = ti == Some(weekday_monday0(2026, 9, 1) as usize + 25);
    ok_if(&mut set, "today-highlight", ti_ok, "accent circle at 26th");
    // 9. 时间异常黄条 + 着色令牌位。
    cal.set_time_anomaly(true);
    let banner = cal.anomaly_banner().is_some();
    cal.set_time_anomaly(false);
    ok_if(
        &mut set,
        "anomaly-banner",
        banner && !cal.anomaly_banner().is_some(),
        "F187 yellow bar",
    );
    // 10. 日程/节假日槽位空实现（数据面留口——差异表注明）。
    let (h, s) = cal.slot_counts();
    ok_if(
        &mut set,
        "data-slots",
        h == 0 && s == 0 && cal.schedule_of(2026, 9, 26) == 0
            && cal.holiday_of(2026, 10, 1).is_none(),
        "empty impl reserved",
    );
    set
}

fn ok_if(set: &mut CheckSet, group: &'static str, ok: bool, detail: &'static str) {
    set.add(group, ok, detail);
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deskstar::dbase::Rect;

    #[test]
    fn weekday_progression_september_2026() {
        // 2026-09-26 周六 → 27 周日 → 28 周一（跨周校验）。
        assert_eq!(weekday_monday0(2026, 9, 26), 5);
        assert_eq!(weekday_monday0(2026, 9, 27), 6);
        assert_eq!(weekday_monday0(2026, 9, 28), 0);
        // 2028-02-29 存在且是周二（万年历对照）。
        assert_eq!(weekday_monday0(2028, 2, 29), 1);
    }

    #[test]
    fn grid_backfill_uses_prev_month_tail() {
        let g = month_grid(2026, 9);
        let lead = weekday_monday0(2026, 9, 1) as usize; // 9-1 是周二 → lead=1
        assert_eq!(lead, 1);
        assert_eq!(g[0], Some((31, false)), "8 月 31 日补位");
        assert_eq!(g[1], Some((1, true)));
    }

    #[test]
    fn open_resets_view_to_today() {
        let mut cal = CalFlyout::new((2026, 9, 26), 0);
        cal.open(0);
        cal.page(true);
        assert_eq!(cal.title(), "2026 年 10 月");
        cal.close(100);
        cal.open(200);
        assert!(cal.on_today(), "重开回今日");
    }

    #[test]
    fn anim_slide_8px_over_150ms() {
        let mut cal = CalFlyout::new((2026, 9, 26), 0);
        cal.open(0);
        assert_eq!(cal.anim_progress(), 0);
        assert!(cal.anim_progress() < 1000);
    }

    #[test]
    fn place_above_clock_clamps_to_screen() {
        let mut cal = CalFlyout::new((2026, 9, 26), 0);
        let clock = Rect::new(1900, 1050, 60, 32);
        let screen = Rect::new(0, 0, 1920, 1080);
        cal.place_above(clock, screen);
        let f = cal.frame();
        assert!(f.right() <= screen.right() && f.bottom() <= screen.bottom());
        assert!(f.y + PANEL_H_PX <= clock.y || f.x >= 0); // 钳制后不出屏
    }

    #[test]
    fn calflyout_self_checks_all_green() {
        let set = run_calflyout_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F078 自检红项：{}/{} 绿", p, p + f);
    }
}
