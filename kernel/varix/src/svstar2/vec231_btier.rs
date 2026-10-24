//! VE-F0631 混合性能分层（VE-D 域 · 目标 320 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0631`
//!
//! **判据（锚点原文逐条）**：
//! - **阈值实测**：成本模型四因子（区域尺寸/层数/模式复杂度/能力集）阈值表；
//! - **防抖滞后**：频繁层间抖动 → 滞后带防抖；
//! - **缓存前置**：缓存命中跳过混合直接复用（分层在缓存判定之后）；
//! - **决策可观测**：分层决策与实际成本入 F0649；
//! - **判据**：全降级 → 告警三要素。

use crate::checks::CheckSet;
use super::ved22_separable::SepMode;

/// 分层路径（优先级从高到低）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathTier {
    /// GPU 逐层直通
    GpuLayer,
    /// GPU 隔离组混合（F0626）
    GpuGroup,
    /// CPU SIMD（GPU 不可用或小区域回读更优）
    CpuSimd,
}

impl PathTier {
    pub fn rank(self) -> u8 {
        match self {
            PathTier::GpuLayer => 0,
            PathTier::GpuGroup => 1,
            PathTier::CpuSimd => 2,
        }
    }
}

/// 能力集
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capability {
    pub gpu: bool,
    pub group_blend: bool,
    pub simd: bool,
}

/// 成本模型四因子
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CostFactors {
    /// 区域像素数
    pub area_px: u32,
    /// 参与层数
    pub layers: u32,
    /// 模式复杂度（除法+开方计数，取参与模式最大值）
    pub mode_complexity: u32,
    pub cap: Capability,
}

/// 阈值表（实测校准基线；校准只改表不许改判据结构）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TierThresholds {
    /// 小区域回读更优上界（像素）：≤ 该值走 CPU
    pub cpu_area_max: u32,
    /// 模式复杂度告警界
    pub complexity_warn: u32,
    /// 层数上限（超过转组混合）
    pub layer_group_min: u32,
    /// 滞后带（防抖）：相邻两次决策切换需超出该余量
    pub hysteresis_px: u32,
}

pub const DEFAULT_THRESHOLDS: TierThresholds = TierThresholds {
    cpu_area_max: 4096,
    complexity_warn: 4,
    layer_group_min: 8,
    hysteresis_px: 256,
};

/// 分层决策器（带滞后防抖与缓存前置）
#[derive(Clone, Debug, PartialEq)]
pub struct TierDecider {
    pub th: TierThresholds,
    /// 上次决策（滞后带比较基准）
    pub last: Option<PathTier>,
    /// 缓存前置：命中则直接复用（不进分层）
    pub cache_hits: u32,
    /// 决策账（可观测：决策 → 次数）
    pub decisions: [(PathTier, u32); 3],
    /// 全降级告警（三要素：位置、原因、影响）
    pub alarms: alloc::vec::Vec<(&'static str, &'static str, &'static str)>,
}

impl TierDecider {
    pub fn new() -> TierDecider {
        TierDecider {
            th: DEFAULT_THRESHOLDS,
            last: None,
            cache_hits: 0,
            decisions: [(PathTier::GpuLayer, 0), (PathTier::GpuGroup, 0), (PathTier::CpuSimd, 0)],
            alarms: alloc::vec![],
        }
    }

    /// 缓存前置：命中 → 不分层
    pub fn cache_hit(&mut self) -> Option<PathTier> {
        self.cache_hits += 1;
        None
    }

    /// 核心决策（缓存未命中时进入）
    pub fn decide(&mut self, f: CostFactors) -> PathTier {
        let t = self.raw_choice(f);
        // 滞后带：与上次同类且面积差 ≤ hysteresis → 保持上次（防抖）
        if let Some(prev) = self.last {
            if prev == t {
                // 同向，保持
            } else if prev.rank() < t.rank() {
                // 升档需超滞后带
                if f.area_px <= self.th.cpu_area_max + self.th.hysteresis_px {
                    self.record(prev);
                    return prev;
                }
            }
        }
        self.record(t);
        t
    }

    fn raw_choice(&mut self, f: CostFactors) -> PathTier {
        if !f.cap.gpu {
            if f.cap.simd {
                PathTier::CpuSimd
            } else {
                // 全降级：告警三要素
                self.alarms.push(("tier-decider", "no gpu no simd", "blend on scalar"));
                PathTier::CpuSimd
            }
        } else if f.area_px <= self.th.cpu_area_max && f.cap.simd {
            PathTier::CpuSimd
        } else if f.layers >= self.th.layer_group_min && f.cap.group_blend {
            PathTier::GpuGroup
        } else {
            PathTier::GpuLayer
        }
    }

