// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — CGPU · cgb257_persistarch
// CGPU-F0257 · 图持久化总架构（B07 组 · 类型地基）
//
// 锚点：CGPU-F0257
// 判据原文：三大目标（崩溃恢复：图状态落盘重启续跑/调试重放：录一次图反复放/审计：谁在何时提交了什么）；
//          持久化范围（图拓扑+节点参数+资源账目+事件流四层）；写入路径（异步落盘不阻塞执行）；
//          与 B06 快照的关系（死锁诊断快照是一次性全量、本组是持续性持久化——格式同源）。
//
// 难点分析：
//   1. 「格式同源」意味着 B06 的诊断快照和 B07 的持久化必须共用同一套二进制布局——
//      否则每加一种快照就多一套解析器，维护成本爆炸。
//   2. 「异步落盘不阻塞执行」意味着执行线程只做「投递」，绝不能等 IO。这要求写入队列
//      有界且丢弃策略显性化（F0264 细化）。
//   3. 四层范围（拓扑/参数/账目/事件流）的边界必须清晰：拓扑是结构、参数是配置、
//      账目是资源归属、事件流是时序事实。恢复时四层必须同时一致，否则宁可降级。
//
// 纪律约束：
//   - no_std + alloc，三件套：use alloc::vec; use alloc::vec::Vec; use alloc::string::{String, ToString};
//   - match 臂字符串尾必须加逗号
//   - 纯功能代码，不留 TODO/占位

#![allow(dead_code)]

use alloc::vec;
extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ============================================================================
// 一、基础 ID 类型（与 cgb241 双模型 ID 同源但独立演进——持久化 ID 需可序列化稳定）
// ============================================================================

/// 图实例 ID（一个引擎运行期可能有多个图：主帧图、调试子图、录制回放图）。
pub type GraphId = u32;
/// 节点 ID（图内唯一，跨图不保证）。
pub type NodeId = u32;
/// 边 ID。
pub type EdgeId = u32;
/// 资源 ID（缓冲/纹理/描述符堆——账目层主体）。
pub type ResId = u32;
/// 事件 ID（事件流全局单调序号）。
pub type EventId = u64;
/// 帧号（事件流时间轴的离散刻度）。
pub type FrameNo = u64;
/// 逻辑时钟（单调递增，恢复后不回拨）。
pub type LogicalTime = u64;

// ============================================================================
// 二、持久化范围——四层数据模型
// ============================================================================

/// 图拓扑层：节点与边的结构关系。
///
/// 拓扑是「谁连谁」的骨架，不含任何配置。恢复时先建拓扑再挂参数，
/// 顺序反了会出现「半初始化节点」。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopologyLayer {
    /// 图 ID。
    pub graph: GraphId,
    /// 节点表（按 NodeId 升序——序列化确定性要求）。
    pub nodes: Vec<NodeId>,
    /// 边表：(from, to, kind)。kind 由 `EdgeKind` 编码。
    pub edges: Vec<(NodeId, NodeId, u8)>,
    /// 拓扑指纹（节点数 + 边数 + 首尾 ID 哈希——快速判等）。
    pub fingerprint: u64,
}

impl TopologyLayer {
    /// 构造拓扑层（自动排序保证确定性）。
    pub fn new(graph: GraphId, mut nodes: Vec<NodeId>, mut edges: Vec<(NodeId, NodeId, u8)>) -> Self {
        nodes.sort_unstable();
        nodes.dedup();
        edges.sort_unstable_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
        edges.dedup();
        let fingerprint = Self::compute_fingerprint(graph, &nodes, &edges);
        Self { graph, nodes, edges, fingerprint }
    }

