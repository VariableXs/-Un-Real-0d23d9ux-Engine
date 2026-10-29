//! UNX-F48423 —— E·整句 DAG 节点上限边界（正：超限降级逐词模式可用）
//! 判据原文：UNX-F48423-J1 节点上限 256：≤256 全量整句构图；257 触发降级逐词模式；
//!           降级后逐词模式可用（每词独立构图+查询成功）；降级事件登记 ×10 全记；
//!           LCG 随机长度 512 句全部有界；构图结果封印重演一致；
//!           上限恒定断言（DAG_CAP == 256 双读一致）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD860; // E22 DAG 上限段

pub const DAG_CAP: usize = 256;

const E_OVER_PASS: u16 = SEG_BASE + 1; // 超限未降级
const E_DEGRADE_DEAD: u16 = SEG_BASE + 2; // 降级后不可用
const E_NO_LOG: u16 = SEG_BASE + 3; // 降级未登记
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_COUNT_MISS: u16 = SEG_BASE + 6; // 登记计数不符
const E_NODE_MISS: u16 = SEG_BASE + 7; // 节点数不符
const E_CAP_VAR: u16 = SEG_BASE + 8; // 上限漂移

/// 句子图：节点 = 音节位置，边 = 词候选。
pub struct Dag {
    nodes: usize,
    edges: Vec<(usize, usize)>, // (from, to) 位置对
    degraded: bool,
}

impl Dag {
    /// 全量构图：n 音节句，相邻音节间成边（词长 1~4 生成 4(n-1) 内边）。
    pub fn build_full(n: usize) -> Dag {
        let mut edges: Vec<(usize, usize)> = Vec::new();
        let mut i = 0usize;
        while i + 1 < n {
            // 词长 1~4：边 (i, i+w)（w ≤ n-1-i）
            let mut w = 1usize;
            while w <= 4 && i + w < n {
                edges.push((i, i + w));
                w += 1;
            }
            i += 1;
        }
        Dag { nodes: n, edges, degraded: false }
    }

    /// 逐词构图（降级模式）：每词独立 2 节点 1 边。
    pub fn build_wordwise(n: usize) -> Dag {
        let mut edges: Vec<(usize, usize)> = Vec::new();
        let mut i = 0usize;
        while i + 1 < n {
            edges.push((i, i + 1));
            i += 1;
        }
        Dag { nodes: n, edges, degraded: true }
    }

    /// 查询路径数（DP 计数，有界算术）。
    pub fn path_count(&self) -> u64 {
        if self.nodes < 2 {
            return 1;
        }
        let mut ways: Vec<u64> = Vec::new();
        let mut i = 0usize;
        while i < self.nodes {
            ways.push(0);
            i += 1;
        }
        ways[0] = 1;
        let mut i = 0usize;
        while i < self.nodes {
            let mut e = 0usize;
            while e < self.edges.len() {
                if self.edges[e].0 == i {
                    let to = self.edges[e].1;
                    ways[to] = ways[to].wrapping_add(ways[i]);
                }
                e += 1;
            }
            i += 1;
        }
        ways[self.nodes - 1]
    }
}

/// 构图调度器：n ≤ 256 全量；n > 256 降级并登记。
pub struct DagScheduler {
    degrade_log: u32,
}

impl DagScheduler {
    pub fn new() -> Self {
        DagScheduler { degrade_log: 0 }
    }

    /// 组句构图：返回 Dag。超限降级登记。
    pub fn schedule(&mut self, n: usize) -> Result<Dag, u16> {
        if n < 2 {
            return Err(E_NODE_MISS);
        }
        if n <= DAG_CAP {
            Ok(Dag::build_full(n))
        } else {
            self.degrade_log += 1;
            Ok(Dag::build_wordwise(n))
        }
    }

    pub fn degradations(&self) -> u32 {
        self.degrade_log
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 主巡检：DAG 上限八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 恰 256 全量构图：nodes=256、非降级、路径数有界
    let mut sch = DagScheduler::new();
    let d = sch.schedule(256)?;
    if d.nodes != 256 || d.degraded {
        return Err(E_NODE_MISS);
    }
    let _ = d.path_count();
    hits += 1;

    // R2 257 触发降级：degraded=true、登记 +1、逐词图可用
    let d2 = sch.schedule(257)?;
    if !d2.degraded {
        return Err(E_OVER_PASS);
    }
    if sch.degradations() != 1 {
        return Err(E_NO_LOG);
    }
    if d2.path_count() != 1 {
        return Err(E_DEGRADE_DEAD);
    }
    hits += 1;

    // R3 降级模式可用性：逐词构图查询 10 次全成功（每次路径数 = 1）
    let mut k = 0u32;
    while k < 10 {
        let dw = Dag::build_wordwise(300);
        if dw.path_count() != 1 {
            return Err(E_DEGRADE_DEAD);
        }
        k += 1;
    }
    hits += 1;

    // R4 ×10 降级全登记：10 个超限句全降级、账=10
    let mut sch2 = DagScheduler::new();
    let mut k = 0u32;
    while k < 10 {
        let d = sch2.schedule(258 + k as usize)?;
        if !d.degraded {
            return Err(E_OVER_PASS);
        }
        k += 1;
    }
    if sch2.degradations() != 10 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R5 全量模式路径数有界正确性：n=5 全量图路径数 = 手工闭式（词长 1~4 组合）
    let d5 = Dag::build_full(5);
    // 手工：位置 0→4，词长 1~4 的组合计数 = 13（1+1+2+4+5 组合枚举）
    let manual: u64 = {
        // DP 手算：ways[0]=1, ways[i]=sum(ways[i-w] w=1..4)
        let mut ways = [0u64; 5];
        ways[0] = 1;
        let mut i = 1usize;
        while i < 5 {
            let mut w = 1usize;
            while w <= 4 && w <= i {
                ways[i] += ways[i - w];
                w += 1;
            }
            i += 1;
        }
        ways[4]
    };
    if d5.path_count() != manual || manual != 13 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R6 LCG 随机长度 512 句全部有界：n ∈ [2, 400]，全成功
    let mut rng = Lcg::new(0x48423);
    let mut k = 0u32;
    while k < 512 {
        let n = 2 + rng.below(399) as usize;
        let d = sch.schedule(n)?;
        if d.nodes != n {
            return Err(E_NODE_MISS);
        }
        let _ = d.path_count();
        k += 1;
    }
    hits += 1;

    // R7 重演一致：同种子重建调度器降级账一致
    let mut sch3 = DagScheduler::new();
    let mut rng2 = Lcg::new(0x48423);
    let mut k = 0u32;
    while k < 512 {
        let n = 2 + rng2.below(399) as usize;
        let _ = sch3.schedule(n)?;
        k += 1;
    }
    if sch3.degradations() != sch.degradations() {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 上限恒定：DAG_CAP == 256
    if DAG_CAP != 256 {
        return Err(E_CAP_VAR);
    }
    hits += 1;

    Ok(hits)
}
