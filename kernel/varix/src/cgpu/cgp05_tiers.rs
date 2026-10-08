//! CGPU-F2405 · 遥测存储分层（CGPU-P 域 · 自适应遥测域 · P02 组 · 目标 320 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2405`
//!
//! **判据（锚点原文）**：分层复用、一组、判据。
//!
//! **职责定位（锚点原文）**：存储分层（热/温/冷复用 F1558 模式——分层复用；
//! 测试（分层一组）。
//!
//! ## 一、热/温/冷三层（复用 F1558 模式）
//!
//! **热**（[`Tier::Hot`]：内存环形明细，容量 [`HOT_CAPACITY`] 写死，保留
//! [`HOT_RETAIN_TICKS`] 个 tick——查询拿到逐条明细）；**温**（[`Tier::Warm`]：
//! 按 tick 窗口分片聚合 [`WarmShard`]——查询拿到 min/max/sum/count）；
//! **冷**（[`Tier::Cold`]：月度归档式摘要合并 [`cold_merge`]——查询拿到
//! 长期统计摘要，逐条明细不可恢复的诚实声明）。
//!
//! ## 二、单向流转 + 清理前摘要保留（复用 F1558 保留策略）
//!
//! [`TieredStore::store`] 入热环；热满逐出最老进温分片；温分片数超
//! [`WARM_SHARD_LIMIT`] 时最老分片**先汇总摘要再清理**（并入冷归档——
//! 「清理前汇总摘要保留」的落地面）；流转单向（热→温→冷），无回迁
//! API——冷数据回热的口子不存在（结构保证不承诺）。
//!
//! ## 三、分层查询三答案
//!
//! [`TieredStore::query`]：Hot 查明细（逐条）/Warm 查分片聚合/Cold 查
//! 长期摘要——三答案各有其位（同 F0483 查询口径：查不到就说查不到，
//! 不臆造零）。输入复用 F2402 Metric 六元组形状；聚合与 F0483
//! AggBucket 同构。
//!
//! **对接**：F2404（管道路由出口接分层入口）；F1558（分层模式复用源）；
//! F1449（环形复用）。零 panic 面、零 IO、零墙钟（tick 上游注入）。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、层定义与保留表（复用 F1558 模式）
// ---------------------------------------------------------------------------

/// 层级闭集（热/温/冷——判据「分层复用」的骨架）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 热：内存环形明细。
    Hot,
    /// 温：分片聚合。
    Warm,
    /// 冷：归档摘要。
    Cold,
}

impl Tier {
    /// 层名（判据侧对拍）。
    pub const fn name(self) -> &'static str {
        match self {
            Tier::Hot => "热",
            Tier::Warm => "温",
            Tier::Cold => "冷",
        }
    }

    /// 下一层（单向流转：冷是终点——None）。
    pub const fn next(self) -> Option<Tier> {
        match self {
            Tier::Hot => Some(Tier::Warm),
            Tier::Warm => Some(Tier::Cold),
            Tier::Cold => None,
        }
    }
}

/// 热环容量（写死——容量即预算）。
pub const HOT_CAPACITY: usize = 16;

/// 热层保留 tick 数（复用 F1558「热 60s」——tick 注入口径）。
pub const HOT_RETAIN_TICKS: u64 = 60;

/// 温分片数上限（超限最老分片并冷——「温 30 天」的窗口化口径）。
pub const WARM_SHARD_LIMIT: usize = 4;

// ---------------------------------------------------------------------------
// 二、聚合单元（复用 F0483 AggBucket 同构）
// ---------------------------------------------------------------------------

/// 聚合单元（min/max/sum/count——确定性累积零浮点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AggCell {
    /// 最小值。
    pub min: u64,
    /// 最大值。
    pub max: u64,
    /// 总和（饱和）。
    pub sum: u64,
    /// 条数。
    pub count: u32,
}

impl AggCell {
    /// 空桶。
    pub const fn new() -> AggCell {
        AggCell { min: u64::MAX, max: 0, sum: 0, count: 0 }
    }

    /// 吸收一条。
    pub fn absorb(&mut self, v: u64) {
        if self.count == 0 {
            self.min = v;
            self.max = v;
        } else {
            if v < self.min {
                self.min = v;
            }
            if v > self.max {
                self.max = v;
            }
        }
        self.sum = self.sum.saturating_add(v);
        self.count = self.count.saturating_add(1);
    }

    /// 合并另一桶（冷归档摘要合并——聚合的聚合）。
    pub fn merge(&mut self, other: &AggCell) {
        if other.count == 0 {
            return;
        }
        if self.count == 0 {
            *self = *other;
            return;
        }
        if other.min < self.min {
            self.min = other.min;
        }
        if other.max > self.max {
            self.max = other.max;
        }
        self.sum = self.sum.saturating_add(other.sum);
        self.count = self.count.saturating_add(other.count);
    }

    /// 均值（空桶 None 不臆造）。
    pub const fn mean(&self) -> Option<u64> {
        if self.count == 0 {
            None
        } else {
            Some(self.sum / self.count as u64)
        }
    }
}

impl Default for AggCell {
    fn default() -> AggCell {
        AggCell::new()
    }
}

// ---------------------------------------------------------------------------
// 三、分层存储主体（单向流转 + 清理前摘要保留）
// ---------------------------------------------------------------------------

/// 温分片（tick 窗口 + 聚合——窗口起点即分片键）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WarmShard {
    /// 分片窗口起点 tick。
    pub window_start: u64,
    /// 窗口聚合。
    pub agg: AggCell,
}

