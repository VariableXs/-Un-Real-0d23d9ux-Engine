//! VE-F1804 · 域自检（判据逐条对应，见 `vej04_pointlight.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 平方反比 → `J04-衰减-平方反比对拍`、`J04-衰减-近距爆炸钳制`
//! - 范围钳制 → `J04-范围-early-out与最小半径`
//! - 双曲线 → `J04-曲线-物理默认艺术可选`、`J04-曲线-越界回退物理`
//! - 三档并发 → `J04-并发-三档阶梯裁剪`、`J04-并发-未超档零动作`
//! - 强度 NaN/负钳制（F1802 量纲纪律） → `J04-参数-强度与颜色钳制告警`
//! - 不静默丢弃 → `J04-并发-超档显性告警`
//! - O(1)/像素贡献 → `J04-贡献-着色入口O1`
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use super::vej04_pointlight::*;
use crate::checks::CheckSet;

/// VE-F1804 域自检。
pub fn run_vej04_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vej04");

    // ---- 平方反比 ----

    {
        let l = PointLight::new((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 1.0, 100.0, FalloffCurve::Physical).0;
        let a1 = attenuation(&l, 1.0);
        let a2 = attenuation(&l, 2.0);
        let a10 = attenuation(&l, 10.0);
        let ok = (a1 - 1.0).abs() < 1e-6
            && (a2 - 0.25).abs() < 1e-6
            && (a10 - 0.01).abs() < 1e-5;
        set.add("J04-衰减-平方反比对拍", ok, "");
    }

    // ---- 近距爆炸钳制 ----

    {
        let l = PointLight::new((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 1.0, 100.0, FalloffCurve::Physical).0;
        let near = attenuation(&l, 0.0);
        let clamped = (near - 1.0 / MIN_DISTANCE_SQ).abs() < 1e-3 && near.is_finite();
        set.add("J04-衰减-近距爆炸钳制", clamped, "");
    }

    // ---- 范围钳制：early-out ----

    {
        let l = PointLight::new((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 1.0, 10.0, FalloffCurve::Physical).0;
        let inside = attenuation(&l, 9.999) > 0.0;
        let boundary = attenuation(&l, 10.0) == 0.0;
        let outside = attenuation(&l, 50.0) == 0.0;
        let nonfinite = attenuation(&l, f32::NAN) == 0.0;
        // 范围≤0 → 钳最小半径 + 告警。
        let (l2, warn) =
            PointLight::new((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 1.0, -5.0, FalloffCurve::Physical);
        let clamped_range = l2.range == MIN_RANGE_RADIUS
            && warn.iter().any(|w| w.code == "W_RANGE_CLAMPED");
        set.add(
            "J04-范围-early-out与最小半径",
            inside && boundary && outside && nonfinite && clamped_range,
            "",
        );
    }

    // ---- 双曲线 ----

    {
        let art = PointLight::new(
            (0.0, 0.0, 0.0),
            (1.0, 1.0, 1.0),
            1.0,
            10.0,
            FalloffCurve::Artistic { exponent: 2.0 },
        )
        .0;
        let phys = PointLight::new((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 1.0, 10.0, FalloffCurve::Physical).0;
        // 艺术曲线窗口化：中心 1、中点 (1-0.5)^2、边界 0；与物理曲线不同值。
        let art_ok = (attenuation(&art, 0.0) - 1.0).abs() < 1e-6
            && (attenuation(&art, 5.0) - 0.25).abs() < 1e-6
            && attenuation(&art, 10.0) == 0.0;
        let differs = (attenuation(&art, 5.0) - attenuation(&phys, 5.0)).abs() > 1e-3;
        let doc = POINTLIGHT_SPEC_DOC.contains("艺术曲线");
        set.add("J04-曲线-物理默认艺术可选", art_ok && differs && doc, "");
    }

    // ---- 曲线越界回退 ----

    {
        let (fb, warn) = PointLight::new(
            (0.0, 0.0, 0.0),
            (1.0, 1.0, 1.0),
            1.0,
            10.0,
            FalloffCurve::Artistic { exponent: 99.0 },
        );
        let nan_case = PointLight::new(
            (0.0, 0.0, 0.0),
            (1.0, 1.0, 1.0),
            1.0,
            10.0,
            FalloffCurve::Artistic { exponent: f32::NAN },
        )
        .0;
        set.add(
            "J04-曲线-越界回退物理",
            fb.curve == FalloffCurve::Physical
                && nan_case.curve == FalloffCurve::Physical
                && warn.iter().any(|w| w.code == "W_CURVE_FALLBACK"),
            "",
        );
    }

    // ---- 参数钳制告警 ----

    {
        let (l, warn) =
            PointLight::new((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), -3.0, 10.0, FalloffCurve::Physical);
        let intensity_ok = l.intensity_cd == 0.0 && warn.iter().any(|w| w.code == "W_INTENSITY_CLAMPED");
        let (l2, warn2) = PointLight::new(
            (0.0, 0.0, 0.0),
            (-0.5, 2.0, f32::NAN),
            1.0,
            10.0,
            FalloffCurve::Physical,
        );
        let color_ok = l2.color.r == 0.0 && l2.color.g == 1.0 && l2.color.b == 0.0
            && warn2.len() == 3;
        set.add("J04-参数-强度与颜色钳制告警", intensity_ok && color_ok, "");
    }

    // ---- 三档并发裁剪 ----

    {
        let mut lights = Vec::new();
        for i in 0..64u32 {
            let dist = if i < 20 { 1.0 + i as f32 } else { 100.0 + i as f32 };
            lights.push(
                PointLight::new((dist, 0.0, 0.0), (1.0, 1.0, 1.0), 1.0, 1000.0, FalloffCurve::Physical)
                    .0,
            );
        }
        let (kept, report, warn) = cull_to_tier(&lights, (0.0, 0.0, 0.0), 0);
        let cut = kept.len() == 16
            && report.dropped == 48
            && report.tier == 16
            && warn.is_some();
        let max_kept_dist = kept.iter().map(|l| l.pos.0).fold(0.0f32, f32::max);
        let near_first = max_kept_dist < 100.0;
        // 三档阶梯常量在册（16/32/64）。
        let tiers = CONCURRENCY_TIERS == [16, 32, 64];
        set.add("J04-并发-三档阶梯裁剪", cut && near_first && tiers, "");
    }

    // ---- 未超档零动作 ----

    {
        let lights: Vec<PointLight> = (0..16u32)
            .map(|i| {
                PointLight::new((i as f32, 0.0, 0.0), (1.0, 1.0, 1.0), 1.0, 100.0, FalloffCurve::Physical)
                    .0
            })
            .collect();
        let (kept, report, warn) = cull_to_tier(&lights, (0.0, 0.0, 0.0), 0);
        set.add(
            "J04-并发-未超档零动作",
            kept.len() == 16 && report.dropped == 0 && warn.is_none(),
            "",
        );
    }

    // ---- 着色入口 O(1) ----

    {
        let l = PointLight::new((0.0, 0.0, 0.0), (1.0, 0.5, 0.25), 4.0, 10.0, FalloffCurve::Physical).0;
        let (r, g, b) = contribution(&l, (2.0, 0.0, 0.0));
        let inside_ok = (r - 1.0).abs() < 1e-5 && (g - 0.5).abs() < 1e-5 && (b - 0.25).abs() < 1e-5;
        let (r2, g2, b2) = contribution(&l, (100.0, 0.0, 0.0));
        let outside_zero = (r2, g2, b2) == (0.0, 0.0, 0.0);
        set.add("J04-贡献-着色入口O1", inside_ok && outside_zero, "");
    }

    set
}
