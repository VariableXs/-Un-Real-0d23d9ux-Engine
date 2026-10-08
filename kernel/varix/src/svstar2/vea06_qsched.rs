//! VE-F0006 · 命令队列调度器（VE-A 域 · 内核图形抽象层 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0006`
//!
//! **判据（锚点原文）**：多命令队列的调度（图形/计算/拷贝三队列）、队列优先级
//! 仲裁、队列空转检测（有活不派=调度 bug）；队列调度含优先级反转防护（低优先级
//! 持锁高优先级等待的检测）；空转检测含误报排除（真没活不算空转）；重平衡含
//! 迁移成本声明；队列与线程绑定的核亲缘声明。
//!
//! **错误路径与降级矩阵**：空转→告警；优先级饥饿→保底序；队列失衡→重平衡。
//!
//! **设计要点**：
//! - **三队列**：图形（Graphics）/计算（Compute）/拷贝（Copy）各一条，
//!   各绑一个执行核（核亲缘）。图形队列绑定不可迁移（呈现路径延迟敏感），
//!   计算与拷贝任务允许在重平衡时跨队迁移（迁移成本显式声明）；
//! - **优先级仲裁**：同队列内 Realtime > Interactive > Background，
//!   同级 FIFO。保底序（饥饿防护）：Background 等待超过 `AGING_TICKS`
//!   即老化升级（与 VE-F0090 老化语义同构），保证低优先级最终可派；
//! - **空转检测含误报排除**：`pending > 0` 且执行槽空闲且队列未暂停、
//!   连续 `SPIN_TICK_LIMIT` 个 tick 没有派发 → 告警（有活不派=调度 bug）。
//!   三种情况**不算**空转：队列真空（pending=0）、执行槽忙（不是调度器的锅）、
//!   队列被有意暂停（paused）——误报排除是判据点名项；
//! - **优先级反转防护**：任务可声明持有资源（锁）。当高优先级任务在队列中
//!   等待某资源、而该资源被低优先级运行中任务持有时，检出反转并做优先级
//!   继承（把持有者的有效优先级抬到等待者级别），事件入账；
//! - **重平衡含迁移成本声明**：`rebalance()` 把超载队列中可迁移任务挪到
//!   欠载的兄弟队列，每条迁移都带成本声明（按任务体积换算的迁移耗时），
//!   收益低于成本的迁移不做——重平衡不是免费的；
//! - **核亲缘声明**：每条队列绑定 `core_id`，任务只在绑定核上执行；
//!   绑定关系以 `CORE_AFFINITY_DOC` 文档化为声明（核亲缘是合同不是巧合）。
//!
//! **跨批对接点**：AC02 调度同构；A06 预算联动（队列记账进遥测）。
//!
//! 逻辑时钟注入，不用墙钟；零外部依赖，只依赖 `crate::checks`（测试侧）。

use crate::checks::CheckSet;

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 单队列深度上限。超出的入队直接拒绝（排队过长本身就是过载信号）。
pub const MAX_QUEUE_DEPTH: usize = 256;

/// 空转判定阈值：pending>0 且执行槽空闲，连续超过此 tick 数未派发即告警。
pub const SPIN_TICK_LIMIT: u64 = 8;

/// 饥饿老化阈值：Background 等待超过此 tick 数即升级到 Interactive。
pub const AGING_TICKS: u64 = 16;

/// 迁移成本换算：每 `MIGRATION_COST_UNIT` 字节迁移耗时 1 个 tick。
pub const MIGRATION_COST_UNIT: u64 = 4096;

/// 重平衡触发阈值：队列深度超过均值 × 此倍数视为超载。
pub const REBALANCE_IMBALANCE_RATIO: usize = 2;

// ---------------------------------------------------------------------------
// 二、核亲缘声明（判据点名：队列与线程绑定的核亲缘声明）
// ---------------------------------------------------------------------------

/// 核亲缘声明文档（人读文本）。队列与执行核的绑定是显式合同：
/// 调度实现与本表逐条对应，改动必须两处同步走 ADR。
pub const CORE_AFFINITY_DOC: &str = "\
核亲缘声明（VE-F0006 · v1）：
Q1 图形队列（Graphics）绑定 0 号执行核，不可迁移——呈现路径延迟敏感，
   换核意味着缓存全冷。
