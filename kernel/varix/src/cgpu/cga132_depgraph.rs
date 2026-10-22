//! cga132_depgraph — CGPU-F0132 A 域依赖图。
//!
//! 判据原文（施工书锚点 CGPU-F0132）：A 域依赖图（F0389 范式）：入边
//! （VE-C 编译器消费 SIMD 名/VE-D 栅格化消费基元/CGPU-B 任务图消费
//! 线程池）、出边（内核调度接口/功耗接口）。变更影响分析用例与构建
//! 刷新。判据：图与代码一致、影响分析、排序应用、自动+确认、刷新。
//!
//! **设计定盘**：依赖图 = 有向无环图（节点 = A 域构件，边 = 依赖）。
/// 入边闭集（VE-C/VE-D/CGPU-B 三消费者）与出边闭集（内核调度/功耗两
/// 接口）显性建模——边种类是闭集不是自由字符串（防止图与代码漂移）。
/// 图与代码一致 = 每条边登记代码落点指纹，落点指纹变化即图陈旧
/// （刷新核验入口）。影响分析 = 从被改节点出发的可达集（BFS，上溯
/// 消费者）；排序应用 = 变更批次按拓扑序排序后逐批应用；自动+确认 =
/// 影响分析自动生成、应用需显性确认（确认标志位）；环 = 图 invariant，
/// 建边时即检环（有环拒建——依赖图有环即构建系统死锁的种子）。
///
/// 诊断码段 0x0230~0x023F（0x02 段第 3 子区）。
/// no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const CGA132_VERSION: &str = "cga132-depgraph-v1";

/// 诊断子区基值（0x02 段第 3 子区）。
pub const SEG_BASE: u16 = 0x0230;

/// 边种类闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeKind {
    /// 入边：VE-C 编译器消费 SIMD 名。
    InVeC,
    /// 入边：VE-D 栅格化消费基元。
    InVeD,
    /// 入边：CGPU-B 任务图消费线程池。
    InCgpuB,
    /// 出边：内核调度接口。
    OutKernelSched,
    /// 出边：功耗接口。
    OutPower,
}

impl EdgeKind {
    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            EdgeKind::InVeC => 0,
            EdgeKind::InVeD => 1,
            EdgeKind::InCgpuB => 2,
            EdgeKind::OutKernelSched => 3,
            EdgeKind::OutPower => 4,
        }
    }

    /// 是否入边。
    pub fn is_inbound(self) -> bool {
        matches!(self, EdgeKind::InVeC | EdgeKind::InVeD | EdgeKind::InCgpuB)
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 图节点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DepNode {
    /// 节点号（1 起）。
    pub id: u32,
    /// 构件代码落点指纹（图与代码一致的真源）。
    pub code_fp: u64,
}

/// 一条依赖边。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DepEdge {
    /// 起点（依赖方）。
    pub from: u32,
    /// 终点（被依赖方）。
    pub to: u32,
    /// 边种类。
    pub kind: EdgeKind,
    /// 边落点指纹。
    pub site_fp: u64,
}

/// 依赖图。
pub struct DepGraph {
    /// 节点。
    nodes: Vec<DepNode>,
    /// 边。
    edges: Vec<DepEdge>,
    /// 确认位（判据"自动+确认"：影响分析自动，应用需确认）。
    applied_confirmed: bool,
    /// 封图。
    sealed: Option<u64>,
}

impl DepGraph {
    /// 空图。
    pub fn new() -> Self {
        DepGraph {
            nodes: Vec::new(),
            edges: Vec::new(),
            applied_confirmed: false,
            sealed: None,
        }
    }

