//! VE-F0712 · 稀疏路径优化（长细线）
//!
//! 职责：稀疏路径（长细线——一根线穿过整个屏幕但覆盖极少像素）的优化——
//! 包围盒驱动的全帧处理是浪费（bbox 巨大但覆盖小），
//! 优化=边定向处理（只处理线附近像素——Bresenham 类直接描线或细条带处理）。
//! 触发检测：路径 bbox 面积/实际覆盖比超高时切换稀疏模式。

use alloc::vec;
use alloc::vec::Vec;

/// 稀疏度判定：bbox 面积与实际覆盖的比值（千分位覆盖密度）。
/// 覆盖密度低于阈值（默认 50‰）即稀疏。
pub fn sparsity(bbox_area: u64, actual_cover_px: u64, threshold_permille: u32) -> bool {
    if bbox_area == 0 {
        return false;
    }
    let density = actual_cover_px * 1000 / bbox_area;
    density < threshold_permille as u64
}

/// Bresenham 直接描线：稀疏模式下只写线经过的像素（不建 AEL、不遍历 bbox）。
/// 返回线经过的整数像素坐标表。
pub fn bresenham(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut pts = Vec::new();
    let (mut x, mut y) = (x0, y0);
    loop {
        pts.push((x, y));
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    pts
}

/// 细条带处理：线宽 w 的描边——沿 Bresenham 主线向法线方向展开 w/2 像素。
pub fn thin_stroke(pts: &[(i32, i32)], half_width: u32) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    let hw = half_width as i32;
    for &(x, y) in pts {
        for dy in -hw..=hw {
            for dx in -hw..=hw {
                if dx * dx + dy * dy <= (hw * hw) {
                    out.push((x + dx, y + dy));
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// 模式切换决策：稀疏度 + 线宽 → 描线模式 or 常规填充。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SparseMode {
    DirectLine,
    ThinBand,
    RegularFill,
}

pub fn pick_sparse_mode(bbox_area: u64, cover: u64, stroke_width: u32) -> SparseMode {
    if !sparsity(bbox_area, cover, 50) {
        return SparseMode::RegularFill;
    }
    if stroke_width <= 2 {
        SparseMode::DirectLine
    } else {
        SparseMode::ThinBand
    }
}

/// 性能对拍模型：bbox 驱动 vs 定向处理的像素访问量。
pub fn visited_pixels(bbox_area: u64, line_len: u64, sparse: bool) -> u64 {
    if sparse {
        line_len // 只访问线附近
    } else {
        bbox_area // 全 bbox 遍历
    }
}

pub fn run_vec312_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec312");
    // 族一：稀疏检测
    s.add("长对角线判稀疏", sparsity(1920 * 1080, 2203, 50), "diag-sparse");
    s.add("实心矩形不稀疏", !sparsity(100 * 100, 10000, 50), "solid");
    s.add("零 bbox 不判稀疏", !sparsity(0, 0, 50), "zero-bbox");
    s.add("阈值可调", sparsity(1000, 60, 50) && !sparsity(1000, 60, 100), "threshold");
    // 族二：Bresenham 正确性
    let line = bresenham(0, 0, 4, 0);
    s.add("水平线逐点", line == vec![(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)], "h-line");
    let diag = bresenham(0, 0, 3, 3);
    s.add("对角线不跳点", diag == vec![(0, 0), (1, 1), (2, 2), (3, 3)], "diag");
    let steep = bresenham(0, 0, 1, 3);
    s.add("陡线连通", {
        // 相邻点必须共享一坐标（连通性）。
        steep.windows(2).all(|w| (w[0].0 - w[1].0).abs() <= 1 && (w[0].1 - w[1].1).abs() <= 1)
    }, "steep");
    let rev = bresenham(4, 0, 0, 0);
    s.add("反向线同点集", {
        let mut a = rev.clone();
        a.sort_unstable();
        let mut b = line.clone();
        b.sort_unstable();
        a == b
    }, "reverse");
    s.add("端点必含", bresenham(0, 0, 5, 2).last() == Some(&(5, 2)), "endpoint");
    // 族三：细条带
    let band = thin_stroke(&[(0, 0)], 1);
    s.add("半径 1 十字五点", band.len() == 5 && band.contains(&(0, 1)) && band.contains(&(1, 0)), "cross");
    let band2 = thin_stroke(&[(0, 0)], 2);
    s.add("半径 2 圆盘", band2.len() == 13, "disk2");
    s.add("条带去重", thin_stroke(&[(0, 0), (0, 0)], 1).len() == 5, "dedup");
    // 族四：模式切换
    s.add("稀疏+细线→直接描线", pick_sparse_mode(100000, 300, 1) == SparseMode::DirectLine, "pick-line");
    s.add("稀疏+粗线→条带", pick_sparse_mode(100000, 300, 4) == SparseMode::ThinBand, "pick-band");
    s.add("不稀疏→常规填充", pick_sparse_mode(100, 100, 1) == SparseMode::RegularFill, "pick-fill");
    // 族五：性能对拍
    s.add("定向处理访问量小", visited_pixels(2073600, 2203, true) < visited_pixels(2073600, 2203, false), "visited");
    s.add("对角线场景提升显著", {
        let gain = visited_pixels(2073600, 2203, false) / visited_pixels(2073600, 2203, true);
        gain > 100
    }, "gain");
    s.add("触发阈值显性", sparsity(1000, 40, 50) == true, "trigger");
    s
}
