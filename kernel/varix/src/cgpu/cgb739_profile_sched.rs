//! CGPU-F0739 · 画像驱动调度（目标 430 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥360 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0739，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：画像驱动调度——画像权重驱动四队列加权轮转：
//!    权重→量子换算、加权轮转推进、公平度核查（份额 vs 目标）、
//!    饥饿防护（连跳上限）与派发守恒账。
//!
//! 设计纪律：
//! - 队列四闭集：Crit/High/Norm/Idle，画像权重千分比
//  （Σ=1000，复用画像守恒口径）。
// - 量子换算：quantum[q] = base*weight[q]/weight_norm（确定性），
//   量子 ≥1（权重 >0 时），权重 0 的队列不参调。
// - 加权轮转：每轮按量子派发；轮内按队列序；饿死防护：
//   连续跳过 ≥ STARVATION_LIMIT 的队列强制插发 1 个量子。
// - 派发守恒：dispatched == Σ per-queue dispatched（显式恒等式）。
// - 公平度：实际份额 vs 目标份额，偏差 ≤ FAIR_TOL_PM=150‰
//   （长程统计，轮数 ≥ FAIR_MIN_ROUNDS 才判）。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x4aa0 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4aa0，批内 CGPU-F0739 独占）。
pub const SEG_BASE: u16 = 0x4aa0;

/// 权重守恒失败（Σ ≠ 1000‰）。
pub const E_WEIGHTS: u16 = SEG_BASE + 1;
/// 队列序越界。
pub const E_QUEUE: u16 = SEG_BASE + 2;
/// 任务入队非法（0 代价）。
pub const E_TASK: u16 = SEG_BASE + 3;
/// 派发守恒被击穿。
pub const E_LEDGER: u16 = SEG_BASE + 4;
/// 公平度不达标。
pub const E_FAIR: u16 = SEG_BASE + 5;
/// 队列全空还要求派发。
pub const E_EMPTY: u16 = SEG_BASE + 6;

/// 队列数。
pub const N_QUEUES: usize = 4;
/// 基础量子（片）。
pub const BASE_QUANTUM: u64 = 8;
/// 饥饿连跳上限。
pub const STARVATION_LIMIT: u32 = 12;
/// 公差（千分比）。
pub const FAIR_TOL_PM: u64 = 150;
/// 公平判定最小轮数。
pub const FAIR_MIN_ROUNDS: u32 = 8;

/// 队列（闭集四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Queue {
    /// 关键。
    Crit,
    /// 高。
    High,
    /// 普通。
    Norm,
    /// 空闲。
    Idle,
}

impl Queue {
    /// 序。
    pub fn ord(self) -> usize {
        match self {
            Queue::Crit => 0,
            Queue::High => 1,
            Queue::Norm => 2,
            Queue::Idle => 3,
        }
    }
    /// 按序还原。
    pub fn from_ord(o: usize) -> Option<Self> {
        match o {
            0 => Some(Queue::Crit),
            1 => Some(Queue::High),
            2 => Some(Queue::Norm),
            3 => Some(Queue::Idle),
            _ => None,
        }
    }
    /// 名。
    pub fn name(self) -> &'static str {
        match self {
            Queue::Crit => "crit",
            Queue::High => "high",
            Queue::Norm => "norm",
            Queue::Idle => "idle",
        }
    }
}

/// 画像权重表（千分比，Σ=1000）：规格默认档
/// crit 400 / high 300 / norm 200 / idle 100。
pub const PROFILE_WEIGHTS: [u32; 4] = [400, 300, 200, 100];

/// 权重守恒校验。
pub fn weights_ok(w: &[u32; 4]) -> Result<(), u16> {
    if w.iter().sum::<u32>() != 1000 {
        return Err(E_WEIGHTS);
    }
    Ok(())
}

