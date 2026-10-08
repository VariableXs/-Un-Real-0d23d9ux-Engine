//! H2 电源呈现引擎 · 深化批次三（F291 耗电排行 + F287 附加时钟——
//! 排行采样聚合、异常检测、归因文案、跨时区换算的唯一实现）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F291 耗电排行**：排行准确性（对账 F060 计量——本层是呈现
//!   侧聚合，计量数字一律来自调用方注入，不自行产数）；异常判定
//!   3 倍阈值（对比自身滑动基线，不是对别人——新应用不误报）；
//!   归因文案覆盖率（Top10 每条有说法——机检）；**只展示不越权**
//!   （排行输出类型不带执行字段——结构上没有「结束进程」按钮的
//!   数据通路，越权在类型层不存在）；
//! - **F287 附加时钟**：上限 2（第三个拒绝——机判）、命名显示、
//!   昼夜图标随当地时刻（6-18 昼）、**离线正确性**（纯偏移换算，
//!   不碰网络——断网用例的结构保证）。
//!
//! 时间纪律：分钟戳由调用方注入；时区偏移以分钟为单位（含半小时
//! 时区：5.5h = 330）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// F291 耗电排行
// ---------------------------------------------------------------------------

/// 一个采样：应用 + 毫瓦（呈现侧不做计量，只聚合注入值）。
#[derive(Clone, Copy, Debug)]
pub struct PowerSample {
    pub app: &'static str,
    pub mw: u32,
    pub minute: u64,
}

/// 排行条目：聚合值 + 异常旗标 + 归因（TopN 每条必须带得出说法）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankRow {
    pub app: &'static str,
    pub mw: u32,
    /// 异常：本窗口均值 > 3× 自身历史基线（3 倍阈值——对自身比）。
    pub anomaly: bool,
    /// 归因文案（人话——Top10 覆盖率机检的对象）。
    pub attribution: &'static str,
}

/// 滑动基线窗口（分钟——与 F060 账本窗口同谱）。
pub const BASELINE_WINDOW_MIN: u64 = 60;
/// 异常阈值：3 倍（判据原值）。
pub const ANOMALY_FACTOR: u32 = 3;

/// 聚合排行：近 `window_min` 分钟内逐应用求和降序 + 异常旗标 +
/// 归因。历史基线 = `window_min` 之前的同长窗口均值（无历史不判
/// 异常——「零历史不误报」是判据的机制化）。
pub fn rank(samples: &[PowerSample], window_min: u64, now_min: u64) -> Vec<RankRow> {
    let win_lo = now_min.saturating_sub(window_min);
    let base_lo = now_min.saturating_sub(window_min * 2);
    // 当前窗口聚合。
    let mut apps: Vec<(&'static str, u32, u32)> = Vec::new(); // (app, sum, count)
    for s in samples {
        if s.minute >= win_lo && s.minute <= now_min {
            match apps.iter_mut().find(|(a, _, _)| *a == s.app) {
                Some((_, sum, cnt)) => {
                    *sum += s.mw;
                    *cnt += 1;
                }
                None => apps.push((s.app, s.mw, 1)),
            }
        }
    }
    // 历史基线聚合（仅当前窗口之前、基线窗之内的样本）。
    let mut rows: Vec<RankRow> = apps
        .into_iter()
        .map(|(app, sum, cnt)| {
            let hist: Vec<u32> = samples
                .iter()
                .filter(|s| s.app == app && s.minute >= base_lo && s.minute < win_lo)
                .map(|s| s.mw)
                .collect();
            let anomaly = if hist.is_empty() || cnt == 0 {
                false
            } else {
                let cur_avg = sum / cnt;
                let hist_avg = hist.iter().sum::<u32>() / hist.len() as u32;
                hist_avg > 0 && cur_avg > hist_avg * ANOMALY_FACTOR
            };
            RankRow { app, mw: sum, anomaly, attribution: attribute_of(app) }
        })
        .collect();
    rows.sort_by(|a, b| b.mw.cmp(&a.mw).then(a.app.cmp(b.app)));
    rows
}

/// 归因映射（人话文案表——覆盖率的唯一来源；未知应用有兜底句，
/// 兜底句也是「说法」——覆盖率判据按「非空归因」机检）。
fn attribute_of(app: &str) -> &'static str {
    match app {
        a if a.contains("browser") || a.contains("web") => "网页活动：渲染与网络活动",
        a if a.contains("game") || a.contains("d3d") => "游戏：GPU 重负载渲染",
        a if a.contains("player") || a.contains("media") => "媒体播放：解码持续运行",
        a if a.contains("build") || a.contains("compiler") => "构建任务：CPU 满载编译",
        a if a.contains("sync") || a.contains("cloud") => "云同步：后台传输",
        _ => "常规活动：按前台使用计",
    }
}

