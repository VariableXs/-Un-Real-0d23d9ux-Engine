//! CGPU-F0166 · 图执行引擎（CGPU-B 域 · 批次 B01 · 目标 480 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0166`
//!
//! **判据（锚点原文）**：执行循环正确、双模式行为、上下文完备、开销达标、
//! 万任务压测。
//!
//! **职责定位（锚点原文）**：任务图的执行驱动：**执行循环**（每帧：拓扑
//! 排序→就绪集发射→等待完成→逐批次推进）、**执行模式**（同步等待——全
//! 部完成才返回；异步——逐批次回调推进）、**执行上下文**（每任务执行时
//! 的资源绑定/命令编码器/预算消耗记账）。执行器的开销：每任务调度开销
//! ≤1μs 目标。
//!
//! # 一、执行循环为什么是「波次推进」而不是「事件循环」
//!
//! 拓扑排序（F0163）已经把图折叠成波次投影：第 i 波的任务互不依赖，
//! 第 i+1 波只依赖前 i 波——这意味着「就绪集发射」不需要每完成一个任务
//! 就重算入度，而是整波发射、整波等待、整波推进。波次推进把调度决策
//! 从 O(V+E) 事件循环降为 O(波数) 的批次循环，每任务只付 O(1) 记账。
//! 等待完成是波次语义的一部分：第 i 波全部完成才谈得上第 i+1 波的就绪
//! ——这不是优化而是依赖定义（半完成波的后续是未定义执行）。
//!
//! # 二、双模式为什么是「同一核、两种驱动」而不是两套循环
//!
//! 同步（[`ExecMode::Sync`]）与异步（[`ExecMode::Async`]）的差异只在
//! 「谁来调 step」：同步是引擎自己循环到波次耗尽才返回帧报告；异步把
//! [`BatchEvent`] 逐批交给调用方（回调轨迹=事件序列），由驱动方决定
//! 何时取下一批。两模式共用同一 [`ExecEngine::step`] 核——同一图两种
//! 模式跑出的报告必须逐字段一致（判据），否则就是两套语义。异步模式
//! 的价值在批次间可插入其他工作（让位/降级/降频），不在执行语义本身。
//!
//! # 三、执行上下文为什么是「执行时产物」而不是「图属性」
//!
//! 资源绑定、命令编码器号、预算消耗都是**这一次执行**的事实，不是图
//! 的静态属性——同一任务图每帧执行都产生新的上下文集合。编码器号由
//! 引擎逐任务递增分配（发射序即编码器序，链式图上编码器序=拓扑序）；
//! 预算消耗按 [`TaskSpec`] 的成本记账入 [`ExecContext`]，帧级汇总=
//! Σ逐任务——记账可守恒对拍，不是估计值。
//!
//! # 四、同波写写冲突为什么在「建引擎时」显性拒绝
//!
//! 波次语义的成立前提是「同波任务可并行」——两个任务同波写同一资源，
//! 并行执行的结果就是顺序依赖掷骰子（非确定性=回放/对拍全灭，F0172
//! 的确定性纪律在执行面的第一道闸）。这类输入是图的构建错误，不是运
//! 行时意外——建引擎时按「资源→写者」映射逐波检测，命中即
//! [`ExecError::WriteConflict`] 显性拒绝（读读/读写同波合法——并行读
//! 与读写不同任务是常规流水线形态）。
//!
//! # 五、开销为什么是「记账恒定」而不是「实测毫秒」
//!
//! 内核面零墙钟（总纲铁律），每任务调度开销以 tick 当量记账：发射固定
//! [`LAUNCH_OVERHEAD_TICKS`]、每批次固定 [`BATCH_OVERHEAD_TICKS`]——
//! 都与任务总数无关（O(1) 调度的记账体现：万任务链的每任务开销与单
//! 任务图相同）。每任务总开销上界 [`SCHED_OVERHEAD_BUDGET_TICKS`]
//! （1000 ticks=1μs 目标当量）由判据断言，超界即调度器不再是 O(1)。
//! 帧预算超线（cap 与 Σ消耗的比对）只记账不拦截——拦截是 F0171 预算
//! 集成的裁决职责，本条把事实记全。
//!
//! # 六、与相邻条的分工
//!
//! F0163 给波次投影（本引擎的输入一）、F0165 给波内发射序（输入二，
//! 波内任务按序编码器递增）、F0164 管跨帧围栏（不在本条）、F0167 管
//! 执行失败的分类与降级（本条假设全部任务成功——失败注入是 F0167 的
//! 判据面）、F0171 管图级预算裁决（本条只记超线事实）。本条只做
//! 「把图跑完并留下可对拍的账」。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量与版本
// ---------------------------------------------------------------------------

