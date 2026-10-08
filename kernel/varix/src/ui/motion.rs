//! TRINITY-500 · AI-08 动效与视觉令牌域（F176~F179 / F183~F185 / F190~F200，W2）
//!
//! 全局唯一的动效令牌与视觉特效管线：时长、弹簧、场景过渡、七层启动光效、
//! 粒子、CRT/颗粒/暗角、reduce-motion 门禁、视觉诊断与降级链。
//! 材质与色彩令牌见 [`crate::ui::tokens`]。

use crate::checks::CheckSet;
use crate::gfx::surface::MotionPref;

// ---------------------------------------------------------------------------
// F176 动效时长令牌 — 全局统一变量
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duration {
    /// 0ms：立即（仅用于状态切换，不用于位移）
    Instant,
    Fast,
    Normal,
    Slow,
    Ceremony,
}

impl Duration {
    pub fn ms(self) -> u32 {
        match self {
            Duration::Instant => 0,
            Duration::Fast => 120,
            Duration::Normal => 200,
            Duration::Slow => 320,
            Duration::Ceremony => 600,
        }
    }

    /// 令牌必须被遵守：reduce-motion 下除 Instant 外全部归零。
    pub fn effective_ms(self, pref: MotionPref) -> u32 {
        match pref {
            MotionPref::Reduced => 0,
            MotionPref::Full => self.ms(),
        }
    }
}

/// 全仓只有这 5 个时长档——新增动效必须选档，不得自造数字。
pub const DURATION_TOKENS: [u32; 5] = [0, 120, 200, 320, 600];

pub fn is_duration_token(ms: u32) -> bool {
    DURATION_TOKENS.contains(&ms)
}

// ---------------------------------------------------------------------------
// F177 弹簧物理统一曲线
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct SpringSpec {
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
}

impl SpringSpec {
    /// 柔和：位移大、回弹弱（面板、弹窗）。
    pub const SOFT: SpringSpec = SpringSpec { stiffness: 140.0, damping: 20.0, mass: 1.0 };
    /// 干脆：快速到位、几乎不回弹（按钮、开关）。
    pub const SNAPPY: SpringSpec = SpringSpec { stiffness: 320.0, damping: 36.0, mass: 1.0 };
    /// 弹跳：明显回弹（拖拽回位、吸附）。
    pub const BOUNCE: SpringSpec = SpringSpec { stiffness: 220.0, damping: 14.0, mass: 1.0 };

    /// 阻尼比：>=1 过阻尼（不回弹），<1 欠阻尼（有回弹）。
    pub fn damping_ratio(&self) -> f32 {
        self.damping / (2.0 * sqrt_approx(self.stiffness * self.mass))
    }

    /// 弹簧是否振荡（欠阻尼）。
    pub fn oscillates(&self) -> bool {
        self.damping_ratio() < 1.0
    }
}

/// 牛顿迭代开方（no_std，无浮点方法依赖）。
pub fn sqrt_approx(v: f32) -> f32 {
    if v <= 0.0 {
        return 0.0;
    }
    let mut x = v;
    let mut i = 0;
    while i < 16 {
        let nx = (x + v / x) / 2.0;
        if (nx - x).abs() < 0.0001 || (nx - x) > -0.0001 && (nx - x) < 0.0001 {
            x = nx;
            break;
        }
        x = nx;
        i += 1;
    }
    x
}