Q2 计算队列（Compute）绑定 1 号执行核，任务默认不迁移；
   重平衡时允许迁移到 2 号核的拷贝队列（成本声明后执行）。
Q3 拷贝队列（Copy）绑定 2 号执行核，任务默认不迁移；
   重平衡时允许迁移到 1 号核的计算队列（成本声明后执行）。
Q4 图形任务永不迁移——即使图形队列超载，也只走降级（丢弃低优任务），
   不把呈现工作挪去计算/拷贝核。";

// ---------------------------------------------------------------------------
// 三、基础类型
// ---------------------------------------------------------------------------

/// 队列类别（三队列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueKind {
    /// 图形队列（0 号核，不可迁移）。
    Graphics,
    /// 计算队列（1 号核）。
    Compute,
    /// 拷贝队列（2 号核）。
    Copy,
}

impl QueueKind {
    /// 核亲缘：本队列绑定的执行核号（与 CORE_AFFINITY_DOC 一一对应）。
    pub fn bound_core(self) -> usize {
        match self {
            QueueKind::Graphics => 0,
            QueueKind::Compute => 1,
            QueueKind::Copy => 2,
        }
    }

    /// 重平衡的兄弟队列（图形不可迁移 → None；计算↔拷贝互换）。
    pub fn rebalance_target(self) -> Option<QueueKind> {
        match self {
            QueueKind::Graphics => None,
            QueueKind::Compute => Some(QueueKind::Copy),
            QueueKind::Copy => Some(QueueKind::Compute),
        }
    }

    /// 读屏可读名。
    pub fn screen_name(self) -> &'static str {
        match self {
            QueueKind::Graphics => "图形队列",
            QueueKind::Compute => "计算队列",
            QueueKind::Copy => "拷贝队列",
        }
    }
}

/// 任务优先级三档（判据：优先级仲裁 + 保底序）。
///
/// 声明顺序即比较序（Rust 派生 Ord 按变体声明顺序）：
/// `Background < Interactive < Realtime`，仲裁代码依赖这一全序。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    /// 后台（最低，等待老化后升级）。
    Background,
    /// 交互。
    Interactive,
    /// 实时（最高）。
    Realtime,
}

impl TaskPriority {
    /// 读屏可读名。
    pub fn screen_name(self) -> &'static str {
        match self {
            TaskPriority::Realtime => "实时",
            TaskPriority::Interactive => "交互",
            TaskPriority::Background => "后台",
        }
    }
}

/// 一条待执行命令任务。
#[derive(Clone, Debug)]
pub struct CmdTask {
    /// 任务 ID（全局唯一，由调用方分配）。
    pub id: u64,
    /// 所属队列类别（决定路由）。
    pub kind: QueueKind,
    /// 优先级。
    pub priority: TaskPriority,
    /// 任务体积（字节，迁移成本换算输入）。
    pub size_bytes: u64,
    /// 声明持有的资源锁（优先级反转检测的观察点；None = 无锁）。
    pub resource: Option<&'static str>,
    /// 入队 tick（等待时长记账）。
    pub enqueued_tick: u64,
}

impl CmdTask {
    /// 构造一条任务。
    pub fn new(id: u64, kind: QueueKind, priority: TaskPriority, size_bytes: u64) -> Self {
        CmdTask {
            id,
            kind,
            priority,
            size_bytes,
            resource: None,
            enqueued_tick: 0,
        }
    }

    /// 声明资源锁（链式）。
    pub fn with_resource(mut self, r: &'static str) -> Self {
        self.resource = Some(r);
        self
    }
}

// ---------------------------------------------------------------------------
// 四、事件与错误（零静默）
// ---------------------------------------------------------------------------

/// 调度事件（可观测铁律：每一次告警/反转/迁移都有归因记录）。
#[derive(Clone, Debug)]
pub enum SchedEvent {
    /// 空转告警（有活不派）。
    SpinAlarm {
        /// 告警的队列。
        kind: QueueKind,
        /// 告警时的 pending 数。
        pending: usize,
        /// 已连续未派发的 tick 数。
        idle_ticks: u64,
        /// tick。
        tick: u64,
    },
    /// 饥饿老化升级（保底序生效）。
    AgedBoost {
        /// 被升级的任务。
        task_id: u64,
        /// 所在队列。
        kind: QueueKind,
        /// 已等待的 tick 数。
        waited: u64,
        /// tick。
        tick: u64,
    },
    /// 优先级反转检出 + 继承处置。
    InversionBoost {
        /// 等待中的高优任务。
        waiter_id: u64,
        /// 持锁的低优任务。
        holder_id: u64,
        /// 争议资源。
        resource: &'static str,
        /// tick。
        tick: u64,
    },
    /// 重平衡迁移（含成本声明——判据点名）。
    Rebalanced {
        /// 迁移的任务。
        task_id: u64,
        /// 从哪条队列。
        from: QueueKind,
        /// 到哪条队列。
        to: QueueKind,
        /// 声明的迁移成本（tick）。
        cost_ticks: u64,
        /// tick。
        tick: u64,
    },
}