/// TopN 归因覆盖率机检（F291 判据的机判形式）。
pub fn attribution_coverage(rows: &[RankRow], top: usize) -> bool {
    rows.iter().take(top).all(|r| !r.attribution.is_empty())
}

// ---------------------------------------------------------------------------
// F287 附加时钟
// ---------------------------------------------------------------------------

/// 一只附加时钟：名字 + 时区偏移（分钟，可负——UTC+5:30 = 330）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldClock {
    pub name: String,
    pub offset_min: i32,
}

/// 时钟托盘：上限 2（判据原值）——第三个加不进来。
pub const EXTRA_CLOCK_CAP: usize = 2;
/// 昼/夜分界（当地时刻 6:00–17:59 为昼）。
pub const DAY_START_H: u32 = 6;
pub const DAY_END_H: u32 = 18;

/// 换算：本地分钟戳 + 偏移 → (当地时, 分, 当地日是否比本机日偏移)。
/// 纯整数运算——离线正确性的结构保证（不查表不联网）。
pub fn convert(local_min_of_day: u64, offset_min: i32) -> (u32, u32, i32) {
    let shifted = local_min_of_day as i64 + offset_min as i64;
    let day_shift = shifted.div_euclid(1440) as i32;
    let mod_min = shifted.rem_euclid(1440) as u32;
    ((mod_min / 60) as u32, (mod_min % 60) as u32, day_shift)
}

/// 昼夜图标：当地时刻判定（跨日界的偏移换算已经 convert 归一）。
pub fn day_icon(hour: u32) -> bool {
    (DAY_START_H..DAY_END_H).contains(&hour)
}

/// 时钟托盘：增删查 + 上限。
pub struct ClockTray {
    pub clocks: Vec<WorldClock>,
}

impl ClockTray {
    pub fn new() -> ClockTray {
        ClockTray { clocks: Vec::new() }
    }

    /// 添加：超上限拒绝（返回 None——调用方给「最多两只」提示）。
    pub fn add(&mut self, name: &str, offset_min: i32) -> Option<usize> {
        if self.clocks.len() >= EXTRA_CLOCK_CAP {
            return None;
        }
        self.clocks.push(WorldClock { name: name.into(), offset_min });
        Some(self.clocks.len() - 1)
    }

    /// 移除（按序号）。
    pub fn remove(&mut self, idx: usize) -> bool {
        if idx < self.clocks.len() {
            self.clocks.remove(idx);
            true
        } else {
            false
        }
    }

