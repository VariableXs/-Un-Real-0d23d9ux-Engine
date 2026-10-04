//! m600motion — VARIX-M600 AI-12 动效与空间域 (F276~F300)
//!
//! 动效编排器/物理弹簧引擎/空间层级语法/窗口飞行轨迹/焦点位移叙事/
//! 视差深度引擎/粒子工坊/流体模拟装饰/3D 桌面全景/手势空间映射/
//! 转场语法书/微交互词典/动效时间轴编辑/性能自适应动效/动效降级守门/
//! 帧预算法庭/减少动效尊重/动效录制回放/缓动曲线实验室/深度雾与景深/
//! 空间音频联动/动效词典 SDK/动效回归走廊/眩晕安全评估/动效年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F276 — 动效编排器：轨道与冲突检测
// ===========================================================================

/// 一条动效轨道（起止时刻 + 缓动档）。
#[derive(Clone, Copy)]
pub struct MotionTrack {
    pub at_ms: u32,
    pub dur_ms: u32,
    pub easing: u8, // 0=standard 1=decel 2=accel
}

impl MotionTrack {
    pub fn valid(&self) -> bool {
        self.dur_ms > 0 && self.dur_ms <= 2000 && self.easing <= 2
    }
    pub fn overlaps(&self, other: &MotionTrack) -> bool {
        self.at_ms < other.at_ms + other.dur_ms && other.at_ms < self.at_ms + self.dur_ms
    }
}

/// 编排总时长 = 最晚结束时刻。
pub fn orchestration_total_ms(tracks: &[MotionTrack]) -> u32 {
    tracks.iter().map(|t| t.at_ms + t.dur_ms).max().unwrap_or(0)
}

// ===========================================================================
// F277 — 物理弹簧引擎：半隐式欧拉定点积分
// ===========================================================================

#[derive(Clone, Copy)]
pub struct SpringState {
    pub pos: i32, // 定点：单位 = 毫像素
    pub vel: i32,
}

impl SpringState {
    /// 单步：vel += (target-pos)*stiffness/1000 - vel*damping/1000。
    /// 定点截断保护：阻尼项被截断为 0 时把速度直接拉向 0，保证收敛。
    pub fn step(&mut self, target: i32, stiffness_permille: i32, damping_permille: i32) {
        let disp = target - self.pos;
        let mut damp = self.vel as i64 * damping_permille as i64 / 1000;
        if damp == 0 && damping_permille > 0 && self.vel != 0 {
            damp = self.vel as i64;
        }
        let accel = disp as i64 * stiffness_permille as i64 / 1000 - damp;
        self.vel = (self.vel as i64 + accel).clamp(-1_000_000, 1_000_000) as i32;
        self.pos = (self.pos as i64 + self.vel as i64).clamp(-1_000_000_000, 1_000_000_000) as i32;
    }
    pub fn settled(&self, target: i32) -> bool {
        (self.pos - target).abs() < 10 && self.vel.abs() < 10
    }
}

// ===========================================================================
// F278 — 空间层级语法：z 深度映射
// ===========================================================================

pub const LAYER_DESKTOP: u32 = 0;
pub const LAYER_WINDOW: u32 = 100;
pub const LAYER_PANEL: u32 = 200;
pub const LAYER_OVERLAY: u32 = 300;
pub const LAYER_CURSOR: u32 = 400;

/// 层级合法：必须落在五个标准层的之一（步进 100）。
pub fn layer_z_valid(z: u32) -> bool {
    z % 100 == 0 && z <= 400
}

// ===========================================================================
// F279 — 窗口飞行轨迹：permille 插值
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Vec2 {
    pub x: i32,
    pub y: i32,
}

/// 线性插值：t 为 0~1000‰。
pub fn flight_pos(from: Vec2, to: Vec2, t_permille: u16) -> Vec2 {
    let t = t_permille.min(1000) as i32;
    Vec2 {
        x: from.x + (to.x - from.x) * t / 1000,
        y: from.y + (to.y - from.y) * t / 1000,
    }
}

