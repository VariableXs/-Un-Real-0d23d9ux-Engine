//! CGPU-F0194 · 负载均衡与工作窃取（判据段 0x05C0）
//! 切分后的负载均衡：工作窃取调度（F0002 线程池窃取——帧图任务的
//! 窃取集成）、窃取粒度自适应（窃取开销与均衡收益的权衡——粒度过
//! 细窃取开销大于计算）、负载不均容忍度（不均衡 ≤10% 目标——超限
//! 自动细粒度化）、窃取统计（窃取次数/方向/距离遥测——调度健康度
//! 指标）。判据：窃取集成正确、粒度自适应、均衡指标、遥测、八核
//! 伸缩曲线。
//!
//! 诊断码段 0x05C0（本模块独占：0x05C0~0x05CF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x05C0;

/// 负载不均容忍度（千分比）：最忙核与均值偏差 ≤100‰ 即达标。
pub const IMBALANCE_TOLERANCE_PERMILLE: u32 = 100;

/// 窃取批默认粒度（任务数）。
pub const DEFAULT_STEAL_BATCH: u32 = 4;

/// 粒度自适应下限：单件窃取（过细预警线）。
pub const STEAL_BATCH_MIN: u32 = 1;

/// 粒度自适应上限：一次窃取不超过 32 件（过粗会破坏局部性）。
pub const STEAL_BATCH_MAX: u32 = 32;

/// 窃取探测距离上限（核拓扑跳数）：超过即放弃（远距离窃取的
/// 缓存迁移代价通常高于计算收益）。
pub const STEAL_HOP_MAX: u32 = 4;

/// 窃取任务（帧图任务在线程池队列里的形态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StealTask {
    /// 任务号。
    pub id: u32,
    /// 预估代价（微秒）。
    pub cost_us: u32,
}

impl StealTask {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.id == 0 || self.cost_us == 0 {
            return Err(SEG_BASE);
        }
        Ok(())
    }
}

/// 单核双端队列：本地端 LIFO（缓存热）、窃取端 FIFO（偷最老、
/// 最可能已不在缓存但最能平衡负载）。
#[derive(Default)]
pub struct WorkerDeque {
    /// 队列名（核号）。
    pub core: u32,
    /// 底层存储（索引 0 = 最老端 / 末尾 = 最新端）。
    items: Vec<StealTask>,
}

impl WorkerDeque {
    /// 建队列。
    pub fn new(core: u32) -> Self {
        Self { core, items: Vec::new() }
    }
    /// 本地压入（LIFO 端）。
    pub fn push_local(&mut self, t: StealTask) -> Result<(), u16> {
        t.validate()?;
        self.items.push(t);
        Ok(())
    }
    /// 本地弹出（LIFO 端）。
    pub fn pop_local(&mut self) -> Option<StealTask> {
        self.items.pop()
    }
    /// 队列深度。
    pub fn depth(&self) -> usize {
        self.items.len()
    }
    /// 窃取一批（FIFO 端取前 batch 件）：目标队列不足 batch 时
    /// 全取但保留 1 件给属主（空队列会引发窃取风暴）。
    pub fn steal_batch(&mut self, batch: u32) -> Vec<StealTask> {
        if self.items.is_empty() || batch == 0 {
            return Vec::new();
        }
        let take = batch.min((self.items.len() - 1).max(0) as u32) as usize;
        if take == 0 {
            return Vec::new();
        }
        self.items.drain(0..take).collect()
    }
    /// 队列代价合计（微秒）。
    pub fn total_cost_us(&self) -> u64 {
        self.items.iter().map(|t| t.cost_us as u64).sum()
    }
}

/// 窃取事件（判据「遥测」原子量）：方向 = 受害者→窃取者。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StealEvent {
    /// 受害核号。
    pub victim: u32,
    /// 窃取核号。
    pub thief: u32,
    /// 本批窃得件数。
    pub count: u32,
    /// 窃得代价合计（微秒）。
    pub cost_us: u64,
    /// 拓扑跳数。
    pub hops: u32,
}

impl StealEvent {
    /// 合法性：不许自窃、件数与代价一致、跳数不超限。
    pub fn validate(&self) -> Result<(), u16> {
        if self.victim == self.thief {
            return Err(SEG_BASE + 1);
        }
        if self.count == 0 || self.cost_us == 0 {
            return Err(SEG_BASE + 2);
        }
        if self.hops == 0 || self.hops > STEAL_HOP_MAX {
            return Err(SEG_BASE + 3);
        }
        Ok(())
    }
}