    /// 呈现行：`名字 HH:MM [昼/夜图标]`（命名显示判据的落地）。
    pub fn render_line(&self, idx: usize, local_min_of_day: u64) -> Option<String> {
        let c = self.clocks.get(idx)?;
        let (h, m, _) = convert(local_min_of_day, c.offset_min);
        let icon = if day_icon(h) { "☀" } else { "☾" };
        Some(alloc::format!("{} {:02}:{:02} {}", c.name, h, m, icon))
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2power_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2power");
    // 排行：求和降序；同值按名稳定（可回放）。
    let samples = [
        PowerSample { app: "browser", mw: 3000, minute: 10 },
        PowerSample { app: "browser", mw: 2000, minute: 20 },
        PowerSample { app: "game", mw: 9000, minute: 30 },
        PowerSample { app: "sync", mw: 500, minute: 40 },
    ];
    let rows = rank(&samples, 60, 60);
    set.add(
        "h2power rank order",
        rows.len() == 3
            && rows[0].app == "game"
            && rows[0].mw == 9000
            && rows[1].app == "browser"
            && rows[1].mw == 5000,
        "sum desc",
    );
    // 3 倍异常：对自身基线比——有历史暴增才旗标。
    let hist = [
        PowerSample { app: "browser", mw: 1000, minute: 10 },
        PowerSample { app: "browser", mw: 1000, minute: 20 },
        PowerSample { app: "browser", mw: 1000, minute: 30 },
        PowerSample { app: "browser", mw: 1000, minute: 40 },
    ];
    // 窗口 60/now 120：当前窗（60..120）内 1000×3 + 10000；
    // 均值 3250 > 3×1000 历史基线 → 旗标。
    let cur = [
        PowerSample { app: "browser", mw: 1000, minute: 70 },
        PowerSample { app: "browser", mw: 1000, minute: 80 },
        PowerSample { app: "browser", mw: 1000, minute: 90 },
        PowerSample { app: "browser", mw: 10000, minute: 100 },
    ];
    let mut all = hist.to_vec();
    all.extend_from_slice(&cur);
    let rows2 = rank(&all, 60, 120);
    set.add(
        "h2power 3x anomaly",
        rows2.len() == 1 && rows2[0].anomaly,
        "spike flagged",
    );
    // 平稳应用不误报；零历史不误报。
    let calm = rank(&hist, 60, 60);
    set.add(
        "h2power no false positive",
        calm[0].app == "browser" && !calm[0].anomaly,
        "steady stays calm",
    );
    let fresh = [PowerSample { app: "game", mw: 9999, minute: 5 }];
    set.add(
        "h2power zero history",
        !rank(&fresh, 60, 10)[0].anomaly,
        "no history no flag",
    );
    // 归因覆盖率：Top10 全有说法；已知类命中映射表。
    set.add(
        "h2power attribution cover",
        attribution_coverage(&rows, 10)
            && rows.iter().find(|r| r.app == "browser").unwrap().attribution
                == "网页活动：渲染与网络活动",
        "top covered",
    );
    // 只展示不越权：RankRow 无执行字段（类型断言——编译即证明，
    // 此处锁定字段面防回退）。
    let r = &rows[0];
    set.add(
        "h2power display only",
        matches!(r, RankRow { app: _, mw: _, anomaly: _, attribution: _ }),
        "no actuator field",
    );
    // F287 换算：正偏移跨日、负偏移跨日、半小时时区。
    set.add(
        "h2power tz east",
        convert(23 * 60 + 30, 60) == (0, 30, 1),
        "UTC+1 crosses day",
    );
    set.add(
        "h2power tz west",
        convert(0 * 60 + 30, -60) == (23, 30, -1),
        "UTC-1 crosses back",
    );
    set.add(
        "h2power tz half hour",
        convert(12 * 60, 330) == (17, 30, 0),
        "UTC+5:30 same day",
    );
    // 昼夜图标：当地 6-17 昼。
    set.add(
        "h2power day icon",
        day_icon(6) && day_icon(17) && !day_icon(5) && !day_icon(18),
        "6–18 daylight",
    );
    // 托盘：上限 2、命名显示、删除后可再加。
    let mut tray = ClockTray::new();
    tray.add("东京", 540);
    tray.add("伦敦", 0);
    let third = tray.add("纽约", -300);
    set.add(
        "h2power clock cap",
        third.is_none() && tray.clocks.len() == 2,
        "third refused",
    );
    let line = tray.render_line(0, 12 * 60 + 5);
    set.add(
        "h2power named display",
        line.as_deref() == Some("东京 21:05 ☾"),
        "name + local time + icon",
    );
    tray.remove(1);
    set.add(
        "h2power remove re-add",
        tray.remove(5) == false && tray.add("伦敦", 0).is_some(),
        "slot freed",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2power_all_green() {
        let set = run_h2power_checks();
        assert!(set.all_passed(), "h2power 自检有红项");
        assert!(!set.truncated(), "h2power 自检溢出");
    }

    #[test]
    fn all_timezones_round_the_clock() {
        // 全 24 时 × ±14h 偏移：换算结果恒在 0..24 时域内（纯函数不变式）。
        for local in (0u64..1440).step_by(97) {
            for off in [-840i32, -330, -60, 0, 60, 330, 840] {
                let (h, m, _) = convert(local, off);
                assert!(h < 24 && m < 60, "local={local} off={off}");
            }
        }
    }

    #[test]
    fn anomaly_needs_history_factor() {
        // 恰 3 倍不旗标（> 严格大于），3 倍零一点旗标——阈值边界诚实。
        let mut s: Vec<PowerSample> = (0..4)
            .map(|i| PowerSample { app: "browser", mw: 1000, minute: i })
            .collect();
        s.push(PowerSample { app: "browser", mw: 3000, minute: 70 }); // avg cur = 1500? no: (1000*3+3000)/4=1500
        let rows = rank(&s, 60, 80);
        // 当前窗均值 1500 < 3×1000 → 不旗标。
        assert!(!rows[0].anomaly);
        s.push(PowerSample { app: "browser", mw: 9000, minute: 75 }); // (3000+9000+3000)/5 = 3000
        let rows2 = rank(&s, 60, 90);
        assert!(rows2[0].anomaly, "3x exceeded");
    }
}
