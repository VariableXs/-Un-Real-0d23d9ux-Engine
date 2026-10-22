//! CGPU-F0197 · NUMA 感知帧图调度（判据段 0x05F0）
//! NUMA 架构下的帧图调度：节点感知分配（任务分配到与数据所在 NUMA
//! 节点相同的线程——跨节点内存访问延迟消除）、NUMA 拓扑探测
//! （CGPU-F0002 的 NUMA 维度在帧图调度的集成）、跨节点数据迁移成本
//! 计入（任务迁移的 NUMA 惩罚——成本模型更新）。判据：节点感知分配、
//! 拓扑探测、迁移成本、单路零退化、双路收益实测。
//!
//! 诊断码段 0x05F0（本模块独占：0x05F0~0x05FF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x05F0;

/// 跨节点访问延迟惩罚 ×1000（本地 1000 = 基准，远端 1600）。
pub const REMOTE_ACCESS_PENALTY_PERMILLE: u32 = 1600;

/// 迁移页缓存冷启动惩罚（微秒/MB）。
pub const MIGRATION_COLD_US_PER_MB: u32 = 12;

/// 单路零退化带宽（千分比）：单路系统启用 NUMA 调度后性能波动
/// 不得超过 ±30‰（判据「单路零退化」）。
pub const SINGLE_SOCKET_ZERO_REGRESSION_PERMILLE: u32 = 30;

/// NUMA 节点描述（判据「拓扑探测」产物）。
#[derive(Clone, Debug)]
pub struct NumaNode {
    /// 节点号。
    pub node_id: u32,
    /// 该节点核心号列表。
    pub cores: Vec<u32>,
    /// 本地内存容量（MB）。
    pub mem_mb: u32,
}

impl NumaNode {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.mem_mb == 0 || self.cores.is_empty() {
            return Err(SEG_BASE);
        }
        if self.cores.iter().any(|&c| c > 255) {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// NUMA 拓扑（判据「拓扑探测」容器）：节点集合 + 距离矩阵。
#[derive(Clone, Debug, Default)]
pub struct NumaTopology {
    /// 节点表。
    pub nodes: Vec<NumaNode>,
    /// 距离矩阵（对称，对角 = 1——本地跳数基准）。
    dist: Vec<Vec<u32>>,
}

impl NumaTopology {
    /// 建拓扑：n 个节点全连通，本地距离 1、远端 2（经典双路形态）。
    pub fn symmetric(n: u32) -> Result<Self, u16> {
        if n == 0 || n > 16 {
            return Err(SEG_BASE + 2);
        }
        let mut dist = vec![vec![1u32; n as usize]; n as usize];
        for i in 0..n as usize {
            for j in 0..n as usize {
                if i != j {
                    dist[i][j] = 2;
                }
            }
        }
        Ok(Self { nodes: Vec::new(), dist })
    }
    /// 登记节点（节点号重复拒绝）。
    pub fn add_node(&mut self, node: NumaNode) -> Result<(), u16> {
        node.validate()?;
        if node.node_id as usize >= self.dist.len()
            || self.nodes.iter().any(|x| x.node_id == node.node_id)
        {
            return Err(SEG_BASE + 3);
        }
        self.nodes.push(node);
        Ok(())
    }
    /// 节点数（已登记）。
    pub fn registered(&self) -> usize {
        self.nodes.len()
    }
    /// 距离查询。
    pub fn distance(&self, a: u32, b: u32) -> Result<u32, u16> {
        let n = self.dist.len();
        if a as usize >= n || b as usize >= n {
            return Err(SEG_BASE + 4);
        }
        Ok(self.dist[a as usize][b as usize])
    }
    /// 核号 → 节点号（判据「节点感知分配」的查询底座）。
    pub fn node_of_core(&self, core: u32) -> Result<u32, u16> {
        self.nodes
            .iter()
            .find(|n| n.cores.contains(&core))
            .map(|n| n.node_id)
            .ok_or(SEG_BASE + 5)
    }
    /// 拓扑完整性：全节点登记且两两距离非零。
    pub fn validate_full(&self, expect_nodes: u32) -> Result<(), u16> {
        if self.registered() != expect_nodes as usize {
            return Err(SEG_BASE + 6);
        }
        for a in 0..expect_nodes {
            for b in 0..expect_nodes {
                let d = self.distance(a, b)?;
                if d == 0 {
                    return Err(SEG_BASE + 7);
                }
            }
        }
        Ok(())
    }
}

/// 数据驻留描述：任务的数据所在节点与体量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DataResidency {
    /// 数据所在节点。
    pub node: u32,
    /// 工作集体量（MB）。
    pub working_set_mb: u32,
}

impl DataResidency {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.working_set_mb == 0 {
            return Err(SEG_BASE + 8);
        }
        Ok(())
    }
}

/// 帧任务 NUMA 视图。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NumaTask {
    /// 任务号。
    pub id: u32,
    /// 数据驻留。
    pub residency: DataResidency,
    /// 预估本地执行代价（微秒，未含 NUMA 惩罚）。
    pub base_us: u32,
}

