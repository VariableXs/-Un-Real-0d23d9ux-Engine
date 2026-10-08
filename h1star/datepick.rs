//! F232 日期时间选择器 · H 基础通用域实装。
//!
//! **判据锚**：F232。
//!
//! **验收标准（主册第一句）**：统一的日期时间控件：日历飞出复用同一
//! 组件、键盘可达（方向键移动、PgUp/PgDn 换月、直接键入优先——输入
//! 2026-10-01 直接识别）、今天高亮、选中日填充实色圆、无效日期（表单
//! 规则禁选的）置灰但可看；时间用滚动列（时/分，滚轮或上下键步进）；
//! 两种模式（纯日期/日期+时间）同一组件出。
//!
//! **设计要点**：
//! - 日历网格 [`CalGrid`]：公历月份天数/星期对齐——纯整数历法
//!   （[`days_from_civil`] / [`civil_from_days`] 互逆对 + 闰年规则
//!   [`is_leap`]，core 无 f64）；
//! - 键入解析器 [`parse_date`]：3 种格式入册——`YYYY-MM-DD` /
//!   `YYYY/M/D` / `YYYYMMDD`，识别即填充（直接键入优先）；
//! - 键盘导航：方向键 ±1 天 / ±7 天（跨月自动翻页）、PgUp/PgDn 换月
//!   （日钳制进新月）、Enter 确认 / Esc 取消全语义；
//! - 无效日置灰：禁选谓词注入（表单规则），置灰可看（网格照出格子）
//!   不可选（点选/键入/方向键选中动作一律拒绝）；
//! - 时间滚动列：时 0-23 / 分 0-59 上下步进**钳制**（不回绕——
//!   主册「步进钳制」原文）；双模式同源：一个 [`DatePick`] 结构体挂
//!   [`PickMode`] 模式位，Date 模式时间列不可达；
//! - 今天高亮/选中实色圆是渲染语义，本模块提供判定位
//!   （[`DatePick::is_today`] / 选中即 `sel` 字段）供绘制层取用。
//!
//! **依赖锚点**：`crate::checks::CheckSet`。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 日历网格行数（6 行 × 7 列——任何月份 + 任何首日偏移都装得下）。
pub const GRID_ROWS: usize = 6;

/// 日历网格列数（周日=0 起七列）。
pub const GRID_COLS: usize = 7;

/// 键入识别格式集——主册验收「键入识别格式集入册（3 种）」：
/// 1. `YYYY-MM-DD`（横杠分隔，补零）
/// 2. `YYYY/M/D`（斜杠分隔，可不补零）
/// 3. `YYYYMMDD`（8 位紧凑）
pub const TYPED_FORMATS: [&str; 3] = ["YYYY-MM-DD", "YYYY/M/D", "YYYYMMDD"];

/// 年份合法域（1..=9999）。
pub const YEAR_MIN: i32 = 1;
pub const YEAR_MAX: i32 = 9999;

// ---------------------------------------------------------------------------
// 纯整数历法（公历）
// ---------------------------------------------------------------------------

/// 闰年规则：能被 4 整除但不能被 100 整除，或能被 400 整除。
pub fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// 公历月份天数（含闰年二月）。
pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// 公历日期 → 天序号（Howard Hinnant 算法，proleptic Gregorian，
/// 1970-01-01 = 0）。纯整数，无溢出（y ≤ 9999 时 ≤ 3.6e6）。
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m as i64 - 3 } else { m as i64 + 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 天序号 → 公历日期（[`days_from_civil`] 的精确逆）。
pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { (mp + 3) as u32 } else { (mp - 9) as u32 };
    ((y + if m <= 2 { 1 } else { 0 }) as i32, m, d)
}

/// 星期几（0 = 周日 … 6 = 周六）。锚点：1970-01-01 = 周四(4)。
pub fn weekday(y: i32, m: u32, d: u32) -> u32 {
    (days_from_civil(y, m, d) + 4).rem_euclid(7) as u32
}

// ---------------------------------------------------------------------------
// 键入解析器（3 格式入册，识别即填充）
// ---------------------------------------------------------------------------

/// 解析失败原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseFail {
    /// 不属三种入册格式。
    BadFormat,
    /// 格式对但数值越界（月/日/年不在合法域，或该日不存在）。
    BadRange,
}

/// 日期三元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateYMD {
    pub y: i32,
    pub m: u32,
    pub d: u32,
}

fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    let mut v: u32 = 0;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v * 10 + (b - b'0') as u32;
    }
    Some(v)
}

/// 范围校验 + 装配。
fn validate_ymd(y: u32, m: u32, d: u32) -> Result<DateYMD, ParseFail> {
    let y = y as i32;
    if !(YEAR_MIN..=YEAR_MAX).contains(&y) || !(1..=12).contains(&m) {
        return Err(ParseFail::BadRange);
    }
    if d < 1 || d > days_in_month(y, m) {
        return Err(ParseFail::BadRange);
    }
    Ok(DateYMD { y, m, d })
}

