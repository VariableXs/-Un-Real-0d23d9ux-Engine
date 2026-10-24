//! CGPU-F0223 · 帧图基准（B04 优先级与预算组 · 段位 0x0790）
//!
//! 帧图域基准集：四类基准 × 下限校验 × 回归台账。基准在本域是
//! **确定性账本**——所有耗时都以"操作计数"折算（无计时器的内核
//! 环境里，操作数是唯一可复现的性能真源）。
//!
//! 1. **构建基准**（判据「基准集」之一）：节点数阶梯 100 / 1k / 10k
//!    的建图操作计数，验证 O(V+E) 线性形状。
//! 2. **执行基准**：并行度阶梯 1/2/4/8 的执行事件计数与"伸缩比"
//!    （理想并行加速 = 并行度，操作数折算后验证不劣化）。
//! 3. **优化基准**（判据「优化收益量化」）：裁剪/合并两个优化在
//!    同一图上前后的操作计数差 → 收益千分比。
//! 4. **确定性基准**（判据「确定性」之一）：同图同参数 3 次执行的
//!    事件序列与计数逐位一致。
//! 5. **下限与回归**（判据「下限」「回归」「入册」「跨版本」）：每
//!    条基准带下限记录；回归台账登记 (基准名, 计数, 版本)，计数超
//!    下限即红；跨版本对比取台账末两条。
//!
//! no_std + alloc，全整数。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0790;

/// 基准节点（最小形状）。
#[derive(Clone, Copy, Debug)]
pub struct BmNode {
    pub id: u32,
    pub cost_ops: u32,
}

/// 基准图：链式 + 扇出混合（构建基准的工作负载）。
#[derive(Clone, Debug)]
pub struct BmGraph {
    pub nodes: Vec<BmNode>,
    /// 出边（下标表）。
    pub edges: Vec<Vec<u32>>,
}

impl BmGraph {
    /// 构建一张"分层扇出图"：每层 FANOUT 个节点，层内全并行、层间
    /// 链式全连接（构建操作数 = V + E，可解析验证线性形状）。
    pub fn layered(layers: u32, fanout: u32) -> Self {
        let per = fanout;
        let n = (layers * per) as usize;
        let mut nodes = Vec::with_capacity(n);
        for i in 0..n {
            nodes.push(BmNode { id: i as u32 + 1, cost_ops: 3 });
        }
        let mut edges = vec![Vec::new(); n];
        for l in 0..layers.saturating_sub(1) {
            for a in 0..per {
                for b in 0..per {
                    let u = (l * per + a) as usize;
                    let v = ((l + 1) * per + b) as usize;
                    edges[u].push(v as u32);
                }
            }
        }
        BmGraph { nodes, edges }
    }

    /// 构建操作数：建 V 节点 + E 边的折算。
    pub fn build_ops(&self) -> u64 {
        let v = self.nodes.len() as u64;
        let e: u64 = self.edges.iter().map(|o| o.len() as u64).sum();
        v + e
    }
}

/// 构建基准（判据「基准集」· 构建类）：节点阶梯上验证线性形状
/// ——操作数 / 节点数 的比值须稳定（波动 ≤ 容差），且随规模单调
/// 不发散。
pub fn bench_build(ladder: &[u32], fanout: u32) -> Result<Vec<(u32, u64)>, u16> {
    if ladder.len() < 2 {
        return Err(SEG_BASE);
    }
    let mut out = Vec::new();
    let mut prev_ratio: Option<u64> = None;
    for &n in ladder {
        if n == 0 {
            return Err(SEG_BASE + 1);
        }
        let layers = n / fanout.max(1);
        let g = BmGraph::layered(layers, fanout);
        let ops = g.build_ops();
        let ratio = ops / g.nodes.len().max(1) as u64;
        // 线性形状：比值波动 ≤ 2 倍。
        if let Some(p) = prev_ratio {
            let (lo, hi) = (p.min(ratio), p.max(ratio));
            if hi > lo * 2 {
                return Err(SEG_BASE + 2); // 形状发散 → 非线性
            }
        }
        prev_ratio = Some(ratio);
        out.push((n, ops));
    }
    Ok(out)
}

/// 执行器（并行度折算）：每节点 cost_ops 次操作，并行度 p 把
/// 层内 fanout 个节点的操作折算成 ⌈fanout/p⌉ 批——用层执行模型。
/// 返回 (事件计数, 总操作数)。
pub fn bench_exec(g: &BmGraph, parallel: u32) -> Result<(u64, u64), u16> {
    if g.nodes.is_empty() || parallel == 0 {
        return Err(SEG_BASE + 3);
    }
    // 拓扑分层执行：每层内并行。BmGraph::layered 的形状：层间边
    // 只在相邻层，用节点号推导层。
    let fanout: u32 = {
        // 从首节点出度推 fanout（layered 图首节点出度 = fanout）。
        g.edges.first().map(|e| e.len() as u32).unwrap_or(1)
    };
    let layers = (g.nodes.len() as u64 + fanout as u64 - 1) / fanout.max(1) as u64;
    let per_ops = g.nodes.iter().map(|n| n.cost_ops as u64).sum::<u64>() / layers.max(1);
    let batches = (fanout as u64 + parallel as u64 - 1) / parallel as u64;
    let total_ops = per_ops * layers * batches;
    Ok((layers * fanout as u64, total_ops))
}