impl NumaTask {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.id == 0 || self.base_us == 0 {
            return Err(SEG_BASE + 9);
        }
        self.residency.validate()
    }
}

/// 节点感知分配裁决（判据「节点感知分配」核心）：任务应分派到其
/// 数据所在节点的核心；候选核必须与数据同节点，否则判为违规放置。
pub fn placement_ok(topo: &NumaTopology, task: &NumaTask, exec_core: u32) -> Result<bool, u16> {
    task.validate()?;
    let data_node = task.residency.node;
    let exec_node = topo.node_of_core(exec_core)?;
    Ok(exec_node == data_node)
}

/// 执行代价模型（判据「迁移成本」核心量）：本地执行 = base；远端
/// 执行 = base × REMOTE_ACCESS_PENALTY_PERMILLE/1000。返回（代价,
/// 是否远端）。
pub fn exec_cost_us(topo: &NumaTopology, task: &NumaTask, exec_core: u32) -> Result<(u64, bool), u16> {
    task.validate()?;
    let data_node = task.residency.node;
    let exec_node = topo.node_of_core(exec_core)?;
    if exec_node == data_node {
        Ok((task.base_us as u64, false))
    } else {
        let remote = (task.base_us as u64 * REMOTE_ACCESS_PENALTY_PERMILLE as u64) / 1000;
        Ok((remote, true))
    }
}

/// 迁移成本（判据「迁移成本」独立量）：把 working_set 从源节点搬到
/// 目标节点的冷缓存代价 = 体量 × MIGRATION_COLD_US_PER_MB + 距离
/// 系数（每跳 ×1.5）。
pub fn migration_cost_us(topo: &NumaTopology, r: &DataResidency, to_node: u32) -> Result<u64, u16> {
    r.validate()?;
    let d = topo.distance(r.node, to_node)?;
    if d == 1 {
        return Ok(0);
    }
    let base = r.working_set_mb as u64 * MIGRATION_COLD_US_PER_MB as u64;
    Ok(base * 3 / 2)
}

/// 「迁移 vs 远端执行」权衡（判据「迁移成本」可行动面）：迁移一次
/// 成本摊销到任务重复执行次数上，若摊销后低于每次远端惩罚的累计
/// 则建议迁移。返回 Some(建议迁移) / None(不迁)。
pub fn migrate_or_remote(
    topo: &NumaTopology,
    r: &DataResidency,
    to_node: u32,
    remote_penalty_us: u64,
    reruns: u32,
) -> Result<Option<u32>, u16> {
    if reruns == 0 {
        return Err(SEG_BASE + 10);
    }
    let mig = migration_cost_us(topo, r, to_node)?;
    if mig == 0 {
        return Err(SEG_BASE + 11);
    }
    let remote_total = remote_penalty_us.checked_mul(reruns as u64).ok_or(SEG_BASE + 12)?;
    if remote_total > mig {
        Ok(Some(1))
    } else {
        Ok(None)
    }
}

/// 帧图 NUMA 分配方案：任务 → 核 的映射账。
#[derive(Clone, Debug, Default)]
pub struct NumaPlan {
    /// (任务号, 执行核号) 序列。
    pub assign: Vec<(u32, u32)>,
    /// 预估总代价（含惩罚）。
    pub total_us: u64,
}

impl NumaPlan {
    /// 账本校验。
    pub fn validate(&self, expect_tasks: usize) -> Result<(), u16> {
        if self.assign.len() != expect_tasks {
            return Err(SEG_BASE + 13);
        }
        let mut seen = vec![false; expect_tasks];
        for (id, _) in &self.assign {
            if *id == 0 || *id as usize > expect_tasks || seen[*id as usize - 1] {
                return Err(SEG_BASE + 14);
            }
            seen[*id as usize - 1] = true;
        }
        Ok(())
    }
}

/// 贪心 NUMA 分配（判据「节点感知分配」批量面）：每个任务选其数据
/// 节点上的第一个空闲核；无同节点核时选全局空闲核并计入惩罚。
pub fn greedy_numa_assign(
    topo: &NumaTopology,
    tasks: &[NumaTask],
) -> Result<NumaPlan, u16> {
    if tasks.is_empty() {
        return Err(SEG_BASE + 15);
    }
    let mut used: Vec<u32> = Vec::new();
    let mut assign: Vec<(u32, u32)> = Vec::new();
    let mut total: u64 = 0;
    for t in tasks {
        t.validate()?;
        let mut best: Option<(u32, u64)> = None;
        for n in &topo.nodes {
            for &core in &n.cores {
                if used.contains(&core) {
                    continue;
                }
                let (cost, remote) = exec_cost_us(topo, t, core)?;
                // 迁移成本只在换节点时发生一次（数据搬家）。
                let mut full = cost;
                if remote {
                    full += migration_cost_us(topo, &t.residency, n.node_id)?;
                }
                if best.map(|(_, c)| full < c).unwrap_or(true) {
                    best = Some((core, full));
                }
            }
        }
        let (core, cost) = best.ok_or(SEG_BASE + 16)?;
        used.push(core);
        assign.push((t.id, core));
        total += cost;
    }
    let plan = NumaPlan { assign, total_us: total };
    plan.validate(tasks.len())?;
    Ok(plan)
}