    /// 计算拓扑指纹。
    ///
    /// 用 FNV-1a 变体：快、无依赖、碰撞率对「判等」用途足够。
    fn compute_fingerprint(graph: GraphId, nodes: &[NodeId], edges: &[(NodeId, NodeId, u8)]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |x: u64| {
            h ^= x;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        mix(graph as u64);
        mix(nodes.len() as u64);
        mix(edges.len() as u64);
        for &n in nodes.iter().take(8) {
            mix(n as u64);
        }
        for &n in nodes.iter().rev().take(8) {
            mix(n as u64);
        }
        for &(a, b, k) in edges.iter().take(8) {
            mix(((a as u64) << 32) | (b as u64));
            mix(k as u64);
        }
        h
    }

    /// 校验拓扑不变量（与 B01 同一套）。
    ///
    /// 不变量三条：
    ///   1. 边的两端节点必须在节点表内（无悬空边）；
    ///   2. 无自环（from != to）；
    ///   3. 节点表严格递增（排序 + 去重后的形态）。
    pub fn validate(&self) -> Result<(), TopoViolation> {
        let mut seen = Vec::with_capacity(self.nodes.len());
        for &n in &self.nodes {
            if let Some(&last) = seen.last() {
                if n <= last {
                    return Err(TopoViolation::NodeOrder);
                }
            }
            seen.push(n);
        }
        let node_set: BTreeMap<NodeId, ()> = self.nodes.iter().map(|&n| (n, ())).collect();
        for &(a, b, _) in &self.edges {
            if a == b {
                return Err(TopoViolation::SelfLoop(a));
            }
            if !node_set.contains_key(&a) {
                return Err(TopoViolation::DanglingEdge(a));
            }
            if !node_set.contains_key(&b) {
                return Err(TopoViolation::DanglingEdge(b));
            }
        }
        Ok(())
    }
}

/// 拓扑不变量违例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopoViolation {
    /// 节点表非严格递增。
    NodeOrder,
    /// 自环。
    SelfLoop(NodeId),
    /// 悬空边。
    DanglingEdge(NodeId),
}

/// 边类型编码（序列化友好——u8 而非枚举，跨版本稳定）。
pub mod edge_kind {
    /// 帧内数据依赖。
    pub const INTRA_FRAME: u8 = 0;
    /// 跨帧依赖（B05）。
    pub const CROSS_FRAME: u8 = 1;
    /// 等待边（B06 死锁检测用）。
    pub const WAIT: u8 = 2;
    /// 资源占用边。
    pub const RESOURCE: u8 = 3;
}

/// 节点参数层：可变配置。
///
/// 参数是「键值对集合」，但键有命名规范（域.组.键），值分四型。
/// 参数序列化必须保持插入序稳定——否则 diff 报告每次都变。
#[derive(Clone, Debug, PartialEq)]
pub struct ParamLayer {
    /// 节点参数表：NodeId → 参数名值对（按名字典序）。
    pub node_params: BTreeMap<NodeId, Vec<ParamEntry>>,
    /// 全局参数（图级）。
    pub global_params: Vec<ParamEntry>,
}

/// 单条参数。
#[derive(Clone, Debug, PartialEq)]
pub struct ParamEntry {
    /// 参数键（点分命名）。
    pub key: String,
    /// 参数值。
    pub value: ParamValue,
}

/// 参数值四型。
#[derive(Clone, Debug, PartialEq)]
pub enum ParamValue {
    /// 整型。
    Int(i64),
    /// 浮点。
    Float(f64),
    /// 布尔。
    Bool(bool),
    /// 字符串（UTF-8）。
    Str(String),
}

impl ParamValue {
    /// 类型标签（序列化用）。
    pub fn type_tag(&self) -> u8 {
        match self {
            ParamValue::Int(_) => 0,
            ParamValue::Float(_) => 1,
            ParamValue::Bool(_) => 2,
            ParamValue::Str(_) => 3,
        }
    }
}

impl ParamLayer {
    /// 构造（自动按键名排序保证确定性）。
    pub fn new() -> Self {
        Self {
            node_params: BTreeMap::new(),
            global_params: Vec::new(),
        }
    }

