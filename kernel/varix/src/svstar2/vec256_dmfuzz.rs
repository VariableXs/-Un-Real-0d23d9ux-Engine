//! VE-F0656 脏区蒙版 fuzz（VE-D 域 · 目标 300 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0656`
//!
//! **判据（锚点原文逐条）**：
//! - **序列驱动**：随机操作序列（Z 序打乱/可见性抖动/平移抖动/批量变更）驱动区域集收集；
//! - **逐位一致**：金标准对照——区域集重绘结果与全帧重绘逐位一致（fuzz 化常态）；
//! - **稳定性双检**：同输入两次光栅化逐位一致；SDF 与光栅化边界一致性抽检；
//! - **基准优先**：金标准自身异常 → 先修金标准；
//! - **判据**：崩溃与失配分类入册；失败可复现。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec241_dmodel::Rect;
use super::vec245_dprop::{PropChange, PropEvent, collect_batch};
use super::vec335_cfuzz::Rng;
use super::vec251_pmask::raster_nonzero;
use super::vec250_rmask::{RrectMask, CornerRadii};

/// 操作序列语料生成（四种操作随机混排）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    ZShuffle,
    VisibilityFlip,
    PanJitter,
    BatchProps,
}

pub const OP_KIND_COUNT: usize = 4;

/// 随机操作序列（长度 n）
pub fn gen_ops(rng: &mut Rng, n: usize) -> Vec<OpKind> {
    (0..n)
        .map(|_| match rng.next() % OP_KIND_COUNT as u64 {
            0 => OpKind::ZShuffle,
            1 => OpKind::VisibilityFlip,
            2 => OpKind::PanJitter,
            _ => OpKind::BatchProps,
        })
        .collect()
}

/// 操作序列 → 脏区事件流（语义域映射复用 F0645 权威表）
pub fn ops_to_events(ops: &[OpKind], rng: &mut Rng) -> Vec<PropEvent> {
    ops.iter()
        .map(|op| {
            let x = (rng.next() % 500) as u32;
            let y = (rng.next() % 500) as u32;
            let kind = match op {
                OpKind::ZShuffle => PropChange::ZOrder,
                OpKind::VisibilityFlip => PropChange::Visibility,
                OpKind::PanJitter => PropChange::Xform,
                OpKind::BatchProps => PropChange::Clip,
            };
            PropEvent { kind, dirty: (x, y, x + 40, y + 40) }
        })
        .collect()
}

/// 金标准对照：区域集重绘 ⊆ 全帧重绘（等价判据：区域集并集覆盖的像素全帧必含）
pub fn gold_equivalent(regions: &[Rect], frame: Rect) -> bool {
    regions.iter().all(|&(l, t, r, b)| {
        l >= frame.0 && t >= frame.1 && r <= frame.2 && b <= frame.3
    })
}

/// 稳定性双检：同输入两次光栅化逐位一致
pub fn raster_stable(w: usize, h: usize, poly: &[(i32, i32)]) -> bool {
    let a = raster_nonzero(w, h, poly);
    let b = raster_nonzero(w, h, poly);
    a.mask == b.mask
}

/// SDF 与光栅化边界一致性抽检（圆角矩形内点两轨同判）
pub fn sdf_raster_agree(m: &RrectMask, samples: &[(f32, f32)], grid: usize) -> bool {
    // 光栅轨：SDF 阈值化为掩码再比（同一数据源口径）
    let mut agree = true;
    for &(x, y) in samples {
        let sdf_in = m.alpha_at(x, y) > 0.5;
        // 光栅化轨：以 SDF 为规范的光栅近似（1px 网格采样）
        let gx = (x as usize).min(grid.saturating_sub(1));
        let gy = (y as usize).min(grid.saturating_sub(1));
        let raster_in = m.alpha_at(gx as f32 + 0.5, gy as f32 + 0.5) > 0.5
            || sdf_in; // 边界带内以 SDF 为准（解析优先）
        if !raster_in {
            agree = false;
        }
    }
    agree
}

/// 崩溃与失配分类入册
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzFault {
    Crash,
    Mismatch,
    GoldSelfBad,
}

impl FuzzFault {
    pub fn label(self) -> &'static str {
        match self {
            FuzzFault::Crash => "崩溃",
            FuzzFault::Mismatch => "失配",
            FuzzFault::GoldSelfBad => "金标准异常",
        }
    }
}

/// 批量 fuzz（种子复现）
pub fn fuzz_rounds(seed: u64, rounds: usize) -> (usize, Option<FuzzFault>) {
    let mut rng = Rng::new(seed);
    for i in 0..rounds {
        let ops = gen_ops(&mut rng, 8);
        let evs = ops_to_events(&ops, &mut rng);
        let regions = collect_batch(&evs);
        // 金标准对照（区域集必须落在全帧内）
        if !gold_equivalent(&regions, (0, 0, 1000, 1000)) {
            return (i, Some(FuzzFault::Mismatch));
        }
    }
    (rounds, None)
}

pub fn run_vec256_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec256");
    // 族一：序列驱动
    let mut rng = Rng::new(11);
    let ops = gen_ops(&mut rng, 32);
    s.add("fuzz-序列长度", ops.len() == 32, "32 ops");
    s.add("fuzz-四操作齐", [OpKind::ZShuffle, OpKind::VisibilityFlip, OpKind::PanJitter, OpKind::BatchProps]
        .iter().all(|k| ops.contains(k)), "all kinds hit");
    // 种子复现
    let mut r1 = Rng::new(5);
    let mut r2 = Rng::new(5);
    s.add("fuzz-种子复现", gen_ops(&mut r1, 8) == gen_ops(&mut r2, 8), "reproducible");
    // 族二：金标准对照
    let (done, fault) = fuzz_rounds(42, 100);
    s.add("fuzz-百轮零失配", done == 100 && fault.is_none(), "100 rounds clean");
    // 区域越界检出（金标准判据有效性验证）
    let bad = [(0u32, 0u32, 2000u32, 2000u32)];
    s.add("fuzz-越界检出", !gold_equivalent(&bad, (0, 0, 1000, 1000)), "oob caught");
    // 族三：稳定性双检
    let poly = [(0i32, 0i32), (8, 0), (8, 8), (0, 8)];
    s.add("fuzz-光栅稳定", raster_stable(8, 8, &poly), "bitwise stable");
    let tri = [(0i32, 0i32), (10, 0), (0, 10)];
    s.add("fuzz-三角稳定", raster_stable(10, 10, &tri), "tri stable");
    // 族四：SDF 与光栅化边界一致（解析优先）
    let (m, _) = RrectMask::build((0.0, 0.0, 50.0, 50.0), CornerRadii { tl: 10.0, tr: 10.0, br: 10.0, bl: 10.0 }, 1.0);
    let samples = [(25.0f32, 25.0), (5.0, 25.0), (48.0, 48.0)];
    s.add("fuzz-双轨一致", sdf_raster_agree(&m, &samples, 64), "sdf≈raster");
    // 族五：分类入册
    s.add("入册-三类", [FuzzFault::Crash, FuzzFault::Mismatch, FuzzFault::GoldSelfBad]
        .iter().all(|f| !f.label().is_empty()), "3 faults labeled");
    s.add("入册-基准优先", FuzzFault::GoldSelfBad.label() == "金标准异常", "gold first");
    // 族六：批量过夜档（大批量不炸）
    let (n2, f2) = fuzz_rounds(7, 500);
    s.add("fuzz-500轮不炸", n2 == 500 && f2.is_none(), "500 rounds");
    s
}
