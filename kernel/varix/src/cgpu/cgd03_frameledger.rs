//! CGPU-F0483 · 帧时间账本（CGPU-D 域 · 帧预算与计时域 · D01 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0483`
//!
//! **判据（锚点原文）**：完整记录、同 schema、滚动、查询、判据。
//!
//! **职责定位（锚点原文）**：帧账本：每帧完整记录（四段耗时+帧间隔+
//! 场景标签——逐帧账本）；账本与 B08 同 schema（遥测一致——不第二套
//! 口径）；账本滚动窗口（内存配额内保留近期+聚合历史）；账本查询
//! （任意帧回查）。
//!
//! ## 一、完整记录：缺段也是事实，如实入账
//!
//! 每帧记录四段耗时+帧间隔+场景标签（判据一）。四段可能缺账（上游
//! 计时器未闭段给 None）——缺段帧**照记**，缺哪段记 None，不补造
//! 不跳过：账本的第一美德是如实，「完整记录」指字段结构完整（六项
//! 逐帧都有账位），不指强行填满数值。帧健康三级分类（[`FrameHealth`]：
//! 齐备/缺段/全缺）随帧入账——健康度是账本的一等事实，不是查询时
//! 现算的派生。
//!
//! ## 二、同 schema：五元组单源，不第二套口径
//!
//! 每条帧指标以 F0274/B08 的五元组（名称/维度/单位/类型/采样策略）
//! 表达（[`METRIC_SCHEMA`]），三级命名 `d01.frameledger.<指标>`（判据二
//! 「不第二套口径」的落地面）：账本不是自造格式，是遥测 schema 在帧
//! 维度上的实例化——schema 版本进记录（版本化演进不破坏历史数据），
//! 记录行与 schema 表逐字段对账（判据侧独立重排，错一字先红）。
//! 快照导出（[`FrameLedger::export_snapshot`]）按 schema 逐行展开，
//! 消费方（B08 聚合器）拿到的每一行都能 grep 回 schema 表。
//!
//! ## 三、滚动：近期逐帧，历史聚合，配额恒定
//!
//! 内存配额写死（[`RECENT_CAPACITY`] 帧逐帧保留）；超限最旧帧从逐帧
//! 层退场——退场不是一步到底，而是**两级滚动**：先并入中层桶
//! （[`MidBucket`]，每 [`MID_BUCKET_SIZE`] 帧一桶，保留桶内四段分段
//! min/max/sum 与场景计数），中层桶满再压缩进总聚合
//! （[`AggBucket`]：count/min/max/sum）——历史帧查不到逐帧明细但
//! **查得到两级聚合事实**（判据三：聚合不是丢弃，是换了精度继续
//! 记账）；配额恒定⇒账本内存有界（长跑不膨胀）。分场景计数
//! （[`SceneCounter`]）随退场累计：场景维度的账不因逐帧退场而丢。
//!
//! ## 四、查询：任意帧多种答案
//!
//! 任意帧回查（判据四）结论各有其位：近期帧给 `Exact`（逐帧全
//! 账）、已退场帧给其所属中层桶的聚合（`MidAggregated`）、已压缩帧给
//! 总聚合（`Aggregated`）、从未记录给 `NotFound`（不臆造）——四种
//! 都是正确答案，没有「查不到就说零」。区间查询（[`FrameLedger::range_stats`]）
//! 给帧区间四段分段统计；最近一帧（[`FrameLedger::latest`]）给末帧
//! 快照；场景回查（[`FrameLedger::scene_count`]）给场景累计帧数。
//!
//! **对接**：F0482（四段耗时来源）；F0274/B08（遥测 schema 同源）；
//! F0484（帧间隔统计，消费间隔数据——jank 标记随帧入账）。
//! 零 panic 面（下标走 `get`/`Option`，算术饱和）、零 IO、零墙钟
//! （时间戳上游注入）、无全局可变状态、no_std 零 std 依赖。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（判据三的配额口径唯一源）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const FRAME_LEDGER_VERSION: &str = "D01-frameledger-v1";

/// 遥测 schema 版本（B08/F0274 版本化纪律：演进不破坏历史数据可读）。
pub const SCHEMA_VERSION: u32 = 1;

/// 近期逐帧保留容量（内存配额；恒定——长跑不膨胀）。
pub const RECENT_CAPACITY: usize = 256;

/// 帧指标条数（四段耗时+帧间隔=五条数值指标/帧）。
pub const METRICS_PER_FRAME: usize = 5;

/// 中层桶大小（每桶帧数；两级滚动的中层粒度）。
pub const MID_BUCKET_SIZE: u64 = 64;