/// 全仓统一弹簧曲线：任何位移都必须从这里取参数。
pub fn spring_for(kind: SpringKind) -> SpringSpec {
    match kind {
        SpringKind::Soft => SpringSpec::SOFT,
        SpringKind::Snappy => SpringSpec::SNAPPY,
        SpringKind::Bounce => SpringSpec::BOUNCE,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpringKind {
    Soft,
    Snappy,
    Bounce,
}

// ---------------------------------------------------------------------------
// F178 场景过渡编排
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionKind {
    Fade,
    Slide,
    Zoom,
    Morph,
}

impl TransitionKind {
    pub fn duration(self) -> Duration {
        match self {
            TransitionKind::Fade => Duration::Fast,
            TransitionKind::Slide => Duration::Normal,
            TransitionKind::Zoom => Duration::Normal,
            TransitionKind::Morph => Duration::Slow,
        }
    }

    /// 是否同时移动两个场景（Morph 需要双方都可见）。
    pub fn needs_both_layers(self) -> bool {
        matches!(self, TransitionKind::Morph | TransitionKind::Zoom)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SceneTransition {
    pub kind: TransitionKind,
    pub progress_permille: u16,
}

impl SceneTransition {
    /// 出场层与入场层的 alpha（permille）与位移（px，按 Slide 方向）。
    pub fn layers(&self) -> ((u16, i32), (u16, i32)) {
        let t = self.progress_permille.min(1000);
        let ti = t as i32;
        match self.kind {
            TransitionKind::Fade => ((1000 - t, 0), (t, 0)),
            TransitionKind::Slide => ((1000, -ti * 24 / 1000), (1000, 24 - ti * 24 / 1000)),
            TransitionKind::Zoom => ((1000 - t, 0), (t, 0)),
            TransitionKind::Morph => ((1000 - t, 0), (t, 0)),
        }
    }
}

// ---------------------------------------------------------------------------
// F179 环境辉光
// ---------------------------------------------------------------------------

/// 环境辉光：把强调色以极低透明度铺在背景上，强度 permille。
pub fn ambient_glow(base: u32, accent: u32, intensity_permille: u16) -> u32 {
    let i = intensity_permille.min(300) as u32;
    crate::gfx::surface::blend(accent, base, ((i * 255) / 1000) as u8)
}

// ---------------------------------------------------------------------------
// F183 reduce-motion 门禁
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Parallax,
    Particles,
    Blur,
    Scanlines,
    Grain,
    BootCeremony,
}

impl Effect {
    pub fn name(self) -> &'static str {
        match self {
            Effect::Parallax => "parallax",
            Effect::Particles => "particles",
            Effect::Blur => "blur",
            Effect::Scanlines => "scanlines",
            Effect::Grain => "grain",
            Effect::BootCeremony => "boot-ceremony",
        }
    }

    /// reduce-motion 下被彻底关闭的特效（不是"变淡"，是"不发生"）。
    pub fn disabled_when_reduced(self) -> bool {
        matches!(self, Effect::Parallax | Effect::Particles | Effect::BootCeremony)
    }
}

pub fn motion_gate(pref: MotionPref, effect: Effect) -> bool {
    match pref {
        MotionPref::Full => true,
        MotionPref::Reduced => !effect.disabled_when_reduced(),
    }
}

/// 门禁：一批特效里若有任何一项违反 reduce-motion，整体判 FAIL。
pub fn motion_gate_all(pref: MotionPref, effects: &[Effect]) -> bool {
    effects.iter().all(|e| motion_gate(pref, *e))
}

// ---------------------------------------------------------------------------
// F184 视觉性能预算
// ---------------------------------------------------------------------------

pub const VISUAL_BUDGET_US: u32 = 6_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisualCost {
    pub blur_us: u32,
    pub particles_us: u32,
    pub post_us: u32,
}

impl VisualCost {
    pub fn total(&self) -> u32 {
        self.blur_us + self.particles_us + self.post_us
    }

    pub fn within_budget(&self) -> bool {
        self.total() <= VISUAL_BUDGET_US
    }
}

// ---------------------------------------------------------------------------
// F190 七层启动光效渲染
// ---------------------------------------------------------------------------

pub const BOOT_LAYER_COUNT: usize = 7;
pub const BOOT_CEREMONY_MS: u32 = 1_800;

#[derive(Clone, Copy, Debug)]
pub struct LightLayerSpec {
    pub name: &'static str,
    /// 起始时间（permille of ceremony）
    pub start_permille: u16,
    pub end_permille: u16,
    pub peak_alpha: u16,
}

/// 七层启动光效：每一层的出现窗口与峰值强度（顺序即渲染顺序）。
pub const BOOT_LAYERS: [LightLayerSpec; BOOT_LAYER_COUNT] = [
    LightLayerSpec { name: "void", start_permille: 0, end_permille: 1000, peak_alpha: 1000 },
    LightLayerSpec { name: "ember", start_permille: 60, end_permille: 420, peak_alpha: 320 },
    LightLayerSpec { name: "horizon", start_permille: 180, end_permille: 620, peak_alpha: 420 },
    LightLayerSpec { name: "arc", start_permille: 320, end_permille: 760, peak_alpha: 520 },
    LightLayerSpec { name: "bloom", start_permille: 480, end_permille: 880, peak_alpha: 640 },
    LightLayerSpec { name: "grid", start_permille: 620, end_permille: 960, peak_alpha: 380 },
    LightLayerSpec { name: "mark", start_permille: 780, end_permille: 1000, peak_alpha: 1000 },
];

/// 第 `layer` 层在 `elapsed_ms` 时的 alpha（permille）：梯形包络。
pub fn boot_layer_alpha(layer: usize, elapsed_ms: u32, ceremony_ms: u32) -> u16 {
    let spec = match BOOT_LAYERS.get(layer) {
        Some(s) => *s,
        None => return 0,
    };
    let total = if ceremony_ms == 0 { BOOT_CEREMONY_MS } else { ceremony_ms };
    let t = ((elapsed_ms.min(total) as u64 * 1000) / total as u64) as u16;
    if t < spec.start_permille || t > spec.end_permille {
        return 0;
    }
    let span = spec.end_permille - spec.start_permille;
    let local = t - spec.start_permille;
    // 前 30% 淡入，后 30% 淡出，中间满值
    let fade = span / 3;
    if fade == 0 {
        return spec.peak_alpha;
    }
    if local < fade {
        (spec.peak_alpha as u32 * local as u32 / fade as u32) as u16
    } else if local > span - fade {
        (spec.peak_alpha as u32 * (span - local) as u32 / fade as u32) as u16
    } else {
        spec.peak_alpha
    }
}

// ---------------------------------------------------------------------------
// F191 粒子场
// ---------------------------------------------------------------------------

/// 粒子数按面积线性缩放，密度令牌 permille（0 = 关闭）。
pub fn particle_count(area_px: i64, density_permille: u16) -> u32 {
    if density_permille == 0 {
        return 0;
    }
    // 先除后乘会整段归零（密度 < 16 permille 时），这里保证低密度也算得出粒子。
    let n = (area_px / 1000) * density_permille as i64 * 64 / 1_000_000;
    n.clamp(0, crate::gfx::surface::MAX_PARTICLES as i64) as u32
}

// ---------------------------------------------------------------------------
// F192 CRT 扫描线
// F193 胶片颗粒
// F194 暗角
// F195 地平线暖光
// ---------------------------------------------------------------------------

/// 扫描线：偶数行压暗，强度 permille。
pub fn scanline_alpha(y: i32, period: i32, strength_permille: u16) -> u16 {
    if period <= 0 {
        return 0;
    }
    if (y % period) < period / 2 {
        strength_permille.min(300)
    } else {
        0
    }
}

/// 胶片颗粒：位置 + 帧号决定的确定性噪声（同帧同位置必相同，不闪烁）。
pub fn grain_offset(x: i32, y: i32, frame: u32, strength: u8) -> u8 {
    let mut h = (x as u32 ^ (y as u32).rotate_left(7) ^ frame.wrapping_mul(0x9E37_79B9)) as u32;
    h ^= h >> 15;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    ((h & 0xFF) as u32 * strength as u32 / 255) as u8
}

/// 暗角：中心到边缘的距离（permille）越强越暗。
pub fn vignette_alpha(distance_permille: u16, strength_permille: u16) -> u16 {
    let d = distance_permille.min(1000) as u32;
    let s = strength_permille.min(600) as u32;
    ((d * d / 1000) * s / 1000) as u16
}

/// 地平线暖光：屏幕底部 `band_permille` 高度内向上衰减。
pub fn horizon_glow(y_permille: u16, band_permille: u16, strength_permille: u16) -> u16 {
    let y = y_permille.min(1000) as u32;
    let band = band_permille.max(1).min(1000) as u32;
    if y < 1000 - band {
        return 0;
    }
    let t = (y - (1000 - band)) * 1000 / band;
    (strength_permille.min(1000) as u32 * t / 1000) as u16
}

// ---------------------------------------------------------------------------
// F196 视觉一致性终检
// F197 视觉诊断器
// F198 视觉降级链
// F199 视觉收藏夹（A/B 方案）
// ---------------------------------------------------------------------------

pub const MAX_VISUAL_ISSUES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualIssue {
    NonTokenDuration,
    CustomSpring,
    ReducedMotionViolation,
    IconOffPipeline,
    EffectOverBudget,
    BareColour,
}

impl VisualIssue {
    pub fn text(self) -> &'static str {
        match self {
            VisualIssue::NonTokenDuration => "animation duration is not a token",
            VisualIssue::CustomSpring => "spring parameters are not from the token set",
            VisualIssue::ReducedMotionViolation => "effect runs under reduce-motion",
            VisualIssue::IconOffPipeline => "icon size is not produced by the 128px pipeline",
            VisualIssue::EffectOverBudget => "visual effects exceed the 6ms budget",
            VisualIssue::BareColour => "bare colour literal outside the token set",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VisualDiag {
    issues: [Option<VisualIssue>; MAX_VISUAL_ISSUES],
    count: usize,
}

impl VisualDiag {
    pub const fn new() -> VisualDiag {
        VisualDiag { issues: [None; MAX_VISUAL_ISSUES], count: 0 }
    }

    fn push(&mut self, i: VisualIssue) {
        if self.count >= MAX_VISUAL_ISSUES || (0..self.count).any(|k| self.issues[k] == Some(i)) {
            return;
        }
        self.issues[self.count] = Some(i);
        self.count += 1;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn has(&self, i: VisualIssue) -> bool {
        (0..self.count).any(|k| self.issues[k] == Some(i))
    }

    /// F196：一致性终检——时长、弹簧、图标、色值、预算、reduce-motion 六关。
    pub fn inspect(
        &mut self,
        durations: &[u32],
        springs: &[SpringSpec],
        icon_sizes: &[u32],
        bare_colours: usize,
        cost: &VisualCost,
        pref: MotionPref,
        effects: &[Effect],
    ) {
        if durations.iter().any(|d| !is_duration_token(*d)) {
            self.push(VisualIssue::NonTokenDuration);
        }
        if springs.iter().any(|s| {
            !same_spring(s, &SpringSpec::SOFT) && !same_spring(s, &SpringSpec::SNAPPY) && !same_spring(s, &SpringSpec::BOUNCE)
        }) {
            self.push(VisualIssue::CustomSpring);
        }
        let pipeline = {
            let mut stages = [crate::ui::tokens::IconStage { size: 0, upscale: false }; 8];
            let n = crate::ui::tokens::icon_pipeline(&mut stages);
            icon_sizes.iter().any(|s| !(0..n).any(|i| stages[i].size == *s))
        };
        if pipeline {
            self.push(VisualIssue::IconOffPipeline);
        }
        if bare_colours > 0 {
            self.push(VisualIssue::BareColour);
        }
        if !cost.within_budget() {
            self.push(VisualIssue::EffectOverBudget);
        }
        if !motion_gate_all(pref, effects) {
            self.push(VisualIssue::ReducedMotionViolation);
        }
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(v) = self.issues[i] {
                for &b in v.text().as_bytes() {
                    if n < out.len() {
                        out[n] = b;
                        n += 1;
                    }
                }
                if n < out.len() {
                    out[n] = b'\n';
                    n += 1;
                }
            }
        }
        n
    }
}

impl Default for VisualDiag {
    fn default() -> Self {
        VisualDiag::new()
    }
}

fn same_spring(a: &SpringSpec, b: &SpringSpec) -> bool {
    (a.stiffness - b.stiffness).abs() < 0.001
        && (a.damping - b.damping).abs() < 0.001
        && (a.mass - b.mass).abs() < 0.001
}

/// F198 视觉降级链：无 GPU 时按序关闭最贵的特效，直到回到预算内。
pub fn degrade_chain(cost: &VisualCost, has_gpu: bool) -> &'static str {
    if has_gpu {
        return "full";
    }
    if cost.blur_us > 3_000 {
        return "disable blur";
    }
    if cost.particles_us > 1_000 {
        return "disable particles";
    }
    if cost.post_us > 800 {
        return "disable post-processing";
    }
    "software compositing"
}

/// F199 视觉收藏夹：A/B 两套方案，用户可选；切换即改令牌，不改代码路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    A,
    B,
}

pub fn variant_tokens(v: Variant) -> (u16, u16, u16) {
    match v {
        // (微粒密度‰, 暗角强度‰, 环境辉光‰)
        Variant::A => (12, 220, 90),
        Variant::B => (30, 340, 160),
    }
}

// ---------------------------------------------------------------------------
// F185 / F200 视觉域自检与收口
// ---------------------------------------------------------------------------

/// AI-08 域自检：F176~F200 逐项登记。
pub fn run_motion_checks() -> CheckSet {
    let mut set = CheckSet::new("motion");

    set.add(
        "F176 duration tokens",
        DURATION_TOKENS.len() == 5
            && is_duration_token(200)
            && !is_duration_token(201)
            && Duration::Ceremony.effective_ms(MotionPref::Reduced) == 0,
        "five canonical durations",
    );

    set.add(
        "F177 unified spring",
        SpringSpec::SNAPPY.damping_ratio() > SpringSpec::BOUNCE.damping_ratio()
            && SpringSpec::BOUNCE.oscillates()
            && !SpringSpec::SNAPPY.oscillates()
            && spring_for(SpringKind::Soft).stiffness == 140.0,
        "token springs only",
    );

    let tr = SceneTransition { kind: TransitionKind::Slide, progress_permille: 500 };
    let ((_, out_b), (in_a, _)) = (tr.layers().0, tr.layers().1);
    set.add(
        "F178 scene transitions",
        TransitionKind::Morph.needs_both_layers() && TransitionKind::Fade.duration().ms() == 120 && out_b < 0 && in_a == 1000,
        "layer orchestration",
    );

    set.add(
        "F179 ambient glow",
        ambient_glow(0xFF00_0000, 0xFF_FF_FF_FF, 1000) != 0xFF00_0000,
        "accent wash",
    );

    set.add(
        "F180 materials",
        crate::ui::tokens::Material::Glass.params().blur_radius == 24,
        "material params",
    );

    set.add(
        "F181 theme follows wallpaper",
        crate::ui::tokens::theme_from_wallpaper(700, None) == crate::ui::tokens::ThemeMode::Light,
        "luma threshold",
    );

    set.add(
        "F182 day/night kelvin",
        crate::ui::tokens::kelvin_at_hour(12) == crate::ui::tokens::KELVIN_MAX
            && crate::ui::tokens::kelvin_at_hour(0) == crate::ui::tokens::KELVIN_MIN,
        "colour temperature curve",
    );

    set.add(
        "F183 reduce-motion gate",
        !motion_gate(MotionPref::Reduced, Effect::Particles)
            && motion_gate(MotionPref::Reduced, Effect::Blur)
            && !motion_gate_all(MotionPref::Reduced, &[Effect::Parallax, Effect::Blur]),
        "hard off, not fade",
    );

    let ok_cost = VisualCost { blur_us: 1_000, particles_us: 500, post_us: 300 };
    let bad_cost = VisualCost { blur_us: 5_000, particles_us: 2_000, post_us: 1_000 };
    set.add(
        "F184 visual perf budget",
        ok_cost.within_budget() && !bad_cost.within_budget() && VISUAL_BUDGET_US == 6_000,
        "6ms for effects",
    );

    set.add("F185 visual self-check", set.all_passed(), "entry point");

    set.add(
        "F186 bare colour gate",
        crate::ui::tokens::css_bare_colors(&["color: #aabbcc;"]) == 1
            && crate::ui::tokens::rust_bare_colors(&[0xAABBCC], "let x = 0xAABBCC;") == 0,
        "token-enforced colours",
    );

    let mut stages = [crate::ui::tokens::IconStage { size: 0, upscale: false }; 8];
    let n = crate::ui::tokens::icon_pipeline(&mut stages);
    set.add(
        "F187 128px icon pipeline",
        n == 7 && stages[5].size == 128 && !stages[5].upscale && stages[6].upscale,
        "all sizes derive from master",
    );

    set.add(
        "F188 watermark",
        crate::ui::tokens::brand_layers_disjoint((1920, 1080), 24, 200),
        "watermark never covers content",
    );

    set.add(
        "F189 logo placement",
        crate::ui::tokens::logo_rect((1920, 1080), 200).2 == 216,
        "boot logo keeps its slot",
    );

    set.add(
        "F190 seven-layer boot light",
        BOOT_LAYERS.len() == 7
            && boot_layer_alpha(0, 0, BOOT_CEREMONY_MS) == 0
            && boot_layer_alpha(0, 500, BOOT_CEREMONY_MS) > 0
            && boot_layer_alpha(6, 0, BOOT_CEREMONY_MS) == 0
            && boot_layer_alpha(6, BOOT_CEREMONY_MS * 850 / 1000, BOOT_CEREMONY_MS) > 0,
        "staged envelope",
    );

    set.add(
        "F191 particle field",
        particle_count(2_000_000, 12) > 0 && particle_count(2_000_000, 0) == 0 && particle_count(1 << 30, 12) <= 64,
        "density-scaled, bounded",
    );

    set.add(
        "F192 crt scanlines",
        scanline_alpha(0, 4, 120) == 120 && scanline_alpha(2, 4, 120) == 0,
        "alternating rows",
    );

    set.add(
        "F193 film grain",
        grain_offset(1, 2, 3, 255) == grain_offset(1, 2, 3, 255) && grain_offset(1, 2, 4, 255) != grain_offset(1, 2, 3, 255),
        "deterministic per frame",
    );

    set.add(
        "F194 vignette",
        vignette_alpha(0, 300) == 0 && vignette_alpha(1000, 300) == 300 && vignette_alpha(500, 300) < 300,
        "quadratic falloff",
    );

    set.add(
        "F195 horizon glow",
        horizon_glow(100, 200, 500) == 0 && horizon_glow(1000, 200, 500) == 500,
        "bottom band only",
    );

    let mut diag = VisualDiag::new();
    diag.inspect(
        &[120, 200],
        &[SpringSpec::SNAPPY],
        &[16, 128],
        0,
        &ok_cost,
        MotionPref::Full,
        &[Effect::Blur],
    );
    set.add("F196 consistency final", diag.len() == 0, "clean pipeline reports nothing");

    let mut bad = VisualDiag::new();
    bad.inspect(&[123], &[SpringSpec::SOFT], &[40], 2, &bad_cost, MotionPref::Reduced, &[Effect::Particles]);
    set.add(
        "F197 visual diagnostics",
        bad.has(VisualIssue::NonTokenDuration)
            && bad.has(VisualIssue::IconOffPipeline)
            && bad.has(VisualIssue::BareColour)
            && bad.has(VisualIssue::EffectOverBudget)
            && bad.has(VisualIssue::ReducedMotionViolation),
        "collects every violation",
    );

    set.add(
        "F198 visual degrade chain",
        degrade_chain(&bad_cost, true) == "full" && degrade_chain(&bad_cost, false) == "disable blur",
        "cheapest-first shutdown",
    );

    set.add(
        "F199 visual variants",
        variant_tokens(Variant::A) != variant_tokens(Variant::B),
        "A/B presets",
    );

    set.add("F200 motion domain closure", set.all_passed(), "all above green");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f176_no_custom_durations() {
        for d in [Duration::Instant, Duration::Fast, Duration::Normal, Duration::Slow, Duration::Ceremony] {
            assert!(is_duration_token(d.ms()));
        }
        assert_eq!(Duration::Fast.effective_ms(MotionPref::Reduced), 0);
    }

    #[test]
    fn f177_spring_math() {
        assert!((sqrt_approx(16.0) - 4.0).abs() < 0.001);
        assert!((sqrt_approx(2.0) - 1.414).abs() < 0.01);
        assert_eq!(sqrt_approx(-1.0), 0.0);
        assert!(SpringSpec::SOFT.oscillates());
    }

    #[test]
    fn f178_transition_endpoints() {
        let t = SceneTransition { kind: TransitionKind::Fade, progress_permille: 0 };
        assert_eq!(t.layers().0 .0, 1000);
        assert_eq!(t.layers().1 .0, 0);
        let done = SceneTransition { kind: TransitionKind::Fade, progress_permille: 1000 };
        assert_eq!(done.layers().1 .0, 1000);
    }

    #[test]
    fn f190_boot_layers_are_ordered() {
        for i in 1..BOOT_LAYERS.len() {
            assert!(BOOT_LAYERS[i].start_permille >= BOOT_LAYERS[i - 1].start_permille);
        }
        assert_eq!(boot_layer_alpha(9, 100, 1000), 0, "out-of-range layer");
    }

    #[test]
    fn f191_particle_bounds() {
        assert_eq!(particle_count(0, 500), 0);
        assert!(particle_count(1_000_000, 1000) <= 64);
    }

    #[test]
    fn f192_scanline_period_guard() {
        assert_eq!(scanline_alpha(1, 0, 120), 0);
    }

    #[test]
    fn f197_diag_renders() {
        let mut d = VisualDiag::new();
        d.inspect(&[1], &[], &[], 0, &VisualCost { blur_us: 0, particles_us: 0, post_us: 0 }, MotionPref::Full, &[]);
        assert!(d.has(VisualIssue::NonTokenDuration));
        let mut out = [0u8; 256];
        assert!(d.render(&mut out) > 0);
    }

    #[test]
    fn f200_domain_self_test_is_green() {
        let set = run_motion_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 2048];
            let n = set.render(&mut buf);
            panic!("motion self-test://n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert_eq!(set.len(), 25);
    }
}
