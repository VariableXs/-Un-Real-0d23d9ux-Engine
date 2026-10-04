//! 深化层 · F580 滚动条端点双击（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F580 节）：
//! ①「中段三分判定」的几何账——两端 24px 端点区之外的中段按三等分
//!   划分（上/中/下），任一分区双击一律无动作（防误触覆盖全段）；
//! ②「飞掠动画可打断」——用户中途滚动即打断飞掠：帧序列停在打断点
//!   （位置不跳变、不续飞），打断次数计账；
//! ③「万项性能账」——跳转目标行 → 虚拟化窗口锚定：一次重排钉出可见
//!   行窗（不逐行扫），锚定必覆盖目标行；
//! ④「双击节流」——节流窗内的再次双击不叠加触发（三连击第三击不重启动画）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::scrolledge::{DblHit, EdgeDblClick, ENDPOINT_ZONE_PX, FLY_FRAME_BUDGET_MS};

// 行高口径：与基础模块 scrolledge 底端目标 ((items-1)×24px) 单一源对齐。
pub const ROW_PX: i64 = 24;

/// 双击节流窗（ms——窗内再次双击一律吞掉）。
pub const DBL_THROTTLE_MS: u64 = 500;

// ---------------------------------------------------------------------------
// 端点判定几何 · 中段三分
// ---------------------------------------------------------------------------

/// 轨道分区（端点区 + 中段三分的完整几何账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackZone {
    TopEnd,
    MidUpper,
    MidCenter,
    MidLower,
    BottomEnd,
}

/// 几何判定（只读坐标无状态）：端点 24px 区命中 + 中段三分落位。
pub fn classify_zone(track_px: u32, y_px: u32) -> TrackZone {
    if y_px <= ENDPOINT_ZONE_PX {
        return TrackZone::TopEnd;
    }
    if y_px >= track_px.saturating_sub(ENDPOINT_ZONE_PX) {
        return TrackZone::BottomEnd;
    }
    let mid = (y_px - ENDPOINT_ZONE_PX) as u64;
    let span = track_px.saturating_sub(2 * ENDPOINT_ZONE_PX).max(1) as u64;
    match (mid * 3) / span {
        0 => TrackZone::MidUpper,
        1 => TrackZone::MidCenter,
        _ => TrackZone::MidLower,
    }
}

// ---------------------------------------------------------------------------
// 可打断飞掠引擎（时长随距离 + ease-out 缓动 + 打断不续飞）
// ---------------------------------------------------------------------------

/// 可打断飞掠引擎。基础件的 FlyOver 只有推进无打断路径——本引擎补上
/// 「用户滚动即停」：时长公式与基础件同式（dist/4，下限 160、封顶 800）。
pub struct FlyEngine {
    from_px: i64,
    to_px: i64,
    duration_ms: u64,
    elapsed_ms: u64,
    interrupted: bool,
    interrupts: u32,
    worst_frame_ms: u64,
}

impl FlyEngine {
    pub fn launch(from_px: i64, to_px: i64) -> FlyEngine {
        let dist = (to_px - from_px).unsigned_abs();
        FlyEngine {
            from_px,
            to_px,
            duration_ms: (dist / 4).min(800).max(160),
            elapsed_ms: 0,
            interrupted: false,
            interrupts: 0,
            worst_frame_ms: 0,
        }
    }

    /// 推进一帧（返回是否仍在飞）。超帧预算入账不静默。
    pub fn frame(&mut self, dt_ms: u64) -> bool {
        if self.interrupted || self.elapsed_ms >= self.duration_ms {
            return false;
        }
        self.elapsed_ms = (self.elapsed_ms + dt_ms).min(self.duration_ms);
        if dt_ms > self.worst_frame_ms {
            self.worst_frame_ms = dt_ms;
        }
        self.elapsed_ms < self.duration_ms
    }

    /// 用户滚动打断：停在当前进度点（不跳变、不续飞），计一次账。
    pub fn interrupt(&mut self) -> bool {
        let was_flying = !self.interrupted && self.elapsed_ms < self.duration_ms;
        if was_flying {
            self.interrupted = true;
            self.interrupts += 1;
        }
        was_flying
    }

