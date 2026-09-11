//! AURORA-1000 AI-10 · 窗口动画与过渡（A226~A250，W2）
//!
//! 物理弹簧动效引擎：统一缓动曲线、临界阻尼弹簧、窗口开合/飞行动效、
//! 贴靠预览、场景过渡、reduce-motion 降级、帧驱动调度器（与输入解耦）。
//! 全部为纯逻辑，固定容量 + 定点整数（缩放 256），无 std/alloc/unsafe/宏/泛型。

use crate::checks::CheckSet;

/// 缓动采样区间端点（输入/输出均为 0..=255）。
pub const EASE_MAX: u8 = 255;

// ---------------------------------------------------------------------------
// A226 — 动效设计系统：统一缓动曲线（0..=255 → 0..=255 纯函数）
// ---------------------------------------------------------------------------

/// 四类统一缓动曲线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

/// 线性：恒等映射。
pub fn ease_linear(t: u8) -> u8 {
    t
}

/// 缓入（三次）：起步慢、收尾快。
pub fn ease_in(t: u8) -> u8 {
    let x = t as u32;
    ((x * x * x) / (255u32 * 255u32)) as u8
}

/// 缓出（三次）：起步快、收尾慢。
pub fn ease_out(t: u8) -> u8 {
    let x = t as u32;
    let inv = 255u32.saturating_sub(x);
    let c = (inv * inv * inv) / (255u32 * 255u32);
    255u32.saturating_sub(c) as u8
}

/// 缓入缓出（smoothstep）：两端均平滑。单次除法保证整数单调。
pub fn ease_in_out(t: u8) -> u8 {
    let x = t as u32;
    let num = 3u32 * 255u32 * x * x - 2u32 * x * x * x;
    (num / (255u32 * 255u32)) as u8
}

/// 曲线分发器：设计系统的统一入口。
pub fn ease(curve: Curve, t: u8) -> u8 {
    match curve {
        Curve::Linear => ease_linear(t),
        Curve::EaseIn => ease_in(t),
        Curve::EaseOut => ease_out(t),
        Curve::EaseInOut => ease_in_out(t),
    }
}

// ---------------------------------------------------------------------------
// A227 — 物理弹簧曲线：临界阻尼弹簧逐步积分
// ---------------------------------------------------------------------------

/// 弹簧状态（定点，缩放 256）：position/velocity 均相对同一量纲。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spring {
    pub position: i32,
    pub velocity: i32,
}

/// 半隐式欧拉一步：位置向目标钳制（零越界）+ 最小推进（防整数截断停滞）。
pub fn spring_step(s: Spring, target: i32) -> Spring {
    let k = 3i32; // 刚度分子
    let c = 56i32; // 阻尼分子（≈临界 2*sqrt(k/256)*256）
    let dx = target.saturating_sub(s.position);
    let mut accel = (dx.saturating_mul(k).saturating_sub(s.velocity.saturating_mul(c))) / 256;
    if accel == 0 && dx != 0 {
        // 整数截断导致零加速时至少推进 1，保证收敛。
        accel = if dx > 0 { 1 } else { -1 };
    }
    let v = s.velocity.saturating_add(accel);
    // 位置钳制：朝目标移动时不越过目标（零越界）。
    let p_raw = s.position.saturating_add(v);
    let p = if v > 0 {
        p_raw.min(target)
    } else if v < 0 {
        p_raw.max(target)
    } else {
        p_raw
    };
    // 收敛吸附：足够接近且速度足够小则瞬锁目标，避免整数残差抖动。
    if target.saturating_sub(p).abs() <= 4 && v.abs() <= 4 {
        Spring { position: target, velocity: 0 }
    } else {
        Spring { position: p, velocity: v }
    }
}

/// 是否到达目标（position==target 且 velocity==0）。
pub fn spring_converged(s: Spring, target: i32) -> bool {
    s.position == target && s.velocity == 0
}

// ---------------------------------------------------------------------------
// A228 — 窗口开合动画：进度 → 缩放/透明度/位移
// ---------------------------------------------------------------------------

/// 窗口某一帧的呈现姿态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowPose {
    pub scale_permille: u16, // 缩放（‰，1000=原大）
    pub alpha_permille: u16, // 不透明度（‰，1000=全不透明）
    pub dy: i16,             // 纵向位移（负=上移，像素）
}

/// 打开：从 80% 缩放 + 透明 + 下移，过渡到原大 + 不透明 + 归位。
pub fn window_open_pose(progress: u8, curve: Curve) -> WindowPose {
    let e = ease(curve, progress) as u32;
    let scale = 800u16 + ((1000u16 - 800u16) * e as u16) / 255;
    let alpha = ((1000u32 * e) / 255) as u16;
    let dy = (((100i32) * (255 - e as i32)) / 255) as i16;
    WindowPose { scale_permille: scale.min(1000), alpha_permille: alpha.min(1000), dy }
}

/// 关闭：与打开镜像（原大→80%，不透明→透明，归位→下移）。
pub fn window_close_pose(progress: u8, curve: Curve) -> WindowPose {
    let e = ease(curve, progress) as u32;
    let scale = 1000u16 - ((1000u16 - 800u16) * e as u16) / 255;
    let alpha = ((1000u32 * (255 - e as u32)) / 255) as u16;
    let dy = (((100i32) * e as i32) / 255) as i16;
    WindowPose { scale_permille: scale, alpha_permille: alpha.min(1000), dy }
}

