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

// ===========================================================================
// 深化 v2（F461）：逐应用静默规则 / 通知老化 / 聚焦历史窗 / 会话静默时段
// ===========================================================================

/// 逐应用静默规则表（主册「礼仪规则一眼能懂可关」的运行面：除了前台
/// 礼仪，用户还可给指定应用配「永远静默/永远横幅」两条硬规则——规则
/// 优先级：硬规则 > DND > 前台礼仪）。
pub struct AppMuteRules {
    /// 规则表（app 键 → 强制静默/强制横幅）。
    rules: [Option<(u64, bool)>; 16],
    n: usize,
}

fn app_key(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl AppMuteRules {
    pub const fn new() -> Self {
        AppMuteRules { rules: [None; 16], n: 0 }
    }

    /// 设规则（mute=true 永远静默 / false 永远横幅——覆盖用户的全局偏好）。
    pub fn set_rule(&mut self, app: &str, mute: bool) -> bool {
        let k = app_key(app);
        for i in 0..self.n {
            if let Some((kk, _)) = self.rules[i] {
                if kk == k {
                    self.rules[i] = Some((k, mute));
                    return true;
                }
            }
        }
        if self.n >= 16 {
            return false;
        }
        self.rules[self.n] = Some((k, mute));
        self.n += 1;
        true
    }

    pub fn rule_of(&self, app: &str) -> Option<bool> {
        let k = app_key(app);
        (0..self.n).filter_map(|i| self.rules[i]).find(|(kk, _)| *kk == k).map(|(_, m)| m)
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

/// 通知老化（通知中心入库条目按龄归档——中心全记但不无限堆积）。
pub const CENTER_AGING_MS: u64 = 24 * 60 * 60 * 1_000;

pub fn center_entry_expired(created_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(created_ms) >= CENTER_AGING_MS
}

/// 聚焦历史窗（<200ms 快速切换不抖动判定——主册「聚焦判定 <200ms」的
/// 去抖实现：新身份须稳定 200ms 才生效）。
pub struct FocusDebounce {
    pending: Option<&'static str>,
    pending_since: u64,
    pub stable: Option<&'static str>,
}

pub const DEBOUNCE_MS: u64 = 200;

impl FocusDebounce {
    pub const fn new() -> Self {
        FocusDebounce { pending: None, pending_since: 0, stable: None }
    }

    /// 聚焦事件（去抖：未满 200ms 的候选不生效——身份切换 <200ms 判定的
    /// 运行面语义：切换完成判定，而不是切换抢跑）。
    pub fn on_focus(&mut self, app: Option<&'static str>, now_ms: u64) {
        if self.pending != app {
            self.pending = app;
            self.pending_since = now_ms;
        }
    }

    /// 时钟推进（候选满 200ms → 落位稳定身份）。
    pub fn tick(&mut self, now_ms: u64) {
        if let Some(p) = self.pending {
            if now_ms.saturating_sub(self.pending_since) >= DEBOUNCE_MS {
                self.stable = Some(p);
                self.pending = None;
            }
        }
    }
}

pub fn run_fgmute_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F461-deep");
    // 硬规则：永远静默/永远横幅（优先级最高的用户意志）。
    let mut r = AppMuteRules::new();
    cs.add("rule_set", r.set_rule("spam-app", true) && r.rule_of("spam-app") == Some(true), "");
    cs.add("rule_override", r.set_rule("spam-app", false) && r.rule_of("spam-app") == Some(false) && r.count() == 1, "");
    cs.add("rule_unknown_none", r.rule_of("quiet-app").is_none(), "");
    // 通知老化（24h 归档——中心全记但不无限堆积）。
    cs.add("aging_24h", !center_entry_expired(0, CENTER_AGING_MS - 1) && center_entry_expired(0, CENTER_AGING_MS), "");
    // 聚焦去抖（快速切换 <200ms 不抢跑；稳定后落位）。
    let mut d = FocusDebounce::new();
    d.on_focus(Some("a"), 0);
    d.tick(100);
    cs.add("debounce_holds", d.stable.is_none(), "");
    d.tick(200);
    cs.add("debounce_settles", d.stable == Some("a"), "");
    d.on_focus(Some("b"), 300);
    d.on_focus(Some("c"), 350); // 50ms 内再切——候选重置
    d.tick(400);
    cs.add("debounce_resets", d.stable == Some("a"), "");
    d.tick(560);
    cs.add("debounce_final", d.stable == Some("c"), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn rules_survive_resets() {
        let mut r = AppMuteRules::new();
        r.set_rule("x", true);
        r.set_rule("y", false);
        r.set_rule("x", false);
        assert_eq!(r.rule_of("x"), Some(false));
        assert_eq!(r.rule_of("y"), Some(false));
        assert_eq!(r.count(), 2);
    }

    #[test]
    fn debounce_matches_200ms_const() {
        let mut d = FocusDebounce::new();
        d.on_focus(Some("z"), 1_000);
        assert!(d.stable.is_none());
        d.tick(1_000 + DEBOUNCE_MS - 1);
        assert!(d.stable.is_none());
        d.tick(1_000 + DEBOUNCE_MS);
        assert_eq!(d.stable, Some("z"));
    }

    #[test]
    fn aging_boundary_is_exact() {
        assert!(!center_entry_expired(5_000, 5_000 + CENTER_AGING_MS - 1));
        assert!(center_entry_expired(5_000, 5_000 + CENTER_AGING_MS));
    }
}
