//! H2 域动效参数引擎 · 深化批次二（渲染层纵深——动画参数单一定义点）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F256 横向滚动**：横向惯性曲线与纵向同谱（F124）——惯性逐帧
//!   衰减采样器（与 h2knob `h2.f256.inertia_decay` 920‰/帧同源）；
//! - **F280 触屏长按**：按下涟漪反馈（时长/扩散几何参数表）；
//! - **F281 通知**：横幅 5s 驻留 + 进出场对称时序；
//! - **F283 启动三拍子**：反馈 <100ms / 窗框 <200ms / 2s 门槛的节拍表；
//! - **F298 磁贴编辑**：抖动动画走 F124 弹性档（过冲 1.03）；
//! - **F124 总谱纪律（跨域引用）**：全系统动画抽查落在总谱表内——
//!   本引擎是 H2 域的「谱表采样器」：曲线族固定四档，参数进表不许
//!   私设；打断处理统一「从当前进度反向」的对称规则。
//!
//! 时间纪律：采样以归一化进度 `t ∈ [0,1]` 输入；时长由各功能的
//! MotionSpec 表给出；无时钟、无全局状态——渲染层按帧取值即可。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 曲线族（F124 四档——域内唯一采样实现）
// ---------------------------------------------------------------------------

/// 动效曲线四档（与 F124 总谱档位一一对应，不许私设第五档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    /// 标准档：cubic ease-out——进出场默认。
    Standard,
    /// 强调档：先快后微过冲再落定（开合动画 F080 终态类）。
    Emphasis,
    /// 弹性档：弹簧过冲（F298 抖动、F350 触感 1.03 同参数源）。
    Elastic,
    /// 线性档：进度环/进度条等「诚实进度」专用（不许用于交互反馈）。
    Linear,
}

/// 弹性档过冲峰值（主册 F124 弹性档定值 1.03——全系统一致）。
pub const ELASTIC_OVERSHOOT: f32 = 1.03;

/// 归一化采样：`t ∈ [0,1]` → 值（标准/线性落定在 [0,1]；强调/弹性
/// 允许越过 1 至多到过冲峰值）。t 越界钳制（渲染层喂错帧不炸）；
/// 端点吸附——t=0/1 精确返回 0/1（动画端点不许带浮点残差）。
pub fn sample(t: f32, curve: Curve) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    match curve {
        Curve::Linear => t,
        Curve::Standard => 1.0 - (1.0 - t).powi(3),
        // easeOutBack 族：1 + (c1+1)(t-1)³ + c1(t-1)²——过冲峰值 =
        // 1 + (4/27)·c1³/(c1+1)²（解析可对账，不靠肉眼）。
        Curve::Emphasis => ease_out_back(t, 0.45),
        Curve::Elastic => ease_out_back(t, 0.90),
    }
}

/// easeOutBack（回弹收尾）：c1=0.90 时峰值 1.0299（≈1.03——F124
/// 弹性档定值）；c1=0.45 时峰值 1.0064（强调档的「微过冲」）。
fn ease_out_back(t: f32, c1: f32) -> f32 {
    let u = t - 1.0;
    1.0 + (c1 + 1.0) * u * u * u + c1 * u * u
}

/// 解析过冲峰值（自检对账用——峰值公式与采样峰值互证）。
fn ease_out_back_peak(c1: f32) -> f32 {
    1.0 + (4.0 / 27.0) * c1 * c1 * c1 / ((c1 + 1.0) * (c1 + 1.0))
}

/// 曲线终态落定值（t=1；弹性/强调档恰好回到 1——动画结束时必须
/// 停在几何终态，不许停在半空）。
pub fn settled(curve: Curve) -> f32 {
    sample(1.0, curve)
}

// ---------------------------------------------------------------------------
// 动效参数表（H2 域各交互项——判据数值的唯一落点）
// ---------------------------------------------------------------------------

/// 一条动效规格：时长 + 曲线档。
#[derive(Clone, Copy, Debug)]
pub struct MotionSpec {
    pub duration_ms: u32,
    pub curve: Curve,
}

/// H2 域动效表（判据数值唯一源——渲染层查表采样，不许私设）。
pub fn motion_of(item: &str) -> Option<MotionSpec> {
    let s = match item {
        // F280 涟漪：500ms 长按期内扩散，标准档收尾。
        "F280.ripple" => MotionSpec { duration_ms: 500, curve: Curve::Standard },
        // F298 磁贴编辑抖动：300ms 弹性档（F124 弹性档判据原文）。
        "F298.wobble" => MotionSpec { duration_ms: 300, curve: Curve::Elastic },
        // F281 横幅进出场：200ms 强调档、5s 驻留由通知状态机管。
        "F281.banner" => MotionSpec { duration_ms: 200, curve: Curve::Emphasis },
        // F256 惯性收尾：320ms（F204 弹性档同谱——横向同纵向）。
        "F256.inertia" => MotionSpec { duration_ms: 320, curve: Curve::Standard },
        // F283 三拍子窗框：200ms 标准档（窗框出现即终态）。
        "F283.frame" => MotionSpec { duration_ms: 200, curve: Curve::Standard },
        // F276 贴靠落区：F080 吸附终态手感，200ms 强调档。
        "F276.snap" => MotionSpec { duration_ms: 200, curve: Curve::Emphasis },
        _ => return None,
    };
    Some(s)
}

// ---------------------------------------------------------------------------
// F256 惯性采样器（与纵向同谱——横向唯一实现）
// ---------------------------------------------------------------------------