/// 飞行时长下限：速度不得超过 10px/ms（否则瞬移感）。
pub fn flight_duration_ok(dist_px: u32, dur_ms: u32) -> bool {
    dur_ms > 0 && dist_px as u64 <= dur_ms as u64 * 10
}

// ===========================================================================
// F280 — 焦点位移叙事：位移量可解释
// ===========================================================================

/// 焦点跳变距离不得超过视口对角的一半。
pub fn focus_shift_ok(from: Vec2, to: Vec2, viewport_diag: u32) -> bool {
    let dx = (to.x - from.x).abs() as u64;
    let dy = (to.y - from.y).abs() as u64;
    let dist_sq = dx * dx + dy * dy;
    let half_diag = viewport_diag as u64 / 2;
    dist_sq <= half_diag * half_diag
}

// ===========================================================================
// F281 — 视差深度引擎：深度偏移
// ===========================================================================

/// 偏移 = depth_permille × 指针位移 / 1000（depth 越大动得越少）。
pub fn parallax_offset(depth_permille: u16, pointer_dx: i32) -> i32 {
    (pointer_dx as i64 * (1000 - depth_permille.min(1000)) as i64 / 1000) as i32
}

// ===========================================================================
// F282 — 粒子工坊：粒子预算
// ===========================================================================

pub const PARTICLE_CAP: u32 = 2048;

/// 档位：0=满 2048，1=一半，2=关闭。
pub fn particle_budget(tier: u8) -> u32 {
    match tier {
        0 => PARTICLE_CAP,
        1 => PARTICLE_CAP / 2,
        _ => 0,
    }
}

// ===========================================================================
// F283 — 流体模拟装饰：装饰性模拟成本
// ===========================================================================

/// 成本 = cells × iters，装饰上限 4096 单位。
pub fn fluid_cost(cells: u32, iters: u32) -> u32 {
    cells.saturating_mul(iters)
}

pub fn fluid_is_decorative(cells: u32, iters: u32) -> bool {
    fluid_cost(cells, iters) <= 4096
}

// ===========================================================================
// F284 — 3D 桌面全景：视角夹持
// ===========================================================================

pub const MAX_YAW_DEG_MILLI: i32 = 30_000;  // 30°，千分之一度
pub const MAX_PITCH_DEG_MILLI: i32 = 15_000; // 15°

/// 夹持后的 (yaw, pitch)。
pub fn clamp_view(yaw_milli: i32, pitch_milli: i32) -> (i32, i32) {
    (
        yaw_milli.clamp(-MAX_YAW_DEG_MILLI, MAX_YAW_DEG_MILLI),
        pitch_milli.clamp(-MAX_PITCH_DEG_MILLI, MAX_PITCH_DEG_MILLI),
    )
}

// ===========================================================================
// F285 — 手势空间映射：滑动阈值
// ===========================================================================

pub const GESTURE_THRESHOLD_PERMILLE: u32 = 100; // 屏宽 10%

/// 滑动距离达到屏宽 10% 且时长不过长（≤600ms）才算手势。
pub fn gesture_action(swipe_px: u32, screen_w: u32, dur_ms: u32) -> bool {
    swipe_px * 1000 >= screen_w * GESTURE_THRESHOLD_PERMILLE && dur_ms > 0 && dur_ms <= 600
}

// ===========================================================================
// F286 — 转场语法书：转场类型与时长
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Push,
    Modal,
    Crossfade,
    None,
}

impl Transition {
    pub fn dur_ms_cap(&self) -> u32 {
        match self {
            Transition::Push => 400,
            Transition::Modal => 350,
            Transition::Crossfade => 250,
            Transition::None => 0,
        }
    }
    pub fn within_cap(&self, dur_ms: u32) -> bool {
        dur_ms <= self.dur_ms_cap()
    }
}

// ===========================================================================
// F287 — 微交互词典：常用微动效
// ===========================================================================