    pub fn interrupted(&self) -> bool { self.interrupted }

    pub fn interrupt_count(&self) -> u32 { self.interrupts }

    /// 当前位置（ease-out 三次缓动 p = 1-(1-t)³，纯整数 permille 运算）。
    pub fn position_px(&self) -> i64 {
        let t_pm = if self.duration_ms == 0 {
            1_000
        } else {
            (self.elapsed_ms * 1_000) / self.duration_ms
        };
        let inv = (1_000 - t_pm.min(1_000)) as u64;
        let p_pm = 1_000u64.saturating_sub(inv * inv * inv / 1_000_000);
        self.from_px + ((self.to_px - self.from_px) * p_pm as i64) / 1_000
    }

    pub fn finished(&self) -> bool { !self.interrupted && self.elapsed_ms >= self.duration_ms }

    pub fn within_frame_budget(&self) -> bool { self.worst_frame_ms <= FLY_FRAME_BUDGET_MS }
}

// ---------------------------------------------------------------------------
// 双击节流
// ---------------------------------------------------------------------------

/// 双击节流账：节流窗内的再次双击吞掉（三连击第三击不叠加触发）。
pub struct DblThrottle {
    window_ms: u64,
    last_fire_ms: Option<u64>,
    fired: u32,
    swallowed: u32,
}

impl DblThrottle {
    pub fn new(window_ms: u64) -> DblThrottle { DblThrottle { window_ms, last_fire_ms: None, fired: 0, swallowed: 0 } }

    /// 双击申请：true = 放行、false = 窗内吞掉（双账诚实分计）。
    pub fn attempt(&mut self, ms: u64) -> bool {
        match self.last_fire_ms {
            Some(t) if ms.saturating_sub(t) < self.window_ms => {
                self.swallowed += 1;
                false
            }
            _ => {
                self.last_fire_ms = Some(ms);
                self.fired += 1;
                true
            }
        }
    }

    pub fn fired(&self) -> u32 { self.fired }

    pub fn swallowed(&self) -> u32 { self.swallowed }
}

// ---------------------------------------------------------------------------
// 万项虚拟化窗口锚
// ---------------------------------------------------------------------------

/// 虚拟化窗口锚：跳转目标行一次重排钉出可见行窗（不逐行扫）。
pub struct VirtAnchor {
    pub first_row: usize,
    pub rows: usize,
    relayouts: u32,
}

impl VirtAnchor {
    /// 锚定：目标 px → 首可见行（钳到列表尾窗内），恰好一次重排。
    pub fn anchor(items: usize, target_px: i64, viewport_rows: usize) -> VirtAnchor {
        let rows = viewport_rows.max(1);
        let total = items.max(1) as i64;
        let first = (target_px / ROW_PX).clamp(0, total - 1) as usize;
        let first = first.min(items.saturating_sub(rows));
        VirtAnchor { first_row: first, rows, relayouts: 1 }
    }

    pub fn one_relayout(&self) -> bool { self.relayouts == 1 }

    /// 锚定窗必覆盖目标行（跳转直达红线）。
    pub fn covers(&self, target_row: usize) -> bool { target_row >= self.first_row && target_row < self.first_row + self.rows }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f580_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 三分几何：1000px 轨——端点区外中段三分落位正确。
    cs.add(
        "mid track split in thirds",
        classify_zone(1_000, 25) == TrackZone::MidUpper
            && classify_zone(1_000, 500) == TrackZone::MidCenter
            && classify_zone(1_000, 975) == TrackZone::MidLower,
        "",
    );

    // 2) 防误触覆盖全段：端点区经基础件命中；中段三分任一分区双击一律 Middle 无动作、零飞掠。
    let mut e2 = EdgeDblClick::new(1_000, 10_000);
    let mid_all_no_fly = classify_zone(1_000, 0) == TrackZone::TopEnd
        && classify_zone(1_000, 1_000) == TrackZone::BottomEnd
        && e2.dblclick_at(25, 0) == DblHit::Middle
        && e2.dblclick_at(500, 0) == DblHit::Middle
        && e2.dblclick_at(975, 0) == DblHit::Middle
        && e2.no_fly();
    cs.add("all mid thirds no action", mid_all_no_fly, "");

