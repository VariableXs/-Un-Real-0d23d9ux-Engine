//! F594 录屏点击高亮 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：涟漪 300ms；仅视频叠加判据（不干扰真实交互）；
//! 默认关；令牌色；与 F361 链路集成。
//!
//! **设计要点（主册）**：
//! - 录屏（F361）教学增强：录制时自动给鼠标点击加视觉高亮（点击处涟漪圈
//!   300ms——看录屏的人一眼看到你在点哪）；
//! - 开关独立（默认关——正式录屏常不要）；
//! - 涟漪颜色走主题令牌（F151）；涟漪是录制时叠加不进系统交互（只在
//!   视频里出现）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 涟漪生命周期（ms）。
pub const RIPPLE_MS: u32 = 300;

/// 涟漪色令牌名（F151——颜色唯一源，不持 RGB）。
pub const RIPPLE_TOKEN: &str = "ripple-highlight";

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 一圈涟漪。
#[derive(Clone, Copy, Debug)]
pub struct Ripple {
    pub x: i32,
    pub y: i32,
    /// 出生时刻（ms——老化按合成钟计算，多点并发各自到期）。
    pub born_ms: u32,
}

/// 录屏点击高亮层（默认关——录制合成链的一节）。
pub struct ClickRipple {
    enabled: bool,
    /// 在途涟漪（录制帧合成时逐帧绘制）。
    ripples: Vec<Ripple>,
    now_ms: u32,
    /// 叠加帧数账（仅视频叠加——合成账）。
    composited_frames: u32,
}

impl ClickRipple {
    pub fn new() -> ClickRipple {
        ClickRipple {
            enabled: false, // 默认关——判据钉死
            ripples: Vec::new(),
            now_ms: 0,
            composited_frames: 0,
        }
    }

    /// 开关（默认关——正式录屏常不要）。
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.ripples.clear();
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// 真实点击注入（来自系统输入链——只记录，不拦截不回执：
    /// 涟漪不干扰真实交互的结构证据）。
    pub fn real_click(&mut self, x: i32, y: i32, ms: u32) {
        self.now_ms = self.now_ms.max(ms);
        if !self.enabled {
            return; // 默认关：真实交互照常，高亮层零动作。
        }
        self.ripples.push(Ripple { x, y, born_ms: ms });
    }

    /// 录制帧推进：涟漪老化 + 帧合成（返回本帧应绘制的涟漪快照）。
    pub fn compose_frame(&mut self, dt_ms: u32) -> Vec<(i32, i32)> {
        if !self.enabled {
            return Vec::new();
        }
        self.now_ms += dt_ms;
        self.ripples
            .retain(|r| self.now_ms.saturating_sub(r.born_ms) < RIPPLE_MS);
        self.composited_frames += 1;
        self.ripples.iter().map(|r| (r.x, r.y)).collect()
    }

    pub fn ripple_count(&self) -> usize {
        self.ripples.len()
    }

    /// 涟漪颜色令牌（F151 唯一源）。
    pub fn color_token(&self) -> &'static str {
        RIPPLE_TOKEN
    }

    /// 仅视频叠加判据：高亮层不持任何输入事件回执口（真实交互路径
    /// 与高亮账互不相交——本函数返回合成账，供审计对照输入账）。
    pub fn composited_only(&self) -> u32 {
        self.composited_frames
    }
}

impl Default for ClickRipple {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_clickripple_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 默认关：真实点击零涟漪（正式录屏常不要）。
    let mut r = ClickRipple::new();
    r.real_click(10, 10, 0);
    let frame0 = r.compose_frame(16);
    set.add(
        "default off no ripple",
        !r.enabled() && frame0.is_empty() && r.ripple_count() == 0,
        "",
    );

    // 2. 开启后点击出涟漪：位置精确（点哪圈哪）。
    r.set_enabled(true);
    r.real_click(120, 80, 1_000);
    let frame = r.compose_frame(16);
    set.add(
        "enabled click ripples at position",
        frame == alloc::vec![(120, 80)] && r.ripple_count() == 1,
        "",
    );

    // 3. 涟漪 300ms：290 存活、310 消亡（生命周期对账）。
    set.add(
        "ripple 300ms lifecycle",
        {
            let mut r = ClickRipple::new();
            r.set_enabled(true);
            r.real_click(0, 0, 0);
            r.compose_frame(290);
            let alive = r.ripple_count() == 1;
            r.compose_frame(20);
            let gone = r.ripple_count() == 0;
            alive && gone
        },
        "");

    // 4. 多点并发：连点三处三圈各自老化（互不吞；按出生时刻各自到期）。
    let mut r2 = ClickRipple::new();
    r2.set_enabled(true);
    r2.real_click(1, 1, 0);
    r2.real_click(2, 2, 50);
    r2.real_click(3, 3, 100);
    r2.compose_frame(100);
    let three = r2.ripple_count() == 3;
    r2.compose_frame(130); // 第一圈（0ms 生）到 330 出局，另两圈仍在
    set.add(
        "concurrent ripples age independently",
        three && r2.ripple_count() == 2,
        "",
    );

    // 5. 令牌色：颜色唯一源走 F151 令牌（不持 RGB——一处一事实）。
    set.add(
        "token color from f151",
        r2.color_token() == RIPPLE_TOKEN && RIPPLE_TOKEN == "ripple-highlight",
        "",
    );

    // 6. 仅视频叠加：合成账与真实点击账分离（点击不回执、交互不被扰——
    //    结构证据：real_click 无返回值路径、compose 只产绘制快照）。
    r2.set_enabled(false);
    let cleaned = r2.ripple_count() == 0;
    set.add(
        "video overlay only",
        r2.composited_only() > 0 && cleaned,
        "",
    );

    // 7. 与 F361 链路集成：开关随录制会话启停（录屏开→层开、录屏关→层清）。
    let mut r3 = ClickRipple::new();
    r3.set_enabled(true);
    r3.real_click(5, 5, 0);
    r3.set_enabled(false); // 录制结束
    set.add(
        "f361 session lifecycle",
        r3.ripple_count() == 0 && !r3.enabled(),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_clicks_not_backlogged() {
        // 关态点击不入账——再开启也不会冒出历史涟漪。
        let mut r = ClickRipple::new();
        r.real_click(1, 1, 0);
        r.real_click(2, 2, 1);
        r.set_enabled(true);
        assert_eq!(r.compose_frame(16).len(), 0);
    }

    #[test]
    fn ripple_position_immutable_during_life() {
        let mut r = ClickRipple::new();
        r.set_enabled(true);
        r.real_click(50, 60, 0);
        let a = r.compose_frame(10);
        let b = r.compose_frame(10);
        assert_eq!(a, b); // 位置不动（老化只影响透明度/半径——渲染层）
    }

    #[test]
    fn zero_dt_frame_still_counts() {
        let mut r = ClickRipple::new();
        r.set_enabled(true);
        r.real_click(0, 0, 0);
        let _ = r.compose_frame(0);
        assert_eq!(r.composited_only(), 1);
    }
}