pub const MICRO_INTERACTIONS: [(&str, u32); 6] =
    [("tap", 80), ("toggle", 120), ("expand", 200), ("dismiss", 160), ("ripple", 300), ("hover", 100)];

pub fn micro_duration(name: &str) -> Option<u32> {
    MICRO_INTERACTIONS.iter().find(|(n, _)| *n == name).map(|(_, d)| *d)
}

// ===========================================================================
// F288 — 动效时间轴编辑：窗口裁剪
// ===========================================================================

/// 保留与 [from_ms, to_ms] 有交集的轨道。
pub fn trim_tracks(tracks: &[MotionTrack], from_ms: u32, to_ms: u32) -> usize {
    tracks.iter().filter(|t| t.at_ms < to_ms && t.at_ms + t.dur_ms > from_ms).count()
}

// ===========================================================================
// F289 — 性能自适应动效：按实测帧率降档
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdaptiveTier {
    Rich,    // ≥55fps
    Balanced,// ≥35fps
    Frugal,  // 其余
}

pub fn adaptive_tier(fps: u32) -> AdaptiveTier {
    if fps >= 55 {
        AdaptiveTier::Rich
    } else if fps >= 35 {
        AdaptiveTier::Balanced
    } else {
        AdaptiveTier::Frugal
    }
}

// ===========================================================================
// F290 — 动效降级守门：负载门槛
// ===========================================================================

/// CPU 负载 permille：<600 全量，<850 简化，否则只留必要动效。
pub fn degrade_gate(load_permille: u16) -> u8 {
    if load_permille < 600 {
        0 // full
    } else if load_permille < 850 {
        1 // reduced
    } else {
        2 // essential only
    }
}

// ===========================================================================
// F291 — 帧预算法庭：逐项成本合计
// ===========================================================================

pub const FRAME_BUDGET_US: u32 = 16_600;

pub fn frame_over_budget(costs_us: &[u32]) -> bool {
    costs_us.iter().sum::<u32>() > FRAME_BUDGET_US
}

// ===========================================================================
// F292 — 减少动效尊重：偏好开关
// ===========================================================================

/// 用户开启减少动效时，只允许"必要"动效（如进度反馈）。
pub fn motion_allowed(reduce_pref: bool, essential: bool) -> bool {
    !reduce_pref || essential
}

// ===========================================================================
// F293 — 动效录制回放：一致性比对
// ===========================================================================

/// 回放结果与录制逐轨道一致（起止与缓动全等）。
pub fn replay_match(recorded: &[MotionTrack], replayed: &[MotionTrack]) -> bool {
    recorded.len() == replayed.len()
        && recorded.iter().zip(replayed.iter()).all(|(a, b)| {
            a.at_ms == b.at_ms && a.dur_ms == b.dur_ms && a.easing == b.easing
        })
}

// ===========================================================================
// F294 — 缓动曲线实验室：定点缓动
// ===========================================================================

/// t ∈ [0,1000]‰ → 输出 [0,1000]‰。
/// 0=linear 1=ease-in(quad) 2=ease-out(quad) 3=ease-in-out(quad)。
pub fn ease(t_permille: u16, kind: u8) -> u16 {
    let t = t_permille.min(1000) as u32;
    let out = match kind {
        0 => t,
        1 => t * t / 1000,
        2 => 1000 - (1000 - t) * (1000 - t) / 1000,
        3 => {
            if t < 500 {
                2 * t * t / 1000
            } else {
                let d = 1000 - t;
                1000 - 2 * d * d / 1000
            }
        }
        _ => t,
    };
    out as u16
}

// ===========================================================================
// F295 — 深度雾与景深：距离雾化
// ===========================================================================

/// 雾浓度 permille = 距离/最大距离 × 700（上限 700，保底可见）。
pub fn fog_alpha(dist: u32, max_dist: u32) -> u16 {
    if max_dist == 0 {
        return 0;
    }
    (dist.min(max_dist) * 700 / max_dist) as u16
}

