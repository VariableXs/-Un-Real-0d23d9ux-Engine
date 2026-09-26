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

// ===========================================================================
// 深化 v4（F461）：三级优先级裁决管线 / DND 四档矩阵 / 容量诚实面 /
// 窗口账下溢保护
// ===========================================================================

/// 统一裁决管线（优先级链一处一事实：应用硬规则 > DND 档（F341）>
/// 前台礼仪（本项主体）——v1 的 judge(from_focused_app) 只吃布尔，
/// 本入口把身份追踪与规则表接进同一条链，调用方零拼装）。
pub fn decide(
    tracker: &FocusTracker,
    rules: &AppMuteRules,
    gate: &MannerGate,
    app: &'static str,
) -> BannerVerdict {
    if let Some(mute) = rules.rule_of(app) {
        // 硬规则是用户明示意志：永远静默 / 永远横幅（压过 DND 与礼仪）。
        return if mute { BannerVerdict::SilentAll } else { BannerVerdict::Show };
    }
    gate.judge(tracker.is_foreground(app))
}

/// DND 四档语义表（F341：0 关 / 1 仅优先 / 2 仅头像 / 3 全静默——
/// ≥1 均压前台礼仪；越界档按最严处理，不猜不装）。
pub fn dnd_tier_verdict(tier: u8) -> BannerVerdict {
    match tier {
        0 => BannerVerdict::Show,          // 关：走前台礼仪正常链
        1..=3 => BannerVerdict::SilentAll, // 任一开启档：全静默
        _ => BannerVerdict::SilentAll,
    }
}

/// 容量诚实面锚（8 应用槽 / 16 规则槽——满额拒绝返回 false，不静默覆盖）。
pub const APP_SLOT_CAP: usize = 8;
pub const RULE_SLOT_CAP: usize = 16;

pub fn run_fgmute_v4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F461-v4");
    // 1) 三级优先级管线：硬规则「永远静默」压过前台礼仪。
    let mut tracker = FocusTracker::new();
    let _ = tracker.open_window("editor");
    tracker.focus(Some("editor"), 1_000);
    let mut rules = AppMuteRules::new();
    let _ = rules.set_rule("editor", true);
    let gate = MannerGate { enabled: true, dnd_tier: 0 };
    cs.add("hardrule_mute_wins", decide(&tracker, &rules, &gate, "editor") == BannerVerdict::SilentAll, "");
    // 硬规则「永远横幅」压过 DND 3 档（用户明示要看）。
    let _ = rules.set_rule("editor", false);
    let loud_gate = MannerGate { enabled: true, dnd_tier: 3 };
    cs.add("hardrule_banner_beats_dnd", decide(&tracker, &rules, &loud_gate, "editor") == BannerVerdict::Show, "");
    // 2) 无硬规则时走 DND > 礼仪链：DND 关 + 前台聚焦 → 抑制入中心。
    let rules_clean = AppMuteRules::new();
    let plain_gate = MannerGate { enabled: true, dnd_tier: 0 };
    cs.add("no_rule_uses_manner", decide(&tracker, &rules_clean, &plain_gate, "editor") == BannerVerdict::SuppressToCenter, "");
    // 3) DND 四档矩阵（0=走礼仪链；1/2/3=全静默）。
    cs.add("dnd_matrix", dnd_tier_verdict(0) == BannerVerdict::Show
        && dnd_tier_verdict(1) == BannerVerdict::SilentAll
        && dnd_tier_verdict(2) == BannerVerdict::SilentAll
        && dnd_tier_verdict(3) == BannerVerdict::SilentAll, "");
    // 4) 容量诚实面：窗口槽 8 满拒绝、规则槽 16 满拒绝。
    let mut many = FocusTracker::new();
    let apps = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let mut opened = 0usize;
    for a in apps {
        if many.open_window(a) {
            opened += 1;
        }
    }
    cs.add("app_cap_exact", opened == APP_SLOT_CAP && !many.open_window("i"), "");
    let mut full_rules = AppMuteRules::new();
    for i in 0..RULE_SLOT_CAP + 2 {
        let name = match i {
            0 => "r0", 1 => "r1", 2 => "r2", 3 => "r3", 4 => "r4", 5 => "r5",
            6 => "r6", 7 => "r7", 8 => "r8", 9 => "r9", 10 => "r10", 11 => "r11",
            12 => "r12", 13 => "r13", 14 => "r14", _ => "rx",
        };
        let _ = full_rules.set_rule(name, true);
    }
    // 16 槽恰好收下 16 个不同应用（同名覆盖不入新账；超额诚实拒绝）。
    cs.add("rule_cap_exact", full_rules.count() == RULE_SLOT_CAP, "");
    // 5) 窗口账下溢保护：未开窗关闭 = false；全关后再关 = false（账不透支）。
    let mut guard = FocusTracker::new();
    cs.add("close_unknown_false", !guard.close_window("never-opened"), "");
    let _ = guard.open_window("solo");
    let _ = guard.close_window("solo");
    cs.add("double_close_false", !guard.close_window("solo"), "");
    cs
}

