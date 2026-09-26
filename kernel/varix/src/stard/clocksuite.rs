//! F100 时钟套件 · 完整设计（STAR I 主册 G-C-30）。
//!
//! **判据（主册）**：闹钟睡眠唤醒触发实测（合盖 2h 场景）；世界时差与
//! 标准时区库 12 城全对；倒计时到点全链（音效+通知）录屏。
//!
//! **设计要点（主册）**：
//! - 四合一件：世界时钟（多城市卡）/ 秒表（圈计）/ 倒计时（预设+自定义）/
//!   闹钟（多组+重复规则）；闹钟到点走通知中心（F077）+ 音效方案（F079）
//!   + 全屏提醒（可选）；
//! - 窗口 480×560px 顶部四标签切换；世界时钟卡：城市名+时差+模拟钟面
//!   （4K 渲染）；秒表大数字 40px 等宽（E7）+ 圈列表；倒计时圆环进度
//!   （80fps）；闹钟列表行编辑（时间选择器+重复星期胶囊钮）；
//! - 闹钟与倒计时持久化（配置层，还原点覆盖）；世界时钟城市列表记忆；
//!   秒表会话内；闹钟重复规则支持单次/每日/工作日/自选星期；贪睡 5 分钟
//!   （按钮大目标 64px）；倒计时完成后自动可重启；秒表圈次自动标记最快
//!   最慢色；钟面刻度 4K 抗锯齿；时差显示含「明天/昨天」跨界标注；
//! - 系统睡眠中闹钟 → 唤醒源（F065 白名单显式唤醒）；免打扰（F077）→
//!   高优先级通道照弹；城市库无 → 搜索框+时区手选兜底。
//!
//! 时间全由调用方注入（unix ms），tz 表内建 12 城标准偏移（DST 城市
//! 如实标注——按 tzdata 同源偏移，夏令时切换由 F187 守护联动）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计与设计细节）
// ---------------------------------------------------------------------------

/// 贪睡时长（分钟）。
pub const SNOOZE_MIN: u64 = 5;

/// 倒计时预设（分钟）。
pub const COUNTDOWN_PRESETS: [u64; 4] = [1, 3, 5, 10];

/// 闹钟唤醒源白名单登记（F065 显式唤醒——睡眠中照触发）。
pub const WAKE_SOURCE_TAG: &str = "clocksuite.alarm";

/// 高优先级通知通道（F077 免打扰照弹）。
pub const NOTIFY_PRIORITY_BYPASS: bool = true;

/// 窗口规格（px）。
pub const WINDOW_W_PX: u32 = 480;
pub const WINDOW_H_PX: u32 = 560;

// ---------------------------------------------------------------------------
// 世界时钟（12 城标准偏移表 + 日界标注）
// ---------------------------------------------------------------------------

/// 城市时区（UTC 偏移分钟；DST 城市标注——切换联动 F187）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct City {
    pub name: &'static str,
    /// UTC 偏移（分钟，东正西负）。
    pub offset_min: i32,
    /// 有夏令时（偏移表给标准时——DST 期由守护校正）。
    pub dst: bool,
}

/// 12 城标准表（判据：12 城全对）。
pub const CITIES: [City; 12] = [
    City { name: "北京", offset_min: 8 * 60, dst: false },
    City { name: "东京", offset_min: 9 * 60, dst: false },
    City { name: "首尔", offset_min: 9 * 60, dst: false },
    City { name: "新加坡", offset_min: 8 * 60, dst: false },
    City { name: "悉尼", offset_min: 10 * 60, dst: true },
    City { name: "莫斯科", offset_min: 3 * 60, dst: false },
    City { name: "伦敦", offset_min: 0, dst: true },
    City { name: "巴黎", offset_min: 60, dst: true },
    City { name: "纽约", offset_min: -5 * 60, dst: true },
    City { name: "洛杉矶", offset_min: -8 * 60, dst: true },
    City { name: "迪拜", offset_min: 4 * 60, dst: false },
    City { name: "圣保罗", offset_min: -3 * 60, dst: true },
];