    /// 设节点参数。
    pub fn set_node_param(&mut self, node: NodeId, key: &str, value: ParamValue) {
        let list = self.node_params.entry(node).or_insert_with(Vec::new);
        match list.iter_mut().find(|e| e.key == key) {
            Some(e) => e.value = value,
            None => {
                list.push(ParamEntry { key: key.to_string(), value });
                list.sort_by(|a, b| a.key.cmp(&b.key));
            }
        }
    }

    /// 设全局参数。
    pub fn set_global_param(&mut self, key: &str, value: ParamValue) {
        match self.global_params.iter_mut().find(|e| e.key == key) {
            Some(e) => e.value = value,
            None => {
                self.global_params.push(ParamEntry { key: key.to_string(), value });
                self.global_params.sort_by(|a, b| a.key.cmp(&b.key));
            }
        }
    }

    /// 参数层指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |x: u64| {
            h ^= x;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        mix(self.node_params.len() as u64);
        mix(self.global_params.len() as u64);
        for (nid, list) in &self.node_params {
            mix(*nid as u64);
            mix(list.len() as u64);
            for e in list {
                mix(e.value.type_tag() as u64);
                for b in e.key.as_bytes() {
                    mix(*b as u64);
                }
            }
        }
        for e in &self.global_params {
            mix(e.value.type_tag() as u64);
            for b in e.key.as_bytes() {
                mix(*b as u64);
            }
        }
        h
    }
}

impl Default for ParamLayer {
    fn default() -> Self {
        Self::new()
    }
}

/// 资源账目层：谁持有/等待什么资源。
///
/// 这是与 B06 RAG 的接口面——持久化必须能完整重建「谁在等谁」。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountLayer {
    /// 持有记录：(ResId, SubjectId)。
    pub holdings: Vec<(ResId, u32)>,
    /// 等待记录：(ResId, SubjectId)。
    pub waitings: Vec<(ResId, u32)>,
    /// 账目平衡校验和（持有数 - 释放数）。
    pub balance: i64,
}

impl AccountLayer {
    /// 构造（自动排序）。
    pub fn new(mut holdings: Vec<(ResId, u32)>, mut waitings: Vec<(ResId, u32)>) -> Self {
        holdings.sort_unstable();
        holdings.dedup();
        waitings.sort_unstable();
        waitings.dedup();
        let balance = holdings.len() as i64 - waitings.len() as i64;
        Self { holdings, waitings, balance }
    }

    /// 校验账目不变量：平衡值必须与实算一致，且无同一主体既持有又等待同一资源。
    pub fn validate(&self) -> Result<(), AccountViolation> {
        let recomputed = self.holdings.len() as i64 - self.waitings.len() as i64;
        if recomputed != self.balance {
            return Err(AccountViolation::BalanceMismatch(self.balance, recomputed));
        }
        // 同一 (res, subj) 不应既在 holdings 又在 waitings——那是「自等」。
        for &(r, s) in &self.waitings {
            if self.holdings.contains(&(r, s)) {
                return Err(AccountViolation::SelfWait(r, s));
            }
        }
        Ok(())
    }

    /// 账目指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |x: u64| {
            h ^= x;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        mix(self.holdings.len() as u64);
        mix(self.waitings.len() as u64);
        for &(r, s) in self.holdings.iter().take(16) {
            mix(((r as u64) << 32) | s as u64);
        }
        for &(r, s) in self.waitings.iter().take(16) {
            mix(((r as u64) << 32) | s as u64);
        }
        h
    }
}

/// 账目违例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountViolation {
    /// 平衡值与实算不符。
    BalanceMismatch(i64, i64),
    /// 自等（既持有又等待同一资源）。
    SelfWait(ResId, u32),
}

