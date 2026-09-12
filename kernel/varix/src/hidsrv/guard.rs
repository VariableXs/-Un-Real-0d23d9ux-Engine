//! VARIABLE-200 AI-06 · 输入域仪表/手势/验收面（F146~F150）。
//!
//! 纯逻辑 + 固定容量数组，无分配。

use crate::checks::CheckSet;
use crate::galaxy::rt::DetPrng;
use crate::hidsrv::{
    event_bit, EventKind, EventQueue, FocusSystem, InputEvent, KeyboardFsm,
    Ps2Keyboard, Ps2Mouse, KeyPhase, SC_A, SC_E0, SC_F0, SC_LCTRL,
};
use crate::hidsrv::keys::{
    FloodGuard, InjectionGuard, InputSource, LatencyMeter, WheelMode, WheelSmoother, WheelTokens,
};
use crate::hidsrv::DisciplineVerdict;

// ---------------------------------------------------------------------------
// F146 输入仪表盘 — 实时事件率/延迟可视化（开发者模式）
// ---------------------------------------------------------------------------

pub const KIND_MAX: usize = 10;

#[derive(Clone, Copy)]
pub struct InputDashboard {
    pub enabled: bool,
    counts: [u64; KIND_MAX],
    window_us: u64,
    window_count: u64,
    pub p50_us: u32,
    pub p99_us: u32,
    /// 采样窗口数。
    pub windows: u64,
    /// 本窗口最坏延迟。
    pub worst_us: u32,
}

impl InputDashboard {
    pub const fn new(enabled: bool) -> InputDashboard {
        InputDashboard {
            enabled,
            counts: [0u64; KIND_MAX],
            window_us: 0,
            window_count: 0,
            p50_us: 0,
            p99_us: 0,
            windows: 0,
            worst_us: 0,
        }
    }

    /// 记录一条事件（未开启仪表时零成本：只累加计数）。
    pub fn on_event(&mut self, kind: EventKind, latency_us: u32) {
        let b = event_bit(kind);
        if b < KIND_MAX {
            self.counts[b] += 1;
        }
        self.window_count += 1;
        if latency_us > self.worst_us {
            self.worst_us = latency_us;
        }
        if !self.enabled {
            return;
        }
        if latency_us > self.p99_us {
            self.p99_us = latency_us;
        }
        if self.p50_us == 0 {
            self.p50_us = latency_us;
        } else {
            // 指数近似中位数（整数）。
            self.p50_us = (self.p50_us as u64 * 3 / 4 + latency_us as u64 / 4) as u32;
        }
    }

    /// 关闭采样窗口，返回本窗口事件率（事件/秒）。
    pub fn close_window(&mut self, now_us: u64) -> u64 {
        let span = now_us.saturating_sub(self.window_us);
        self.window_us = now_us;
        let rate = if span == 0 { 0 } else { self.window_count * 1_000_000 / span };
        self.window_count = 0;
        self.windows += 1;
        rate
    }

    pub fn count_of(&self, kind: EventKind) -> u64 {
        let b = event_bit(kind);
        if b < KIND_MAX {
            self.counts[b]
        } else {
            0
        }
    }

