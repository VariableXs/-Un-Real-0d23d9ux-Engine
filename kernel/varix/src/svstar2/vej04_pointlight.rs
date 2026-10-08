//! VE-F1804 · 点光（VE-J 域 · 光照与阴影 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1804`
//!
//! **判据（锚点原文）**：实现位置光（位置/颜色/强度/范围四参数）的物理平方
//! 反比衰减加范围钳制，提供衰减曲线双模式（物理衰减为默认+艺术曲线可选），
//! 并定义多光源管理语义（并发点光数量与性能缩放关系）。判据四条：**平方反比、
//! 范围钳制、双曲线、三档并发**。
//!
//! **错误路径与降级矩阵**：范围≤0→钳制到最小有效半径并告警；强度 NaN/负→
//! 钳制（对接 F1802 物理量纲纪律：cd 制、非负）；艺术曲线参数越界→回退物理
//! 曲线；并发超档→按距离+重要性裁剪并显性告警（不静默丢弃）；平方反比在极
//! 近距离的数值爆炸→近距离钳制保护（最小距离平方下限）。
//!
//! **设计要点**：
//! - **平方反比**：attenuation = 1/d²，d² 下限 [`MIN_DISTANCE_SQ`] 钳制
//!   （极近距离数值爆炸防护——物理正确性让位于数值稳定性，注记显性）；
//! - **范围钳制**：d ≥ range 即 0（early-out，超出范围像素跳过着色——
//!   主优化点，O(1)/像素）；范围≤0 钳到 [`MIN_RANGE_RADIUS`] 并告警；
//! - **双曲线**：[`FalloffCurve::Physical`] 默认（纯平方反比）；`Artistic`
//!   可选（窗口化衰减 `(1-d/range)^p`，指数有界）——参数越界回退物理
//!   曲线并告警（回退显性，不静默换曲线）；
//! - **三档并发**：16/32/64 光阶梯（对接 F1811/F1814 基准定标）；超档按
//!   "重要性 = 强度 / (d²+ε)" 排序裁剪，被裁数量显性入账（不静默丢弃），
//!   裁剪保序稳定（同重要性按 id 序——确定性纪律）；
//! - **参数钳制**：颜色线性域逐通道钳到 [0,1]；强度钳到非负有限（NaN/负
//!   →0 + 告警）；全部钳制动作留痕可查。
//!
//! **跨批对接点**：衰减语义与 F1827 点光阴影共享范围参数（[`PointLight::range`]
//! 为单一事实源）；管理经 F1807；裁剪策略与 F1803 排序器共用语义；fuzz 进
//! F1813；三档基准进 F1814。
//!
//! 逻辑 tick 注入，零墙钟；零外部依赖，只依赖 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 最小有效范围半径（范围≤0 的钳制落点，米）。
pub const MIN_RANGE_RADIUS: f32 = 0.05;

/// 最小距离平方下限（平方反比极近距爆炸保护，米²）。
pub const MIN_DISTANCE_SQ: f32 = 1e-4;

/// 艺术曲线指数下界。
pub const ARTISTIC_EXPONENT_MIN: f32 = 0.5;

/// 艺术曲线指数上界。
pub const ARTISTIC_EXPONENT_MAX: f32 = 4.0;

/// 并发点光三档阶梯（16/32/64——F1811/F1814 基准定标口径）。
pub const CONCURRENCY_TIERS: [usize; 3] = [16, 32, 64];

/// 点光规格注记（常量上传每帧一次 + early-out 主优化点）。
pub const POINTLIGHT_SPEC_DOC: &str = "\
点光规格（VE-F1804 · v1）：位置/线性颜色/强度(cd)/范围半径四参数；\
物理平方反比衰减（默认）+ 艺术曲线（可选）；范围钳制 early-out 是\
主优化点；着色器常量每帧上传一次；并发 16/32/64 三档阶梯对接基准。";

// ---------------------------------------------------------------------------
// 二、参数块（四参数 + 曲线枚举，构造即钳制）
// ---------------------------------------------------------------------------

/// 衰减曲线双模式。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FalloffCurve {
    /// 物理平方反比（默认）。
    Physical,
    /// 艺术曲线：attenuation = (1 - d/range)^exponent（窗口化，指数有界）。
    Artistic { /// 曲线指数（有界 [ARTISTIC_EXPONENT_MIN, MAX]）。
               exponent: f32 },
}

/// 裁剪/钳制告警记录（全部降级动作显性入账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParamWarning {
    /// 参数名。
    pub field: &'static str,
    /// 告警码。
    pub code: &'static str,
    /// 人话说明（原值 → 处置）。
    pub detail: String,
}

/// 线性颜色（线性域三分量，构造即钳制到 [0,1]）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearRgb {
    /// R（线性）。
    pub r: f32,
    /// G（线性）。
    pub g: f32,
    /// B（线性）。
    pub b: f32,
}