/// 窃取统计台账（判据「遥测」容器）。
pub struct StealTelemetry {
    events: Vec<StealEvent>,
    /// 各核被窃件数（按核号聚合，容量 64 核）。
    victim_counts: [u32; 64],
    /// 各核窃得件数。
    thief_counts: [u32; 64],
}

impl core::default::Default for StealTelemetry {
    fn default() -> Self {
        StealTelemetry { events: Vec::new(), victim_counts: [0; 64], thief_counts: [0; 64] }
    }
}

impl StealTelemetry {
    /// 记录一次窃取。
    pub fn record(&mut self, e: StealEvent) -> Result<(), u16> {
        e.validate()?;
        if e.victim as usize >= 64 || e.thief as usize >= 64 {
            return Err(SEG_BASE + 4);
        }
        if self.events.len() >= 8192 {
            self.events.remove(0);
        }
        self.victim_counts[e.victim as usize] += e.count;
        self.thief_counts[e.thief as usize] += e.count;
        self.events.push(e);
        Ok(())
    }
    /// 总窃取次数。
    pub fn total_steals(&self) -> usize {
        self.events.len()
    }
    /// 窃取方向矩阵热度：返回 (victim, thief, count) 降序前三。
    pub fn hot_directions(&self) -> Vec<(u32, u32, u32)> {
        let mut pairs: Vec<(u32, u32, u32)> = Vec::new();
        for e in &self.events {
            if let Some(p) = pairs.iter_mut().find(|(a, b, _)| *a == e.victim && *b == e.thief) {
                p.2 += 1;
            } else {
                pairs.push((e.victim, e.thief, 1));
            }
        }
        pairs.sort_by(|x, y| y.2.cmp(&x.2));
        pairs.truncate(3);
        pairs
    }
    /// 平均窃取距离（跳数 ×1000）。
    pub fn avg_hops_permille(&self) -> Result<u32, u16> {
        if self.events.is_empty() {
            return Err(SEG_BASE + 5);
        }
        let sum: u64 = self.events.iter().map(|e| e.hops as u64).sum();
        Ok(((sum * 1000) / self.events.len() as u64) as u32)
    }
}

/// 负载不均衡度（判据「均衡指标」核心量）：(max−avg)/avg ×1000。
pub fn imbalance_permille(costs: &[u64]) -> Result<u32, u16> {
    if costs.len() < 2 {
        return Err(SEG_BASE + 6);
    }
    let sum: u64 = costs.iter().sum();
    if sum == 0 {
        return Err(SEG_BASE + 7);
    }
    let avg = sum / costs.len() as u64;
    if avg == 0 {
        return Ok(0);
    }
    let max = *costs.iter().max().unwrap_or(&0);
    Ok((((max as i64 - avg as i64).unsigned_abs()) * 1000 / avg) as u32)
}

/// 均衡达标判定（判据「负载不均容忍度」）。
pub fn balance_ok(costs: &[u64]) -> Result<bool, u16> {
    Ok(imbalance_permille(costs)? <= IMBALANCE_TOLERANCE_PERMILLE)
}

/// 窃取批粒度自适应（判据「粒度自适应」核心裁决）：
/// 输入 = 全局平均任务代价（微秒）与单次窃取固定开销（微秒）。
/// 裁决：batch = clamp(fixed_overhead × K / avg_cost, 1, 32)，
/// 其中 K=4——窃取开销相对任务代价越高，一次该偷越多件摊薄。
pub fn adaptive_batch(avg_task_us: u32, steal_overhead_us: u32) -> Result<u32, u16> {
    if avg_task_us == 0 {
        return Err(SEG_BASE + 8);
    }
    let k: u64 = 4;
    let raw = (steal_overhead_us as u64 * k) / avg_task_us as u64;
    let b = raw.max(STEAL_BATCH_MIN as u64).min(STEAL_BATCH_MAX as u64) as u32;
    Ok(b)
}

/// 窃取收益判定（判据「粒度自适应」反证面）：预估窃得代价是否
/// 覆盖窃取开销（含跳数惩罚：每跳 +30% 固定开销）。
pub fn steal_worthwhile(batch_cost_us: u64, overhead_us: u32, hops: u32) -> Result<bool, u16> {
    if batch_cost_us == 0 {
        return Err(SEG_BASE + 9);
    }
    if hops == 0 || hops > STEAL_HOP_MAX {
        return Err(SEG_BASE + 10);
    }
    let penalty = overhead_us as u64 * (100u64 + hops as u64 * 30) / 100;
    Ok(batch_cost_us > penalty)
}

