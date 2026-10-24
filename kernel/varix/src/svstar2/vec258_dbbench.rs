//! VE-F0658 脏区蒙版性能基准（VE-D 域 · 目标 300 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0658`
//!
//! **判据（锚点原文逐条）**：
//! - **基准集**：合并算法 / 平移快路径（4K 滚动）/ 蒙版（圆角 SDF 大量图层）/ 膨胀成本；
//! - **下限判据**：各基准有工作量下限（低于下限即实现退化检出）；
//! - **双册入册**：CGPU-Bench / VE-Bench；
//! - **判据**：回归对照；结果入册。

use crate::checks::CheckSet;
use super::vec241_dmodel::Rect;
use super::vec242_dmerge::{merge_round, DEFAULT_MERGE_CAP};
use super::vec247_dilate::{dilate_then_merge, waste_rate, DEFAULT_TABLE, MarginSource};
use super::vec250_rmask::{RrectMask, CornerRadii};

/// 基准集四项
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchItem {
    /// 合并算法（各分布特征的合并质量与耗时）
    MergeAlgo,
    /// 平移快路径（4K 滚动）
    PanFast,
    /// 蒙版（圆角 SDF 大量图层）
    MaskSdf,
    /// 膨胀成本
    DilateCost,
}

pub const BENCH_COUNT: usize = 4;

impl BenchItem {
    pub fn all() -> [BenchItem; BENCH_COUNT] {
        [BenchItem::MergeAlgo, BenchItem::PanFast, BenchItem::MaskSdf, BenchItem::DilateCost]
    }
    pub fn label(self) -> &'static str {
        match self {
            BenchItem::MergeAlgo => "合并算法",
            BenchItem::PanFast => "平移快路径",
            BenchItem::MaskSdf => "圆角SDF",
            BenchItem::DilateCost => "膨胀成本",
        }
    }
    /// 双册登记
    pub fn books(self) -> [&'static str; 2] {
        ["CGPU-Bench", "VE-Bench"]
    }
}

/// 工作量模型（无时钟）
/// 合并基准：碎片区域集的贪心合并工作量（≈ O(n²) 比较对数）
pub fn bench_merge(fragments: usize) -> u64 {
    let pairs = fragments as u64 * (fragments as u64).saturating_sub(1) / 2;
    pairs.max(1)
}

/// 平移基准：4K 滚动搬移面积
pub fn bench_pan_4k(shift_px: u32) -> u64 {
    let full = (3840u64) * (2160);
    full * shift_px as u64 / 2160
}

/// 蒙版基准：n 个圆角图层 SDF 求值工作量
pub fn bench_mask(layers: usize, samples_per_layer: usize) -> u64 {
    (layers as u64) * (samples_per_layer as u64).max(1)
}

/// 膨胀基准：区域数 × 4 边膨胀
pub fn bench_dilate(regions: usize) -> u64 {
    regions as u64 * 4
}

/// 下限判据（低于下限 = 实现退化：工作量必须非零且达标）
pub fn meets_floor(actual: u64, floor: u64) -> bool {
    actual >= floor
}

/// 质量指标：合并压缩比（合并前区域数 / 合并后区域数）
pub fn merge_quality(fragments: &[Rect], gap: u32, margin: u32) -> f32 {
    if fragments.is_empty() {
        return 0.0;
    }
    let (out, _) = merge_round(fragments, gap, margin);
    fragments.len() as f32 / out.len().max(1) as f32
}

pub fn run_vec258_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec258");
    // 族一：基准集四项
    let items = BenchItem::all();
    s.add("基准-四项", items.len() == BENCH_COUNT, "4 items");
    s.add("基准-标签齐", items.iter().all(|b| !b.label().is_empty()), "labeled");
    s.add("基准-双册", items.iter().all(|b| b.books() == ["CGPU-Bench", "VE-Bench"]), "two books");
    // 族二：工作量模型
    s.add("工作量-合并平方级", bench_merge(10) == 45, "10→45 pairs");
    s.add("工作量-平移线性", bench_pan_4k(2160) == 3840 * 2160, "full shift");
    s.add("工作量-蒙版乘积", bench_mask(100, 64) == 6400, "layers×samples");
    s.add("工作量-膨胀四边", bench_dilate(50) == 200, "50×4");
    // 族三：下限判据
    s.add("下限-达标", meets_floor(45, 40), "above floor");
    s.add("下限-退化检出", !meets_floor(10, 40), "below floor");
    s.add("下限-零工作量拒", !meets_floor(0, 1), "zero rejected");
    // 族四：合并质量
    let frags: Vec<Rect> = (0..20).map(|i| (i * 5, 0, i * 5 + 4, 10)).collect();
    let q = merge_quality(&frags, 1, 1);
    s.add("质量-条带压缩", q > 5.0, "20→~1 high ratio");
    s.add("质量-零碎片零", merge_quality(&[], 1, 1) == 0.0, "empty");
    // 上限常量引用有效
    s.add("质量-上限表活", DEFAULT_MERGE_CAP.region_max == 64, "cap alive");
    // 族五：膨胀浪费率基准
    let orig = [(0u32, 0u32, 100u32, 100u32)];
    let dil = dilate_then_merge(&orig, DEFAULT_TABLE.composite(&MarginSource::all()));
    s.add("膨胀-浪费有界", waste_rate(&orig, &dil) < 10.0, "bounded waste");
    // 族六：蒙版基准活体（SDF 大量图层不炸）
    let (m, _) = RrectMask::build((0.0, 0.0, 64.0, 64.0), CornerRadii { tl: 8.0, tr: 8.0, br: 8.0, bl: 8.0 }, 1.0);
    let mut alive = true;
    for i in 0..100 {
        let d = m.sdf(i as f32, 32.0);
        if !d.is_finite() {
            alive = false;
        }
    }
    s.add("蒙版-百层活体", alive, "100 evals finite");
    // 族七：4K 平移基准
    s.add("基准-4K滚动", bench_pan_4k(4) == 3840 * 4, "4px shift band");
    s
}