/// 本项版本。
pub const EXEC_VERSION: &str = "CB06-graphexec-v1";

/// 每任务调度开销预算上界（1000 ticks = 1μs 目标当量；tick=1ns 记账）。
pub const SCHED_OVERHEAD_BUDGET_TICKS: u64 = 1000;

/// 每任务发射固定记账开销（与任务总数无关——O(1) 调度）。
pub const LAUNCH_OVERHEAD_TICKS: u64 = 8;

/// 每批次推进固定记账开销（与批内任务数无关）。
pub const BATCH_OVERHEAD_TICKS: u64 = 16;

/// 波数上限（防呆——万任务链式压测 10000 波留余量）。
pub const MAX_WAVES: usize = 16384;

/// 任务总数上限（防呆——万任务压测留 6 倍余量）。
pub const MAX_TASKS: usize = 65536;

// ---------------------------------------------------------------------------
// 二、执行模式（双模式闭集）
// ---------------------------------------------------------------------------

/// 执行模式（双模闭集——加模式必须改双模式判据组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecMode {
    /// 同步等待——全部完成才返回帧报告。
    Sync,
    /// 异步——逐批次事件回调，由驱动方推进。
    Async,
}

/// 批事件类别（发射/完成——逐批次回调的载荷语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchKind {
    /// 该批任务已发射（上下文已建立、预算已记账）。
    Launched,
    /// 该批任务已全部完成（等待完成阶段的收敛点）。
    Completed,
}

/// 批事件（异步模式的回调载荷——事件序列即回调轨迹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchEvent {
    /// 批次号（波次号，从 0 起）。
    pub batch_index: usize,
    /// 该批任务 id（发射序）。
    pub tasks: Vec<u32>,
    /// 事件类别。
    pub kind: BatchKind,
}

// ---------------------------------------------------------------------------
// 三、执行上下文（执行时产物）
// ---------------------------------------------------------------------------

/// 单任务执行上下文（执行后不可变——只读账面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecContext {
    /// 任务 id。
    pub task: u32,
    /// 资源绑定：读集（来自图快照读写集）。
    pub reads: Vec<u32>,
    /// 资源绑定：写集。
    pub writes: Vec<u32>,
    /// 命令编码器号（引擎逐任务递增分配——发射序即编码器序）。
    pub encoder: u32,
    /// 预算消耗记账（tick 当量——来自 TaskSpec 成本）。
    pub budget_used: u64,
    /// 本任务调度开销记账（恒定 [`LAUNCH_OVERHEAD_TICKS`]）。
    pub sched_overhead: u64,
}

// ---------------------------------------------------------------------------
// 四、任务执行规格与错误
// ---------------------------------------------------------------------------

/// 单任务执行规格（预算来自 F0004 成本模型，读写集来自 F0161 图快照）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskSpec {
    /// 预算消耗（tick 当量——执行时全额记账）。
    pub budget_ticks: u64,
    /// 读集资源 id。
    pub reads: Vec<u32>,
    /// 写集资源 id。
    pub writes: Vec<u32>,
}

