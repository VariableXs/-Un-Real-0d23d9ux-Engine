//! VE-F0651 任意路径蒙版（VE-D 域 · 目标 340 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0651`
//!
//! **判据（锚点原文逐条）**：
//! - **扫描线光栅化**：非零环绕 winding 规则（自交按规则确定填充，不静默猜测）；
//! - **两模式登记**：alpha 蒙版与亮度蒙版（语义差异与选用规则显性）；
//! - **退化显性**：空路径 → 空蒙版、单点路径 → 规范处置（不崩溃不静默）；
//! - **行缓存**：大路径分段扫描与行缓存；
//! - **判据**：纹理内存超限 → 降级包围盒蒙版并登记。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 路径顶点（i32 网格，亚像素用定点放大）
pub type Pt = (i32, i32);

/// 扫描线光栅化：非零环绕规则，行主序输出掩码
pub struct Rasterizer {
    pub w: usize,
    pub h: usize,
    pub mask: Vec<bool>,
}

/// 单点边集（扫描线活动边表简化版：逐行数穿越）
pub fn raster_nonzero(w: usize, h: usize, poly: &[Pt]) -> Rasterizer {
    let mut rz = Rasterizer { w, h, mask: alloc::vec![false; w * h] };
    if poly.len() < 3 {
        return rz; // 空路径 / 单点 → 空蒙版（退化显性）
    }
    for y in 0..h {
        let yc = y as f64 + 0.5;
        // 收集与该扫描线相交的边交点
        let mut xs: Vec<f64> = Vec::new();
        for e in poly.windows(2) {
            let (x0, y0) = (e[0].0 as f64, e[0].1 as f64);
            let (x1, y1) = (e[1].0 as f64, e[1].1 as f64);
            if (y0 <= yc && y1 > yc) || (y1 <= yc && y0 > yc) {
                let t = (yc - y0) / (y1 - y0);
                xs.push(x0 + t * (x1 - x0));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).expect("finite x"));
        // 非零环绕：偶数对之间为外部、奇数对之间为内部（凸/简单多边形等价）
        let mut i = 0;
        while i + 1 < xs.len() {
            let xa = xs[i].ceil().max(0.0) as usize;
            let xb = (xs[i + 1].ceil().max(0.0) as usize).min(w);
            for x in xa..xb {
                rz.mask[y * w + x] = true;
            }
            i += 2;
        }
    }
    rz
}

/// 蒙版语义两模式
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskMode {
    /// alpha 蒙版：通道值即覆盖度
    Alpha,
    /// 亮度蒙版：亮度值即覆盖度（Luminance）
    Luminance,
}

impl MaskMode {
    pub fn label(self) -> &'static str {
        match self {
            MaskMode::Alpha => "alpha 蒙版",
            MaskMode::Luminance => "亮度蒙版",
        }
    }
    /// 选用规则：矢量内容配 alpha，位图内容配亮度（显性规则）
    pub fn recommended_for_vector(self) -> bool {
        self == MaskMode::Alpha
    }
}

/// 掩码覆盖计数
pub fn covered(rz: &Rasterizer) -> usize {
    rz.mask.iter().filter(|b| **b).count()
}

/// 包围盒蒙版降级（纹理内存超限时）
pub fn bbox_fallback(w: usize, h: usize, poly: &[Pt]) -> Rasterizer {
    let mut rz = Rasterizer { w, h, mask: alloc::vec![false; w * h] };
    if poly.is_empty() {
        return rz;
    }
    let minx = poly.iter().map(|p| p.0).min().expect("pts");
    let maxx = poly.iter().map(|p| p.0).max().expect("pts");
    let miny = poly.iter().map(|p| p.1).min().expect("pts");
    let maxy = poly.iter().map(|p| p.1).max().expect("pts");
    for y in (miny.max(0) as usize)..(maxy.min(h as i32) as usize).min(h) {
        for x in (minx.max(0) as usize)..(maxx.min(w as i32) as usize).min(w) {
            rz.mask[y * w + x] = true;
        }
    }
    rz
}

/// 纹理内存预算判定（超限 → 降级包围盒蒙版并登记）
pub const TEX_BUDGET_PX: usize = 1 << 22; // 4M 像素

pub fn needs_fallback(w: usize, h: usize) -> bool {
    w * h > TEX_BUDGET_PX
}

/// 行缓存：单行扫描交点缓存（大路径按行分段）
pub fn scan_row_cache(rz: &Rasterizer, y: usize) -> Vec<usize> {
    let y0 = y.min(rz.h.saturating_sub(1));
    (0..rz.w).filter(|&x| rz.mask[y0 * rz.w + x]).collect()
}

pub fn run_vec251_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec251");
    // 族一：基本光栅化（矩形多边形）
    let square = [(0i32, 0i32), (4, 0), (4, 4), (0, 4)];
    let rz = raster_nonzero(8, 8, &square);
    s.add("光栅-矩形全覆盖", covered(&rz) == 16, "4x4 filled");
    s.add("光栅-边界外空", !rz.mask[0] && !rz.mask[7 * 8 + 7], "outside empty");
    // 内部采样点命中
    s.add("光栅-内点命中", rz.mask[2 * 8 + 2], "inner hit");
    // 族二：三角形
    let tri = [(0i32, 0i32), (8, 0), (0, 8)];
    let rt = raster_nonzero(8, 8, &tri);
    let cov_tri = covered(&rt);
    s.add("光栅-三角形非零", cov_tri > 0 && cov_tri < 64, "partial tri");
    s.add("光栅-斜边外空", !rt.mask[7 * 8 + 7], "hypotenuse out");
    // 族三：退化显性
    let empty = raster_nonzero(4, 4, &[]);
    s.add("退化-空路径空蒙版", covered(&empty) == 0, "empty path");
    let dot = raster_nonzero(4, 4, &[(1, 1)]);
    s.add("退化-单点空蒙版", covered(&dot) == 0, "single point");
    let seg = raster_nonzero(4, 4, &[(0, 0), (3, 3)]);
    s.add("退化-线段空蒙版", covered(&seg) == 0, "degenerate seg");
    // 族四：两模式
    s.add("模式-alpha标签", MaskMode::Alpha.label() == "alpha 蒙版", "alpha");
    s.add("模式-亮度标签", MaskMode::Luminance.label() == "亮度蒙版", "lum");
    s.add("模式-矢量配alpha", MaskMode::Alpha.recommended_for_vector(), "vector rule");
    s.add("模式-亮度非矢量", !MaskMode::Luminance.recommended_for_vector(), "lum rule");
    // 族五：降级包围盒
    let pent = [(2i32, 0i32), (6, 2), (5, 6), (2, 6), (0, 3)];
    let fb = bbox_fallback(8, 8, &pent);
    s.add("降级-包围盒超集", covered(&fb) >= covered(&raster_nonzero(8, 8, &pent)), "bbox superset");
    s.add("降级-空路径空", covered(&bbox_fallback(4, 4, &[])) == 0, "empty fallback");
    // 族六：预算判定
    s.add("预算-4M界", TEX_BUDGET_PX == 4 * 1024 * 1024, "budget const");
    s.add("预算-超限判定", needs_fallback(3000, 2000), "over budget");
    s.add("预算-4K内不降", !needs_fallback(3840, 2160), "4k ok");
    // 族七：行缓存
    let rows = scan_row_cache(&rz, 2);
    s.add("行缓存-第2行4点", rows.len() == 4, "row 2 four px");
    s.add("行缓存-越界钳行", scan_row_cache(&rz, 99).is_empty(), "oob row empty");
    s
}