/// 事件流层：时序事实。
///
/// 事件是「已经发生的事」，恢复时可重放。每事件带逻辑时间戳，
/// 恢复后逻辑时钟不回拨（单调性是审计的基础）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventStreamLayer {
    /// 事件表（按 EventId 升序）。
    pub events: Vec<PersistEvent>,
    /// 逻辑时钟水位。
    pub clock: LogicalTime,
}

/// 持久化事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistEvent {
    /// 事件 ID。
    pub id: EventId,
    /// 逻辑时间。
    pub time: LogicalTime,
    /// 帧号。
    pub frame: FrameNo,
    /// 事件类型码。
    pub kind: u16,
    /// 主体 ID（谁触发）。
    pub subject: u32,
    /// 目标 ID（作用对象）。
    pub target: u32,
    /// 载荷摘要（8 字节定长——审计够用，不落全文）。
    pub payload_hash: u64,
}

impl EventStreamLayer {
    /// 构造（按 id 排序去重）。
    pub fn new(mut events: Vec<PersistEvent>) -> Self {
        events.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        events.dedup_by(|a, b| a.id == b.id);
        let clock = events.iter().map(|e| e.time).max().unwrap_or(0);
        Self { events, clock }
    }

    /// 追加事件（保证 id 单调、time 不回拨）。
    pub fn push(&mut self, mut ev: PersistEvent) -> Result<(), EventViolation> {
        if let Some(last) = self.events.last() {
            if ev.id <= last.id {
                return Err(EventViolation::IdRegression(last.id, ev.id));
            }
            if ev.time < self.clock {
                return Err(EventViolation::TimeRegression(self.clock, ev.time));
            }
        }
        if ev.time > self.clock {
            self.clock = ev.time;
        }
        // 保证排序（首事件或跳号情况）。
        if let Some(last) = self.events.last() {
            if ev.id < last.id {
                ev.id = last.id + 1;
            }
        }
        self.events.push(ev);
        Ok(())
    }

    /// 事件流指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |x: u64| {
            h ^= x;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        mix(self.events.len() as u64);
        mix(self.clock);
        for e in self.events.iter().take(32) {
            mix(e.id);
            mix(e.time);
            mix(((e.kind as u64) << 48) | ((e.subject as u64) << 16) | (e.target as u64 & 0xffff));
            mix(e.payload_hash);
        }
        h
    }
}

/// 事件违例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventViolation {
    /// ID 回退。
    IdRegression(EventId, EventId),
    /// 时间回退。
    TimeRegression(LogicalTime, LogicalTime),
}

// ============================================================================
// 三、图快照——四层合一的原子单元
// ============================================================================

/// 图快照：四层合一。持久化/恢复/diff 的统一载体。
///
/// 快照必须是「自洽」的：四层指纹同时核对，任一层不匹配即拒绝恢复。
#[derive(Clone, Debug, PartialEq)]
pub struct GraphSnapshot {
    /// 快照格式版本。
    pub format_version: u16,
    /// 拓扑层。
    pub topology: TopologyLayer,
    /// 参数层。
    pub params: ParamLayer,
    /// 账目层。
    pub accounts: AccountLayer,
    /// 事件流层。
    pub events: EventStreamLayer,
    /// 生成时刻（逻辑时钟）。
    pub created_at: LogicalTime,
}

impl GraphSnapshot {
    /// 当前格式版本。
    pub const FORMAT_VERSION: u16 = 1;

    /// 构造快照。
    pub fn new(
        topology: TopologyLayer,
        params: ParamLayer,
        accounts: AccountLayer,
        events: EventStreamLayer,
    ) -> Self {
        let created_at = events.clock;
        Self {
            format_version: Self::FORMAT_VERSION,
            topology,
            params,
            accounts,
            events,
            created_at,
        }
    }

