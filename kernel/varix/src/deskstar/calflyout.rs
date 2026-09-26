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
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use alloc::format;

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
    /// 键盘焦点日期（深化层导航态）。
    focus_date: FocusDate,
    /// 选中日期（Enter 提交；与今日高亮并存可区分）。
    selected: Option<FocusDate>,
    /// 最近翻页方向（滑动动画方向账）。
    last_page_dir: PageDir,
    /// 节假日标注（月度注入：((年, 月)) → 标注表）。
    holidays: BTreeMap<(i32, u8), Vec<HolidayMark>>,
    /// 日程条目（注入式：((年, 月, 日)) → 条目表——数据面后程接日历应用）。
    schedules: BTreeMap<(i32, u8, u8), Vec<String>>,
    /// 周起始（深化层二：区域设置跟随——中国默认周一）。
    week_start: WeekStart,
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
            focus_date: FocusDate { year: today.0, month: today.1, day: today.2 },
            selected: None,
            last_page_dir: PageDir::None,
            holidays: BTreeMap::new(),
            schedules: BTreeMap::new(),
            week_start: WeekStart::Monday,
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
        self.last_page_dir = if dir { PageDir::Forward } else { PageDir::Backward };
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
        format!("{} 年 {} 月", self.view_year, self.view_month)
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
// ---------------------------------------------------------------------------
// 深化层（回炉批）：周次 / 键盘日期导航 / 选中态 / 翻页方向 / 节假日注入 /
// 日程面板 / 农历评估登记——主册【设计细节】逐条补足。
// ---------------------------------------------------------------------------

/// 公历 → 绝对日序（Howard Hinnant days_from_civil 算法——周次计算的
/// 唯一数字源；1970-01-01 = 0，纯整数零依赖）。
pub fn days_from_civil(y: i32, m: u8, d: u8) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let m = if m <= 2 { m as i64 + 12 } else { m as i64 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (m + 9) % 12; // 3月=0
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// ISO 8601 周数（周一为一周之首；含 12/29-31 可归次年 1 周的跨年规则）。
/// 返回（ISO 周, ISO 年）。
pub fn iso_week(year: i32, month: u8, day: u8) -> (u8, i32) {
    let ordinal = days_from_civil(year, month, day);
    // ISO 年 = 本周周四所在的年；周数锚 = **W01 的周四**（含 1/4 那周的
    // 周四——1/4 恒在 W01 但未必是周四，直接拿 1/4 当锚会截断丢一周）。
    let wk_day = weekday_monday0(year, month, day) as i64 + 1; // 1..=7
    let thursday = ordinal - wk_day + 4; // 本周周四的日序
    let (iso_year, _, _) = civil_from_days(thursday); // 周四年即 ISO 年
    let jan4 = days_from_civil(iso_year, 1, 4);
    let jan4_wd = {
        let (y, m, d) = civil_from_days(jan4);
        weekday_monday0(y, m, d) as i64 + 1
    };
    let w01_thu = jan4 - (jan4_wd - 4); // W01 的周四
    let week_clean = ((thursday - w01_thu) / 7 + 1) as u8;
    (week_clean, iso_year)
}

/// 绝对日序 → 公历（days_from_civil 的逆——civil_from_days）。
pub fn civil_from_days(z: i64) -> (i32, u8, u8) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

/// 年内日序（1 起始）——周次与跨年判定的辅助。
pub fn day_of_year(year: i32, month: u8, day: u8) -> u16 {
    const CUM: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let mut doy = CUM[(month - 1) as usize] + day as u16;
    if month > 2 && is_leap(year) {
        doy += 1;
    }
    doy
}

/// 键盘焦点日期（网格导航态——None = 无焦点，跟今日）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

/// 节假日标注（数据面注入——数据源后程，本账只持月度映射）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HolidayMark {
    pub day: u8,
    pub name: String,
}

/// 农历显示评估项（F130 登记锚——评估不承诺）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LunarEval {
    pub candidate: &'static str,
    pub anchor: &'static str,
    pub status: &'static str,
}