/// 本地时刻（UTC 偏移换算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 相对参考日的日界：-1 昨天 / 0 今天 / 1 明天。
    pub day_shift: i32,
}

/// civil 算法（Howard Hinnant days_from_civil / civil_from_days——标准
/// 时区库同源口径，12 城全对的算法本体）。
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64;
    let mp = ((m + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }), m, d)
}

/// unix 秒 + 城市偏移 → 本地时刻（含日界标注）。
pub fn local_time(unix_sec: i64, offset_min: i32) -> LocalTime {
    let local = unix_sec + offset_min as i64 * 60;
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    LocalTime {
        year,
        month,
        day,
        hour: (secs / 3600) as u32,
        minute: ((secs % 3600) / 60) as u32,
        second: (secs % 60) as u32,
        day_shift: 0, // 参考日由调用方对拍（本地日 vs 本机日）。
    }
}

/// 日界标注：本机本地日 vs 城市本地日。
pub fn day_shift_label(unix_sec: i64, home_offset_min: i32, city_offset_min: i32) -> &'static str {
    let home_day = (unix_sec + home_offset_min as i64 * 60).div_euclid(86_400);
    let city_day = (unix_sec + city_offset_min as i64 * 60).div_euclid(86_400);
    match city_day - home_day {
        1 => "明天",
        -1 => "昨天",
        2 => "后天",
        -2 => "前天",
        0 => "今天",
        d if d > 0 => "更晚",
        _ => "更早",
    }
}

/// 时差标注文本（±h:mm）。
pub fn offset_label(offset_min: i32) -> String {
    let sign = if offset_min >= 0 { "+" } else { "-" };
    let a = offset_min.abs();
    alloc::format!("UTC{}{}:{:02}", sign, a / 60, a % 60)
}

// ---------------------------------------------------------------------------
// 秒表（圈计 + 最快最慢标记）
// ---------------------------------------------------------------------------

/// 秒表。
pub struct Stopwatch {
    /// 运行中累计（ms，冻结在停止时刻）。
    pub running: bool,
    /// 本段起点（注入时钟）。
    pub started_at: u64,
    pub elapsed_ms: u64,
    /// 各圈累计时刻（ms）。
    pub laps: Vec<u64>,
}

impl Stopwatch {
    pub fn new() -> Stopwatch {
        Stopwatch { running: false, started_at: 0, elapsed_ms: 0, laps: Vec::new() }
    }

    pub fn start(&mut self, now_ms: u64) {
        if !self.running {
            self.running = true;
            self.started_at = now_ms;
        }
    }

    pub fn stop(&mut self, now_ms: u64) {
        if self.running {
            self.elapsed_ms += now_ms.saturating_sub(self.started_at);
            self.running = false;
        }
    }

    pub fn current_ms(&self, now_ms: u64) -> u64 {
        self.elapsed_ms + if self.running { now_ms.saturating_sub(self.started_at) } else { 0 }
    }

    /// 记一圈（当前总时刻）。返回圈序。
    pub fn lap(&mut self, now_ms: u64) -> usize {
        let t = self.current_ms(now_ms);
        self.laps.push(t);
        self.laps.len()
    }

    pub fn reset(&mut self) {
        *self = Stopwatch::new();
    }

    /// 圈时长列表。
    pub fn lap_durations(&self) -> Vec<u64> {
        let mut out = Vec::new();
        let mut prev = 0u64;
        for t in &self.laps {
            out.push(t - prev);
            prev = *t;
        }
        out
    }

    /// 最快/最慢圈序号（None=圈不足 2）。
    pub fn fastest_slowest(&self) -> (Option<usize>, Option<usize>) {
        let d = self.lap_durations();
        if d.len() < 2 {
            return (None, None);
        }
        let mut fast = 0usize;
        let mut slow = 0usize;
        for (i, v) in d.iter().enumerate() {
            if *v < d[fast] {
                fast = i;
            }
            if *v > d[slow] {
                slow = i;
            }
        }
        (Some(fast), Some(slow))
    }
}