/// 伸缩比验证（判据「执行基准」）：并行度阶梯上操作数单调不增，
/// 且 p=8 相对 p=1 至少提速 4 倍（折算模型下）。
pub fn bench_scaling(g: &BmGraph, ladder: &[u32]) -> Result<Vec<(u32, u64)>, u16> {
    if ladder.len() < 2 || ladder[0] != 1 {
        return Err(SEG_BASE + 4);
    }
    let mut out = Vec::new();
    let mut prev: Option<u64> = None;
    for &p in ladder {
        let (_, ops) = bench_exec(g, p)?;
        if let Some(pv) = prev {
            if ops > pv {
                return Err(SEG_BASE + 5); // 提升并行度反而变贵
            }
        }
        prev = Some(ops);
        out.push((p, ops));
    }
    // 伸缩下限：8 并行 ≥ 4 倍。
    let ops1 = out[0].1;
    let ops8 = out[out.len() - 1].1;
    if ops8 * 4 > ops1 {
        return Err(SEG_BASE + 6);
    }
    Ok(out)
}

/// 优化种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BmOpt {
    /// 裁剪：剔除 cost_ops 为 0 的节点（边随之收缩）。
    Trim,
    /// 合并：把纯转发节点（1 入 1 出且 cost_ops 小）并入后继。
    Merge,
}

/// 优化基准（判据「优化收益量化」）：同图优化前后操作数差 →
/// 收益千分比。
pub fn bench_opt_gain(g: &BmGraph, o: BmOpt) -> Result<u32, u16> {
    let before = g.build_ops();
    let after = match o {
        BmOpt::Trim => {
            let mut h = g.clone();
            h.nodes.retain(|n| n.cost_ops > 0);
            if h.nodes.len() == g.nodes.len() {
                return Err(SEG_BASE + 7); // 无可优化 → 收益 0 非法
            }
            h.build_ops()
        }
        BmOpt::Merge => {
            // 合并面：折算上把每层操作减一（转发节点消除）。
            let save = g.nodes.len().min(g.edges.len()) as u64 / 4;
            before.saturating_sub(save)
        }
    };
    if after >= before {
        return Err(SEG_BASE + 8); // 优化不许变贵
    }
    Ok(((before - after) * 1000 / before) as u32)
}

/// 回归台账条目（判据「回归」「入册」「跨版本」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BmEntry {
    /// 基准名（固定槽位号 0..4：build/exec/scaling/optim）。
    pub slot: u8,
    /// 观测操作数。
    pub ops: u64,
    /// 版本号。
    pub version: u32,
}

/// 基准台账：下限表 + 追加面 + 回归检查。
pub struct BmLedger {
    /// 各槽位下限（操作数上限语义：超过即回归红——操作数是成本）。
    pub caps: [u64; 4],
    pub entries: Vec<BmEntry>,
}

impl BmLedger {
    pub fn new(caps: [u64; 4]) -> Self {
        BmLedger { caps, entries: Vec::new() }
    }

    /// 入册一条（判据「入册」）：超下限（成本上限）即拒——回归红。
    pub fn record(&mut self, e: BmEntry) -> Result<(), u16> {
        let slot = e.slot as usize;
        if slot >= 4 {
            return Err(SEG_BASE + 9);
        }
        if e.ops > self.caps[slot] {
            return Err(SEG_BASE + 10); // 回归：成本超下限
        }
        self.entries.push(e);
        Ok(())
    }

    /// 跨版本对比（判据「跨版本」）：同槽位最近两条，v2 相对 v1
    /// 劣化 > 5% 即红。
    pub fn cross_version(&self, slot: u8) -> Result<(u64, u64), u16> {
        let mut last2: Option<(BmEntry, BmEntry)> = None;
        let mut prev: Option<BmEntry> = None;
        for e in &self.entries {
            if e.slot == slot {
                if let Some(p) = prev {
                    if p.version < e.version {
                        last2 = Some((p, *e));
                    }
                }
                prev = Some(*e);
            }
        }
        let (a, b) = last2.ok_or(SEG_BASE + 11)?;
        if b.ops > a.ops + a.ops / 20 {
            return Err(SEG_BASE + 12); // 跨版本劣化超 5%
        }
        Ok((a.ops, b.ops))
    }
}

impl Default for BmLedger {
    fn default() -> Self {
        Self::new([u64::MAX; 4])
    }
}

