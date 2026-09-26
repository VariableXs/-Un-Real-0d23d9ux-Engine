//! 深化层 · F585 焦点跟随鼠标（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F585 节）：
//! ①「跟随判定滞回门」——窗口间快速抖动跨越（间隔 < HYST_MS）不启动
//!   驻留（防扫过抖动），同窗移动照常放行（基础件驻留可积累）；
//! ②「透传分账」——跟随获焦与点击动作两条独立账：首击动作完整送达
//!   （点击永不被跟随吞），计数互不混淆；
//! ③「浮层豁免 O(1) 引擎」——豁免类别位图化：任意浮层在显即整门封闭
//!   （一次位测试判定，替代逐类扫描）；
//! ④「开关关闭零侵入」——关闭后移动事件只答命中、零转发零状态变更
//!   （对账：转发计数 = 0），点击照常（既有焦点链行为不动）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::focusfollow::{ExemptKind, FocusFollow, EXEMPT_KINDS, DWELL_MS};

use alloc::vec::Vec;

/// 抖动滞回阈值（ms——两次换窗间隔小于此值视为扫过抖动，不启动驻留）。
pub const HYST_MS: u64 = 60;

// ---------------------------------------------------------------------------
// 浮层豁免 O(1) 位图引擎
// ---------------------------------------------------------------------------

/// 浮层豁免位图（每类一位——在显即封门，判定 O(1)）。
pub struct OverlayBits {
    bits: u8,
}

impl OverlayBits {
    pub fn new() -> OverlayBits { OverlayBits { bits: 0 } }

    fn bit_of(kind: ExemptKind) -> u8 {
        match kind {
            ExemptKind::ImeCandidate => 1 << 0,
            ExemptKind::TrayFlyout => 1 << 1,
            ExemptKind::ToastBanner => 1 << 2,
        }
    }

    /// 浮层在显（置位）。
    pub fn show(&mut self, kind: ExemptKind) { self.bits |= Self::bit_of(kind); }

    /// 浮层关闭（清位；未置位先清安全无副作用）。
    pub fn hide(&mut self, kind: ExemptKind) { self.bits &= !Self::bit_of(kind); }

    /// 任意浮层在显？O(1) 一次位测试。
    pub fn any_shown(&self) -> bool { self.bits != 0 }

    pub fn shown_count(&self) -> u32 { self.bits.count_ones() }
}

impl Default for OverlayBits { fn default() -> Self { Self::new() } }

// ---------------------------------------------------------------------------
// 跟随门（滞回 + 豁免 + 零侵入三道闸，过闸事件进基础件引擎）
// ---------------------------------------------------------------------------

pub struct FollowGate {
    follow: FocusFollow,
    geo: Vec<(u64, i32, i32, u32, u32)>,
    bits: OverlayBits,
    last_win: Option<u64>,
    last_change_ms: Option<u64>,
    forwards: u32,
    swallowed_jitter: u32,
    blocked_overlay: u32,
    click_actions: u32,
}

impl FollowGate {
    pub fn new() -> FollowGate {
        FollowGate {
            follow: FocusFollow::new(),
            geo: Vec::new(),
            bits: OverlayBits::new(),
            last_win: None, last_change_ms: None,
            forwards: 0, swallowed_jitter: 0, blocked_overlay: 0, click_actions: 0,
        }
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.follow.set_enabled(on);
    }

    pub fn enabled(&self) -> bool { self.follow.enabled() }

    /// 几何同步（门与基础件双写——基础件引擎需要完整视野）。
    pub fn sync_win(&mut self, id: u64, x: i32, y: i32, w: u32, h: u32) {
        match self.geo.iter_mut().find(|(i, ..)| *i == id) {
            Some(slot) => *slot = (id, x, y, w, h),
            None => self.geo.push((id, x, y, w, h)),
        }
        self.follow.sync_win(id, x, y, w, h);
    }

    pub fn overlay(&mut self, kind: ExemptKind, shown: bool) {
        if shown {
            self.bits.show(kind);
        } else {
            self.bits.hide(kind);
        }
        self.follow.overlay(kind, shown);
    }

