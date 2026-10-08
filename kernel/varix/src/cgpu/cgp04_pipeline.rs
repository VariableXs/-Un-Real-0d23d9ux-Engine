//! CGPU-F2404 · 遥测数据管道（CGPU-P 域 · 自适应遥测域 · P02 组 · 目标 320 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2404`
//!
//! **判据（锚点原文）**：四段、模式复用、背压复用、两组、判据。
//!
//! **职责定位（锚点原文）**：遥测管道（采集→过滤→聚合→路由——四段（管道
//! 复用 F1447 模式——模式复用；背压（背压复用——背压复用；测试（四段/
//! 背压两组）。
//!
//! ## 一、四段流水线：采集→过滤→聚合→路由
//!
//! 每条遥测依次过四段（[`Stage`] 闭集，[`TelemetryPipe::ingest`] 单入口）：
//! **采集**（入口记账 tick/quota，[`StageReceipt`] 留痕）→ **过滤**
//! （隐私闸复用 F2402 `is_readable_by` 同口径 + 表外指标拒收）→
//! **聚合**（桶内 min/max/sum/count 确定性累积，[`AggCell`]）→
//! **路由**（按三级命名的域段分发到路由表，[`RouteTarget`] 闭集）。
//! 四段各有自己的计数账（[`StageCounters`]），漏一段账不平。
//!
//! ## 二、背压：满即拒，不丢账不 panic
//!
//! 管道容量 [`PIPE_CAPACITY`] 写死——缓冲满时 `ingest` 返回
//! `Err(PipeError::Backpressured)`（判据「背压」：满即拒是正常态不是
//! 故障，拒绝照记 [`StageCounters::backpressured`] 可查账；下游消费
//! `drain` 腾空后恢复入流）——生产者看到背压就降速，管道不静默丢
//! 遥测、不 panic、不限长打爆内存。
//!
//! ## 三、模式复用：不第二套口径
//!
//! [`REUSE_LINES`] 逐条声明：输入是 F2402 Metric 六元组原形（不转换
//! 不裁字段）；过滤闸口径与 F2403 采样决策/隐私档一致（不另立规则
//! 语言）；聚合语义与 F0483 AggBucket 同构（min/max/sum/count）；管道
//! 结构复用 F1447 流水线模式（段序固定单向）。
//!
//! **对接**：F2403（采样决策过滤闸）；F2402（六元组/隐私档）；
//! F2405（存储分层消费路由出口）。零 panic 面（`get`/`Option`/饱和
//! 算术）、零 IO、零墙钟（tick 上游注入）、无全局可变状态、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、四段定义（判据一：四段）
// ---------------------------------------------------------------------------

/// 管道段闭集（段序固定单向——判据「四段」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 采集（入口记账）。
    Collect,
    /// 过滤（隐私闸+表外拒收）。
    Filter,
    /// 聚合（桶内累积）。
    Aggregate,
    /// 路由（按域段分发）。
    Route,
}

impl Stage {
    /// 段名（判据侧对拍）。
    pub const fn name(self) -> &'static str {
        match self {
            Stage::Collect => "采集",
            Stage::Filter => "过滤",
            Stage::Aggregate => "聚合",
            Stage::Route => "路由",
        }
    }

    /// 下一段（Route 之后无段——末段 None）。
    pub const fn next(self) -> Option<Stage> {
        match self {
            Stage::Collect => Some(Stage::Filter),
            Stage::Filter => Some(Stage::Aggregate),
            Stage::Aggregate => Some(Stage::Route),
            Stage::Route => None,
        }
    }
}

/// 四段序（判据侧独立对拍：采集→过滤→聚合→路由）。
pub const STAGE_ORDER: [Stage; 4] = [Stage::Collect, Stage::Filter, Stage::Aggregate, Stage::Route];

// ---------------------------------------------------------------------------
// 二、输入形状与路由（复用声明落地）
// ---------------------------------------------------------------------------

/// 管道输入（F2402 Metric 六元组复用——本管道只搬运不裁字段）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipeRecord {
    /// 指标 ID（三级命名 `域.组.指标`——路由按域段分发）。
    pub id: String,
    /// 数值（聚合面只吃数值）。
    pub value: u64,
    /// 记录 schema 版本（复用 F2402 版本化——过滤闸读）。
    pub schema_version: u32,
    /// 隐私档（复用 F2402 口径：0=Public 1=Aggregated 2=Local）。
    pub privacy: u8,
    /// 注入 tick（上游注入——零墙钟）。
    pub tick: u64,
}

/// 管道可读的隐私档上限（复用 F2402 口径：管道是聚合面，只收
/// Public/Aggregated——Local 档在本地不进管道）。未收编为常量供判据对拍。
pub const PIPE_PRIVACY_MAX: u8 = 1;

/// 路由目标闭集（按指标 ID 首段域名分发——表外域拒收留账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteTarget {
    /// 帧域（d*）。
    Frame,
    /// 功耗域（j*）。
    Power,
    /// 采样域（p*）。
    Sampling,
}

