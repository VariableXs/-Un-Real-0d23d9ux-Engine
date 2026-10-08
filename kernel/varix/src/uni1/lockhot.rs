//! F403 Win+L 锁屏快捷 · 完整设计（STAR I 主册 G-I-03）。
//!
//! **判据（主册）**：锁定 <300ms 实测；媒体暂停续播；窗口状态保持；
//! 锁屏摘要只计数判据；与 F316 闲置锁屏同一入口殊途同归。＋通12。
//!
//! **设计要点（主册）**：Win+L（VARIX 组合键）一键进 F238 锁屏——锁定
//! 要快才有安全感；锁定瞬间后台媒体暂停（解锁回来续播）、未保存文档
//! 不受影响（F311 恢复点照常）；锁屏期间通知照常进中心但锁屏摘要
//! 只计数（F238 隐私纪律）。
//!
//! 本模块是锁屏快捷**语义核**：锁定状态机（延迟预算记账）、媒体暂停/
//! 续播对、窗口状态保持清单、通知计数面。锁屏画面本体属 F238（H1 域），
//! 闲置自动锁屏属 F316——两者经由 [`LockGate::lock`] 同一入口进来
//! （殊途同归判据），来源以 [`LockSource`] 区分。
//!
//! 时间注入式（毫秒戳），无外部依赖。

use crate::checks::CheckSet;
use crate::uni1::ubase::RingLog;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 锁定延迟判线（ms）——「锁定 <300ms 实测」。
pub const LOCK_BUDGET_MS: u64 = 300;

/// 锁定路径分段预算（合计 = LOCK_BUDGET_MS；分段记账便于归因超时）：
/// 输入分发 → 状态冻结 → 画面切入。
pub const BUDGET_DISPATCH_MS: u64 = 40;
pub const BUDGET_FREEZE_MS: u64 = 160;
pub const BUDGET_PRESENT_MS: u64 = 100;

/// 通知摘要上限：锁屏只显示「N 条通知」计数（F238 隐私纪律——内容
/// 永远不进锁屏面）。
pub const SUMMARY_COUNT_ONLY: bool = true;

/// 解锁失败退避封顶（ms）——指数翻倍到 30s 封顶（防爆破：1s→2s→4s
/// →8s→16s→30s；F238 锁屏安全语义，唯一登记点）。
pub const UNLOCK_BACKOFF_CAP_MS: u64 = 30_000;

/// 第 n 次连续失败后的退避时长（ms）：n=1 → 1s，翻倍封顶。
/// n = 0 不构成退避（返回 0——调用方语义：无失败即无等待）。
pub fn unlock_backoff_ms(consecutive_failures: u64) -> u64 {
    if consecutive_failures == 0 {
        return 0;
    }
    let shift = (consecutive_failures - 1).min(5);
    let raw = 1_000u64 << shift;
    raw.min(UNLOCK_BACKOFF_CAP_MS)
}

/// 锁定来源（殊途同归的两个门）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockSource {
    /// F403：Win+L 手动锁定。
    Hotkey,
    /// F316：闲置自动锁屏。
    Idle,
}

/// 一条锁屏期间的通知：进中心是全文，锁屏摘要只有计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockedNotif {
    pub at_ms: u64,
    /// 全文长度记账（进中心的正文长度）——锁屏摘要不消费此字段。
    pub body_len: u32,
}

/// 媒体暂停对：暂停时刻 + 解锁续播时刻。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaPair {
    pub paused_at_ms: u64,
    pub resumed_at_ms: Option<u64>,
}

/// 锁定分段耗时（一次锁定的三段记账，供延迟归因）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockTimings {
    pub dispatch_ms: u64,
    pub freeze_ms: u64,
    pub present_ms: u64,
}

impl LockTimings {
    pub fn total_ms(&self) -> u64 {
        self.dispatch_ms + self.freeze_ms + self.present_ms
    }

    pub fn within_budget(&self) -> bool {
        self.total_ms() <= LOCK_BUDGET_MS
    }
}

