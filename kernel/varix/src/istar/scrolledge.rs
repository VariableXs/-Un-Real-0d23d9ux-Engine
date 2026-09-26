//! F580 滚动条端点双击 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：端点 24px 判定区；双击跳转；中段无动作；飞掠动画
//! 时长；万项性能。
//!
//! **设计要点（主册）**：
//! - 滚动条双击顶部 = 跳列表首、双击底部 = 跳列表尾（长列表快捷直达）；
//! - 双击中段无动作（防误触——端点语义只在两端 24px 区域生效）；
//! - 跳转带 F204 平滑滚动（长列表飞掠可视反馈）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 端点判定区（px——两端各 24px）。
pub const ENDPOINT_ZONE_PX: u32 = 24;

/// 飞掠帧预算（ms/帧——F204 平滑滚动 80fps 线）。
pub const FLY_FRAME_BUDGET_MS: u64 = 12;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 双击命中结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DblHit {
    ToTop,
    ToBottom,
    /// 中段——无动作（防误触）。
    Middle,
}

/// 飞掠动画账（从当前位置到目标位置的平滑滚动）。
pub struct FlyOver {
    pub active: bool,
    pub from_px: i64,
    pub to_px: i64,
    pub elapsed_ms: u64,
    /// 最差帧间隔账（万项性能对账）。
    worst_frame_ms: u64,
}

impl FlyOver {
    fn start(from: i64, to: i64) -> FlyOver {
        FlyOver {
            active: true,
            from_px: from,
            to_px: to,
            elapsed_ms: 0,
            worst_frame_ms: 0,
        }
    }

    /// 推进一帧（帧间隔入账——超预算计违约）。
    pub fn frame(&mut self, dt_ms: u64) {
        if !self.active {
            return;
        }
        self.elapsed_ms += dt_ms;
        if dt_ms > self.worst_frame_ms {
            self.worst_frame_ms = dt_ms;
        }
        if self.elapsed_ms >= self.duration_ms() {
            self.active = false;
            self.elapsed_ms = self.duration_ms();
        }
    }

    /// 飞掠时长（随距离缓增——封顶 800ms 防超长列表久等）。
    pub fn duration_ms(&self) -> u64 {
        let dist = (self.to_px - self.from_px).unsigned_abs();
        (dist / 4).min(800).max(160)
    }

    /// 当前进度位置（0..=1000‰）。
    pub fn progress_permille(&self) -> u32 {
        if !self.active && self.elapsed_ms >= self.duration_ms() {
            return 1_000;
        }
        ((self.elapsed_ms * 1_000) / self.duration_ms()) as u32
    }

    pub fn within_frame_budget(&self) -> bool {
        self.worst_frame_ms <= FLY_FRAME_BUDGET_MS
    }
}

/// 滚动条端点双击判定器。
pub struct EdgeDblClick {
    /// 滚动条长度（px）。
    track_px: u32,
    fly: Option<FlyOver>,
    /// 万项账（列表项数——性能口径）。
    items: usize,
}

impl EdgeDblClick {
    pub fn new(track_px: u32, items: usize) -> EdgeDblClick {
        EdgeDblClick {
            track_px,
            fly: None,
            items,
        }
    }

    /// 双击判定：位置在两端 ENDPOINT_ZONE_PX 内 → 跳转；中段 → 无动作。
    pub fn dblclick_at(&mut self, y_px: u32, current_px: i64) -> DblHit {
        let hit = if y_px <= ENDPOINT_ZONE_PX {
            DblHit::ToTop
        } else if y_px >= self.track_px.saturating_sub(ENDPOINT_ZONE_PX) {
            DblHit::ToBottom
        } else {
            DblHit::Middle
        };
        match hit {
            DblHit::ToTop => self.fly = Some(FlyOver::start(current_px, 0)),
            DblHit::ToBottom => {
                let bottom = (self.items as i64 - 1).max(0) * 24; // 行高 24px 口径
                self.fly = Some(FlyOver::start(current_px, bottom));
            }
            DblHit::Middle => {}
        }
        hit
    }

    pub fn flying(&self) -> bool {
        self.fly.as_ref().map(|f| f.active).unwrap_or(false)
    }

    pub fn fly_target(&self) -> Option<i64> {
        self.fly.as_ref().map(|f| f.to_px)
    }

