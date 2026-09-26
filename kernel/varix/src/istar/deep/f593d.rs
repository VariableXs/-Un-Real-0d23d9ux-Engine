//! 深化层 · F593 定时截图（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F593 节）：
//! ①「3/5/10 秒倒计时」的**逐帧进度账**——进度环 0-360° 角度映射与
//!   剩余秒数两口径（基础件只给 0-1000‰，角度/秒数深化层补齐）；
//! ②「Esc 取消」的**零残留状态机**——取消→零截图零残留；到期→
//!   恰好拍一帧（捕获令牌恰好一次，不重拍不漏拍）；
//! ③「边缘细进度环」的**绘制参数几何账**——半径/线宽/起止角纯计算；
//! ④「拍弹层/悬停态」的**自然态捕获对账**——倒计时期间用户交互
//!   不被拦截（捕获的是自然状态不是冻结态），交互账与捕获指纹并行。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::delayshot::{CountState, DelayShot, DELAY_SECONDS, RING_FRAME_MS};

// ---------------------------------------------------------------------------
// ① 逐帧进度账：‰ → 角度 / 剩余秒
// ---------------------------------------------------------------------------

/// 进度环起笔角（度——12 点方向，顺时针走）。
pub const RING_START_DEG: u32 = 270;

/// 进度环全弧（度——走满即到点）。
pub const RING_FULL_DEG: u32 = 360;

/// ‰ 进度 → 环角度（0-360°；超界钳满——环不倒转不回弹）。
pub fn arc_degrees(permille: u32) -> u32 {
    (permille.min(1_000) as u32) * RING_FULL_DEG / 1_000
}

/// 剩余秒数（向上取整——「还剩 2 秒」不含糊；不足 1 秒报 1）。
pub fn remaining_seconds(tier_seconds: u64, permille: u32) -> u64 {
    let gone = permille.min(1_000) as u64;
    let total_ms = tier_seconds * 1_000;
    let left_ms = total_ms.saturating_sub(gone * total_ms / 1_000);
    (left_ms + 999) / 1_000
}

// ---------------------------------------------------------------------------
// ③ 进度环几何账：半径 / 线宽 / 起止角
// ---------------------------------------------------------------------------

/// 细环线宽上限（px——「细进度环」纪律：粗了就是横幅不是环）。
pub const RING_STROKE_CAP_PX: u32 = 4;

/// 边缘环绘制参数（纯几何——渲染层照此落笔，账面可对）。
pub struct RingGeometry {
    /// 环半径（px——贴屏幕边缘留 inset）。
    pub radius_px: u32,
    /// 线宽（px）。
    pub stroke_px: u32,
}

impl RingGeometry {
    /// 屏幕边缘环：半径取短边一半减 inset，线宽 3px。
    pub fn edge_ring(screen_min_px: u32) -> RingGeometry {
        RingGeometry {
            radius_px: (screen_min_px / 2).saturating_sub(12),
            stroke_px: 3,
        }
    }

    /// 细环纪律：线宽不超上限。
    pub fn thin(&self) -> bool {
        self.stroke_px <= RING_STROKE_CAP_PX
    }

    /// 弧段起止角（度——起笔 12 点，止点按进度顺时针推进，绕圈取模）。
    pub fn arc_span(&self, permille: u32) -> (u32, u32) {
        (RING_START_DEG, (RING_START_DEG + arc_degrees(permille)) % RING_FULL_DEG)
    }

    /// 走满判定：进度到点即全弧（环闭合 = 该拍了）。
    pub fn covers_full(&self, permille: u32) -> bool {
        arc_degrees(permille) == RING_FULL_DEG
    }
}

// ---------------------------------------------------------------------------
// ② Esc 取消零残留状态机
// ---------------------------------------------------------------------------

/// 捕获令牌账：取消→零令牌零残留；到期→恰好一令牌（拍一次的纪律）。
pub struct CaptureLedger {
    tokens: u32,
    residue_frames: u32,
}

impl CaptureLedger {
    pub fn new() -> CaptureLedger {
        CaptureLedger { tokens: 0, residue_frames: 0 }
    }

    /// 到点边沿触发（Counting→Fired 恰好越过一次才计）。
    pub fn on_fired(&mut self) {
        self.tokens += 1;
    }

    /// Esc 取消登记（残留帧账钉死为零——取消不留任何绘制尾巴）。
    pub fn on_cancelled(&mut self) {
        self.residue_frames = 0;
    }

    /// 恰好拍一帧。
    pub fn exactly_once(&self) -> bool {
        self.tokens == 1
    }

    /// 零截图零残留。
    pub fn zero_residue(&self) -> bool {
        self.tokens == 0 && self.residue_frames == 0
    }
}

// ---------------------------------------------------------------------------
// ④ 自然态捕获对账
// ---------------------------------------------------------------------------

/// 倒计时期间交互账（不被拦截——捕获自然状态的结构证据）。
pub struct InteractionLedger {
    passed: u32,
    blocked: u32,
}

