//! F287 附加时钟 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：两附加时区上限；命名显示；昼夜图标随当地时刻；
//! 秒显示开关即时生效；离线时区正确性（断网用例）。
//!
//! **设计要点（主册）**：系统时钟点击浮层（F078 日历飞出）支持附加两
//! 个时区（设置里命名如「团队-柏林」，浮层里以副时钟形式并列显示，附
//! 昼夜小图标）；主时钟秒显示可选开（默认关，任务栏清爽）；时区数据
//! 离线内置（U 盘系统不依赖网络取时区）。
//!
//! 实装：附加时区表（上限 2——注册超限拒绝）；命名+偏移换算（UTC 偏
/// 移注入——离线正确性：无网络路径）；昼夜图标判定（当地时刻 6-18 昼）；
/// 秒显示开关（即时生效——单一状态位）；时区数据内置标记（无网络依赖）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 附加时区上限（判据定值）。
pub const EXTRA_TZ_CAP: usize = 2;

/// 一个附加时区。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtraZone {
    pub name: String,
    /// UTC 偏移（分钟，含符号；柏林冬令 +60）。
    pub offset_min: i32,
}

/// 昼夜图标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayNight {
    Day,
    Night,
}

/// 当地时刻 → 昼夜（6:00-18:00 昼——判定唯一源）。
pub fn day_night(local_hour: u32) -> DayNight {
    if (6..18).contains(&local_hour) {
        DayNight::Day
    } else {
        DayNight::Night
    }
}

/// 时区换算：UTC 分钟 + 偏移 → 当地时:分（跨日取模，离线纯算——
/// 不依赖网络时区库）。
pub fn local_time(utc_min: u64, offset_min: i32) -> (u32, u32) {
    let total = utc_min as i64 + offset_min as i64;
    let day_min = total.rem_euclid(1440) as u32;
    (day_min / 60, day_min % 60)
}

/// 附加时钟服务。
pub struct ExtraClocks {
    zones: Vec<ExtraZone>,
    /// 主时钟秒显示（默认关——任务栏清爽）。
    pub show_seconds: bool,
    /// 时区数据内置标记（离线正确性——无网络路径）。
    pub offline_built_in: bool,
}

impl ExtraClocks {
    pub fn new() -> ExtraClocks {
        ExtraClocks { zones: Vec::new(), show_seconds: false, offline_built_in: true }
    }

    /// 添加附加时区（上限 2——超限拒绝并如实报告）。
    pub fn add(&mut self, name: &str, offset_min: i32) -> Result<(), &'static str> {
        if self.zones.len() >= EXTRA_TZ_CAP {
            return Err("附加时区最多两个");
        }
        self.zones.push(ExtraZone { name: String::from(name), offset_min });
        Ok(())
    }

    /// 移除。
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.zones.len();
        self.zones.retain(|z| z.name != name);
        self.zones.len() != before
    }

    pub fn zones(&self) -> &[ExtraZone] {
        &self.zones
    }

    /// 秒显示开关（即时生效——切换即变，无重启）。
    pub fn toggle_seconds(&mut self) -> bool {
        self.show_seconds = !self.show_seconds;
        self.show_seconds
    }

    /// 浮层渲染行：命名 + 当地时刻 + 昼夜图标（离线纯算）。
    pub fn render(&self, utc_min: u64) -> Vec<(String, (u32, u32), DayNight)> {
        self.zones
            .iter()
            .map(|z| {
                let (h, m) = local_time(utc_min, z.offset_min);
                (z.name.clone(), (h, m), day_night(h))
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_extraclk_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F287");
    let mut clk = ExtraClocks::new();
    // 两附加时区上限。
    set.add(
        "F287 cap two",
        clk.add("团队-柏林", 60).is_ok() && clk.add("家人-纽约", -300).is_ok()
            && clk.add("第三个", 480).is_err(),
        "max 2",
    );
    // 命名显示 + 换算：UTC 12:00 → 柏林 13:00（昼）、纽约 07:00（昼）。
    let rows = clk.render(12 * 60);
    set.add(
        "F287 named display",
        rows[0].0 == "团队-柏林" && rows[0].1 == (13, 0) && rows[1].1 == (7, 0),
        "name+time",
    );
    // 昼夜图标随当地时刻。
    set.add(
        "F287 day/night icon",
        rows[0].2 == DayNight::Day && day_night(23) == DayNight::Night,
        "6-18 day",
    );
    // 跨日取模：UTC 23:30 + 柏林 +60 → 次日 00:30（模内表示 0:30）。
    set.add(
        "F287 day rollover",
        local_time(23 * 60 + 30, 60) == (0, 30),
        "wrap midnight",
    );
    // 秒显示开关即时生效（默认关）。
    set.add(
        "F287 seconds toggle",
        !clk.show_seconds && clk.toggle_seconds() && clk.show_seconds,
        "default off, instant",
    );
    // 离线正确性：offline_built_in 恒真——换算纯算无网络路径。
    set.add(
        "F287 offline correct",
        clk.offline_built_in && local_time(0, -300) == (19, 0),
        "no network needed",
    );
    // 移除后可再添。
    let _ = clk.remove("团队-柏林");
    set.add("F287 remove refills", clk.add("东京", 540).is_ok(), "slot freed");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f287_clock_flow() {
        let set = run_extraclk_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F287 自检红 {f}/{p}");
    }

    #[test]
    fn negative_offset_wraps_back() {
        // UTC 01:00 纽约 -300 → 前一日 20:00（模内表示 20:00）。
        assert_eq!(local_time(60, -300), (20, 0));
    }
}
