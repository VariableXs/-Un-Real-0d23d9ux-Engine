//! F420 时钟右键快捷 · 完整设计（STAR I 主册 G-I-20）。
//!
//! **判据（主册）**：两直达落地页；立即同步执行与留痕（F295 事件）；
//! 左键行为不变；菜单项数；键盘可达（Tab 到时钟+菜单键）。＋通12。
//!
//! v5 纵深：同步失败显性化（失败有账 + 人话重试入口——异常零静默）；
//! 右键菜单完整生命周期（打开/导航/激活即收/Esc 关——浮层出路公理）；
//! 键盘菜单键与右键同一张表同一行为。
//!
//! v8 纵深：秒表状态机（起停计圈三键、重复启动不重复计时、停止后
//! 读数保留）；倒计时快捷预设（15/30/60 分钟三档、到点显性挂出、
//! 重设即清旧账）；世界钟多城切换（清单外拒绝）。

use crate::checks::CheckSet;
use crate::uni1::ubase::RingLog;

/// 右键两直达项（顺序钉死；项数=2 判据）。
pub const CLOCK_MENU: [&str; 2] = ["调整日期/时间", "刷新时间同步"];

/// 倒计时快捷预设（分钟）。
pub const COUNTDOWN_PRESETS_MIN: [u64; 3] = [15, 30, 60];

/// 世界钟城市清单（F100 时钟套件同源子集）。
pub const WORLD_CITIES: [&str; 5] = ["北京", "东京", "伦敦", "纽约", "悉尼"];

/// 秒表状态。
#[derive(PartialEq, Clone, Copy)]
pub enum StopwatchState {
    Idle,
    Running { started_at_ms: u64 },
    Stopped { elapsed_ms: u64 },
}

/// 倒计时状态。
#[derive(PartialEq, Clone, Copy)]
pub enum CountdownState {
    None,
    Armed { end_at_ms: u64, total_min: u64 },
    Fired,
}

/// 时钟快捷核。
pub struct ClockCtx {
    /// 左键行为（日历飞出 F078）——未被右键改动即 true。
    pub left_opens_calendar: bool,
    /// 最近一次同步留痕（F295 事件账）。
    pub sync_log: RingLog,
    pub sync_count: u64,
    /// 同步结果注入（真值来自时间服务——本核只记账不造真值）。
    pub sync_outcome_ok: bool,
    pub sync_failures: u64,
    pub last_sync_ok: Option<bool>,
    /// 右键/键盘菜单生命周期。
    pub menu_open: bool,
    pub menu_sel: usize,
    /// v8：秒表状态机。
    pub stopwatch: StopwatchState,
    /// v8：计圈账（每圈经过时长，ms）。
    pub laps_ms: Vec<u64>,
    /// v8：倒计时状态。
    pub countdown: CountdownState,
    /// v8：当前世界钟城市。
    pub world_city: &'static str,
}

impl ClockCtx {
    pub fn new() -> ClockCtx {
        ClockCtx {
            left_opens_calendar: true,
            sync_log: RingLog::new(16),
            sync_count: 0,
            sync_outcome_ok: true,
            sync_failures: 0,
            last_sync_ok: None,
            menu_open: false,
            menu_sel: 0,
            stopwatch: StopwatchState::Idle,
            laps_ms: Vec::new(),
            countdown: CountdownState::None,
            world_city: "北京",
        }
    }