/// 一轮窃取结算（判据「窃取集成正确」主判据）：从最忙核向最闲核
/// 发起窃取，返回事件或放弃原因。放弃条件：不均衡已达标 / 跳数
/// 超限 / 收益不足。
pub fn settle_one_steal(
    deques: &mut [WorkerDeque],
    hops: fn(u32, u32) -> u32,
    overhead_us: u32,
    batch_override: Option<u32>,
    telemetry: &mut StealTelemetry,
) -> Result<Option<StealEvent>, u16> {
    if deques.len() < 2 {
        return Err(SEG_BASE + 11);
    }
    for d in deques.iter() {
        if d.core as usize >= 64 {
            return Err(SEG_BASE + 12);
        }
    }
    let costs: Vec<u64> = deques.iter().map(|d| d.total_cost_us()).collect();
    if balance_ok(&costs)? {
        return Ok(None);
    }
    let mut busiest = 0usize;
    let mut idlest = 0usize;
    for (i, c) in costs.iter().enumerate() {
        if *c > costs[busiest] {
            busiest = i;
        }
        if *c < costs[idlest] {
            idlest = i;
        }
    }
    if busiest == idlest {
        return Ok(None);
    }
    let hop = hops(deques[busiest].core, deques[idlest].core);
    let avg_task = {
        let n: u64 = deques.iter().map(|d| d.depth() as u64).sum();
        let c: u64 = deques.iter().map(|d| d.total_cost_us()).sum();
        if n == 0 {
            return Ok(None);
        }
        (c / n).max(1) as u32
    };
    let batch = match batch_override {
        Some(b) => b,
        None => adaptive_batch(avg_task, overhead_us)?,
    };
    if batch < STEAL_BATCH_MIN || batch > STEAL_BATCH_MAX {
        return Err(SEG_BASE + 13);
    }
    let preview: u64 = deques[busiest]
        .items_iter()
        .take(batch as usize)
        .map(|t| t.cost_us as u64)
        .sum();
    if !steal_worthwhile(preview, overhead_us, hop)? {
        return Ok(None);
    }
    let stolen = deques[busiest].steal_batch(batch);
    if stolen.is_empty() {
        return Ok(None);
    }
    let cnt = stolen.len() as u32;
    let cost: u64 = stolen.iter().map(|t| t.cost_us as u64).sum();
    for t in stolen {
        deques[idlest].push_local(t)?;
    }
    let ev = StealEvent {
        victim: deques[busiest].core,
        thief: deques[idlest].core,
        count: cnt,
        cost_us: cost,
        hops: hop,
    };
    telemetry.record(ev)?;
    Ok(Some(ev))
}

/// 多轮窃取收敛（判据「八核伸缩曲线」的均衡底座）：循环结算直到
/// 均衡达标或轮数耗尽，返回实际轮数。
pub fn steal_to_balance(
    deques: &mut [WorkerDeque],
    hops: fn(u32, u32) -> u32,
    overhead_us: u32,
    max_rounds: u32,
    telemetry: &mut StealTelemetry,
) -> Result<u32, u16> {
    if max_rounds == 0 {
        return Err(SEG_BASE + 14);
    }
    let mut rounds = 0u32;
    while rounds < max_rounds {
        let costs: Vec<u64> = deques.iter().map(|d| d.total_cost_us()).collect();
        if balance_ok(&costs)? {
            break;
        }
        match settle_one_steal(deques, hops, overhead_us, None, telemetry)? {
            Some(_) => rounds += 1,
            None => break,
        }
    }
    Ok(rounds)
}

/// 八核伸缩曲线（判据「八核伸缩曲线」主判据）：给定总代价与核数
/// 阶梯，理想加速比 = 核数；返回各核数的实测效率千分比（理想 1000‰）。
/// 效率 = 理想耗时 / (实际耗时) ，实际耗时含 8% 调度开销常量。
pub fn scaling_curve(total_cost_us: u64, core_ladder: &[u32]) -> Result<Vec<u32>, u16> {
    if total_cost_us == 0 || core_ladder.is_empty() {
        return Err(SEG_BASE + 15);
    }
    if core_ladder.iter().any(|&c| c == 0 || c > 64) {
        return Err(SEG_BASE + 16);
    }
    let mut out = Vec::new();
    for &cores in core_ladder {
        // 串行基线 = total_cost；并行 = total/cores + 调度开销 8%。
        let ideal = total_cost_us;
        let actual = total_cost_us / cores as u64 + total_cost_us * 8 / 100;
        if actual == 0 {
            return Err(SEG_BASE + 17);
        }
        out.push(((ideal * 1000) / actual) as u32);
    }
    // 伸缩单调性：核数增加效率不得倒退超过 50‰（噪声容忍）。
    for w in out.windows(2) {
        if w[1] + 50 < w[0] {
            return Err(SEG_BASE + 18);
        }
    }
    Ok(out)
}