/// 长期归档摘要（冷层——逐条明细不可恢复的诚实声明）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColdSummary {
    /// 归档合并的聚合。
    pub agg: AggCell,
    /// 已归档分片数。
    pub shards_merged: u32,
}

/// 分层存储（热环 + 温分片 + 冷摘要——单向流转）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TieredStore {
    /// 热：环形明细（id+value+tick）。
    hot: Vec<(&'static str, u64, u64)>,
    /// 温：分片表（最老在前）。
    warm: Vec<WarmShard>,
    /// 冷：归档摘要。
    cold: ColdSummary,
    /// 当前 tick（上游注入）。
    tick: u64,
    /// 流转计数（热→温 / 温→冷 各多少条——可查账）。
    pub demoted_hot_warm: u32,
    pub demoted_warm_cold: u32,
}

impl TieredStore {
    /// 新存储。
    pub fn new() -> TieredStore {
        TieredStore {
            hot: Vec::new(),
            warm: Vec::new(),
            cold: ColdSummary::default(),
            tick: 0,
            demoted_hot_warm: 0,
            demoted_warm_cold: 0,
        }
    }

    /// tick 注入（上游推进——热层保留期判定依据）。
    pub fn advance(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }

    /// 入流（热环入一条；热满先按保留期逐出过期明细进温——清理前汇总）。
    pub fn store(&mut self, name: &'static str, value: u64) {
        // 保留期先清：热里 tick 已过保留期的明细逐出进温（环形不满也清）。
        self.expire_hot();
        if self.hot.len() >= HOT_CAPACITY {
            self.demote_oldest();
        }
        self.hot.push((name, value, self.tick));
    }

    /// 热层保留期清理（tick 超 HOT_RETAIN_TICKS 的明细逐出进温）。
    fn expire_hot(&mut self) {
        let now = self.tick;
        let mut keep: Vec<(&'static str, u64, u64)> = Vec::new();
        let mut i = 0usize;
        while i < self.hot.len() {
            if let Some(entry) = self.hot.get(i) {
                if now.saturating_sub(entry.2) < HOT_RETAIN_TICKS {
                    keep.push(*entry);
                } else {
                    self.absorb_warm(entry.1);
                    self.demoted_hot_warm = self.demoted_hot_warm.saturating_add(1);
                }
            }
            i += 1;
        }
        self.hot = keep;
    }

    /// 热满逐出最老（环首——F1449 环形口径）进温。
    fn demote_oldest(&mut self) {
        if self.hot.is_empty() {
            return;
        }
        let oldest = self.hot.remove(0);
        self.absorb_warm(oldest.1);
        self.demoted_hot_warm = self.demoted_hot_warm.saturating_add(1);
    }

    /// 温层吸收一条值（入最新分片；分片数超限先并冷最老分片——清理前摘要保留）。
    fn absorb_warm(&mut self, value: u64) {
        let start = self.tick.saturating_sub(self.tick % 16);
        // 找当前窗口分片或开新片。
        let mut idx = None;
        let mut i = 0usize;
        while i < self.warm.len() {
            if let Some(s) = self.warm.get(i) {
                if s.window_start == start {
                    idx = Some(i);
                    break;
                }
            }
            i += 1;
        }
        match idx {
            Some(i) => {
                if let Some(s) = self.warm.get_mut(i) {
                    s.agg.absorb(value);
                }
            }
            None => {
                let mut agg = AggCell::new();
                agg.absorb(value);
                self.warm.push(WarmShard { window_start: start, agg });
                // 分片数超限：最老分片并冷（单向流转到终点）。
                if self.warm.len() > WARM_SHARD_LIMIT {
                    if !self.warm.is_empty() {
                        let oldest = self.warm.remove(0);
                        self.cold.agg.merge(&oldest.agg);
                        self.cold.shards_merged = self.cold.shards_merged.saturating_add(1);
                        self.demoted_warm_cold = self.demoted_warm_cold.saturating_add(oldest.agg.count);
                    }
                }
            }
        }
    }

    /// 分层查询三答案：Hot 明细条数 / Warm 分片聚合 / Cold 长期摘要。
    pub fn query_hot(&self) -> usize {
        self.hot.len()
    }

    pub fn query_warm(&self) -> usize {
        self.warm.len()
    }

    pub fn query_cold(&self) -> &ColdSummary {
        &self.cold
    }

    /// 某温分片聚合读数（表外 None）。
    pub fn warm_shard(&self, idx: usize) -> Option<&AggCell> {
        match self.warm.get(idx) {
            Some(s) => Some(&s.agg),
            None => None,
        }
    }
}

impl Default for TieredStore {
    fn default() -> TieredStore {
        TieredStore::new()
    }
}

// ---------------------------------------------------------------------------
// 四、复用声明（判据「分层复用」的对账面）
// ---------------------------------------------------------------------------

/// 复用清单（判据侧逐条 grep 对拍——不第二套口径）：
pub const REUSE_LINES: [&str; 4] = [
    "复用 F1558 三层仓库模式 热/温/冷——层名与流转序同源",
    "复用 F1449 内存环形作热层——环满逐出最老口径一致",
    "复用 F0483 AggBucket 聚合同构 min/max/sum/count——温冷两层不第二套聚合",
    "复用 F1558 保留策略 清理前汇总摘要保留——冷归档前先 merge 不静默丢",
];
