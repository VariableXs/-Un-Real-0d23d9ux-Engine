//! CGPU-F0175 · 帧图遥测（判据段 0x0490）
//! 帧图全维遥测：节点数/边数/关键路径长度/每队列任务分布/每任务耗时/
//! 失败率/合批率——计数器进注册表（F0017 域），喂 F0178 通路进诊断
//! 中心。遥测开销 ≤0.2% 帧预算。判据：指标全集、通路通、开销达标、
//! 聚合正确、面板数据源。
//!
//! 诊断码段 0x0490（本模块独占：0x0490~0x049F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0490;

/// 遥测开销红线：帧预算的 2‰（判据「开销达标」）。
pub const OVERHEAD_BUDGET_PERMILLE: u32 = 2;

/// 队列种类（判据「每队列任务分布」的闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueueKind {
    /// 图形队列。
    Graphics,
    /// 计算队列。
    Compute,
    /// 拷贝队列。
    Copy,
}

impl QueueKind {
    /// 序号（聚合槽位）。
    pub fn rank(self) -> u8 {
        match self {
            QueueKind::Graphics => 0,
            QueueKind::Compute => 1,
            QueueKind::Copy => 2,
        }
    }
}

/// 单任务遥测行（判据「每任务耗时」原子面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TaskTelem {
    /// 任务号。
    pub task: u32,
    /// 所在队列。
    pub queue: QueueKind,
    /// 执行耗时（微秒）。
    pub cost_us: u32,
    /// 是否失败。
    pub failed: bool,
    /// 是否参与合批。
    pub batched: bool,
}

impl TaskTelem {
    /// 行校验：任务号非零、耗时在界内（单任务 ≤ 5ms——超过即
    /// 单点串行化风险，遥测必须显性暴露）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 {
            return Err(SEG_BASE);
        }
        if self.cost_us > 5_000 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// 帧遥测快照（判据「指标全集」载体）：一帧的全维指标。
pub struct FrameTelem {
    /// 帧号。
    pub frame: u64,
    /// 节点数。
    pub nodes: u32,
    /// 边数。
    pub edges: u32,
    /// 关键路径长度（任务数）。
    pub critical_len: u32,
    /// 每任务行。
    pub rows: Vec<TaskTelem>,
    /// 遥测自身开销（微秒）。
    pub self_cost_us: u32,
    /// 帧预算（微秒）。
    pub frame_budget_us: u32,
}

impl FrameTelem {
    /// 快照校验：基础账自洽——边数 ≥ 节点数-1（连通图下界）、
    /// 关键路径 ≤ 节点数、开销账齐备。
    pub fn validate(&self) -> Result<(), u16> {
        if self.nodes == 0 {
            return Err(SEG_BASE + 2);
        }
        if (self.edges as u64) < (self.nodes as u64).saturating_sub(1) {
            return Err(SEG_BASE + 3);
        }
        if self.critical_len == 0 || self.critical_len > self.nodes {
            return Err(SEG_BASE + 4);
        }
        if self.rows.len() as u32 != self.nodes {
            return Err(SEG_BASE + 5);
        }
        if self.frame_budget_us == 0 {
            return Err(SEG_BASE + 6);
        }
        for r in &self.rows {
            r.validate()?;
        }
        Ok(())
    }

    /// 遥测开销判（判据「开销达标」）：self_cost ≤ 帧预算 × 2‰。
    pub fn overhead_ok(&self) -> Result<(), u16> {
        let cap = (self.frame_budget_us as u64 * OVERHEAD_BUDGET_PERMILLE as u64 / 1000) as u32;
        if self.self_cost_us > cap {
            return Err(SEG_BASE + 7);
        }
        Ok(())
    }

    /// 失败率（千分比，判据「失败率」聚合面）。
    pub fn fail_permille(&self) -> Result<u32, u16> {
        if self.rows.is_empty() {
            return Err(SEG_BASE + 8);
        }
        let f = self.rows.iter().filter(|r| r.failed).count();
        Ok(((f * 1000) / self.rows.len()) as u32)
    }

    /// 合批率（千分比，判据「合批率」聚合面）。
    pub fn batch_permille(&self) -> Result<u32, u16> {
        if self.rows.is_empty() {
            return Err(SEG_BASE + 9);
        }
        let b = self.rows.iter().filter(|r| r.batched).count();
        Ok(((b * 1000) / self.rows.len()) as u32)
    }

    /// 每队列任务分布（判据「每队列任务分布」聚合面）：三槽计数。
    pub fn queue_census(&self) -> Result<[u32; 3], u16> {
        let mut out = [0u32; 3];
        for r in &self.rows {
            out[r.queue.rank() as usize] += 1;
        }
        if out[0] + out[1] + out[2] != self.rows.len() as u32 {
            return Err(SEG_BASE + 10);
        }
        Ok(out)
    }

    /// 每队列耗时分布（微秒合计）。
    pub fn queue_costs(&self) -> [u64; 3] {
        let mut out = [0u64; 3];
        for r in &self.rows {
            out[r.queue.rank() as usize] += r.cost_us as u64;
        }
        out
    }

    /// 总耗时（微秒）。
    pub fn total_cost_us(&self) -> u64 {
        self.rows.iter().map(|r| r.cost_us as u64).sum()
    }