impl Default for Stopwatch {
    fn default() -> Self {
        Self::new()
    }
}

/// 秒表文本 mm:ss.cs。
pub fn stopwatch_label(ms: u64) -> String {
    alloc::format!("{:02}:{:02}.{:02}", ms / 60_000, (ms % 60_000) / 1000, (ms % 1000) / 10)
}

// ---------------------------------------------------------------------------
// 倒计时（圆环进度 + 到点全链 + 重启）
// ---------------------------------------------------------------------------

/// 倒计时状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdState {
    Idle,
    Running,
    Fired,
}

/// 倒计时。
pub struct Countdown {
    pub state: CdState,
    /// 总时长（ms）。
    pub total_ms: u64,
    /// 结束时刻（注入钟）。
    pub end_at: u64,
    /// 到点全链账（音效/通知/全屏提醒三件）。
    pub fired_sound: bool,
    pub fired_notify: bool,
    pub fired_fullscreen: bool,
    pub fire_count: u64,
}

impl Countdown {
    pub fn new() -> Countdown {
        Countdown {
            state: CdState::Idle,
            total_ms: 0,
            end_at: 0,
            fired_sound: false,
            fired_notify: false,
            fired_fullscreen: false,
            fire_count: 0,
        }
    }

    /// 启动（预设或自定义分钟）。
    pub fn start(&mut self, minutes: u64, now_ms: u64) {
        self.total_ms = minutes * 60_000;
        self.end_at = now_ms + self.total_ms;
        self.state = CdState::Running;
        self.fired_sound = false;
        self.fired_notify = false;
        self.fired_fullscreen = false;
    }

    /// tick：到点触发全链（音效 F079 + 通知 F077 高优先级 + 全屏提醒）。
    pub fn tick(&mut self, now_ms: u64, fullscreen: bool) -> bool {
        if self.state == CdState::Running && now_ms >= self.end_at {
            self.state = CdState::Fired;
            self.fired_sound = true; // F079 音效方案。
            self.fired_notify = true; // F077 高优先级通道（免打扰照弹）。
            self.fired_fullscreen = fullscreen;
            self.fire_count += 1;
            return true;
        }
        false
    }

    /// 圆环进度（千分比——80fps 渲染的数据面）。
    pub fn progress_permille(&self, now_ms: u64) -> u32 {
        if self.state != CdState::Running || self.total_ms == 0 {
            return 0;
        }
        let remain = self.end_at.saturating_sub(now_ms);
        (1000 - remain.min(self.total_ms) * 1000 / self.total_ms) as u32
    }

    /// 完成后重启（自动可重启——同参数再来一轮）。
    pub fn restart(&mut self, now_ms: u64) {
        let total = self.total_ms;
        self.start(total / 60_000, now_ms);
    }
}

impl Default for Countdown {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 闹钟（重复规则 + 贪睡 + 唤醒源 + 免打扰照弹）
// ---------------------------------------------------------------------------

/// 重复规则。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Once,
    Daily,
    Weekdays,
    /// 自选星期位图（bit0=周一 … bit6=周日）。
    Custom(u8),
}

impl Repeat {
    /// 某星期（1=周一 … 7=周日）是否触发。
    pub fn matches_weekday(self, wd: u32) -> bool {
        match self {
            Repeat::Once => false, // Once 由 fire 后禁用表达。
            Repeat::Daily => true,
            Repeat::Weekdays => (1..=5).contains(&wd),
            Repeat::Custom(bits) => bits & (1 << (wd - 1)) != 0,
        }
    }
}

/// 闹钟。
#[derive(Clone, Debug, PartialEq)]
pub struct Alarm {
    pub id: u32,
    pub hour: u32,
    pub minute: u32,
    pub enabled: bool,
    pub repeat: Repeat,
    pub label: String,
}