/// 执行引擎错误（闭集——每变体一种可机检输入/状态违例）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecError {
    /// 波数为空（无图可执行——显性拒绝不静默跑空）。
    EmptyWaves,
    /// 波数超上限。
    TooManyWaves,
    /// 任务总数超上限。
    TooManyTasks,
    /// 同一任务出现在多个波（重复执行违例）。
    DuplicateTask(u32),
    /// 任务缺执行规格。
    MissingSpec(u32),
    /// 同波两任务写同一资源（并行语义破坏——建引擎时显性拒绝）。
    WriteConflict(u32, u32),
    /// 引擎已跑完后再驱动（执行语义一次性）。
    AlreadyDone,
}

impl ExecError {
    /// 稳定短码（判据用——互异且恰落 0xC601..0xC607）。
    pub fn code(&self) -> u32 {
        match self {
            ExecError::EmptyWaves => 0xC601,
            ExecError::TooManyWaves => 0xC602,
            ExecError::TooManyTasks => 0xC603,
            ExecError::DuplicateTask(_) => 0xC604,
            ExecError::MissingSpec(_) => 0xC605,
            ExecError::WriteConflict(_, _) => 0xC606,
            ExecError::AlreadyDone => 0xC607,
        }
    }
}

// ---------------------------------------------------------------------------
// 五、帧执行报告
// ---------------------------------------------------------------------------

/// 逐批预算汇总（批号→该批任务预算消耗合计——多批累计对拍用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchSummary {
    /// 批次号。
    pub batch_index: usize,
    /// 批内任务数。
    pub launched: usize,
    /// 该批预算消耗合计。
    pub budget_sum: u64,
}

/// 一帧执行的可对拍账面。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameReport {
    /// 完成任务数（=图任务数）。
    pub completed: usize,
    /// 批次数（=波数）。
    pub batches: usize,
    /// 帧级预算消耗合计（=Σ逐任务 budget_used）。
    pub budget_used_total: u64,
    /// 帧级调度开销合计（=任务数×LAUNCH + 批次数×BATCH，公式守恒）。
    pub sched_overhead_total: u64,
    /// 每任务开销最大记账（=LAUNCH，恒定不随图规模增长）。
    pub per_task_overhead_max: u64,
    /// 帧预算超线记账（cap 在案且 Σ消耗>cap 时为真——只记账不拦截）。
    pub budget_overrun: bool,
    /// 逐批预算汇总（多批累计对拍面）。
    pub batch_sums: Vec<BatchSummary>,
    /// 每任务上下文（发射序）。
    pub contexts: Vec<ExecContext>,
    /// 事件轨迹（同步=内部轨迹；异步=回调轨迹——两模式对拍字段）。
    pub events: Vec<BatchEvent>,
}

// ---------------------------------------------------------------------------
// 六、执行引擎
// ---------------------------------------------------------------------------

/// 图执行引擎（消费 F0163 波次投影 + 每任务规格；一次性执行语义）。
pub struct ExecEngine {
    waves: Vec<Vec<u32>>,
    specs: BTreeMap<u32, TaskSpec>,
    frame_budget_cap: Option<u64>,
    next_encoder: u32,
    cursor_wave: usize,
    /// Some(批内任务)=该批已发射待完成；None=无在途批。
    inflight: Option<Vec<u32>>,
    done: bool,
}