// ---------------------------------------------------------------------------
// A229 — 最小化飞行动效：起点→终点插值 + 弧线偏移
// ---------------------------------------------------------------------------

/// 整数矩形（像素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// 从窗口矩形飞向坞站矩形的某一帧矩形（含上抛物线弧）。
pub fn minimize_flight(progress: u8, from: Rect, to: Rect) -> Rect {
    let e = ease(Curve::EaseInOut, progress) as i32;
    let x = from.x + ((to.x - from.x) * e) / 255;
    let y = from.y + ((to.y - from.y) * e) / 255;
    let w = from.w + ((to.w - from.w) * e) / 255;
    let h = from.h + ((to.h - from.h) * e) / 255;
    // 抛物线弧：中点最高，peak=高/3，向上抬（y 减小）；两端点弧为 0。
    let peak = from.h.max(0) / 3;
    let dev = e - 128i32;
    let denom = 128i32 * 128i32;
    let shape = (denom - dev * dev).max(0);
    let arc = if e == 0 || e == 255 {
        0
    } else {
        (peak * shape) / denom
    };
    Rect { x, y: y - arc, w, h }
}

// ---------------------------------------------------------------------------
// A230 — 贴靠预览动画：半透明目标矩形计算
// ---------------------------------------------------------------------------

/// 贴靠预览矩形 + 高亮透明度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapPreview {
    pub rect: Rect,
    pub alpha_permille: u16,
}

/// 激活时给出半透明高亮矩形；未激活时透明（不绘制）。
pub fn snap_preview(active: bool, target: Rect) -> SnapPreview {
    if !active {
        SnapPreview { rect: target, alpha_permille: 0 }
    } else {
        SnapPreview { rect: target, alpha_permille: 350 }
    }
}

/// 按屏幕与边（0左/1右/2上/3下）计算半屏贴靠目标矩形。
pub fn snap_edge_rect(screen: Rect, edge: u8) -> Rect {
    match edge {
        0 => Rect { x: screen.x, y: screen.y, w: screen.w / 2, h: screen.h },
        1 => Rect { x: screen.x + screen.w / 2, y: screen.y, w: screen.w / 2, h: screen.h },
        2 => Rect { x: screen.x, y: screen.y, w: screen.w, h: screen.h / 2 },
        _ => Rect { x: screen.x, y: screen.y + screen.h / 2, w: screen.w, h: screen.h / 2 },
    }
}

// ---------------------------------------------------------------------------
// A231 — 场景切换过渡：淡入淡出/滑动，双场景 alpha 权重
// ---------------------------------------------------------------------------

/// 淡入淡出：返回 (场景A不透明度, 场景B不透明度)，A 渐隐、B 渐显。
pub fn scene_fade(progress: u8) -> (u8, u8) {
    let e = ease(Curve::EaseInOut, progress);
    let a = ((255u32 * (255 - e as u32)) / 255) as u8;
    let b = e;
    (a, b)
}

/// 滑动：返回 (场景A左移量, 场景B进入量)，单位‰（相对宽）。
pub fn scene_slide(progress: u8) -> (i16, i16) {
    let e = ease(Curve::EaseInOut, progress) as i16;
    let off_a = -((100i16 * e) / 255);
    let off_b = ((100i16 * (255 - e)) / 255);
    (off_a, off_b)
}

// ---------------------------------------------------------------------------
// A232 — 拖拽弹性：越界橡皮筋阻尼
// ---------------------------------------------------------------------------

/// 橡皮筋：未超界原样返回；超界部分按 bound/(bound+|over|) 衰减。
pub fn drag_elastic(offset: i32, bound: i32) -> i32 {
    let b = bound.abs();
    if offset >= 0 {
        if offset <= b {
            offset
        } else {
            let over = offset - b;
            b + (over * b) / (b + over)
        }
    } else if offset >= -b {
        offset
    } else {
        let over = (-offset) - b;
        -(b + (over * b) / (b + over))
    }
}

// ---------------------------------------------------------------------------
// A233 — 动效时长令牌
// ---------------------------------------------------------------------------

/// 时长令牌（帧数）。Instant=0 用于边界/降级直达。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurationToken {
    Instant,
    Fast,
    Normal,
    Slow,
    Cinematic,
}

/// 令牌 → 帧数。
pub fn duration_frames(t: DurationToken) -> u16 {
    match t {
        DurationToken::Instant => 0,
        DurationToken::Fast => 8,
        DurationToken::Normal => 16,
        DurationToken::Slow => 32,
        DurationToken::Cinematic => 60,
    }
}

// ---------------------------------------------------------------------------
// A234 — 动效与输入解耦：帧驱动调度器（固定 [Anim; 16]）
// ---------------------------------------------------------------------------

/// 调度器容量。
pub const MAX_ANIMS: usize = 16;

/// 动画种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimKind {
    Open,
    Close,
    Minimize,
    Snap,
    Scene,
    Drag,
}

/// 单条动画记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anim {
    pub id: u8,
    pub kind: AnimKind,
    pub start_frame: u32,
    pub duration: u16,
    pub curve: Curve,
    pub active: bool,
    pub cancelled: bool,
}

impl Anim {
    pub const fn idle() -> Anim {
        Anim {
            id: 0,
            kind: AnimKind::Open,
            start_frame: 0,
            duration: 0,
            curve: Curve::Linear,
            active: false,
            cancelled: false,
        }
    }
}

