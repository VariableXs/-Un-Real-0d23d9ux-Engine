//! CGPU-F0483 · 帧时间账本（CGPU-D 域 · 帧预算与计时域 · D01 组 · 目标 360 行）。
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
//! 逐帧都有账位），不指强行填满数值。
//!
//! ## 二、同 schema：五元组单源，不第二套口径
//!
//! 每条帧指标以 F0274/B08 的五元组（名称/维度/单位/类型/采样策略）
//! 表达（[`METRIC_SCHEMA`]），三级命名 `d01.frameledger.<指标>`（判据二
//! 「不第二套口径」的落地面）：账本不是自造格式，是遥测 schema 在帧
//! 维度上的实例化——schema 版本进记录（版本化演进不破坏历史数据），
//! 记录行与 schema 表逐字段对账（判据侧独立重排，错一字先红）。
//!
//! ## 三、滚动：近期逐帧，历史聚合，配额恒定
//!
//! 内存配额写死（[`RECENT_CAPACITY`] 帧逐帧保留）；超限最旧帧从逐帧
//! 层退场、数值并入历史聚合桶（[`AggBucket`]：count/min/max/sum）——
//! 历史帧查不到逐帧明细但**查得到聚合事实**（判据三：聚合不是丢弃，
//! 是换了精度继续记账）；配额恒定⇒账本内存有界（长跑不膨胀）。
//!
//! ## 四、查询：任意帧三种答案
//!
//! 任意帧回查（判据四）三种结论各有其位：近期帧给 `Exact`（逐帧全
//! 账）、已聚合帧给 `Aggregated`（聚合统计）、从未记录给 `NotFound`
//! （不臆造）——三种都是正确答案，没有第四种「查不到就说零」。
//!
//! **对接**：F0482（四段耗时来源）；F0274/B08（遥测 schema 同源）；
//! F0484（帧间隔统计，消费间隔数据）。零 panic 面（下标走 `get`/
//! `Option`，算术饱和）、零 IO、零墙钟（时间戳上游注入）、无全局可变
//! 状态、no_std 零 std 依赖。

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
// 三、帧记录与账本（判据一、判据三）
// ---------------------------------------------------------------------------

/// 逐帧记录（六项结构完整：id+四段+间隔+标签+版本）。
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
}

impl AggBucket {
    /// 吸收一帧（四段齐备才进 min/max/sum——缺段帧只进 count）。
    pub fn absorb(&mut self, rec: &FrameRecord) {
        self.count = self.count.saturating_add(1);
        if rec.stages_complete() {
            let mut total: u64 = 0;
            let mut i = 0usize;
            while i < 4 {
                if let Some(Some(v)) = rec.stage_ns.get(i) {
                    total = total.saturating_add(*v);
                }
                i += 1;
            }
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
    }

    /// 均值（count=0 给 None——不报幻觉数字）。
    pub fn mean_total_ns(&self) -> Option<u64> {
        if self.count == 0 {
            None
        } else {
            Some(self.sum_total_ns / self.count)
        }
    }
}

/// 查询结论（判据四：三种答案各有其位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryResult {
    /// 近期帧：逐帧全账。
    Exact(FrameRecord),
    /// 历史帧：聚合统计。
    Aggregated(AggBucket),
    /// 从未记录：不臆造。
    NotFound,
}

/// 帧时间账本（判据一/三：完整记录+滚动窗口）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameLedger {
    recent: Vec<FrameRecord>,
    history: AggBucket,
    first_frame_id: Option<u64>,
    last_frame_id: Option<u64>,
    last_stamp_ns: Option<u64>,
    overflowed: u64,
}

impl FrameLedger {
    /// 新账本。
    pub const fn new() -> FrameLedger {
        FrameLedger {
            recent: Vec::new(),
            history: AggBucket {
                count: 0,
                min_total_ns: None,
                max_total_ns: None,
                sum_total_ns: 0,
            },
            first_frame_id: None,
            last_frame_id: None,
            last_stamp_ns: None,
            overflowed: 0,
        }
    }

    /// 记一帧（判据一：结构完整入账；帧间隔由相邻帧时间戳自动算——
    /// 零墙钟：时间戳上游注入）。
    pub fn record(
        &mut self,
        frame_id: u64,
        stage_ns: [Option<u64>; 4],
        stamp_ns: u64,
        scene: &str,
    ) {
        let interval_ns = match self.last_stamp_ns {
            Some(prev) => Some(stamp_ns.saturating_sub(prev)),
            None => None,
        };
        self.last_stamp_ns = Some(stamp_ns);
        if self.first_frame_id.is_none() {
            self.first_frame_id = Some(frame_id);
        }
        self.last_frame_id = Some(frame_id);
        let rec = FrameRecord {
            frame_id,
            stage_ns,
            interval_ns,
            scene: scene.to_string(),
            schema_version: SCHEMA_VERSION,
        };
        self.recent.push(rec);
        if self.recent.len() > RECENT_CAPACITY {
            // 滚动：最旧帧退场进聚合桶（判据三：聚合不是丢弃）。
            let evicted = match self.recent.get(0) {
                Some(r) => r.clone(),
                None => return,
            };
            self.recent.remove(0);
            self.history.absorb(&evicted);
            self.overflowed = self.overflowed.saturating_add(1);
        }
    }

    /// 任意帧回查（判据四：Exact/Aggregated/NotFound 三种答案）。
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
        // 已退场进聚合的帧：id 在 [first, first+overflowed) 区间内。
        if let (Some(first), Some(last)) = (self.first_frame_id, self.last_frame_id) {
            let agg_end = first.saturating_add(self.overflowed);
            if self.overflowed > 0 && frame_id >= first && frame_id < agg_end && frame_id <= last {
                return QueryResult::Aggregated(self.history);
            }
        }
        QueryResult::NotFound
    }

    /// 近期帧数。
    pub fn recent_len(&self) -> usize {
        self.recent.len()
    }

    /// 历史聚合桶。
    pub const fn history(&self) -> &AggBucket {
        &self.history
    }

    /// 退场帧数。
    pub const fn overflowed(&self) -> u64 {
        self.overflowed
    }

    /// schema 对账行（判据二运行时面：记录行与 schema 表逐字段对齐——
    /// 第 i 个值对应 METRIC_SCHEMA[i]，名称逐条 grep 可验）。
    pub fn schema_line(&self, rec: &FrameRecord) -> String {
        let vals = rec.metric_values();
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

impl Default for FrameLedger {
    fn default() -> FrameLedger {
        FrameLedger::new()
    }
}