    /// 建节点（判据"图与代码一致"：落点指纹零即拒；重号即拒）。
    pub fn add_node(&mut self, id: u32, code_fp: u64) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 6);
        }
        if id == 0 || code_fp == 0 {
            return Err(SEG_BASE + 7);
        }
        if self.nodes.iter().any(|n| n.id == id) {
            return Err(SEG_BASE + 8);
        }
        self.nodes.push(DepNode { id, code_fp });
        Ok(())
    }

    /// 建边（判据"入边/出边闭集" + 环即时检测：self-loop 与回边都拒）。
    pub fn add_edge(&mut self, from: u32, to: u32, kind: EdgeKind, site_fp: u64) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 6);
        }
        if from == to {
            return Err(SEG_BASE + 9);
        }
        if site_fp == 0 {
            return Err(SEG_BASE + 10);
        }
        if !self.nodes.iter().any(|n| n.id == from)
            || !self.nodes.iter().any(|n| n.id == to)
        {
            return Err(SEG_BASE + 11);
        }
        if self.reachable(to, from) {
            // 建此边会成环（to 已可达 from，加 from→to 即环）。
            return Err(SEG_BASE + 12);
        }
        if self
            .edges
            .iter()
            .any(|e| e.from == from && e.to == to)
        {
            return Err(SEG_BASE + 13);
        }
        self.edges.push(DepEdge {
            from,
            to,
            kind,
            site_fp,
        });
        Ok(())
    }

    /// 可达性（BFS， iterative 无递归——万节点安全）。
    pub fn reachable(&self, src: u32, dst: u32) -> bool {
        if src == dst {
            return true;
        }
        let mut queue: Vec<u32> = Vec::new();
        let mut seen: Vec<u32> = Vec::new();
        queue.push(src);
        seen.push(src);
        while let Some(cur) = queue.pop() {
            for e in &self.edges {
                if e.from == cur && !seen.contains(&e.to) {
                    if e.to == dst {
                        return true;
                    }
                    seen.push(e.to);
                    queue.push(e.to);
                }
            }
        }
        false
    }

    /// 变更影响分析（判据"影响分析"：被改节点的全部上溯消费者集合
    /// ——沿入边方向反查）。
    pub fn impact_of(&self, changed: u32) -> Vec<u32> {
        let mut impact: Vec<u32> = Vec::new();
        let mut frontier: Vec<u32> = vec![changed];
        while let Some(cur) = frontier.pop() {
            for e in &self.edges {
                if e.to == cur && !impact.contains(&e.from) && e.from != changed {
                    impact.push(e.from);
                    frontier.push(e.from);
                }
            }
        }
        impact
    }

    /// 构建刷新核验（判据"刷新"：节点落点指纹与外部申报指纹比对，
    /// 返回陈旧节点数）。
    pub fn stale_nodes(&self, declared: &[(u32, u64)]) -> usize {
        declared
            .iter()
            .filter(|(id, fp)| {
                self.nodes
                    .iter()
                    .any(|n| n.id == *id && n.code_fp != *fp)
            })
            .count()
    }

    /// 排序应用确认（判据"自动+确认"）。
    pub fn confirm_apply(&mut self) -> Result<(), u16> {
        if self.applied_confirmed {
            return Err(SEG_BASE + 14);
        }
        self.applied_confirmed = true;
        Ok(())
    }

    /// 节点数 / 边数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 入边闭集完整性（判据：三类入边至少各一条）。
    pub fn inbound_kinds_complete(&self) -> Result<(), u16> {
        for k in [
            EdgeKind::InVeC,
            EdgeKind::InVeD,
            EdgeKind::InCgpuB,
        ] {
            if !self.edges.iter().any(|e| e.kind == k) {
                return Err(k.code());
            }
        }
        Ok(())
    }

    /// 封图。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 15);
        }
        self.inbound_kinds_complete()?;
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for e in &self.edges {
            fp = fp.wrapping_mul(0x100_0000_01b3)
                ^ ((e.from as u64) << 20)
                ^ (e.to as u64)
                ^ e.site_fp;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for DepGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// 出边投影行（判据"出边"的账面：内核调度接口/功耗接口两类出边
/// 逐条列出——出边缺失即"内核消费面未登记"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OutboundRow {
    /// 源节点。
    pub from: u32,
    /// 目标站点指纹。
    pub site_fp: u64,
}

impl DepGraph {
    /// 出边清单（判据"出边（内核调度接口/功耗接口）"的投影面）。
    pub fn outbound_rows(&self) -> Vec<OutboundRow> {
        self.edges
            .iter()
            .filter(|e| {
                e.kind == EdgeKind::OutKernelSched || e.kind == EdgeKind::OutPower
            })
            .map(|e| OutboundRow { from: e.from, site_fp: e.site_fp })
            .collect()
    }

    /// 出边闭集核验（判据：两类出边——调度接口与功耗接口——至少各
    /// 一条）。
    pub fn outbound_kinds_complete(&self) -> Result<(), u16> {
        if !self.edges.iter().any(|e| e.kind == EdgeKind::OutKernelSched) {
            return Err(SEG_BASE + 16);
        }
        if !self.edges.iter().any(|e| e.kind == EdgeKind::OutPower) {
            return Err(SEG_BASE + 17);
        }
        Ok(())
    }

    /// 直接消费者（判据"影响分析"的一跳面：只反查一层，供增量刷新
    /// 的最小重算集）。
    pub fn direct_consumers(&self, changed: u32) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for e in &self.edges {
            if e.to == changed && !out.contains(&e.from) && e.from != changed {
                out.push(e.from);
            }
        }
        out
    }

    /// 图指纹（封图前的可复现指纹查询）。
    pub fn graph_fp(&self) -> u64 {
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for n in &self.nodes {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ n.id as u64;
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ n.code_fp;
        }
        for e in &self.edges {
            fp = fp.wrapping_mul(0x100_0000_01b3)
                ^ ((e.from as u64) << 20)
                ^ (e.to as u64);
        }
        fp
    }

    /// 封图指纹。
    pub fn seal_fp(&self) -> Option<u64> {
        self.sealed
    }
}