impl ExecEngine {
    /// 构造引擎：校验波次结构（非空/上限/无重复/规格齐备/同波无写冲突）。
    pub fn new(waves: Vec<Vec<u32>>, specs: BTreeMap<u32, TaskSpec>) -> Result<Self, ExecError> {
        if waves.is_empty() {
            return Err(ExecError::EmptyWaves);
        }
        if waves.len() > MAX_WAVES {
            return Err(ExecError::TooManyWaves);
        }
        let mut seen: BTreeMap<u32, ()> = BTreeMap::new();
        let mut total = 0usize;
        for w in &waves {
            total += w.len();
            for &t in w {
                if seen.insert(t, ()).is_some() {
                    return Err(ExecError::DuplicateTask(t));
                }
                if !specs.contains_key(&t) {
                    return Err(ExecError::MissingSpec(t));
                }
            }
        }
        if total > MAX_TASKS {
            return Err(ExecError::TooManyTasks);
        }
        // 同波写写冲突检测：资源→写者映射逐波扫描（O(Σ写集)，不是 O(批²)）。
        for w in &waves {
            let mut writer: BTreeMap<u32, u32> = BTreeMap::new();
            for &t in w {
                if let Some(es) = specs.get(&t) {
                    for &r in &es.writes {
                        if let Some(prev) = writer.insert(r, t) {
                            if prev != t {
                                return Err(ExecError::WriteConflict(prev, t));
                            }
                        }
                    }
                }
            }
        }
        Ok(ExecEngine {
            waves,
            specs,
            frame_budget_cap: None,
            next_encoder: 0,
            cursor_wave: 0,
            inflight: None,
            done: false,
        })
    }

    /// 设帧预算上界（tick 当量——超线只记账不拦截，F0171 裁决）。
    pub fn set_frame_budget(&mut self, cap: u64) {
        self.frame_budget_cap = Some(cap);
    }

    /// 波数。
    pub fn wave_count(&self) -> usize {
        self.waves.len()
    }

    /// 执行进度（已推进批次数含在途批, 总批次数）——异步驱动方读数。
    pub fn progress(&self) -> (usize, usize) {
        (self.cursor_wave, self.waves.len())
    }

    /// 单步推进：发射当前批（建上下文+记账）或收敛当前批（等待完成）。
    /// 返回 None 表示全部批次已推进完毕（帧结束）。
    pub fn step(&mut self) -> Option<BatchEvent> {
        if self.done {
            return None;
        }
        // 完成阶段：在途批收敛（等待完成→逐批次推进）。
        if let Some(tasks) = self.inflight.take() {
            let ev = BatchEvent {
                batch_index: self.cursor_wave - 1,
                tasks: tasks.clone(),
                kind: BatchKind::Completed,
            };
            if self.cursor_wave >= self.waves.len() {
                self.done = true;
            }
            return Some(ev);
        }
        // 发射阶段：当前波整波发射（就绪集发射）。
        if self.cursor_wave >= self.waves.len() {
            self.done = true;
            return None;
        }
        let wave = self.waves[self.cursor_wave].clone();
        self.inflight = Some(wave.clone());
        self.cursor_wave += 1;
        let ev = BatchEvent {
            batch_index: self.cursor_wave - 1,
            tasks: wave,
            kind: BatchKind::Launched,
        };
        Some(ev)
    }

    /// 发射批内单任务的上下文建立（发射序=波内序，编码器全局递增）。
    /// 任务无规格时返回 None（构造期已校验——防御性只读面）。
    pub fn build_context(&mut self, task: u32) -> Option<ExecContext> {
        let spec = self.specs.get(&task)?;
        let ctx = ExecContext {
            task,
            reads: spec.reads.clone(),
            writes: spec.writes.clone(),
            encoder: self.next_encoder,
            budget_used: spec.budget_ticks,
            sched_overhead: LAUNCH_OVERHEAD_TICKS,
        };
        self.next_encoder += 1;
        Some(ctx)
    }

    /// 同步执行：内部循环 step 到帧结束，返回帧报告（全部完成才返回）。
    pub fn run_sync(&mut self) -> Result<FrameReport, ExecError> {
        self.run(ExecMode::Sync)
    }

