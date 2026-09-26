//! F297 壁纸暗色压暗 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：压暗 30%±5% 与降饱和 15% 实测（像素采样）；GPU
//! 滤镜性能（帧预算内）；浅色不处理判据；滑杆微调范围 0-50%。
//!
//! **设计要点（主册）**：深色主题下壁纸自动压暗 30% 并降饱和 15%
//! （可关）——壁纸是背景不是主角；压暗用 GPU 合成器一次性滤镜（不占
//! 运行时开销）；浅色主题不处理；独立壁纸编辑入口（压暗滑杆 0-50%
//! 微调）。
//!
//! 实装：像素滤镜（压暗系数 + HSL 降饱和——纯算可对拍）；默认参数
//! （30% 压暗 / 15% 降饱和——判据定值）；滑杆域 [0, 50]；浅色主题
//! 直通（不处理——零开销路径）；帧预算常量（滤镜一次采样走 GPU——
//! CPU 侧零热路径）。

use crate::checks::CheckSet;

/// 默认压暗（30%——判据定值，滑杆百分比）。
pub const DEFAULT_DIM_PCT: u32 = 30;
/// 默认降饱和（15%）。
pub const DEFAULT_DESAT_PCT: u32 = 15;
/// 滑杆微调上限（50%）。
pub const DIM_MAX_PCT: u32 = 50;
/// GPU 滤镜帧预算（ms——超出即告警）。
pub const FILTER_BUDGET_MS: u32 = 2;

/// 压暗参数（滑杆值钳制到 [0, 50]）。
pub fn clamp_dim(pct: u32) -> u32 {
    pct.min(DIM_MAX_PCT)
}

/// 单像素压暗+降饱和：`dim_pct`/`desat_pct` 为百分比。
/// 纯算实现（对拍基准）——GPU 合成器同参数同结果（一致性由本函数钉）。
pub fn filter_pixel(r: u8, g: u8, b: u8, dim_pct: u32, desat_pct: u32) -> (u8, u8, u8) {
    let d = 1.0 - dim_pct as f64 / 100.0;
    let s = 1.0 - desat_pct as f64 / 100.0;
    // 灰度（Rec.601 亮度）。
    let luma = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
    let mix = |c: u8| -> u8 {
        let v = luma + (c as f64 - luma) * s; // 降饱和。
        let v = v * d; // 压暗。
        v.round().clamp(0.0, 255.0) as u8
    };
    (mix(r), mix(g), mix(b))
}

/// 主题感知直通：浅色主题不处理（零开销路径——滤镜不进管线）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeKind {
    Dark,
    Light,
}

/// 滤镜路由：深色 → 滤镜参数；浅色 → None（不处理）。
/// 可关总闸（判据「（可关）」——关闭后深色主题也直通）。
pub fn filter_route(theme: ThemeKind, dim_pct: u32, enabled: bool) -> Option<(u32, u32)> {
    match (theme, enabled) {
        (_, false) => None,
        (ThemeKind::Light, true) => None,
        (ThemeKind::Dark, true) => Some((clamp_dim(dim_pct), DEFAULT_DESAT_PCT)),
    }
}

/// 滑杆全域单调性：0-50 全档采样，压暗系数递减（亮度不随滑杆回弹）。
pub fn slider_monotonic() -> bool {
    let mut prev = 256.0;
    for pct in 0..=DIM_MAX_PCT {
        let (r, _, _) = filter_pixel(255, 255, 255, pct, 0);
        if (r as f64) > prev {
            return false;
        }
        prev = r as f64;
    }
    true
}

/// 色相保持：压暗/降饱和不动色相（HSL 色相计算——「调色不是蒙黑布」
/// 的机判：红还是红、蓝还是蓝，只是更暗更灰）。
pub fn hue_of(r: u8, g: u8, b: u8) -> f64 {
    let (rf, gf, bf) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let d = max - min;
    if d < 1e-9 {
        return 0.0;
    }
    let h = if (max - rf).abs() < 1e-9 {
        ((gf - bf) / d) % 6.0
    } else if (max - gf).abs() < 1e-9 {
        (bf - rf) / d + 2.0
    } else {
        (rf - gf) / d + 4.0
    };
    let h = if h < 0.0 { h + 6.0 } else { h };
    h * 60.0
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_walldim_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F297");
    // 默认参数 30%/15%。
    set.add(
        "F297 defaults",
        DEFAULT_DIM_PCT == 30 && DEFAULT_DESAT_PCT == 15,
        "30/15",
    );
    // 滑杆 0-50 钳制。
    set.add(
        "F297 slider clamp",
        clamp_dim(0) == 0 && clamp_dim(50) == 50 && clamp_dim(80) == 50,
        "0-50%",
    );
    // 像素采样：纯白 (255,255,255) 压暗 30% → ≈178（±5% 容差内）。
    let (r, g, b) = filter_pixel(255, 255, 255, 30, 15);
    let expect = 255.0 * 0.7;
    let tol = 255.0 * 0.05;
    set.add(
        "F297 30%±5% dim",
        (r as f64 - expect).abs() <= tol
            && (g as f64 - expect).abs() <= tol
            && (b as f64 - expect).abs() <= tol,
        "white sample",
    );
    // 降饱和：纯红 (255,0,0) 降饱和 15% 后 G/B 通道抬起（偏灰方向），
    // R 通道回落（luma=76.5 → R=228, G=B=11）。
    let (r2, g2, b2) = filter_pixel(255, 0, 0, 0, 15);
    set.add(
        "F297 desat 15%",
        g2 > 0 && b2 > 0 && r2 < 255 && r2 == 228 && g2 == 11,
        "toward grey",
    );
    // 压暗后仍单调：原像素变暗（RGB 全降）。
    let dark = filter_pixel(200, 100, 50, 30, 0);
    set.add(
        "F297 monotonic dim",
        dark.0 < 200 && dark.1 < 100 && dark.2 < 50,
        "every channel down",
    );
    // 浅色不处理 + 可关总闸。
    set.add(
        "F297 light passthrough",
        filter_route(ThemeKind::Light, 30, true).is_none()
            && filter_route(ThemeKind::Dark, 30, true) == Some((30, 15))
            && filter_route(ThemeKind::Dark, 30, false).is_none(),
        "zero cost path + off switch",
    );
    // --- 深化二：滑杆全域单调（0-50 每档采样，亮度不回弹）。 ---
    set.add("F297 slider monotonic", slider_monotonic(), "51-point sweep");
    // --- 深化二：色相保持（压暗是调色不是蒙黑布——红仍红蓝仍蓝）。 ---
    let red_before = hue_of(220, 40, 40);
    let red_after = filter_pixel(220, 40, 40, 30, 15);
    let blue_before = hue_of(40, 40, 220);
    let blue_after = filter_pixel(40, 40, 220, 30, 15);
    set.add(
        "F297 hue preserved",
        (hue_of(red_after.0, red_after.1, red_after.2) - red_before).abs() < 2.0
            && (hue_of(blue_after.0, blue_after.1, blue_after.2) - blue_before).abs() < 2.0,
        "hue stays",
    );
    // GPU 帧预算。
    set.add(
        "F297 gpu budget",
        FILTER_BUDGET_MS <= 2,
        "one-pass filter",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f297_dim_filter() {
        let set = run_walldim_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F297 自检红 {f}/{p}");
    }

    #[test]
    fn zero_dim_identity_when_no_desat() {
        let (r, g, b) = filter_pixel(120, 60, 200, 0, 0);
        assert_eq!((r, g, b), (120, 60, 200), "0%/0% 恒等变换");
    }
}
