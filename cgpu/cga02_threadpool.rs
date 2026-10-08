//! CGPU-F0002 · 渲染线程池与工作窃取调度器（CGPU-A 域 · GA02 · 目标 460 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0002`
//!
//! **判据（锚点原文）**：八核下瓦片负载不均衡 ≤5%、同帧重跑逐像素一致、
//! 抢占让位点 2ms 纪律实测、专核不受后台任务干扰、窃取开销 ≤3%。
//!
//! # 一、为 CPU 侧渲染提供确定性并行底座
//!
//! - **池规模 = 物理核数 - 2**（[`PoolSpec`]）：保留系统专核与合成器
//!   专核各一；下限钳制 1（单核平台不缩到零）。
//! - **核心亲和性可配**（[`AffinityMap`]）：合成器专核策略默认开——
//!   专核 worker 永不接收后台任务（判据 C2-亲和）。
//! - **任务模型**：渲染任务树（[`TaskTree`]）——一帧 = 根任务，瓦片
//!   = 叶任务；窃取按任务树层级（[`Tier`]）。
//! - **窃取策略**（[`StealEngine`]）：优先偷同层（瓦片间均衡），同层
//!   全空才跨层回退（避免深度窃取破坏缓存局部性）；偷的是受害者队列
//!   头（最老任务），本地取队尾（最新任务）。
//! - **优先级三档**（[`Priority`]）：实时帧 / 交互 / 后台预热；高优先
//!   级任务入队使后台任务在最近的协作式让位点让出（[`CHECKPOINT_MS`]
//!   = 2ms 纪律，[`BackgroundJob::yield_due`]）。
//!
//! # 二、确定性纪律：任务完成顺序不影响像素结果
//!
//! 每瓦片独立缓冲（[`TileJob::execute`] 只写自己的 [`TileOutput`]），
//! 归并按瓦片固定序（[`merge_frame`] 以 tile_idx 升序拼帧）——完成
//! 顺序打乱后同帧重跑逐像素一致（判据 C2-确定性两例）。这是「原生效
//! 果重建」对拍的前提。
//!
//! # 三、监控面与开销账面
//!
//! 每线程队列深度、窃取次数、缓存命中率入遥测（[`WorkerStats`]，
//! [`Telemetry::drain`]）；窃取开销 ≤3% 入 CGPU-Bench 账面
//! （[`STEAL_OVERHEAD_TARGET_PCT`]），实测未回填不虚报。

use crate::checks::CheckSet;
use alloc::collections::VecDeque;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 诊断码：CGPU 域 0x3900 段续编（F0001 已占 0x3900-0x3906）。
// ---------------------------------------------------------------------------

/// 池参数非法（核数为零等）。
pub const ERR_POOL_ARG: u16 = 0x3907;
/// 任务树非法（孤儿节点 / 层级断裂）。
pub const ERR_TREE_SHAPE: u16 = 0x3908;
/// 瓦片输出缺件（确定性归并拒绝）。
pub const ERR_TILE_MISSING: u16 = 0x3909;
/// 抢占请求落在非法状态。
pub const ERR_PREEMPT_STATE: u16 = 0x390A;

// ---------------------------------------------------------------------------
// 池规格与亲和
// ---------------------------------------------------------------------------

/// 协作式检查点让位间隔（锚点：每 2ms 一个让位点）。
pub const CHECKPOINT_MS: u32 = 2;

/// 窃取开销目标（占池吞吐百分比；入 CGPU-Bench 账面）。
pub const STEAL_OVERHEAD_TARGET_PCT: u32 = 3;

/// 池规格：物理核数派生 worker 数。
#[derive(Clone, Copy, Debug)]
pub struct PoolSpec {
    /// 物理核数。
    pub physical_cores: u32,
}

impl PoolSpec {
    /// worker 数 = 物理核数 - 2（系统专核 + 合成器专核），下限 1。
    pub fn worker_count(&self) -> Result<u32, u16> {
        if self.physical_cores == 0 {
            return Err(ERR_POOL_ARG);
        }
        Ok(self.physical_cores.saturating_sub(2).max(1))
    }
}

/// 亲和策略：合成器专核是否隔离后台任务（默认开）。
#[derive(Clone, Copy, Debug)]
pub struct AffinityMap {
    /// 合成器专核策略开关（默认 true）。
    pub compositor_isolated: bool,
}

impl Default for AffinityMap {
    fn default() -> Self {
        AffinityMap { compositor_isolated: true }
    }
}