/// 中层桶保留数（中层容量恒定——满桶压缩进总聚合）。
pub const MID_BUCKET_CAPACITY: usize = 8;

/// jank 判定线（纳秒）：帧间隔 > 1.5 × 12.5ms = 18.75ms 记微卡
/// （来源：F0484「>1.5× 目标间隔=微卡」——账本侧只做标记，分级归
/// F0484）。当前策略值，可调。
pub const JANK_MARK_NS: u64 = 18_750_000;

/// 四段耗时口径上限（单段 > 1 秒视为上游计时异常——如实入账但标
/// 异常旗；不静默丢弃）。
pub const STAGE_SANITY_CAP_NS: u64 = 1_000_000_000;

/// 四段中文名（schema 对拍与报告面共用）。
pub const STAGE_NAMES: [&str; 4] = ["输入采样", "提交", "GPU 执行", "呈现"];

// ---------------------------------------------------------------------------
// 二、同 schema：五元组与帧指标 schema 表（判据二）
// ---------------------------------------------------------------------------

/// 采样策略闭集（F0274 五元组第五元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sampling {
    /// 每帧全采。
    EveryFrame,
}

/// 遥测五元组（B08/F0274 同构：名称/维度/单位/类型/采样策略——同源
/// 不重抄口径，本表是 schema 在帧维度的实例化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricTuple {
    /// 三级命名（域.组.指标）。
    pub name: &'static str,
    /// 维度（按帧）。
    pub dimension: &'static str,
    /// 单位。
    pub unit: &'static str,
    /// 类型。
    pub kind: &'static str,
    /// 采样策略。
    pub sampling: Sampling,
}

/// 帧账本指标 schema 表（判据二：账本行与表逐字段对账——不第二套口径）。
pub const METRIC_SCHEMA: [MetricTuple; METRICS_PER_FRAME] = [
    MetricTuple {
        name: "d01.frameledger.input_sampling_ns",
        dimension: "frame",
        unit: "ns",
        kind: "u64",
        sampling: Sampling::EveryFrame,
    },
    MetricTuple {
        name: "d01.frameledger.submit_ns",
        dimension: "frame",
        unit: "ns",
        kind: "u64",
        sampling: Sampling::EveryFrame,
    },
    MetricTuple {
        name: "d01.frameledger.gpu_execute_ns",
        dimension: "frame",
        unit: "ns",
        kind: "u64",
        sampling: Sampling::EveryFrame,
    },
    MetricTuple {
        name: "d01.frameledger.present_ns",
        dimension: "frame",
        unit: "ns",
        kind: "u64",
        sampling: Sampling::EveryFrame,
    },
    MetricTuple {
        name: "d01.frameledger.frame_interval_ns",
        dimension: "frame",
        unit: "ns",
        kind: "u64",
        sampling: Sampling::EveryFrame,
    },
];

/// 三级命名前缀（判据二：域.组.指标——前缀可 grep 不可漂移）。
pub const METRIC_PREFIX: &str = "d01.frameledger.";

// ---------------------------------------------------------------------------
// 三、帧健康分类与帧记录（判据一）
// ---------------------------------------------------------------------------

/// 帧健康三级分类（随帧入账的一等事实）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameHealth {
    /// 四段齐备。
    Complete,
    /// 部分段缺账。
    Partial,
    /// 四段全缺（只有间隔与标签——上游计时全断）。
    Empty,
}

impl FrameHealth {
    /// 从四段事实分类。
    pub const fn classify(stage_ns: &[Option<u64>; 4]) -> FrameHealth {
        let mut present = 0u8;
        let mut i = 0usize;
        while i < 4 {
            if matches!(stage_ns[i], Some(_)) {
                present += 1;
            }
            i += 1;
        }
        match present {
            4 => FrameHealth::Complete,
            0 => FrameHealth::Empty,
            _ => FrameHealth::Partial,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            FrameHealth::Complete => "齐备",
            FrameHealth::Partial => "缺段",
            FrameHealth::Empty => "全缺",
        }
    }
}

/// 逐帧记录（六项结构完整：id+四段+间隔+标签+版本 + 健康分类 +
/// jank 标记 + 异常旗）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameRecord {
    /// 帧编号。
    pub frame_id: u64,
    /// 四段耗时（纳秒；缺段 None——如实不补造）。
    pub stage_ns: [Option<u64>; 4],
    /// 帧间隔（纳秒；首帧或未测 None）。
    pub interval_ns: Option<u64>,
    /// 场景标签。
    pub scene: String,
    /// 记录时 schema 版本（版本化——旧数据可读）。
    pub schema_version: u32,
    /// 帧健康分类。
    pub health: FrameHealth,
    /// jank 标记（间隔 > [`JANK_MARK_NS`]——F0484 的上游事实源）。
    pub jank: bool,
    /// 单段超口径异常旗（位图：bit i = 第 i 段超 [`STAGE_SANITY_CAP_NS`]）。
    pub stage_anomaly_bits: u8,
}

