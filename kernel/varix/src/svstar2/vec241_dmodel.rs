//! VE-F0641 脏区追踪三档模型（VE-D 域 · 目标 380 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0641`
//!
//! **判据（锚点原文逐条）**：
//! - **三档语义**：帧级全重绘 / 矩形集档（默认）/ 精确档（远景预留显性 N/A）；
//! - **自动选档**：区域复杂度 + 成本模型（碎片化超限或覆盖率超阈 → 升档全帧）；
//! - **决策透明**：档位判定与理由入 F0649 度量可查；
//! - **预留诚实**：精确档未实现 → 显性 N/A 不假装可用；
//! - **判据**：矩形集失效 → 升档兜底。

use crate::checks::CheckSet;

/// 矩形（左、上、右、下，半开区间约定）
pub type Rect = (u32, u32, u32, u32);

/// 三档模型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirtyTier {
    /// 帧级全重绘（最保守）
    FullFrame,
    /// 矩形集档（默认）
    RectSet,
    /// 精确档（像素级差分——远景预留态）
    Precise,
}

impl DirtyTier {
    /// 精确档诚实声明：当前未实现，显性 N/A
    pub fn available(self) -> bool {
        self != DirtyTier::Precise
    }
    pub fn label(self) -> &'static str {
        match self {
            DirtyTier::FullFrame => "全帧",
            DirtyTier::RectSet => "矩形集",
            DirtyTier::Precise => "精确档(N/A)",
        }
    }
}

/// 选档策略表（阈值实测校准）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TierPolicy {
    /// 区域数超限（碎片化）→ 全帧
    pub frag_max: u32,
    /// 覆盖率超阈（矩形集面积占画面比）→ 全帧
    pub cover_max: f32,
    /// 画面面积（全帧基准）
    pub frame_area: u32,
}

pub const DEFAULT_POLICY: TierPolicy = TierPolicy {
    frag_max: 32,
    cover_max: 0.6,
    frame_area: 1_000_000,
};

/// 选档决策（附理由，决策透明）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TierDecision {
    pub tier: DirtyTier,
    pub reason: &'static str,
}

/// 核心选档：输入区域集，输出档位与理由
pub fn choose_tier(regions: &[Rect], policy: &TierPolicy) -> TierDecision {
    // 碎片化：区域数超限
    if regions.len() as u32 > policy.frag_max {
        return TierDecision { tier: DirtyTier::FullFrame, reason: "碎片化超限" };
    }
    // 覆盖率：矩形集总面积（含重叠，保守口径）占画面比
    let area: u64 = regions
        .iter()
        .map(|&(l, t, r, b)| {
            (r.saturating_sub(l) as u64) * (b.saturating_sub(t) as u64)
        })
        .sum();
    let cover = area as f32 / policy.frame_area as f32;
    if cover > policy.cover_max {
        return TierDecision { tier: DirtyTier::FullFrame, reason: "覆盖率超阈" };
    }
    TierDecision { tier: DirtyTier::RectSet, reason: "默认矩形集" }
}

/// 执行语义：矩形集档的实际重绘面积（重叠区域重复计入——保守执行口径）
pub fn rectset_draw_area(regions: &[Rect]) -> u64 {
    regions
        .iter()
        .map(|&(l, t, r, b)| (r.saturating_sub(l) as u64) * (b.saturating_sub(t) as u64))
        .sum()
}

/// 全帧档执行面积 = 画面
pub fn fullframe_draw_area(policy: &TierPolicy) -> u64 {
    policy.frame_area as u64
}

/// 升档兜底：矩形集执行成本超过全帧成本 → 建议升档（防"省区域反更贵"）
pub fn upgrade_check(regions: &[Rect], policy: &TierPolicy) -> bool {
    rectset_draw_area(regions) >= fullframe_draw_area(policy)
}

pub fn run_vec241_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec241");
    let pol = DEFAULT_POLICY;
    // 族一：三档语义
    s.add("三档-矩形集可用", DirtyTier::RectSet.available(), "rectset avail");
    s.add("三档-全帧可用", DirtyTier::FullFrame.available(), "fullframe avail");
    s.add("三档-精确档显性NA", !DirtyTier::Precise.available(), "precise N/A honest");
    s.add("三档-标签", DirtyTier::Precise.label().contains("N/A"), "na label");
    // 族二：自动选档——默认矩形集
    let d1 = choose_tier(&[(0, 0, 100, 100), (200, 200, 300, 300)], &pol);
    s.add("选档-默认矩形集", d1.tier == DirtyTier::RectSet, "default");
    s.add("选档-理由可查", d1.reason == "默认矩形集", "reason");
    // 族三：碎片化升档
    let frags: alloc::vec::Vec<Rect> = (0..40).map(|i| (i * 10, 0, i * 10 + 5, 5)).collect();
    let d2 = choose_tier(&frags, &pol);
    s.add("选档-碎片化升全帧", d2.tier == DirtyTier::FullFrame && d2.reason == "碎片化超限", "frag → full");
    // 族四：覆盖率升档
    let big = [(0u32, 0u32, 1000u32, 1000u32)];
    let d3 = choose_tier(&big, &pol);
    s.add("选档-覆盖率升全帧", d3.tier == DirtyTier::FullFrame && d3.reason == "覆盖率超阈", "cover → full");
    // 覆盖率临界：0.6 整（含重叠保守口径下不超）
    let edge = [(0u32, 0u32, 600u32, 1000u32)];
    let d4 = choose_tier(&edge, &pol);
    s.add("选档-临界不升", d4.tier == DirtyTier::RectSet, "≤0.6 keep");
    // 族五：执行面积
    let two = [(0u32, 0u32, 100u32, 100u32), (50u32, 0u32, 150u32, 100u32)];
    s.add("执行-重叠保守计", rectset_draw_area(&two) == 20000, "overlap counted");
    s.add("执行-全帧面积", fullframe_draw_area(&pol) == 1_000_000, "frame area");
    // 族六：升档兜底
    let huge = [(0u32, 0u32, 1100u32, 1000u32)];
    s.add("兜底-超全帧升档", upgrade_check(&huge, &pol), "rect cost ≥ full");
    s.add("兜底-正常不升", !upgrade_check(&[(0, 0, 10, 10)], &pol), "small keep");
    // 族七：决策透明（判定+理由成对）
    let ds = [d1, d2, d3];
    s.add("透明-理由非空", ds.iter().all(|d| !d.reason.is_empty()), "reasons paired");
    s
}