/// 纯函数：给定当前帧与起止，返回 0..=255 进度（不忙等、与输入无关）。
/// 边界：duration 为 0 时瞬时直达（255）。
pub fn progress_at(frame: u32, start_frame: u32, duration: u16) -> u8 {
    if duration == 0 {
        return EASE_MAX;
    }
    if frame <= start_frame {
        return 0;
    }
    let elapsed = frame - start_frame;
    let dur = duration as u32;
    if elapsed >= dur {
        return EASE_MAX;
    }
    ((elapsed * 255) / dur) as u8
}

/// 帧驱动动画调度器：按帧号推进、到期完成、可取消。
#[derive(Clone, Copy, Debug)]
pub struct Scheduler {
    slots: [Anim; MAX_ANIMS],
    count: usize,
}

impl Scheduler {
    pub const fn new() -> Scheduler {
        Scheduler { slots: [Anim::idle(); MAX_ANIMS], count: 0 }
    }

    /// 入队一条动画；队列满返回 false。
    pub fn schedule(&mut self, a: Anim) -> bool {
        if self.count >= MAX_ANIMS {
            return false;
        }
        self.slots[self.count] = a;
        self.count += 1;
        true
    }

    /// 取消（按 id）；重复取消返回 false。
    pub fn cancel(&mut self, id: u8) -> bool {
        for i in 0..self.count {
            if self.slots[i].id == id && self.slots[i].active && !self.slots[i].cancelled {
                let mut a = self.slots[i];
                a.cancelled = true;
                a.active = false;
                self.slots[i] = a;
                return true;
            }
        }
        false
    }

    /// 当前进度：运行中按帧算，已完成为 255，已取消为 None。
    pub fn progress_of(&self, id: u8, frame: u32) -> Option<u8> {
        for i in 0..self.count {
            let a = self.slots[i];
            if a.id == id {
                if a.cancelled {
                    return None;
                }
                if !a.active {
                    return Some(255);
                }
                return Some(progress_at(frame, a.start_frame, a.duration));
            }
        }
        None
    }

    /// 按当前帧推进：到点的动画置为完成（不忙等）。
    pub fn advance(&mut self, frame: u32) {
        for i in 0..self.count {
            let mut a = self.slots[i];
            if a.active && !a.cancelled {
                if progress_at(frame, a.start_frame, a.duration) >= 255 {
                    a.active = false;
                }
                self.slots[i] = a;
            }
        }
    }