impl AffinityMap {
    /// 某 worker 是否可接某优先级任务：专核隔离开启时，0 号 worker
    /// 视为合成器专核，不接后台任务。
    pub fn admits(&self, worker_idx: u32, p: Priority) -> bool {
        if self.compositor_isolated && worker_idx == 0 {
            return p != Priority::Background;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 任务模型：任务树（帧 → 瓦片）与优先级三档
// ---------------------------------------------------------------------------

/// 优先级三档（锚点：实时帧/交互/后台预热）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Priority {
    /// 后台预热（最低）。
    Background,
    /// 交互。
    Interactive,
    /// 实时帧（最高）。
    Realtime,
}

/// 任务树层级（窃取按层）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Tier {
    /// 根任务（一帧）。
    Frame,
    /// 中间层（管线段）。
    Stage,
    /// 叶任务（瓦片）。
    Tile,
}

/// 任务树节点。
#[derive(Clone, Debug)]
pub struct TaskNode {
    /// 节点 id。
    pub id: u32,
    /// 父节点（根为 None）。
    pub parent: Option<u32>,
    /// 层级。
    pub tier: Tier,
    /// 优先级。
    pub priority: Priority,
}

/// 渲染任务树：一帧 = 根任务，瓦片 = 叶任务。
#[derive(Clone, Debug)]
pub struct TaskTree {
    /// 节点表（id 即下标；构造方保证连续无洞）。
    pub nodes: Vec<TaskNode>,
}

impl TaskTree {
    /// 构造一帧任务树：1 根 + `tiles` 个瓦片叶（瓦片优先级继承帧）。
    pub fn frame(tiles: u32, priority: Priority) -> TaskTree {
        let mut nodes = Vec::new();
        nodes.push(TaskNode { id: 0, parent: None, tier: Tier::Frame, priority });
        for t in 0..tiles {
            nodes.push(TaskNode { id: 1 + t, parent: Some(0), tier: Tier::Tile, priority });
        }
        TaskTree { nodes }
    }

    /// 形状校验：每个非根节点父存在且层序合法（Frame>Stage>Tile）。
    pub fn validate(&self) -> Result<(), u16> {
        for n in &self.nodes {
            match n.parent {
                None if n.tier == Tier::Frame => {}
                None => return Err(ERR_TREE_SHAPE),
                Some(p) => {
                    let pn = self.nodes.get(p as usize).ok_or(ERR_TREE_SHAPE)?;
                    let ok = match n.tier {
                        Tier::Frame => false,
                        Tier::Stage => pn.tier == Tier::Frame,
                        Tier::Tile => pn.tier == Tier::Frame || pn.tier == Tier::Stage,
                    };
                    if !ok {
                        return Err(ERR_TREE_SHAPE);
                    }
                }
            }
        }
        Ok(())
    }

    /// 瓦片叶 id 列表（升序）。
    pub fn tile_ids(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self
            .nodes
            .iter()
            .filter(|n| n.tier == Tier::Tile)
            .map(|n| n.id)
            .collect();
        v.sort_unstable();
        v
    }
}

// ---------------------------------------------------------------------------
// 瓦片执行与确定性归并
// ---------------------------------------------------------------------------

/// 瓦片任务：独立缓冲，执行只写自己的输出（确定性纪律的物理载体）。
#[derive(Clone, Debug)]
pub struct TileJob {
    /// 瓦片号（帧内唯一，归并序键）。
    pub tile_idx: u32,
    /// 瓦片像素数。
    pub pixels: u32,
    /// 确定性着色种子（同输入同像素的函数源）。
    pub seed: u32,
}

/// 瓦片输出（独立缓冲）。
#[derive(Clone, Debug)]
pub struct TileOutput {
    /// 对应瓦片号。
    pub tile_idx: u32,
    /// 瓦片像素（由 seed 确定性生成）。
    pub buf: Vec<u32>,
}

impl TileJob {
    /// 执行：确定性着色（FNV 变体扩散），只写自身缓冲。
    pub fn execute(&self) -> TileOutput {
        let mut h = self.seed ^ (self.tile_idx.wrapping_mul(0x9E37_79B9));
        let mut buf = Vec::with_capacity(self.pixels as usize);
        for _ in 0..self.pixels {
            h = h.wrapping_mul(0x0100_0193) ^ (h >> 7);
            buf.push(h);
        }
        TileOutput { tile_idx: self.tile_idx, buf }
    }
}

/// 帧归并：按 tile_idx **固定升序**拼帧（完成顺序无关 → 确定性）。
/// 缺件返回 `ERR_TILE_MISSING`（拒绝半帧）。
pub fn merge_frame(tile_px: u32, outputs: &[TileOutput]) -> Result<Vec<u32>, u16> {
    let mut sorted: Vec<&TileOutput> = outputs.iter().collect();
    sorted.sort_unstable_by_key(|o| o.tile_idx);
    let mut frame = Vec::new();
    for (want, o) in sorted.iter().enumerate() {
        if o.tile_idx != want as u32 {
            return Err(ERR_TILE_MISSING);
        }
        if o.buf.len() != tile_px as usize {
            return Err(ERR_TILE_MISSING);
        }
        frame.extend_from_slice(&o.buf);
    }
    Ok(frame)
}

// ---------------------------------------------------------------------------
// 调度引擎：每 worker 双端队列 + 同层优先窃取 + 协作式抢占
// ---------------------------------------------------------------------------

/// 每 worker 遥测（锚点：队列深度/窃取次数/缓存命中率）。
#[derive(Clone, Copy, Default, Debug)]
pub struct WorkerStats {
    /// 当前队列深度。
    pub queue_depth: u32,
    /// 成功窃取次数。
    pub steal_count: u32,
    /// 缓存命中。
    pub cache_hit: u32,
    /// 缓存未命中。
    pub cache_miss: u32,
}

impl WorkerStats {
    /// 缓存命中率（万分比；未采样返回 None）。
    pub fn hit_rate_bp(&self) -> Option<u32> {
        let total = self.cache_hit + self.cache_miss;
        if total == 0 {
            None
        } else {
            Some(self.cache_hit * 10_000 / total)
        }
    }
}

/// 后台任务执行上下文：协作式让位点每 [`CHECKPOINT_MS`] 一个；
/// 高优先级入队后 `preempt_requested` 置位，任务在下一让位点让出。
#[derive(Clone, Copy, Debug)]
pub struct BackgroundJob {
    /// 已执行时长（ms）。
    pub ran_ms: u32,
    /// 抢占请求位。
    pub preempt_requested: bool,
}

impl BackgroundJob {
    /// 新后台任务（未跑、未抢）。
    pub fn new() -> Self {
        BackgroundJob { ran_ms: 0, preempt_requested: false }
    }