    /// 推进飞掠（返回是否仍在飞）。
    pub fn step(&mut self, dt_ms: u64) -> bool {
        match &mut self.fly {
            Some(f) => {
                f.frame(dt_ms);
                f.active
            }
            None => false,
        }
    }

    /// 帧预算对账（万项性能——飞掠不掉帧）。
    pub fn frames_within_budget(&self) -> bool {
        self.fly.as_ref().map(|f| f.within_frame_budget()).unwrap_or(true)
    }

    /// 中段双击不产生飞掠（防误触判据）。
    pub fn no_fly(&self) -> bool {
        self.fly.is_none()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_scrolledge_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 端点 24px 判定区：顶部 0/24 命中顶、底部 976/1000 命中底（1000px 轨）。
    let mut e = EdgeDblClick::new(1_000, 10_000);
    set.add(
        "endpoint zones 24px",
        e.dblclick_at(0, 500) == DblHit::ToTop
            && e.dblclick_at(24, 500) == DblHit::ToTop
            && e.dblclick_at(1_000, 500) == DblHit::ToBottom
            && e.dblclick_at(976, 500) == DblHit::ToBottom,
        "",
    );

    // 2. 中段无动作：双击 500px 处不产生飞掠（防误触）。
    let mut e2 = EdgeDblClick::new(1_000, 10_000);
    let mid = e2.dblclick_at(500, 300);
    set.add(
        "middle double click no action",
        mid == DblHit::Middle && e2.no_fly() && !e2.flying(),
        "",
    );

    // 3. 双击跳转：顶部飞到 0、底部飞到列表尾（万项 ×24px 行高）。
    let mut e3 = EdgeDblClick::new(1_000, 10_000);
    e3.dblclick_at(0, 100_000);
    let top_target = e3.fly_target() == Some(0);
    e3.dblclick_at(999, 0);
    let bottom_target = e3.fly_target() == Some(239_976);
    set.add("double click jumps to ends", top_target && bottom_target, "");

    // 4. 飞掠动画：有过程（0→1000‰ 单调）、到站停飞。
    let mut e4 = EdgeDblClick::new(1_000, 10_000);
    e4.dblclick_at(1_000, 0);
    let flying_now = e4.flying();
    e4.step(12);
    let p1 = match e4.progress() {
        p => p,
    };
    for _ in 0..200 {
        e4.step(12);
    }
    set.add(
        "fly over animated and stops",
        flying_now && p1 > 0 && !e4.flying(),
        "",
    );

    // 5. 帧预算对账：12ms 帧全过；插一帧 20ms 记违约。
    let mut e5 = EdgeDblClick::new(1_000, 10_000);
    e5.dblclick_at(1_000, 50_000);
    for _ in 0..50 {
        e5.step(12);
    }
    let clean = e5.frames_within_budget();
    e5.dblclick_at(1_000, 50_000);
    e5.step(20);
    set.add(
        "frame budget violations tracked",
        clean && !e5.frames_within_budget(),
        "",
    );

    // 6. 万项性能口径：10000 项底端目标 = (10000-1)×24（行高单一源）。
    let mut e6 = EdgeDblClick::new(1_000, 10_000);
    e6.dblclick_at(1_000, 0);
    set.add(
        "ten thousand items target",
        e6.fly_target() == Some(9_999 * 24),
        "",
    );

    set
}

impl EdgeDblClick {
    fn progress(&self) -> u32 {
        self.fly.as_ref().map(|f| f.progress_permille()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_scales_with_distance_capped() {
        let near = FlyOver::start(0, 100);
        let far = FlyOver::start(0, 1_000_000);
        assert!(near.duration_ms() >= 160);
        assert_eq!(far.duration_ms(), 800);
    }

    #[test]
    fn zone_boundary_exclusive_bottom() {
        // 顶区 [0, 24]；底区 [track-24, track]。975px 恰在区外（1000-24=976 起）。
        let mut e = EdgeDblClick::new(1_000, 100);
        assert_eq!(e.dblclick_at(975, 0), DblHit::Middle);
        assert_eq!(e.dblclick_at(976, 0), DblHit::ToBottom);
    }

    #[test]
    fn step_without_fly_false() {
        let mut e = EdgeDblClick::new(100, 10);
        assert!(!e.step(12));
    }
}