/// 锁屏闸门状态机。
pub struct LockGate {
    pub locked: bool,
    /// 本次锁定来源（None = 未锁）。
    pub source: Option<LockSource>,
    /// 锁定时刻与最近一次锁定的分段耗时（<300ms 判据的账）。
    pub locked_at_ms: Option<u64>,
    pub last_timings: Option<LockTimings>,
    /// 媒体：锁定时若在播 → 暂停；解锁 → 续播。None = 无媒体会话。
    pub media: Option<MediaPair>,
    /// 锁定时刻的窗口状态清单（id → 可恢复标记）。解锁原样归还。
    pub window_states: Vec<(u64, bool)>,
    /// 锁屏期间进来的通知（全文进中心由调用方负责；这里记摘要账）。
    pub locked_notifs: Vec<LockedNotif>,
    /// 锁定次数 / 预算超限次数（诊断面）。
    pub lock_count: u64,
    pub budget_exceeded: u64,
    /// 分来源锁定计数（殊途同归的来源账——两条门各进多少次）。
    pub hotkey_locks: u64,
    pub idle_locks: u64,
    /// 连续解锁失败次数（成功即清零——退避窗计算基准）。
    pub unlock_failures: u64,
    /// 退避窗截止时刻（None = 无退避；期间正确密码也拒绝——诚实拒绝
    /// 不假装通过）。
    pub locked_out_until_ms: Option<u64>,
    /// 退避窗内被拒的尝试数（含正确密码——安全窗纪律）。
    pub rejected_in_lockout: u64,
    /// 锁定历史环（kind="lock"/"unlock"，可回放最近会话——第十三章）。
    pub history: RingLog,
}

impl LockGate {
    pub fn new() -> LockGate {
        LockGate {
            locked: false,
            source: None,
            locked_at_ms: None,
            last_timings: None,
            media: None,
            window_states: Vec::new(),
            locked_notifs: Vec::new(),
            lock_count: 0,
            budget_exceeded: 0,
            hotkey_locks: 0,
            idle_locks: 0,
            unlock_failures: 0,
            locked_out_until_ms: None,
            rejected_in_lockout: 0,
            history: RingLog::new(32),
        }
    }

    /// 注册媒体会话状态（调用方在锁定前告知：是否在播）。
    pub fn set_media_playing(&mut self, playing: bool, now_ms: u64) {
        self.media = if playing { Some(MediaPair { paused_at_ms: 0, resumed_at_ms: None }) } else { None };
        let _ = now_ms;
    }

    /// 锁定：同一入口承接 Win+L 与闲置锁屏。
    ///
    /// `timings` 由调用方按三段实测注入——模块不虚构耗时数字（诚实
    /// 记账：超预算也如实入账并计数）。
    pub fn lock(&mut self, source: LockSource, now_ms: u64, timings: LockTimings) -> bool {
        if self.locked {
            return false; // 已锁再锁 = 无动作（幂等）。
        }
        self.locked = true;
        self.source = Some(source);
        self.locked_at_ms = Some(now_ms);
        self.last_timings = Some(timings);
        self.lock_count += 1;
        match source {
            LockSource::Hotkey => self.hotkey_locks += 1,
            LockSource::Idle => self.idle_locks += 1,
        }
        self.history.push(
            now_ms,
            "lock",
            match source {
                LockSource::Hotkey => "hotkey",
                LockSource::Idle => "idle",
            },
            "",
        );
        if !timings.within_budget() {
            self.budget_exceeded += 1;
        }
        // 媒体暂停：锁定瞬间在播 → 暂停（续播等解锁）。
        if let Some(m) = self.media.as_mut() {
            if m.resumed_at_ms.is_none() && m.paused_at_ms == 0 {
                m.paused_at_ms = now_ms;
            }
        }
        true
    }

    /// 锁屏期间收到通知：照常入中心（这里记账），锁屏摘要只计数。
    pub fn notif_while_locked(&mut self, now_ms: u64, body_len: u32) -> bool {
        if !self.locked {
            return false;
        }
        self.locked_notifs.push(LockedNotif { at_ms: now_ms, body_len });
        true
    }

    /// 锁屏摘要计数（只计数判据的唯一出口——没有内容出口）。
    pub fn summary_count(&self) -> u32 {
        self.locked_notifs.len() as u32
    }

    /// 解锁：媒体续播 + 窗口状态原样归还。退避窗内拒绝（安全窗纪律）。
    pub fn unlock(&mut self, now_ms: u64) -> bool {
        if !self.locked {
            return false;
        }
        if let Some(until) = self.locked_out_until_ms {
            if now_ms < until {
                self.rejected_in_lockout += 1;
                self.history.push(now_ms, "unlock", "denied", "lockout");
                return false;
            }
        }
        self.locked_out_until_ms = None;
        self.unlock_failures = 0;
        self.history.push(now_ms, "unlock", "ok", "");
        if let Some(m) = self.media.as_mut() {
            if m.resumed_at_ms.is_none() {
                m.resumed_at_ms = Some(now_ms);
            }
        }
        self.locked = false;
        self.source = None;
        self.locked_notifs.clear();
        true
    }