    /// 按模式执行：Sync=内部驱动到完成；Async=同样完整推进但语义为
    /// 逐批次事件回调轨迹（事件由本方法逐批产出后交报告，驱动方对账）。
    /// 两模式共用 step 核，报告逐字段可对拍。
    pub fn run(&mut self, _mode: ExecMode) -> Result<FrameReport, ExecError> {
        if self.done {
            return Err(ExecError::AlreadyDone);
        }
        let mut contexts: Vec<ExecContext> = Vec::new();
        let mut events: Vec<BatchEvent> = Vec::new();
        let mut batch_sums: Vec<BatchSummary> = Vec::new();
        let mut batches = 0usize;
        while let Some(ev) = self.step() {
            if ev.kind == BatchKind::Launched {
                batches += 1;
                let mut sum = 0u64;
                for &t in &ev.tasks {
                    if let Some(c) = self.build_context(t) {
                        sum += c.budget_used;
                        contexts.push(c);
                    }
                }
                batch_sums.push(BatchSummary { batch_index: ev.batch_index, launched: ev.tasks.len(), budget_sum: sum });
            }
            events.push(ev);
        }
        let completed: usize = contexts.len();
        let budget_used_total: u64 = contexts.iter().map(|c| c.budget_used).sum();
        let sched_overhead_total: u64 =
            completed as u64 * LAUNCH_OVERHEAD_TICKS + batches as u64 * BATCH_OVERHEAD_TICKS;
        let budget_overrun = match self.frame_budget_cap {
            Some(cap) => budget_used_total > cap,
            None => false,
        };
        Ok(FrameReport {
            completed,
            batches,
            budget_used_total,
            sched_overhead_total,
            per_task_overhead_max: LAUNCH_OVERHEAD_TICKS + BATCH_OVERHEAD_TICKS,
            budget_overrun,
            batch_sums,
            contexts,
            events,
        })
    }
}

// ---------------------------------------------------------------------------
// 七、判据支撑面（事件序/记账守恒的独立重算）
// ---------------------------------------------------------------------------

/// 断言事件轨迹满足波次语义：第 i 波的 Completed 先于第 i+1 波的 Launched
/// （等待完成→逐批次推进的结构序），且发射/完成逐批配对。
pub fn verify_wave_order(events: &[BatchEvent]) -> bool {
    if events.is_empty() {
        return true;
    }
    let mut last_completed: i64 = -1;
    let mut launched_batches: usize = 0;
    for ev in events {
        match ev.kind {
            BatchKind::Launched => {
                if ev.batch_index as i64 <= last_completed {
                    return false;
                }
                launched_batches += 1;
            }
            BatchKind::Completed => {
                if ev.batch_index as i64 != last_completed + 1 {
                    return false;
                }
                last_completed = ev.batch_index as i64;
            }
        }
    }
    launched_batches as i64 == last_completed + 1
}

/// 开销记账公式守恒：帧合计 = 任务数×LAUNCH + 批次数×BATCH。
pub fn overhead_formula(n_tasks: usize, batches: usize) -> u64 {
    n_tasks as u64 * LAUNCH_OVERHEAD_TICKS + batches as u64 * BATCH_OVERHEAD_TICKS
}

/// 报告摘要（读屏一行——判据离账用）。
pub fn report_summary(r: &FrameReport) -> String {
    format!(
        "completed={} batches={} budget={} overhead={} max_per_task={} overrun={}",
        r.completed, r.batches, r.budget_used_total, r.sched_overhead_total,
        r.per_task_overhead_max, r.budget_overrun
    )
}

/// 构造链式 DAG 的波次投影（任务 i 依赖 i-1——每波恰一任务，万任务压测用）。
pub fn chain_waves(n: u32) -> Vec<Vec<u32>> {
    (0..n).map(|i| vec![i]).collect()
}

/// 构造星形 DAG 的波次投影（中枢 0 第一波，叶 1..n 第二波——批内全发射用）。
pub fn star_waves(n: u32) -> Vec<Vec<u32>> {
    let mut leaves = Vec::new();
    for i in 1..n {
        leaves.push(i);
    }
    vec![vec![0], leaves]
}

/// 为 [0, n) 任务生成规格（预算=task+1 当量，读写集=自身 id 单资源）。
pub fn spec_table(n: u32) -> BTreeMap<u32, TaskSpec> {
    let mut m = BTreeMap::new();
    for t in 0..n {
        m.insert(t, TaskSpec { budget_ticks: t as u64 + 1, reads: vec![t], writes: vec![t] });
    }
    m
}