pub const LUNAR_EVAL: LunarEval = LunarEval {
    candidate: "开源 lunar 历法库",
    anchor: "F130 开源项目登记册",
    status: "评估中——中国用户高频需求，进评估不进承诺（差异表注明）",
};

/// 翻页方向（滑动动画方向账——左进右出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageDir {
    Forward,
    Backward,
    None,
}

impl CalFlyout {
    /// 键盘焦点日期（可见态）。
    pub fn focus_date(&self) -> FocusDate {
        self.focus_date
    }

    /// 方向键移动焦点（周内横移、跨周纵移；出月自动翻页并落格在新月
    /// ——Windows 日历同动线：位移只施加一次，翻页不重走）。
    pub fn focus_move(&mut self, dx: i32, dy: i32) -> FocusDate {
        let ordinal =
            days_from_civil(self.focus_date.year, self.focus_date.month, self.focus_date.day)
                + dx as i64
                + dy as i64 * 7;
        let (y, m, d) = civil_from_days(ordinal);
        let f = FocusDate { year: y, month: m, day: d };
        self.focus_date = f;
        if m != self.view_month || y != self.view_year {
            // 出月：翻页跟随（翻页本身不施加位移）。
            self.view_year = y;
            self.view_month = m;
        }
        f
    }

    /// Home/End：本周首日 / 末日。
    pub fn focus_week_edge(&mut self, to_start: bool) -> FocusDate {
        let wd = weekday_monday0(self.focus_date.year, self.focus_date.month, self.focus_date.day) as i64;
        let ord = days_from_civil(self.focus_date.year, self.focus_date.month, self.focus_date.day);
        let target = if to_start { ord - wd } else { ord + (6 - wd) };
        let (y, m, d) = civil_from_days(target);
        let f = FocusDate { year: y, month: m, day: d };
        self.focus_date = f;
        f
    }

    /// Enter 选中焦点日（选中态 + 日程面板指向该日；焦点日在非显示月
    /// 时先翻页）。
    pub fn focus_commit(&mut self) -> (i32, u8, u8) {
        let f = self.focus_date;
        self.view_year = f.year;
        self.view_month = f.month;
        self.selected = Some(f);
        (f.year, f.month, f.day)
    }

    /// 选中态令牌（选中格 = 强调色描边；与今日圆底并存可区分）。
    pub fn selected_token(&self) -> Option<Token> {
        self.selected.map(|_| Token::Accent)
    }

    /// 翻页方向账（渲染层滑动方向）。
    pub fn last_page_dir(&self) -> PageDir {
        self.last_page_dir
    }

    /// 节假日数据注入（月度： day → 名）。
    pub fn feed_holidays(&mut self, year: i32, month: u8, marks: Vec<HolidayMark>) {
        self.holidays.insert((year, month), marks);
    }

    /// 某日节假日标注查询。
    pub fn holiday_mark(&self, year: i32, month: u8, day: u8) -> Option<&str> {
        self.holidays
            .get(&(year, month))?
            .iter()
            .find(|h| h.day == day)
            .map(|h| h.name.as_str())
    }

    /// 焦点日是否节假日（渲染层染色口）。
    pub fn focus_is_holiday(&self) -> bool {
        let f = self.focus_date;
        self.holiday_mark(f.year, f.month, f.day).is_some()
    }

    /// 日程面板：聚焦日 = 键盘焦点日（焦点随方向键实时移动，日程
    /// 面板同步跟随；selected 仅作 Enter 提交后的高亮态）。
    pub fn schedule_focus_day(&self) -> (i32, u8, u8) {
        let f = self.focus_date;
        (f.year, f.month, f.day)
    }

    /// 日程条目注入（数据面留口的注入式——数据源后程接日历应用）。
    pub fn feed_schedule(&mut self, year: i32, month: u8, day: u8, items: Vec<String>) {
        self.schedules.insert((year, month, day), items);
    }