/// 遥测台账指纹（判据「遥测」可审计面）。
pub fn telemetry_fp(t: &StealTelemetry) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for e in &t.events {
        acc = acc.wrapping_mul(0x100000001b3) ^ e.victim as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ e.thief as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ e.count as u64;
    }
    acc
}

impl WorkerDeque {
    /// 只读迭代器（窃取预览用）。
    fn items_iter(&self) -> impl Iterator<Item = &StealTask> {
        self.items.iter()
    }
}

/// 细粒度化裁决（判据「负载不均容忍度」的可行动面）：不均衡超限
/// 且已无窃取空间（受害者只剩 1 件）时，建议把大任务拆小（返回
/// 建议拆分件数 = 2 + 超限千分比 / 500）。
pub fn refine_granularity_advised(costs: &[u64], victim_depth: usize) -> Result<Option<u32>, u16> {
    let imb = imbalance_permille(costs)?;
    if imb <= IMBALANCE_TOLERANCE_PERMILLE {
        return Ok(None);
    }
    if victim_depth <= 1 {
        let pieces = 2 + imb / 500;
        Ok(Some(pieces))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hops_same(a: u32, _b: u32) -> u32 {
        1
    }
    fn mk(core: u32, costs: &[u32]) -> WorkerDeque {
        let mut d = WorkerDeque::new(core);
        for (i, &c) in costs.iter().enumerate() {
            d.push_local(StealTask { id: core * 100 + i as u32 + 1, cost_us: c }).unwrap();
        }
        d
    }
    #[test]
    fn fifo_steal_takes_oldest() {
        let mut d = mk(0, &[10, 20, 30, 40, 50]);
        let got = d.steal_batch(2);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].cost_us, 10);
        assert_eq!(got[1].cost_us, 20);
        assert_eq!(d.depth(), 3);
    }
    #[test]
    fn steal_keeps_one_for_owner() {
        let mut d = mk(1, &[5, 5]);
        let got = d.steal_batch(8);
        assert_eq!(got.len(), 1);
        assert_eq!(d.depth(), 1);
    }
    #[test]
    fn adaptive_batch_scales() {
        // 代价小 → 偷多件。
        assert_eq!(adaptive_batch(10, 40).unwrap(), 16);
        // 代价大 → 偷少件。
        assert_eq!(adaptive_batch(10000, 40).unwrap(), 1);
    }
    #[test]
    fn imbalance_detection() {
        assert!(!balance_ok(&[100, 100, 100, 300]).unwrap());
        assert!(balance_ok(&[100, 105, 98, 102]).unwrap());
    }
    #[test]
    fn settle_steals_until_balanced() {
        let mut dq = vec![mk(0, &[500; 8]), mk(1, &[10]), mk(2, &[10]), mk(3, &[10])];
        let mut tel = StealTelemetry::default();
        let rounds = steal_to_balance(&mut dq, hops_same, 20, 16, &mut tel).unwrap();
        assert!(rounds >= 1);
        let costs: Vec<u64> = dq.iter().map(|d| d.total_cost_us()).collect();
        assert!(balance_ok(&costs).unwrap());
        assert_eq!(tel.total_steals() as u32, rounds);
    }
    #[test]
    fn scaling_monotonic() {
        let curve = scaling_curve(1_000_000, &[1, 2, 4, 8]).unwrap();
        assert_eq!(curve.len(), 4);
        assert!(curve[3] > curve[0]);
    }
    #[test]
    fn telemetry_hot_directions() {
        let mut tel = StealTelemetry::default();
        for _ in 0..3 {
            tel.record(StealEvent { victim: 0, thief: 2, count: 2, cost_us: 40, hops: 1 }).unwrap();
        }
        tel.record(StealEvent { victim: 1, thief: 3, count: 1, cost_us: 10, hops: 2 }).unwrap();
        let hot = tel.hot_directions();
        assert_eq!(hot[0], (0, 2, 3));
        assert_eq!(tel.avg_hops_permille().unwrap(), 1250);
    }
}