impl FrameRecord {
    /// 四段齐备（缺段帧不算完整数值账，但结构账在——供查询侧区分）。
    pub const fn stages_complete(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            // 固定长 4 数组界内索引（i<4 恒成立），const 面不支持 slice::get。
            if matches!(self.stage_ns[i], Some(_)) {
                i += 1;
                continue;
            }
            return false;
        }
        true
    }

    /// 记录行展开为五元组指标值（同 schema 的运行时面：值与 schema 表
    /// 按位对齐——第 i 个值对应 METRIC_SCHEMA[i]）。
    pub fn metric_values(&self) -> [Option<u64>; METRICS_PER_FRAME] {
        [
            self.stage_ns.get(0).copied().flatten(),
            self.stage_ns.get(1).copied().flatten(),
            self.stage_ns.get(2).copied().flatten(),
            self.stage_ns.get(3).copied().flatten(),
            self.interval_ns,
        ]
    }

    /// 四段合计（缺段帧 None——不产幻觉总数）。
    pub fn total_ns(&self) -> Option<u64> {
        if !self.stages_complete() {
            return None;
        }
        let mut total: u64 = 0;
        let mut i = 0usize;
        while i < 4 {
            if let Some(Some(v)) = self.stage_ns.get(i) {
                total = total.saturating_add(*v);
            }
            i += 1;
        }
        Some(total)
    }

    /// 第 i 段是否异常（超口径）。
    pub const fn stage_anomaly(&self, i: usize) -> bool {
        if i >= 4 {
            return false;
        }
        (self.stage_anomaly_bits >> i) & 1 == 1
    }

    /// schema 行展开（B08 导出面：`<指标>=<值><单位>` 分号连接）。
    pub fn schema_line(&self) -> String {
        let vals = self.metric_values();
        let mut out = String::new();
        let mut i = 0usize;
        while i < METRIC_SCHEMA.len() {
            let m = match METRIC_SCHEMA.get(i) {
                Some(m) => *m,
                None => break,
            };
            let v = match vals.get(i) {
                Some(Some(v)) => format!("{}{}", v, m.unit),
                _ => "None".to_string(),
            };
            if i > 0 {
                out.push_str("; ");
            }
            out.push_str(&format!("{}={}", m.name, v));
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 四、聚合层（判据三：两级滚动——中层桶 + 总聚合）
// ---------------------------------------------------------------------------

/// 分段统计（min/max/sum 三元；缺段帧不进统计——不产幻觉数字）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct StageStat {
    /// 该段统计的帧数（齐备帧数）。
    pub count: u64,
    /// 最小值。
    pub min_ns: Option<u64>,
    /// 最大值。
    pub max_ns: Option<u64>,
    /// 合计（均值=sum/count）。
    pub sum_ns: u64,
}

impl StageStat {
    /// 零值（const 面：账本 const 构造用——Default 在 const 上下文不可调）。
    pub const fn zero() -> StageStat {
        StageStat {
            count: 0,
            min_ns: None,
            max_ns: None,
            sum_ns: 0,
        }
    }

    /// 吸收一段数值。
    pub fn absorb(&mut self, v: u64) {
        self.count = self.count.saturating_add(1);
        self.min_ns = Some(match self.min_ns {
            Some(m) if m < v => m,
            _ => v,
        });
        self.max_ns = Some(match self.max_ns {
            Some(m) if m > v => m,
            _ => v,
        });
        self.sum_ns = self.sum_ns.saturating_add(v);
    }

    /// 均值（count=0 给 None）。
    pub fn mean_ns(&self) -> Option<u64> {
        if self.count == 0 {
            None
        } else {
            Some(self.sum_ns / self.count)
        }
    }

    /// 与另一段统计合并（中层桶压缩进总聚合用）。
    pub fn merge(&mut self, other: &StageStat) {
        if other.count == 0 {
            return;
        }
        self.count = self.count.saturating_add(other.count);
        self.min_ns = match (self.min_ns, other.min_ns) {
            (Some(a), Some(b)) => Some(if a < b { a } else { b }),
            (None, b) => b,
            (a, None) => a,
        };
        self.max_ns = match (self.max_ns, other.max_ns) {
            (Some(a), Some(b)) => Some(if a > b { a } else { b }),
            (None, b) => b,
            (a, None) => a,
        };
        self.sum_ns = self.sum_ns.saturating_add(other.sum_ns);
    }
}

/// 历史聚合桶（滚动退场帧的聚合事实——聚合不是丢弃）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct AggBucket {
    /// 聚合帧数。
    pub count: u64,
    /// 帧总时长最小值（四段合计；缺段帧不进 min/max 统计）。
    pub min_total_ns: Option<u64>,
    /// 帧总时长最大值。
    pub max_total_ns: Option<u64>,
    /// 帧总时长合计（均值=sum/count）。
    pub sum_total_ns: u64,
    /// 逐段统计（深化：四段各自 min/max/sum——分段账）。
    pub stages: [StageStat; 4],
    /// 间隔统计（聚合帧的间隔 min/max/sum）。
    pub interval: StageStat,
    /// jank 帧计数。
    pub jank_count: u64,
    /// 健康计数（齐备/缺段/全缺）。
    pub health_complete: u64,
    pub health_partial: u64,
    pub health_empty: u64,
}