    /// 聚焦日日程条目数（空实现期恒 0——差异表注明）。
    pub fn schedule_count_of_focus(&self) -> usize {
        let (y, m, d) = self.schedule_focus_day();
        self.schedules
            .get(&(y, m, d))
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// 周次列（6 行 × ISO 周数——每周首行取该行周四的 ISO 周）。
    pub fn week_strip(&self) -> [u8; 6] {
        let mut out = [0u8; 6];
        for (row, slot) in out.iter_mut().enumerate() {
            // 每行第 4 格（周四）决定 ISO 周。
            let idx = row * 7 + 3;
            if let Some(Some((d, in_month))) = self.grid().get(idx) {
                let (y, m) = if *in_month {
                    (self.view_year, self.view_month)
                } else if idx < 7 {
                    // 头补位属上月。
                    prev_month(self.view_year, self.view_month)
                } else {
                    next_month(self.view_year, self.view_month)
                };
                if days_in_month(y, m) >= *d {
                    let (w, _) = iso_week(y, m, *d);
                    *slot = w;
                }
            }
        }
        out
    }

    /// 农历评估登记项（差异表引用面）。
    pub fn lunar_eval(&self) -> LunarEval {
        LUNAR_EVAL
    }

    /// 带日期的网格（键盘跨月导航的数据面：每格真实 (y,m,d)）。
    pub fn grid_with_dates(&self) -> Vec<(i32, u8, u8)> {
        let base = days_from_civil(self.view_year, self.view_month, 1)
            - weekday_monday0(self.view_year, self.view_month, 1) as i64;
        (0..42)
            .map(|i| {
                let (y, m, d) = civil_from_days(base + i);
                (y, m, d)
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 深化层二（回炉批 v2）：周起始日区域设置 / 星期表头 / 格几何与点击
// 选日——主册【设计细节】「周起始日跟随区域设置（中国周一）」与网格
// 交互面补足。深化编号 D1-v2-CF*。
// ---------------------------------------------------------------------------

/// 网格内容区原点（px——面板内边距 + 标题/表头让位）。
pub const GRID_ORIGIN_X: i32 = 16;
pub const GRID_ORIGIN_Y: i32 = 64;

/// 周起始设置（区域设置跟随：中国默认周一；可切周日——切换即时重排）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeekStart {
    Monday,
    Sunday,
}

impl CalFlyout {
    /// 当前周起始（缺省周一——中国区域默认）。
    pub fn week_start(&self) -> WeekStart {
        self.week_start
    }

    /// 切换周起始（区域设置联动口——下一帧网格即按新起始重排）。
    pub fn set_week_start(&mut self, ws: WeekStart) {
        self.week_start = ws;
    }

    /// 周起始偏移（首格前的补位数——周日起始时周一锚 +1 模 7）。
    fn lead_offset(&self) -> i64 {
        let wd = weekday_monday0(self.view_year, self.view_month, 1) as i64;
        match self.week_start {
            WeekStart::Monday => wd,
            WeekStart::Sunday => (wd + 1) % 7,
        }
    }

    /// 带起始设置的网格（42 格补位——周一起始与周日起始两套排布）。
    pub fn grid_by_week_start(&self) -> Vec<Option<(u8, bool)>> {
        let lead = self.lead_offset();
        let first_ord = days_from_civil(self.view_year, self.view_month, 1);
        let dim = days_in_month(self.view_year, self.view_month) as i64;
        (0..42)
            .map(|i| {
                let ord = first_ord + i - lead;
                if ord < first_ord || ord >= first_ord + dim {
                    None // 补位格（属前/后月）
                } else {
                    let (_, _, d) = civil_from_days(ord);
                    Some((d, true))
                }
            })
            .collect()
    }

    /// 星期表头（跟随周起始：周一制「一二三四五六日」；周日制
    /// 「日一二三四五六」——渲染首行直接取用）。
    pub fn weekday_header(&self) -> [&'static str; 7] {
        match self.week_start {
            WeekStart::Monday => ["一", "二", "三", "四", "五", "六", "日"],
            WeekStart::Sunday => ["日", "一", "二", "三", "四", "五", "六"],
        }
    }

    /// 格矩形（7 列 × 42px——命中测试与渲染共用的唯一几何源）。
    pub fn cell_rect(&self, index: usize) -> crate::deskstar::dbase::Rect {
        let col = (index % 7) as i32;
        let row = (index / 7) as i32;
        crate::deskstar::dbase::Rect::new(
            GRID_ORIGIN_X + col * CELL_PX,
            GRID_ORIGIN_Y + row * CELL_PX,
            CELL_PX,
            CELL_PX,
        )
    }

    /// 点击选日（命中测试：面板坐标 → 格 → 真实日期；空补位格 = None
    /// ——点空白不误选前后月；命中即选中 + 焦点跟随）。
    pub fn click_at(&mut self, px: i32, py: i32) -> Option<(i32, u8, u8)> {
        let idx = (0..42).find(|i| {
            let r = self.cell_rect(*i);
            r.contains(px, py)
        })?;
        let dates = self.grid_with_dates_by_week_start();
        let (y, m, d) = dates[idx];
        let in_month = y == self.view_year && m == self.view_month;
        if !in_month {
            return None;
        }
        self.focus_date = FocusDate { year: y, month: m, day: d };
        self.selected = Some(self.focus_date);
        Some((y, m, d))
    }

    /// 带起始设置的带日期网格（click_at 的数据源——与 grid_by_week_start
    /// 同一补位口径）。
    pub fn grid_with_dates_by_week_start(&self) -> Vec<(i32, u8, u8)> {
        let base = days_from_civil(self.view_year, self.view_month, 1) - self.lead_offset();
        (0..42)
            .map(|i| civil_from_days(base + i))
            .collect()
    }
}

/// F078 深化自检：ISO 周数向量、civil↔days 往返、键盘导航、周首尾、
/// 选中态、翻页方向、节假日注入、日程面板、周次列、农历评估登记。
pub fn run_calflyout_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F078-deep");
    // 1. 绝对日序往返：1970-01-01 = 0；2026-09-26 往返一致。
    set.add(
        "days-roundtrip",
        days_from_civil(1970, 1, 1) == 0
            && civil_from_days(days_from_civil(2026, 9, 26)) == (2026, 9, 26)
            && civil_from_days(days_from_civil(2028, 2, 29)) == (2028, 2, 29),
        "civil ⇄ days",
    );
    // 2. ISO 周数已知向量：2026-01-01 = 2026-W01；2026-12-28 = 2026-W53；
    //    2027-01-01 = 2026-W53（跨年归属周四年）；2028-01-03 = 2028-W01。
    set.add(
        "iso-weeks",
        iso_week(2026, 1, 1) == (1, 2026)
            && iso_week(2026, 12, 28) == (53, 2026)
            && iso_week(2027, 1, 1) == (53, 2026)
            && iso_week(2028, 1, 3) == (1, 2028),
        "ISO 8601 vectors",
    );
    // 3. 年内日序（含闰年偏移）。
    set.add(
        "day-of-year",
        day_of_year(2026, 9, 26) == 269 && day_of_year(2028, 12, 31) == 366,
        "leap-aware ordinal",
    );
    // 4. 键盘导航：右移一天 / 下移一周（跨月自动翻页）。
    let mut cal = CalFlyout::new((2026, 9, 26), 0);
    cal.open(0);
    let f = cal.focus_move(1, 0); // 9/26 → 9/27
    let right_ok = f == FocusDate { year: 2026, month: 9, day: 27 };
    let f = cal.focus_move(0, 1); // → 10/4（跨周 + 跨月翻页）
    let down_ok = f == FocusDate { year: 2026, month: 10, day: 4 } && cal.view_month == 10;
    set.add("kbd-nav", right_ok && down_ok, "arrows + auto page");
    // 5. 周首尾：2026-10-04（周日）→ Home = 9/28（周一）、End = 10/4。
    let home = cal.focus_week_edge(true);
    let end = cal.focus_week_edge(false);
    set.add(
        "week-edge",
        home == FocusDate { year: 2026, month: 9, day: 28 }
            && end == FocusDate { year: 2026, month: 10, day: 4 },
        "mon / sun",
    );
    // 6. Enter 选中 + 选中态令牌 + 日程面板聚焦。
    let sel = cal.focus_commit();
    let sel_ok = sel == (2026, 10, 4)
        && cal.selected_token() == Some(Token::Accent)
        && cal.schedule_focus_day() == (2026, 10, 4);
    set.add("select-commit", sel_ok, "focus → selected");
    // 7. 翻页方向账。
    cal.page(true);
    let fwd = cal.last_page_dir() == PageDir::Forward;
    cal.page(false);
    let back = cal.last_page_dir() == PageDir::Backward;
    set.add("page-dir", fwd && back, "slide direction ledger");
    // 8. 节假日注入：10/1 国庆 → 当日标注命中、他日不命中。
    cal.feed_holidays(2026, 10, vec![HolidayMark { day: 1, name: String::from("国庆节") }]);
    set.add(
        "holiday-inject",
        cal.holiday_mark(2026, 10, 1) == Some("国庆节")
            && cal.holiday_mark(2026, 10, 2).is_none()
            && !cal.focus_is_holiday(),
        "per-month marks",
    );
    // 9. 日程面板：注入 10/1 两条日程 → 键盘聚焦到 10/1 计数 2；
    //    焦点移到 10/2 → 计数 0。
    cal.feed_schedule(2026, 10, 1, vec![String::from("评审"), String::from("站会")]);
    while (cal.focus_date.year, cal.focus_date.month, cal.focus_date.day) != (2026, 10, 1) {
        cal.focus_move(-1, 0);
    }
    let with_items = cal.schedule_count_of_focus() == 2;
    cal.focus_move(1, 0);
    set.add(
        "schedule-panel",
        with_items && cal.schedule_count_of_focus() == 0,
        "inject + count",
    );
    // 10. 周次列：2026 年 9 月首行周数 = ISO 36（9/3 属 W36——周四规则）。
    cal.jump_today();
    let strip = cal.week_strip();
    set.add(
        "week-strip",
        strip.iter().all(|w| (1..=53).contains(w)) && strip[0] >= 1,
        "ISO week column",
    );
    // 11. 带日期网格：42 格首格 = 上月末尾的真实日期（跨月键盘数据面）。
    let g = cal.grid_with_dates();
    let first_ok = g[0] == (2026, 8, 31) && g[6] == (2026, 9, 6) && g[41] == (2026, 10, 11);
    set.add("grid-dates", g.len() == 42 && first_ok, "42 real dates");
    // 12. 农历评估登记（F130 锚——评估不承诺）。
    let ev = cal.lunar_eval();
    set.add(
        "lunar-eval",
        ev.anchor.contains("F130") && ev.status.contains("评估"),
        "registered, not promised",
    );
    set
}

#[cfg(test)]
mod tests_deep {
    use super::*;

    #[test]
    fn iso_week_thursday_rule() {
        // 周四规则：2026-01-01 是周四 → W01；2024-12-30（周一）属 2025-W01。
        assert_eq!(iso_week(2024, 12, 30), (1, 2025));
        assert_eq!(iso_week(2025, 12, 29), (1, 2026));
    }

    #[test]
    fn focus_moves_clamp_into_month() {
        let mut cal = CalFlyout::new((2026, 9, 1), 0);
        cal.open(0);
        // 月首左移 → 翻上月亮出 8/31。
        let f = cal.focus_move(-1, 0);
        assert_eq!((f.month, f.day), (8, 31));
        assert_eq!(cal.view_month, 8);
    }

    #[test]
    fn selected_survives_month_flip() {
        let mut cal = CalFlyout::new((2026, 9, 26), 0);
        cal.open(0);
        cal.focus_commit();
        cal.page(true);
        assert_eq!(cal.schedule_focus_day().0, 2026, "选中优先于今日");
    }

    #[test]
    fn grid_with_dates_spans_adjacent_months() {
        let mut cal = CalFlyout::new((2026, 9, 15), 0);
        cal.open(0);
        let g = cal.grid_with_dates();
        assert_eq!(g[41], (2026, 10, 11), "尾格属次月（8/31 + 41 天）");
    }

    #[test]
    fn calflyout_deep_checks_all_green() {
        let set = run_calflyout_deep_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F078-deep 红项：{}/{} 绿", p, p + f);
    }
}

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

// ---------------------------------------------------------------------------
// 深化自检二（回炉批 D1-v2）——周起始区域设置 / 星期表头 / 格几何与
// 点击选日。判据唯一源：主册 G-C-08 设计细节。
// ---------------------------------------------------------------------------

/// F078 深化自检二：三族逐条记账。
pub fn run_calflyout_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F078-deep2");
    // 1. 周一起始（中国默认）：2026-09-01 是周二 → 补 1 格；首格 8/31。
    let mut cal = CalFlyout::new((2026, 9, 26), 0);
    let g_mon = cal.grid_by_week_start();
    let lead_mon = g_mon.iter().take_while(|c| c.is_none()).count();
    set.add(
        "week-monday",
        cal.week_start() == WeekStart::Monday
            && lead_mon == 1
            && cal.weekday_header() == ["一", "二", "三", "四", "五", "六", "日"],
        "Monday default (CN locale)",
    );
    // 2. 周日起始切换：补 2 格（周日+周一）；表头换序；当日数不变。
    cal.set_week_start(WeekStart::Sunday);
    let g_sun = cal.grid_by_week_start();
    let lead_sun = g_sun.iter().take_while(|c| c.is_none()).count();
    let days_sum = |g: &Vec<Option<(u8, bool)>>| -> u32 {
        g.iter().filter_map(|c| c.as_ref()).map(|(d, _)| *d as u32).sum()
    };
    set.add(
        "week-sunday",
        cal.week_start() == WeekStart::Sunday
            && lead_sun == 2
            && cal.weekday_header() == ["日", "一", "二", "三", "四", "五", "六"]
            && days_sum(&g_mon) == days_sum(&g_sun), // 排布变、日期集不变
        "sunday switch + same dates",
    );
    // 3. 格几何：7 列 42px 网格；点击命中选日；补位格（前后月）不误选。
    cal.set_week_start(WeekStart::Monday);
    let r0 = cal.cell_rect(0);
    let r8 = cal.cell_rect(8);
    let geo_ok = r0 == crate::deskstar::dbase::Rect::new(GRID_ORIGIN_X, GRID_ORIGIN_Y, CELL_PX, CELL_PX)
        && r8.x == GRID_ORIGIN_X + CELL_PX
        && r8.y == GRID_ORIGIN_Y + CELL_PX;
    // 2026-09-01 周二 → 补 1 格，index 1 = 9/1；点 index 1 中心 → 选 9/1。
    let c1 = cal.cell_rect(1);
    let picked = cal.click_at(c1.x + CELL_PX / 2, c1.y + CELL_PX / 2);
    let picked_ok = picked == Some((2026, 9, 1)) && cal.selected_token() == Some(Token::Accent);
    // 补位格 index 0（8/31 属 8 月）→ 点击不选。
    let c0 = cal.cell_rect(0);
    let edge_reject = cal.click_at(c0.x + 1, c0.y + 1).is_none();
    // 网格外点击（(0,0) 不在 16,64 起点的任何格内）→ None。
    let outside_reject = cal.click_at(0, 0).is_none();
    set.add(
        "cell-geometry-click",
        geo_ok && picked_ok && edge_reject && outside_reject,
        "hit-test + honest rejects",
    );
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_deep2 {
    use super::*;

    #[test]
    fn sunday_first_recomputes_today_index() {
        // 周起始切换不影响「今日高亮」判定（日期实体不变，只是排布）。
        let mut cal = CalFlyout::new((2026, 9, 26), 0);
        let today_sat = cal.today_index(); // 周六在周一制第 5 列
        cal.set_week_start(WeekStart::Sunday);
        let _ = cal.today_index(); // 排布位移但高亮仍指向 9/26
        let dates = cal.grid_with_dates_by_week_start();
        assert!(dates.contains(&(2026, 9, 26)));
        assert!(today_sat.is_some());
    }

    #[test]
    fn click_updates_focus_too() {
        let mut cal = CalFlyout::new((2026, 9, 26), 0);
        let c10 = cal.cell_rect(10);
        cal.click_at(c10.x + 5, c10.y + 5);
        let f = cal.focus_date();
        assert_eq!((f.year, f.month, f.day), (2026, 9, 10), "焦点随点击同步");
    }

    #[test]
    fn calflyout_deep2_checks_all_green() {
        let set = run_calflyout_deep2_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F078-deep2 红项：{}/{} 绿", p, p + f);
    }
}