#[cfg(test)]
mod v4_tests {
    use super::*;

    #[test]
    fn pipeline_full_chain_order() {
        // 无聚焦、无规则、DND 关 + 礼仪开 → 未聚焦应用正常弹横幅。
        let t = FocusTracker::new();
        let r = AppMuteRules::new();
        let g = MannerGate { enabled: true, dnd_tier: 0 };
        assert_eq!(decide(&t, &r, &g, "somewhere-else"), BannerVerdict::Show);
    }

    #[test]
    fn app_slot_cap_then_frees() {
        let mut t = FocusTracker::new();
        for a in ["a", "b", "c", "d", "e", "f", "g", "h"] {
            assert!(t.open_window(a));
        }
        assert!(!t.open_window("i")); // 满额诚实拒绝
        let _ = t.close_window("a");  // 腾出一格又能进
        assert!(t.open_window("i"));
    }

    #[test]
    fn dnd_overflow_tier_is_strictest() {
        assert_eq!(dnd_tier_verdict(200), BannerVerdict::SilentAll);
    }
}


// ===========================================================================
// 深化 v7（F461）：裁决审计账 / 静音规则清单持久化（W7G1）/
// 聚焦抖动指纹 / 免打扰分档回归
// ===========================================================================
//
// v7 主轴（主册判据的二阶展开）：
// 1. 裁决审计账——每条横幅的「放行/降级/静默」记账：单调守卫 +
//   逐裁决 tally + 抑制率（前台礼貌的量化面）。
// 2. 规则清单持久化——用户逐应用静音规则是用户数据：W7G1 完整性
//   清单（count + 全表 FNV 摘要——与 F465 同范式）。
// 3. 聚焦抖动指纹——200ms 防抖窗口外的高频焦点切换 = 环境异常
//   （全屏应用轮询抢焦点），账面显性化。
// 4. 免打扰分档回归——DND 各档裁决语义（v3 面）复核。

use crate::genstar2::vxdict::fnv1a;

// ---------------------------------------------------------------------------
// 裁决审计账
// ---------------------------------------------------------------------------

/// 账面容量。
pub const VERDICT_LEDGER_CAP: usize = 16;

pub struct VerdictLedger {
    ring: [(u64, u8); VERDICT_LEDGER_CAP], // (时刻, 裁决码)
    head: usize,
    n: usize,
    pub out_of_order_rejected: usize,
}

impl VerdictLedger {
    pub const fn new() -> Self {
        VerdictLedger { ring: [(0, 0); VERDICT_LEDGER_CAP], head: 0, n: 0, out_of_order_rejected: 0 }
    }

    pub fn push(&mut self, at_ms: u64, verdict_code: u8) -> bool {
        if self.n > 0 {
            let last = (self.head + VERDICT_LEDGER_CAP - 1) % VERDICT_LEDGER_CAP;
            if at_ms < self.ring[last].0 {
                self.out_of_order_rejected += 1;
                return false;
            }
        }
        self.ring[self.head] = (at_ms, verdict_code);
        self.head = (self.head + 1) % VERDICT_LEDGER_CAP;
        self.n = (self.n + 1).min(VERDICT_LEDGER_CAP);
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 逐裁决码 tally（码 0-255 全域；返回高频码 top 无需——直接全表
    /// tally 用 [u16; 3]：0=横幅 1=仅中心 2=静默，码 >2 不记账时拒绝）。
    pub fn tally3(&self) -> Option<[u16; 3]> {
        let mut t = [0u16; 3];
        for i in 0..self.n {
            let idx = (self.head + VERDICT_LEDGER_CAP - self.n + i) % VERDICT_LEDGER_CAP;
            let c = self.ring[idx].1;
            if c > 2 {
                return None; // 账面出现非法码 = 账本被污染（显性化）
            }
            t[c as usize] += 1;
        }
        Some(t)
    }

    /// 抑制率 permille（静默 + 仅中心 ÷ 总数——前台礼貌的量化）。
    pub fn suppression_permille(&self) -> Option<u32> {
        let t = self.tally3()?;
        if self.n == 0 {
            return None;
        }
        let suppressed = t[1] + t[2];
        Some((suppressed as u32 * 1_000 / self.n as u32) as u32)
    }
}

// ---------------------------------------------------------------------------
// 聚焦抖动指纹
// ---------------------------------------------------------------------------

/// 抖动窗（2s 内 ≥5 次焦点切换）。
pub const FOCUS_JITTER_WINDOW_MS: u64 = 2_000;
pub const FOCUS_JITTER_THRESHOLD: usize = 5;

pub struct FocusJitter {
    ring: [u64; FOCUS_JITTER_THRESHOLD + 8], // 切换时刻
    head: usize,
    n: usize,
}

impl FocusJitter {
    pub const fn new() -> Self {
        FocusJitter { ring: [0; FOCUS_JITTER_THRESHOLD + 8], head: 0, n: 0 }
    }

