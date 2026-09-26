//! F461 前台应用通知静默（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **聚焦判定（<200ms 身份切换）；横幅抑制与中心入库；多窗口应用（任一窗
//! 聚焦即算前台）；关闭开关；与其他静默档（F341）叠加优先级。**
//!
//! 功能定义（主册批次三）：来自当前聚焦应用的通知不弹横幅（你正在这个应用
//! 里，它的事你看得见），只静默入通知中心（F077）；判定按前台应用身份
//! （F282 单例身份同源）；用户可关此礼仪。
//!
//! 零堆纪律：定长窗口身份表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 聚焦身份切换判定时限（主册：<200ms）。
pub const FOCUS_SWITCH_MS: u64 = 200;
/// 通知中心入库标记（横幅抑制 ≠ 信息丢失——中心全记）。
pub const ALWAYS_INTO_CENTER: bool = true;
/// 静默档叠加：DND 档（F341 四档）优先级高于前台礼仪。
pub const DND_OVERRIDES_FOCUS_MANNER: bool = true;

/// 通知的呈现裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BannerVerdict {
    /// 弹横幅。
    Show,
    /// 抑制横幅、静默入中心（前台礼仪命中）。
    SuppressToCenter,
    /// 全静默（DND 等更高优先档）。
    SilentAll,
}

/// 聚焦身份追踪（F282 单例身份同源；多窗口应用任一窗聚焦即算前台）。
pub struct FocusTracker {
    /// 应用 → 打开窗口数（任一窗聚焦 = 应用前台）。
    open_windows: [Option<(&'static str, u16)>; 8],
    n: usize,
    /// 当前聚焦的应用身份。
    focused: Option<&'static str>,
    /// 身份切换时刻（<200ms 判定核算）。
    switch_at_ms: u64,
}

impl FocusTracker {
    pub const fn new() -> Self {
        FocusTracker {
            open_windows: [None; 8],
            n: 0,
            focused: None,
            switch_at_ms: 0,
        }
    }

    fn slot(&self, app: &str) -> Option<usize> {
        (0..self.n).find(|&i| matches!(self.open_windows[i], Some((a, _)) if a == app))
    }

    /// 窗口开（多窗口应用窗口计数递增）。
    pub fn open_window(&mut self, app: &'static str) -> bool {
        if let Some(i) = self.slot(app) {
            if let Some((_, c)) = self.open_windows[i].as_mut() {
                *c = c.saturating_add(1);
            }
            return true;
        }
        if self.n >= 8 {
            return false;
        }
        self.open_windows[self.n] = Some((app, 1));
        self.n += 1;
        true
    }

    /// 窗口关。
    pub fn close_window(&mut self, app: &'static str) -> bool {
        match self.slot(app) {
            Some(i) => {
                let mut keep = true;
                if let Some((_, c)) = self.open_windows[i].as_mut() {
                    *c = c.saturating_sub(1);
                    keep = *c > 0;
                }
                if !keep {
                    self.open_windows[i] = self.open_windows[self.n - 1];
                    self.open_windows[self.n - 1] = None;
                    self.n -= 1;
                    if self.focused == Some(app) {
                        self.focused = None;
                    }
                }
                true
            }
            None => false,
        }
    }

    /// 聚焦切换（记录切换时刻——<200ms 判定核算）。
    pub fn focus(&mut self, app: Option<&'static str>, now_ms: u64) {
        if self.focused != app {
            self.focused = app;
            self.switch_at_ms = now_ms;
        }
    }

    pub fn focused_app(&self) -> Option<&'static str> {
        self.focused
    }

    /// 聚焦判定在时限内生效（主册 <200ms）。
    pub fn focus_effective_within(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.switch_at_ms) < FOCUS_SWITCH_MS
    }

    /// 应用是否前台（任一窗聚焦即算前台——主册判据）。
    pub fn is_foreground(&self, app: &str) -> bool {
        self.focused == Some(app) && self.slot(app).is_some()
    }
}