impl LinearRgb {
    /// 构造：非有限/越界逐通道钳制（留告警）。
    pub fn new(r: f32, g: f32, b: f32, warn: &mut Vec<ParamWarning>) -> Self {
        let base = warn.len();
        let mut clamp_ch = |v: f32, name: &'static str| -> f32 {
            if !v.is_finite() {
                warn.push(ParamWarning {
                    field: name,
                    code: "W_COLOR_NONFINITE",
                    detail: format!("颜色通道 {name} 非有限 → 钳制到 0"),
                });
                0.0
            } else if v < 0.0 {
                warn.push(ParamWarning {
                    field: name,
                    code: "W_COLOR_CLAMPED",
                    detail: format!("颜色通道 {name}={v} 为负 → 钳制到 0"),
                });
                0.0
            } else if v > 1.0 {
                warn.push(ParamWarning {
                    field: name,
                    code: "W_COLOR_CLAMPED",
                    detail: format!("颜色通道 {name}={v} 越上界 → 钳制到 1"),
                });
                1.0
            } else {
                v
            }
        };
        let out = LinearRgb { r: clamp_ch(r, "r"), g: clamp_ch(g, "g"), b: clamp_ch(b, "b") };
        let _ = base;
        out
    }
}

/// 点光参数块（位置/颜色线性/强度 cd/范围半径/曲线）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    /// 位置（场景空间）。
    pub pos: (f32, f32, f32),
    /// 颜色（线性域）。
    pub color: LinearRgb,
    /// 强度（cd 物理制，非负）。
    pub intensity_cd: f32,
    /// 范围半径（衰减钳制边界；≤0 已钳制）。
    pub range: f32,
    /// 衰减曲线（越界艺术参数已回退物理）。
    pub curve: FalloffCurve,
}

impl PointLight {
    /// 构造：全部参数当场钳制并留告警（构造即合法——使用侧零校验负担）。
    pub fn new(
        pos: (f32, f32, f32),
        color: (f32, f32, f32),
        intensity_cd: f32,
        range: f32,
        curve: FalloffCurve,
    ) -> (Self, Vec<ParamWarning>) {
        let mut warn = Vec::new();
        // 范围≤0 / 非有限 → 钳到最小有效半径（范围钳制判据）。
        let range = if !range.is_finite() || range <= 0.0 {
            warn.push(ParamWarning {
                field: "range",
                code: "W_RANGE_CLAMPED",
                detail: format!("范围 {range} 无效 → 钳制到最小有效半径 {MIN_RANGE_RADIUS}"),
            });
            MIN_RANGE_RADIUS
        } else {
            range
        };
        // 强度 NaN/负 → 0（F1802 物理量纲纪律：cd 非负有限）。
        let intensity_cd = if !intensity_cd.is_finite() || intensity_cd < 0.0 {
            warn.push(ParamWarning {
                field: "intensity_cd",
                code: "W_INTENSITY_CLAMPED",
                detail: format!("强度 {intensity_cd} 非法（NaN/负）→ 钳制到 0 cd"),
            });
            0.0
        } else {
            intensity_cd
        };
        // 艺术曲线参数越界 → 回退物理曲线（回退显性）。
        let curve = match curve {
            FalloffCurve::Artistic { exponent }
                if !exponent.is_finite()
                    || exponent < ARTISTIC_EXPONENT_MIN
                    || exponent > ARTISTIC_EXPONENT_MAX =>
            {
                warn.push(ParamWarning {
                    field: "curve",
                    code: "W_CURVE_FALLBACK",
                    detail: format!(
                        "艺术曲线指数 {exponent} 越界 [{ARTISTIC_EXPONENT_MIN},{ARTISTIC_EXPONENT_MAX}] → 回退物理曲线"
                    ),
                });
                FalloffCurve::Physical
            }
            other => other,
        };
        let c = LinearRgb::new(color.0, color.1, color.2, &mut warn);
        (
            PointLight { pos, color: c, intensity_cd, range, curve },
            warn,
        )
    }
}

// ---------------------------------------------------------------------------
// 三、衰减计算（平方反比 + 范围钳制 + 双曲线，O(1)）
// ---------------------------------------------------------------------------

/// 单光源对某距离的衰减系数（O(1)；范围外为 0——early-out 判据）。
pub fn attenuation(light: &PointLight, dist: f32) -> f32 {
    curve_attenuation(light.curve, light.range, dist)
}