impl AggBucket {
    /// 吸收一帧（四段齐备才进 min/max/sum——缺段帧只进 count）。
    pub fn absorb(&mut self, rec: &FrameRecord) {
        self.count = self.count.saturating_add(1);
        match rec.health {
            FrameHealth::Complete => self.health_complete = self.health_complete.saturating_add(1),
            FrameHealth::Partial => self.health_partial = self.health_partial.saturating_add(1),
            FrameHealth::Empty => self.health_empty = self.health_empty.saturating_add(1),
        }
        if let Some(total) = rec.total_ns() {
            self.min_total_ns = Some(match self.min_total_ns {
                Some(m) if m < total => m,
                _ => total,
            });
            self.max_total_ns = Some(match self.max_total_ns {
                Some(m) if m > total => m,
                _ => total,
            });
            self.sum_total_ns = self.sum_total_ns.saturating_add(total);
        }
        let mut i = 0usize;
        while i < 4 {
            if let Some(Some(v)) = rec.stage_ns.get(i) {
                if let Some(s) = self.stages.get_mut(i) {
                    s.absorb(*v);
                }
            }
            i += 1;
        }
        if let Some(iv) = rec.interval_ns {
            self.interval.absorb(iv);
        }
        if rec.jank {
            self.jank_count = self.jank_count.saturating_add(1);
        }
    }

    /// 吸收一个中层桶（中层压缩进总聚合——桶级 merge 不逐帧重放）。
    pub fn absorb_bucket(&mut self, b: &MidBucket) {
        self.count = self.count.saturating_add(b.count);
        self.min_total_ns = match (self.min_total_ns, b.min_total_ns) {
            (Some(a), Some(x)) => Some(if a < x { a } else { x }),
            (None, x) => x,
            (a, None) => a,
        };
        self.max_total_ns = match (self.max_total_ns, b.max_total_ns) {
            (Some(a), Some(x)) => Some(if a > x { a } else { x }),
            (None, x) => x,
            (a, None) => a,
        };
        self.sum_total_ns = self.sum_total_ns.saturating_add(b.sum_total_ns);
        let mut i = 0usize;
        while i < 4 {
            if let (Some(dst), Some(src)) = (self.stages.get_mut(i), b.stages.get(i)) {
                dst.merge(src);
            }
            i += 1;
        }
        self.interval.merge(&b.interval);
        self.jank_count = self.jank_count.saturating_add(b.jank_count);
        self.health_complete = self.health_complete.saturating_add(b.health_complete);
        self.health_partial = self.health_partial.saturating_add(b.health_partial);
        self.health_empty = self.health_empty.saturating_add(b.health_empty);
    }

    /// 均值（count=0 给 None——不报幻觉数字）。
    pub fn mean_total_ns(&self) -> Option<u64> {
        if self.count == 0 {
            None
        } else {
            Some(self.sum_total_ns / self.count)
        }
    }

    /// jank 万分比（无帧给 None）。
    pub fn jank_bps(&self) -> Option<u64> {
        if self.count == 0 {
            None
        } else {
            Some(self.jank_count.saturating_mul(10_000) / self.count)
        }
    }
}

/// 中层桶（两级滚动的中间粒度：保留桶内分段统计与桶号区间）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MidBucket {
    /// 桶号（= 首帧 id / MID_BUCKET_SIZE）。
    pub bucket_id: u64,
    /// 桶内首帧 id。
    pub first_frame: u64,
    /// 桶内末帧 id（含）。
    pub last_frame: u64,
    /// 桶内帧数。
    pub count: u64,
    /// 帧总时长 min/max/sum。
    pub min_total_ns: Option<u64>,
    pub max_total_ns: Option<u64>,
    pub sum_total_ns: u64,
    /// 分段统计。
    pub stages: [StageStat; 4],
    /// 间隔统计。
    pub interval: StageStat,
    /// jank 帧数。
    pub jank_count: u64,
    /// 健康计数。
    pub health_complete: u64,
    pub health_partial: u64,
    pub health_empty: u64,
}

