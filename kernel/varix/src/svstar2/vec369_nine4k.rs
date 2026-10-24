//! VE-F0769 九宫格 4K 精度
//! 职责：大画布（3840×2160）下的亚像素精度——像素对齐量化、
//!       UV 插值精度（大 UV 跨度下的误差界）、整数路径防漂移。
//! 契约：像素对齐后误差 < 0.5px；UV 误差相对 < 1e-6；量化可逆（量化前后整数部分不变）。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 像素对齐量化（最近整数像素边界）
pub fn snap_px(v: f64) -> f64 {
    v.round()
}

/// 量化误差
pub fn snap_err(v: f64) -> f64 {
    (v - snap_px(v)).abs()
}

/// 大画布 UV 插值误差界：线性插值在 [0,N] 区间上的浮点相对误差上界
pub fn uv_err_bound(uv_span: f64) -> f64 {
    // f64 机器精度 2^-52，跨度线性累积 ≤ 3 ulp × span
    uv_span * 3.0 * (2.0f64).powi(-52)
}

/// 4K 画布常量
pub const CANVAS_W: f64 = 3840.0;
pub const CANVAS_H: f64 = 2160.0;

/// 整数路径（防漂移的定点量化）
pub fn to_fixed(v: f64, frac_bits: u32) -> i64 {
    (v * (1i64 << frac_bits) as f64).round() as i64
}

pub fn from_fixed(q: i64, frac_bits: u32) -> f64 {
    q as f64 / (1i64 << frac_bits) as f64
}

/// 定点往返误差 < 1/2^frac_bits
pub fn fixed_roundtrip(v: f64, frac_bits: u32) -> f64 {
    (v - from_fixed(to_fixed(v, frac_bits), frac_bits)).abs()
}

/// 4K 下的九宫格量化布局（全部坐标 snap 到整数像素）
pub fn snap_cells(cells: &[(f64, f64, f64, f64)]) -> Vec<(f64, f64, f64, f64)> {
    cells.iter().map(|&(x, y, w, h)| {
        let (x0, y0) = (snap_px(x), snap_px(y));
        let (x1, y1) = (snap_px(x + w), snap_px(y + h));
        (x0, y0, x1 - x0, y1 - y0)
    }).collect()
}

pub fn run_vec369_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec369");
    // 族一：像素对齐
    s.add("对齐-整点", snap_px(100.0) == 100.0, "integer kept");
    s.add("对齐-上取", snap_px(100.4) == 100.0, "round down");
    s.add("对齐-下取", snap_px(100.6) == 101.0, "round up");
    s.add("对齐-误差界", snap_err(100.49) < 0.5, "< 0.5px");
    // 全画布范围内误差 < 0.5
    let mut max_err = 0.0f64;
    let mut k = 0;
    while k < 1000 {
        let v = k as f64 * CANVAS_W / 1000.0 + 0.37;
        max_err = max_err.max(snap_err(v));
        k += 1;
    }
    s.add("对齐-全画布", max_err <= 0.5, "4K max err ≤ 0.5");
    // 族二：UV 精度
    let b4k = uv_err_bound(3840.0);
    s.add("UV-4K界", b4k < 1e-9, "4K span < 1e-9");
    s.add("UV-相对界", b4k / 3840.0 < 1e-12, "relative tiny");
    s.add("UV-零跨度", uv_err_bound(0.0) == 0.0, "zero span");
    // 族三：定点防漂移
    s.add("定点-16位往返", fixed_roundtrip(1024.5, 16) < 1.0 / 65536.0, "1/65536");
    s.add("定点-整点无损", from_fixed(to_fixed(100.0, 8), 8) == 100.0, "integer exact");
    // 大数值不漂移
    s.add("定点-4K无损", from_fixed(to_fixed(3840.0, 8), 8) == 3840.0, "3840 exact");
    s.add("定点-误差界", fixed_roundtrip(0.7, 16) <= 1.0 / 65536.0 + 1e-15, "bounded");
    // 32 位更精
    s.add("定点-32位更精", fixed_roundtrip(0.123456789, 32) <= fixed_roundtrip(0.123456789, 16), "32 ≤ 16 err");
    // 族四：量化布局
    let cells = alloc::vec![(0.0, 0.0, 100.3, 80.7), (100.3, 0.0, 200.6, 80.7)];
    let snapped = snap_cells(&cells);
    s.add("量化-数量", snapped.len() == 2, "2 cells");
    // 量化后宽高为整数
    s.add("量化-整数宽", snapped.iter().all(|&(_, _, w, _)| w == w.round()), "int width");
    s.add("量化-整数高", snapped.iter().all(|&(_, _, _, h)| h == h.round()), "int height");
    // 量化后 x+w 恒等（右边界不漂移）
    s.add("量化-右边界守恒", {
        let (_, _, w0, _) = snapped[0];
        let (x1, _, _, _) = snapped[1];
        (x1 - (0.0 + w0)).abs() < 1e-9
    }, "shared edge kept");
    // 族五：4K 面积守恒（量化后总面积偏差有界）
    let big = alloc::vec![(0.0, 0.0, 1920.5, 1080.5), (1920.5, 0.0, 1920.5, 1080.5), (0.0, 1080.5, 1920.5, 1080.5), (1920.5, 1080.5, 1920.5, 1080.5)];
    let sq = snap_cells(&big);
    let area_q: f64 = sq.iter().map(|c| c.2 * c.3).sum();
    let area_orig: f64 = big.iter().map(|c| c.2 * c.3).sum();
    s.add("面积-4K守恒", (area_q - area_orig).abs() < 4.0, "quantized area within 4px²");
    // 族六：非负性
    s.add("量化-非负宽", snapped.iter().all(|&(_, _, w, _)| w >= 0.0), "non-negative");
    s
}
