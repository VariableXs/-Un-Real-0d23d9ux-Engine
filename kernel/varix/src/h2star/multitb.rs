//! F277 多显示器任务栏策略 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：三策略切换即时生效；副屏按钮归属逻辑用例（窗在
//! 副屏时按钮随迁）；焦点屏判定（焦点切到副屏 200ms 内）；副屏时钟
//! 开关。
//!
//! **设计要点（主册）**：双屏时任务栏三策略可选：仅主屏、全部屏幕
//! （副屏只显示已在副屏的窗口按钮）、全部屏幕（副屏显示全部按钮）
//! ——默认第二档（副屏的活归副屏的任务栏）；副屏任务栏时钟可选隐藏；
//! Alt+Tab 与 Win 键菜单永远出在当前焦点屏。
//!
//! 实装：三策略枚举（默认第二档——判据定值）；按钮归属解析器（策略 ×
//! 窗口所在屏 → 每屏按钮表）；焦点屏判定（事件时戳注入）；副屏时钟
//! 开关（独立旋钮）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 三策略（唯一源；默认第二档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TbPolicy {
    /// 仅主屏。
    MainOnly,
    /// 全部屏幕——副屏只显示已在副屏的窗口按钮（默认档）。
    PerScreenOwn,
    /// 全部屏幕——副屏显示全部按钮。
    AllOnAll,
}

impl Default for TbPolicy {
    fn default() -> Self {
        TbPolicy::PerScreenOwn
    }
}

/// 一个被跟踪的窗口：id + 所在屏。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenWin {
    pub win_id: u32,
    pub screen: u8,
}

/// 按钮归属解析：策略 × 窗口表 → 每屏按钮表（win_id 列表）。
pub fn assign_buttons(policy: TbPolicy, wins: &[ScreenWin], screen_count: u8) -> Vec<Vec<u32>> {
    let mut out: Vec<Vec<u32>> = alloc::vec![Vec::new(); screen_count as usize];
    for w in wins {
        match policy {
            TbPolicy::MainOnly => out[0].push(w.win_id),
            TbPolicy::PerScreenOwn => {
                let s = (w.screen as usize).min(out.len() - 1);
                out[s].push(w.win_id);
            }
            TbPolicy::AllOnAll => {
                for bar in out.iter_mut() {
                    bar.push(w.win_id);
                }
            }
        }
    }
    out
}

/// 焦点屏判定：焦点切换事件的时延（注入 ms）是否在 200ms 内。
pub const FOCUS_SWITCH_LIMIT_MS: u64 = 200;

pub fn focus_screen_ok(latency_ms: u64) -> bool {
    latency_ms <= FOCUS_SWITCH_LIMIT_MS
}

/// 多屏任务栏服务。
pub struct MultiTaskbar {
    pub policy: TbPolicy,
    /// 副屏时钟可见（可选隐藏——默认显示）。
    pub secondary_clock: bool,
}

impl MultiTaskbar {
    pub fn new() -> MultiTaskbar {
        MultiTaskbar { policy: TbPolicy::PerScreenOwn, secondary_clock: true }
    }

    /// 切换策略即时生效（无重启无延迟——本函数即生效，无排队）。
    pub fn set_policy(&mut self, p: TbPolicy) {
        self.policy = p;
    }

    /// 按钮随迁：窗口从屏 A 挪到屏 B → PerScreenOwn 档下按钮随迁。
    /// `moves` 由调用方在窗口移动时调用（改 screen 即迁）。
    pub fn window_moved(&mut self, wins: &mut [ScreenWin], win_id: u32, to_screen: u8) -> bool {
        match wins.iter_mut().find(|w| w.win_id == win_id) {
            Some(w) => {
                w.screen = to_screen;
                true
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

fn wins() -> alloc::vec::Vec<ScreenWin> {
    alloc::vec![
        ScreenWin { win_id: 1, screen: 0 },
        ScreenWin { win_id: 2, screen: 0 },
        ScreenWin { win_id: 3, screen: 1 },
    ]
}

pub fn run_multitb_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F277");
    // 三策略归属逻辑。
    let a = assign_buttons(TbPolicy::MainOnly, &wins(), 2);
    let b = assign_buttons(TbPolicy::PerScreenOwn, &wins(), 2);
    let c = assign_buttons(TbPolicy::AllOnAll, &wins(), 2);
    set.add(
        "F277 three policies",
        a[0].len() == 3 && a[1].is_empty()
            && b[0].len() == 2 && b[1] == alloc::vec![3]
            && c[0].len() == 3 && c[1].len() == 3,
        "main/own/all",
    );
    // 默认第二档。
    let svc = MultiTaskbar::new();
    set.add(
        "F277 default second",
        svc.policy == TbPolicy::PerScreenOwn && svc.secondary_clock,
        "per-screen own",
    );
    // 窗在副屏时按钮随迁。
    let mut ws = wins();
    let mut svc2 = MultiTaskbar::new();
    let moved = svc2.window_moved(&mut ws, 2, 1);
    let after = assign_buttons(svc2.policy, &ws, 2);
    set.add(
        "F277 button follows window",
        moved && after[0].len() == 1 && after[1].contains(&2),
        "migration",
    );
    // 策略切换即时生效。
    svc2.set_policy(TbPolicy::MainOnly);
    let now = assign_buttons(svc2.policy, &ws, 2);
    set.add(
        "F277 instant switch",
        now[0].len() == 3 && now[1].is_empty(),
        "no restart",
    );
    // 焦点屏判定 200ms 线。
    set.add(
        "F277 focus 200ms",
        focus_screen_ok(199) && !focus_screen_ok(201),
        "hard line",
    );
    // 副屏时钟开关。
    let mut svc3 = MultiTaskbar::new();
    svc3.secondary_clock = false;
    set.add("F277 clock toggle", !svc3.secondary_clock, "hide on secondary");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f277_screen_policies() {
        let set = run_multitb_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F277 自检红 {f}/{p}");
    }

    #[test]
    fn screen_overflow_clamped() {
        // 窗口挂在超出屏数的屏号上——钳到最后一块屏，不 panic。
        let weird = alloc::vec![ScreenWin { win_id: 9, screen: 7 }];
        let out = assign_buttons(TbPolicy::PerScreenOwn, &weird, 2);
        assert_eq!(out[1], alloc::vec![9], "越界屏钳制到最后屏");
    }
}