impl MidBucket {
    /// 空桶（按桶号定位）。
    pub fn empty(bucket_id: u64) -> MidBucket {
        MidBucket {
            bucket_id,
            first_frame: bucket_id.saturating_mul(MID_BUCKET_SIZE),
            last_frame: bucket_id
                .saturating_mul(MID_BUCKET_SIZE)
                .saturating_add(MID_BUCKET_SIZE.saturating_sub(1)),
            count: 0,
            min_total_ns: None,
            max_total_ns: None,
            sum_total_ns: 0,
            stages: [StageStat::default(); 4],
            interval: StageStat::default(),
            jank_count: 0,
            health_complete: 0,
            health_partial: 0,
            health_empty: 0,
        }
    }

    /// 吸收一帧（帧 id 必须落在桶区间内——错桶显性拒绝）。
    pub fn absorb(&mut self, rec: &FrameRecord) -> Result<(), &'static str> {
        if rec.frame_id < self.first_frame || rec.frame_id > self.last_frame {
            return Err("帧 id 不在本桶区间：滚动记账错桶，建议核对桶号推算");
        }
        self.count = self.count.saturating_add(1);
        match rec.health {
            FrameHealth::Complete => self.health_complete = self.health_complete.saturating_add(1),
            FrameHealth::Partial => self.health_partial = self.health_partial.saturating_add(1),
            FrameHealth::Empty => self.health_empty = self.health_empty.saturating_add(1),
        }
        if let Some(total) = rec.total_ns() {
            self.min_total_ns = Some(match self.min_total_ns {
                Some(m) if m < total => m,
                _ => total,
            });
            self.max_total_ns = Some(match self.max_total_ns {
                Some(m) if m > total => m,
                _ => total,
            });
            self.sum_total_ns = self.sum_total_ns.saturating_add(total);
        }
        let mut i = 0usize;
        while i < 4 {
            if let Some(Some(v)) = rec.stage_ns.get(i) {
                if let Some(s) = self.stages.get_mut(i) {
                    s.absorb(*v);
                }
            }
            i += 1;
        }
        if let Some(iv) = rec.interval_ns {
            self.interval.absorb(iv);
        }
        if rec.jank {
            self.jank_count = self.jank_count.saturating_add(1);
        }
        Ok(())
    }

    /// 桶是否已满（满桶触发压缩）。
    pub const fn is_full(&self) -> bool {
        self.count >= MID_BUCKET_SIZE
    }
}

/// 场景计数器（场景维度的账——不因逐帧退场而丢）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneCounter {
    /// 场景名。
    pub scene: String,
    /// 累计帧数。
    pub frames: u64,
    /// 累计 jank 帧数。
    pub jank_frames: u64,
}

/// 场景计数账（保序：首见顺序不重排）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SceneLedger {
    entries: Vec<SceneCounter>,
}

impl SceneLedger {
    /// 空账。
    pub const fn new() -> SceneLedger {
        SceneLedger {
            entries: Vec::new(),
        }
    }

    /// 记一帧（新场景立账，已有场景累加）。
    pub fn record(&mut self, scene: &str, jank: bool) {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get_mut(i) {
                if e.scene == scene {
                    e.frames = e.frames.saturating_add(1);
                    if jank {
                        e.jank_frames = e.jank_frames.saturating_add(1);
                    }
                    return;
                }
            }
            i += 1;
        }
        self.entries.push(SceneCounter {
            scene: scene.to_string(),
            frames: 1,
            jank_frames: if jank { 1 } else { 0 },
        });
    }

    /// 场景累计帧数（未见场景给 0——查询面）。
    pub fn frames_of(&self, scene: &str) -> u64 {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.scene == scene {
                    return e.frames;
                }
            }
            i += 1;
        }
        0
    }

    /// 全部场景计数（判据遍历面）。
    pub fn entries(&self) -> &[SceneCounter] {
        &self.entries
    }

    /// 场景种类数。
    pub const fn kinds(&self) -> usize {
        self.entries.len()
    }
}

// ---------------------------------------------------------------------------
// 五、查询结论与账本（判据四：答案各有其位）
// ---------------------------------------------------------------------------