    pub fn active_count(&self) -> usize {
        (0..self.count)
            .filter(|i| self.slots[*i].active && !self.slots[*i].cancelled)
            .count()
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// A235 — reduce-motion 降级：启用时全部瞬时直达
// ---------------------------------------------------------------------------

/// 应用 reduce-motion：启用时直接返回终点值 255。
pub fn apply_reduce_motion(enabled: bool, value: u8) -> u8 {
    if enabled {
        EASE_MAX
    } else {
        value
    }
}

/// 进度计算纳入 reduce-motion：启用时瞬时直达。
pub fn reduce_motion_progress(enabled: bool, frame: u32, start: u32, dur: u16) -> u8 {
    if enabled {
        EASE_MAX
    } else {
        progress_at(frame, start, dur)
    }
}

// ---------------------------------------------------------------------------
// A236 — 动效预览工具：将曲线采样进固定缓冲
// ---------------------------------------------------------------------------

/// 把曲线在给定时长内均匀采样进 out（最多 64 点），返回实际点数。
pub fn preview_samples(curve: Curve, duration: u16, out: &mut [u8]) -> usize {
    let n = out.len().min(64);
    for i in 0..n {
        let frame = if duration == 0 {
            0
        } else {
            (i as u32) * (duration as u32) / (n as u32)
        };
        let p = progress_at(frame, 0, duration);
        out[i] = ease(curve, p);
    }
    n
}

// ---------------------------------------------------------------------------
// A237 — 动效 A/B 收藏：固定容量收藏集
// ---------------------------------------------------------------------------

/// 收藏集容量。
pub const MAX_FAVORITES: usize = 8;

/// A/B 收藏的动效预设 id 集合（去重、固定容量）。
#[derive(Clone, Copy, Debug)]
pub struct Favorites {
    ids: [u8; MAX_FAVORITES],
    count: usize,
}

impl Favorites {
    pub const fn new() -> Favorites {
        Favorites { ids: [0; MAX_FAVORITES], count: 0 }
    }

    pub fn add(&mut self, id: u8) -> bool {
        if self.contains(id) {
            return false;
        }
        if self.count >= MAX_FAVORITES {
            return false;
        }
        self.ids[self.count] = id;
        self.count += 1;
        true
    }

    pub fn contains(&self, id: u8) -> bool {
        (0..self.count).any(|i| self.ids[i] == id)
    }

    pub fn remove(&mut self, id: u8) -> bool {
        for i in 0..self.count {
            if self.ids[i] == id {
                for j in i..self.count - 1 {
                    self.ids[j] = self.ids[j + 1];
                }
                self.count -= 1;
                return true;
            }
        }
        false
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// A238 — 动效与音效联动：进度 → 音效提示
// ---------------------------------------------------------------------------

/// 音效提示点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioCue {
    None,
    WhooshStart,
    Tick,
    WhooshEnd,
}

/// 跨进度时触发对应音效（如穿过阈值）。
pub fn motion_audio_cue(prev: u8, now: u8) -> AudioCue {
    if prev < 24 && now >= 24 {
        AudioCue::WhooshStart
    } else if prev < 128 && now >= 128 {
        AudioCue::Tick
    } else if prev < 240 && now >= 240 {
        AudioCue::WhooshEnd
    } else {
        AudioCue::None
    }
}

/// 音效音量随进度（0..=255）。
pub fn motion_volume(now: u8) -> u8 {
    now
}

// ---------------------------------------------------------------------------
// A239 — 动效一致性 lint：校验动画配置合法
// ---------------------------------------------------------------------------

/// 配置合法：曲线有效且时长不超预算（≤120 帧）。
pub fn lint_motion(duration: u16, curve: Curve) -> bool {
    let curve_ok = matches!(curve, Curve::Linear | Curve::EaseIn | Curve::EaseOut | Curve::EaseInOut);
    curve_ok && duration <= 120
}

// ---------------------------------------------------------------------------
// A240 — 动效性能预算：活跃条数预算
// ---------------------------------------------------------------------------

/// 活跃动画预算（等于调度器容量）。
pub const MOTION_ACTIVE_BUDGET: usize = MAX_ANIMS;

/// 是否仍在预算内。
pub fn within_motion_budget(active: usize) -> bool {
    active <= MOTION_ACTIVE_BUDGET
}

/// 估计负载（‰，相对最大容量）。
pub fn motion_cost_permille(active: usize) -> u16 {
    ((active as u32 * 1000) / MOTION_ACTIVE_BUDGET as u32) as u16
}

// ---------------------------------------------------------------------------
// A241 — 动效可观测：负载度量化
// ---------------------------------------------------------------------------

/// 活跃条数 → 负载千分比。
pub fn motion_load_permille(active: usize) -> u16 {
    ((active as u32 * 1000) / MAX_ANIMS as u32) as u16
}

// ---------------------------------------------------------------------------
// A242 — 动效文档
// ---------------------------------------------------------------------------

/// 动效设计系统说明（单数据源描述）。
pub fn motion_design_doc() -> &'static str {
    "AURORA motion design system v1: unified easing, critically-damped spring, frame-driven scheduler"
}

// ---------------------------------------------------------------------------
// A243 — 动效自检收口：核心不变量汇总
// ---------------------------------------------------------------------------

/// 核心不变量（供自检调用）：缓动端点、弹簧收敛、reduce 直达、调度到期。
pub fn motion_selfcheck_invariants() -> bool {
    let ok_ease = ease(Curve::Linear, 0) == 0
        && ease(Curve::Linear, 255) == 255
        && ease(Curve::EaseInOut, 0) == 0
        && ease(Curve::EaseInOut, 255) == 255;

    let mut s = Spring { position: 0, velocity: 0 };
    let mut conv = false;
    for _ in 0..1000 {
        s = spring_step(s, 256000);
        if spring_converged(s, 256000) {
            conv = true;
            break;
        }
    }

    let rm = reduce_motion_progress(true, 50, 0, 16) == 255;

    let mut sch = Scheduler::new();
    let _ = sch.schedule(Anim {
        id: 9,
        kind: AnimKind::Open,
        start_frame: 0,
        duration: 1,
        curve: Curve::Linear,
        active: true,
        cancelled: false,
    });
    sch.advance(5);
    let done = sch.progress_of(9, 5) == Some(255);

    ok_ease && conv && rm && done
}

// ---------------------------------------------------------------------------
// A244 — 窗口动画与过渡自检（由 run_motion_checks 实现，见文件尾）
// ---------------------------------------------------------------------------

// （该自检即下方的 run_motion_checks 入口。）

// ---------------------------------------------------------------------------
// A245 — 窗口动画与过渡性能预算：单帧时长预算
// ---------------------------------------------------------------------------

/// 单帧动效预算（ms），60fps ≈ 16ms。
pub const MOTION_FRAME_BUDGET_MS: u16 = 16;

/// 单帧耗是否不超预算。
pub fn within_frame_budget(frame_ms: u16) -> bool {
    frame_ms <= MOTION_FRAME_BUDGET_MS
}

// ---------------------------------------------------------------------------
// A246 — 窗口动画与过渡可观测：运行统计
// ---------------------------------------------------------------------------

/// 动效运行统计（无分配）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionStats {
    pub active: usize,
    pub frames_run: u32,
    pub completed: u32,
    pub cancelled: u32,
}

impl MotionStats {
    pub const fn new() -> MotionStats {
        MotionStats { active: 0, frames_run: 0, completed: 0, cancelled: 0 }
    }

    pub fn record(&mut self, completed: bool, cancelled: bool) {
        self.frames_run = self.frames_run.saturating_add(1);
        if completed {
            self.completed = self.completed.saturating_add(1);
        }
        if cancelled {
            self.cancelled = self.cancelled.saturating_add(1);
        }
    }

    pub fn active_load_permille(&self, max: usize) -> u16 {
        if max == 0 {
            0
        } else {
            ((self.active as u32 * 1000) / max as u32) as u16
        }
    }
}

// ---------------------------------------------------------------------------
// A247 — 窗口动画与过渡模糊测试：对任意输入均不恐慌
// ---------------------------------------------------------------------------

/// 模糊入口：任意帧/时长/reduce/曲线组合，恒返回 0..=255。
pub fn motion_fuzz(frame: u32, dur: u16, reduce: bool, curve: Curve) -> u8 {
    let p = reduce_motion_progress(reduce, frame, 0, dur);
    ease(curve, p)
}

// ---------------------------------------------------------------------------
// A248 — 窗口动画与过渡文档
// ---------------------------------------------------------------------------

/// 模块文档小节。
pub fn motion_doc_section() -> &'static str {
    "window animation & transition: open/close, minimize flight, snap preview, scene transition, reduce-motion"
}

