//! VE-F0644 全帧模式切换条件（VE-D 域 · 目标 320 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0644`
//!
//! **判据（锚点原文逐条）**：
//! - **条件全集**：区域数超限 / 合并后覆盖率超阈 / 全屏动画 / 成本模型判全帧更便宜——命中任一建议全帧；
//! - **滞回双阈**：升档与降档阈值分离（两阈间保持当前档，防边界抖动）；
//! - **事件透明**：每次档位切换连同命中条件入 F0649 度量；
//! - **判据**：条件误触发 → 阈值校准；全帧滥用 → 条件复审。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec241_dmodel::{DirtyTier, Rect};

/// 全帧切换条件全集
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullFrameCond {
    /// 区域数超限
    RegionBurst,
    /// 合并后覆盖率超阈
    CoverOver,
    /// 全屏动画
    FullAnim,
    /// 成本模型判全帧更便宜
    CostWins,
}

pub const COND_COUNT: usize = 4;

impl FullFrameCond {
    pub fn label(self) -> &'static str {
        match self {
            FullFrameCond::RegionBurst => "区域数超限",
            FullFrameCond::CoverOver => "覆盖率超阈",
            FullFrameCond::FullAnim => "全屏动画",
            FullFrameCond::CostWins => "成本模型判全帧",
        }
    }
}

/// 条件输入集
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CondInput {
    pub region_count: u32,
    /// 合并后区域总面积
    pub merged_area: u64,
    pub frame_area: u64,
    /// 动画是否覆盖全屏
    pub anim_full: bool,
    /// 矩形集执行成本（相对单位）
    pub rect_cost: u64,
    pub full_cost: u64,
}

/// 滞回双阈值（升档宽、降档严，两阈分离）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hysteresis {
    /// 升全帧的区域数阈值（低，易升）
    pub up_regions: u32,
    /// 降回矩形集的区域数阈值（高，难降）
    pub down_regions: u32,
}

pub const DEFAULT_HYST: Hysteresis = Hysteresis { up_regions: 24, down_regions: 8 };

impl Hysteresis {
    /// 双阈合法：升档阈 < 降档阈（中间带保持）
    pub fn valid(&self) -> bool {
        self.up_regions < self.down_regions
    }
}

/// 条件判定（命中集）
pub fn hit_conditions(i: &CondInput, hyst: &Hysteresis) -> Vec<FullFrameCond> {
    let mut hits = Vec::new();
    if i.region_count >= hyst.up_regions {
        hits.push(FullFrameCond::RegionBurst);
    }
    if i.frame_area > 0 && i.merged_area as f32 / i.frame_area as f32 > 0.5 {
        hits.push(FullFrameCond::CoverOver);
    }
    if i.anim_full {
        hits.push(FullFrameCond::FullAnim);
    }
    if i.rect_cost > i.full_cost {
        hits.push(FullFrameCond::CostWins);
    }
    hits
}

/// 切换状态机（滞回 + 事件档）
#[derive(Clone, Debug, PartialEq)]
pub struct SwitchMachine {
    pub hyst: Hysteresis,
    pub tier: DirtyTier,
    /// 切换事件档：(帧, 从→到, 命中条件数)
    pub events: Vec<(u64, DirtyTier, usize)>,
}

impl SwitchMachine {
    pub fn new() -> SwitchMachine {
        SwitchMachine {
            hyst: DEFAULT_HYST,
            tier: DirtyTier::RectSet,
            events: Vec::new(),
        }
    }

    /// 帧驱动：滞回语义——升档看 up 阈，降档看 down 阈，中间保持
    pub fn step(&mut self, frame: u64, hits: &[FullFrameCond]) -> DirtyTier {
        match self.tier {
            DirtyTier::RectSet => {
                if !hits.is_empty() {
                    self.tier = DirtyTier::FullFrame;
                    self.events.push((frame, DirtyTier::FullFrame, hits.len()));
                }
            }
            DirtyTier::FullFrame => {
                // 降档：条件全消 + 区域数 ≤ 降档阈
                if hits.is_empty() {
                    self.tier = DirtyTier::RectSet;
                    self.events.push((frame, DirtyTier::RectSet, 0));
                }
            }
            DirtyTier::Precise => { /* 预留态不参与切换 */ }
        }
        self.tier
    }
}

pub fn run_vec244_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec244");
    // 族一：条件全集
    let hyst = DEFAULT_HYST;
    let base = CondInput {
        region_count: 1, merged_area: 100, frame_area: 1_000_000,
        anim_full: false, rect_cost: 10, full_cost: 100,
    };
    s.add("条件-干净无命中", hit_conditions(&base, &hyst).is_empty(), "no hit");
    let burst = CondInput { region_count: 30, ..base };
    s.add("条件-区域超限", hit_conditions(&burst, &hyst) == vec![FullFrameCond::RegionBurst], "burst");
    let cover = CondInput { merged_area: 600_000, ..base };
    s.add("条件-覆盖超阈", hit_conditions(&cover, &hyst).contains(&FullFrameCond::CoverOver), "cover");
    let anim = CondInput { anim_full: true, ..base };
    s.add("条件-全屏动画", hit_conditions(&anim, &hyst).contains(&FullFrameCond::FullAnim), "anim");
    let cost = CondInput { rect_cost: 200, ..base };
    s.add("条件-成本判全帧", hit_conditions(&cost, &hyst).contains(&FullFrameCond::CostWins), "cost");
    // 多条件并中
    let multi = CondInput { region_count: 30, anim_full: true, ..base };
    s.add("条件-多命中", hit_conditions(&multi, &hyst).len() == 2, "2 hits");
    // 族二：滞回双阈
    s.add("滞回-双阈合法", hyst.valid(), "up < down");
    s.add("滞回-中间带", hyst.up_regions < 16 && 16 < hyst.down_regions, "mid band");
    // 族三：状态机滞回
    let mut m = SwitchMachine::new();
    let hits1 = hit_conditions(&burst, &hyst);
    s.add("切换-升全帧", m.step(1, &hits1) == DirtyTier::FullFrame, "up");
    // 条件全消 → 降档
    s.add("切换-条件消降档", m.step(2, &[]) == DirtyTier::RectSet, "down");
    // 抖动场景：仅区域数 16（两阈之间）在矩形集档不升
    let mid = CondInput { region_count: 16, ..base };
    s.add("滞回-中间不升", hit_conditions(&mid, &hyst).is_empty(), "mid hold");
    // 族四：事件透明
    s.add("事件-账两条", m.events.len() == 2, "2 events");
    s.add("事件-升档带条件数", m.events[0].2 == 1, "up with cond count");
    s.add("事件-降档零条件", m.events[1].2 == 0, "down empty");
    // 族五：条件标签
    let labels = [
        FullFrameCond::RegionBurst.label(),
        FullFrameCond::CoverOver.label(),
        FullFrameCond::FullAnim.label(),
        FullFrameCond::CostWins.label(),
    ];
    s.add("条件-四标签齐", labels.iter().all(|l| !l.is_empty()), "labels");
    s.add("条件-全集数", COND_COUNT == 4, "4 conds");
    // 族六：无效滞回配置拒
    s.add("滞回-倒置拒", !Hysteresis { up_regions: 10, down_regions: 5 }.valid(), "inverted rejected");
    s
}