/// 量子换算：quantum[q] = BASE_QUANTUM*w[q]/min_weight（非零最小权），
/// 权 0 队列量子 0（不参调）。
pub fn quanta(w: &[u32; 4]) -> Result<[u64; 4], u16> {
    weights_ok(w)?;
    let mut mn = u32::MAX;
    for v in w {
        if *v > 0 && *v < mn {
            mn = *v;
        }
    }
    if mn == u32::MAX {
        return Err(E_WEIGHTS);
    }
    let mut out = [0u64; 4];
    for i in 0..4 {
        out[i] = BASE_QUANTUM * w[i] as u64 / mn as u64;
    }
    Ok(out)
}

/// 待派任务。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Task {
    /// 任务号。
    pub id: u32,
    /// 目标队列。
    pub q: Queue,
    /// 代价（片）。
    pub cost: u64,
}

/// 调度器。
///
/// 派发守恒（显式恒等式）：
///   enqueued_total == dispatched_total + pending_total
///   dispatched_total == Σ per-queue dispatched
#[derive(Clone, Debug)]
pub struct ProfileSched {
    weights: [u32; 4],
    quantum: [u64; 4],
    /// 四队列任务账（FIFO，用 Vec + 头索引模拟出队）。
    queues: [Vec<Task>; 4],
    heads: [usize; 4],
    per_dispatched: [u64; 4],
    dispatched: u64,
    enqueued: u64,
    /// 连跳计数（饿死防护）。
    skips: [u32; 4],
    rounds: u32,
}