// ---------------------------------------------------------------------------
// A249 — 窗口动画与过渡降级链：按负载分级
// ---------------------------------------------------------------------------

/// 降级等级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeLevel {
    Full,
    Reduced,
    Minimal,
    Static,
}

/// 按负载千分比选择降级等级。
pub fn motion_degrade(load_permille: u16) -> DegradeLevel {
    if load_permille >= 1000 {
        DegradeLevel::Static
    } else if load_permille >= 750 {
        DegradeLevel::Minimal
    } else if load_permille >= 400 {
        DegradeLevel::Reduced
    } else {
        DegradeLevel::Full
    }
}

/// 按活跃条数选择降级等级。
pub fn motion_degrade_active(active: usize) -> DegradeLevel {
    motion_degrade(motion_load_permille(active))
}

// ---------------------------------------------------------------------------
// A250 — 窗口动画与过渡域自检收口
// ---------------------------------------------------------------------------

/// 域自检：25 项不变量全真。
pub fn run_motion_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-motion");

    // A226 动效设计系统
    set.add(
        "A226 动效设计系统",
        ease(Curve::Linear, 0) == 0
            && ease(Curve::Linear, 255) == 255
            && ease(Curve::EaseIn, 0) == 0
            && ease(Curve::EaseIn, 255) == 255
            && ease(Curve::EaseOut, 0) == 0
            && ease(Curve::EaseOut, 255) == 255
            && ease(Curve::EaseInOut, 0) == 0
            && ease(Curve::EaseInOut, 255) == 255,
        "easing endpoints",
    );

    // A227 物理弹簧曲线
    let mut s = Spring { position: 0, velocity: 0 };
    let mut conv = false;
    for _ in 0..1000 {
        s = spring_step(s, 256000);
        if spring_converged(s, 256000) {
            conv = true;
            break;
        }
    }
    set.add("A227 物理弹簧曲线", conv && s.position == 256000, "spring converges");

    // A228 窗口开合动画
    let o0 = window_open_pose(0, Curve::EaseOut);
    let o1 = window_open_pose(255, Curve::EaseOut);
    let c0 = window_close_pose(0, Curve::EaseOut);
    let c1 = window_close_pose(255, Curve::EaseOut);
    set.add(
        "A228 窗口开合动画",
        o0.alpha_permille == 0
            && o1.alpha_permille == 1000
            && o1.scale_permille == 1000
            && c0.alpha_permille == 1000
            && c1.alpha_permille == 0,
        "open/close",
    );

    // A229 最小化飞行动效
    let from = Rect { x: 100, y: 100, w: 400, h: 300 };
    let to = Rect { x: 10, y: 10, w: 120, h: 90 };
    let f0 = minimize_flight(0, from, to);
    let f1 = minimize_flight(255, from, to);
    set.add(
        "A229 最小化飞行动效",
        f0.x == from.x
            && f0.y == from.y
            && f1.x == to.x
            && f1.y == to.y
            && f1.w == to.w
            && f1.h == to.h,
        "flight endpoints",
    );

    // A230 贴靠预览动画
    let sp = snap_preview(true, to);
    let sp0 = snap_preview(false, to);
    set.add("A230 贴靠预览动画", sp.alpha_permille > 0 && sp0.alpha_permille == 0, "snap alpha");

    // A231 场景切换过渡
    let (fa, fb) = scene_fade(0);
    let (fa1, fb1) = scene_fade(255);
    set.add("A231 场景切换过渡", fa == 255 && fb == 0 && fa1 == 0 && fb1 == 255, "fade endpoints");

    // A232 拖拽弹性
    set.add(
        "A232 拖拽弹性",
        drag_elastic(50, 100) == 50
            && drag_elastic(200, 100).abs() < 200
            && drag_elastic(-200, 100).abs() < 200,
        "rubber band",
    );

    // A233 动效时长令牌
    set.add(
        "A233 动效时长令牌",
        duration_frames(DurationToken::Instant) == 0
            && duration_frames(DurationToken::Normal) == 16
            && duration_frames(DurationToken::Cinematic) == 60,
        "tokens",
    );

    // A234 动效与输入解耦
    let mut sch = Scheduler::new();
    let _ = sch.schedule(Anim {
        id: 1,
        kind: AnimKind::Open,
        start_frame: 0,
        duration: 16,
        curve: Curve::EaseOut,
        active: true,
        cancelled: false,
    });
    let _ = sch.schedule(Anim {
        id: 2,
        kind: AnimKind::Close,
        start_frame: 0,
        duration: 16,
        curve: Curve::EaseOut,
        active: true,
        cancelled: false,
    });
    let c1 = sch.cancel(1);
    let c2 = sch.cancel(1);
    let p = progress_at(16, 0, 16);
    let zero = progress_at(5, 0, 0);
    set.add(
        "A234 动效与输入解耦",
        c1 && !c2 && p == 255 && zero == 255 && MAX_ANIMS == 16,
        "scheduler",
    );

    // A235 reduce-motion 降级
    set.add(
        "A235 reduce-motion 降级",
        reduce_motion_progress(true, 0, 0, 16) == 255
            && reduce_motion_progress(true, 100, 0, 16) == 255
            && apply_reduce_motion(true, 123) == 255
            && reduce_motion_progress(false, 0, 0, 16) == 0,
        "reduced instant",
    );

    // A236 动效预览工具
    let mut buf = [0u8; 8];
    let n = preview_samples(Curve::EaseOut, 16, &mut buf);
    set.add("A236 动效预览工具", n == 8 && buf[0] <= buf[7], "preview samples");

    // A237 动效 A/B 收藏
    let mut fav = Favorites::new();
    let a = fav.add(7);
    let b = fav.add(7);
    let _ = fav.add(8);
    let r = fav.remove(7);
    set.add(
        "A237 动效 A/B 收藏",
        a && !b && fav.contains(8) && r && !fav.contains(7),
        "favorites",
    );

    // A238 动效与音效联动
    let cue = motion_audio_cue(0, 30);
    set.add(
        "A238 动效与音效联动",
        cue == AudioCue::WhooshStart && motion_volume(128) == 128,
        "audio cue",
    );

    // A239 动效一致性 lint
    set.add(
        "A239 动效一致性 lint",
        lint_motion(16, Curve::EaseOut)
            && lint_motion(16, Curve::EaseInOut)
            && !lint_motion(200, Curve::EaseOut),
        "lint",
    );

    // A240 动效性能预算
    set.add(
        "A240 动效性能预算",
        within_motion_budget(10)
            && !within_motion_budget(MAX_ANIMS + 1)
            && motion_cost_permille(8) < 1000,
        "budget",
    );

    // A241 动效可观测
    set.add(
        "A241 动效可观测",
        motion_load_permille(8) == (8u32 * 1000 / MAX_ANIMS as u32) as u16,
        "load",
    );

    // A242 动效文档
    set.add(
        "A242 动效文档",
        motion_design_doc().len() > 0 && motion_design_doc().starts_with("AURORA"),
        "doc",
    );

    // A243 动效自检收口
    set.add("A243 动效自检收口", motion_selfcheck_invariants(), "invariants");

    // A244 窗口动画与过渡自检
    set.add(
        "A244 窗口动画与过渡自检",
        CheckSet::new("aurora-motion").domain == "aurora-motion"
            && ease(Curve::EaseInOut, 64) <= ease(Curve::EaseInOut, 128),
        "self-check",
    );

    // A245 窗口动画与过渡性能预算
    set.add(
        "A245 窗口动画与过渡性能预算",
        within_frame_budget(16) && !within_frame_budget(17) && MOTION_FRAME_BUDGET_MS == 16,
        "frame budget",
    );

    // A246 窗口动画与过渡可观测
    let mut stats = MotionStats::new();
    stats.record(true, false);
    stats.record(false, true);
    set.add(
        "A246 窗口动画与过渡可观测",
        stats.frames_run == 2 && stats.completed == 1 && stats.cancelled == 1
            && stats.active_load_permille(MAX_ANIMS) <= 1000,
        "stats",
    );

    // A247 窗口动画与过渡模糊测试
    let fz = motion_fuzz(12345, 600, true, Curve::EaseInOut);
    set.add(
        "A247 窗口动画与过渡模糊测试",
        motion_fuzz(0, 0, false, Curve::EaseInOut) <= 255 && fz <= 255,
        "fuzz",
    );

    // A248 窗口动画与过渡文档
    set.add("A248 窗口动画与过渡文档", motion_doc_section().len() > 0, "doc section");

    // A249 窗口动画与过渡降级链
    set.add(
        "A249 窗口动画与过渡降级链",
        motion_degrade(0) == DegradeLevel::Full
            && motion_degrade(1000) == DegradeLevel::Static
            && motion_degrade_active(MAX_ANIMS) == DegradeLevel::Static,
        "degrade",
    );

    // A250 窗口动画与过渡域自检收口
    set.add(
        "A250 窗口动画与过渡域自检收口",
        set.domain == "aurora-motion" && motion_selfcheck_invariants(),
        "closure",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a226_easing_endpoints() {
        assert_eq!(ease_linear(0), 0);
        assert_eq!(ease_linear(255), 255);
        assert_eq!(ease_in(0), 0);
        assert_eq!(ease_in(255), 255);
        assert_eq!(ease_out(0), 0);
        assert_eq!(ease_out(255), 255);
        assert_eq!(ease_in_out(0), 0);
        assert_eq!(ease_in_out(255), 255);
    }

    #[test]
    fn a226_easing_monotonic() {
        let mut prev = 0u8;
        for t in 1u8..=255 {
            assert!(ease(Curve::EaseIn, t) >= prev);
            prev = ease(Curve::EaseIn, t);
        }
        prev = 0;
        for t in 1u8..=255 {
            assert!(ease(Curve::EaseInOut, t) >= prev);
            prev = ease(Curve::EaseInOut, t);
        }
    }

    #[test]
    fn a227_spring_converges() {
        let mut s = Spring { position: 0, velocity: 0 };
        let target = 256000i32;
        let mut ok = false;
        for _ in 0..1000 {
            s = spring_step(s, target);
            if spring_converged(s, target) {
                ok = true;
                break;
            }
        }
        assert!(ok);
        assert_eq!(s.position, target);
    }

    #[test]
    fn a227_spring_no_overshoot() {
        let mut s = Spring { position: 0, velocity: 0 };
        let target = 256000i32;
        for _ in 0..400 {
            s = spring_step(s, target);
            assert!(s.position <= target, "position must not exceed target");
            if spring_converged(s, target) {
                break;
            }
        }
    }

    #[test]
    fn a228_window_open_close() {
        let open = window_open_pose(0, Curve::EaseOut);
        let open_end = window_open_pose(255, Curve::EaseOut);
        assert_eq!(open.alpha_permille, 0);
        assert_eq!(open_end.alpha_permille, 1000);
        assert_eq!(open_end.scale_permille, 1000);
        let close = window_close_pose(0, Curve::EaseOut);
        let close_end = window_close_pose(255, Curve::EaseOut);
        assert_eq!(close.alpha_permille, 1000);
        assert_eq!(close_end.alpha_permille, 0);
    }

    #[test]
    fn a229_minimize_endpoints() {
        let from = Rect { x: 100, y: 100, w: 400, h: 300 };
        let to = Rect { x: 10, y: 10, w: 120, h: 90 };
        let a = minimize_flight(0, from, to);
        let b = minimize_flight(255, from, to);
        assert_eq!((a.x, a.y, a.w, a.h), (from.x, from.y, from.w, from.h));
        assert_eq!((b.x, b.y, b.w, b.h), (to.x, to.y, to.w, to.h));
    }

    #[test]
    fn a229_minimize_arc_peak_mid() {
        let from = Rect { x: 0, y: 0, w: 400, h: 300 };
        let to = Rect { x: 0, y: 0, w: 0, h: 0 };
        let mid = minimize_flight(128, from, to);
        let start = minimize_flight(0, from, to);
        // 中点弧最高点（y 最小）。
        assert!(mid.y <= start.y, "arc lifts at mid");
    }

    #[test]
    fn a230_snap_preview() {
        let target = Rect { x: 0, y: 0, w: 100, h: 100 };
        assert_eq!(snap_preview(false, target).alpha_permille, 0);
        assert!(snap_preview(true, target).alpha_permille > 0);
        let left = snap_edge_rect(target, 0);
        assert_eq!((left.x, left.y, left.w, left.h), (0, 0, 50, 100));
        let bottom = snap_edge_rect(target, 3);
        assert_eq!((bottom.x, bottom.y, bottom.w, bottom.h), (0, 50, 100, 50));
    }

    #[test]
    fn a231_scene_fade() {
        assert_eq!(scene_fade(0), (255, 0));
        assert_eq!(scene_fade(255), (0, 255));
        // 进度 0：A 在原位、B 在右侧外（offset=+100）。
        let (a, b) = scene_slide(0);
        assert_eq!((a, b), (0, 100));
        // 进度 255：A 滑出左侧（offset=-100）、B 归位。
        let (a1, b1) = scene_slide(255);
        assert_eq!((a1, b1), (-100, 0));
    }

    #[test]
    fn a232_drag_elastic_bound() {
        assert_eq!(drag_elastic(50, 100), 50);
        assert_eq!(drag_elastic(-50, 100), -50);
        assert!(drag_elastic(200, 100).abs() < 200);
        assert!(drag_elastic(-200, 100).abs() < 200);
        // bound=0：任意越界均被夹到 0。
        assert_eq!(drag_elastic(200, 0), 0);
    }

    #[test]
    fn a233_duration_tokens() {
        assert_eq!(duration_frames(DurationToken::Instant), 0);
        assert_eq!(duration_frames(DurationToken::Fast), 8);
        assert_eq!(duration_frames(DurationToken::Normal), 16);
        assert_eq!(duration_frames(DurationToken::Slow), 32);
        assert_eq!(duration_frames(DurationToken::Cinematic), 60);
    }

    #[test]
    fn a234_scheduler_progress() {
        let mut sch = Scheduler::new();
        assert!(sch.schedule(Anim {
            id: 1,
            kind: AnimKind::Open,
            start_frame: 0,
            duration: 16,
            curve: Curve::EaseOut,
            active: true,
            cancelled: false,
        }));
        assert_eq!(sch.progress_of(1, 0), Some(0));
        assert_eq!(sch.progress_of(1, 8), Some(127));
        assert_eq!(sch.progress_of(1, 16), Some(255));
        // 推进后变为完成。
        sch.advance(16);
        assert_eq!(sch.active_count(), 0);
    }

    #[test]
    fn a234_scheduler_cancel() {
        let mut sch = Scheduler::new();
        let _ = sch.schedule(Anim {
            id: 1,
            kind: AnimKind::Open,
            start_frame: 0,
            duration: 16,
            curve: Curve::EaseOut,
            active: true,
            cancelled: false,
        });
        assert!(sch.cancel(1));
        assert!(!sch.cancel(1), "repeated cancel must fail");
        assert_eq!(sch.progress_of(1, 5), None, "cancelled -> None");
    }

    #[test]
    fn a234_scheduler_full() {
        let mut sch = Scheduler::new();
        for i in 0..MAX_ANIMS {
            assert!(sch.schedule(Anim {
                id: i as u8,
                kind: AnimKind::Drag,
                start_frame: 0,
                duration: 8,
                curve: Curve::Linear,
                active: true,
                cancelled: false,
            }));
        }
        assert_eq!(sch.len(), MAX_ANIMS);
        // 第 17 条入队失败（队列满边界）。
        assert!(!sch.schedule(Anim {
            id: 99,
            kind: AnimKind::Drag,
            start_frame: 0,
            duration: 8,
            curve: Curve::Linear,
            active: true,
            cancelled: false,
        }));
    }

    #[test]
    fn a234_progress_zero_duration() {
        // duration 0 瞬时直达。
        assert_eq!(progress_at(5, 0, 0), 255);
        let mut sch = Scheduler::new();
        let _ = sch.schedule(Anim {
            id: 1,
            kind: AnimKind::Open,
            start_frame: 0,
            duration: 0,
            curve: Curve::Linear,
            active: true,
            cancelled: false,
        });
        assert_eq!(sch.progress_of(1, 0), Some(255));
    }

    #[test]
    fn a235_reduce_motion_instant() {
        assert_eq!(reduce_motion_progress(true, 0, 0, 16), 255);
        assert_eq!(reduce_motion_progress(true, 999, 0, 16), 255);
        assert_eq!(apply_reduce_motion(true, 123), 255);
        assert_eq!(reduce_motion_progress(false, 0, 0, 16), 0);
        assert_eq!(reduce_motion_progress(false, 8, 0, 16), 127);
    }

    #[test]
    fn a236_preview_samples() {
        let mut buf = [0u8; 8];
        let n = preview_samples(Curve::EaseOut, 16, &mut buf);
        assert_eq!(n, 8);
        for i in 1..8 {
            assert!(buf[i] >= buf[i - 1], "ease-out non-decreasing");
        }
        let mut tiny = [0u8; 4];
        assert_eq!(preview_samples(Curve::Linear, 0, &mut tiny), 4);
    }

    #[test]
    fn a237_favorites() {
        let mut fav = Favorites::new();
        assert!(fav.add(7));
        assert!(!fav.add(7), "duplicate rejected");
        assert!(fav.contains(7));
        assert!(fav.add(8));
        assert!(fav.remove(7));
        assert!(!fav.contains(7));
        // 填满后拒绝新增。
        for i in 0..MAX_FAVORITES {
            let _ = fav.add(100 + i as u8);
        }
        assert!(!fav.add(250), "full favorites rejected");
    }

    #[test]
    fn a238_audio_cue() {
        assert_eq!(motion_audio_cue(0, 30), AudioCue::WhooshStart);
        assert_eq!(motion_audio_cue(100, 130), AudioCue::Tick);
        assert_eq!(motion_audio_cue(200, 250), AudioCue::WhooshEnd);
        assert_eq!(motion_audio_cue(50, 60), AudioCue::None);
        assert_eq!(motion_volume(200), 200);
    }

    #[test]
    fn a239_lint() {
        assert!(lint_motion(16, Curve::EaseOut));
        assert!(lint_motion(120, Curve::EaseInOut));
        assert!(!lint_motion(200, Curve::EaseOut));
    }

    #[test]
    fn a240_budget() {
        assert!(within_motion_budget(10));
        assert!(within_motion_budget(MAX_ANIMS));
        assert!(!within_motion_budget(MAX_ANIMS + 1));
        assert!(motion_cost_permille(8) < 1000);
        assert_eq!(motion_cost_permille(MAX_ANIMS), 1000);
    }

    #[test]
    fn a241_observability() {
        assert_eq!(motion_load_permille(0), 0);
        assert_eq!(motion_load_permille(8), 500);
        assert_eq!(motion_load_permille(MAX_ANIMS), 1000);
    }

    #[test]
    fn a242_doc() {
        let d = motion_design_doc();
        assert!(d.len() > 0);
        assert!(d.starts_with("AURORA"));
    }

    #[test]
    fn a243_selfcheck_invariants() {
        assert!(motion_selfcheck_invariants());
    }

    #[test]
    fn a245_frame_budget() {
        assert!(within_frame_budget(16));
        assert!(within_frame_budget(1));
        assert!(!within_frame_budget(17));
        assert_eq!(MOTION_FRAME_BUDGET_MS, 16);
    }

    #[test]
    fn a246_stats() {
        let mut stats = MotionStats::new();
        assert_eq!(stats.frames_run, 0);
        stats.record(true, false);
        stats.record(false, true);
        assert_eq!(stats.frames_run, 2);
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.cancelled, 1);
        assert_eq!(stats.active_load_permille(MAX_ANIMS), 0);
    }

    #[test]
    fn a247_fuzz_no_panic() {
        // 任意极端输入都不应恐慌，且输出恒在 0..=255。
        for f in [0u32, 1, 100000, u32::MAX] {
            for d in [0u16, 1, 600, u16::MAX] {
                for rm in [true, false] {
                    let v = motion_fuzz(f, d, rm, Curve::EaseInOut);
                    assert!(v <= 255);
                }
            }
        }
    }

    #[test]
    fn a248_doc_section() {
        assert!(motion_doc_section().len() > 0);
    }

    #[test]
    fn a249_degrade() {
        assert_eq!(motion_degrade(0), DegradeLevel::Full);
        assert_eq!(motion_degrade(400), DegradeLevel::Reduced);
        assert_eq!(motion_degrade(750), DegradeLevel::Minimal);
        assert_eq!(motion_degrade(1000), DegradeLevel::Static);
        assert_eq!(motion_degrade_active(MAX_ANIMS), DegradeLevel::Static);
        assert_eq!(motion_degrade_active(0), DegradeLevel::Full);
    }

    #[test]
    fn a250_checks_complete() {
        let set = run_motion_checks();
        assert_eq!(set.domain, "aurora-motion");
        assert_eq!(set.len(), 25, "exactly A226..A250");
        assert!(set.all_passed(), "all motion checks must pass");
        let (passed, failed) = set.tally();
        assert_eq!((passed, failed), (25, 0));
    }
}