    /// 下一让位点绝对时刻（当前已完成时长向上取整到 CHECKPOINT_MS 边界）。
    pub fn next_yield_at(&self) -> u32 {
        ((self.ran_ms / CHECKPOINT_MS) + 1) * CHECKPOINT_MS
    }

    /// 是否已到让位点（ran_ms 恰为让位边界）。
    pub fn at_yield_point(&self) -> bool {
        self.ran_ms % CHECKPOINT_MS == 0 && self.ran_ms > 0
    }

    /// 是否应让出：抢占请求置位且到达下一让位点。
    pub fn yield_due(&self, now_ms: u32) -> bool {
        self.preempt_requested && now_ms >= self.next_yield_at()
    }
}

impl Default for BackgroundJob {
    fn default() -> Self {
        Self::new()
    }
}

/// 单 worker 本地队列 + 统计。
#[derive(Debug)]
pub struct WorkerQueue {
    /// worker 序号（0 号为合成器专核，见 [`AffinityMap`]）。
    pub idx: u32,
    /// 本地双端队列（尾取头偷）。
    deque: VecDeque<u32>,
    /// 统计。
    pub stats: WorkerStats,
}

impl WorkerQueue {
    /// 新 worker 队列。
    pub fn new(idx: u32) -> Self {
        WorkerQueue { idx, deque: VecDeque::new(), stats: WorkerStats::default() }
    }

    /// 入队（尾插；深度统计刷新）。
    pub fn push(&mut self, task: u32) {
        self.deque.push_back(task);
        self.stats.queue_depth = self.deque.len() as u32;
    }

    /// 本地取（尾弹：最新任务，缓存热）。
    pub fn pop_local(&mut self) -> Option<u32> {
        let t = self.deque.pop_back();
        self.stats.queue_depth = self.deque.len() as u32;
        t
    }

    /// 被偷端（头弹：最老任务——对受害者缓存最不热的，窃取痛感最小）。
    pub fn steal_head(&mut self) -> Option<u32> {
        let t = self.deque.pop_front()?;
        self.stats.queue_depth = self.deque.len() as u32;
        Some(t)
    }

    /// 当前深度。
    pub fn depth(&self) -> u32 {
        self.deque.len() as u32
    }
}

/// 调度引擎：分发 + 同层优先窃取。
pub struct StealEngine {
    /// worker 队列组。
    pub workers: Vec<WorkerQueue>,
    /// 亲和策略。
    pub affinity: AffinityMap,
}

impl StealEngine {
    /// 新引擎（池规格 + 亲和）。
    pub fn new(spec: PoolSpec, affinity: AffinityMap) -> Result<Self, u16> {
        let n = spec.worker_count()?;
        let mut workers = Vec::new();
        for i in 0..n {
            workers.push(WorkerQueue::new(i));
        }
        Ok(StealEngine { workers, affinity })
    }