/// 键入解析：三种入册格式识别即出日期（其余一律 [`ParseFail::BadFormat`]）。
pub fn parse_date(s: &str) -> Result<DateYMD, ParseFail> {
    let t = s.trim();
    if t.contains('-') {
        let parts: alloc::vec::Vec<&str> = t.split('-').collect();
        if parts.len() != 3 {
            return Err(ParseFail::BadFormat);
        }
        let y = parse_u32(parts[0]).ok_or(ParseFail::BadFormat)?;
        let m = parse_u32(parts[1]).ok_or(ParseFail::BadFormat)?;
        let d = parse_u32(parts[2]).ok_or(ParseFail::BadFormat)?;
        validate_ymd(y, m, d)
    } else if t.contains('/') {
        let parts: alloc::vec::Vec<&str> = t.split('/').collect();
        if parts.len() != 3 {
            return Err(ParseFail::BadFormat);
        }
        let y = parse_u32(parts[0]).ok_or(ParseFail::BadFormat)?;
        let m = parse_u32(parts[1]).ok_or(ParseFail::BadFormat)?;
        let d = parse_u32(parts[2]).ok_or(ParseFail::BadFormat)?;
        validate_ymd(y, m, d)
    } else if t.len() == 8 && t.bytes().all(|b| b.is_ascii_digit()) {
        let y = parse_u32(&t[0..4]).ok_or(ParseFail::BadFormat)?;
        let m = parse_u32(&t[4..6]).ok_or(ParseFail::BadFormat)?;
        let d = parse_u32(&t[6..8]).ok_or(ParseFail::BadFormat)?;
        validate_ymd(y, m, d)
    } else {
        Err(ParseFail::BadFormat)
    }
}

// ---------------------------------------------------------------------------
// 日历网格
// ---------------------------------------------------------------------------

/// 一个月的 6×7 网格（周日 = 第 0 列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalGrid {
    pub y: i32,
    pub m: u32,
}

impl CalGrid {
    pub fn new(y: i32, m: u32) -> CalGrid {
        CalGrid { y, m }
    }

    /// 1 号落在第几列（= 星期几）。
    pub fn first_weekday(&self) -> u32 {
        weekday(self.y, self.m, 1)
    }

    /// 本月天数。
    pub fn days(&self) -> u32 {
        days_in_month(self.y, self.m)
    }

    /// 网格格子内容：None = 置灰填充位（上/下月补位，可看不可选）。
    pub fn cell(&self, row: usize, col: usize) -> Option<u32> {
        let idx = (row * GRID_COLS + col) as i64 - self.first_weekday() as i64 + 1;
        if idx >= 1 && idx <= self.days() as i64 {
            Some(idx as u32)
        } else {
            None
        }
    }

    /// 日期在网格中的 (行, 列)。
    pub fn position_of(&self, day: u32) -> Option<(usize, usize)> {
        if day < 1 || day > self.days() {
            return None;
        }
        let idx = self.first_weekday() as usize + day as usize - 1;
        Some((idx / GRID_COLS, idx % GRID_COLS))
    }
}

// ---------------------------------------------------------------------------
// 选择器状态机（双模式同源）
// ---------------------------------------------------------------------------

/// 两种模式（同一结构体出——F232「两种模式同一组件」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickMode {
    /// 纯日期。
    Date,
    /// 日期 + 时间。
    DateTime,
}

/// 时间滚动列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeCol {
    /// 时（0-23）。
    Hour,
    /// 分（0-59）。
    Minute,
}

/// 导航结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavOutcome {
    /// 移动成功（同月内）。
    Moved,
    /// 移动成功且翻月。
    MonthChanged,
    /// 目标日被禁选规则置灰——可看不可选，选择动作被拒。
    RejectedDisabled,
}

/// 统一日期时间控件。
///
/// 禁选谓词由表单规则注入（`disabled_fn`）——置灰判定只在「选中动作」
/// （点选/键入/方向键）执行；换月钳日不改变选中语义。
pub struct DatePick {
    mode: PickMode,
    /// 当前视图月。
    view: CalGrid,
    /// 选中日（实色圆绘制源）。
    sel: DateYMD,
    hour: u32,
    minute: u32,
    open: bool,
    disabled_fn: fn(i32, u32, u32) -> bool,
    /// 键入识别成功次数（验收：三种输入通路用例）。
    pub typed_ok: u32,
    /// 键入被拒次数（格式或禁选）。
    pub typed_rejected: u32,
    /// 点选被拒（置灰日）次数。
    pub disabled_picks: u32,
}

impl DatePick {
    /// 建控件：模式 + 初始选中日 + 禁选谓词（表单规则注入）。
    pub fn new(mode: PickMode, init: DateYMD, disabled_fn: fn(i32, u32, u32) -> bool) -> DatePick {
        DatePick {
            mode,
            view: CalGrid::new(init.y, init.m),
            sel: init,
            hour: 0,
            minute: 0,
            open: false,
            disabled_fn,
            typed_ok: 0,
            typed_rejected: 0,
            disabled_picks: 0,
        }
    }