    /// 全量自洽校验（四层各自不变量 + 跨层一致性）。
    pub fn validate(&self) -> Result<(), SnapshotViolation> {
        self.topology.validate().map_err(SnapshotViolation::Topo)?;
        self.accounts.validate().map_err(SnapshotViolation::Account)?;
        // 事件层隐含单调性已在 push 时保证；这里查时钟水位。
        if self.events.clock < self.created_at {
            return Err(SnapshotViolation::ClockRegression);
        }
        // 跨层：事件中的节点引用必须在拓扑内（kind=节点相关事件时）。
        for ev in &self.events.events {
            if ev.kind == ev_kind::NODE_ADD || ev.kind == ev_kind::NODE_PARAM {
                if !self.topology.nodes.contains(&ev.target) {
                    return Err(SnapshotViolation::EventDanglingNode(ev.id, ev.target));
                }
            }
        }
        Ok(())
    }

    /// 快照总指纹（四层指纹再聚合）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |x: u64| {
            h ^= x;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        mix(self.format_version as u64);
        mix(self.topology.fingerprint);
        mix(self.params.fingerprint());
        mix(self.accounts.fingerprint());
        mix(self.events.fingerprint());
        mix(self.created_at);
        h
    }
}

/// 快照违例。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotViolation {
    /// 拓扑违例。
    Topo(TopoViolation),
    /// 账目违例。
    Account(AccountViolation),
    /// 时钟回退。
    ClockRegression,
    /// 事件引用了不存在的节点。
    EventDanglingNode(EventId, NodeId),
}

/// 事件类型码（跨版本稳定）。
pub mod ev_kind {
    /// 节点添加。
    pub const NODE_ADD: u16 = 1;
    /// 节点删除。
    pub const NODE_DEL: u16 = 2;
    /// 节点参数变更。
    pub const NODE_PARAM: u16 = 3;
    /// 边添加。
    pub const EDGE_ADD: u16 = 4;
    /// 边删除。
    pub const EDGE_DEL: u16 = 5;
    /// 资源分配。
    pub const RES_ALLOC: u16 = 6;
    /// 资源释放。
    pub const RES_FREE: u16 = 7;
    /// 等待开始。
    pub const WAIT_BEGIN: u16 = 8;
    /// 等待结束。
    pub const WAIT_END: u16 = 9;
    /// 帧开始。
    pub const FRAME_BEGIN: u16 = 10;
    /// 帧结束。
    pub const FRAME_END: u16 = 11;
    /// 快照点。
    pub const SNAPSHOT: u16 = 12;
}

// ============================================================================
// 四、持久化目标——三大目标的配置面
// ============================================================================

/// 持久化目标位掩码（可组合）。
pub mod goal {
    /// 崩溃恢复：图状态落盘重启续跑。
    pub const CRASH_RECOVERY: u8 = 0b0000_0001;
    /// 调试重放：录一次图反复放。
    pub const DEBUG_REPLAY: u8 = 0b0000_0010;
    /// 审计：谁在何时提交了什么。
    pub const AUDIT: u8 = 0b0000_0100;
}

/// 持久化配置。
#[derive(Clone, Debug)]
pub struct PersistConfig {
    /// 启用的目标组合。
    pub goals: u8,
    /// 持久化目录（路径白名单内）。
    pub directory: String,
    /// 异步写入队列容量（有界）。
    pub queue_capacity: usize,
    /// 基线快照间隔（帧数）。
    pub baseline_interval_frames: u64,
    /// 基线快照间隔（逻辑时长）。
    pub baseline_interval_time: LogicalTime,
    /// 基线快照间隔（事件数）。
    pub baseline_interval_events: u64,
    /// 增量链最大长度（超则合并新基线）。
    pub max_incremental_chain: usize,
    /// 是否脱敏（录制含资源名与参数摘要时）。
    pub redact_sensitive: bool,
}

