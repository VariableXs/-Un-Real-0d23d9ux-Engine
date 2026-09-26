//! F029 深化批次二 · 枚举索引与监视器判定面（compatstar2/deep · G-A-29）。
//!
//! 批次一深化覆盖 DISPLAY_DEVICE/DEVMODE/字段掩码；本批补齐：枚举索引
//! 语义（iModeNum 越界如实 None——ENUM_MAX 口径）、MonitorFromPoint/
//! MonitorFromWindow 判定（MONITOR_DEFAULTTONULL/TOPRIMARY 语义）、分辨率
//! 切换确认倒计时（F446 联动：确认链的 15 秒窗口）、路径拓扑预留形状
//! （DISPLAYCONFIG 单屏 = 1 路径——多屏期扩容不动契约的形状面）。
//!
//! 零堆纪律：定长档位表复用，无 alloc。

use crate::checks::CheckSet;

/// 分辨率切换确认窗口 15 秒（F446 联动口径）。
pub const MODE_CONFIRM_WINDOW_S: u32 = 15;
/// MONITOR_DEFAULTTONULL（点在屏外 → None）。
pub const MONITOR_DEFAULTTONULL: u32 = 0;
/// MONITOR_DEFAULTTOPRIMARY（点在屏外 → 回主屏）。
pub const MONITOR_DEFAULTTOPRIMARY: u32 = 1;

/// EnumDisplaySettings 索引语义：档位表内返回 Some、越界如实 None
/// （iModeNum 枚举契约——MS 文档字段级对拍的索引面）。
pub fn enum_display_settings(i: usize) -> Option<(u32, u32)> {
    crate::compatstar2::moneum::RES_MODES.get(i).copied()
}

/// 枚举档位总数（ENUM_MAX_SETTINGS 口径）。
pub fn enum_max_settings() -> usize {
    crate::compatstar2::moneum::RES_MODES.len()
}

/// MonitorFromPoint：点落在屏幕矩形内 → 命中主屏；屏外按策略
/// （NULL → None / TOPRIMARY → 主屏回退）。
pub fn monitor_from_point(x: i32, y: i32, screen_w: i32, screen_h: i32, default: u32) -> Option<u32> {
    let inside = x >= 0 && y >= 0 && x < screen_w && y < screen_h;
    if inside {
        Some(1) // 单屏：主屏句柄恒 1
    } else if default == MONITOR_DEFAULTTOPRIMARY {
        Some(1)
    } else {
        None // MONITOR_DEFAULTTONULL
    }
}

/// MonitorFromWindow：最小化窗口不落任何屏 → 按 TOPRIMARY 回主屏
/// （MS 语义：最小化窗口坐标无意义，取默认主屏）。
pub fn monitor_from_window(minimized: bool) -> u32 {
    if minimized {
        MONITOR_DEFAULTTOPRIMARY
    } else {
        1
    }
}

/// 切换确认倒计时（F446：确认期内可取消，归零即应用）。
pub struct ModeConfirm {
    pub remaining_s: u32,
    pub armed: bool,
    pub applied: u32,
    pub cancelled: u32,
}

impl ModeConfirm {
    pub fn start() -> Self {
        ModeConfirm { remaining_s: MODE_CONFIRM_WINDOW_S, armed: true, applied: 0, cancelled: 0 }
    }
    /// tick：归零 → 应用（返回 true 一次）。
    pub fn tick(&mut self, dt_s: u32) -> bool {
        if !self.armed {
            return false;
        }
        if self.remaining_s > dt_s {
            self.remaining_s -= dt_s;
            false
        } else {
            self.remaining_s = 0;
            self.armed = false;
            self.applied += 1;
            true
        }
    }
    /// 用户取消（回到旧分辨率——确认链的撤销出口）。
    pub fn cancel(&mut self) {
        if self.armed {
            self.armed = false;
            self.cancelled += 1;
        }
    }
}

/// 路径拓扑预留形状：单屏实现恰好 1 条 DISPLAYCONFIG 路径
/// （多屏期扩容不动契约——接口冻结 ADR-PR-001 的形状账面）。
pub fn topology_path_count(monitors: usize) -> usize {
    monitors.max(1)
}

/// 域自检（深化批次二）。
pub fn run_f029d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F029-moneum-d2");
    // 1) 枚举索引：档位 0 与末档命中、越界如实 None（ENUM_MAX 口径）。
    let max = enum_max_settings();
    let first = enum_display_settings(0);
    let last = enum_display_settings(max - 1);
    cs.add(
        "enum_index_bounds",
        first.is_some() && last.is_some() && enum_display_settings(max).is_none() && enum_display_settings(usize::MAX >> 8).is_none(),
        "",
    );
    // 2) MonitorFromPoint：屏内命中；屏外 NULL → None；TOPRIMARY → 主屏。
    cs.add(
        "monitor_from_point",
        monitor_from_point(100, 100, 1920, 1080, MONITOR_DEFAULTTONULL) == Some(1)
            && monitor_from_point(-1, 100, 1920, 1080, MONITOR_DEFAULTTONULL).is_none()
            && monitor_from_point(5000, 5000, 1920, 1080, MONITOR_DEFAULTTOPRIMARY) == Some(1),
        "",
    );
    // 3) MonitorFromWindow：最小化 → 回主屏。
    cs.add("monitor_from_window", monitor_from_window(true) == MONITOR_DEFAULTTOPRIMARY && monitor_from_window(false) == 1, "");
    // 4) 确认倒计时：14s 未到、15s 归零应用；取消出口计数。
    let mut cf = ModeConfirm::start();
    let not_yet = !cf.tick(14);
    let fired = cf.tick(1);
    let mut cf2 = ModeConfirm::start();
    cf2.tick(3);
    cf2.cancel();
    cs.add("confirm_countdown", not_yet && fired && cf.applied == 1 && cf2.cancelled == 1 && cf2.applied == 0, "");
    // 5) 拓扑形状：单屏 1 路径；0 屏也保 1（单屏实现不塌缩）。
    cs.add("topology_path_shape", topology_path_count(1) == 1 && topology_path_count(0) == 1, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_roster_matches_modes() {
        // 档位表与主面 RES_MODES 逐档一致（一处一事实：表只有一份）。
        for (i, expect) in crate::compatstar2::moneum::RES_MODES.iter().enumerate() {
            assert_eq!(enum_display_settings(i), Some(*expect));
        }
    }

    #[test]
    fn confirm_boundary_exact() {
        let mut cf = ModeConfirm::start();
        for _ in 0..14 {
            assert!(!cf.tick(1), "确认期内不触发");
        }
        assert!(cf.tick(1), "恰 15s 触发");
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f029d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