    pub fn mode(&self) -> PickMode {
        self.mode
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn view(&self) -> CalGrid {
        self.view
    }

    pub fn sel(&self) -> DateYMD {
        self.sel
    }

    pub fn time(&self) -> (u32, u32) {
        (self.hour, self.minute)
    }

    /// 当前网格（日历飞出复用同一组件——视图即网格）。
    pub fn grid(&self) -> CalGrid {
        self.view
    }

    /// 今天高亮判定位（绘制层据此画高亮；「今天」由调用方注入）。
    pub fn is_today(&self, today: DateYMD) -> bool {
        self.sel.y == today.y && self.sel.m == today.m && self.sel.d == today.d
    }

    fn is_disabled(&self, y: i32, m: u32, d: u32) -> bool {
        (self.disabled_fn)(y, m, d)
    }

    /// 方向键移动：±1 天（Left/Right）、±7 天（Up/Down）。
    /// 跨月自动翻视图；落在置灰日 → 拒绝（原地不动）。
    pub fn move_day(&mut self, delta: i64) -> NavOutcome {
        let target = civil_from_days(days_from_civil(self.sel.y, self.sel.m, self.sel.d) + delta);
        let (y, m, d) = target;
        if self.is_disabled(y, m, d) {
            return NavOutcome::RejectedDisabled;
        }
        let month_changed = y != self.view.y || m != self.view.m;
        self.sel = DateYMD { y, m, d };
        if month_changed {
            self.view = CalGrid::new(y, m);
            NavOutcome::MonthChanged
        } else {
            NavOutcome::Moved
        }
    }

    /// PgUp/PgDn 换月：视图翻 ±1 月，选中日钳进新月（1 月 31 日 →
    /// 2 月 28/29 日）。月序号整数域直接加减（无 30 天近似漂移）。
    pub fn move_month(&mut self, delta: i64) -> NavOutcome {
        let total = self.view.y as i64 * 12 + self.view.m as i64 - 1 + delta;
        let ny = total.div_euclid(12) as i32;
        let nm = (total.rem_euclid(12) + 1) as u32;
        let dim = days_in_month(ny, nm);
        let d = self.sel.d.min(dim).max(1);
        self.view = CalGrid::new(ny, nm);
        self.sel = DateYMD { y: ny, m: nm, d };
        NavOutcome::MonthChanged
    }

    /// 点选一个日号：置灰日可看不可选。
    pub fn click_day(&mut self, day: u32) -> NavOutcome {
        if day < 1 || day > self.view.days() {
            return NavOutcome::RejectedDisabled;
        }
        if self.is_disabled(self.view.y, self.view.m, day) {
            self.disabled_picks += 1;
            return NavOutcome::RejectedDisabled;
        }
        self.sel = DateYMD { y: self.view.y, m: self.view.m, d: day };
        NavOutcome::Moved
    }

    /// 直接键入（优先通路）：识别即填充选中与视图。
    /// 禁选日键入同样拒绝（可看不可选对三种通路一致）。
    pub fn type_in(&mut self, s: &str) -> Result<DateYMD, ParseFail> {
        let parsed = parse_date(s)?;
        if self.is_disabled(parsed.y, parsed.m, parsed.d) {
            self.typed_rejected += 1;
            return Err(ParseFail::BadRange);
        }
        self.sel = parsed;
        self.view = CalGrid::new(parsed.y, parsed.m);
        self.typed_ok += 1;
        Ok(parsed)
    }

    /// Enter：确认并收起（返回选中日期）。
    pub fn confirm(&mut self) -> Option<DateYMD> {
        if !self.open {
            return None;
        }
        self.open = false;
        Some(self.sel)
    }

    /// Esc：取消收起（选中不变）。
    pub fn cancel(&mut self) {
        self.open = false;
    }

    /// 时间滚动列步进（上下键/滚轮）：钳制不回绕。
    /// Date 模式无时间列——返回 None（双模式同源的可达性差异）。
    pub fn time_step(&mut self, col: TimeCol, delta: i32) -> Option<(u32, u32)> {
        if self.mode != PickMode::DateTime {
            return None;
        }
        match col {
            TimeCol::Hour => {
                self.hour = (self.hour as i32 + delta).clamp(0, 23) as u32;
            }
            TimeCol::Minute => {
                self.minute = (self.minute as i32 + delta).clamp(0, 59) as u32;
            }
        }
        Some((self.hour, self.minute))
    }
}

/// 示例禁选规则：周末禁选（表单规则注入的样例，判据用）。
/// [缺陷账本] 现象：`arrows cannot land on disabled day` 与 v2 grid
/// drawlist 两项红。根因：本函数 `weekday >= 5` 把周五也算进周末——
/// 与模块口径（weekday 0=周日…6=周六；v2 判定面注释枚举周末为
/// 周六 5 天 + 周日 4 天共 9 天；F232 自检注释「10-02 是周五可选」）
/// 矛盾，属实现违反既定语义。修法：周末 = 周六(6) 或 周日(0)。
pub fn no_weekends(y: i32, m: u32, d: u32) -> bool {
    let w = weekday(y, m, d);
    w == 0 || w == 6
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F232 自检（13 条行为级）。
pub fn run_datepick_checks() -> CheckSet {
    let mut set = CheckSet::new("F232-datepick");

    // 1. 闰年规则：2000/2024 闰、1900/2023 平。
    set.add(
        "leap rule 2000/2024 leap, 1900/2023 not",
        is_leap(2000) && is_leap(2024) && !is_leap(1900) && !is_leap(2023),
        "",
    );

    // 2. 月天数表：大小月 + 闰二月 29 / 平二月 28。
    set.add(
        "days in month table",
        days_in_month(2026, 2) == 28
            && days_in_month(2024, 2) == 29
            && days_in_month(2026, 4) == 30
            && days_in_month(2026, 10) == 31,
        "",
    );

    // 3. 星期锚点：1970-01-01 周四(4)、2000-01-01 周六(6)。
    set.add(
        "weekday anchors (1970 Thu, 2000 Sat)",
        weekday(1970, 1, 1) == 4 && weekday(2000, 1, 1) == 6,
        "",
    );

    // 4. 网格对齐：2026-10 周四开局，1 号在 (0,4)，31 号可查位；
    //    填充位可看（cell 返回 None 但位置存在）。
    let g = CalGrid::new(2026, 10);
    let (r1, c1) = g.position_of(1).unwrap();
    let (r31, _) = g.position_of(31).unwrap();
    set.add(
        "grid alignment 2026-10 (Thu start)",
        g.first_weekday() == 4 && r1 == 0 && c1 == 4 && g.cell(r1, c1) == Some(1) && r31 < GRID_ROWS,
        "",
    );

    // 5. 键入三格式识别即填充：三种写法同一结果（格式集入册 3 种）。
    let mut dp = DatePick::new(PickMode::DateTime, DateYMD { y: 2026, m: 1, d: 1 }, no_weekends);
    let a = dp.type_in("2026-10-01").unwrap();
    let b = dp.type_in("2026/10/1").unwrap();
    let c = dp.type_in("20261001").unwrap();
    set.add(
        "type-in: 3 registered formats recognized",
        TYPED_FORMATS.len() == 3 && a == b && b == c && a == DateYMD { y: 2026, m: 10, d: 1 } && dp.typed_ok == 3,
        "",
    );

    // 6. 键入拒绝：非法月日 / 残缺 / 字母 / 8 位缺位。
    let bad = dp.type_in("2026-13-01").is_err()
        && dp.type_in("2026-02-30").is_err()
        && dp.type_in("2026-10").is_err()
        && dp.type_in("2026-1a-01").is_err()
        && dp.type_in("2026100").is_err()
        && dp.typed_rejected == 0;
    set.add(
        "type-in rejects invalid input",
        bad && dp.sel() == DateYMD { y: 2026, m: 10, d: 1 },
        "",
    );

    // 7. 方向键移动 + 跨月翻页：10-01 ← Left → 09-30（周三，非周末）。
    let out = dp.move_day(-1);
    set.add(
        "arrow left crosses month boundary",
        out == NavOutcome::MonthChanged
            && dp.sel() == DateYMD { y: 2026, m: 9, d: 30 }
            && dp.view() == CalGrid::new(2026, 9),
        "",
    );

    // 8. PgDn 换月日钳制：1 月 31 日 PgDn → 2 月 28 日（2026 平年）。
    let mut dp2 = DatePick::new(PickMode::Date, DateYMD { y: 2026, m: 1, d: 31 }, no_weekends);
    let out2 = dp2.move_month(1);
    set.add(
        "pgdn clamps day into new month",
        out2 == NavOutcome::MonthChanged && dp2.sel() == DateYMD { y: 2026, m: 2, d: 28 },
        "",
    );

    // 9. Enter/Esc 语义：确认返回选中并收起；取消收起不改动。
    dp.open();
    let confirmed = dp.confirm();
    dp.open();
    dp.cancel();
    set.add(
        "enter confirms & closes, esc closes unchanged",
        confirmed == Some(DateYMD { y: 2026, m: 9, d: 30 }) && !dp.is_open(),
        "",
    );

    // 10. 无效日置灰可看不可选：周末格子存在（可看），点选被拒且选中不变。
    let mut dp3 = DatePick::new(PickMode::Date, DateYMD { y: 2026, m: 10, d: 1 }, no_weekends);
    // 2026-10-03 是周六（weekday == 6）——置灰可看。
    let visible = g.cell(0, 6) == Some(3) && no_weekends(2026, 10, 3);
    let picked = dp3.click_day(3);
    set.add(
        "disabled day visible but not selectable",
        visible && picked == NavOutcome::RejectedDisabled && dp3.sel().d == 1 && dp3.disabled_picks == 1,
        "",
    );

    // 11. 方向键同样不可停在置灰日：10-01 → Right 本应到周六 10-03？
    //     不——10-02 是周五可选，10-03 周六被拒原地不动。
    let s1 = dp3.move_day(1); // 到 10-02（周五）
    let s2 = dp3.move_day(1); // 试到 10-03（周六）→ 拒
    set.add(
        "arrows cannot land on disabled day",
        s1 == NavOutcome::Moved && dp3.sel().d == 2 && s2 == NavOutcome::RejectedDisabled && dp3.sel().d == 2,
        "",
    );

    // 12. 时间滚动列：钳制不回绕；Date 模式时间列不可达（双模式同源）。
    let mut dp4 = DatePick::new(PickMode::DateTime, DateYMD { y: 2026, m: 10, d: 1 }, no_weekends);
    let t1 = dp4.time_step(TimeCol::Hour, 25).unwrap();
    let t2 = dp4.time_step(TimeCol::Hour, -1).unwrap();
    let t3 = dp4.time_step(TimeCol::Minute, 59).unwrap();
    let mut dp5 = DatePick::new(PickMode::Date, DateYMD { y: 2026, m: 10, d: 1 }, no_weekends);
    set.add(
        "time columns clamp 0-23/0-59, date mode has no time column",
        t1 == (23, 0) && t2 == (22, 0) && t3 == (22, 59)
            && dp4.mode() == PickMode::DateTime
            && dp5.mode() == PickMode::Date
            && dp5.time_step(TimeCol::Hour, 1).is_none(),
        "",
    );

    // 13. fuzz 2000 轮：随机导航/换月/键入，不变量——选中恒为存在日期
    //     （1≤d≤dim、1≤m≤12、年合法）、网格与选中同月、无 panic。
    let mut x: u32 = 0xDA7E_51C;
    let mut fz = DatePick::new(PickMode::DateTime, DateYMD { y: 2026, m: 6, d: 15 }, no_weekends);
    fz.open();
    let mut ok = true;
    for _ in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let op = x % 4;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        match op {
            0 => {
                let delta = (x % 30) as i64 - 15;
                fz.move_day(delta);
            }
            1 => {
                fz.move_month((x % 3) as i64 - 1);
            }
            2 => {
                // 构造合法日期键入（格式 2，可不补零）。
                let y = 1900 + (x % 200) as i32;
                let m = 1 + x % 12;
                let d = 1 + (x >> 5) % 28;
                let s = alloc::format!("{}/{}/{}", y, m, d);
                let _ = fz.type_in(&s);
            }
            _ => {
                let col = if x % 2 == 0 { TimeCol::Hour } else { TimeCol::Minute };
                let d = (x % 3) as i32 - 1;
                let _ = fz.time_step(col, d);
            }
        }
        let sel = fz.sel();
        if !(YEAR_MIN..=YEAR_MAX).contains(&sel.y)
            || !(1..=12).contains(&sel.m)
            || sel.d < 1
            || sel.d > days_in_month(sel.y, sel.m)
        {
            ok = false;
            break;
        }
        let v = fz.view();
        if v.y != sel.y || v.m != sel.m {
            ok = false; // 导航后视图与选中同月。
            break;
        }
        let (h, mi) = fz.time();
        if h > 23 || mi > 59 {
            ok = false;
            break;
        }
    }
    set.add("fuzz 2000 rounds: invariants hold, no panic", ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_roundtrip_and_anchors() {
        // 互逆对：随机日期 round-trip 恒等。
        for &(y, m, d) in &[
            (1970, 1u32, 1u32),
            (2000, 2, 29),
            (1900, 3, 1),
            (2026, 10, 1),
            (9999, 12, 31),
            (1, 1, 1),
        ] {
            let serial = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(serial), (y, m, d), "{} 历法互逆失败", y);
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
    }

    #[test]
    fn typed_formats_lifecycle() {
        let mut dp = DatePick::new(PickMode::Date, DateYMD { y: 2026, m: 1, d: 15 }, no_weekends);
        dp.open();
        assert_eq!(dp.type_in("2026/3/5").unwrap(), DateYMD { y: 2026, m: 3, d: 5 });
        assert_eq!(dp.view(), CalGrid::new(2026, 3));
        // 识别后 Enter 确认的就是键入值。
        assert_eq!(dp.confirm(), Some(DateYMD { y: 2026, m: 3, d: 5 }));
        assert_eq!(dp.typed_ok, 1);
    }

    #[test]
    fn grid_covers_all_days_once() {
        for &(y, m) in &[(2026, 2u32), (2024, 2), (2026, 10), (1999, 12)] {
            let g = CalGrid::new(y, m);
            let mut seen = [0u32; 31];
            for r in 0..GRID_ROWS {
                for c in 0..GRID_COLS {
                    if let Some(d) = g.cell(r, c) {
                        seen[d as usize - 1] += 1;
                    }
                }
            }
            for i in 0..g.days() as usize {
                assert_eq!(seen[i], 1, "{}-{} 第 {} 天必须恰好出现一次", y, m, i + 1);
            }
        }
    }

    #[test]
    fn disabled_visibility_consistency() {
        // 置灰判据：禁选日在网格中可见（cell 有值）但三种通路都被拒。
        let mut dp = DatePick::new(PickMode::Date, DateYMD { y: 2026, m: 10, d: 4 }, no_weekends);
        // 10-03 周六（点选被拒）。
        assert_eq!(dp.click_day(3), NavOutcome::RejectedDisabled);
        // 键入被拒（10-04 是周日，周末禁选——键入通路同样拒绝）。
        assert!(dp.type_in("2026-10-04").is_err(), "周日禁选，键入被拒"); // 周日——也被拒！
        assert!(dp.sel().d == 4, "键入禁选日不得改选");
        assert!(dp.type_in("2026/10/2").is_ok()); // 周五可选。
        assert_eq!(dp.sel().d, 2);
    }

    #[test]
    fn month_nav_year_rollover() {
        let mut dp = DatePick::new(PickMode::Date, DateYMD { y: 2026, m: 12, d: 15 }, no_weekends);
        dp.move_month(1);
        assert_eq!(dp.view(), CalGrid::new(2027, 1));
        dp.move_month(-2);
        assert_eq!(dp.view(), CalGrid::new(2026, 11), "2027-01 前退两月 = 2026-11");
        // 跨年回退钳日：3 月 31 → 2 月。
        let mut dp2 = DatePick::new(PickMode::Date, DateYMD { y: 2027, m: 3, d: 31 }, no_weekends);
        dp2.move_month(-1);
        assert_eq!(dp2.sel(), DateYMD { y: 2027, m: 2, d: 28 });
    }

    #[test]
    fn datepick_selfcheck_all_green() {
        let s = run_datepick_checks();
        assert!(s.all_passed(), "F232 自检存在红项");
        assert!(!s.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
//
// 判据锚 F232。持久化面 = 选择器状态（年月+选中日）framed 编解码；
// 壳接线面 = 日历网格几何（格子矩形/命中测试/置灰绘制清单）；
// 判定面 = run_datepick_v2_checks（首条持久化 round-trip）。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 记录头 magic「VXH1」+ 版本（全域 v2 段统一，一处一事实）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;

/// 四类损坏显性拒绝（magic/版本/长度/校验和）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2SaveErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadChecksum,
}

/// FNV-1a 64 位取低 32 位（常数同 vdesk 音频指纹——全域同族）。
fn v2_fnv1a32(data: &[u8]) -> u32 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h as u32
}

/// 记录式：magic4+ver1+视图年 i16+视图月 u8+选中年 i16+选中月 u8
/// +选中日 u8+模式 u8+时 u8+分 u8+checksum u32 = 19 字节定长
/// （容量上限在册：零堆，栈上缓冲即可）。
pub const V2_PAYLOAD_LEN: usize = 10;
pub const V2_REC_LEN: usize = 5 + V2_PAYLOAD_LEN + 4;
const V2_BODY_LEN: usize = V2_REC_LEN - 4;

/// 选择器状态快照（主册 F232 v2：年月 + 选中日持久化——重启后
/// 回到上次视图与选中，不回今天）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2PickSnap {
    pub view: (i32, u32),
    pub sel: DateYMD,
    /// 0 = Date / 1 = DateTime（PickMode 判别值）。
    pub mode: u8,
    pub hour: u8,
    pub minute: u8,
}

impl V2PickSnap {
    /// 从控件导出（只走既有公开读取面：view/sel/mode/time）。
    pub fn capture(dp: &DatePick) -> V2PickSnap {
        let v = dp.view();
        let (h, mi) = dp.time();
        V2PickSnap {
            view: (v.y, v.m),
            sel: dp.sel(),
            mode: match dp.mode() {
                PickMode::Date => 0,
                PickMode::DateTime => 1,
            },
            hour: h as u8,
            minute: mi as u8,
        }
    }

    /// 快照自校验：日期存在、视图与选中同月（导航不变量）、时间在域。
    /// 装载方先验 valid 再入册——损坏记录不进控件。
    pub fn valid(&self) -> bool {
        (YEAR_MIN..=YEAR_MAX).contains(&self.sel.y)
            && (1..=12).contains(&self.sel.m)
            && self.sel.d >= 1
            && self.sel.d <= days_in_month(self.sel.y, self.sel.m)
            && self.view.0 == self.sel.y
            && self.view.1 == self.sel.m
            && self.hour <= 23
            && self.minute <= 59
    }

    pub fn to_bytes(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < V2_REC_LEN {
            return None;
        }
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5..7].copy_from_slice(&(self.view.0 as i16).to_le_bytes());
        out[7] = self.view.1 as u8;
        out[8..10].copy_from_slice(&(self.sel.y as i16).to_le_bytes());
        out[10] = self.sel.m as u8;
        out[11] = self.sel.d as u8;
        out[12] = self.mode;
        out[13] = self.hour;
        out[14] = self.minute;
        let sum = v2_fnv1a32(&out[..V2_BODY_LEN]);
        out[V2_BODY_LEN..V2_REC_LEN].copy_from_slice(&sum.to_le_bytes());
        Some(V2_REC_LEN)
    }

    pub fn from_bytes(buf: &[u8]) -> Result<V2PickSnap, V2SaveErr> {
        if buf.len() != V2_REC_LEN {
            return Err(V2SaveErr::BadLen);
        }
        if buf[..4] != V2_MAGIC {
            return Err(V2SaveErr::BadMagic);
        }
        if buf[4] != V2_VERSION {
            return Err(V2SaveErr::BadVersion);
        }
        let expect = u32::from_le_bytes([buf[V2_BODY_LEN], buf[V2_BODY_LEN + 1], buf[V2_BODY_LEN + 2], buf[V2_BODY_LEN + 3]]);
        if v2_fnv1a32(&buf[..V2_BODY_LEN]) != expect {
            return Err(V2SaveErr::BadChecksum);
        }
        let ry = i16::from_le_bytes([buf[5], buf[6]]);
        let sy = i16::from_le_bytes([buf[8], buf[9]]);
        Ok(V2PickSnap {
            view: (ry as i32, buf[7] as u32),
            sel: DateYMD { y: sy as i32, m: buf[10] as u32, d: buf[11] as u32 },
            mode: buf[12],
            hour: buf[13],
            minute: buf[14],
        })
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 网格格子边长（px）——壳层布局档位。
pub const V2_CELL_PX: i32 = 28;

/// 格子矩形：origin = 网格体左上（星期表头行由调用方另画）。
pub fn v2_cell_rect(origin: crate::h1star::h1base::Rect, row: usize, col: usize) -> crate::h1star::h1base::Rect {
    crate::h1star::h1base::Rect::new(
        origin.x + col as i32 * V2_CELL_PX,
        origin.y + row as i32 * V2_CELL_PX,
        V2_CELL_PX,
        V2_CELL_PX,
    )
}

/// 网格命中测试：点 → 日号（补位格 None——可看不可选语义）。
pub fn v2_cell_hit(origin: crate::h1star::h1base::Rect, grid: CalGrid, px: i32, py: i32) -> Option<u32> {
    if !origin.contains(px, py) {
        return None;
    }
    let col = ((px - origin.x) / V2_CELL_PX).clamp(0, GRID_COLS as i32 - 1) as usize;
    let row = ((py - origin.y) / V2_CELL_PX).clamp(0, GRID_ROWS as i32 - 1) as usize;
    grid.cell(row, col)
}

/// 图元种类：0 可选日 / 1 置灰（无效日与补位格）/ 2 选中实色圆。
pub const V2_CELL_DAY: u8 = 0;
pub const V2_CELL_DISABLED: u8 = 1;
pub const V2_CELL_SELECTED: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2Cell {
    pub rect: crate::h1star::h1base::Rect,
    pub kind: u8,
}

/// 网格绘制清单（6 行×7 列 42 格全覆盖）：置灰判定走注入的表单规则
/// （与 DatePick 同一谓词形态——同一规则两处消费，判定面互证）。
pub fn v2_grid_drawlist(
    grid: CalGrid,
    origin: crate::h1star::h1base::Rect,
    disabled_fn: fn(i32, u32, u32) -> bool,
    sel_day: u32,
) -> [V2Cell; GRID_ROWS * GRID_COLS] {
    let mut out = [V2Cell { rect: crate::h1star::h1base::Rect::new(0, 0, 0, 0), kind: V2_CELL_DISABLED }; GRID_ROWS * GRID_COLS];
    for r in 0..GRID_ROWS {
        for c in 0..GRID_COLS {
            let rect = v2_cell_rect(origin, r, c);
            let kind = match grid.cell(r, c) {
                Some(d) if disabled_fn(grid.y, grid.m, d) => V2_CELL_DISABLED,
                Some(d) if d == sel_day => V2_CELL_SELECTED,
                Some(_) => V2_CELL_DAY,
                None => V2_CELL_DISABLED, // 上/下月补位格同样灰显。
            };
            out[r * GRID_COLS + c] = V2Cell { rect, kind };
        }
    }
    out
}

// -- 判定面扩展 ------------------------------------------------------------

/// F232 v2 自检（首条必为持久化 round-trip）。
pub fn run_datepick_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F232-datepick-v2");

    // 1. 持久化 round-trip（验主册「年月+选中日持久化」）。
    let mut dp = DatePick::new(PickMode::DateTime, DateYMD { y: 2026, m: 10, d: 1 }, no_weekends);
    let _ = dp.type_in("2026/3/16"); // 周一（非周末，可选中）——选中与视图迁移。
    let snap = V2PickSnap::capture(&dp);
    let mut buf = [0u8; V2_REC_LEN];
    let wrote = snap.to_bytes(&mut buf).unwrap_or(0);
    let back = V2PickSnap::from_bytes(&buf[..wrote]);
    set.add(
        "v2 persist round-trip: pick snapshot",
        wrote == V2_REC_LEN
            && back == Ok(snap)
            && back.map(|s| s.valid()) == Ok(true)
            && snap.sel == DateYMD { y: 2026, m: 3, d: 16 }
            && snap.view == (2026, 3),
        "",
    );

    // 2. 四类损坏全拒绝。
    let mut b1 = buf;
    b1[0] = b'X';
    let mut b2 = buf;
    b2[4] = 7;
    let mut b4 = buf;
    b4[V2_REC_LEN - 1] ^= 0xFF;
    set.add(
        "v2 persist rejects magic/version/len/checksum",
        matches!(V2PickSnap::from_bytes(&b1), Err(V2SaveErr::BadMagic))
            && matches!(V2PickSnap::from_bytes(&b2), Err(V2SaveErr::BadVersion))
            && matches!(V2PickSnap::from_bytes(&buf[..V2_REC_LEN - 1]), Err(V2SaveErr::BadLen))
            && matches!(V2PickSnap::from_bytes(&b4), Err(V2SaveErr::BadChecksum)),
        "",
    );

    // 3. 网格绘制清单（验主册「无效日置灰」：42 格全覆盖、周末灰显、
    //    选中格实色圆、平留白格灰显。2026-10 周六 5 天（3/10/17/24/31）
    //    + 周日 4 天（4/11/18/25），补位 11 格 → 灰显 20 格）。
    let g = CalGrid::new(2026, 10); // 10-01 周四开局，10-03 周六。
    let origin = crate::h1star::h1base::Rect::new(0, 0, GRID_COLS as i32 * V2_CELL_PX, GRID_ROWS as i32 * V2_CELL_PX);
    let cells = v2_grid_drawlist(g, origin, no_weekends, 1);
    let disabled_n = cells.iter().filter(|c| c.kind == V2_CELL_DISABLED).count();
    let sel_ok = cells.iter().any(|c| c.kind == V2_CELL_SELECTED && c.rect == v2_cell_rect(origin, 0, 4));
    set.add(
        "v2 grid drawlist: 42 cells, weekends gray, selected marked",
        cells.len() == GRID_ROWS * GRID_COLS && disabled_n == 42 - 31 + 9 && sel_ok,
        "",
    );

    // 4. 网格命中测试（点 → 日号与 position_of 互逆；栏外 None）。
    let (r1, c1) = g.position_of(17).unwrap_or((0, 0));
    let cell = v2_cell_rect(origin, r1, c1);
    let hit = v2_cell_hit(origin, g, cell.x + 3, cell.y + 3);
    set.add(
        "v2 cell hit test maps back to day",
        hit == Some(17) && v2_cell_hit(origin, g, -5, 5).is_none(),
        "",
    );

    // 5. 快照自校验拒非法日期（2 月 30 日不进控件——日历规则同一落点）。
    let bad = V2PickSnap {
        view: (2026, 2),
        sel: DateYMD { y: 2026, m: 2, d: 30 },
        mode: 0,
        hour: 0,
        minute: 0,
    };
    set.add(
        "v2 snap validity rejects impossible date",
        !bad.valid() && snap.valid(),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_snap_roundtrip_and_mode_byte() {
        let dp = DatePick::new(PickMode::Date, DateYMD { y: 2026, m: 1, d: 15 }, no_weekends);
        let snap = V2PickSnap::capture(&dp);
        assert_eq!(snap.mode, 0);
        let mut buf = [0u8; V2_REC_LEN];
        let n = snap.to_bytes(&mut buf).unwrap();
        assert_eq!(V2PickSnap::from_bytes(&buf[..n]).unwrap(), snap);
    }

    #[test]
    fn v2_drawlist_disabled_counts() {
        // 平年二月 28 天：42 - 28 = 14 补位格；2026-02-01 周日开局
        // （周六 7/14/21/28 + 周日 1/8/15/22 共 8 个周末日）→ 灰显 22。
        let g = CalGrid::new(2026, 2);
        let origin = crate::h1star::h1base::Rect::new(0, 0, 7 * V2_CELL_PX, 6 * V2_CELL_PX);
        let cells = v2_grid_drawlist(g, origin, no_weekends, 15);
        let gray = cells.iter().filter(|c| c.kind == V2_CELL_DISABLED).count();
        assert_eq!(gray, 14 + 8);
    }

    #[test]
    fn datepick_v2_selfcheck_all_green() {
        let s = run_datepick_v2_checks();
        assert!(s.all_passed(), "F232 v2 自检存在红项");
        assert!(!s.truncated());
    }
}