    fn hit(&self, x: i32, y: i32) -> Option<u64> {
        self.geo.iter().rev()
            .find(|(_, wx, wy, w, h)| x >= *wx && y >= *wy
                && (x - *wx) < *w as i32 && (y - *wy) < *h as i32)
            .map(|(id, ..)| *id)
    }

    /// 鼠标移动：三道闸 → 基础件驻留/转焦引擎（不重写引擎本体）。
    pub fn mouse_move(&mut self, x: i32, y: i32, ms: u64) -> Option<u64> {
        let id = self.hit(x, y);
        if !self.enabled() {
            return id; // 零侵入闸：只答命中，零转发零状态变更
        }
        if self.bits.any_shown() {
            self.blocked_overlay += 1;
            return None; // 浮层在显：整门封闭
        }
        // 滞回闸：只在「换窗」时判定——同窗移动照常放行（驻留可积累）。
        // 抖动被吞时刷新时钟（连续抖动全吞）且不动 last_win（弹回原窗按同窗放行）。
        if id != self.last_win {
            let since = match self.last_change_ms {
                Some(t) => ms.saturating_sub(t),
                None => u64::MAX,
            };
            self.last_change_ms = Some(ms);
            if since < HYST_MS {
                self.swallowed_jitter += 1;
                return None;
            }
        }
        self.last_win = id;
        self.forwards += 1;
        self.follow.mouse_move(x, y, ms)
    }

    /// 点击：透传账——动作完整送达（返回命中窗），与跟随互不吞。
    pub fn click(&mut self, x: i32, y: i32) -> Option<u64> {
        let hit = self.follow.click(x, y);
        if hit.is_some() {
            self.click_actions += 1;
        }
        hit
    }

    pub fn focus(&self) -> Option<u64> { self.follow.focus() }

    pub fn pass_through_count(&self) -> u32 { self.follow.pass_through_count() }

    pub fn forwards(&self) -> u32 { self.forwards }

    pub fn swallowed_jitter(&self) -> u32 { self.swallowed_jitter }

    pub fn blocked_overlay(&self) -> u32 { self.blocked_overlay }

    pub fn click_actions(&self) -> u32 { self.click_actions }

    pub fn overlays_shown(&self) -> bool { self.bits.any_shown() }
}

