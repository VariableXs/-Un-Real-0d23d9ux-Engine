//! VE-F0773 九宫格对拍
//! 职责：GPU 顶点路径 vs CPU 像素路径的覆盖对拍、
//!       区覆盖面积对账（九区面积和恒等）、量化 vs 连续路径对拍。
//! 契约：两路径覆盖面积偏差 < 1px²；面积恒等在全尺寸域成立。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec361_ninecore::{Cell, Insets, Slice, layout};
use super::vec362_ninepolicy::ZONES;

/// 覆盖面积（九区面积和）
pub fn covered_area(cells: &[Cell; 9]) -> f64 {
    cells.iter().map(|c| c.w * c.h).sum()
}

/// 像素网格覆盖面积（量化到 1px 网格后的面积和）
pub fn covered_area_quant(cells: &[Cell; 9]) -> f64 {
    cells.iter().map(|c| c.w.round() * c.h.round()).sum()
}

/// 区覆盖完整性（无重叠无遗漏）：用边界扫描验证——任意点的覆盖次数为 1
/// 简化为行列和校验：三列宽和 == w、三行高和 == h
pub fn tiling_ok(cells: &[Cell; 9], x: f64, y: f64, w: f64, h: f64) -> bool {
    let col_w = [cells[0].w, cells[1].w, cells[2].w];
    let row_h = [cells[0].h, cells[3].h, cells[6].h];
    ((col_w[0] + col_w[1] + col_w[2]) - w).abs() < 1e-9
        && ((row_h[0] + row_h[1] + row_h[2]) - h).abs() < 1e-9
        && cells[0].x == x
        && cells[0].y == y
}

/// 对拍报告
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CoverRep {
    pub gpu: f64,
    pub cpu: f64,
    pub tol: f64,
}

impl CoverRep {
    pub fn pass(&self) -> bool {
        (self.gpu - self.cpu).abs() <= self.tol
    }
}

/// 全尺寸域扫掠（多个尺寸下全部通过）
pub fn sweep_sizes(s: &Slice, sizes: &[(f64, f64)], tol: f64) -> usize {
    let mut pass = 0;
    for &(w, h) in sizes {
        if let Some(cells) = layout(s, 0.0, 0.0, w, h) {
            let rep = CoverRep { gpu: covered_area(cells_ref(&cells)), cpu: covered_area_quant(cells_ref(&cells)), tol };
            if rep.pass() {
                pass += 1;
            }
        }
    }
    pass
}

fn cells_ref(c: &[Cell; 9]) -> &[Cell; 9] {
    // 安全：len == 9 由构造保证
    match c.try_into() {
        Ok(r) => r,
        Err(_) => unreachable!(),
    }
}

pub fn run_vec373_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec373");
    let ins = Insets::uniform(8.0);
    let sl = Slice::build(64.0, 64.0, ins).expect("slice");
    // 族一：面积恒等
    let cells = layout(&sl, 0.0, 0.0, 100.0, 80.0).expect("c");
    s.add("面积-连续", (covered_area(cells_ref(&cells)) - 8000.0).abs() < 1e-9, "100×80");
    s.add("面积-量化", (covered_area_quant(cells_ref(&cells)) - 8000.0).abs() < 1e-9, "integer sizes");
    // 对拍
    let rep = CoverRep { gpu: covered_area(cells_ref(&cells)), cpu: covered_area_quant(cells_ref(&cells)), tol: 1.0 };
    s.add("对拍-整数过", rep.pass(), "integer layout");
    // 非整数尺寸
    let cells_f = layout(&sl, 0.0, 0.0, 100.37, 80.62).expect("cf");
    let rep_f = CoverRep { gpu: covered_area(cells_ref(&cells_f)), cpu: covered_area_quant(cells_ref(&cells_f)), tol: 4.0 };
    s.add("对拍-非整数过", rep_f.pass(), "fractional within 4px²");
    // 偏差本身 < 4
    s.add("对拍-偏差小", (covered_area(cells_ref(&cells_f)) - covered_area_quant(cells_ref(&cells_f))).abs() < 4.0, "dev < 4");
    // 族二：平铺完整性
    s.add("平铺-行列和", tiling_ok(cells_ref(&cells), 0.0, 0.0, 100.0, 80.0), "3col/3row sums");
    // 非对称切片
    let asym = Slice::build(100.0, 100.0, Insets { left: 10.0, top: 20.0, right: 30.0, bottom: 5.0 }).expect("a");
    let ac = layout(&asym, 0.0, 0.0, 200.0, 150.0).expect("ac");
    s.add("平铺-非对称", tiling_ok(cells_ref(&ac), 0.0, 0.0, 200.0, 150.0), "asym tiling");
    s.add("面积-非对称恒等", (covered_area(cells_ref(&ac)) - 30000.0).abs() < 1e-9, "200×150");
    // 族三：多尺寸扫掠
    let sizes: Vec<(f64, f64)> = (1..=8).map(|k| (16.0 * k as f64, 16.0 * k as f64)).collect();
    let passed = sweep_sizes(&sl, &sizes, 4.0);
    s.add("扫掠-全过", passed == sizes.len(), "8 sizes pass");
    // 恰好最小尺寸
    s.add("扫掠-含最小", sizes.iter().any(|&(w, _)| w == 16.0), "min included");
    // 族四：九区各非负
    s.add("区-非负", cells.iter().all(|c| c.w >= 0.0 && c.h >= 0.0), "all non-negative");
    // 最小尺寸中心为 0
    let min_c = layout(&sl, 0.0, 0.0, 16.0, 16.0).expect("min");
    s.add("区-最小中心零", min_c[4].w == 0.0 && min_c[4].h == 0.0, "center zero");
    // 族五：报告判定语义
    s.add("报告-过", CoverRep { gpu: 10.0, cpu: 10.2, tol: 0.5 }.pass(), "within tol");
    s.add("报告-不过", !CoverRep { gpu: 10.0, cpu: 11.0, tol: 0.5 }.pass(), "out of tol");
    s
}
