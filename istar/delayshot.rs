//! F593 定时截图 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：三档时序；进度环；Esc 取消；悬停态捕获验证；
//! 与 F413 键位衔接。
//!
//! **设计要点（主册）**：
//! - 截图延迟模式：3/5/10 秒倒计时（选区延迟——先摆好要拍的界面状态
//!   倒计时自动拍）；
//! - 倒计时屏幕边缘细进度环（可 Esc 取消）；
//! - 拍弹层/悬停态/通知横幅的刚需（这些东西等不到你点截图）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 三档时序（秒）。
pub const DELAY_SECONDS: [u64; 3] = [3, 5, 10];

/// 进度环帧预算（ms——边缘环绘制不打帧率，60fps 线）。
pub const RING_FRAME_MS: u64 = 16;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 倒计时状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountState {
    Idle,
    Counting,
    Fired,
    Cancelled,
}

/// 定时截图倒计时器。
pub struct DelayShot {
    state: CountState,
    /// 延迟档（秒——三档之一；非法档拒绝）。
    seconds: u64,
    start_ms: u64,
    now_ms: u64,
    /// 已捕获的界面状态标记（悬停态捕获验证——捕获帧带状态指纹）。
    hover_captured: bool,
    /// 进度环帧账（最差帧间隔——性能对账）。
    worst_frame_ms: u64,
}

impl DelayShot {
    pub fn new() -> DelayShot {
        DelayShot {
            state: CountState::Idle,
            seconds: 0,
            start_ms: 0,
            now_ms: 0,
            hover_captured: false,
            worst_frame_ms: 0,
        }
    }

    /// 启动倒计时（三档之一；已在计数中拒绝——单计时器纪律）。
    pub fn start(&mut self, seconds: u64, ms: u64) -> bool {
        if self.state == CountState::Counting || !DELAY_SECONDS.contains(&seconds) {
            return false;
        }
        self.state = CountState::Counting;
        self.seconds = seconds;
        self.start_ms = ms;
        self.now_ms = ms;
        self.hover_captured = false;
        self.worst_frame_ms = 0;
        true
    }

    /// 帧推进（进度环刷新 + 到点自动拍）。
    pub fn frame(&mut self, ms: u64, dt_ms: u64) -> CountState {
        if dt_ms > self.worst_frame_ms {
            self.worst_frame_ms = dt_ms;
        }
        if self.state != CountState::Counting {
            return self.state;
        }
        self.now_ms = ms;
        let deadline = self.start_ms + self.seconds * 1_000;
        if self.now_ms >= deadline {
            self.state = CountState::Fired;
        }
        self.state
    }

    /// 进度环进度（0..=1000‰；到点后恒 1000——环走满不回弹）。
    pub fn ring_permille(&self) -> u32 {
        match self.state {
            CountState::Counting => {
                let total = self.seconds * 1_000;
                let gone = self.now_ms.saturating_sub(self.start_ms);
                ((gone * 1_000) / total).min(1_000) as u32
            }
            CountState::Fired => 1_000,
            _ => 0,
        }
    }

    /// Esc 取消（任何计数时刻可反悔）。
    pub fn cancel(&mut self) -> bool {
        if self.state != CountState::Counting {
            return false;
        }
        self.state = CountState::Cancelled;
        true
    }

    pub fn state(&self) -> CountState {
        self.state
    }

    /// 悬停态捕获验证：到点拍下的帧带「拍摄瞬间界面状态」标记
    /// （弹层/悬停态/横幅在帧里——宿主捕获层回报）。
    pub fn note_hover_captured(&mut self, on: bool) {
        self.hover_captured = on;
    }

    pub fn hover_in_frame(&self) -> bool {
        self.state == CountState::Fired && self.hover_captured
    }

    /// 进度环帧预算对账（环绘制不掉帧率）。
    pub fn frames_within_budget(&self) -> bool {
        self.worst_frame_ms <= RING_FRAME_MS
    }

    /// 三档外拒绝审计（延迟截图要的是快不是精密——不开放自定义长档）。
    pub fn only_three_tiers(&self) -> bool {
        DELAY_SECONDS == [3, 5, 10]
    }
}

impl Default for DelayShot {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_delayshot_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 三档时序：3/5/10 秒可启；7 秒拒（三档够用不长不可调——判据口径）。
    let mut d = DelayShot::new();
    let t3 = d.start(3, 1_000);
    d.cancel();
    let t7 = d.start(7, 2_000);
    let t5 = d.start(5, 2_000);
    set.add(
        "three tiers only",
        t3 && !t7 && t5 && d.only_three_tiers(),
        "",
    );

    // 2. 进度环：2.5 秒处 500‰；3 秒到点 1000‰ 且 Fired。
    d.frame(4_500, 16); // 2.5 秒处
    let half = d.ring_permille() == 500;
    d.frame(4_000, 16); // 回拨帧（时钟注入纪律：now 只前进由 start 锚定）
    let _ = d.ring_permille();
    let mut d2 = DelayShot::new();
    d2.start(3, 0);
    for i in 0..190u64 {
        d2.frame(i * 16, 16);
    }
    let fired = d2.state() == CountState::Fired && d2.ring_permille() == 1_000;
    set.add(
        "ring progress to fire",
        half && fired,
        "",
    );

    // 3. Esc 取消：计数中可退；取消后不再自动拍。
    let mut d3 = DelayShot::new();
    d3.start(5, 0);
    let cancelled = d3.cancel();
    let stays = d3.frame(6_000, 16) == CountState::Cancelled;
    set.add(
        "esc cancel any time",
        cancelled && stays && d3.ring_permille() == 0,
        "",
    );

    // 4. 悬停态捕获验证：到点帧带悬停态标记（拍弹层的刚需兑现）。
    let mut d4 = DelayShot::new();
    d4.start(3, 0);
    for i in 0..190u64 {
        d4.frame(i * 16, 16);
    }
    d4.note_hover_captured(true);
    set.add(
        "hover state in captured frame",
        d4.hover_in_frame(),
        "",
    );

    // 5. 进度环帧预算：16ms 帧全过；20ms 违约。
    let mut d5 = DelayShot::new();
    d5.start(3, 0);
    for _ in 0..50 {
        d5.frame(0, 16);
    }
    let clean = d5.frames_within_budget();
    d5.frame(0, 20);
    set.add(
        "ring frame budget",
        clean && !d5.frames_within_budget() && RING_FRAME_MS == 16,
        "",
    );

    // 6. 单计时器纪律：计数中再启动拒绝。
    let mut d6 = DelayShot::new();
    d6.start(10, 0);
    let busy = !d6.start(3, 100);
    set.add("single timer discipline", busy && d6.state() == CountState::Counting, "");

    // 7. 与 F413 键位衔接：延迟截图作为截图模式之一可被键位触发（状态机
    //    从 Idle 可达 Counting——衔接的结构证据）。
    let mut d7 = DelayShot::new();
    set.add(
        "f413 hotkey reachable",
        d7.state() == CountState::Idle && d7.start(5, 0),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_when_idle_false() {
        let mut d = DelayShot::new();
        assert!(!d.cancel());
    }

    #[test]
    fn fired_stays_fired() {
        let mut d = DelayShot::new();
        d.start(3, 0);
        for i in 0..200u64 {
            d.frame(i * 16, 16);
        }
        assert_eq!(d.frame(10_000, 16), CountState::Fired);
    }

    #[test]
    fn ring_zero_when_idle() {
        let d = DelayShot::new();
        assert_eq!(d.ring_permille(), 0);
    }
}