// ===========================================================================
// F296 — 空间音频联动：声像映射
// ===========================================================================

/// 屏幕 x → 声像 [-1000, 1000]（左负右正）。
pub fn pan_for_x(x: i32, screen_w: u32) -> i32 {
    if screen_w == 0 {
        return 0;
    }
    let half = screen_w as i64 / 2;
    let rel = (x as i64 - half) * 1000 / half.max(1);
    rel.clamp(-1000, 1000) as i32
}

// ===========================================================================
// F297 — 动效词典 SDK：命名令牌
// ===========================================================================

pub const MOTION_TOKENS: [(&str, u32, u8); 5] =
    [("rise", 220, 1), ("settle", 320, 1), ("snap", 90, 2), ("drift", 500, 0), ("blink", 140, 0)];

pub fn motion_token(name: &str) -> Option<(u32, u8)> {
    MOTION_TOKENS.iter().find(|(n, _, _)| *n == name).map(|(_, d, e)| (*d, *e))
}

// ===========================================================================
// F298 — 动效回归走廊：金样容差比对
// ===========================================================================

/// 逐点偏差 ≤ tol‰ 视为未回归。
pub fn corridor_check(actual: &[u16], golden: &[u16], tol_permille: u16) -> bool {
    actual.len() == golden.len()
        && actual.iter().zip(golden.iter()).all(|(a, g)| {
            let d = if *a > *g { a - g } else { g - a };
            d <= tol_permille
        })
}

// ===========================================================================
// F299 — 眩晕安全评估：大面积运动限值
// ===========================================================================

/// 安全条件：旋转 ≤5°、每秒缩放 ≤200‰、无全屏闪烁。
pub fn vertigo_safe(rot_deg_milli: i32, zoom_permille_per_s: u32, fullscreen_flash: bool) -> bool {
    rot_deg_milli.abs() <= 5_000 && zoom_permille_per_s <= 200 && !fullscreen_flash
}

// ===========================================================================
// F300 — 动效年报：年度动效统计
// ===========================================================================

pub const MOTION_REPORT_SECTIONS: [&str; 4] = ["counts", "latency", "degrades", "corridor"];

#[derive(Clone, Copy)]
pub struct MotionYearStats {
    pub tracks_total: u32,
    pub regressions: u32,
    pub degrade_events: u32,
}