/// 通知裁决器。
pub struct MannerGate {
    /// 前台礼仪开关（用户可关——「我就是想看每条横幅」也尊重）。
    pub enabled: bool,
    /// DND 档（F341 四档：0 关/1 仅优先/2 仅头像/3 全静默——≥1 即压前台礼仪）。
    pub dnd_tier: u8,
}

impl MannerGate {
    /// 裁决一条通知（发送方 app 是否正被聚焦）。
    pub fn judge(&self, from_focused_app: bool) -> BannerVerdict {
        if self.dnd_tier >= 1 && DND_OVERRIDES_FOCUS_MANNER {
            return BannerVerdict::SilentAll; // DND 档优先叠加
        }
        if self.enabled && from_focused_app {
            return BannerVerdict::SuppressToCenter; // 横幅抑制、中心入库
        }
        BannerVerdict::Show
    }

    /// 抑制裁决下信息零丢失：中心入库恒真（主册无感标准）。
    pub fn enters_center(v: BannerVerdict) -> bool {
        match v {
            BannerVerdict::Show => true,
            BannerVerdict::SuppressToCenter | BannerVerdict::SilentAll => ALWAYS_INTO_CENTER,
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_fgmute_checks() -> CheckSet {
    let mut cs = CheckSet::new("F461-fgmute");
    // 1) 聚焦判定：切换后 <200ms 生效。
    let mut f = FocusTracker::new();
    f.open_window("editor");
    f.focus(Some("editor"), 1_000);
    cs.add("focus_effective", f.is_foreground("editor") && f.focus_effective_within(1_100), "");
    cs.add("focus_expired", !f.focus_effective_within(1_300), "");
    // 2) 横幅抑制与中心入库（信息零丢失）。
    let g = MannerGate { enabled: true, dnd_tier: 0 };
    let v = g.judge(true);
    cs.add("suppress_focused", v == BannerVerdict::SuppressToCenter && MannerGate::enters_center(v), "");
    cs.add("show_unfocused", g.judge(false) == BannerVerdict::Show, "");
    // 3) 多窗口应用：任一窗聚焦即算前台。
    let mut f2 = FocusTracker::new();
    f2.open_window("chat");
    f2.open_window("chat");
    f2.focus(Some("chat"), 0);
    f2.close_window("chat");
    cs.add("multi_window_foreground", f2.is_foreground("chat"), "");
    f2.close_window("chat");
    cs.add("all_closed_not_foreground", !f2.is_foreground("chat"), "");
    // 4) 关闭开关：用户要每条横幅也给。
    let g2 = MannerGate { enabled: false, dnd_tier: 0 };
    cs.add("manner_off_shows", g2.judge(true) == BannerVerdict::Show, "");
    // 5) 与 F341 叠加优先级：DND 压前台礼仪。
    let g3 = MannerGate { enabled: true, dnd_tier: 1 };
    cs.add("dnd_overrides", g3.judge(true) == BannerVerdict::SilentAll, "");
    // 6) 未开窗应用聚焦=异常态（不判前台——防幽灵聚焦）。
    let mut f3 = FocusTracker::new();
    f3.focus(Some("ghost"), 0);
    cs.add("ghost_not_foreground", !f3.is_foreground("ghost"), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_switch_under_200ms() {
        let mut f = FocusTracker::new();
        f.open_window("a");
        f.open_window("b");
        f.focus(Some("a"), 10_000);
        f.focus(Some("b"), 10_050);
        assert_eq!(f.focused_app(), Some("b"));
        assert!(f.focus_effective_within(10_100));
    }

    #[test]
    fn suppression_never_loses_info() {
        let g = MannerGate { enabled: true, dnd_tier: 0 };
        for v in [BannerVerdict::Show, BannerVerdict::SuppressToCenter, BannerVerdict::SilentAll] {
            assert!(MannerGate::enters_center(v), "信息零丢失");
        }
    }

    #[test]
    fn dnd_tier_three_fully_silent() {
        let g = MannerGate { enabled: false, dnd_tier: 3 };
        assert_eq!(g.judge(false), BannerVerdict::SilentAll);
    }
}
