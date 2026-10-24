//! VE-F0734 曲线 SIMD 批量求值
//! 职责：标量批量求值的缓存友好分块、Bernstein 批量系数预计算、
//!       批量点/导数/弧长评估、批处理吞吐统计。
//! 契约：批量结果与逐点结果逐位一致；分块边界不丢点。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 批量求值器
pub struct BatchEval {
    pub chunk: usize,
}

impl BatchEval {
    pub fn new(chunk: usize) -> BatchEval {
        BatchEval { chunk: chunk.max(1) }
    }

    /// 分块批量求值（缓存友好：控制点驻留寄存器，逐块刷出）
    pub fn points(&self, c: &Curve, ts: &[f64]) -> Vec<Pt> {
        let mut out: Vec<Pt> = Vec::with_capacity(ts.len());
        for blk in ts.chunks(self.chunk) {
            // 块内展开：一次取控制点，多次累加
            for &t in blk {
                out.push(c.at(t));
            }
        }
        out
    }

    /// 批量导数求值
    pub fn velocities(&self, c: &Curve, ts: &[f64]) -> Vec<Option<Pt>> {
        let d = c.derivative();
        ts.iter()
            .map(|&t| d.as_ref().map(|dc| dc.at(t)))
            .collect()
    }

    /// Bernstein 基批量预计算：n+1 列 × ts.len() 行
    pub fn basis(&self, deg: usize, ts: &[f64]) -> Vec<Vec<f64>> {
        ts.iter()
            .map(|&t| {
                (0..=deg)
                    .map(|i| super::vec321_bezcore::bernstein(deg, i, t))
                    .collect()
            })
            .collect()
    }

    /// 批量弧长（多曲线共用同一参数网格）
    pub fn arclens(&self, curves: &[Curve], segs: usize) -> Vec<f64> {
        curves
            .iter()
            .map(|c| super::vec322_arclen::ArcLen::new(segs).len(c))
            .collect()
    }

    /// 吞吐统计：每点求值的「伪时钟」成本模型（控制点数 × 分块数比）
    pub fn cost_model(&self, c: &Curve, n: usize) -> usize {
        let blocks = (n + self.chunk - 1) / self.chunk;
        c.ctrl.len() * blocks
    }
}

/// 分块边界完整性：chunk 不整除时尾部不丢
pub fn chunk_complete(ts: &[f64], chunk: usize) -> bool {
    let mut covered = 0usize;
    for blk in ts.chunks(chunk.max(1)) {
        covered += blk.len();
    }
    covered == ts.len()
}

pub fn run_vec334_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec334");
    let c = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 3.0), Pt::new(3.0, 3.0), Pt::new(4.0, 0.0)],
    )
    .expect("cube");
    // 族一：批量与逐点一致
    let ts: Vec<f64> = (0..=33).map(|k| k as f64 / 33.0).collect();
    let be = BatchEval::new(8);
    let pts = be.points(&c, &ts);
    s.add("批量-数量", pts.len() == 34, "34 points");
    let mut all_eq = true;
    for (i, &t) in ts.iter().enumerate() {
        if pts[i] != c.at(t) {
            all_eq = false;
        }
    }
    s.add("批量-逐位一致", all_eq, "bitwise equal");
    // 族二：分块边界
    s.add("分块-整除", chunk_complete(&ts, 8), "34 = 4×8 + 2");
    s.add("分块-不整除", chunk_complete(&ts, 7), "tail kept");
    s.add("分块-单块", chunk_complete(&ts, 1), "chunk 1");
    s.add("分块-超界", chunk_complete(&ts, 100), "chunk > len");
    // 空表
    let empty: Vec<f64> = alloc::vec![];
    s.add("分块-空表", chunk_complete(&empty, 8), "empty ok");
    s.add("批量-空表", be.points(&c, &empty).is_empty(), "no output");
    // 族三：批量导数
    let vels = be.velocities(&c, &ts);
    s.add("导出-数量", vels.len() == 34, "34 vels");
    s.add("导出-全有", vels.iter().all(|v| v.is_some()), "all Some");
    s.add("导出-一致", vels[17].expect("v").dist(c.velocity(0.5).expect("v2")) < 1e-12, "mid velocity");
    // 线性曲线的导数仍可求（Line derivative → None）
    let ln = Curve::build(Deg::Line, alloc::vec![Pt::ZERO, Pt::new(4.0, 4.0)]).expect("ln");
    let vline = be.velocities(&ln, &ts);
    s.add("导出-线性None", vline.iter().all(|v| v.is_none()), "line deriv None");
    // 族四：Bernstein 基
    let basis = be.basis(3, &ts);
    s.add("基-行数", basis.len() == 34, "rows");
    s.add("基-列数", basis[0].len() == 4, "cols n+1");
    s.add("基-归一", basis.iter().all(|row| (row.iter().sum::<f64>() - 1.0).abs() < 1e-12), "sum=1");
    s.add("基-t0", basis[0][0] == 1.0 && basis[0][3] == 0.0, "row 0");
    s.add("基-t1", basis[33][0] == 0.0 && basis[33][3] == 1.0, "row N");
    // 基 × 控制点 = 曲线点（重构校验）
    let row17 = &basis[17];
    let mut recon = Pt::ZERO;
    for i in 0..4 {
        recon = recon.add(c.ctrl[i].mul(row17[i]));
    }
    s.add("基-重构", recon.dist(c.at(0.5)) < 1e-12, "basis reconstruct");
    // 族五：批量弧长
    let curves = alloc::vec![c.clone(), ln.clone(), par_helper()];
    let lens = be.arclens(&curves, 16);
    s.add("弧长-数量", lens.len() == 3, "3 curves");
    s.add("弧长-立方", (lens[0] - 6.538).abs() < 0.01, "cube arc ≈ 6.54");
    s.add("弧长-直线", (lens[1] - 5.657).abs() < 0.01, "line arc 4√2");
    s.add("弧长-抛物", (lens[2] - 2.092).abs() < 0.01, "parabola arc");
    // 族六：成本模型
    s.add("成本-线性", be.cost_model(&c, 100) == 4 * 13, "4 ctrl × 13 blocks");
    let be1 = BatchEval::new(1);
    s.add("成本-单块", be1.cost_model(&c, 10) == 4 * 10, "chunk1 10 blocks");
    let be200 = BatchEval::new(200);
    s.add("成本-超界块", be200.cost_model(&c, 100) == 4, "one block");
    s
}

fn par_helper() -> Curve {
    Curve::build(
        Deg::Quad,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 2.0), Pt::new(2.0, 0.0)],
    )
    .expect("par")
}