impl MotionYearStats {
    pub fn report_ready(&self) -> bool {
        self.tracks_total > 0 && MOTION_REPORT_SECTIONS.len() == 4
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600motion_checks() -> CheckSet {
    let mut set = CheckSet::new("m600motion");

    // F276 编排器
    let tracks = [
        MotionTrack { at_ms: 0, dur_ms: 200, easing: 1 },
        MotionTrack { at_ms: 150, dur_ms: 300, easing: 0 },
        MotionTrack { at_ms: 500, dur_ms: 100, easing: 2 },
    ];
    set.add("F276 orchestrator", tracks.iter().all(|t| t.valid()) && orchestration_total_ms(&tracks) == 600, "valid+total");
    set.add("F276 conflict detect", tracks[0].overlaps(&tracks[1]) && !tracks[1].overlaps(&tracks[2]), "overlap");

    // F277 弹簧
    let mut sp = SpringState { pos: 0, vel: 0 };
    for _ in 0..200 {
        sp.step(1000, 120, 200);
    }
    let final_pos = sp.pos; // 末态读取：先存再断言
    let settled = sp.settled(1000);
    set.add("F277 spring settle", settled && (990..=1010).contains(&final_pos), "converges");
    set.add("F277 spring damping", {
        let mut s = SpringState { pos: 0, vel: 1000 };
        s.step(0, 0, 1000); // 纯阻尼：速度衰减
        s.vel < 1000
    }, "damped");

    // F278 空间层级
    set.add("F278 spatial layers", layer_z_valid(LAYER_PANEL) && layer_z_valid(LAYER_CURSOR) && !layer_z_valid(350), "z levels");

    // F279 飞行轨迹
    let p = flight_pos(Vec2 { x: 0, y: 0 }, Vec2 { x: 1000, y: -500 }, 500);
    set.add("F279 flight path", p.x == 500 && p.y == -250, "lerp mid");
    set.add("F279 flight clamped", flight_pos(Vec2 { x: 0, y: 0 }, Vec2 { x: 100, y: 0 }, 1500).x == 100, "t clamp");
    set.add("F279 flight duration", flight_duration_ok(2000, 240) && !flight_duration_ok(2000, 100), "speed cap 10px/ms");

    // F280 焦点位移
    set.add(
        "F280 focus narrative",
        focus_shift_ok(Vec2 { x: 0, y: 0 }, Vec2 { x: 300, y: 400 }, 1000)
            && !focus_shift_ok(Vec2 { x: 0, y: 0 }, Vec2 { x: 900, y: 900 }, 1000),
        "half-diagonal",
    );

    // F281 视差
    set.add("F281 parallax", parallax_offset(800, 100) == 20 && parallax_offset(0, 100) == 100, "depth ratio");

    // F282 粒子
    set.add("F282 particles", particle_budget(0) == 2048 && particle_budget(1) == 1024 && particle_budget(2) == 0, "budget tiers");

    // F283 流体
    set.add("F283 fluid decor", fluid_is_decorative(64, 32) && !fluid_is_decorative(256, 64) && fluid_cost(2, 3) == 6, "cost cap");

    // F284 全景
    let (yaw, pitch) = clamp_view(50_000, -20_000);
    set.add("F284 3d panorama", yaw == 30_000 && pitch == -15_000 && clamp_view(10_000, 5_000) == (10_000, 5_000), "clamped");

    // F285 手势
    set.add("F285 gesture map", gesture_action(150, 1000, 300) && !gesture_action(50, 1000, 300) && !gesture_action(500, 1000, 900), "10% & <=600ms");

    // F286 转场
    set.add(
        "F286 transition grammar",
        Transition::Push.within_cap(400) && !Transition::Push.within_cap(500)
            && Transition::Crossfade.dur_ms_cap() == 250
            && Transition::None.dur_ms_cap() == 0,
        "duration caps",
    );

    // F287 微交互词典
    set.add("F287 micro dictionary", micro_duration("tap") == Some(80) && micro_duration("ripple") == Some(300) && micro_duration("nope").is_none(), "lookup");

    // F288 时间轴编辑
    set.add("F288 timeline trim", trim_tracks(&tracks, 100, 300) == 2 && trim_tracks(&tracks, 700, 900) == 0, "window cut");

    // F289 性能自适应
    set.add(
        "F289 adaptive tier",
        adaptive_tier(60) == AdaptiveTier::Rich && adaptive_tier(40) == AdaptiveTier::Balanced && adaptive_tier(20) == AdaptiveTier::Frugal,
        "fps tiers",
    );

    // F290 降级守门
    set.add("F290 degrade gate", degrade_gate(500) == 0 && degrade_gate(700) == 1 && degrade_gate(900) == 2, "load gates");

    // F291 帧预算
    set.add("F291 frame budget", !frame_over_budget(&[8_000, 6_000]) && frame_over_budget(&[10_000, 8_000]), "16.6ms court");

    // F292 减少动效
    set.add("F292 reduce motion", motion_allowed(true, true) && !motion_allowed(true, false) && motion_allowed(false, false), "pref respected");

    // F293 录制回放
    let echo = [
        MotionTrack { at_ms: 0, dur_ms: 200, easing: 1 },
        MotionTrack { at_ms: 150, dur_ms: 300, easing: 0 },
        MotionTrack { at_ms: 500, dur_ms: 100, easing: 2 },
    ];
    let drift = [
        MotionTrack { at_ms: 0, dur_ms: 200, easing: 1 },
        MotionTrack { at_ms: 150, dur_ms: 300, easing: 0 },
        MotionTrack { at_ms: 520, dur_ms: 100, easing: 2 },
    ];
    set.add("F293 replay match", replay_match(&tracks, &echo) && !replay_match(&tracks, &drift), "identical");

    // F294 缓动实验室
    set.add("F294 easing lab", ease(250, 0) == 250 && ease(1000, 0) == 1000, "linear");
    set.add("F294 ease in-out", ease(500, 3) == 500 && ease(0, 3) == 0 && ease(1000, 3) == 1000, "symmetry");
    set.add("F294 ease out quad", ease(1000, 2) == 1000 && ease(500, 2) == 750, "decel");

    // F295 深度雾
    set.add("F295 fog & dof", fog_alpha(500, 1000) == 350 && fog_alpha(2000, 1000) == 700 && fog_alpha(100, 0) == 0, "fog curve");

    // F296 空间音频
    set.add("F296 spatial audio", pan_for_x(0, 1000) == -1000 && pan_for_x(1000, 1000) == 1000 && pan_for_x(500, 1000) == 0, "pan map");

    // F297 词典 SDK
    set.add("F297 motion sdk", motion_token("rise") == Some((220, 1)) && motion_token("snap") == Some((90, 2)) && motion_token("zoom").is_none(), "tokens");

    // F298 回归走廊
    let golden = [0u16, 250, 500, 750, 1000];
    let ok = [0u16, 260, 510, 760, 1010];
    let bad = [0u16, 400, 500, 750, 1000];
    set.add("F298 regression corridor", corridor_check(&ok, &golden, 20) && !corridor_check(&bad, &golden, 20), "tolerance");

    // F299 眩晕安全
    set.add(
        "F299 vertigo safety",
        vertigo_safe(4_000, 150, false) && !vertigo_safe(6_000, 150, false) && !vertigo_safe(0, 300, false) && !vertigo_safe(0, 100, true),
        "limits",
    );

    // F300 动效年报
    let stats = MotionYearStats { tracks_total: 420, regressions: 3, degrade_events: 12 };
    set.add(
        "F300 motion report",
        stats.report_ready() && MOTION_REPORT_SECTIONS.len() == 4 && !MotionYearStats { tracks_total: 0, regressions: 0, degrade_events: 0 }.report_ready(),
        "sections+counts",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f277_spring_converges() {
        let mut s = SpringState { pos: -500, vel: 0 };
        for _ in 0..500 {
            s.step(0, 100, 150);
        }
        assert!(s.settled(0), "pos={} vel={}", s.pos, s.vel);
    }

    #[test]
    fn f279_lerp_boundaries() {
        let a = Vec2 { x: -100, y: 100 };
        let b = Vec2 { x: 100, y: -100 };
        let p0 = flight_pos(a, b, 0);
        let p1 = flight_pos(a, b, 1000);
        assert_eq!((p0.x, p0.y), (-100, 100));
        assert_eq!((p1.x, p1.y), (100, -100));
    }

    #[test]
    fn f294_easing_monotonic() {
        let mut prev = 0u16;
        for t in (0..=1000).step_by(50) {
            let v = ease(t, 2);
            assert!(v >= prev, "ease-out must be non-decreasing at t={}", t);
            prev = v;
        }
        assert_eq!(prev, 1000);
    }

    #[test]
    fn f296_pan_extremes() {
        assert_eq!(pan_for_x(-999, 1000), -1000); // clamp 左
        assert_eq!(pan_for_x(250, 1000), -500);
        assert_eq!(pan_for_x(0, 0), 0); // 除零保护
    }

    #[test]
    fn f292_reduce_motion_matrix() {
        assert!(motion_allowed(false, false));
        assert!(motion_allowed(false, true));
        assert!(motion_allowed(true, true));
        assert!(!motion_allowed(true, false));
    }

    #[test]
    fn f300_motion_domain_selfcheck_all_pass() {
        let set = run_m600motion_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