/// 触发动作（通知中心面消费）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fired {
    pub alarm_id: u32,
    pub sound: bool,
    /// F077 高优先级通道（免打扰照弹）。
    pub notify_bypass: bool,
    /// F065 唤醒源（睡眠中显式唤醒——白名单登记）。
    pub wake_source: &'static str,
}

/// 闹钟引擎。
#[derive(Default)]
pub struct Alarms {
    pub list: Vec<Alarm>,
    next_id: u32,
    /// 已触发过的 Once 闹钟 id（不重复触发）。
    fired_once: Vec<u32>,
}

impl Alarms {
    pub fn add(&mut self, hour: u32, minute: u32, repeat: Repeat, label: &str) -> u32 {
        self.next_id += 1;
        self.list.push(Alarm {
            id: self.next_id,
            hour: hour.min(23),
            minute: minute.min(59),
            enabled: true,
            repeat,
            label: String::from(label),
        });
        self.next_id
    }

    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.list.len();
        self.list.retain(|a| a.id != id);
        self.list.len() != before
    }

    pub fn set_enabled(&mut self, id: u32, on: bool) -> bool {
        match self.list.iter_mut().find(|a| a.id == id) {
            Some(a) => {
                a.enabled = on;
                true
            }
            None => false,
        }
    }

    /// 检查触发：给定本机本地时刻 + 星期。返回触发动作（0~1 条——同分钟
    /// 只触发一次，Once 触发后自动禁用）。
    pub fn check(&mut self, hour: u32, minute: u32, weekday: u32) -> Option<Fired> {
        for a in self.list.iter_mut() {
            if !a.enabled || a.hour != hour || a.minute != minute {
                continue;
            }
            let due = match a.repeat {
                Repeat::Once => !self.fired_once.contains(&a.id),
                r => r.matches_weekday(weekday),
            };
            if !due {
                continue;
            }
            if a.repeat == Repeat::Once {
                self.fired_once.push(a.id);
                a.enabled = false; // Once 触发后自动禁用。
            }
            return Some(Fired {
                alarm_id: a.id,
                sound: true,                          // F079 音效方案。
                notify_bypass: NOTIFY_PRIORITY_BYPASS, // F077 高优先级照弹。
                wake_source: WAKE_SOURCE_TAG,          // F065 唤醒源白名单。
            });
        }
        None
    }

    /// 贪睡：+5 分钟新临时闹钟（Once 语义）。
    pub fn snooze(&mut self, hour: u32, minute: u32) -> u32 {
        let mut m = minute + SNOOZE_MIN as u32;
        let mut h = hour;
        if m >= 60 {
            m -= 60;
            h = (h + 1) % 24;
        }
        self.add(h, m, Repeat::Once, "贪睡")
    }
}