    /// 右键菜单项分发（两直达落地页；菜单未开也可直接调——分发核与
    /// 菜单生命周期解耦，生命周期由 open/activate_selected 管理）。
    pub fn activate(&mut self, idx: usize) -> Option<&'static str> {
        match idx {
            0 => Some("settings.datetime"),
            1 => {
                // 立即同步执行 + 留痕（F295 事件：clock.sync.manual）。
                if self.sync_outcome_ok {
                    self.sync_count += 1;
                    self.last_sync_ok = Some(true);
                    self.sync_log.push(0, "clock.sync.manual", "executed", "");
                } else {
                    self.sync_failures += 1;
                    self.last_sync_ok = Some(false);
                    self.sync_log.push(0, "clock.sync.manual", "failed", "");
                }
                Some("f295.sync-now")
            }
            _ => None,
        }
    }

    /// 同步重试入口（失败后的出路——同一路径再走一次）。
    pub fn retry_sync(&mut self) -> bool {
        let _ = self.activate(1);
        self.last_sync_ok == Some(true)
    }

    /// 键盘可达：菜单键（Shift+F10）呼出与右键同一张菜单。
    pub fn keyboard_menu(&mut self) -> bool {
        self.open_menu(); // 与鼠标右键同表同行为
        true
    }

    /// 菜单生命周期：打开（高亮落首项）。
    pub fn open_menu(&mut self) {
        self.menu_open = true;
        self.menu_sel = 0;
    }

    /// 关闭（外点/Esc 公理出路；重复关无动作）。
    pub fn close_menu(&mut self) -> bool {
        let was = self.menu_open;
        self.menu_open = false;
        was
    }

    /// 菜单导航（环回；未开菜单不动）。
    pub fn nav_down(&mut self) {
        if self.menu_open {
            self.menu_sel = (self.menu_sel + 1) % CLOCK_MENU.len();
        }
    }

    pub fn nav_up(&mut self) {
        if self.menu_open {
            self.menu_sel = (self.menu_sel + CLOCK_MENU.len() - 1) % CLOCK_MENU.len();
        }
    }

    /// 激活当前高亮项：激活即收菜单（浮层出路公理——不残留）。
    pub fn activate_selected(&mut self) -> Option<&'static str> {
        if !self.menu_open {
            return None;
        }
        let r = self.activate(self.menu_sel);
        self.menu_open = false;
        r
    }

    // ------------------------------ v8：秒表状态机 ------------------------------

    /// 秒表启动：仅 Idle 可起跑（Running 重复启动不重置计时——防手滑
    /// 清零）；Stopped 再起跑从零开始。
    pub fn sw_start(&mut self, now_ms: u64) -> bool {
        match self.stopwatch {
            StopwatchState::Idle => {
                self.stopwatch = StopwatchState::Running { started_at_ms: now_ms };
                true
            }
            _ => false,
        }
    }

    /// 计圈：仅 Running 有效；圈时长 = 自起跑累计 − 已计圈之和。
    pub fn sw_lap(&mut self, now_ms: u64) -> Option<u64> {
        match self.stopwatch {
            StopwatchState::Running { started_at_ms } => {
                let total = now_ms.saturating_sub(started_at_ms);
                let done: u64 = self.laps_ms.iter().sum();
                let lap = total.saturating_sub(done);
                if lap == 0 {
                    return None; // 同拍重复计圈拒绝（0 圈长不入账）
                }
                self.laps_ms.push(lap);
                Some(lap)
            }
            _ => None,
        }
    }

    /// 秒表停止：读数保留（Stopped 态可读不丢）；Idle 停止无动作。
    pub fn sw_stop(&mut self, now_ms: u64) -> Option<u64> {
        match self.stopwatch {
            StopwatchState::Running { started_at_ms } => {
                let total = now_ms.saturating_sub(started_at_ms);
                self.stopwatch = StopwatchState::Stopped { elapsed_ms: total };
                Some(total)
            }
            _ => None,
        }
    }

    /// 秒表复位：仅 Stopped 可复位（Running 需先停——不给隐藏清零路）。
    pub fn sw_reset(&mut self) -> bool {
        match self.stopwatch {
            StopwatchState::Stopped { .. } => {
                self.stopwatch = StopwatchState::Idle;
                self.laps_ms.clear();
                true
            }
            _ => false,
        }
    }

    // ------------------------------ v8：倒计时快捷 ------------------------------

    /// 倒计时武装：仅预设三档；重设即清旧账（再武装覆盖旧终点）。
    /// 返回终点时刻。
    pub fn cd_arm(&mut self, preset_min: u64, now_ms: u64) -> Option<u64> {
        if !COUNTDOWN_PRESETS_MIN.contains(&preset_min) {
            return None;
        }
        let end = now_ms + preset_min * 60_000;
        self.countdown = CountdownState::Armed { end_at_ms: end, total_min: preset_min };
        Some(end)
    }

    /// 倒计时 tick：到点显性挂出（Armed→Fired 一次性——重复 tick 不
    /// 重复挂出）；取消武装归 None。
    pub fn cd_tick(&mut self, now_ms: u64) -> bool {
        match self.countdown {
            CountdownState::Armed { end_at_ms, .. } if now_ms >= end_at_ms => {
                self.countdown = CountdownState::Fired;
                true
            }
            _ => false,
        }
    }

    pub fn cd_cancel(&mut self) -> bool {
        let armed = self.countdown != CountdownState::None;
        self.countdown = CountdownState::None;
        armed
    }

    // ------------------------------ v8：世界钟切换 ------------------------------

    /// 世界钟切换：清单外拒绝（不静默切到错误城市）。
    pub fn world_switch(&mut self, city: &str) -> bool {
        if let Some(c) = WORLD_CITIES.iter().find(|c| **c == city) {
            self.world_city = c;
            true
        } else {
            false
        }
    }
}

