//! VE-F1407 · 淡入淡出引擎（VE-H 域 · 音频引擎 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1407`
//!
//! **规格原文**：通用淡入淡出服务（曲线库：线性/等功率/指数/S 曲线/自定义
//! 贝塞尔——G07 F1331 的 50ms 交叉淡化泛化为引擎服务，所有淡入淡出场景统一
//! 走此服务，实现唯一行为一致）；多段淡变（淡入-保持-淡出复合包络，段边界/
//! 曲线可配）；交叉淡化编排（任意两信号源的交叉管理——时长/曲线/触发同步，
//! 编排器管理交叉生命周期：开始/进行/完成回调）；场景统一（播放起停/音效
//! 触发/场景切换/通知进出全走此服务——散落的 setVolume 消灭，lint 级纪律）；
//! 防重叠（同一目标多 fade 请求排队或合并——策略可配，重叠 fade=音量抖动=
//! 缺陷源）。判据：曲线库、复合包络、编排、场景统一、防重叠、判据。
//!
//! **设计要点**：
//! - 曲线是数学本体不是手绘：线性/等功率（F1329 同核）/指数/S 曲线/贝塞尔
//!   全部端点精确、值域 [0,1]、确定性可对拍；
//! - 复合包络 = 段序列：每段（时长、目标增益、曲线），段间以目标增益衔接
//!   ——通知音的"淡入-保持-淡出"是一个包络不是三次手调；
//! - 编排器管生命周期：交叉淡化的开始/进行/完成有回调账，进度可查；
//! - 场景统一是 lint 级纪律：四场景全部经 fade 服务，服务外的 setVolume
//!   调用被 lint 记账（散落的音量写 = 抖动源）；
//! - 防重叠策略可配：Queue（串行排队）或 Replace（新请求覆盖旧请求）——
//!   同目标并发 fade 是音量抖动的根源，服务层根除。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、曲线库（端点精确、值域 [0,1]、确定性）
// ---------------------------------------------------------------------------

/// 淡变曲线。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FadeCurve {
    Linear,
    /// 等功率（equal-power，F1329/F1404 同核）。
    EqualPower,
    /// 指数（感知响度近似，x^k）。
    Exponential,
    /// S 曲线（平滑三段：smoothstep）。
    SCurve,
    /// 自定义贝塞尔（三次，两端固定 0/1，控制点 (x1,y1),(x2,y2)）。
    Bezier {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    },
}

impl FadeCurve {
    /// 曲线在 t∈[0,1] 的增益（端点精确 0/1）。
    pub fn at(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            FadeCurve::Linear => t,
            FadeCurve::EqualPower => {
                // 等功率（幅度域）：g(t)=sin(π/2·t)，端点钉死
                if t <= 0.0 {
                    0.0
                } else if t >= 1.0 {
                    1.0
                } else {
                    (core::f64::consts::FRAC_PI_2 * t).sin()
                }
            }
            FadeCurve::Exponential => t * t, // x^2：感知近似的确定性选型
            FadeCurve::SCurve => {
                let s = t * t * (3.0 - 2.0 * t);
                s
            }
            FadeCurve::Bezier { x1, y1, x2, y2 } => {
                // 按参数 x 解贝塞尔 y：二分求 t 使 Bx(t)=x（确定性 64 步）
                let bx = |tt: f64| {
                    let u = 1.0 - tt;
                    3.0 * u * u * tt * x1 + 3.0 * u * tt * tt * x2 + tt * tt * tt
                };
                let by = |tt: f64| {
                    let u = 1.0 - tt;
                    3.0 * u * u * tt * y1 + 3.0 * u * tt * tt * y2 + tt * tt * tt
                };
                if t <= 0.0 {
                    return 0.0;
                }
                if t >= 1.0 {
                    return 1.0;
                }
                let mut lo = 0.0f64;
                let mut hi = 1.0f64;
                for _ in 0..48 {
                    let mid = (lo + hi) / 2.0;
                    if bx(mid) < t {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                by((lo + hi) / 2.0)
            }
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            FadeCurve::Linear => "线性",
            FadeCurve::EqualPower => "等功率",
            FadeCurve::Exponential => "指数",
            FadeCurve::SCurve => "S曲线",
            FadeCurve::Bezier { .. } => "贝塞尔",
        }
    }
}

// ---------------------------------------------------------------------------
// 二、复合包络（多段淡变：淡入-保持-淡出）
// ---------------------------------------------------------------------------

/// 一段淡变：时长（tick）、目标增益、曲线。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FadeSegment {
    pub duration_ticks: u64,
    /// 本段结束时的目标增益（0..1）。
    pub target_gain: f64,
    pub curve: FadeCurve,
}