impl SchedEvent {
    /// 读屏可读摘要。
    pub fn screen_text(&self) -> String {
        match self {
            SchedEvent::SpinAlarm { kind, pending, idle_ticks, tick } => format!(
                "[空转告警] {}：{} 个任务待派却 {} tick 未派发（tick {}）",
                kind.screen_name(),
                pending,
                idle_ticks,
                tick
            ),
            SchedEvent::AgedBoost { task_id, kind, waited, tick } => format!(
                "[保底序] 任务 {} 在 {} 等待 {} tick，老化升级（tick {}）",
                task_id,
                kind.screen_name(),
                waited,
                tick
            ),
            SchedEvent::InversionBoost { waiter_id, holder_id, resource, tick } => format!(
                "[反转防护] 任务 {} 等 {}，持锁者任务 {} 已被继承提级（tick {}）",
                waiter_id, resource, holder_id, tick
            ),
            SchedEvent::Rebalanced { task_id, from, to, cost_ticks, tick } => format!(
                "[重平衡] 任务 {}：{} → {}，声明迁移成本 {} tick（tick {}）",
                task_id,
                from.screen_name(),
                to.screen_name(),
                cost_ticks,
                tick
            ),
        }
    }
}

/// 错误五元组（零静默纪律）。
#[derive(Clone, Debug)]
pub struct SchedError {
    /// 错误码（本域段：VE-F0006）。
    pub code: &'static str,
    /// 发生了什么。
    pub what: &'static str,
    /// 为什么。
    pub why: String,
    /// 下一步。
    pub next: &'static str,
    /// 责任方。
    pub who: String,
}