    pub fn total(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < KIND_MAX {
            n += self.counts[i];
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F147 触控板手势预留 — 双指滚动语义占位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GestureKind {
    None,
    TwoFingerScroll,
    TapClick,
    Pinch,
    Swipe,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScrollDelta {
    pub dx: i32,
    pub dy: i32,
}

/// 双指滚动：位移按令牌换算为滚轮格（符号与沿袭约定一致：上滑 = 上滚）。
pub fn two_finger_scroll(dy: i32, dx: i32, pixels_per_notch: i32) -> ScrollDelta {
    if pixels_per_notch <= 0 {
        return ScrollDelta { dx: 0, dy: 0 };
    }
    ScrollDelta { dx: dx / pixels_per_notch, dy: dy / pixels_per_notch }
}

#[derive(Clone, Copy)]
pub struct TrackpadState {
    pub fingers: u8,
    pub last: ScrollDelta,
    pub kind: GestureKind,
    /// 双指滚动累计格数。
    pub scrolled: i64,
    /// 轻拍计数。
    pub taps: u32,
}

impl TrackpadState {
    pub const fn new() -> TrackpadState {
        TrackpadState { fingers: 0, last: ScrollDelta { dx: 0, dy: 0 }, kind: GestureKind::None, scrolled: 0, taps: 0 }
    }

    pub fn update(&mut self, fingers: u8, dy: i32, dx: i32, pixels_per_notch: i32) -> GestureKind {
        self.fingers = fingers;
        if fingers == 2 {
            let d = two_finger_scroll(dy, dx, pixels_per_notch);
            self.last = d;
            self.scrolled += d.dy as i64;
            self.kind = GestureKind::TwoFingerScroll;
        } else {
            self.kind = GestureKind::None;
        }
        self.kind
    }

    pub fn tap(&mut self) -> bool {
        self.taps += 1;
        self.kind = GestureKind::TapClick;
        true
    }
}

/// 触控板能力占位：当前平台未启用（显式降级）。
pub const fn trackpad_supported() -> bool {
    false
}

// ---------------------------------------------------------------------------
// F148 事件域 fuzz — 恶意事件序列下内核与 UI 不倒
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct FuzzReport {
    pub bytes_fed: u32,
    /// 解码出的合法事件数。
    pub decoded: u32,
    /// 畸形序列数。
    pub malformed: u32,
    /// 是否全程无 panic/越界（有界收敛即存活）。
    pub survived: bool,
    /// 队列丢弃（有界丢弃，不是崩溃）。
    pub dropped: u64,
    /// 被限流的事件数。
    pub limited: u64,
}

/// 随机轰击键盘解码器：任意字节序列都必须有界收敛。
pub fn fuzz_keyboard(seed: u64, rounds: usize) -> FuzzReport {
    let mut rng = DetPrng::new(seed);
    let mut kb = Ps2Keyboard::new();
    let mut decoded = 0u32;
    let mut i = 0usize;
    while i < rounds {
        let b = (rng.next_u64() & 0xFF) as u8;
        if kb.feed(b).is_some() {
            decoded += 1;
        }
        i += 1;
    }
    FuzzReport {
        bytes_fed: rounds as u32,
        decoded,
        malformed: kb.malformed,
        survived: true,
        dropped: 0,
        limited: 0,
    }
}

/// 随机轰击鼠标解码器：同步位缺失的垃圾流必须被安全丢弃。
pub fn fuzz_mouse(seed: u64, rounds: usize) -> FuzzReport {
    let mut rng = DetPrng::new(seed);
    let mut ms = Ps2Mouse::new(true);
    let mut decoded = 0u32;
    let mut i = 0usize;
    while i < rounds {
        let b = (rng.next_u64() & 0xFF) as u8;
        if ms.feed(b).is_some() {
            decoded += 1;
        }
        i += 1;
    }
    FuzzReport {
        bytes_fed: rounds as u32,
        decoded,
        malformed: ms.malformed,
        survived: true,
        dropped: 0,
        limited: 0,
    }
}

/// 超频轰击：队列与限流器都必须有界。
pub fn fuzz_flood(seed: u64, rounds: usize) -> FuzzReport {
    let mut rng = DetPrng::new(seed);
    let mut q = EventQueue::new();
    let mut fg = FloodGuard::new(64, 1000);
    let mut decoded = 0u32;
    let mut i = 0usize;
    while i < rounds {
        let dev = (rng.next_u64() % 4) as u8;
        let now = (rng.next_u64() % 2000) as u64;
        if fg.admit(dev, now) {
            let e = InputEvent::motion(now, EventKind::MouseMove, 1, 1, dev);
            let _ = q.push(e);
            decoded += 1;
        }
        i += 1;
    }
    FuzzReport {
        bytes_fed: rounds as u32,
        decoded,
        malformed: 0,
        survived: q.len() <= crate::hidsrv::EVENT_QUEUE_CAP,
        dropped: q.dropped,
        limited: fg.total_limited,
    }
}

/// 事件域 fuzz 汇总。
pub fn fuzz_hid_domain(seeds: &[u64]) -> bool {
    let mut s = 0usize;
    while s < seeds.len() {
        let k = fuzz_keyboard(seeds[s], 256);
        let m = fuzz_mouse(seeds[s], 256);
        let f = fuzz_flood(seeds[s], 512);
        if !k.survived || !m.survived || !f.survived || f.decoded == 0 {
            return false;
        }
        // 键盘轰击必须要么解码出事件，要么明确记畸形——不允许"消失"。
        if k.decoded + k.malformed == 0 {
            return false;
        }
        s += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// F149 输入域自检 — 端到端闭环
// ---------------------------------------------------------------------------

/// 中断 → 解码 → 状态机 → 纪律 → 快捷键 → 焦点 → 延迟 闭环。
pub fn input_closed_loop() -> bool {
    let mut kb = Ps2Keyboard::new();
    let pressed = kb.feed(SC_LCTRL).is_some();
    let a = kb.feed(SC_A);
    let d = match a {
        Some(d) => d,
        None => return false,
    };
    if d.phase != KeyPhase::Press {
        return false;
    }
    let mut fsm = KeyboardFsm::new();
    let _ = fsm.down(SC_LCTRL, 0);
    let _ = fsm.down(SC_A, 1);
    let chord = fsm.is_chord(crate::hidsrv::MOD_CTRL, SC_A);
    let mut disc = crate::hidsrv::aurora_discipline();
    let verdict = disc.judge(crate::hidsrv::MOD_CTRL | crate::hidsrv::MOD_ALT, 0x2C);
    let mut focus = FocusSystem::new();
    let _ = focus.add(1, 0, true, 0);
    let _ = focus.add(2, 1, true, 1);
    let clicked = focus.click(1, 1, 2, true);
    let mut meter = LatencyMeter::new();
    let total = meter.record(2_000, 3_000, 4_000);
    // 扩展键：E0 前缀 + F0 断码。
    let _ = kb.feed(SC_E0);
    let _ = kb.feed(SC_F0);
    let mut inj = InjectionGuard::new();
    let blocked = !inj.admit(InputSource::Synthetic, crate::hidsrv::MOD_CTRL | crate::hidsrv::MOD_ALT, 0x5A);
    pressed
        && chord
        && verdict == DisciplineVerdict::Intercepted
        && clicked
        && total == 9_000
        && meter.within_redline()
        && blocked
        && kb.last().is_some()
}

// ---------------------------------------------------------------------------
// F150 手感验收流程 — 与 Windows 版盲测对比
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeelVerdict {
    /// 手感无损（Windows 侧胜出率低于阈值）。
    NoLoss,
    /// 存在可感知退化。
    Degraded,
    /// 样本不足，结论无效。
    Inconclusive,
}

/// 最小盲测样本数。
pub const FEEL_MIN_TRIALS: u32 = 20;
/// Windows 侧允许的胜出率上限（千分比）：10%。
pub const FEEL_LOSS_THRESHOLD_PERMILLE: u32 = 100;

#[derive(Clone, Copy)]
pub struct FeelAcceptance {
    pub trials: u32,
    pub wins_varix: u32,
    pub wins_windows: u32,
    pub ties: u32,
}

impl FeelAcceptance {
    pub const fn new() -> FeelAcceptance {
        FeelAcceptance { trials: 0, wins_varix: 0, wins_windows: 0, ties: 0 }
    }

    pub fn record(&mut self, varix_wins: bool, windows_wins: bool) {
        self.trials += 1;
        if varix_wins {
            self.wins_varix += 1;
        }
        if windows_wins {
            self.wins_windows += 1;
        }
        if !varix_wins && !windows_wins {
            self.ties += 1;
        }
    }

    pub fn windows_loss_permille(&self) -> u32 {
        if self.trials == 0 {
            return 0;
        }
        self.wins_windows * 1000 / self.trials
    }

    pub fn verdict(&self) -> FeelVerdict {
        if self.trials < FEEL_MIN_TRIALS {
            return FeelVerdict::Inconclusive;
        }
        if self.windows_loss_permille() <= FEEL_LOSS_THRESHOLD_PERMILLE {
            FeelVerdict::NoLoss
        } else {
            FeelVerdict::Degraded
        }
    }
}

/// 手感验收的仪表前置条件：延迟红线与仪表均已就位。
pub fn feel_preconditions_met(meter: &LatencyMeter) -> bool {
    meter.samples > 0 && meter.within_redline()
}

/// 滚轮手感令牌与按键重复设置的对齐检查（走查项）。
pub fn wheel_feel_aligned(tokens: &WheelTokens, mode: WheelMode) -> bool {
    tokens.valid()
        && ((mode == WheelMode::Line && tokens.lines_per_notch >= 1)
            || (mode == WheelMode::Pixel && tokens.pixels_per_line >= 8))
}

/// 事件回放一致性：同名序列两次回放必须逐事件一致（手感 bug 可复现）。
pub fn replay_deterministic() -> bool {
    let mut rec = crate::hidsrv::keys::Replayer::new();
    let _ = rec.start_record();
    let mut i = 0u64;
    while i < 8 {
        let _ = rec.record(InputEvent::key(i, EventKind::KeyDown, SC_A as u32, 0, 0));
        i += 1;
    }
    let _ = rec.stop_record();
    let mut a = [0u64; 8];
    let _ = rec.start_replay();
    let mut k = 0usize;
    while let Some(e) = rec.next() {
        a[k] = e.tick_us;
        k += 1;
    }
    let mut b = [0u64; 8];
    let _ = rec.start_replay();
    let mut j = 0usize;
    while let Some(e) = rec.next() {
        b[j] = e.tick_us;
        j += 1;
    }
    a == b && k == 8 && j == 8
}

/// 滚轮平滑在两种模式下的收敛（走查：不得残留）。
pub fn wheel_drains_clean(mode: WheelMode) -> bool {
    let mut w = WheelSmoother::new(mode, WheelTokens::default_ws());
    w.feed(3);
    let _ = w.drain_total(512);
    w.pending() == 0
}

// ---------------------------------------------------------------------------
// 自检扩展（F146~F150 共 5 项）
// ---------------------------------------------------------------------------

pub fn extend_checks(set: &mut CheckSet) {
    // F146 输入仪表盘
    let mut dash = InputDashboard::new(true);
    dash.on_event(EventKind::KeyDown, 4_000);
    dash.on_event(EventKind::KeyDown, 8_000);
    dash.on_event(EventKind::MouseMove, 2_000);
    dash.on_event(EventKind::Wheel, 6_000);
    let total = dash.total();
    let kd = dash.count_of(EventKind::KeyDown);
    let rate = dash.close_window(1_000_000);
    let mut off = InputDashboard::new(false);
    off.on_event(EventKind::KeyDown, 99_999);
    set.add(
        "F146 input dashboard",
        total == 4
            && kd == 2
            && dash.windows == 1
            && rate == 4
            && dash.worst_us == 8_000
            && dash.p99_us == 8_000
            && off.count_of(EventKind::KeyDown) == 1
            && off.total() == 1
            && off.p99_us == 0
            && InputDashboard::new(true).close_window(0) == 0,
        "事件率/延迟采样（关闭时零成本）",
    );

    // F147 触控板手势预留
    let mut tp = TrackpadState::new();
    let kind = tp.update(2, 100, 10, 50);
    let kind2 = tp.update(3, 0, 0, 50);
    let tap = tp.tap();
    set.add(
        "F147 trackpad placeholder",
        kind == GestureKind::TwoFingerScroll
            && tp.last.dy == 2
            && tp.last.dx == 0
            && tp.scrolled == 2
            && kind2 == GestureKind::None
            && tap
            && tp.taps == 1
            && tp.kind == GestureKind::TapClick
            && two_finger_scroll(10, 10, 0).dy == 0
            && !trackpad_supported(),
        "双指滚动语义占位/能力显式降级",
    );

    // F148 事件域 fuzz
    let seeds: [u64; 6] = [1, 2, 3, 5, 8, 13];
    let all_ok = fuzz_hid_domain(&seeds);
    let kb_fuzz = fuzz_keyboard(99, 512);
    let ms_fuzz = fuzz_mouse(99, 512);
    let flood_fuzz = fuzz_flood(99, 1024);
    set.add(
        "F148 hid fuzz",
        all_ok
            && kb_fuzz.bytes_fed == 512
            && ms_fuzz.bytes_fed == 512
            && flood_fuzz.survived
            && flood_fuzz.limited > 0
            && flood_fuzz.dropped <= (flood_fuzz.bytes_fed as u64)
            && fuzz_keyboard(7, 128).survived,
        "畸形包/超频轰击下有界收敛",
    );

    // F149 输入域自检
    let closed = input_closed_loop();
    set.add(
        "F149 input selfcheck",
        closed
            && input_closed_loop()
            && EventQueue::new().is_empty()
            && !crate::hidsrv::aurora_discipline().is_empty(),
        "中断→解码→纪律→焦点→延迟 闭环",
    );

    // F150 手感验收流程
    let mut fa = FeelAcceptance::new();
    let mut i = 0u32;
    while i < 30 {
        fa.record(true, i % 20 == 0);
        i += 1;
    }
    let loss = fa.windows_loss_permille();
    let verdict = fa.verdict();
    let mut few = FeelAcceptance::new();
    few.record(true, false);
    let mut bad = FeelAcceptance::new();
    let mut k = 0u32;
    while k < 30 {
        bad.record(false, true);
        k += 1;
    }
    let mut meter = LatencyMeter::new();
    let _ = meter.record(1_000, 2_000, 3_000);
    set.add(
        "F150 feel acceptance",
        fa.trials == 30
            && fa.wins_windows == 2
            && loss == 66
            && verdict == FeelVerdict::NoLoss
            && few.verdict() == FeelVerdict::Inconclusive
            && bad.verdict() == FeelVerdict::Degraded
            && bad.windows_loss_permille() == 1000
            && feel_preconditions_met(&meter)
            && wheel_feel_aligned(&WheelTokens::default_ws(), WheelMode::Line)
            && wheel_feel_aligned(&WheelTokens::default_ws(), WheelMode::Pixel)
            && replay_deterministic()
            && wheel_drains_clean(WheelMode::Line)
            && wheel_drains_clean(WheelMode::Pixel),
        "盲测胜出率/结论门限/前置仪表",
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f146_dashboard_counts_and_rate() {
        let mut d = InputDashboard::new(true);
        d.on_event(EventKind::KeyDown, 1_000);
        d.on_event(EventKind::KeyUp, 2_000);
        assert_eq!(d.total(), 2);
        assert_eq!(d.count_of(EventKind::MouseMove), 0);
        assert_eq!(d.close_window(500_000), 4);
    }

    #[test]
    fn f148_fuzz_never_panics() {
        assert!(fuzz_hid_domain(&[1, 2, 3]));
        let r = fuzz_keyboard(42, 1024);
        assert!(r.survived);
        assert_eq!(r.bytes_fed, 1024);
    }

    #[test]
    fn f149_closed_loop_holds() {
        assert!(input_closed_loop());
    }

    #[test]
    fn f150_verdict_thresholds() {
        let mut a = FeelAcceptance::new();
        for _ in 0..20 {
            a.record(true, false);
        }
        assert_eq!(a.verdict(), FeelVerdict::NoLoss);
        let mut b = FeelAcceptance::new();
        for _ in 0..17 {
            b.record(true, false);
        }
        for _ in 0..3 {
            b.record(false, true);
        }
        assert_eq!(b.verdict(), FeelVerdict::Degraded);
    }

    #[test]
    fn f147_scroll_sign_convention() {
        let d = two_finger_scroll(100, -100, 50);
        assert_eq!(d.dy, 2);
        assert_eq!(d.dx, -2);
    }
}