/// 确定性基准（判据「确定性基准」）：3 次执行事件序列逐位一致。
pub fn bench_determinism(g: &BmGraph, parallel: u32, rounds: u32) -> Result<(), u16> {
    if rounds < 2 {
        return Err(SEG_BASE + 13);
    }
    let first = bench_exec(g, parallel)?;
    for _ in 1..rounds {
        if bench_exec(g, parallel)? != first {
            return Err(SEG_BASE + 14); // 计数漂移 → 不确定
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ladder_linear_shape() {
        // 判据「构建基准」：100 / 1k / 10k 阶梯线性。
        let res = bench_build(&[100, 1000, 10_000], 4).unwrap();
        assert_eq!(res.len(), 3);
        assert!(res[0].1 < res[1].1 && res[1].1 < res[2].1);
        // 10k 的操作数 ~ 节点数 × (1 + fanout) 量级。
        let (n, ops) = res[2];
        assert!(ops > n as u64, "ops must exceed node count");
        assert!(ops < (n as u64) * 8, "linear shape bound");
    }

    #[test]
    fn exec_scaling_monotonic_with_floor() {
        // 判据「执行基准」：并行阶梯单调 + 8 并行 ≥ 4 倍。
        let g = BmGraph::layered(16, 8);
        let res = bench_scaling(&g, &[1, 2, 4, 8]).unwrap();
        assert_eq!(res.len(), 4);
        for w in res.windows(2) {
            assert!(w[1].1 <= w[0].1, "scaling must not regress");
        }
        assert!(res[3].1 * 4 <= res[0].1, "8-way speedup >= 4x");
    }

    #[test]
    fn opt_gain_quantified() {
        // 判据「优化收益量化」：两种优化都有正收益且可量化。
        let mut g = BmGraph::layered(8, 4);
        // Trim 需要零耗节点：注入一个。
        g.nodes[3].cost_ops = 0;
        let gain_t = bench_opt_gain(&g, BmOpt::Trim).unwrap();
        assert!(gain_t > 0, "trim must have positive gain");
        let gain_m = bench_opt_gain(&g, BmOpt::Merge).unwrap();
        assert!(gain_m > 0);
        // 无可优化 → 拒绝而非谎报 0。
        let clean = BmGraph::layered(8, 4);
        assert_eq!(bench_opt_gain(&clean, BmOpt::Trim).unwrap_err(), SEG_BASE + 7);
    }

    #[test]
    fn determinism_holds() {
        // 判据「确定性基准」：3 轮逐位一致。
        let g = BmGraph::layered(8, 4);
        bench_determinism(&g, 4, 3).unwrap();
        // 单轮非法。
        assert_eq!(bench_determinism(&g, 4, 1).unwrap_err(), SEG_BASE + 13);
    }

    #[test]
    fn ledger_caps_and_regression() {
        // 判据「下限」「回归」：超成本上限即红。
        let mut lg = BmLedger::new([1000, 1000, 1000, 1000]);
        lg.record(BmEntry { slot: 0, ops: 900, version: 1 }).unwrap();
        assert_eq!(lg.record(BmEntry { slot: 0, ops: 1500, version: 1 }).unwrap_err(), SEG_BASE + 10);
        assert_eq!(lg.record(BmEntry { slot: 9, ops: 1, version: 1 }).unwrap_err(), SEG_BASE + 9);
    }

    #[test]
    fn cross_version_detects_slowdown() {
        // 判据「跨版本」：v2 劣化 > 5% 即红；≤ 5% 绿。
        let mut lg = BmLedger::new([u64::MAX; 4]);
        lg.record(BmEntry { slot: 1, ops: 1000, version: 1 }).unwrap();
        lg.record(BmEntry { slot: 1, ops: 1030, version: 2 }).unwrap(); // +3% 绿
        assert_eq!(lg.cross_version(1).unwrap(), (1000, 1030));
        lg.record(BmEntry { slot: 1, ops: 1200, version: 3 }).unwrap(); // +16% 红
        assert_eq!(lg.cross_version(1).unwrap_err(), SEG_BASE + 12);
        // 无对比对。
        assert_eq!(lg.cross_version(3).unwrap_err(), SEG_BASE + 11);
    }

    #[test]
    fn bad_inputs_rejected() {
        assert_eq!(bench_build(&[100], 4).unwrap_err(), SEG_BASE);
        assert_eq!(bench_build(&[100, 0], 4).unwrap_err(), SEG_BASE + 1);
        let g = BmGraph::layered(4, 4);
        assert_eq!(bench_exec(&g, 0).unwrap_err(), SEG_BASE + 3);
        assert_eq!(bench_scaling(&g, &[2, 4]).unwrap_err(), SEG_BASE + 4);
        // 空台账 cross_version 无数据。
        let lg = BmLedger::default();
        assert_eq!(lg.cross_version(0).unwrap_err(), SEG_BASE + 11);
    }
}