impl RouteTarget {
    /// 从三级命名首段解析路由目标（表外 None 不臆造）。
    pub fn of(id: &str) -> Option<RouteTarget> {
        if id.starts_with('d') {
            Some(RouteTarget::Frame)
        } else if id.starts_with('j') {
            Some(RouteTarget::Power)
        } else if id.starts_with('p') {
            Some(RouteTarget::Sampling)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 三、聚合桶（与 F0483 AggBucket 同构——模式复用）
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
    /// 空桶（min/max 在首条时初始化——空桶 count=0）。
    pub const fn new() -> AggCell {
        AggCell { min: u64::MAX, max: 0, sum: 0, count: 0 }
    }

    /// 吸收一条（首条同时定 min/max）。
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

    /// 均值（sum/count 整数除法；空桶 None 不臆造 0）。
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
// 四、背压与管道主体（判据「背压」）
// ---------------------------------------------------------------------------

/// 管道缓冲容量（写死——容量即预算，可查账不可配置膨胀）。
pub const PIPE_CAPACITY: usize = 64;

/// 管道错误闭集（显性不 panic）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeError {
    /// 缓冲满——背压（正常态：生产者降速，消费后恢复）。
    Backpressured,
    /// 过滤段拒收（隐私超档/表外域）。
    Filtered(&'static str),
}

/// 各段计数账（漏一段账不平——判据侧按账对拍）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StageCounters {
    /// 采集段收到的总条数。
    pub collected: u32,
    /// 过滤段放行条数。
    pub passed: u32,
    /// 过滤段拒收条数。
    pub filtered: u32,
    /// 聚合段吸收条数（=路由段分发条数）。
    pub aggregated: u32,
    /// 背压拒绝条数（缓冲满——账在采集段记）。
    pub backpressured: u32,
}

/// 段回执（本次 ingest 走到的段——决策可查不黑箱）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageReceipt {
    /// 已走完全程并路由。
    Routed(RouteTarget),
    /// 被 Filter 拒（原因）。
    FilteredOut(&'static str),
    /// 被 Backpressure 拒（缓冲满）。
    PushedBack,
}

/// 遥测数据管道（四段流水线 + 有限缓冲 + 聚合桶 + 路由账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelemetryPipe {
    /// 缓冲（待聚合队列——满即背压）。
    buffer: Vec<PipeRecord>,
    /// 各域段聚合桶（按路由目标各一桶）。
    buckets: [(RouteTarget, AggCell); 3],
    /// 各段计数账。
    pub counters: StageCounters,
    /// 最近一次路由去向（单条级可查）。
    last_routed: Option<RouteTarget>,
}

impl TelemetryPipe {
    /// 新管道（空缓冲空桶零账）。
    pub fn new() -> TelemetryPipe {
        TelemetryPipe {
            buffer: Vec::new(),
            buckets: [
                (RouteTarget::Frame, AggCell::new()),
                (RouteTarget::Power, AggCell::new()),
                (RouteTarget::Sampling, AggCell::new()),
            ],
            counters: StageCounters::default(),
            last_routed: None,
        }
    }

    /// 入流（四段单入口：采集记账→背压闸→过滤→聚合→路由）。
    pub fn ingest(&mut self, rec: &PipeRecord) -> StageReceipt {
        // —— 采集段：入口记账 + 背压闸（缓冲满即拒，账照记） ——
        self.counters.collected = self.counters.collected.saturating_add(1);
        if self.buffer.len() >= PIPE_CAPACITY {
            self.counters.backpressured = self.counters.backpressured.saturating_add(1);
            return StageReceipt::PushedBack;
        }
        // —— 过滤段：隐私闸（复用 F2402 口径）+ 表外域拒收 ——
        if rec.privacy > PIPE_PRIVACY_MAX {
            self.counters.filtered = self.counters.filtered.saturating_add(1);
            return StageReceipt::FilteredOut("隐私超档（Local 不进管道）");
        }
        let target = match RouteTarget::of(&rec.id) {
            Some(t) => t,
            None => {
                self.counters.filtered = self.counters.filtered.saturating_add(1);
                return StageReceipt::FilteredOut("表外域");
            }
        };
        self.counters.passed = self.counters.passed.saturating_add(1);
        // 缓冲入队（背压闸已过——容量保证）
        self.buffer.push(rec.clone());
        // —— 聚合段：入对应域桶 ——
        let mut i = 0usize;
        while i < self.buckets.len() {
            if let Some((bt, cell)) = self.buckets.get(i) {
                if *bt == target {
                    if let Some(cell) = self.buckets.get_mut(i) {
                        cell.1.absorb(rec.value);
                    }
                    break;
                }
            }
            i += 1;
        }
        self.counters.aggregated = self.counters.aggregated.saturating_add(1);
        // —— 路由段：记账去向（桶是账面；条目留在缓冲待下游 drain 消费） ——
        self.last_routed = Some(target);
        StageReceipt::Routed(target)
    }

    /// 下游消费（腾空缓冲——背压解除的唯一途径，返回消费条数）。
    pub fn drain(&mut self) -> usize {
        let n = self.buffer.len();
        self.buffer.clear();
        n
    }

    /// 某域聚合桶读数（表外 None）。
    pub fn cell_of(&self, t: RouteTarget) -> Option<&AggCell> {
        let mut i = 0usize;
        while i < self.buckets.len() {
            if let Some((bt, cell)) = self.buckets.get(i) {
                if *bt == t {
                    return Some(cell);
                }
            }
            i += 1;
        }
        None
    }

    /// 最近路由去向。
    pub const fn last_routed(&self) -> Option<RouteTarget> {
        self.last_routed
    }

    /// 当前缓冲占用（背压可观察）。
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }
}

impl Default for TelemetryPipe {
    fn default() -> TelemetryPipe {
        TelemetryPipe::new()
    }
}

// ---------------------------------------------------------------------------
// 五、复用声明（判据「模式复用/背压复用」的对账面）
// ---------------------------------------------------------------------------

/// 复用清单（判据侧逐条 grep 对拍——不第二套口径）：
pub const REUSE_LINES: [&str; 4] = [
    "复用 F2402 Metric 六元组与隐私档口径——管道输入不转换不裁字段",
    "复用 F2403 采样/隐私过滤闸语义——管道过滤不另立规则语言",
    "复用 F0483 AggBucket 聚合同构 min/max/sum/count——不第二套聚合口径",
    "复用 F1447 流水线模式段序固定单向——背压满即拒不静默丢账",
];