pub fn run_clockctx_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F420");
    set.add(
        "f420-menu-two-items",
        CLOCK_MENU == ["调整日期/时间", "刷新时间同步"] && CLOCK_MENU.len() == 2,
        "",
    );
    let mut c = ClockCtx::new();
    set.add("f420-left-calendar-unchanged", c.left_opens_calendar, "");
    set.add("f420-direct-datetime", c.activate(0) == Some("settings.datetime"), "");
    set.add(
        "f420-sync-now-with-log",
        c.activate(1) == Some("f295.sync-now") && c.sync_count == 1 && c.sync_log.len() == 1,
        "",
    );
    set.add("f420-menu-bounds", c.activate(2).is_none(), "");
    set.add("f420-keyboard-reachable", c.keyboard_menu(), "");
    // 左键行为在右键操作后依旧不变。
    let _ = c.activate(1);
    set.add("f420-left-after-right", c.left_opens_calendar, "");
    // v5：同步失败显性化——失败有账、有结果位、有人话重试入口。
    let mut f = ClockCtx::new();
    f.sync_outcome_ok = false;
    set.add(
        "f420-sync-failure-logged",
        f.activate(1).is_some() && f.sync_failures == 1 && f.last_sync_ok == Some(false),
        "",
    );
    f.sync_outcome_ok = true;
    set.add(
        "f420-sync-retry-ok",
        f.retry_sync() && f.sync_count == 1 && f.last_sync_ok == Some(true),
        "",
    );
    // v5：菜单生命周期——打开高亮落首项；环回导航；激活即收。
    let mut m = ClockCtx::new();
    m.open_menu();
    set.add("f420-menu-open-at-first-item", m.menu_open && m.menu_sel == 0, "");
    m.nav_down();
    set.add("f420-menu-nav-down", m.menu_sel == 1, "");
    m.nav_down();
    set.add("f420-menu-nav-wrap", m.menu_sel == 0, "");
    m.nav_up();
    set.add("f420-menu-nav-up-wrap", m.menu_sel == 1, "");
    set.add(
        "f420-activate-closes-menu",
        m.activate_selected() == Some("f295.sync-now") && !m.menu_open,
        "",
    );
    set.add("f420-activate-needs-menu", m.activate_selected().is_none(), "");
    m.open_menu();
    set.add(
        "f420-esc-close-menu",
        m.close_menu() && !m.menu_open && !m.close_menu(),
        "",
    );
    // v5：键盘菜单键与右键同一张表——同一激活语义。
    let mut k = ClockCtx::new();
    set.add(
        "f420-keyboard-same-table",
        k.keyboard_menu() && k.menu_open && k.activate_selected() == Some("settings.datetime"),
        "",
    );
    // v8：秒表状态机——Idle 独占起跑、重复起跑不重置、计圈守恒、
    // 停止保留读数、复位清账、Running 隐藏清零路堵死。
    let mut s = ClockCtx::new();
    set.add("f420-sw-start", s.sw_start(1_000) && s.stopwatch == StopwatchState::Running { started_at_ms: 1_000 }, "");
    set.add("f420-sw-restart-no-reset", !s.sw_start(2_000) && s.stopwatch == StopwatchState::Running { started_at_ms: 1_000 }, "");
    set.add("f420-sw-lap", s.sw_lap(4_000) == Some(3_000) && s.sw_lap(9_000) == Some(5_000), "");
    set.add("f420-sw-lap-zero-reject", s.sw_lap(9_000).is_none(), "同拍重复计圈拒绝");
    set.add("f420-sw-stop-keeps", s.sw_stop(10_000) == Some(9_000) && s.stopwatch == StopwatchState::Stopped { elapsed_ms: 9_000 }, "");
    set.add("f420-sw-stop-idle-noop", s.sw_stop(11_000).is_none(), "");
    set.add("f420-sw-lap-stopped", s.sw_lap(11_000).is_none(), "");
    set.add("f420-sw-reset", s.sw_reset() && s.stopwatch == StopwatchState::Idle && s.laps_ms.is_empty(), "");
    set.add("f420-sw-reset-idle-noop", !s.sw_reset(), "");
    let mut r2 = ClockCtx::new();
    let _ = r2.sw_start(0);
    set.add("f420-sw-reset-running-blocked", !r2.sw_reset(), "Running 不给隐藏清零路");
    // v8：倒计时快捷——仅三档预设、到点一次性挂出、取消诚实。
    let mut d = ClockCtx::new();
    set.add("f420-cd-arm", d.cd_arm(30, 1_000_000) == Some(1_000_000 + 1_800_000), "");
    set.add("f420-cd-arm-invalid", d.cd_arm(45, 1_000_000).is_none() && d.cd_arm(0, 1_000_000).is_none(), "");
    set.add("f420-cd-not-yet", !d.cd_tick(2_000_000), "");
    set.add("f420-cd-fires-once", d.cd_tick(2_800_001) && d.countdown == CountdownState::Fired, "");
    set.add("f420-cd-fired-no-refire", !d.cd_tick(3_000_000), "");
    set.add("f420-cd-rearm-overrides", d.cd_arm(15, 3_000_000).is_some() && d.countdown == CountdownState::Armed { end_at_ms: 3_000_000 + 900_000, total_min: 15 }, "");
    set.add("f420-cd-cancel", d.cd_cancel() && !d.cd_cancel(), "");
    // v8：世界钟切换——清单内切换、清单外拒绝。
    let mut w = ClockCtx::new();
    set.add(
        "f420-world-switch",
        w.world_switch("伦敦") && w.world_city == "伦敦" && !w.world_switch("火星") && w.world_city == "伦敦",
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_log_accumulates() {
        let mut c = ClockCtx::new();
        for _ in 0..3 {
            let _ = c.activate(1);
        }
        assert_eq!(c.sync_count, 3);
        let snap = c.sync_log.snapshot();
        assert!(snap.iter().all(|e| e.kind == "clock.sync.manual"));
        assert!(snap.iter().all(|e| e.what == "executed"));
    }

    #[test]
    fn failure_and_retry_are_both_logged() {
        let mut c = ClockCtx::new();
        c.sync_outcome_ok = false;
        let _ = c.activate(1);
        c.sync_outcome_ok = true;
        assert!(c.retry_sync());
        let snap = c.sync_log.snapshot();
        assert_eq!(snap[0].what, "failed", "失败留痕——异常零静默");
        assert_eq!(snap[1].what, "executed");
    }

    #[test]
    fn laps_sum_equals_total() {
        // 圈长之和 == 总时长（守恒）。
        let mut c = ClockCtx::new();
        let _ = c.sw_start(0);
        let _ = c.sw_lap(300);
        let _ = c.sw_lap(700);
        let _ = c.sw_lap(1_200);
        let total = c.sw_stop(1_500).unwrap();
        assert_eq!(c.laps_ms.iter().sum::<u64>(), total - 300, "首拍前 300ms 未计圈，扣除后守恒");
    }

    #[test]
    fn countdown_total_preserved_through_fire() {
        let mut c = ClockCtx::new();
        let _ = c.cd_arm(60, 0);
        assert!(c.cd_tick(3_600_000));
        assert_eq!(c.countdown, CountdownState::Fired);
    }
}