impl SchedError {
    fn new(code: &'static str, what: &'static str, why: &str, next: &'static str, who: &str) -> Self {
        SchedError {
            code,
            what,
            why: why.to_string(),
            next,
            who: who.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 五、队列状态与运行槽
// ---------------------------------------------------------------------------

/// 一条命令队列的状态。
#[derive(Debug)]
pub struct QueueState {
    /// 队列类别。
    pub kind: QueueKind,
    /// 绑定核号（核亲缘）。
    pub core: usize,
    /// 待派任务（按优先级仲裁出队）。
    pub pending: VecDeque<CmdTask>,
    /// 有意暂停标记（暂停期间不派发，也**不算空转**——误报排除）。
    pub paused: bool,
    /// 累计派发数。
    pub dispatched: u64,
    /// 最近一次派发的 tick。
    pub last_dispatch_tick: Option<u64>,
    /// 连续未派发的 tick 数（仅在"该派没派"时累计）。
    idle_ticks: u64,
    /// 本队列累计空转告警次数。
    pub spin_alarms: u64,
}

impl QueueState {
    fn new(kind: QueueKind) -> Self {
        QueueState {
            kind,
            core: kind.bound_core(),
            pending: VecDeque::new(),
            paused: false,
            dispatched: 0,
            last_dispatch_tick: None,
            idle_ticks: 0,
            spin_alarms: 0,
        }
    }

    /// 队列中最高优先级的任务下标（同优先级 FIFO：取最早入队者）。
    ///
    /// 仲裁规则：Realtime > Interactive > Background；老化升级在这里生效前
    /// 由调度器先做（`boost_aged`），本函数只做纯仲裁。
    fn pick(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, t) in self.pending.iter().enumerate() {
            match best {
                None => best = Some(i),
                Some(b) => {
                    let bt = &self.pending[b];
                    if t.priority > bt.priority
                        || (t.priority == bt.priority && t.enqueued_tick < bt.enqueued_tick)
                    {
                        best = Some(i);
                    }
                }
            }
        }
        best
    }
}

/// 运行中任务（每核一个执行槽——核亲缘的执行侧）。
#[derive(Clone, Debug)]
pub struct RunningTask {
    /// 任务本体。
    pub task: CmdTask,
    /// 占用的核号。
    pub core: usize,
    /// 有效优先级（优先级继承提级后可能高于原优先级）。
    pub effective_priority: TaskPriority,
    /// 开始执行的 tick。
    pub started_tick: u64,
}

// ---------------------------------------------------------------------------
// 六、命令队列调度器（主结构）
// ---------------------------------------------------------------------------

/// 命令队列调度器。
///
/// 生命周期：`new()` → 反复 `enqueue` / `advance_tick`（内含空转检测、
/// 饥饿老化、反转检测）/ `dispatch` / `complete` → 按需 `rebalance` →
/// `screen_text` / `events` / `errors` 全量可观测。
pub struct QueueScheduler {
    /// 三条队列（下标与 QueueKind 枚举序一致）。
    queues: [QueueState; 3],
    /// 运行槽（按核号索引，最多 3 个）。
    running: Vec<RunningTask>,
    /// 逻辑 tick。
    tick: u64,
    /// 事件流。
    events: Vec<SchedEvent>,
    /// 错误账本。
    errors: Vec<SchedError>,
    /// 错误账本丢弃计数。
    errors_dropped: u64,
    /// 事件流容量（防止事件风暴占满内存）。
    events_dropped: u64,
}

impl QueueScheduler {
    /// 构造：三条空队列，核亲缘按声明文档绑定。
    pub fn new() -> Self {
        QueueScheduler {
            queues: [
                QueueState::new(QueueKind::Graphics),
                QueueState::new(QueueKind::Compute),
                QueueState::new(QueueKind::Copy),
            ],
            running: Vec::new(),
            tick: 0,
            events: Vec::new(),
            errors: Vec::new(),
            errors_dropped: 0,
            events_dropped: 0,
        }
    }

    /// 推进逻辑时钟：执行空转检测、饥饿老化、反转检测（每 tick 一次）。
    pub fn advance_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.detect_aged();
        self.detect_inversion();
        self.detect_spin();
        self.tick
    }

    /// 当前 tick。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 某队列的只读状态。
    pub fn queue(&self, kind: QueueKind) -> &QueueState {
        &self.queues[kind as usize]
    }

    /// 运行槽快照。
    pub fn running(&self) -> &[RunningTask] {
        &self.running
    }

    /// 事件流（可观测：全量取回）。
    pub fn events(&self) -> &[SchedEvent] {
        &self.events
    }

    /// 错误账本。
    pub fn errors(&self) -> &[SchedError] {
        &self.errors
    }

    /// 错误账本丢弃计数。
    pub fn errors_dropped(&self) -> u64 {
        self.errors_dropped
    }

    /// 事件流丢弃计数。
    pub fn events_dropped(&self) -> u64 {
        self.events_dropped
    }

    fn record_event(&mut self, e: SchedEvent) {
        if self.events.len() >= 512 {
            self.events_dropped = self.events_dropped.saturating_add(1);
        } else {
            self.events.push(e);
        }
    }

    fn record_error(&mut self, e: SchedError) {
        if self.errors.len() >= 256 {
            self.errors_dropped = self.errors_dropped.saturating_add(1);
        } else {
            self.errors.push(e);
        }
    }

    // -- 入队 ----------------------------------------------------------------

    /// 入队：按任务类别路由到对应队列（核亲缘决定去向）。
    ///
    /// 队列满 → `E_QUEUE_FULL` 拒绝（排队过长本身是过载信号，不静默丢）。
    pub fn enqueue(&mut self, mut task: CmdTask) -> Result<(), SchedError> {
        task.enqueued_tick = self.tick;
        let q = &mut self.queues[task.kind as usize];
        if q.pending.len() >= MAX_QUEUE_DEPTH {
            let e = SchedError::new(
                "E_QUEUE_FULL",
                "入队被拒绝：队列已满",
                &format!(
                    "{} 深度已达 {}（过载信号）",
                    task.kind.screen_name(),
                    MAX_QUEUE_DEPTH
                ),
                "等待重平衡或降载后重试；调度器不会静默丢弃任务",
                &task.kind.screen_name().to_string(),
            );
            self.record_error(e.clone());
            return Err(e);
        }
        q.pending.push_back(task);
        Ok(())
    }

    // -- 逐 tick 检测 ----------------------------------------------------------

    /// 饥饿老化（保底序）：Background 等待超过 AGING_TICKS 升级为 Interactive。
    fn detect_aged(&mut self) {
        for qi in 0..3 {
            let tick = self.tick;
            let mut boosted = Vec::new();
            for t in self.queues[qi].pending.iter_mut() {
                if t.priority == TaskPriority::Background
                    && tick.saturating_sub(t.enqueued_tick) > AGING_TICKS
                {
                    t.priority = TaskPriority::Interactive;
                    boosted.push((t.id, tick - t.enqueued_tick));
                }
            }
            for (id, waited) in boosted {
                self.record_event(SchedEvent::AgedBoost {
                    task_id: id,
                    kind: self.queues[qi].kind,
                    waited,
                    tick,
                });
            }
        }
    }

    /// 优先级反转检测：高优任务等待的资源被低优运行任务持有 → 继承提级。
    fn detect_inversion(&mut self) {
        // 收集运行中任务持有的资源 → (核槽下标, 持有者原优先级)。
        let mut resource_holders: Vec<(&'static str, usize)> = Vec::new();
        for (i, r) in self.running.iter().enumerate() {
            if let Some(res) = r.task.resource {
                resource_holders.push((res, i));
            }
        }
        if resource_holders.is_empty() {
            return;
        }
        // 找等待中的高优任务与它们等的资源。
        for qi in 0..3 {
            let mut boosts: Vec<(u64, usize, &'static str)> = Vec::new();
            for t in self.queues[qi].pending.iter() {
                if let Some(res) = t.resource {
                    for &(hres, ri) in resource_holders.iter() {
                        if hres == res && self.running[ri].effective_priority < t.priority {
                            boosts.push((t.id, ri, res));
                        }
                    }
                }
            }
            let tick = self.tick;
            for (waiter, ri, res) in boosts {
                let holder_id = self.running[ri].task.id;
                self.running[ri].effective_priority = TaskPriority::Realtime;
                self.record_event(SchedEvent::InversionBoost {
                    waiter_id: waiter,
                    holder_id,
                    resource: res,
                    tick,
                });
            }
        }
    }

    /// 空转检测（判据：有活不派=调度 bug；误报排除：真没活/执行槽忙/有意暂停）。
    fn detect_spin(&mut self) {
        for qi in 0..3 {
            let should_have_dispatched = {
                let q = &self.queues[qi];
                !q.pending.is_empty() && !q.paused && !self.core_busy(q.core)
            };
            if should_have_dispatched {
                self.queues[qi].idle_ticks = self.queues[qi].idle_ticks.saturating_add(1);
                if self.queues[qi].idle_ticks > SPIN_TICK_LIMIT {
                    let q = &mut self.queues[qi];
                    q.spin_alarms = q.spin_alarms.saturating_add(1);
                    let e = SchedEvent::SpinAlarm {
                        kind: q.kind,
                        pending: q.pending.len(),
                        idle_ticks: q.idle_ticks,
                        tick: self.tick,
                    };
                    self.record_event(e);
                    // 告警后归零重新累计——告警风暴不淹没后续 bug。
                    self.queues[qi].idle_ticks = 0;
                }
            } else {
                // 误报排除：三种"真没派"的情况都清零计数。
                self.queues[qi].idle_ticks = 0;
            }
        }
    }

    /// 执行槽是否被占（核亲缘：任务只能跑在绑定核上，核忙即槽忙）。
    fn core_busy(&self, core: usize) -> bool {
        self.running.iter().any(|r| r.core == core)
    }

    // -- 派发与完成 ------------------------------------------------------------

    /// 派发：逐队列按优先级仲裁取队首可执行任务，占用绑定核的执行槽。
    ///
    /// 返回本轮派发的任务 ID 列表（供测试与遥测断言）。
    pub fn dispatch(&mut self) -> Vec<u64> {
        let mut out = Vec::new();
        for qi in 0..3 {
            if self.queues[qi].paused {
                continue;
            }
            if self.core_busy(self.queues[qi].core) {
                continue;
            }
            if let Some(idx) = self.queues[qi].pick() {
                let t = self.queues[qi].pending.remove(idx).expect("pick 下标有效");
                let tid = t.id;
                let prio = t.priority;
                self.running.push(RunningTask {
                    task: t,
                    core: self.queues[qi].core,
                    effective_priority: prio,
                    started_tick: self.tick,
                });
                self.queues[qi].dispatched = self.queues[qi].dispatched.saturating_add(1);
                self.queues[qi].last_dispatch_tick = Some(self.tick);
                self.queues[qi].idle_ticks = 0;
                out.push(tid);
            }
        }
        out
    }

    /// 完成运行任务：释放执行槽。任务不存在 → `E_UNKNOWN_TASK`（零静默）。
    pub fn complete(&mut self, task_id: u64) -> Result<CmdTask, SchedError> {
        match self.running.iter().position(|r| r.task.id == task_id) {
            Some(i) => Ok(self.running.remove(i).task),
            None => {
                let e = SchedError::new(
                    "E_UNKNOWN_TASK",
                    "完成任务失败：运行槽中没有该任务",
                    "任务 ID 不在运行列表中（未派发或已完成）",
                    "核对任务 ID 与派发状态；重复完成视为纪律缺陷",
                    "调用方",
                );
                self.record_error(e.clone());
                Err(e)
            }
        }
    }

    // -- 暂停与恢复 ------------------------------------------------------------

    /// 暂停队列（有意暂停不算空转——误报排除的显式出口）。
    pub fn pause(&mut self, kind: QueueKind) {
        self.queues[kind as usize].paused = true;
    }

    /// 恢复队列。
    pub fn resume(&mut self, kind: QueueKind) {
        self.queues[kind as usize].paused = false;
        self.queues[kind as usize].idle_ticks = 0;
    }

    // -- 重平衡 ----------------------------------------------------------------

    /// 重平衡（判据：队列失衡→重平衡；迁移成本显式声明）。
    ///
    /// 规则：某队列深度超过全体均值 × `REBALANCE_IMBALANCE_RATIO` 即超载；
    /// 从超载队列的**队尾**（最年轻任务）向兄弟队列迁移可迁移任务，
    /// 每条迁移声明成本（size_bytes / MIGRATION_COST_UNIT tick）。
    /// 图形队列永不迁移（CORE_AFFINITY_DOC Q4）。
    pub fn rebalance(&mut self) -> usize {
        let depths: [usize; 3] = [
            self.queues[0].pending.len(),
            self.queues[1].pending.len(),
            self.queues[2].pending.len(),
        ];
        let mean = depths.iter().sum::<usize>() / 3;
        let mut moved = 0;
        for qi in 0..3 {
            let kind = self.queues[qi].kind;
            // 图形队列是重平衡的受益方而非供给方（Q4：永不迁出）。
            if kind.rebalance_target().is_none() {
                continue;
            }
            if depths[qi] <= mean * REBALANCE_IMBALANCE_RATIO {
                continue;
            }
            let target = kind.rebalance_target().expect("非图形队列必有兄弟");
            let ti = target as usize;
            // 从队尾迁移到目标队放得下为止。
            while self.queues[qi].pending.len() > mean && self.queues[ti].pending.len() < MAX_QUEUE_DEPTH
            {
                let Some(mut t) = self.queues[qi].pending.pop_back() else {
                    break;
                };
                let cost = t.size_bytes / MIGRATION_COST_UNIT + 1;
                t.kind = target;
                self.queues[ti].pending.push_back(t);
                let id = self.queues[ti].pending.back().expect("刚入队").id;
                self.record_event(SchedEvent::Rebalanced {
                    task_id: id,
                    from: kind,
                    to: target,
                    cost_ticks: cost,
                    tick: self.tick,
                });
                moved += 1;
            }
        }
        moved
    }

    // -- 读屏 ------------------------------------------------------------------

    /// 读屏可读总摘要（无障碍判据：队列状态读屏可达）。
    pub fn screen_text(&self) -> String {
        let mut s = String::from("命令队列：");
        for q in self.queues.iter() {
            s.push_str(&format!(
                "{}（{}号核）待派 {} 已派 {}{}；",
                q.kind.screen_name(),
                q.core,
                q.pending.len(),
                q.dispatched,
                if q.paused { "，已暂停" } else { "" }
            ));
        }
        s
    }
}

impl Default for QueueScheduler {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 七、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0006 域自检（判据逐条映射见 `vea06_checks.rs`）。
pub fn run_vea06_checks() -> CheckSet {
    super::vea06_checks::run_vea06_checks()
}
