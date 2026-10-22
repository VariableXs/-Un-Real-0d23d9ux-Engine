//! CGPU-F0200 · 帧图实时监控面板（判据段 0x0620）
//! 帧图运行时的实时监控：实时 DAG 视图（当前帧的图拓扑实时显示
//! ——节点状态色标：等待/运行/完成/失败）、时间轴甘特图（任务实际
//! 执行时间条——与探针数据叠加）、预算消耗仪表（每任务/每队列的
//! 预算消耗实时条）。面板性能：监控数据采集 ≤0.1ms/帧。判据：
//! DAG 实时、甘特对齐、预算仪表、开销达标、Studio 集成。
//!
//! 诊断码段 0x0620（本模块独占：0x0620~0x062F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0620;

/// 采集开销红线（纳秒/帧）：≤100_000ns = 0.1ms。
pub const COLLECT_BUDGET_NS: u64 = 100_000;

/// 甘特时间轴分辨率（微秒/刻度）。
pub const GANTT_TICK_US: u32 = 100;

/// 节点运行状态（判据「DAG 实时」色标源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeState {
    /// 等待（依赖未就绪）。
    Pending,
    /// 运行中。
    Running,
    /// 完成。
    Done,
    /// 失败。
    Failed,
}

impl NodeState {
    /// 状态色标号（Studio 集成色表索引）。
    pub fn color_index(&self) -> u8 {
        match self {
            NodeState::Pending => 0,
            NodeState::Running => 1,
            NodeState::Done => 2,
            NodeState::Failed => 3,
        }
    }
}

/// 节点实时快照（判据「DAG 实时」原子量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NodeSnap {
    /// 节点号。
    pub node: u32,
    /// 当前状态。
    pub state: NodeState,
    /// 开始时刻（微秒，帧内相对）。
    pub start_us: u32,
    /// 结束时刻（微秒；未结束为 0）。
    pub end_us: u32,
}

impl NodeSnap {
    /// 快照合法性：状态与时间自洽。
    pub fn validate(&self) -> Result<(), u16> {
        if self.node == 0 {
            return Err(SEG_BASE);
        }
        match self.state {
            NodeState::Pending => {
                if self.start_us != 0 || self.end_us != 0 {
                    return Err(SEG_BASE + 1);
                }
            }
            NodeState::Running => {
                if self.start_us == 0 || self.end_us != 0 {
                    return Err(SEG_BASE + 2);
                }
            }
            NodeState::Done | NodeState::Failed => {
                if self.end_us == 0 || self.end_us <= self.start_us {
                    return Err(SEG_BASE + 3);
                }
            }
        }
        Ok(())
    }
    /// 实际执行耗时（未结束返回 0）。
    pub fn duration_us(&self) -> u32 {
        if self.end_us > self.start_us && self.end_us != 0 {
            self.end_us - self.start_us
        } else {
            0
        }
    }
}

/// 甘特条（判据「甘特对齐」渲染原子）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GanttBar {
    /// 节点号。
    pub node: u32,
    /// 起始刻度（GANTT_TICK_US 粒度）。
    pub tick_start: u32,
    /// 跨越刻度数。
    pub ticks: u32,
}

impl GanttBar {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.node == 0 || self.ticks == 0 {
            return Err(SEG_BASE + 4);
        }
        Ok(())
    }
}

/// 快照 → 甘特条（判据「甘特对齐」转换核心）：时间对齐到刻度栅格
/// ——起点向下取整、终点向上取整（不遗漏任何执行区间）。
pub fn snap_to_bar(s: &NodeSnap) -> Result<GanttBar, u16> {
    s.validate()?;
    if s.end_us == 0 {
        return Err(SEG_BASE + 5);
    }
    let tick_start = s.start_us / GANTT_TICK_US;
    let tick_end = (s.end_us + GANTT_TICK_US - 1) / GANTT_TICK_US;
    let ticks = tick_end - tick_start;
    if ticks == 0 {
        return Err(SEG_BASE + 6);
    }
    Ok(GanttBar { node: s.node, tick_start, ticks })
}

/// 甘特重叠检测（判据「甘特对齐」审计面）：同队列快照的时间条
/// 不得重叠（重叠 = 并行度记录错误）。
pub fn bars_disjoint(bars: &[GanttBar]) -> Result<bool, u16> {
    if bars.len() < 2 {
        return Err(SEG_BASE + 7);
    }
    let mut sorted: Vec<&GanttBar> = bars.iter().collect();
    sorted.sort_by_key(|b| b.tick_start);
    for w in sorted.windows(2) {
        if w[0].tick_start + w[0].ticks > w[1].tick_start {
            return Ok(false);
        }
    }
    Ok(true)
}

