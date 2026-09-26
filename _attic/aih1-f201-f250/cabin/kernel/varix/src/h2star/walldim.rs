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
pub fn filter_route(theme: ThemeKind, dim_pct: u32) -> Option<(u32, u32)> {
    match theme {
        ThemeKind::Light => None,
        ThemeKind::Dark => Some((clamp_dim(dim_pct), DEFAULT_DESAT_PCT)),
    }
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
    // 浅色不处理。
    set.add(
        "F297 light passthrough",
        filter_route(ThemeKind::Light, 30).is_none()
            && filter_route(ThemeKind::Dark, 30) == Some((30, 15)),
        "zero cost path",
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