/// 曲线×距离衰减原语（F1804 与 F1805 聚光的距离衰减共用——单一事实源）。
pub fn curve_attenuation(curve: FalloffCurve, range: f32, dist: f32) -> f32 {
    if !dist.is_finite() || dist < 0.0 || dist >= range {
        return 0.0; // 范围钳制：超出范围像素跳过着色。
    }
    match curve {
        FalloffCurve::Physical => {
            // 平方反比 + 近距离钳制（数值爆炸保护）。
            let d2 = (dist * dist).max(MIN_DISTANCE_SQ);
            1.0 / d2
        }
        FalloffCurve::Artistic { exponent } => {
            // 窗口化艺术曲线：边界内 (1-d/range)^p，p 已在构造期验证有界。
            let t = (1.0 - dist / range).clamp(0.0, 1.0);
            t.powf(exponent)
        }
    }
}

/// 对像素点的贡献 = 颜色 × 强度 × 衰减（着色入口；O(1)/像素）。
pub fn contribution(light: &PointLight, px: (f32, f32, f32)) -> (f32, f32, f32) {
    let dx = px.0 - light.pos.0;
    let dy = px.1 - light.pos.1;
    let dz = px.2 - light.pos.2;
    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
    let a = attenuation(light, dist);
    let k = a * light.intensity_cd;
    (light.color.r * k, light.color.g * k, light.color.b * k)
}

// ---------------------------------------------------------------------------
// 四、多光源管理（三档并发 + 距离+重要性裁剪）
// ---------------------------------------------------------------------------

/// 裁剪报告（并发超档→显性告警，不静默丢弃）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CutReport {
    /// 目标档位容量。
    pub tier: usize,
    /// 保留数。
    pub kept: usize,
    /// 被裁数（显性入账）。
    pub dropped: usize,
}

/// 并发超档裁剪：按"重要性 = 强度 / (d²+ε)"降序保留前 tier 个。
///
/// 稳定序：同重要性按数组原序（确定性纪律——同输入同裁剪）。
/// 被裁数量写入报告并生成告警文本（不静默丢弃判据）。
pub fn cull_to_tier(
    lights: &[PointLight],
    px: (f32, f32, f32),
    tier_index: usize,
) -> (Vec<PointLight>, CutReport, Option<String>) {
    let tier = CONCURRENCY_TIERS[tier_index.min(CONCURRENCY_TIERS.len() - 1)];
    if lights.len() <= tier {
        return (
            lights.to_vec(),
            CutReport { tier, kept: lights.len(), dropped: 0 },
            None,
        );
    }
    // 重要性 = 强度 / (d²+ε)：近光与强光优先。
    let mut ranked: Vec<(usize, f32)> = lights
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let dx = px.0 - l.pos.0;
            let dy = px.1 - l.pos.1;
            let dz = px.2 - l.pos.2;
            let d2 = (dx * dx + dy * dy + dz * dz).max(MIN_DISTANCE_SQ);
            (i, l.intensity_cd / d2)
        })
        .collect();
    // 稳定排序：f32 降序 + 原序 tiebreak（sort_by 已稳定，只需统一比较键）。
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    let mut kept: Vec<PointLight> = Vec::new();
    for &(i, _) in ranked.iter().take(tier) {
        kept.push(lights[i]);
    }
    let dropped = lights.len() - tier;
    let report = CutReport { tier, kept: tier, dropped };
    let warn = Some(format!(
        "W_CONCURRENCY_CUT：点光 {} 超出 {} 档容量，按距离+重要性裁剪 {} 盏（显性入账，不静默丢弃）",
        lights.len(),
        tier,
        dropped
    ));
    (kept, report, warn)
}

// ---------------------------------------------------------------------------
// 五、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F1804 域自检（判据逐条映射见 `vej04_checks.rs`）。
pub fn run_vej04_checks() -> CheckSet {
    super::vej04_checks::run_vej04_checks()
}

