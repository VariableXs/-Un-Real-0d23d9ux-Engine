//! VE-F0766 九宫格 GPU 采样实现
//! 职责：九宫格的 GPU 采样路径——顶点生成（9 quad = 18 tri）、
//!       UV 坐标发射、半像素偏移对齐、采样包围盒裁剪。
//! 契约：顶点数恒 36（9 quad × 4）；UV 与顶点一一对应；半像素对齐可开关。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec361_ninecore::{Cell, Slice, layout};
use super::vec362_ninepolicy::{ZONES, uv_of};

/// 顶点（位置 + UV）
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Vtx {
    pub x: f64,
    pub y: f64,
    pub u: f64,
    pub v: f64,
}

/// 顶点生成配置
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct EmitCfg {
    pub half_pixel: bool,
}

/// 生成九宫格顶点（9 quad × 4 顶点 = 36）
pub fn emit_vertices(s: &Slice, dx: f64, dy: f64, w: f64, h: f64, cfg: EmitCfg) -> Option<Vec<Vtx>> {
    let cells = layout(s, dx, dy, w, h)?;
    let hp = if cfg.half_pixel { 0.5 } else { 0.0 };
    let mut out: Vec<Vtx> = Vec::with_capacity(36);
    for (&zone, cell) in ZONES.iter().zip(cells.iter()) {
        let (ux, uy, uw, uh) = uv_of(s, zone);
        let (x0, y0) = (cell.x + hp, cell.y + hp);
        let (x1, y1) = (cell.x + cell.w - hp, cell.y + cell.h - hp);
        // 半像素内缩不得越过区间（零尺寸区跳过内缩）
        let (x0, x1) = if x1 < x0 { (cell.x, cell.x + cell.w) } else { (x0, x1) };
        let (y0, y1) = if y1 < y0 { (cell.y, cell.y + cell.h) } else { (y0, y1) };
        // 两三角形 = 4 顶点（索引展开）
        for (px, py, pu, pv) in [
            (x0, y0, ux, uy),
            (x1, y0, ux + uw, uy),
            (x1, y1, ux + uw, uy + uh),
            (x0, y1, ux, uy + uh),
        ] {
            out.push(Vtx { x: px, y: py, u: pu, v: pv });
        }
    }
    Some(out)
}

/// 顶点数守恒
pub fn count_check(v: &[Vtx]) -> bool {
    v.len() == 36
}

/// 采样包围盒
pub fn sample_bounds(v: &[Vtx]) -> (f64, f64, f64, f64) {
    let mut min = (f64::INFINITY, f64::INFINITY);
    let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in v {
        min.0 = min.0.min(p.x);
        min.1 = min.1.min(p.y);
        max.0 = max.0.max(p.x);
        max.1 = max.1.max(p.y);
    }
    (min.0, min.1, max.0, max.1)
}

/// UV 包围盒（应为整个纹理）
pub fn uv_bounds(v: &[Vtx]) -> (f64, f64, f64, f64) {
    let mut min = (f64::INFINITY, f64::INFINITY);
    let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in v {
        min.0 = min.0.min(p.u);
        min.1 = min.1.min(p.v);
        max.0 = max.0.max(p.u);
        max.1 = max.1.max(p.v);
    }
    (min.0, min.1, max.0, max.1)
}

/// 半像素对齐检查：开启时全部顶点坐标有 .5 偏移分量
pub fn half_pixel_on(v: &[Vtx]) -> bool {
    v.iter().all(|p| (p.x - p.x.floor() - 0.5).abs() < 1e-9 || (p.y - p.y.floor() - 0.5).abs() < 1e-9)
}

pub fn run_vec366_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec366");
    let ins = super::vec361_ninecore::Insets::uniform(8.0);
    let sl = Slice::build(64.0, 64.0, ins).expect("slice");
    // 族一：顶点生成
    let v = emit_vertices(&sl, 0.0, 0.0, 100.0, 80.0, EmitCfg { half_pixel: false }).expect("v");
    s.add("顶点-数量", count_check(&v), "36 verts");
    // 首个 quad = 左上角
    s.add("顶点-左上", v[0] == Vtx { x: 0.0, y: 0.0, u: 0.0, v: 0.0 }, "TL first");
    s.add("顶点-左上对角", v[2] == Vtx { x: 8.0, y: 8.0, u: 8.0, v: 8.0 }, "TL diag");
    // 中心 quad
    let center = &v[16..20];
    s.add("顶点-中心起", center[0].x == 8.0 && center[0].y == 8.0, "center origin");
    s.add("顶点-中心对角", center[2].x == 92.0 && center[2].y == 72.0, "center diag");
    // UV 对角
    s.add("UV-中心对角", center[2].u == 56.0 && center[2].v == 56.0, "center uv diag");
    // 族二：包围盒
    let (sx0, sy0, sx1, sy1) = sample_bounds(&v);
    s.add("采样-范围", sx0 == 0.0 && sy0 == 0.0 && sx1 == 100.0 && sy1 == 80.0, "screen bounds");
    let (ux0, uy0, ux1, uy1) = uv_bounds(&v);
    s.add("UV-全纹理", ux0 == 0.0 && uy0 == 0.0 && ux1 == 64.0 && uy1 == 64.0, "uv full");
    // 族三：半像素偏移
    let vh = emit_vertices(&sl, 0.0, 0.0, 100.0, 80.0, EmitCfg { half_pixel: true }).expect("vh");
    s.add("半像素-数量", count_check(&vh), "36 verts");
    s.add("半像素-偏移在", half_pixel_on(&vh), "0.5 offsets");
    s.add("半像素-关闭无", !half_pixel_on(&v), "off = integer");
    // 首顶点偏移对比
    s.add("半像素-首点", vh[0].x == 0.5 && vh[0].y == 0.5, "first +0.5");
    // 半像素不越界：最小尺寸时角区退化不产生倒置
    let vmin = emit_vertices(&sl, 0.0, 0.0, 16.0, 16.0, EmitCfg { half_pixel: true }).expect("vmin");
    let (a, b, c, d) = sample_bounds(&vmin);
    s.add("半像素-最小不倒置", a < c && b < d, "bounds sane");
    // 族四：布局失败传导
    s.add("布局-过小传导", emit_vertices(&sl, 0.0, 0.0, 8.0, 8.0, EmitCfg { half_pixel: false }).is_none(), "min size gate");
    // 族五：偏移布局
    let vo = emit_vertices(&sl, 10.0, 20.0, 100.0, 80.0, EmitCfg { half_pixel: false }).expect("vo");
    s.add("偏移-首点", vo[0].x == 10.0 && vo[0].y == 20.0, "offset origin");
    let (ox0, oy0, _, _) = sample_bounds(&vo);
    s.add("偏移-包围盒", ox0 == 10.0 && oy0 == 20.0, "bounds offset");
    // 族六：UV 与顶点数一一对应
    s.add("对应-逐点", v.iter().zip(vh.iter()).all(|(a, b)| a.u == b.u - 0.0 || a.u == b.u), "uv pairing sane");
    s
}