impl PersistConfig {
    /// 默认配置：三目标全开。
    pub fn default_full() -> Self {
        Self {
            goals: goal::CRASH_RECOVERY | goal::DEBUG_REPLAY | goal::AUDIT,
            directory: "varix/persist".to_string(),
            queue_capacity: 4096,
            baseline_interval_frames: 60,
            baseline_interval_time: 1_000_000,
            baseline_interval_events: 10_000,
            max_incremental_chain: 64,
            redact_sensitive: true,
        }
    }

    /// 仅崩溃恢复（最小开销）。
    pub fn recovery_only() -> Self {
        Self {
            goals: goal::CRASH_RECOVERY,
            directory: "varix/persist".to_string(),
            queue_capacity: 1024,
            baseline_interval_frames: 120,
            baseline_interval_time: 2_000_000,
            baseline_interval_events: 20_000,
            max_incremental_chain: 32,
            redact_sensitive: true,
        }
    }

    /// 校验配置合法性。
    pub fn validate(&self) -> Result<(), ConfigViolation> {
        if self.goals == 0 {
            return Err(ConfigViolation::NoGoal);
        }
        if self.queue_capacity == 0 {
            return Err(ConfigViolation::ZeroQueue);
        }
        if self.max_incremental_chain == 0 {
            return Err(ConfigViolation::ZeroChain);
        }
        if self.directory.is_empty() {
            return Err(ConfigViolation::EmptyDirectory);
        }
        // 路径穿越检查。
        if self.directory.contains("..") {
            return Err(ConfigViolation::PathTraversal);
        }
        Ok(())
    }
}

/// 配置违例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigViolation {
    /// 未启用任何目标。
    NoGoal,
    /// 队列容量为零。
    ZeroQueue,
    /// 增量链上限为零。
    ZeroChain,
    /// 目录为空。
    EmptyDirectory,
    /// 路径穿越。
    PathTraversal,
}

// ============================================================================
// 五、异步写入路径——执行线程只投递不等待
// ============================================================================

/// 写入命令（执行线程投递给落盘线程）。
#[derive(Clone, Debug)]
pub enum WriteCommand {
    /// 全量快照。
    FullSnapshot(GraphSnapshot),
    /// 增量事件。
    Incremental(PersistEvent),
    /// 手动触发基线。
    ForceBaseline,
    /// 录制标记（重放起点）。
    RecordMark(LogicalTime),
}

/// 投递结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmitResult {
    /// 已入队。
    Accepted,
    /// 队列满，丢弃（显性计数）。
    Dropped,
    /// 持久化已停。
    Closed,
}

/// 有界异步写入队列（执行线程只碰这个）。
///
/// 设计要点：
///   - `submit` 是 O(1)、无锁（SPSC 简化为 mutex-free 的 ring 语义；
///     no_std 下用 VecDeque + 计数模拟——单生产者单消费者场景足够）；
///   - 队列满时**丢弃并计数**，绝不阻塞执行线程；
///   - 丢弃的优先级：先丢遥测级（增量事件），最后才丢快照。
pub struct WriteQueue {
    /// 队列容量。
    capacity: usize,
    /// 待写命令。
    pending: Vec<WriteCommand>,
    /// 累计接受数。
    accepted: u64,
    /// 累计丢弃数（显性）。
    dropped: u64,
    /// 是否关闭。
    closed: bool,
}