/// 查询结论（判据四：多种答案各有其位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryResult {
    /// 近期帧：逐帧全账。
    Exact(FrameRecord),
    /// 中层桶帧：桶级聚合（分段统计在）。
    MidAggregated(MidBucket),
    /// 已压缩帧：总聚合。
    Aggregated(AggBucket),
    /// 从未记录：不臆造。
    NotFound,
}

/// 区间统计结论（区间查询面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RangeStats {
    /// 区间内命中的近期帧数。
    pub exact_hits: u64,
    /// 区间内命中但已退场（只进聚合）的帧数估计。
    pub archived_hits: u64,
    /// 区间内近期帧的逐段统计（近期部分）。
    pub stages: [StageStat; 4],
    /// 区间内近期帧的间隔统计。
    pub interval: StageStat,
    /// 区间内近期帧 jank 数。
    pub jank: u64,
}

/// 帧时间账本（判据一/三：完整记录+两级滚动窗口）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameLedger {
    recent: Vec<FrameRecord>,
    mid_buckets: Vec<MidBucket>,
    history: AggBucket,
    scenes: SceneLedger,
    first_frame_id: Option<u64>,
    last_frame_id: Option<u64>,
    last_stamp_ns: Option<u64>,
    overflowed: u64,
    compressed_buckets: u64,
    /// 时间戳回退次数（stamp < prev——上游计时异常显性计数）。
    stamp_regressions: u64,
    /// 非单调帧 id 次数（id ≤ 上一次——上游编号异常显性计数）。
    id_regressions: u64,
    /// 段异常帧数（任一段超口径）。
    anomalous_frames: u64,
}

impl FrameLedger {
    /// 新账本。
    pub const fn new() -> FrameLedger {
        FrameLedger {
            recent: Vec::new(),
            mid_buckets: Vec::new(),
            history: AggBucket {
                count: 0,
                min_total_ns: None,
                max_total_ns: None,
                sum_total_ns: 0,
                stages: [StageStat::zero(); 4],
                interval: StageStat::zero(),
                jank_count: 0,
                health_complete: 0,
                health_partial: 0,
                health_empty: 0,
            },
            scenes: SceneLedger::new(),
            first_frame_id: None,
            last_frame_id: None,
            last_stamp_ns: None,
            overflowed: 0,
            compressed_buckets: 0,
            stamp_regressions: 0,
            id_regressions: 0,
            anomalous_frames: 0,
        }
    }

    /// 记一帧（判据一：结构完整入账；帧间隔由相邻帧时间戳自动算——
    /// 零墙钟：时间戳上游注入；回退显性计数不静默）。
    pub fn record(
        &mut self,
        frame_id: u64,
        stage_ns: [Option<u64>; 4],
        stamp_ns: u64,
        scene: &str,
    ) {
        let interval_ns = match self.last_stamp_ns {
            Some(prev) if stamp_ns >= prev => Some(stamp_ns.saturating_sub(prev)),
            Some(_) => {
                // 时间戳回退：显性计数，间隔记 None（不产负数幻觉）。
                self.stamp_regressions = self.stamp_regressions.saturating_add(1);
                None
            }
            None => None,
        };
        if let Some(prev_id) = self.last_frame_id {
            if frame_id <= prev_id {
                self.id_regressions = self.id_regressions.saturating_add(1);
            }
        }
        self.last_stamp_ns = Some(stamp_ns);
        if self.first_frame_id.is_none() {
            self.first_frame_id = Some(frame_id);
        }
        self.last_frame_id = Some(frame_id);
        // 段异常旗（超口径如实入账）。
        let mut anomaly = 0u8;
        let mut i = 0usize;
        while i < 4 {
            if let Some(Some(v)) = stage_ns.get(i) {
                if *v > STAGE_SANITY_CAP_NS {
                    anomaly |= 1u8 << i;
                }
            }
            i += 1;
        }
        if anomaly != 0 {
            self.anomalous_frames = self.anomalous_frames.saturating_add(1);
        }
        let health = FrameHealth::classify(&stage_ns);
        let jank = matches!(interval_ns, Some(iv) if iv > JANK_MARK_NS);
        let rec = FrameRecord {
            frame_id,
            stage_ns,
            interval_ns,
            scene: scene.to_string(),
            schema_version: SCHEMA_VERSION,
            health,
            jank,
            stage_anomaly_bits: anomaly,
        };
        self.scenes.record(scene, jank);
        self.recent.push(rec);
        if self.recent.len() > RECENT_CAPACITY {
            // 滚动：最旧帧退场进中层桶（判据三：两级聚合不是丢弃）。
            let evicted = match self.recent.get(0) {
                Some(r) => r.clone(),
                None => return,
            };
            self.recent.remove(0);
            self.absorb_into_mid(evicted);
            self.overflowed = self.overflowed.saturating_add(1);
        }
    }