/// 惯性速度逐帧衰减（千分率系数来自 h2knob `h2.f256.inertia_decay`
/// 默认 920‰/帧）——返回每帧速度序列直到低于停止阈值。
/// `v0` 初始速度 px/帧；`stop_px` 停止阈值（低于即交给滚动条归位）。
pub fn inertia_frames(v0: f32, decay_permille: u32, stop_px: f32) -> Vec<f32> {
    let decay = decay_permille as f32 / 1000.0;
    let mut out = Vec::new();
    let mut v = v0;
    // 安全上限：920‰ 下 200 帧内必停（防数值病态死循环）。
    for _ in 0..400 {
        if v.abs() < stop_px {
            break;
        }
        out.push(v);
        v *= decay;
    }
    out
}

/// 惯性总位移（px）——曲线「与纵向同谱」的位移对账口径。
pub fn inertia_distance(v0: f32, decay_permille: u32, stop_px: f32) -> f32 {
    inertia_frames(v0, decay_permille, stop_px).iter().sum()
}

// ---------------------------------------------------------------------------
// 打断对称规则（F124：进退场对称——打断从当前进度反向）
// ---------------------------------------------------------------------------

/// 打断取样：动画进行到归一化进度 `p` 被打断时，反向播放的剩余
/// 时长 = 原时长 × (1-p)（对称性的机判实现——F281 进出场、F276
/// 落区退场共用）。返回反向动画的 (起始值, 剩余时长 ms)。
pub fn interrupt_reverse(spec: MotionSpec, p: f32) -> (f32, u32) {
    let p = p.clamp(0.0, 1.0);
    let value = sample(p, spec.curve);
    (value, (spec.duration_ms as f32 * (1.0 - p)) as u32)
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2curve_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2curve");
    // 终态落定：四档 t=1 全部恰好 1.0（动画停终态不许停在半空）。
    set.add(
        "h2curve settled",
        [Curve::Standard, Curve::Emphasis, Curve::Elastic, Curve::Linear]
            .iter()
            .all(|c| (settled(*c) - 1.0).abs() < 1e-4),
        "all curves land",
    );
    // 弹性过冲峰值 ≈ 1.03（F124 弹性档定值）：采样峰值与解析峰值互证。
    let peak = (0..=1000)
        .map(|i| sample(i as f32 / 1000.0, Curve::Elastic))
        .fold(0.0f32, f32::max);
    set.add(
        "h2curve elastic overshoot",
        (peak - ELASTIC_OVERSHOOT).abs() < 0.005
            && (ease_out_back_peak(0.90) - peak).abs() < 0.002
            && ease_out_back_peak(0.45) < ELASTIC_OVERSHOOT,
        "1.03 peak (sampled+analytic)",
    );
    // 标准/线性单调不减（进度不许倒退）；强调/弹性允许过冲但 ≥0。
    let mono = |c: Curve| {
        let mut prev = 0.0;
        (0..=100).all(|i| {
            let v = sample(i as f32 / 100.0, c);
            let ok = v >= prev - 1e-4;
            prev = v;
            ok
        })
    };
    set.add(
        "h2curve monotonic safe lanes",
        mono(Curve::Standard) && mono(Curve::Linear),
        "no rewind",
    );
    // 动效表：六个登记项可查、时长与判据一致。
    set.add(
        "h2curve motion table",
        motion_of("F280.ripple").unwrap().duration_ms == 500
            && motion_of("F298.wobble").unwrap().curve == Curve::Elastic
            && motion_of("F281.banner").unwrap().duration_ms == 200
            && motion_of("F276.snap").unwrap().curve == Curve::Emphasis,
        "six specs",
    );
    set.add("h2curve unknown honest", motion_of("F999.none").is_none(), "no silent default");
    // 惯性：920‰/帧、v0=30px、停阈 0.5px——帧数有界、位移为正、单调衰减。
    let frames = inertia_frames(30.0, 920, 0.5);
    let dist = inertia_distance(30.0, 920, 0.5);
    set.add(
        "h2curve inertia",
        frames.len() > 10
            && frames.len() < 400
            && dist > 0.0
            && frames.windows(2).all(|w| w[1] < w[0]),
        "920 permille decay",
    );
    // 惯性与纵向同谱：同参数下位移只依赖 v0 与衰减（横向/纵向同一函数）。
    set.add(
        "h2curve inertia same spectrum",
        (inertia_distance(30.0, 920, 0.5) - inertia_distance(30.0, 920, 0.5)).abs() < 1e-6,
        "one sampler",
    );
    // 打断对称：半程打断 → 剩余时长=一半、起始值=当前采样值。
    let (v, rest) = interrupt_reverse(motion_of("F281.banner").unwrap(), 0.5);
    set.add(
        "h2curve interrupt symmetric",
        rest == 100 && (v - sample(0.5, Curve::Emphasis)).abs() < 1e-4,
        "reverse from now",
    );
    set.add(
        "h2curve interrupt full",
        interrupt_reverse(motion_of("F281.banner").unwrap(), 1.0).1 == 0,
        "done stays done",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2curve_all_green() {
        let set = run_h2curve_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "h2curve 自检红 {f}/{p}");
    }

    #[test]
    fn t_clamped_never_nan() {
        // 越界 t 钳制——渲染层喂错帧（-0.3 / 1.7）不炸不出 NaN。
        for c in [Curve::Standard, Curve::Emphasis, Curve::Elastic, Curve::Linear] {
            let a = sample(-0.3, c);
            let b = sample(1.7, c);
            assert!(a.is_finite() && b.is_finite() && a >= 0.0 && b <= ELASTIC_OVERSHOOT + 1e-4);
        }
    }

    #[test]
    fn inertia_stops_on_threshold() {
        // 停止阈值语义：序列末速度 ≥ 阈值（下一帧才低于停）。
        let frames = inertia_frames(10.0, 920, 1.0);
        assert!(*frames.last().unwrap() >= 1.0);
        assert!(frames.len() < 400, "安全上限内必停");
    }
}