    /// 解锁尝试（v7 深化）：密码正确性由调用方裁决，模块管安全状态机——
    /// 失败 → 连败计数 +1 并设退避窗（指数翻倍封顶 30s）；成功 → 若不在
    /// 退避窗内则解锁并清零连败。锁屏期间尝试才构成事件；未锁时无动作。
    pub fn attempt_unlock(&mut self, now_ms: u64, correct: bool) -> bool {
        if !self.locked {
            return false;
        }
        if correct {
            return self.unlock(now_ms);
        }
        self.unlock_failures += 1;
        let backoff = unlock_backoff_ms(self.unlock_failures);
        self.locked_out_until_ms = Some(now_ms.saturating_add(backoff));
        self.history.push(now_ms, "unlock", "failed", "");
        false
    }

    /// 当前是否处于退避窗（预览用——解锁面据此显示「N 秒后再试」）。
    pub fn in_lockout(&self, now_ms: u64) -> bool {
        self.locked_out_until_ms.map(|until| now_ms < until).unwrap_or(false)
    }

    /// 窗口状态冻结/恢复（保持判据：冻结集与恢复集逐项相等）。
    pub fn freeze_windows(&mut self, states: &[(u64, bool)]) {
        self.window_states = states.to_vec();
    }

    pub fn restore_windows(&self) -> &[(u64, bool)] {
        &self.window_states
    }

    /// 闲置锁屏（F316）复用同入口：来源标注 Idle。
    pub fn lock_idle(&mut self, now_ms: u64, timings: LockTimings) -> bool {
        self.lock(LockSource::Idle, now_ms, timings)
    }