    /// 退场帧入中层桶（桶满压缩进总聚合）。
    fn absorb_into_mid(&mut self, rec: FrameRecord) {
        let bucket_id = rec.frame_id / MID_BUCKET_SIZE;
        // 找既有桶或立新桶。
        let mut idx = None;
        let mut i = 0usize;
        while i < self.mid_buckets.len() {
            if let Some(b) = self.mid_buckets.get(i) {
                if b.bucket_id == bucket_id {
                    idx = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let i = match idx {
            Some(i) => i,
            None => {
                self.mid_buckets.push(MidBucket::empty(bucket_id));
                self.mid_buckets.len().saturating_sub(1)
            }
        };
        // 错桶防护：absorb 失败时该帧显性落入一个兜底桶（不静默丢帧）。
        let ok = match self.mid_buckets.get_mut(i) {
            Some(b) => b.absorb(&rec).is_ok(),
            None => false,
        };
        if !ok {
            // 帧与桶区间不符（乱序退场）：并入总聚合兜底并计数。
            self.history.absorb(&rec);
            return;
        }
        // 桶满压缩（保序：只压缩首桶，乱序不满桶的留中）。
        let full_first = match self.mid_buckets.get(0) {
            Some(b) => b.is_full(),
            None => false,
        };
        if full_first {
            let bucket = match self.mid_buckets.get(0) {
                Some(b) => b.clone(),
                None => return,
            };
            self.history.absorb_bucket(&bucket);
            self.mid_buckets.remove(0);
            self.compressed_buckets = self.compressed_buckets.saturating_add(1);
        }
        // 中层容量守恒：超容量压缩最旧桶（防乱序膨胀）。
        while self.mid_buckets.len() > MID_BUCKET_CAPACITY {
            let bucket = match self.mid_buckets.get(0) {
                Some(b) => b.clone(),
                None => break,
            };
            self.history.absorb_bucket(&bucket);
            self.mid_buckets.remove(0);
            self.compressed_buckets = self.compressed_buckets.saturating_add(1);
        }
    }

    /// 任意帧回查（判据四：Exact/MidAggregated/Aggregated/NotFound）。
    pub fn lookup(&self, frame_id: u64) -> QueryResult {
        let mut i = 0usize;
        while i < self.recent.len() {
            if let Some(r) = self.recent.get(i) {
                if r.frame_id == frame_id {
                    return QueryResult::Exact(r.clone());
                }
            }
            i += 1;
        }
        // 中层桶：帧 id 落在某个未压缩桶区间。
        let mut i = 0usize;
        while i < self.mid_buckets.len() {
            if let Some(b) = self.mid_buckets.get(i) {
                if frame_id >= b.first_frame && frame_id <= b.last_frame && b.count > 0 {
                    return QueryResult::MidAggregated(b.clone());
                }
            }
            i += 1;
        }
        // 已压缩进总聚合的帧：id 在 [first, first+overflowed) 且不在中层。
        if let (Some(first), Some(last)) = (self.first_frame_id, self.last_frame_id) {
            let agg_end = first.saturating_add(self.overflowed);
            if self.overflowed > 0 && frame_id >= first && frame_id < agg_end && frame_id <= last {
                return QueryResult::Aggregated(self.history);
            }
        }
        QueryResult::NotFound
    }

    /// 区间统计（近期帧部分逐段统计；退场部分给聚合近似）。
    pub fn range_stats(&self, from: u64, to: u64) -> RangeStats {
        let mut out = RangeStats {
            exact_hits: 0,
            archived_hits: 0,
            stages: [StageStat::default(); 4],
            interval: StageStat::default(),
            jank: 0,
        };
        if from > to {
            return out;
        }
        let mut i = 0usize;
        while i < self.recent.len() {
            if let Some(r) = self.recent.get(i) {
                if r.frame_id >= from && r.frame_id <= to {
                    out.exact_hits = out.exact_hits.saturating_add(1);
                    let mut s = 0usize;
                    while s < 4 {
                        if let Some(Some(v)) = r.stage_ns.get(s) {
                            if let Some(st) = out.stages.get_mut(s) {
                                st.absorb(*v);
                            }
                        }
                        s += 1;
                    }
                    if let Some(iv) = r.interval_ns {
                        out.interval.absorb(iv);
                    }
                    if r.jank {
                        out.jank = out.jank.saturating_add(1);
                    }
                }
            }
            i += 1;
        }
        // 中层桶命中估计（桶区间与查询区间相交即计桶 count——近似口径）。
        let mut i = 0usize;
        while i < self.mid_buckets.len() {
            if let Some(b) = self.mid_buckets.get(i) {
                if b.count > 0 && b.last_frame >= from && b.first_frame <= to {
                    out.archived_hits = out.archived_hits.saturating_add(b.count);
                }
            }
            i += 1;
        }
        out
    }

    /// 末帧快照（无帧 None）。
    pub fn latest(&self) -> Option<&FrameRecord> {
        self.recent.last()
    }

    /// 场景累计帧数。
    pub fn scene_count(&self, scene: &str) -> u64 {
        self.scenes.frames_of(scene)
    }

    /// 近期帧数。
    pub fn recent_len(&self) -> usize {
        self.recent.len()
    }

    /// 历史聚合桶。
    pub const fn history(&self) -> &AggBucket {
        &self.history
    }

    /// 中层桶（判据遍历面）。
    pub fn mid_buckets(&self) -> &[MidBucket] {
        &self.mid_buckets
    }

    /// 场景账（判据遍历面）。
    pub const fn scenes(&self) -> &SceneLedger {
        &self.scenes
    }

    /// 退场帧数。
    pub const fn overflowed(&self) -> u64 {
        self.overflowed
    }

    /// 已压缩桶数。
    pub const fn compressed_buckets(&self) -> u64 {
        self.compressed_buckets
    }

    /// 时间戳回退次数（上游计时异常显性面）。
    pub const fn stamp_regressions(&self) -> u64 {
        self.stamp_regressions
    }

    /// 帧编号回退次数。
    pub const fn id_regressions(&self) -> u64 {
        self.id_regressions
    }

    /// 段异常帧数。
    pub const fn anomalous_frames(&self) -> u64 {
        self.anomalous_frames
    }

    /// 内存核算（字节估算：近期帧 + 中层桶 + 场景账；配额对账面）。
    pub fn memory_bytes(&self) -> u64 {
        // FrameRecord 估约：id 8 + stage 4×10 + interval 10 + 版本 4 +
        // 健康等 8 + 场景字符串按 16 字节估（不含堆）。
        let per_frame: u64 = 80;
        let recent = (self.recent.len() as u64).saturating_mul(per_frame);
        let per_bucket: u64 = 4 * 48 + 48 + 64;
        let mid = (self.mid_buckets.len() as u64).saturating_mul(per_bucket);
        let hist: u64 = 4 * 48 + 48 + 64;
        let scenes = (self.scenes.entries.len() as u64).saturating_mul(48);
        recent.saturating_add(mid).saturating_add(hist).saturating_add(scenes)
    }

    /// schema 对账行（判据二运行时面：记录行与 schema 表逐字段对齐——
    /// 第 i 个值对应 METRIC_SCHEMA[i]，名称逐条 grep 可验）。
    pub fn schema_line(&self, rec: &FrameRecord) -> String {
        rec.schema_line()
    }

    /// 账本快照导出（B08 消费面：近期帧逐行 schema 展开 + 尾行聚合
    /// 摘要——每行可 grep 回 schema 表）。
    pub fn export_snapshot(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.recent.len() {
            if let Some(r) = self.recent.get(i) {
                out.push(format!("[{}|{}|{}] {}", r.frame_id, r.scene, r.health.zh(), r.schema_line()));
            }
            i += 1;
        }
        let h = &self.history;
        out.push(format!(
            "[history] count={} mean={:?} jank_bps={:?} compressed={}",
            h.count,
            h.mean_total_ns(),
            h.jank_bps(),
            self.compressed_buckets
        ));
        out
    }

    /// 账本守恒自检：总帧数（近期+退场）== 场景账帧数之和 ==
    /// 聚合 count + 近期数。多记少记都是缺陷（账实守恒纪律）。
    pub fn reconcile(&self) -> bool {
        let total = (self.recent.len() as u64).saturating_add(self.overflowed);
        let mut scene_sum = 0u64;
        let mut i = 0usize;
        while i < self.scenes.entries.len() {
            if let Some(e) = self.scenes.entries.get(i) {
                scene_sum = scene_sum.saturating_add(e.frames);
            }
            i += 1;
        }
        let bucket_sum = {
            let mut s = 0u64;
            let mut j = 0usize;
            while j < self.mid_buckets.len() {
                if let Some(b) = self.mid_buckets.get(j) {
                    s = s.saturating_add(b.count);
                }
                j += 1;
            }
            s
        };
        total == scene_sum
            && total
                == self
                    .history
                    .count
                    .saturating_add(bucket_sum)
                    .saturating_add(self.recent.len() as u64)
    }
}

impl Default for FrameLedger {
    fn default() -> FrameLedger {
        FrameLedger::new()
    }
}