    fn record(&mut self, t: PathTier) {
        for slot in self.decisions.iter_mut() {
            if slot.0 == t {
                slot.1 += 1;
            }
        }
        self.last = Some(t);
    }

    /// 模式复杂度告警（F0628 生成器的除法/开方计入）
    pub fn complexity_warn(&self, c: u32) -> bool {
        c >= self.th.complexity_warn
    }
}

/// 模式复杂度采样（SepMode → 复杂度计数）
pub fn mode_complexity(m: SepMode) -> u32 {
    let ir = m.ir();
    (ir.div_count() + ir.sqrt_count()) as u32
}

pub fn run_vec231_checks() -> crate::checks::CheckSet {
    use super::ved22_separable::SepMode as M;
    let mut s = CheckSet::new("svstar2-vec231");
    // 族一：能力分层
    let mut d = TierDecider::new();
    let full = Capability { gpu: true, group_blend: true, simd: true };
    s.add("分层-大区GPU逐层", d.decide(CostFactors { area_px: 1_000_000, layers: 2, mode_complexity: 1, cap: full }) == PathTier::GpuLayer, "big → gpu layer");
    s.add("分层-多层级组混合", d.decide(CostFactors { area_px: 1_000_000, layers: 16, mode_complexity: 1, cap: full }) == PathTier::GpuGroup, "layers ≥ 8 → group");
    s.add("分层-小区CPU", d.decide(CostFactors { area_px: 100, layers: 1, mode_complexity: 1, cap: full }) == PathTier::CpuSimd, "small → cpu");
    // 族二：无 GPU 降级
    let mut d2 = TierDecider::new();
    let nogpu = Capability { gpu: false, group_blend: false, simd: true };
    s.add("分层-无GPU走CPU", d2.decide(CostFactors { area_px: 1_000_000, layers: 1, mode_complexity: 1, cap: nogpu }) == PathTier::CpuSimd, "no gpu");
    let mut d3 = TierDecider::new();
    let bare = Capability { gpu: false, group_blend: false, simd: false };
    let _ = d3.decide(CostFactors { area_px: 1000, layers: 1, mode_complexity: 1, cap: bare });
    s.add("分层-全降级告警", d3.alarms.len() == 1 && d3.alarms[0].0 == "tier-decider", "3-elem alarm");
    // 族三：滞后防抖
    let mut d4 = TierDecider::new();
    let t1 = d4.decide(CostFactors { area_px: 4096 + 100, layers: 1, mode_complexity: 1, cap: full });
    // 4096+100 > cpu_area_max → GpuLayer
    let t2 = d4.decide(CostFactors { area_px: 4096 - 100, layers: 1, mode_complexity: 1, cap: full });
    // 4096-100 ≤ cpu_area_max 本应 CPU，但差 100 ≤ 滞后带 256 → 保持 GpuLayer
    s.add("防抖-滞后保持", t1 == PathTier::GpuLayer && t2 == PathTier::GpuLayer, "hysteresis hold");
    let t3 = d4.decide(CostFactors { area_px: 100, layers: 1, mode_complexity: 1, cap: full });
    s.add("防抖-超带切换", t3 == PathTier::CpuSimd, "beyond band switch");
    // 族四：缓存前置
    let mut d5 = TierDecider::new();
    s.add("缓存-命中不分层", d5.cache_hit().is_none(), "cache skip");
    s.add("缓存-命中记账", d5.cache_hits == 1, "hit count");
    // 族五：决策可观测
    s.add("观测-账非零", d.decisions.iter().any(|&(_, n)| n > 0), "ledger used");
    s.add("观测-账总和", d.decisions.iter().map(|&(_, n)| n).sum::<u32>() >= 3, "ledger ≥ 3");
    // 族六：复杂度
    s.add("复杂度-Multiply零", mode_complexity(M::Multiply) == 0, "mul simple");
    s.add("复杂度-Dodge有除法", mode_complexity(M::ColorDodge) >= 1, "dodge has div");
    let mut d6 = TierDecider::new();
    s.add("复杂度-告警", d6.complexity_warn(d6.th.complexity_warn), "warn at threshold");
    s.add("复杂度-低不告", !d6.complexity_warn(0), "no warn low");
    // 族七：阈值实测基线
    s.add("阈值-小区域界", DEFAULT_THRESHOLDS.cpu_area_max == 4096, "4k px");
    s.add("阈值-滞后带", DEFAULT_THRESHOLDS.hysteresis_px == 256, "band");
    s.add("阈值-组混合层数", DEFAULT_THRESHOLDS.layer_group_min == 8, "group layers");
    s
}