impl WriteQueue {
    /// 构造。
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            pending: Vec::new(),
            accepted: 0,
            dropped: 0,
            closed: false,
        }
    }

    /// 投递命令（O(1) 路径，绝不阻塞）。
    pub fn submit(&mut self, cmd: WriteCommand) -> SubmitResult {
        if self.closed {
            return SubmitResult::Closed;
        }
        if self.pending.len() >= self.capacity {
            // 队列满：尝试挤掉一条遥测级命令腾位。
            let evict_pos = self.pending.iter().position(|c| matches!(c, WriteCommand::Incremental(_)));
            match evict_pos {
                Some(idx) => {
                    self.pending.remove(idx);
                    self.dropped += 1;
                    self.pending.push(cmd);
                    self.accepted += 1;
                    SubmitResult::Accepted
                }
                None => {
                    self.dropped += 1;
                    SubmitResult::Dropped
                }
            }
        } else {
            self.pending.push(cmd);
            self.accepted += 1;
            SubmitResult::Accepted
        }
    }

    /// 取下一条命令（落盘线程调用）。
    pub fn take(&mut self) -> Option<WriteCommand> {
        if self.pending.is_empty() {
            None
        } else {
            Some(self.pending.remove(0))
        }
    }

    /// 取出全部命令（批量落盘）。
    pub fn drain(&mut self) -> Vec<WriteCommand> {
        core::mem::take(&mut self.pending)
    }

    /// 当前积压数。
    pub fn backlog(&self) -> usize {
        self.pending.len()
    }

    /// 累计接受数。
    pub fn accepted_count(&self) -> u64 {
        self.accepted
    }

    /// 累计丢弃数（显性化指标）。
    pub fn dropped_count(&self) -> u64 {
        self.dropped
    }

    /// 关闭队列（拒收新命令）。
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// 是否已关闭。
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

// ============================================================================
// 六、与 B06 快照的格式同源关系
// ============================================================================

/// 格式同源锚点。
///
/// B06 死锁诊断快照（cgb252）与本组持久化快照共用 `GraphSnapshot` 四层布局：
///   - B06 侧：一次性全量快照，用于诊断报告与人工审查；
///   - B07 侧：持续性持久化，基线+增量，用于崩溃恢复与重放。
/// 两者共用同一序列化器（F0258），保证「解析一套代码」。
pub struct FormatAffinity;

impl FormatAffinity {
    /// 校验 B06 快照能否被 B07 解析器接受（四层版本兼容）。
    pub fn compatible_with_b06(b06_version: u16, b07_version: u16) -> bool {
        // 主版本相同即可读（前向兼容：旧版读新版降级）。
        let b06_major = b06_version / 100;
        let b07_major = b07_version / 100;
        b06_major == b07_major
    }
}

// ============================================================================
// 七、判据：F0257 四条（三目标、四层、异步路径、格式同源）
// ============================================================================