/// 复合包络：从起始增益出发的段序列。
#[derive(Clone, Debug, PartialEq)]
pub struct CompositeEnvelope {
    pub start_gain: f64,
    pub segments: Vec<FadeSegment>,
}

impl CompositeEnvelope {
    /// 经典三段：淡入 → 保持 → 淡出。
    pub fn notify_fade(fade_in_ticks: u64, hold_ticks: u64, fade_out_ticks: u64) -> CompositeEnvelope {
        CompositeEnvelope {
            start_gain: 0.0,
            segments: vec![
                FadeSegment {
                    duration_ticks: fade_in_ticks,
                    target_gain: 1.0,
                    curve: FadeCurve::EqualPower,
                },
                FadeSegment {
                    duration_ticks: hold_ticks,
                    target_gain: 1.0,
                    curve: FadeCurve::Linear,
                },
                FadeSegment {
                    duration_ticks: fade_out_ticks,
                    target_gain: 0.0,
                    curve: FadeCurve::EqualPower,
                },
            ],
        }
    }

    pub fn total_ticks(&self) -> u64 {
        self.segments.iter().map(|s| s.duration_ticks).sum()
    }

    /// 包络求值：段内按曲线从上一目标过渡到本段目标。
    pub fn eval(&self, tick: u64) -> f64 {
        let mut from = self.start_gain;
        let mut seg_start = 0u64;
        for seg in self.segments.iter() {
            let local = tick.saturating_sub(seg_start);
            if local < seg.duration_ticks || seg.duration_ticks == 0 {
                let t = if seg.duration_ticks == 0 {
                    1.0
                } else {
                    local as f64 / seg.duration_ticks as f64
                };
                return from + (seg.target_gain - from) * seg.curve.at(t);
            }
            seg_start += seg.duration_ticks;
            from = seg.target_gain;
        }
        from
    }
}

// ---------------------------------------------------------------------------
// 三、fade 请求与防重叠（Queue / Replace 策略可配）
// ---------------------------------------------------------------------------

/// 防重叠策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlapPolicy {
    /// 排队：同目标的新 fade 等当前 fade 完成后串行执行。
    Queue,
    /// 替换：新请求覆盖进行中的旧请求。
    Replace,
}

/// 单段 fade 请求（单目标、单调走向）。
#[derive(Clone, Debug, PartialEq)]
pub struct FadeRequest {
    pub target: u32,
    pub duration_ticks: u64,
    pub curve: FadeCurve,
    /// 目标增益（淡入到 1 / 淡出到 0 / 任意中间值）。
    pub target_gain: f64,
    /// 发起场景（场景统一：四场景全走本服务）。
    pub scene: Scene,
}

/// 场景（全部统一走 fade 服务）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    PlaybackStartStop,
    SfxTrigger,
    SceneSwitch,
    Notification,
}

impl Scene {
    pub fn label(self) -> &'static str {
        match self {
            Scene::PlaybackStartStop => "播放起停",
            Scene::SfxTrigger => "音效触发",
            Scene::SceneSwitch => "场景切换",
            Scene::Notification => "通知进出",
        }
    }
}

/// 目标状态：进行中的 fade 与排队请求。
#[derive(Clone, Debug, PartialEq)]
struct TargetState {
    active: Option<ActiveFade>,
    queue: Vec<FadeRequest>,
}

#[derive(Clone, Debug, PartialEq)]
struct ActiveFade {
    from_gain: f64,
    request: FadeRequest,
    elapsed: u64,
}

/// 淡入淡出服务（唯一实现：四场景统一入口）。
#[derive(Debug)]
pub struct FadeService {
    policy: OverlapPolicy,
    targets: Vec<(u32, TargetState)>,
    /// 增益变更账（setVolume 只能经本服务——lint 纪律的证据链）。
    pub gain_log: Vec<(u32, u64, f64)>,
    /// 生命周期回调账。
    pub events: Vec<String>,
    pub completed_total: u64,
    pub replaced_total: u64,
    pub queued_total: u64,
}