/// 预算仪表（判据「预算仪表」载体）：每任务的预算消耗实时条。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BudgetGauge {
    /// 任务号。
    pub task: u32,
    /// 预算（微秒）。
    pub budget_us: u32,
    /// 已消耗（微秒）。
    pub spent_us: u32,
}

impl BudgetGauge {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 || self.budget_us == 0 {
            return Err(SEG_BASE + 8);
        }
        Ok(())
    }
    /// 消耗千分比（可超 1000 = 超支）。
    pub fn usage_permille(&self) -> Result<u32, u16> {
        self.validate()?;
        Ok((self.spent_us as u64 * 1000 / self.budget_us as u64) as u32)
    }
    /// 仪表档位（Studio 条形色阶）：0=绿(<700‰) 1=黄(700~999‰)
    /// 2=红(≥1000‰)。
    pub fn gauge_level(&self) -> Result<u8, u16> {
        let u = self.usage_permille()?;
        Ok(if u < 700 {
            0
        } else if u < 1000 {
            1
        } else {
            2
        })
    }
}

/// 队列仪表汇总（判据「预算仪表」批量面）：队列内全部任务仪表的
/// 汇总条——总预算/总消耗/最高档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QueueGauge {
    /// 队列号。
    pub queue: u32,
    /// 队列总预算（微秒）。
    pub budget_us: u32,
    /// 队列总消耗（微秒）。
    pub spent_us: u32,
}

impl QueueGauge {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.queue == 0 || self.budget_us == 0 {
            return Err(SEG_BASE + 9);
        }
        Ok(())
    }
    /// 汇总消耗千分比。
    pub fn usage_permille(&self) -> Result<u32, u16> {
        self.validate()?;
        Ok((self.spent_us as u64 * 1000 / self.budget_us as u64) as u32)
    }
}

/// 任务仪表聚合为队列仪表（判据「预算仪表」聚合核心）：队列预算
/// 必须为正且 ≥ 任务预算之和（否则报配额配置错误）。
pub fn aggregate_queue(queue: u32, queue_budget_us: u32, gauges: &[BudgetGauge]) -> Result<QueueGauge, u16> {
    if gauges.is_empty() || queue_budget_us == 0 {
        return Err(SEG_BASE + 10);
    }
    let mut spent: u32 = 0;
    let mut budget_sum: u32 = 0;
    for g in gauges {
        g.validate()?;
        spent = spent.saturating_add(g.spent_us);
        budget_sum = budget_sum.saturating_add(g.budget_us);
    }
    if budget_sum > queue_budget_us {
        return Err(SEG_BASE + 11);
    }
    Ok(QueueGauge { queue, budget_us: queue_budget_us, spent_us: spent })
}

/// DAG 实时视图校验（判据「DAG 实时」主判据）：全帧快照的状态
/// 自洽——Done/Failed 的 end 必须有 start 且帧内单调；Running 的
/// start 不超过当前时刻。
pub fn dag_view_consistent(snaps: &[NodeSnap], now_us: u32) -> Result<usize, u16> {
    if snaps.is_empty() {
        return Err(SEG_BASE + 12);
    }
    let mut seen = vec![false; snaps.len() + 1];
    let mut finished = 0usize;
    for s in snaps {
        s.validate()?;
        if s.node as usize > snaps.len() || seen[s.node as usize - 1] {
            return Err(SEG_BASE + 13);
        }
        seen[s.node as usize - 1] = true;
        if s.state == NodeState::Running && s.start_us > now_us {
            return Err(SEG_BASE + 14);
        }
        if matches!(s.state, NodeState::Done | NodeState::Failed) {
            finished += 1;
        }
    }
    if seen.iter().take(snaps.len()).any(|s| !s) {
        return Err(SEG_BASE + 15);
    }
    Ok(finished)
}

/// 采集开销核算（判据「开销达标」主判据）：监控采集自身耗时（纳秒）
/// 不得超过 COLLECT_BUDGET_NS；返回实际开销占预算千分比。
pub fn collect_overhead_permille(collected_ns: u64, snapshot_count: u32) -> Result<u32, u16> {
    if collected_ns == 0 || snapshot_count == 0 {
        return Err(SEG_BASE + 16);
    }
    if collected_ns > COLLECT_BUDGET_NS {
        return Err(SEG_BASE + 17);
    }
    Ok((collected_ns * 1000 / COLLECT_BUDGET_NS) as u32)
}

/// Studio 集成协议帧（判据「Studio 集成」载体）：面板数据以定长
/// 协议帧输出——帧序号 + 快照数 + 数据指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PanelFrame {
    /// 帧号。
    pub frame: u32,
    /// 快照条数。
    pub snaps: u32,
    /// 数据指纹。
    pub fp: u64,
}