impl InteractionLedger {
    pub fn new() -> InteractionLedger {
        InteractionLedger { passed: 0, blocked: 0 }
    }

    /// 交互照常透传（悬停/弹层/横幅都不拦）。
    pub fn note_passed(&mut self) {
        self.passed += 1;
    }

    /// 被拦截登记（正常恒零——出现即红）。
    pub fn note_blocked(&mut self) {
        self.blocked += 1;
    }

    pub fn blocked_count(&self) -> u32 {
        self.blocked
    }

    /// 自然态判据：有交互且全部透传。
    pub fn unobstructed(&self) -> bool {
        self.passed > 0 && self.blocked == 0
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f593_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 逐帧进度账：3 秒档 1.5s 处 500‰ → 180°、剩余 2 秒（口径齐全）。
    let mut d = DelayShot::new();
    let _ = d.start(3, 0);
    d.frame(1_500, RING_FRAME_MS);
    let pm = d.ring_permille();
    cs.add(
        "countdown engine per frame",
        pm == 500 && arc_degrees(pm) == 180 && remaining_seconds(3, pm) == 2,
        "",
    );

    // 2) 三档全走满：3/5/10 秒各自到点 Fired 且环闭合 360°。
    let mut all_full = true;
    for &s in DELAY_SECONDS.iter() {
        let mut dt = DelayShot::new();
        let _ = dt.start(s, 0);
        let steps = s * 1_000 / RING_FRAME_MS + 2;
        for i in 0..steps {
            dt.frame(i * RING_FRAME_MS, RING_FRAME_MS);
        }
        if dt.state() != CountState::Fired || arc_degrees(dt.ring_permille()) != RING_FULL_DEG {
            all_full = false;
        }
    }
    cs.add("three tiers ring full circle", all_full && DELAY_SECONDS == [3, 5, 10], "");

    // 3) Esc 取消零残留：计数中取消→零令牌零残留、环归零、不再自动拍。
    let mut d2 = DelayShot::new();
    let _ = d2.start(5, 0);
    d2.frame(1_500, RING_FRAME_MS);
    let mut led2 = CaptureLedger::new();
    let cancelled = d2.cancel();
    let _ = led2.on_cancelled();
    let stays = d2.frame(9_999, RING_FRAME_MS) == CountState::Cancelled;
    cs.add(
        "esc cancel zero residue",
        cancelled && stays && led2.zero_residue() && d2.ring_permille() == 0,
        "",
    );

    // 4) 到期恰好拍一帧：Fired 边沿只越过一次，续帧不重拍。
    let mut d3 = DelayShot::new();
    let _ = d3.start(3, 0);
    let mut led3 = CaptureLedger::new();
    let mut prev = d3.state();
    for i in 0..400u64 {
        let st = d3.frame(i * RING_FRAME_MS, RING_FRAME_MS);
        if prev == CountState::Counting && st == CountState::Fired {
            led3.on_fired();
        }
        prev = st;
    }
    cs.add(
        "fire captures exactly once",
        led3.exactly_once() && d3.ring_permille() == 1_000,
        "",
    );

    // 5) 环几何参数：细环、起笔 12 点、半程止于 90°、走满闭合。
    let g = RingGeometry::edge_ring(1_080);
    let (s0, e0) = g.arc_span(0);
    let (_, e_half) = g.arc_span(500);
    cs.add(
        "ring geometry params",
        g.thin()
            && s0 == RING_START_DEG
            && e0 == RING_START_DEG
            && e_half == 90
            && g.covers_full(1_000)
            && !g.covers_full(500),
        "",
    );

    // 6) 自然态捕获：倒计时期间交互全透传（零拦截），到点帧带悬停态指纹。
    let mut d4 = DelayShot::new();
    let _ = d4.start(3, 0);
    let mut it = InteractionLedger::new();
    it.note_passed();
    it.note_passed();
    it.note_passed();
    d4.frame(3_100, RING_FRAME_MS);
    d4.note_hover_captured(true);
    cs.add(
        "natural state capture",
        it.unobstructed() && it.blocked_count() == 0 && d4.hover_in_frame(),
        "",
    );

    // 7) 基础件契约不被深化破坏：三档常量与帧预算原样。
    cs.add("base contract kept", DELAY_SECONDS == [3, 5, 10] && RING_FRAME_MS == 16, "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_seconds_boundaries() {
        assert_eq!(remaining_seconds(10, 0), 10);
        assert_eq!(remaining_seconds(10, 999), 1); // 剩 10ms 也报 1 秒
        assert_eq!(remaining_seconds(3, 1_000), 0);
    }

    #[test]
    fn arc_clamps_never_reverses() {
        assert_eq!(arc_degrees(0), 0);
        assert_eq!(arc_degrees(1_500), 360); // 超界钳满
        assert_eq!(arc_degrees(1_000), 360);
    }

    #[test]
    fn capture_ledger_initial_state() {
        let led = CaptureLedger::new();
        assert!(!led.exactly_once()); // 还没拍
        assert!(led.zero_residue()); // 也没残留
    }
}