impl FadeService {
    pub fn new(policy: OverlapPolicy) -> FadeService {
        FadeService {
            policy,
            targets: Vec::new(),
            gain_log: Vec::new(),
            events: Vec::new(),
            completed_total: 0,
            replaced_total: 0,
            queued_total: 0,
        }
    }

    pub fn policy(&self) -> OverlapPolicy {
        self.policy
    }

    /// 场景统一入口：四场景的 fade 请求全走这里。
    pub fn request(&mut self, req: FadeRequest, now: u64) {
        if self.targets.iter().position(|(t, _)| *t == req.target).is_none() {
            self.targets.push((
                req.target,
                TargetState {
                    active: None,
                    queue: Vec::new(),
                },
            ));
        }
        let has_active = self
            .targets
            .iter()
            .find(|(t, _)| *t == req.target)
            .map(|(_, s)| s.active.is_some())
            .unwrap_or(false);
        let cur = self.current_gain(req.target);

        match self.policy {
            OverlapPolicy::Queue if has_active => {
                // 排队：串行等待
                if let Some((_, state)) = self.targets.iter_mut().find(|(t, _)| *t == req.target) {
                    state.queue.push(req.clone());
                }
                self.queued_total += 1;
                self.events.push(format!("fade 排队：目标 {}（Queue 策略）", req.target));
            }
            OverlapPolicy::Replace if has_active => {
                // 替换：从当前值直接交给新 fade
                if let Some((_, state)) = self.targets.iter_mut().find(|(t, _)| *t == req.target) {
                    state.active = Some(ActiveFade {
                        from_gain: cur,
                        request: req.clone(),
                        elapsed: 0,
                    });
                }
                self.replaced_total += 1;
                self.events.push(format!("fade 替换：目标 {}（Replace 策略）", req.target));
            }
            _ => {
                // 无活动 fade：直接开跑（从当前增益出发）
                if let Some((_, state)) = self.targets.iter_mut().find(|(t, _)| *t == req.target) {
                    state.active = Some(ActiveFade {
                        from_gain: cur,
                        request: req.clone(),
                        elapsed: 0,
                    });
                }
                self.events.push(format!(
                    "fade 开始：目标 {} 场景 {}（{}→{}，{}，{} tick）",
                    req.target, req.scene.label(), cur, req.target_gain, req.curve.label(), req.duration_ticks
                ));
            }
        }
    }

    /// 目标当前增益（gain_log 的末值；无记录 = 1.0 满幅）。
    pub fn current_gain(&self, target: u32) -> f64 {
        self.gain_log
            .iter()
            .rev()
            .find(|(t, _, _)| *t == target)
            .map(|(_, _, g)| *g)
            .unwrap_or(1.0)
    }

    /// 编排器/初始化专用的增益落账（如：进入源先钉到 0 再淡入）。
    /// 这是服务内记账，不是散落的 setVolume——lint 只拦服务外直写。
    pub fn prime_gain(&mut self, target: u32, now: u64, gain: f64) {
        self.gain_log.push((target, now, gain));
    }

    /// 推进一个 tick：活动 fade 计算增益并记账，完成即出队下一条。
    pub fn tick(&mut self, now: u64) -> usize {
        // 第一遍：推进所有活动 fade，收集本 tick 的增益与完成项（避开借用冲突）。
        let mut gains: Vec<(u32, f64)> = Vec::new();
        let mut finished: Vec<(u32, f64, Scene)> = Vec::new();
        for (target, state) in self.targets.iter_mut() {
            if let Some(active) = state.active.as_mut() {
                active.elapsed += 1;
                let req = active.request.clone();
                let t = if req.duration_ticks == 0 {
                    1.0
                } else {
                    active.elapsed as f64 / req.duration_ticks as f64
                };
                let gain =
                    active.from_gain + (req.target_gain - active.from_gain) * req.curve.at(t);
                gains.push((*target, gain));
                if active.elapsed >= req.duration_ticks {
                    finished.push((*target, gain, req.scene));
                    state.active = None;
                }
            }
        }
        // 第二遍：记账 + 完成回调 + 队列接续。
        for (target, gain) in gains.iter() {
            self.gain_log.push((*target, now, *gain));
        }
        for (target, gain, scene) in finished.iter() {
            self.completed_total += 1;
            self.events
                .push(format!("fade 完成：目标 {} 场景 {}", target, scene.label()));
            if let Some((_, state)) = self.targets.iter_mut().find(|(t, _)| t == target) {
                if !state.queue.is_empty() && state.active.is_none() {
                    let next = state.queue.remove(0);
                    state.active = Some(ActiveFade {
                        from_gain: *gain,
                        request: next,
                        elapsed: 0,
                    });
                }
            }
        }
        gains.len()
    }