// ---------------------------------------------------------------------------
// 六、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn physical(range: f32) -> PointLight {
        PointLight::new((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 1.0, range, FalloffCurve::Physical).0
    }

    #[test]
    fn vej04_inverse_square_physical() {
        let l = physical(100.0);
        // 平方反比对拍：d=1 → 1.0；d=2 → 0.25；d=10 → 0.01。
        assert!((attenuation(&l, 1.0) - 1.0).abs() < 1e-6);
        assert!((attenuation(&l, 2.0) - 0.25).abs() < 1e-6);
        assert!((attenuation(&l, 10.0) - 0.01).abs() < 1e-5);
        // 近距离钳制：d→0 时被 MIN_DISTANCE_SQ 托底，不爆炸。
        let near = attenuation(&l, 0.0);
        assert!((near - 1.0 / MIN_DISTANCE_SQ).abs() < 1e-3, "极近距被钳制");
        assert!(near.is_finite());
    }

    #[test]
    fn vej04_range_clamp_early_out() {
        let l = physical(10.0);
        assert!(attenuation(&l, 9.999) > 0.0, "范围内正常衰减");
        assert_eq!(attenuation(&l, 10.0), 0.0, "范围边界即 0（early-out）");
        assert_eq!(attenuation(&l, 50.0), 0.0);
        assert_eq!(attenuation(&l, f32::NAN), 0.0, "非有限距离按范围外处置");
        // 范围≤0 → 钳制最小半径 + 告警。
        let (l2, warn) = PointLight::new((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 1.0, -5.0, FalloffCurve::Physical);
        assert_eq!(l2.range, MIN_RANGE_RADIUS);
        assert!(warn.iter().any(|w| w.code == "W_RANGE_CLAMPED"));
    }

    #[test]
    fn vej04_dual_curves_and_fallback() {
        // 艺术曲线：窗口化衰减，边界归零、中心为 1。
        let art = PointLight::new(
            (0.0, 0.0, 0.0),
            (1.0, 1.0, 1.0),
            1.0,
            10.0,
            FalloffCurve::Artistic { exponent: 2.0 },
        )
        .0;
        assert!((attenuation(&art, 0.0) - 1.0).abs() < 1e-6);
        assert!((attenuation(&art, 5.0) - 0.25).abs() < 1e-6, "(1-0.5)^2=0.25");
        assert_eq!(attenuation(&art, 10.0), 0.0);
        // 指数越界 → 回退物理曲线 + 告警。
        let (fb, warn) = PointLight::new(
            (0.0, 0.0, 0.0),
            (1.0, 1.0, 1.0),
            1.0,
            10.0,
            FalloffCurve::Artistic { exponent: 99.0 },
        );
        assert_eq!(fb.curve, FalloffCurve::Physical, "越界回退物理");
        assert!(warn.iter().any(|w| w.code == "W_CURVE_FALLBACK"));
    }

    #[test]
    fn vej04_param_clamping_warnings() {
        // 强度 NaN/负 → 0（F1802 量纲纪律）。
        let (l, warn) = PointLight::new((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), -3.0, 10.0, FalloffCurve::Physical);
        assert_eq!(l.intensity_cd, 0.0);
        assert!(warn.iter().any(|w| w.code == "W_INTENSITY_CLAMPED"));
        // 颜色越界逐通道钳制。
        let (l2, warn2) = PointLight::new((0.0, 0.0, 0.0), (-0.5, 2.0, f32::NAN), 1.0, 10.0, FalloffCurve::Physical);
        assert_eq!((l2.color.r, l2.color.g, l2.color.b), (0.0, 1.0, 0.0));
        assert_eq!(warn2.iter().filter(|w| w.field.starts_with('c') || w.field.len() == 1).count(), 3);
    }

    #[test]
    fn vej04_three_tier_concurrency_cull() {
        // 64 盏光、档位 16 → 保留 16 裁 48，且保留的是最近/最强的。
        let mut lights = Vec::new();
        for i in 0..64u32 {
            let dist = if i < 20 { 1.0 + i as f32 } else { 100.0 + i as f32 };
            lights.push(
                PointLight::new((dist, 0.0, 0.0), (1.0, 1.0, 1.0), 1.0, 1000.0, FalloffCurve::Physical).0,
            );
        }
        let (kept, report, warn) = cull_to_tier(&lights, (0.0, 0.0, 0.0), 0);
        assert_eq!(kept.len(), 16);
        assert_eq!(report.dropped, 48);
        assert!(warn.is_some(), "超档裁剪必须显性告警");
        // 保留的应是最近的 16 盏（重要性=强度/d²）。
        let max_kept_dist = kept.iter().map(|l| l.pos.0).fold(0.0f32, f32::max);
        assert!(max_kept_dist < 100.0, "裁剪保留近光");
        // 未超档：零裁剪零告警。
        let (kept2, report2, warn2) = cull_to_tier(&lights[..16], (0.0, 0.0, 0.0), 0);
        assert_eq!(kept2.len(), 16);
        assert_eq!(report2.dropped, 0);
        assert!(warn2.is_none());
    }

    #[test]
    fn vej04_contribution_o1() {
        let l = PointLight::new((0.0, 0.0, 0.0), (1.0, 0.5, 0.25), 4.0, 10.0, FalloffCurve::Physical).0;
        let (r, g, b) = contribution(&l, (2.0, 0.0, 0.0));
        // d=2：a=0.25，k=0.25×4=1 → 颜色原样。
        assert!((r - 1.0).abs() < 1e-5 && (g - 0.5).abs() < 1e-5 && (b - 0.25).abs() < 1e-5);
        // 范围外贡献为 0。
        let (r2, g2, b2) = contribution(&l, (100.0, 0.0, 0.0));
        assert_eq!((r2, g2, b2), (0.0, 0.0, 0.0));
    }

    #[test]
    fn vej04_checks_all_green() {
        let set = run_vej04_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F1804 域自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
        );
    }
}