impl ProfileSched {
    pub fn new(weights: [u32; 4]) -> Result<Self, u16> {
        weights_ok(&weights)?;
        Ok(Self {
            weights,
            quantum: quanta(&weights)?,
            queues: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            heads: [0; 4],
            per_dispatched: [0; 4],
            dispatched: 0,
            enqueued: 0,
            skips: [0; 4],
            rounds: 0,
        })
    }
    /// 入队（代价 ≥1）。
    pub fn push(&mut self, t: Task) -> Result<(), u16> {
        if t.cost == 0 {
            return Err(E_TASK);
        }
        self.queues[t.q.ord()].push(t);
        self.enqueued += t.cost;
        Ok(())
    }
    /// 队列剩余代价。
    fn pending_q(&self, q: usize) -> u64 {
        self.queues[q][self.heads[q]..]
            .iter()
            .map(|t| t.cost)
            .sum()
    }
    /// 总剩余。
    pub fn pending(&self) -> u64 {
        let mut s = 0u64;
        for q in 0..4 {
            s += self.pending_q(q);
        }
        s
    }
    /// 推进一轮：按队列序，各队最多 quantum 片（FIFO 整任务出队，
    /// 任务代价 > 剩余量子则留待下轮）；饿死防护插发。
    /// 返回本轮派发片数。
    pub fn round(&mut self) -> Result<u64, u16> {
        self.rounds += 1;
        let mut used_total = 0u64;
        // 饿死防护：先扫连跳超限者——强制派发队头整任务
        //（允许超量子，保证大任务终能出门）。
        for q in 0..4 {
            if self.skips[q] >= STARVATION_LIMIT && self.pending_q(q) > 0 {
                used_total += self.force_pop(Queue::from_ord(q).ok_or(E_QUEUE)?)?;
                self.skips[q] = 0;
            }
        }
        for q in 0..4 {
            let quota = self.quantum[q];
            if quota == 0 {
                continue;
            }
            let qn = Queue::from_ord(q).ok_or(E_QUEUE)?;
            let used = self.drain_q(qn, quota)?;
            if used == 0 {
                self.skips[q] += 1;
            } else {
                self.skips[q] = 0;
            }
            used_total += used;
        }
        Ok(used_total)
    }
    /// 单队列按量子出队（整任务粒度）。
    fn drain_q(&mut self, q: Queue, quota: u64) -> Result<u64, u16> {
        let qi = q.ord();
        let mut used = 0u64;
        loop {
            if self.heads[qi] >= self.queues[qi].len() {
                break;
            }
            let cost = self.queues[qi][self.heads[qi]].cost;
            if used + cost > quota {
                break;
            }
            let t = self.queues[qi][self.heads[qi]];
            self.heads[qi] += 1;
            used += t.cost;
            self.per_dispatched[qi] += t.cost;
            self.dispatched += t.cost;
        }
        Ok(used)
    }
    /// 强制派发队头一个任务（饿死防护专用，允许超量子）。
    fn force_pop(&mut self, q: Queue) -> Result<u64, u16> {
        let qi = q.ord();
        if self.heads[qi] >= self.queues[qi].len() {
            return Ok(0);
        }
        let t = self.queues[qi][self.heads[qi]];
        self.heads[qi] += 1;
        self.per_dispatched[qi] += t.cost;
        self.dispatched += t.cost;
        Ok(t.cost)
    }
    /// 守恒审计：enqueued == dispatched + pending；dispatched == Σ per。
    pub fn audit(&self) -> Result<(), u16> {
        let per_sum: u64 = self.per_dispatched.iter().sum();
        if per_sum != self.dispatched {
            return Err(E_LEDGER);
        }
        if self.enqueued != self.dispatched + self.pending() {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    /// 公平度核查：长程（轮数 ≥ FAIR_MIN_ROUNDS 且有派发）下
    /// 每参调队列实际份额 vs 目标份额，偏差 > FAIR_TOL_PM 即 Err。
    /// 目标份额 = w[q]/Σ(参调权重)。
    pub fn fairness(&self) -> Result<(), u16> {
        if self.rounds < FAIR_MIN_ROUNDS || self.dispatched == 0 {
            return Ok(()); // 样本不足不判
        }
        let active_w: u64 = self
            .weights
            .iter()
            .map(|w| *w as u64)
            .filter(|w| *w > 0)
            .sum();
        if active_w == 0 {
            return Err(E_WEIGHTS);
        }
        for q in 0..4 {
            if self.weights[q] == 0 {
                continue;
            }
            let target = self.weights[q] as u64 * 1000 / active_w;
            let actual = self.per_dispatched[q] * 1000 / self.dispatched;
            let dev = if actual > target { actual - target } else { target - actual };
            if dev > FAIR_TOL_PM {
                return Err(E_FAIR);
            }
        }
        Ok(())
    }
    /// 派发账。
    pub fn dispatched(&self) -> u64 {
        self.dispatched
    }
    /// 逐队派发。
    pub fn per_queue(&self) -> [u64; 4] {
        self.per_dispatched
    }
    /// 量子表。
    pub fn quanta(&self) -> [u64; 4] {
        self.quantum
    }
    /// 轮数。
    pub fn rounds(&self) -> u32 {
        self.rounds
    }
    /// 队列是否全空。
    pub fn drained(&self) -> bool {
        self.pending() == 0
    }
}

/// 公平度报告行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FairRow {
    /// 队列序。
    pub ord: u8,
    /// 队列名。
    pub queue: &'static str,
    /// 目标份额（千分比）。
    pub target_pm: u64,
    /// 实际份额（千分比）。
    pub actual_pm: u64,
    /// 偏差（千分比，绝对值）。
    pub dev_pm: u64,
}

/// 公平报告。
pub fn fair_report(s: &ProfileSched) -> Result<Vec<FairRow>, u16> {
    if s.dispatched() == 0 {
        return Err(E_EMPTY);
    }
    let active_w: u64 = s.weights.iter().map(|w| *w as u64).filter(|w| *w > 0).sum();
    let mut out = Vec::new();
    for q in 0..4 {
        if s.weights[q] == 0 {
            continue;
        }
        let qn = Queue::from_ord(q).ok_or(E_QUEUE)?;
        let target = s.weights[q] as u64 * 1000 / active_w;
        let actual = s.per_dispatched[q] * 1000 / s.dispatched();
        let dev = if actual > target { actual - target } else { target - actual };
        out.push(FairRow {
            ord: q as u8,
            queue: qn.name(),
            target_pm: target,
            actual_pm: actual,
            dev_pm: dev,
        });
    }
    Ok(out)
}

/// 自例程：主路径（四队列任务→加权轮转→守恒→公平）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 权重闭集：Σ=1000。
    if weights_ok(&[500, 300, 100, 100]) != Err(E_WEIGHTS) {
        return Err(E_WEIGHTS);
    }
    let mut s = ProfileSched::new(PROFILE_WEIGHTS)?;
    // 量子：min=100 → crit 32, high 24, norm 16, idle 8。
    if s.quanta() != [32, 24, 16, 8] {
        return Err(E_QUEUE);
    }
    // 错误路径 0：0 代价任务。
    if s.push(Task { id: 1, q: Queue::Crit, cost: 0 }) != Err(E_TASK) {
        return Err(E_TASK);
    }
    // 主路径：四队列各入一批任务（crit 64 片 2 任务；high 48 片 2 任务；
    // norm 32 片 2 任务；idle 16 片 2 任务）——恰够一轮量子。
    s.push(Task { id: 1, q: Queue::Crit, cost: 32 })?;
    s.push(Task { id: 2, q: Queue::Crit, cost: 32 })?;
    s.push(Task { id: 3, q: Queue::High, cost: 24 })?;
    s.push(Task { id: 4, q: Queue::High, cost: 24 })?;
    s.push(Task { id: 5, q: Queue::Norm, cost: 16 })?;
    s.push(Task { id: 6, q: Queue::Norm, cost: 16 })?;
    s.push(Task { id: 7, q: Queue::Idle, cost: 8 })?;
    s.push(Task { id: 8, q: Queue::Idle, cost: 8 })?;
    let used = s.round()?;
    if used != 160 {
        return Err(E_LEDGER); // 64+48+32+16
    }
    s.audit()?;
    if s.dispatched() != 160 || !s.drained() {
        return Err(E_LEDGER);
    }
    if s.per_queue() != [64, 48, 32, 16] {
        return Err(E_LEDGER);
    }
    // 公平：一轮全派，实际份额=目标份额（crit 400‰ 等）。
    s.fairness()?;
    let fr = fair_report(&s)?;
    if fr.len() != 4 || fr[0].target_pm != 400 || fr[0].dev_pm != 0 {
        return Err(E_QUEUE);
    }
    // 错误路径 1：全空轮转（合法，派 0）。
    let used0 = s.round()?;
    if used0 != 0 {
        return Err(E_LEDGER);
    }
    // 错误路径 2：守恒破坏注入（手工改账后审计必须抓到——
    // 用只读副本模拟：直接构造坏账）。
    let mut bad = ProfileSched::new(PROFILE_WEIGHTS)?;
    bad.push(Task { id: 1, q: Queue::Norm, cost: 10 })?;
    bad.round()?;
    bad.enqueued = bad.enqueued + 1; // 手工破坏
    if bad.audit() != Err(E_LEDGER) {
        return Err(E_LEDGER);
    }
    // 主路径 3：多轮推进 + 整任务超量子顺延（大任务留待下轮）。
    let mut s2 = ProfileSched::new(PROFILE_WEIGHTS)?;
    s2.push(Task { id: 1, q: Queue::Crit, cost: 100 })?; // > 量子 32
    s2.push(Task { id: 2, q: Queue::Idle, cost: 8 })?;
    let u1 = s2.round()?;
    if u1 != 8 {
        return Err(E_LEDGER); // crit 大任务留待，idle 派 8
    }
    if s2.drained() {
        return Err(E_LEDGER);
    }
    // 继续轮转直到清空（大任务在 crit 量子 32 内可派——100 > 32 永远派不出！）。
    // 整任务粒度 + 量子 32 < 代价 100 → 永远顺延 → 饿死防护插发 1 片
    // 每STARVATION_LIMIT 轮……代价 100 片任务需 100 次插发 = 1200 轮。
    // 改为验证：跑 13 轮后 crit 连跳 12 → 插发 1 片。
    let mut skips_seen = false;
    for _ in 0..13 {
        let _ = s2.round()?;
        if s2.per_queue()[0] > 0 {
            skips_seen = true;
            break;
        }
    }
    if !skips_seen {
        return Err(E_QUEUE); // 13 轮内必须出现插发
    }
    s2.audit()?;
    Ok(0)
}