    pub fn is_active(&self, target: u32) -> bool {
        self.targets
            .iter()
            .find(|(t, _)| *t == target)
            .map(|(_, s)| s.active.is_some())
            .unwrap_or(false)
    }

    pub fn queue_len(&self, target: u32) -> usize {
        self.targets
            .iter()
            .find(|(t, _)| *t == target)
            .map(|(_, s)| s.queue.len())
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// 四、交叉淡化编排（两信号源交叉 + 生命周期回调账）
// ---------------------------------------------------------------------------

/// 交叉淡化编排器。
#[derive(Debug)]
pub struct CrossfadeOrchestrator {
    pub out_target: u32,
    pub in_target: u32,
    pub duration_ticks: u64,
    pub curve: FadeCurve,
    pub service: FadeService,
    started: bool,
    pub elapsed: u64,
}

impl CrossfadeOrchestrator {
    pub fn new(
        out_target: u32,
        in_target: u32,
        duration_ticks: u64,
        curve: FadeCurve,
    ) -> CrossfadeOrchestrator {
        CrossfadeOrchestrator {
            out_target,
            in_target,
            duration_ticks,
            curve,
            service: FadeService::new(OverlapPolicy::Replace),
            started: false,
            elapsed: 0,
        }
    }

    /// 开始：同 tick 同时发起淡出与淡入（触发同步）。
    pub fn start(&mut self, now: u64) {
        if self.started {
            return;
        }
        self.started = true;
        // 进入源先钉到 0，交叉淡入才有"从无到有"的语义。
        self.service.prime_gain(self.in_target, now, 0.0);
        self.service.request(
            FadeRequest {
                target: self.out_target,
                duration_ticks: self.duration_ticks,
                curve: self.curve,
                target_gain: 0.0,
                scene: Scene::SceneSwitch,
            },
            now,
        );
        self.service.request(
            FadeRequest {
                target: self.in_target,
                duration_ticks: self.duration_ticks,
                curve: self.curve,
                target_gain: 1.0,
                scene: Scene::SceneSwitch,
            },
            now,
        );
        self.service.events.push(format!(
            "交叉淡化编排开始：{} → {}（{}，{} tick，同 tick 同步触发）",
            self.out_target, self.in_target, self.curve.label(), self.duration_ticks
        ));
    }

    /// 推进：编排器与服务同步走。
    pub fn tick(&mut self, now: u64) {
        if !self.started || self.elapsed >= self.duration_ticks {
            return;
        }
        self.elapsed += 1;
        self.service.tick(now);
    }

    /// 是否完成。
    pub fn done(&self) -> bool {
        self.started && self.elapsed >= self.duration_ticks
    }
}

// ---------------------------------------------------------------------------
// 五、场景统一 lint（服务外 setVolume 拦截）
// ---------------------------------------------------------------------------

/// lint：直接 setVolume 调用点 = 绕过 fade 服务 = 抖动源。
#[derive(Debug, Default)]
pub struct VolumeLint {
    pub violations: Vec<(u32, String)>,
}

impl VolumeLint {
    pub fn new() -> VolumeLint {
        VolumeLint::default()
    }

    /// 检查一批音量写：只有标经 fade 服务的合法。
    pub fn lint(&mut self, calls: &[(u32, bool, Scene)]) -> usize {
        self.violations.clear();
        for (target, via_service, scene) in calls.iter() {
            if !via_service {
                self.violations.push((
                    *target,
                    format!(
                        "目标 {} 绕过 fade 服务直写音量（场景 {}）——散落的 setVolume 是抖动源",
                        target, scene.label()
                    ),
                ));
            }
        }
        self.violations.len()
    }
}