    /// 最近一次锁定是否达标（<300ms）。
    pub fn last_lock_within_budget(&self) -> bool {
        self.last_timings.map(|t| t.within_budget()).unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F403 自检。
pub fn run_lockhot_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F403");

    // 预算分段合计 = 300ms 判线（分段常量一致性）。
    set.add(
        "f403-budget-sum",
        BUDGET_DISPATCH_MS + BUDGET_FREEZE_MS + BUDGET_PRESENT_MS == LOCK_BUDGET_MS,
        "",
    );

    // 快速路径：<300ms 判据（三段合计 240ms）。
    let fast = LockTimings { dispatch_ms: 30, freeze_ms: 140, present_ms: 70 };
    let mut g = LockGate::new();
    g.set_media_playing(true, 1_000);
    g.freeze_windows(&[(1, true), (2, true), (3, false)]);
    let ok_lock = g.lock(LockSource::Hotkey, 1_100, fast);
    set.add(
        "f403-lock-under-300ms",
        ok_lock && g.last_lock_within_budget() && g.locked && g.source == Some(LockSource::Hotkey),
        "",
    );

    // 媒体暂停：锁定瞬间暂停；解锁续播。
    set.add(
        "f403-media-pause-resume",
        g.media.map(|m| m.paused_at_ms == 1_100 && m.resumed_at_ms.is_none()).unwrap_or(false)
            && g.unlock(1_400)
            && g.media.map(|m| m.resumed_at_ms == Some(1_400)).unwrap_or(false),
        "",
    );

    // 窗口状态保持：冻结 3 项原样归还。
    set.add("f403-window-states", g.restore_windows().len() == 3 && g.restore_windows()[2] == (3, false), "");

    // 锁屏摘要只计数：期间 5 条通知 → 计数 5，无内容出口。
    let mut g2 = LockGate::new();
    g2.lock(LockSource::Hotkey, 2_000, LockTimings { dispatch_ms: 30, freeze_ms: 150, present_ms: 100 });
    for i in 0..5u64 {
        g2.notif_while_locked(2_100 + i * 100, 240);
    }
    set.add(
        "f403-summary-count-only",
        SUMMARY_COUNT_ONLY && g2.summary_count() == 5 && !g2.locked_notifs.is_empty(),
        "",
    );

    // 解锁后：摘要随锁屏消失（计数归零），新通知不再入锁屏账。
    g2.unlock(3_000);
    set.add("f403-unlock-stops-summary", g2.notif_while_locked(3_100, 100) == false && g2.summary_count() == 0, "");

    // 殊途同归：F316 闲置锁屏走同一 lock 入口，来源可辨。
    let mut g3 = LockGate::new();
    g3.lock_idle(4_000, LockTimings { dispatch_ms: 30, freeze_ms: 140, present_ms: 70 });
    set.add(
        "f403-idle-same-entry",
        g3.locked && g3.source == Some(LockSource::Idle) && g3.lock_count == 1,
        "",
    );

    // 诚实记账：超预算如实计数（不静默）。
    let slow = LockTimings { dispatch_ms: 100, freeze_ms: 200, present_ms: 150 };
    let mut g4 = LockGate::new();
    g4.lock(LockSource::Hotkey, 5_000, slow);
    set.add(
        "f403-over-budget-logged",
        g4.budget_exceeded == 1 && !g4.last_lock_within_budget(),
        "",
    );

    // 幂等：已锁再锁无动作。
    set.add("f403-idempotent", !g4.lock(LockSource::Hotkey, 5_100, fast) && g4.lock_count == 1, "");

    // ---- v7 深化：解锁退避 / 锁定历史环 / 分来源记账 ----

    // 退避表：指数翻倍 + 30s 封顶（唯一登记点常量与函数一致）。
    set.add(
        "f403-backoff-doubles",
        unlock_backoff_ms(0) == 0
            && unlock_backoff_ms(1) == 1_000
            && unlock_backoff_ms(2) == 2_000
            && unlock_backoff_ms(3) == 4_000,
        "",
    );
    set.add(
        "f403-backoff-caps",
        unlock_backoff_ms(5) == 16_000 && unlock_backoff_ms(6) == UNLOCK_BACKOFF_CAP_MS && unlock_backoff_ms(9) == UNLOCK_BACKOFF_CAP_MS,
        "",
    );

    // 失败 → 退避窗建立；窗内正确密码也诚实拒绝（安全窗纪律，计数）。
    let mut g5 = LockGate::new();
    g5.lock(LockSource::Hotkey, 6_000, fast);
    set.add(
        "f403-failure-sets-lockout",
        !g5.attempt_unlock(6_100, false) && g5.in_lockout(6_100) && g5.unlock_failures == 1,
        "",
    );
    set.add(
        "f403-lockout-rejects-correct",
        !g5.attempt_unlock(6_500, true) && g5.rejected_in_lockout == 1 && g5.locked,
        "",
    );

    // 窗外正确密码 → 解锁 + 连败清零 + 退避窗解除。
    set.add(
        "f403-correct-after-window",
        !g5.in_lockout(8_200) && g5.attempt_unlock(8_200, true) && !g5.locked && g5.unlock_failures == 0 && g5.locked_out_until_ms.is_none(),
        "",
    );

    // 未锁时尝试无动作（不构成事件、不设退避）。
    set.add(
        "f403-attempt-when-unlocked",
        !g5.attempt_unlock(9_000, false) && g5.unlock_failures == 0 && g5.locked_out_until_ms.is_none(),
        "",
    );

    // 锁定历史环：lock/unlock 事件按时间序可回放，失败窗拒绝也留痕
    // （未锁时的尝试不构成事件——第 5 拍无记录）。
    let snap = g5.history.snapshot();
    set.add(
        "f403-history-replay",
        snap.len() == 4
            && snap[0].kind == "lock" && snap[0].what == "hotkey"
            && snap[1].kind == "unlock" && snap[1].what == "failed"
            && snap[2].kind == "unlock" && snap[2].what == "denied"
            && snap[3].kind == "unlock" && snap[3].what == "ok",
        "",
    );

    // 分来源记账：两次热键 + 一次闲置，殊途同归各自入账。
    let mut g6 = LockGate::new();
    g6.lock(LockSource::Hotkey, 10_000, fast);
    g6.unlock(10_500);
    g6.lock(LockSource::Idle, 11_000, fast);
    g6.unlock(11_500);
    g6.lock(LockSource::Hotkey, 12_000, fast);
    set.add(
        "f403-per-source-counts",
        g6.hotkey_locks == 2 && g6.idle_locks == 1 && g6.lock_count == 3,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_unlock_full_cycle() {
        let mut g = LockGate::new();
        g.set_media_playing(true, 100);
        g.freeze_windows(&[(7, true)]);
        let t = LockTimings { dispatch_ms: 40, freeze_ms: 160, present_ms: 100 };
        assert_eq!(t.total_ms(), LOCK_BUDGET_MS, "分段合计必须等于判线");
        assert!(g.lock(LockSource::Hotkey, 200, t));
        assert!(g.last_lock_within_budget());
        assert!(g.notif_while_locked(300, 500));
        assert_eq!(g.summary_count(), 1);
        assert!(g.unlock(400));
        assert!(!g.locked);
        assert_eq!(g.media.unwrap().resumed_at_ms, Some(400));
        assert_eq!(g.restore_windows(), &[(7, true)]);
    }

    #[test]
    fn double_lock_is_noop() {
        let mut g = LockGate::new();
        let t = LockTimings { dispatch_ms: 10, freeze_ms: 10, present_ms: 10 };
        assert!(g.lock(LockSource::Idle, 100, t));
        assert!(!g.lock(LockSource::Hotkey, 150, t));
        assert_eq!(g.source, Some(LockSource::Idle), "二次锁定不改变来源");
    }

    #[test]
    fn over_budget_honest() {
        let mut g = LockGate::new();
        let t = LockTimings { dispatch_ms: 250, freeze_ms: 250, present_ms: 250 };
        assert!(g.lock(LockSource::Hotkey, 100, t));
        assert_eq!(g.budget_exceeded, 1);
        assert!(!g.last_lock_within_budget());
    }

    #[test]
    fn no_media_session_is_fine() {
        let mut g = LockGate::new();
        let t = LockTimings { dispatch_ms: 30, freeze_ms: 140, present_ms: 70 };
        assert!(g.lock(LockSource::Hotkey, 100, t));
        assert!(g.unlock(200));
        assert!(g.media.is_none());
    }

    // ---- v7 深化单测 ----

    #[test]
    fn backoff_schedule_doubles_and_caps() {
        assert_eq!(unlock_backoff_ms(0), 0);
        assert_eq!(unlock_backoff_ms(1), 1_000);
        assert_eq!(unlock_backoff_ms(2), 2_000);
        assert_eq!(unlock_backoff_ms(4), 8_000);
        assert_eq!(unlock_backoff_ms(6), UNLOCK_BACKOFF_CAP_MS, "第 6 连败封顶 30s");
        assert_eq!(unlock_backoff_ms(20), UNLOCK_BACKOFF_CAP_MS, "永不超封顶");
    }

    #[test]
    fn lockout_blocks_even_correct_password() {
        let mut g = LockGate::new();
        g.lock(LockSource::Idle, 1_000, LockTimings { dispatch_ms: 30, freeze_ms: 140, present_ms: 70 });
        // 三连败：退避窗 1s → 2s → 4s 逐次建立。
        assert!(!g.attempt_unlock(1_100, false));
        assert_eq!(g.locked_out_until_ms, Some(2_100));
        assert!(!g.attempt_unlock(2_200, false)); // 上一窗 2_100 已过 → 新窗 +2s
        assert_eq!(g.locked_out_until_ms, Some(4_200));
        assert!(!g.attempt_unlock(4_300, false));
        assert_eq!(g.locked_out_until_ms, Some(8_300));
        // 窗内正确密码被拒（并继续计数）。
        assert!(!g.attempt_unlock(5_000, true));
        assert_eq!(g.rejected_in_lockout, 1);
        // 窗外正确密码解锁，全部状态归零。
        assert!(g.attempt_unlock(8_300, true));
        assert!(!g.locked);
        assert_eq!(g.unlock_failures, 0);
        assert!(g.locked_out_until_ms.is_none());
    }

    #[test]
    fn history_ring_evicts_oldest() {
        let mut g = LockGate::new();
        let t = LockTimings { dispatch_ms: 10, freeze_ms: 10, present_ms: 10 };
        // 容量 32：锁/解 17 轮 = 34 事件 → 淘汰最旧 2 条。
        for i in 0..17u64 {
            g.lock(LockSource::Hotkey, i * 1_000, t);
            g.unlock(i * 1_000 + 500);
        }
        assert_eq!(g.history.len(), 32);
        assert_eq!(g.history.evicted, 2);
        let snap = g.history.snapshot();
        assert_eq!(snap[0].at_ms, 1_000, "第 0 轮 lock（at=0）已被淘汰");
        // 分来源账不受环淘汰影响（独立累计）。
        assert_eq!(g.hotkey_locks, 17);
    }
}