impl PanelFrame {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.frame == 0 || self.snaps == 0 || self.fp == 0 {
            return Err(SEG_BASE + 18);
        }
        Ok(())
    }
}

/// 面板帧指纹（判据「Studio 集成」可审计面）：快照序列折并。
pub fn panel_fp(frame: u32, snaps: &[NodeSnap]) -> Result<u64, u16> {
    if snaps.is_empty() {
        return Err(SEG_BASE + 19);
    }
    let mut acc: u64 = 0xcbf29ce484222325;
    acc = acc.wrapping_mul(0x100000001b3) ^ frame as u64;
    for s in snaps {
        acc = acc.wrapping_mul(0x100000001b3) ^ s.node as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ s.state.color_index() as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ s.duration_us() as u64;
    }
    Ok(acc)
}

/// 面板帧组包（判据「Studio 集成」组包核心）。
pub fn make_panel_frame(frame: u32, snaps: &[NodeSnap]) -> Result<PanelFrame, u16> {
    dag_view_consistent(snaps, snaps.iter().map(|s| s.end_us).max().unwrap_or(0))?;
    let fp = panel_fp(frame, snaps)?;
    let f = PanelFrame { frame, snaps: snaps.len() as u32, fp };
    f.validate()?;
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snap(node: u32, state: NodeState, s: u32, e: u32) -> NodeSnap {
        NodeSnap { node, state, start_us: s, end_us: e }
    }
    #[test]
    fn snapshot_state_time_selfconsistent() {
        assert!(snap(1, NodeState::Done, 100, 500).validate().is_ok());
        assert!(snap(1, NodeState::Done, 500, 100).validate().is_err());
        assert!(snap(1, NodeState::Pending, 10, 0).validate().is_err());
        assert!(snap(1, NodeState::Running, 100, 200).validate().is_err());
    }
    #[test]
    fn gantt_grid_alignment() {
        let s = snap(1, NodeState::Done, 250, 480);
        let b = snap_to_bar(&s).unwrap();
        assert_eq!(b.tick_start, 2);
        assert_eq!(b.ticks, 3); // 250~480 覆盖 200/300/400 刻度起点。
        assert!(bars_disjoint(&[b, GanttBar { node: 2, tick_start: 6, ticks: 2 }]).unwrap());
        assert!(!bars_disjoint(&[b, GanttBar { node: 2, tick_start: 4, ticks: 2 }]).unwrap());
    }
    #[test]
    fn gauge_levels() {
        let g = BudgetGauge { task: 1, budget_us: 1000, spent_us: 650 };
        assert_eq!(g.gauge_level().unwrap(), 0);
        let g2 = BudgetGauge { task: 2, budget_us: 1000, spent_us: 800 };
        assert_eq!(g2.gauge_level().unwrap(), 1);
        let g3 = BudgetGauge { task: 3, budget_us: 1000, spent_us: 1200 };
        assert_eq!(g3.gauge_level().unwrap(), 2);
    }
    #[test]
    fn queue_aggregation_quota_check() {
        let gs = vec![
            BudgetGauge { task: 1, budget_us: 400, spent_us: 200 },
            BudgetGauge { task: 2, budget_us: 400, spent_us: 350 },
        ];
        let q = aggregate_queue(1, 1000, &gs).unwrap();
        assert_eq!(q.usage_permille().unwrap(), 550);
        assert!(aggregate_queue(1, 700, &gs).is_err());
    }
    #[test]
    fn dag_view_full_coverage() {
        let snaps = vec![
            snap(1, NodeState::Done, 0, 100),
            snap(2, NodeState::Running, 120, 0),
            snap(3, NodeState::Pending, 0, 0),
        ];
        assert_eq!(dag_view_consistent(&snaps, 200).unwrap(), 1);
        assert!(dag_view_consistent(&snaps, 100).is_err()); // Running 在未来。
    }
    #[test]
    fn collect_overhead_gate() {
        assert_eq!(collect_overhead_permille(50_000, 100).unwrap(), 500);
        assert!(collect_overhead_permille(150_000, 100).is_err());
    }
    #[test]
    fn panel_frame_roundtrip() {
        let snaps = vec![snap(1, NodeState::Done, 0, 200), snap(2, NodeState::Done, 200, 500)];
        let f = make_panel_frame(9, &snaps).unwrap();
        assert_eq!(f.snaps, 2);
        assert_eq!(f.fp, panel_fp(9, &snaps).unwrap());
        assert!(f.validate().is_ok());
    }
}