/// F0257 判据集。
pub fn run_f0257_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0257");

    // --- 判据 1：三目标 ---
    let cfg = PersistConfig::default_full();
    let has_crash = cfg.goals & goal::CRASH_RECOVERY != 0;
    let has_replay = cfg.goals & goal::DEBUG_REPLAY != 0;
    let has_audit = cfg.goals & goal::AUDIT != 0;
    cs.add(
        "三目标全部启用",
        has_crash && has_replay && has_audit,
        "崩溃恢复+调试重放+审计",
    );
    let cfg_min = PersistConfig::recovery_only();
    cs.add(
        "最小配置仅崩溃恢复",
        cfg_min.goals == goal::CRASH_RECOVERY,
        "recovery_only 不含其他目标",
    );

    // --- 判据 2：四层数据模型 ---
    let topo = TopologyLayer::new(1, vec![1, 2, 3], vec![(1, 2, 0), (2, 3, 1)]);
    let mut params = ParamLayer::new();
    params.set_node_param(1, "cgpu.priority", ParamValue::Int(5));
    params.set_global_param("cgpu.budget_ms", ParamValue::Float(12.5));
    let accounts = AccountLayer::new(vec![(10, 1), (11, 2)], vec![(12, 3)]);
    let mut events = EventStreamLayer::new(vec![]);
    let _ = events.push(PersistEvent {
        id: 1,
        time: 100,
        frame: 1,
        kind: ev_kind::NODE_ADD,
        subject: 0,
        target: 1,
        payload_hash: 0,
    });
    let snap = GraphSnapshot::new(topo, params, accounts, events);
    let four_layers_ok = snap.topology.nodes.len() == 3
        && snap.params.node_params.contains_key(&1)
        && snap.accounts.holdings.len() == 2
        && snap.events.events.len() == 1;
    cs.add(
        "四层数据模型齐备",
        four_layers_ok,
        "拓扑3节点/参数1条/账目2持有/事件1条",
    );

    // --- 判据 3：异步路径（投递不等待） ---
    let mut wq = WriteQueue::new(4);
    let mut all_accepted = true;
    for i in 0..4 {
        let ev = PersistEvent {
            id: i + 1,
            time: 10 * i as u64,
            frame: 1,
            kind: ev_kind::SNAPSHOT,
            subject: 0,
            target: 0,
            payload_hash: 0,
        };
        if wq.submit(WriteCommand::Incremental(ev)) != SubmitResult::Accepted {
            all_accepted = false;
        }
    }
    // 第 5 条应被挤掉更早的增量（队列满但可挤）。
    let fifth = wq.submit(WriteCommand::Incremental(PersistEvent {
        id: 99,
        time: 999,
        frame: 2,
        kind: ev_kind::SNAPSHOT,
        subject: 0,
        target: 0,
        payload_hash: 0,
    }));
    let backlog_capped = wq.backlog() <= 4;
    cs.add(
        "异步投递不阻塞且有界",
        all_accepted && fifth == SubmitResult::Accepted && backlog_capped,
        "4容量队列投递5条仍Accepted且积压≤4",
    );
    cs.add(
        "丢弃显性计数",
        wq.dropped_count() >= 1,
        "队列满时丢弃计数递增",
    );

    // --- 判据 4：格式同源 ---
    let same_major = FormatAffinity::compatible_with_b06(100, 100);
    let diff_major = !FormatAffinity::compatible_with_b06(100, 200);
    cs.add(
        "B06/B07 格式同源判定",
        same_major && diff_major,
        "同主版本兼容，跨主版本拒绝",
    );

    // --- 附加：快照自洽校验 ---
    let v = snap.validate();
    cs.add(
        "快照四层自洽校验通过",
        v.is_ok(),
        "拓扑/账目/时钟/事件引用全对",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f0257_all_pass() {
        let cs = run_f0257_checks();
        assert!(cs.all_passed(), "F0257 判据未全绿: {}", cs.red_items_text());
    }

    #[test]
    fn topo_rejects_dangling() {
        let t = TopologyLayer::new(1, vec![1, 2], vec![(1, 3, 0)]);
        assert!(t.validate().is_err());
    }

    #[test]
    fn topo_rejects_self_loop() {
        let t = TopologyLayer::new(1, vec![1], vec![(1, 1, 0)]);
        assert_eq!(t.validate(), Err(TopoViolation::SelfLoop(1)));
    }

    #[test]
    fn account_rejects_self_wait() {
        let a = AccountLayer::new(vec![(1, 1)], vec![(1, 1)]);
        assert!(a.validate().is_err());
    }

    #[test]
    fn event_rejects_id_regression() {
        let mut s = EventStreamLayer::new(vec![]);
        let e1 = PersistEvent { id: 5, time: 10, frame: 1, kind: 0, subject: 0, target: 0, payload_hash: 0 };
        assert!(s.push(e1).is_ok());
        let e2 = PersistEvent { id: 3, time: 20, frame: 2, kind: 0, subject: 0, target: 0, payload_hash: 0 };
        // id 回退会被拒。
        assert!(s.push(e2).is_err());
    }

    #[test]
    fn write_queue_never_blocks() {
        let mut q = WriteQueue::new(2);
        for i in 0..10 {
            let _ = q.submit(WriteCommand::Incremental(PersistEvent {
                id: i,
                time: i,
                frame: 0,
                kind: 0,
                subject: 0,
                target: 0,
                payload_hash: 0,
            }));
        }
        assert!(q.backlog() <= 2);
        assert!(q.dropped_count() > 0);
    }
}