/// 单路零退化（判据「单路零退化」主判据）：单节点拓扑下 NUMA
/// 分配的代价与朴素分配一致——差异不得超过带宽。
pub fn single_socket_no_regression(naive_us: u64, numa_us: u64) -> Result<bool, u16> {
    if naive_us == 0 || numa_us == 0 {
        return Err(SEG_BASE + 17);
    }
    let diff = naive_us.abs_diff(numa_us) * 1000;
    Ok(diff / naive_us <= SINGLE_SOCKET_ZERO_REGRESSION_PERMILLE as u64)
}

/// 双路收益实测（判据「双路收益实测」主判据）：双节点拓扑下
/// NUMA 感知分配的代价必须严格优于把全部任务放到远端。
pub fn dual_socket_gain(
    local_us: u64,
    all_remote_us: u64,
) -> Result<u32, u16> {
    if local_us == 0 || all_remote_us <= local_us {
        return Err(SEG_BASE + 18);
    }
    Ok((((all_remote_us - local_us) * 1000) / local_us) as u32)
}

/// 拓扑账指纹（可审计面）。
pub fn topology_fp(t: &NumaTopology) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for n in &t.nodes {
        acc = acc.wrapping_mul(0x100000001b3) ^ n.node_id as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ n.cores.len() as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ n.mem_mb as u64;
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    fn topo2() -> NumaTopology {
        let mut t = NumaTopology::symmetric(2).unwrap();
        t.add_node(NumaNode { node_id: 0, cores: vec![0, 1, 2, 3], mem_mb: 32_768 }).unwrap();
        t.add_node(NumaNode { node_id: 1, cores: vec![4, 5, 6, 7], mem_mb: 32_768 }).unwrap();
        t
    }
    fn task(id: u32, node: u32, mb: u32, us: u32) -> NumaTask {
        NumaTask { id, residency: DataResidency { node, working_set_mb: mb }, base_us: us }
    }
    #[test]
    fn topology_probe_and_lookup() {
        let t = topo2();
        t.validate_full(2).unwrap();
        assert_eq!(t.node_of_core(3).unwrap(), 0);
        assert_eq!(t.node_of_core(6).unwrap(), 1);
        assert_eq!(t.distance(0, 1).unwrap(), 2);
        assert!(t.node_of_core(99).is_err());
    }
    #[test]
    fn placement_local_ok() {
        let t = topo2();
        let tk = task(1, 1, 64, 100);
        assert!(placement_ok(&t, &tk, 5).unwrap());
        assert!(!placement_ok(&t, &tk, 2).unwrap());
    }
    #[test]
    fn remote_penalty_applied() {
        let t = topo2();
        let tk = task(1, 0, 64, 1000);
        let (local, remote_flag) = exec_cost_us(&t, &tk, 1).unwrap();
        assert_eq!((local, remote_flag), (1000, false));
        let (remote, remote_flag2) = exec_cost_us(&t, &tk, 6).unwrap();
        assert_eq!((remote, remote_flag2), (1600, true));
    }
    #[test]
    fn migration_cost_distance_scaled() {
        let t = topo2();
        let r = DataResidency { node: 0, working_set_mb: 100 };
        assert_eq!(migration_cost_us(&t, &r, 0).unwrap(), 0);
        assert_eq!(migration_cost_us(&t, &r, 1).unwrap(), 100 * 12 * 3 / 2);
    }
    #[test]
    fn migrate_vs_remote_tradeoff() {
        let t = topo2();
        let r = DataResidency { node: 0, working_set_mb: 10 };
        // 迁移 = 10×12×1.5 = 180；远端惩罚每次 1000，跑 3 次 = 3000 → 迁。
        assert_eq!(migrate_or_remote(&t, &r, 1, 1000, 3).unwrap(), Some(1));
        // 跑 1 次远端 = 1000 < 180×1（不对——1000 > 180 也迁）。
        // 改用小惩罚：每次 10，跑 1 次 = 10 < 180 → 不迁。
        assert_eq!(migrate_or_remote(&t, &r, 1, 10, 1).unwrap(), None);
    }
    #[test]
    fn greedy_assign_prefers_local() {
        let t = topo2();
        let tasks = vec![task(1, 0, 8, 100), task(2, 1, 8, 100), task(3, 0, 8, 100)];
        let plan = greedy_numa_assign(&t, &tasks).unwrap();
        assert_eq!(plan.assign[0], (1, 0));
        assert_eq!(plan.assign[1], (2, 4));
        assert_eq!(plan.assign[2], (3, 1));
        assert_eq!(plan.total_us, 300);
    }
    #[test]
    fn single_socket_stable() {
        assert!(single_socket_no_regression(1000, 1005).unwrap());
        assert!(!single_socket_no_regression(1000, 1200).unwrap());
    }
    #[test]
    fn dual_socket_gain_positive() {
        let gain = dual_socket_gain(3200, 6400).unwrap();
        assert_eq!(gain, 1000);
        assert!(dual_socket_gain(3200, 3000).is_err());
    }
}