    // 3) 双击节流：三连击 0/120/240ms——首击放行、后两击吞掉；出窗放行。
    let mut th = DblThrottle::new(DBL_THROTTLE_MS);
    let a = th.attempt(0);
    let b = th.attempt(120);
    let c = th.attempt(240);
    let d = th.attempt(600);
    cs.add(
        "triple click throttled",
        a && !b && !c && d && th.fired() == 2 && th.swallowed() == 2,
        "",
    );

    // 4) 飞掠可打断：飞至半途打断——停在断点、不续飞、恰计一次账。
    let mut fly = FlyEngine::launch(0, 240_000);
    let _ = fly.frame(80);
    let pos_at_cut = fly.position_px();
    let cut = fly.interrupt();
    for _ in 0..50 {
        let _ = fly.frame(12);
    }
    cs.add(
        "fly over interruptible at cut point",
        cut && fly.interrupted()
            && fly.interrupt_count() == 1
            && fly.position_px() == pos_at_cut,
        "",
    );

    // 5) 缓动与到站：位置单调推进、到站即停、12ms 帧全过预算。
    let mut fly2 = FlyEngine::launch(0, 24_000);
    let mut last = 0i64;
    let mut mono = true;
    while fly2.frame(FLY_FRAME_BUDGET_MS) {
        let p = fly2.position_px();
        if p < last {
            mono = false;
        }
        last = p;
    }
    cs.add(
        "easing monotonic and lands",
        mono && fly2.finished()
            && fly2.position_px() == 24_000
            && fly2.within_frame_budget(),
        "",
    );

    // 6) 万项锚定：10000 项、视口 40 行——目标行 9999 被锚定窗覆盖、恰好一次重排。
    let anchor = VirtAnchor::anchor(10_000, 9_999 * ROW_PX, 40);
    cs.add(
        "ten thousand items one relayout",
        anchor.one_relayout() && anchor.covers(9_999) && anchor.first_row == 9_960,
        "",
    );

    // 7) 基础判据不被深化破坏：底部双击目标 = (items-1)×24（行高单一源）。
    let mut e7 = EdgeDblClick::new(1_000, 10_000);
    e7.dblclick_at(1_000, 0);
    cs.add("base bottom target kept", e7.fly_target() == Some(9_999 * ROW_PX), "");

    // 8) 端到端管线：双击判定 → 节流放行 → 飞掠到站 → 虚拟锚定 同链贯通。
    let mut e8 = EdgeDblClick::new(1_000, 10_000);
    let mut th8 = DblThrottle::new(DBL_THROTTLE_MS);
    let hit = e8.dblclick_at(0, 120_000);
    let let_go = th8.attempt(0);
    let target = e8.fly_target().unwrap_or(0);
    let mut fly8 = FlyEngine::launch(120_000, target);
    while fly8.frame(FLY_FRAME_BUDGET_MS) {}
    let anchor8 = VirtAnchor::anchor(10_000, target, 40);
    cs.add(
        "end to end pipeline",
        hit == DblHit::ToTop && let_go && fly8.finished()
            && fly8.position_px() == 0
            && anchor8.covers(0) && anchor8.one_relayout(),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttle_window_boundary_exact() {
        let mut th = DblThrottle::new(500);
        assert!(th.attempt(1_000));
        assert!(!th.attempt(1_499)); // 窗内吞
        assert!(th.attempt(1_500)); // 恰满窗放行
    }

    #[test]
    fn anchor_clamps_when_viewport_exceeds_items() {
        let a = VirtAnchor::anchor(10, 9 * ROW_PX, 40);
        assert_eq!(a.first_row, 0);
        assert!(a.covers(9));
    }

    #[test]
    fn interrupt_at_start_is_honest() {
        let mut fly = FlyEngine::launch(0, 1_000);
        assert!(fly.interrupt()); // 未起飞也可打断（诚实计账）
        assert_eq!(fly.position_px(), 0);
        assert!(!fly.interrupt()); // 已断不再重复计
    }
}