/// 星期推算（civil days → 1=周一 … 7=周日；1970-01-01 是周四）。
pub fn weekday_of(days: i64) -> u32 {
    // days=0 → 周四(4)。
    let wd = (days + 3).rem_euclid(7) as u32 + 1;
    wd
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F100 自检（聚合进 stard 域）。
pub fn run_clocksuite_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F100");

    // —— 12 城时差全对（标准时区库 civil 算法口径）——
    set.add("cities count 12", CITIES.len() == 12, "");
    // 2026-01-15 00:00:00 UTC = unix 1768435200。
    let t: i64 = 1_768_435_200;
    let bj = local_time(t, 8 * 60);
    set.add("beijing 08:00", bj.hour == 8 && bj.minute == 0 && (bj.year, bj.month, bj.day) == (2026, 1, 15), "");
    let ny = local_time(t, -5 * 60);
    set.add("new york 19:00 prev day", ny.hour == 19 && ny.day == 14, "");
    let ld = local_time(t, 0);
    set.add("london 00:00", ld.hour == 0, "");
    set.add("tokyo 09:00", local_time(t, 9 * 60).hour == 9, "");
    set.add("sydney 10:00", local_time(t, 10 * 60).hour == 10, "");
    set.add("la 16:00 prev day", local_time(t, -8 * 60).hour == 16 && local_time(t, -8 * 60).day == 14, "");
    set.add("day shift label", day_shift_label(t, 8 * 60, -5 * 60) == "昨天" && day_shift_label(t, 8 * 60, 9 * 60) == "今天", "");
    set.add("offset label", offset_label(-5 * 60) == "UTC-5:00" && offset_label(9 * 60) == "UTC+9:00" && offset_label(60) == "UTC+1:00", "");

    // —— civil 算法：闰年与跨年锚点 ——
    set.add("civil leap anchor", days_from_civil(2028, 2, 29) - days_from_civil(2028, 2, 28) == 1, "");
    set.add("civil year boundary", days_from_civil(2027, 1, 1) - days_from_civil(2026, 12, 31) == 1, "");

    // —— 星期推算（2026-01-15 是周四）——
    set.add("weekday thursday", weekday_of(days_from_civil(2026, 1, 15)) == 4, "");

    // —— 秒表：圈计 + 最快最慢 ——
    let mut sw = Stopwatch::new();
    sw.start(1000);
    let _ = sw.current_ms(4000); // 3s
    sw.lap(4000);
    let _ = sw.current_ms(6000); // +2s
    sw.lap(6000);
    sw.stop(7000); // +1s → 总 6s
    set.add("stopwatch elapsed", sw.current_ms(9999) == 6_000, "");
    let durs = sw.lap_durations();
    set.add("lap durations", durs == alloc::vec![3_000, 2_000], "");
    let (fast, slow) = sw.fastest_slowest();
    set.add("fastest slowest marks", fast == Some(1) && slow == Some(0), "");
    set.add("stopwatch label", stopwatch_label(6_123) == "00:06.12" && stopwatch_label(372_230) == "06:12.23", "");
    // 单圈不出极值标记。
    let mut sw2 = Stopwatch::new();
    sw2.start(0);
    sw2.lap(1000);
    set.add("single lap no marks", sw2.fastest_slowest() == (None, None), "");

    // —— 倒计时：到点全链（音效+通知）+ 圆环 + 重启 ——
    let mut cd = Countdown::new();
    cd.start(3, 10_000); // 泡面计时 3 分钟。
    set.add("countdown running", cd.state == CdState::Running, "");
    set.add("countdown ring 0", cd.progress_permille(10_000) == 0, "");
    set.add("countdown ring half", cd.progress_permille(10_000 + 90_000) == 500, "");
    set.add("not fired early", !cd.tick(10_000 + 179_999, false), "");
    set.add("fired at end", cd.tick(190_000, false), "");
    set.add("fire chain sound+notify", cd.fired_sound && cd.fired_notify, "");
    cd.restart(200_000);
    set.add("countdown restart", cd.state == CdState::Running && cd.total_ms == 180_000, "");
    set.add("presets", COUNTDOWN_PRESETS == [1, 3, 5, 10], "");

    // —— 闹钟：重复规则 + 贪睡 + 唤醒源 + 免打扰照弹 ——
    let mut al = Alarms::default();
    let id1 = al.add(7, 30, Repeat::Daily, "起床");
    let id2 = al.add(9, 0, Repeat::Weekdays, "站会");
    let id3 = al.add(22, 0, Repeat::Custom(0b0100001), "自选周一/周六");
    set.add("alarm added", id1 == 1 && id2 == 2 && id3 == 3, "");
    // 每日闹钟每天触发。
    set.add("daily fires", al.check(7, 30, 1).is_some(), "");
    set.add("daily fires any day", al.check(7, 30, 6).is_some(), "");
    // 工作日闹钟周末不触发。
    set.add("weekday alarm skips saturday", al.check(9, 0, 6).is_none(), "");
    set.add("weekday alarm fires monday", al.check(9, 0, 1).is_some(), "");
    // 自选星期位图（周一 bit0 / 周六 bit5）。
    set.add("custom fires monday", al.check(22, 0, 1).is_some(), "");
    set.add("custom fires saturday", al.check(22, 0, 6).is_some(), "");
    set.add("custom skips wednesday", al.check(22, 0, 3).is_none(), "");
    // Once 触发后自动禁用。
    let mut al2 = Alarms::default();
    al2.add(8, 0, Repeat::Once, "一次");
    set.add("once fires", al2.check(8, 0, 3).is_some(), "");
    set.add("once fires exactly one", al2.check(8, 0, 3).is_none(), "");
    // 触发动作三件：音效 + 免打扰照弹 + 唤醒源白名单。
    let mut al3 = Alarms::default();
    al3.add(6, 0, Repeat::Once, "合盖场景");
    let f = al3.check(6, 0, 2).unwrap();
    set.add("fired sound + bypass dnd", f.sound && f.notify_bypass, "");
    set.add("fired wake source f065", f.wake_source == WAKE_SOURCE_TAG, "");
    // 贪睡 +5 分钟。
    let sid = al3.snooze(7, 58);
    let snooze_alarm = al3.list.iter().find(|a| a.id == sid).unwrap();
    set.add("snooze 5 min carry", snooze_alarm.hour == 8 && snooze_alarm.minute == 3 && snooze_alarm.repeat == Repeat::Once, "");
    // 删除与开关。
    set.add("alarm toggle", al3.set_enabled(sid, false), "");
    set.add("alarm remove", al3.remove(sid) && !al3.list.iter().any(|a| a.id == sid), "");

    // —— 规格常量 ——
    set.add("snooze 5min", SNOOZE_MIN == 5, "");
    set.add("window spec", WINDOW_W_PX == 480 && WINDOW_H_PX == 560, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twelve_cities_offsets_all_correct() {
        // 2026-06-01 12:00:00 UTC = unix 1780315200（夏令时城市按标准时）。
        let t: i64 = 1_780_315_200;
        let expect: [(&str, i32, u32); 12] = [
            ("北京", 480, 20), ("东京", 540, 21), ("首尔", 540, 21), ("新加坡", 480, 20),
            ("悉尼", 600, 22), ("莫斯科", 180, 15), ("伦敦", 0, 12), ("巴黎", 60, 13),
            ("纽约", -300, 7), ("洛杉矶", -480, 4), ("迪拜", 240, 16), ("圣保罗", -180, 9),
        ];
        for (i, c) in CITIES.iter().enumerate() {
            assert_eq!(c.name, expect[i].0);
            assert_eq!(c.offset_min, expect[i].1);
            let lt = local_time(t, c.offset_min);
            assert_eq!(lt.hour, expect[i].2, "{} 本地小时", c.name);
        }
    }

    #[test]
    fn day_shift_boundary_labels() {
        // 北京 2026-01-15 08:00（UTC 00:00）：
        let t: i64 = 1_768_435_200;
        assert_eq!(day_shift_label(t, 480, 540), "今天"); // 东京 09:00 同日
        assert_eq!(day_shift_label(t, 480, -300), "昨天"); // 纽约 19:00 前一天
        assert_eq!(day_shift_label(t, 480, 600), "今天"); // 悉尼 18:00 同日
        // 走到北京 23:30 → 悉尼已过午夜 → 明天。
        let t2 = t + (15 * 3600 + 1800);
        assert_eq!(day_shift_label(t2, 480, 600), "明天");
    }

    #[test]
    fn stopwatch_multi_round() {
        let mut sw = Stopwatch::new();
        sw.start(0);
        sw.lap(10_000);
        sw.lap(25_000);
        sw.stop(30_000);
        sw.start(100_000); // 暂停后继续。
        assert_eq!(sw.current_ms(105_000), 35_000);
        sw.stop(106_000);
        assert_eq!(sw.elapsed_ms, 36_000);
        assert_eq!(sw.lap_durations(), alloc::vec![10_000, 15_000]);
        let (f, s) = sw.fastest_slowest();
        assert_eq!((f, s), (Some(0), Some(1)));
        sw.reset();
        assert!(sw.laps.is_empty() && sw.elapsed_ms == 0 && !sw.running);
    }

    #[test]
    fn countdown_full_chain_and_ring() {
        let mut cd = Countdown::new();
        cd.start(1, 0);
        // 半程圆环 500‰。
        assert_eq!(cd.progress_permille(30_000), 500);
        // 到点三件全链。
        assert!(cd.tick(60_000, true));
        assert!(cd.fired_sound && cd.fired_notify && cd.fired_fullscreen);
        assert_eq!(cd.fire_count, 1);
        assert_eq!(cd.progress_permille(70_000), 0, "非运行态圆环归零");
        // 重启再来一轮。
        cd.restart(61_000);
        assert!(!cd.fired_sound, "重启后到点账清零");
        assert!(cd.tick(121_000, false));
        assert_eq!(cd.fire_count, 2);
    }

    #[test]
    fn alarm_repeat_matrix() {
        let mut al = Alarms::default();
        al.add(7, 0, Repeat::Daily, "每日");
        al.add(8, 0, Repeat::Weekdays, "工作日");
        al.add(9, 0, Repeat::Custom(0b1111111), "全周自选");
        al.add(10, 0, Repeat::Custom(0b0010000), "仅周五");
        // 全周矩阵。
        for wd in 1..=7 {
            assert!(al.check(7, 0, wd).is_some(), "每日闹钟周{wd}");
            let wd_alarm = al.check(8, 0, wd);
            if wd <= 5 {
                assert!(wd_alarm.is_some(), "工作日闹钟周{wd}");
            } else {
                assert!(wd_alarm.is_none(), "周末静默周{wd}");
            }
            assert!(al.check(9, 0, wd).is_some(), "全周自选周{wd}");
            let fri = al.check(10, 0, wd);
            if wd == 5 {
                assert!(fri.is_some());
            } else {
                assert!(fri.is_none());
            }
        }
    }

    #[test]
    fn once_alarm_and_snooze_chain() {
        let mut al = Alarms::default();
        al.add(6, 30, Repeat::Once, "临时");
        let f = al.check(6, 30, 4).expect("应触发");
        assert_eq!(f.alarm_id, 1);
        assert!(f.sound && f.notify_bypass && f.wake_source == WAKE_SOURCE_TAG);
        // Once 不重复。
        assert!(al.check(6, 30, 4).is_none());
        assert!(al.check(6, 30, 5).is_none());
        // 贪睡：6:35 触发，再贪睡 → 6:40。
        let s1 = al.snooze(6, 30);
        let f2 = al.check(6, 35, 4).expect("贪睡应触发");
        assert_eq!(f2.alarm_id, s1);
        let s2 = al.snooze(6, 35);
        assert!(al.check(6, 40, 4).is_some(), "二次贪睡");
        assert!(al.list.iter().find(|a| a.id == s2).is_some());
    }

    #[test]
    fn civil_roundtrip_fuzz() {
        // civil 算法 round-trip：随机日往返零漂移。
        for &d in &[0i64, 1, 100, 19_723, 20_600, 50_000, 100_000] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d, "day {d} roundtrip");
        }
        // 已知锚：1970-01-01 = day 0；2000-03-01 = day 11017。
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    }

    #[test]
    fn local_time_seconds_and_minutes() {
        // 12:34:56 UTC + 8 → 20:34:56。
        let t: i64 = 12 * 3600 + 34 * 60 + 56;
        let lt = local_time(t, 480);
        assert_eq!((lt.hour, lt.minute, lt.second), (20, 34, 56));
        // 负偏移跨日：00:30 UTC -5 → 前一天 19:30。
        let t2: i64 = 1800;
        let lt2 = local_time(t2, -300);
        assert_eq!(lt2.hour, 19);
    }
}