    pub fn record(&mut self, at_ms: u64) {
        self.ring[self.head] = at_ms;
        self.head = (self.head + 1) % self.ring.len();
        self.n = (self.n + 1).min(self.ring.len());
    }

    /// 窗内切换次数 ≥ 阈值 = 抖动（环境异常显性化）。
    pub fn jittering(&self, now_ms: u64) -> bool {
        (0..self.n)
            .take_while(|&i| {
                let idx = (self.head + self.ring.len() - 1 - i) % self.ring.len();
                now_ms.saturating_sub(self.ring[idx]) <= FOCUS_JITTER_WINDOW_MS
            })
            .count() >= FOCUS_JITTER_THRESHOLD
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 规则清单持久化（W7G1 完整性清单）
// ---------------------------------------------------------------------------

/// v7 魔标（W7G 族）。
pub const FGMUTE_V7_MAGIC: [u8; 4] = *b"W7G1";
/// 长度：魔标(4) + 版本(1) + count(1) + 保留(1) + 摘要(4) + FNV(4) = 16。
pub const FGMUTE_V7_LEN: usize = 16;
pub const FGMUTE_V7_VERSION: u8 = 1;

/// 规则表摘要（app 名 + 静音位 串联 FNV——顺序敏感）。
pub fn rules_digest(rules: &[(&str, bool)]) -> Option<u32> {
    let mut buf = [0u8; 512];
    let mut w = 0usize;
    for (app, mute) in rules {
        for part in [app.as_bytes(), b"\x01", &[*mute as u8], b"\x02"] {
            if w + part.len() > buf.len() {
                return None;
            }
            buf[w..w + part.len()].copy_from_slice(part);
            w += part.len();
        }
    }
    Some(fnv1a(&buf[..w]))
}

/// 序列化（规则清单完整性存档）。
pub fn save_rules_manifest_v7(rules: &[(&str, bool)], out: &mut [u8]) -> Option<usize> {
    if rules.len() > RULE_SLOT_CAP || out.len() < FGMUTE_V7_LEN {
        return None;
    }
    out[..4].copy_from_slice(&FGMUTE_V7_MAGIC);
    out[4] = FGMUTE_V7_VERSION;
    out[5] = rules.len() as u8;
    out[6] = 0;
    let digest = rules_digest(rules)?;
    out[7..11].copy_from_slice(&digest.to_le_bytes());
    let h = fnv1a(&out[..11]);
    out[11] = (h & 0xff) as u8;
    out[12] = ((h >> 8) & 0xff) as u8;
    out[13] = ((h >> 16) & 0xff) as u8;
    out[14] = ((h >> 24) & 0xff) as u8;
    Some(FGMUTE_V7_LEN)
}

/// 反序列化 + 核对（与当前规则表比对——不一致 = 已被改动）。
pub fn verify_rules_manifest_v7(buf: &[u8], rules: &[(&str, bool)]) -> Option<bool> {
    if buf.len() < FGMUTE_V7_LEN || buf[..4] != FGMUTE_V7_MAGIC {
        return None;
    }
    if buf[4] != FGMUTE_V7_VERSION || buf[6] != 0 {
        return None;
    }
    let expect = fnv1a(&buf[..11]);
    let got = buf[11] as u32
        | ((buf[12] as u32) << 8)
        | ((buf[13] as u32) << 16)
        | ((buf[14] as u32) << 24);
    if expect != got {
        return None;
    }
    if buf[5] as usize != rules.len() {
        return Some(false);
    }
    let digest = rules_digest(rules)?;
    let mut stored = [0u8; 4];
    stored.copy_from_slice(&buf[7..11]);
    Some(digest == u32::from_le_bytes(stored))
}

// ---------------------------------------------------------------------------
// 域自检（F461 v7）
// ---------------------------------------------------------------------------

pub fn run_fgmute_v7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F461-v7");
    // 1) 裁决审计账：tally + 抑制率 + 非法码显性 + 单调守卫。
    cs.add("verdict_tally", {
        let mut led = VerdictLedger::new();
        let _ = led.push(100, 0);
        let _ = led.push(200, 1);
        let _ = led.push(300, 2);
        let _ = led.push(400, 2);
        led.tally3() == Some([1, 1, 2])
    }, "");
    cs.add("verdict_suppression_rate", {
        let mut led = VerdictLedger::new();
        let _ = led.push(100, 0);
        for i in 1..4u64 {
            let _ = led.push(i * 100, 2);
        }
        led.suppression_permille() == Some(750) // 3/4 静默
    }, "");
    cs.add("verdict_bad_code_visible", {
        let mut led = VerdictLedger::new();
        let _ = led.push(100, 9); // 非法码入账 → tally 显性 None
        led.tally3().is_none()
    }, "");
    cs.add("verdict_monotonic", {
        let mut led = VerdictLedger::new();
        let _ = led.push(1_000, 0);
        !led.push(500, 1) && led.out_of_order_rejected == 1
    }, "");
    cs.add("verdict_empty_honest", VerdictLedger::new().suppression_permille().is_none(), "");
    // 2) 聚焦抖动：窗内 5 次 = 抖动、分散 = 干净。
    cs.add("focus_jitter_detected", {
        let mut j = FocusJitter::new();
        for i in 0..FOCUS_JITTER_THRESHOLD {
            j.record(i as u64 * 300); // 2s 内 5 次
        }
        j.jittering(1_500)
    }, "");
    cs.add("focus_calm_clean", {
        let mut j = FocusJitter::new();
        for i in 0..3u64 {
            j.record(i * 10_000);
        }
        !j.jittering(30_000)
    }, "");
    // 3) 规则清单持久化：存档一致 / 内容变 / 数量变 / 篡改。
    let mut buf = [0u8; FGMUTE_V7_LEN];
    cs.add("rules_manifest_match", {
        let n = save_rules_manifest_v7(&[("game", true), ("mail", false)], &mut buf).unwrap_or(0);
        verify_rules_manifest_v7(&buf[..n], &[("game", true), ("mail", false)]) == Some(true)
    }, "");
    cs.add("rules_manifest_content_changed", {
        let n = save_rules_manifest_v7(&[("game", true)], &mut buf).unwrap_or(0);
        verify_rules_manifest_v7(&buf[..n], &[("game", false)]) == Some(false)
    }, "");
    cs.add("rules_manifest_count_changed", {
        let n = save_rules_manifest_v7(&[("game", true)], &mut buf).unwrap_or(0);
        verify_rules_manifest_v7(&buf[..n], &[("game", true), ("mail", false)]) == Some(false)
    }, "");
    cs.add("rules_manifest_tamper", {
        let n = save_rules_manifest_v7(&[("game", true)], &mut buf).unwrap_or(0);
        let mut bad = buf;
        bad[5] ^= 0x01;
        verify_rules_manifest_v7(&bad[..n], &[("game", true)]).is_none()
    }, "");
    // 4) 免打扰分档回归（v3 面）+ 前台礼貌回归（v1 面）。
    cs.add("dnd_tier_regression", {
        dnd_tier_verdict(0) == BannerVerdict::Show
            && dnd_tier_verdict(3) == BannerVerdict::SilentAll
    }, "");
    cs.add("focus_switch_const_regression", FOCUS_SWITCH_MS == 200, "");
    cs
}

#[cfg(test)]
mod v7_tests {
    use super::*;

    #[test]
    fn suppression_full_and_zero() {
        let mut all_quiet = VerdictLedger::new();
        let mut all_banner = VerdictLedger::new();
        for i in 0..4u64 {
            let _ = all_quiet.push(i * 100, 2);
            let _ = all_banner.push(i * 100, 0);
        }
        assert_eq!(all_quiet.suppression_permille(), Some(1_000));
        assert_eq!(all_banner.suppression_permille(), Some(0));
    }

    #[test]
    fn jitter_rings_and_stays_honest() {
        let mut j = FocusJitter::new();
        assert!(!j.jittering(0), "空账不抖");
        for i in 0..(FOCUS_JITTER_THRESHOLD + 8) {
            j.record(i as u64 * 100);
        }
        assert_eq!(j.count(), FOCUS_JITTER_THRESHOLD + 8);
        assert!(j.jittering(1_400));
    }

    #[test]
    fn digest_order_sensitive() {
        assert_ne!(
            rules_digest(&[("a", true), ("b", false)]),
            rules_digest(&[("b", false), ("a", true)])
        );
    }
}