    /// 分发瓦片任务：跳过不接该优先级的 worker（专核隔离），轮转均摊。
    /// 返回实际接收任务的 worker 数。
    pub fn distribute(&mut self, task_ids: &[u32], p: Priority) -> u32 {
        let mut served = 0u32;
        let mut cursor = 1usize; // 从 1 号起轮转（0 号专核默认留给合成器）。
        let n = self.workers.len();
        for &t in task_ids {
            let mut placed = false;
            for k in 0..n {
                let i = (cursor + k) % n;
                if self.affinity.admits(self.workers[i].idx, p) {
                    self.workers[i].push(t);
                    placed = true;
                    served += 1;
                    cursor = (i + 1) % n;
                    break;
                }
            }
            if !placed {
                // 全员拒收（单核专核且后台）：任务排队于专核（保活不丢）。
                self.workers[0].push(t);
                served += 1;
            }
        }
        served
    }

    /// 偷取：给 `thief` 找活——**同层优先**：`prefer` 给出期望层（窃贼
    /// 正在做的任务层；空队时由调度器按当前帧上下文给）时，第一轮只在
    /// 头任务层匹配的受害者里偷（瓦片间均衡，护缓存局部性）；同层全空
    /// 才跨层回退（深度窃取仅作兜底）。受害者按队列深度降序选（偷最忙
    /// 的，均衡效果最大）。偷头：对受害者缓存最不热的任务。
    pub fn steal(&mut self, thief: usize, prefer: Option<Tier>, tier_of: &dyn Fn(u32) -> Tier) -> Option<u32> {
        let n = self.workers.len();
        if n < 2 {
            return None;
        }
        // 第一轮：同层受害者（深度降序）。
        let mut victims: Vec<usize> = (0..n).filter(|&i| i != thief).collect();
        victims.sort_by_key(|&i| core::cmp::Reverse(self.workers[i].depth()));
        if let Some(pref) = prefer {
            for &v in &victims {
                if let Some(&t) = self.workers[v].deque.front() {
                    if tier_of(t) == pref {
                        let t = self.workers[v].steal_head()?;
                        self.workers[thief].stats.steal_count += 1;
                        self.workers[thief].push(t);
                        return Some(t);
                    }
                }
            }
            return None; // 有同层偏好时同层全空 → 不跨层（回退由调用方显式给 None）。
        }
        // 第二轮：跨层兜底（prefer=None 显式授权深度窃取）。
        for &v in &victims {
            if let Some(t) = self.workers[v].steal_head() {
                self.workers[thief].stats.steal_count += 1;
                self.workers[thief].push(t);
                return Some(t);
            }
        }
        None
    }

    /// 全池遥测快照（按 worker 序）。
    pub fn telemetry(&self) -> Vec<WorkerStats> {
        self.workers.iter().map(|w| w.stats).collect()
    }
}

/// 负载均衡报告（判据口径：不均衡 ≤5%）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoadBalanceReport {
    /// 最大 worker 负载。
    pub max_load: u32,
    /// 最小 worker 负载。
    pub min_load: u32,
}

impl LoadBalanceReport {
    /// 不均衡率（万分比 = (max-min)/max）；max=0 视为完美均衡。
    pub fn imbalance_bp(&self) -> u32 {
        if self.max_load == 0 {
            return 0;
        }
        (self.max_load - self.min_load) * 10_000 / self.max_load
    }

    /// 是否满足锚点 ≤5%。
    pub fn within_target(&self) -> bool {
        self.imbalance_bp() <= 500
    }
}

/// 对分发结果做均衡报告（按各 worker 队列深度）。
pub fn balance_report(workers: &[WorkerQueue]) -> LoadBalanceReport {
    let mut mx = 0u32;
    let mut mn = u32::MAX;
    for w in workers {
        let d = w.depth();
        mx = mx.max(d);
        mn = mn.min(d);
    }
    if workers.is_empty() {
        mn = 0;
    }
    LoadBalanceReport { max_load: mx, min_load: mn }
}

/// 基准账面条目（入 CGPU-Bench；实测由真机回填）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BenchEntry {
    /// 条目名。
    pub name: &'static str,
    /// 目标（窃取开销 ≤3%）。
    pub target_pct: u32,
    /// 实测百分比（None = 未回填，不虚报）。
    pub measured_pct: Option<u32>,
}

impl BenchEntry {
    /// 目标是否达成（未回填 = false）。
    pub fn target_met(&self) -> bool {
        match self.measured_pct {
            Some(v) => v <= self.target_pct,
            None => false,
        }
    }
}

/// 本件基准账面。
pub const BENCH_LEDGER: [BenchEntry; 2] = [
    BenchEntry { name: "steal-overhead-8c", target_pct: STEAL_OVERHEAD_TARGET_PCT, measured_pct: None },
    BenchEntry { name: "tile-imbalance-8c", target_pct: 5, measured_pct: None },
];

/// 本件判据聚合（由 checks 文件实现）。
pub fn run_cga02_checks() -> CheckSet {
    crate::cgpu::cga02_threadpool_checks::run_cga02_checks()
}