impl Default for FollowGate { fn default() -> Self { Self::new() } }

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f585_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 零侵入（默认关）：移动只答命中——零转发、零转焦。
    let mut g = FollowGate::new();
    g.sync_win(1, 0, 0, 800, 600);
    g.sync_win(2, 800, 0, 800, 600);
    let m1 = g.mouse_move(100, 100, 0);
    let m2 = g.mouse_move(900, 300, 10);
    cs.add(
        "disabled zero intrusion",
        !g.enabled() && m1 == Some(1) && m2 == Some(2)
            && g.forwards() == 0
            && g.focus().is_none(),
        "",
    );

    // 2) 开启后驻留转焦照常（闸不误伤合法驻留——复用基础件 DWELL_MS）。
    g.set_enabled(true);
    g.mouse_move(100, 100, 1_000);
    let early = g.mouse_move(100, 100, 1_100).is_none();
    let got = g.mouse_move(100, 100, 1_120);
    cs.add(
        "dwell transfer kept through gate",
        early && got == Some(1) && g.focus() == Some(1),
        "",
    );

    // 3) 抖动滞回：A→B→A→B 每 25ms 一跨（< HYST_MS）——换窗事件全吞、零转焦；安定后驻留到站。
    let mut g3 = FollowGate::new();
    g3.sync_win(1, 0, 0, 800, 600);
    g3.sync_win(2, 800, 0, 800, 600);
    g3.set_enabled(true);
    let _ = g3.mouse_move(100, 100, 0); // A 落定
    let _ = g3.mouse_move(900, 300, 25); // 抖到 B——吞
    let _ = g3.mouse_move(100, 100, 50); // 抖回 A——同窗放行
    let _ = g3.mouse_move(900, 300, 75); // 又抖到 B——吞
    let jitter_ok = g3.swallowed_jitter() == 2 && g3.focus().is_none();
    let _ = g3.mouse_move(900, 300, 500); // 安定进 B（距上次跨 425ms）
    let settled = g3.mouse_move(900, 300, 620); // 驻留 120ms 到站
    cs.add(
        "jitter hysteresis then settle",
        jitter_ok && settled == Some(2) && g3.focus() == Some(2),
        "",
    );

    // 4) 浮层豁免 O(1)：IME 在显 → 门封（逐次计账）；关掉 → 恢复转焦。
    let mut g4 = FollowGate::new();
    g4.set_enabled(true);
    g4.sync_win(1, 0, 0, 800, 600);
    g4.sync_win(2, 800, 0, 800, 600);
    g4.overlay(ExemptKind::ImeCandidate, true);
    let _ = g4.mouse_move(100, 100, 0);
    let _ = g4.mouse_move(900, 300, 10);
    let shielded = g4.focus().is_none() && g4.blocked_overlay() == 2;
    g4.overlay(ExemptKind::ImeCandidate, false);
    let _ = g4.mouse_move(900, 300, 1_000);
    let resumed = g4.mouse_move(900, 300, 1_120) == Some(2);
    cs.add(
        "overlay bits gate and resume",
        shielded && resumed && !g4.overlays_shown(),
        "",
    );

    // 5) 豁免位图引擎：三类各占一位、O(1) 判定、计数诚实。
    let mut bits = OverlayBits::new();
    bits.show(ExemptKind::ImeCandidate);
    bits.show(ExemptKind::TrayFlyout);
    bits.show(ExemptKind::ToastBanner);
    let all_on = bits.any_shown() && bits.shown_count() == 3;
    bits.hide(ExemptKind::TrayFlyout);
    cs.add(
        "overlay bits engine",
        all_on && bits.any_shown() && bits.shown_count() == 2,
        "",
    );

    // 6) 透传分账：无驻留首击直接动作（点击不被跟随吞）——click 账独立于 follow 转发账。
    let mut g6 = FollowGate::new();
    g6.set_enabled(true);
    g6.sync_win(5, 0, 0, 100, 100);
    let first = g6.click(50, 50);
    cs.add(
        "first click passes through",
        first == Some(5) && g6.click_actions() == 1
            && g6.focus() == Some(5)
            && g6.pass_through_count() == 1
            && g6.forwards() == 0,
        "",
    );

    // 7) 分账不变式：先跟随转焦窗 1，再点击窗 5——两账独立计数，点击直达。
    let mut g7 = FollowGate::new();
    g7.set_enabled(true);
    g7.sync_win(1, 0, 0, 400, 400);
    g7.sync_win(5, 400, 0, 400, 400);
    let _ = g7.mouse_move(100, 100, 0);
    let follow_hit = g7.mouse_move(100, 100, 120); // 跟随转焦窗 1
    let _ = g7.click(500, 50); // 点击窗 5——动作直达
    cs.add(
        "dual ledger separate",
        follow_hit == Some(1) && g7.click_actions() == 1
            && g7.pass_through_count() == 1
            && g7.focus() == Some(5),
        "",
    );

    // 8) 基础常量复用证据：驻留时长与豁免清单单一源（不另立口径）。
    cs.add(
        "base constants reused",
        DWELL_MS == 120 && EXEMPT_KINDS.len() == 3,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hide_before_show_is_safe() {
        let mut b = OverlayBits::new();
        b.hide(ExemptKind::ToastBanner); // 未置位先清——安全无副作用
        assert!(!b.any_shown());
        b.show(ExemptKind::ToastBanner);
        assert_eq!(b.shown_count(), 1);
    }

    #[test]
    fn jitter_boundary_exactly_hyst_passes() {
        let mut g = FollowGate::new();
        g.set_enabled(true);
        g.sync_win(1, 0, 0, 100, 100);
        g.sync_win(2, 100, 0, 100, 100);
        let _ = g.mouse_move(50, 50, 0);
        let _ = g.mouse_move(150, 50, HYST_MS); // 恰满滞回窗——放行
        assert_eq!(g.swallowed_jitter(), 0);
    }

    #[test]
    fn click_outside_no_action() {
        let mut g = FollowGate::new();
        g.sync_win(1, 0, 0, 100, 100);
        assert!(g.click(500, 500).is_none());
        assert_eq!(g.click_actions(), 0);
    }
}