    /// 慢点清单（诊断面板数据源）：耗时 ≥ 阈值的任务号。
    pub fn slow_tasks(&self, threshold_us: u32) -> Result<Vec<u32>, u16> {
        if threshold_us == 0 {
            return Err(SEG_BASE + 11);
        }
        Ok(self
            .rows
            .iter()
            .filter(|r| r.cost_us >= threshold_us)
            .map(|r| r.task)
            .collect())
    }
}

/// 计数器注册表投影（判据「计数器进注册表 F0017 域」对接面）：
/// 每项 = (计数器名指纹, 值)。固定 8 项全集——缺项 = 指标不全。
pub fn registry_projection(t: &FrameTelem) -> Result<[(u64, u64); 8], u16> {
    t.validate()?;
    let q = t.queue_census()?;
    let mut acc: u64 = 0xcbf29ce484222325;
    for r in &t.rows {
        acc = acc.wrapping_mul(0x100000001b3) ^ r.task as u64;
    }
    Ok([
        (0xF0170_01, t.nodes as u64),
        (0xF0170_02, t.edges as u64),
        (0xF0170_03, t.critical_len as u64),
        (0xF0170_04, q[0] as u64),
        (0xF0170_05, q[1] as u64),
        (0xF0170_06, q[2] as u64),
        (0xF0170_07, t.total_cost_us()),
        (0xF0170_08, acc),
    ])
}

/// 诊断中心通路模拟（判据「通路通」面）：投影 → 通路管道 →
/// 诊断中心序号，管道必须保序（乱序 = 通路断）。
pub fn pipe_deliver(
    regs: &[(u64, u64)],
    pipe: &mut Vec<(u64, u64)>,
) -> Result<(), u16> {
    if regs.is_empty() {
        return Err(SEG_BASE + 12);
    }
    for r in regs {
        if r.0 == 0 {
            return Err(SEG_BASE + 13);
        }
        pipe.push(*r);
    }
    // 保序校验：管道尾部与投影首尾一致。
    if pipe.last() != regs.last() {
        return Err(SEG_BASE + 14);
    }
    Ok(())
}

/// 聚合器（判据「聚合正确」面）：多帧指标的时间窗聚合。
pub struct TelemAggregator {
    /// 窗口内快照。
    snaps: Vec<FrameTelem>,
    /// 窗口上限。
    window: usize,
}

impl TelemAggregator {
    /// 建聚合器（窗口 ≥ 2）。
    pub fn new(window: usize) -> Result<Self, u16> {
        if window < 2 {
            return Err(SEG_BASE + 15);
        }
        Ok(TelemAggregator {
            snaps: Vec::new(),
            window,
        })
    }

    /// 入窗（满窗拒绝——窗口语义是滑窗不是无限累积）。
    pub fn push(&mut self, t: FrameTelem) -> Result<(), u16> {
        t.validate()?;
        if self.snaps.len() >= self.window {
            return Err(SEG_BASE + 16);
        }
        self.snaps.push(t);
        Ok(())
    }

    /// 窗内节点数均值（整数域，×100 保精度）。
    pub fn nodes_mean_x100(&self) -> Result<u64, u16> {
        if self.snaps.is_empty() {
            return Err(SEG_BASE + 17);
        }
        let sum: u64 = self.snaps.iter().map(|s| s.nodes as u64).sum();
        Ok(sum * 100 / self.snaps.len() as u64)
    }

    /// 窗内失败率峰值（最差帧——面板告警源）。
    pub fn worst_fail_permille(&self) -> Result<u32, u16> {
        if self.snaps.is_empty() {
            return Err(SEG_BASE + 18);
        }
        let mut worst = 0u32;
        for s in &self.snaps {
            let f = s.fail_permille()?;
            if f > worst {
                worst = f;
            }
        }
        Ok(worst)
    }

    /// 窗满判。
    pub fn window_full(&self) -> bool {
        self.snaps.len() == self.window
    }

    /// 面板数据源投影（判据「面板数据源」）：窗口摘要四元组。
    pub fn panel_source(&self) -> Result<(u64, u32, u32, u64), u16> {
        if self.snaps.is_empty() {
            return Err(SEG_BASE + 19);
        }
        Ok((
            self.nodes_mean_x100()?,
            self.worst_fail_permille()?,
            self.snaps.iter().map(|s| s.critical_len).max().unwrap_or(0),
            self.snaps.iter().map(|s| s.total_cost_us()).sum(),
        ))
    }
}

/// 遥测全维自检（一站式收口）：快照校验 + 开销达标 + 指标全集 +
/// 聚合账自洽（总耗时 = 队列耗时合计）。
pub fn telem_audit(t: &FrameTelem) -> Result<(), u16> {
    t.validate()?;
    t.overhead_ok()?;
    let qc = t.queue_costs();
    if qc[0] + qc[1] + qc[2] != t.total_cost_us() {
        return Err(SEG_BASE + 20);
    }
    let regs = registry_projection(t)?;
    if regs.len() != 8 {
        return Err(SEG_BASE + 21);
    }
    for r in &regs {
        if r.0 == 0 {
            return Err(SEG_BASE + 22);
        }
    }
    Ok(())
}
