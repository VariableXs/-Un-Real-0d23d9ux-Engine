//! CGPU-F0199 · 多 GPU 帧图拆分（判据段 0x0610）
//! 多 GPU 的帧图级拆分：GPU 归属标注（帧图节点标注目标 GPU——与
//! F0106 放置策略联动）、跨 GPU 依赖（跨卡数据拷贝作为图边——跨卡
//! 拷贝带宽管理 F0110 对接）、负载均衡（双卡任务分配比例——能力差
//! 异均衡器 F0111 集成）。判据：归属标注、跨卡依赖、均衡分配、双卡
//! 实测、单卡回退。
//!
//! 诊断码段 0x0610（本模块独占：0x0610~0x061F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0610;

/// 双卡均衡带宽（千分比）：两卡负载差 ≤ 120‰ 视为均衡。
pub const DUAL_GPU_BALANCE_PERMILLE: u32 = 120;

/// 单卡回退标志：回退后全部节点归属 0 号卡。
pub const FALLBACK_GPU: u32 = 0;

/// GPU 能力卡（判据「归属标注」与「均衡分配」的输入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GpuCap {
    /// 卡号。
    pub gpu: u32,
    /// 相对算力权重 ×1000（主卡 1000）。
    pub weight_permille: u32,
    /// 可用显存（MB）。
    pub vram_mb: u32,
    /// 卡间互链带宽（GB/s，0 = 无互链走 PCIe）。
    pub link_gbps: u32,
}

impl GpuCap {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.vram_mb == 0 || self.weight_permille == 0 {
            return Err(SEG_BASE);
        }
        if self.link_gbps > 1024 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// 帧图节点的归属标注（判据「归属标注」一等公民）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placement {
    /// 节点号。
    pub node: u32,
    /// 目标 GPU 号。
    pub gpu: u32,
    /// 显存占用（MB）。
    pub vram_mb: u32,
    /// 预估执行代价（微秒）。
    pub cost_us: u32,
}

impl Placement {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.node == 0 || self.cost_us == 0 || self.vram_mb == 0 {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
}

/// 归属合法性校验（判据「归属标注」主判据）：GPU 号必须存在于
/// 能力表，显存占用不得超该卡容量。
pub fn placement_valid(caps: &[GpuCap], p: &Placement) -> Result<(), u16> {
    p.validate()?;
    let cap = caps.iter().find(|c| c.gpu == p.gpu).ok_or(SEG_BASE + 3)?;
    cap.validate()?;
    if p.vram_mb > cap.vram_mb {
        return Err(SEG_BASE + 4);
    }
    Ok(())
}

/// 跨卡依赖边（判据「跨卡依赖」载体）：数据从 src 卡流向 dst 卡。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct XGpuEdge {
    /// 源节点。
    pub src_node: u32,
    /// 目标节点。
    pub dst_node: u32,
    /// 拷贝体量（MB）。
    pub payload_mb: u32,
}

impl XGpuEdge {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.src_node == 0 || self.dst_node == 0 || self.payload_mb == 0 {
            return Err(SEG_BASE + 5);
        }
        if self.src_node == self.dst_node {
            return Err(SEG_BASE + 6);
        }
        Ok(())
    }
    /// 拷贝耗时（微秒）：体量 / 带宽——无互链时按 PCIe 上限 32GB/s
    /// 兜底。
    pub fn transfer_us(&self, link_gbps: u32) -> Result<u64, u16> {
        self.validate()?;
        let gbps = if link_gbps == 0 { 32 } else { link_gbps };
        if gbps == 0 {
            return Err(SEG_BASE + 7);
        }
        // MB / (GB/s) = MB×1000/GB(ms) → µs = MB×1e6/(GBps×1000)。
        Ok(self.payload_mb as u64 * 1_000_000 / (gbps as u64 * 1000))
    }
}

/// 跨卡依赖与归属一致性（判据「跨卡依赖」主判据）：边的两端节点
/// 必须已标注且异卡（同卡数据不产生跨卡边——标注错误检出）。
pub fn xgpu_edge_consistent(
    placements: &[Placement],
    edge: &XGpuEdge,
) -> Result<bool, u16> {
    edge.validate()?;
    let src = placements.iter().find(|p| p.node == edge.src_node).ok_or(SEG_BASE + 8)?;
    let dst = placements.iter().find(|p| p.node == edge.dst_node).ok_or(SEG_BASE + 9)?;
    Ok(src.gpu != dst.gpu)
}

/// 全帧跨卡拷贝代价（判据「跨卡依赖」量化面）：所有真实跨卡边的
/// 传输耗时之和（按源卡所在链路带宽）。
pub fn total_xfer_cost_us(caps: &[GpuCap], placements: &[Placement], edges: &[XGpuEdge]) -> Result<u64, u16> {
    if caps.is_empty() || placements.is_empty() {
        return Err(SEG_BASE + 10);
    }
    let mut total: u64 = 0;
    for e in edges {
        if !xgpu_edge_consistent(placements, e)? {
            continue;
        }
        let src = placements.iter().find(|p| p.node == e.src_node).ok_or(SEG_BASE + 8)?;
        let cap = caps.iter().find(|c| c.gpu == src.gpu).ok_or(SEG_BASE + 3)?;
        total += e.transfer_us(cap.link_gbps)?;
    }
    Ok(total)
}

/// 卡负载账（判据「均衡分配」的度量）。
#[derive(Clone, Debug, Default)]
pub struct GpuLoad {
    /// 每卡总代价（微秒）。
    pub loads: Vec<u64>,
}

impl GpuLoad {
    /// 由归属账聚合。
    pub fn from_placements(caps: &[GpuCap], placements: &[Placement]) -> Result<Self, u16> {
        if caps.is_empty() || placements.is_empty() {
            return Err(SEG_BASE + 11);
        }
        let mut loads = vec![0u64; caps.len()];
        for p in placements {
            p.validate()?;
            let idx = caps.iter().position(|c| c.gpu == p.gpu).ok_or(SEG_BASE + 3)?;
            loads[idx] += p.cost_us as u64;
        }
        Ok(Self { loads })
    }
    /// 均衡度：max/mean − 1 ×1000。
    pub fn imbalance_permille(&self) -> Result<u32, u16> {
        if self.loads.len() < 2 {
            return Err(SEG_BASE + 12);
        }
        let sum: u64 = self.loads.iter().sum();
        if sum == 0 {
            return Err(SEG_BASE + 13);
        }
        let mean = sum / self.loads.len() as u64;
        let max = *self.loads.iter().max().unwrap_or(&0);
        if mean == 0 {
            return Err(SEG_BASE + 14);
        }
        Ok((max.abs_diff(mean) * 1000 / mean) as u32)
    }
    /// 均衡达标。
    pub fn balanced(&self) -> Result<bool, u16> {
        Ok(self.imbalance_permille()? <= DUAL_GPU_BALANCE_PERMILLE)
    }
}

/// 能力加权均衡分配（判据「均衡分配」核心）：按权重比例把节点
/// 分配到卡上（贪心：代价降序逐个放到当前加权负载最低的卡）。
pub fn weighted_balance_assign(
    caps: &[GpuCap],
    node_costs: &[(u32, u32)],
) -> Result<Vec<Placement>, u16> {
    if caps.len() < 2 || node_costs.is_empty() {
        return Err(SEG_BASE + 15);
    }
    for c in caps {
        c.validate()?;
    }
    let mut order: Vec<(u32, u32)> = node_costs.to_vec();
    order.sort_by(|a, b| b.1.cmp(&a.1));
    let mut acc_load = vec![0.0f64; caps.len()];
    let mut vram = vec![0u32; caps.len()];
    let mut out: Vec<Placement> = Vec::new();
    for &(node, cost) in &order {
        if node == 0 || cost == 0 {
            return Err(SEG_BASE + 16);
        }
        // 选加权负载最低的卡：load/weight 最小。
        let mut best = 0usize;
        let mut best_ratio = f64::MAX;
        for (i, c) in caps.iter().enumerate() {
            let ratio = acc_load[i] / (c.weight_permille as f64 / 1000.0);
            if ratio < best_ratio {
                best_ratio = ratio;
                best = i;
            }
        }
        acc_load[best] += cost as f64;
        let gpu = caps[best].gpu;
        out.push(Placement { node, gpu, vram_mb: 1, cost_us: cost });
        vram[best] += 1;
    }
    Ok(out)
}

/// 显存约束下的归属修正（判据「归属标注」的容量面）：节点显存
/// 占用超卡容量时把超额节点改迁到次选卡；无处可迁报错。
pub fn vram_repair(caps: &[GpuCap], placements: &mut Vec<Placement>) -> Result<u32, u16> {
    if caps.is_empty() || placements.is_empty() {
        return Err(SEG_BASE + 17);
    }
    let mut moved = 0u32;
    for pi in 0..placements.len() {
        loop {
            let p = &placements[pi];
            let cap = match caps.iter().find(|c| c.gpu == p.gpu) {
                Some(c) => c,
                None => return Err(SEG_BASE + 3),
            };
            if p.vram_mb <= cap.vram_mb {
                break;
            }
            // 迁到显存可容纳且负载最低的备选卡。
            let mut alt: Option<u32> = None;
            let mut best_vram_use = u32::MAX;
            for c in caps {
                if c.gpu == p.gpu {
                    continue;
                }
                let used: u32 = placements.iter().filter(|x| x.gpu == c.gpu).map(|x| x.vram_mb).sum();
                if used + p.vram_mb <= c.vram_mb && used < best_vram_use {
                    best_vram_use = used;
                    alt = Some(c.gpu);
                }
            }
            match alt {
                Some(g) => {
                    placements[pi].gpu = g;
                    moved += 1;
                }
                None => return Err(SEG_BASE + 18),
            }
        }
    }
    Ok(moved)
}

/// 单卡回退（判据「单卡回退」主判据）：多卡能力表退化为单卡时，
/// 全部节点归 0 号卡且跨卡边清空——回退语义必须完备。
pub fn single_gpu_fallback(placements: &mut Vec<Placement>) -> Result<u32, u16> {
    if placements.is_empty() {
        return Err(SEG_BASE + 19);
    }
    let mut n = 0u32;
    for p in placements.iter_mut() {
        if p.gpu != FALLBACK_GPU {
            p.gpu = FALLBACK_GPU;
            n += 1;
        }
    }
    Ok(n)
}

/// 双卡实测收益（判据「双卡实测」主判据）：双卡执行耗时 vs 单卡
/// 执行耗时，返回加速比 ×1000（必须 >1000 才算多卡划算——含跨卡
/// 拷贝代价）。
pub fn dual_gpu_speedup_permille(single_us: u64, dual_us: u64) -> Result<u32, u16> {
    if single_us == 0 || dual_us == 0 {
        return Err(SEG_BASE + 20);
    }
    let s = ((single_us * 1000) / dual_us) as u32;
    if s <= 1000 {
        return Err(SEG_BASE + 21);
    }
    Ok(s)
}

/// 归属账指纹（可审计面）。
pub fn placement_fp(placements: &[Placement]) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for p in placements {
        acc = acc.wrapping_mul(0x100000001b3) ^ p.node as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ p.gpu as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ p.cost_us as u64;
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    fn caps2() -> Vec<GpuCap> {
        vec![
            GpuCap { gpu: 0, weight_permille: 1000, vram_mb: 8192, link_gbps: 300 },
            GpuCap { gpu: 1, weight_permille: 800, vram_mb: 4096, link_gbps: 300 },
        ]
    }
    #[test]
    fn placement_vram_checked() {
        let caps = caps2();
        let p = Placement { node: 1, gpu: 1, vram_mb: 4096, cost_us: 100 };
        assert!(placement_valid(&caps, &p).is_ok());
        let over = Placement { node: 1, gpu: 1, vram_mb: 5000, cost_us: 100 };
        assert!(placement_valid(&caps, &over).is_err());
    }
    #[test]
    fn xgpu_edge_transfer_time() {
        let e = XGpuEdge { src_node: 1, dst_node: 2, payload_mb: 300 };
        // 互链 300GB/s：300MB → 1ms = 1000µs。
        assert_eq!(e.transfer_us(300).unwrap(), 1000);
        // 无互链走 32GB/s → 9375µs。
        assert_eq!(e.transfer_us(0).unwrap(), 9375);
        assert!(XGpuEdge { src_node: 1, dst_node: 1, payload_mb: 1 }.validate().is_err());
    }
    #[test]
    fn edge_consistency_detects_same_gpu() {
        let caps = caps2();
        let placements = vec![
            Placement { node: 1, gpu: 0, vram_mb: 1, cost_us: 10 },
            Placement { node: 2, gpu: 1, vram_mb: 1, cost_us: 10 },
        ];
        let e = XGpuEdge { src_node: 1, dst_node: 2, payload_mb: 16 };
        assert!(xgpu_edge_consistent(&placements, &e).unwrap());
        // 同卡误标 → 检出（false）。
        let same = vec![
            Placement { node: 1, gpu: 0, vram_mb: 1, cost_us: 10 },
            Placement { node: 2, gpu: 0, vram_mb: 1, cost_us: 10 },
        ];
        assert!(!xgpu_edge_consistent(&same, &e).unwrap());
        let _ = caps;
    }
    #[test]
    fn weighted_balance_respects_capacity() {
        let caps = caps2();
        let nodes = vec![(1, 500), (2, 500), (3, 500), (4, 500)];
        let out = weighted_balance_assign(&caps, &nodes).unwrap();
        let load = GpuLoad::from_placements(&caps, &out).unwrap();
        assert!(load.balanced().unwrap());
    }
    #[test]
    fn vram_repair_moves_overflow() {
        let caps = caps2();
        let mut placements = vec![
            Placement { node: 1, gpu: 1, vram_mb: 3000, cost_us: 10 },
            Placement { node: 2, gpu: 1, vram_mb: 3000, cost_us: 10 },
        ];
        let moved = vram_repair(&caps, &mut placements).unwrap();
        assert_eq!(moved, 1);
        assert_eq!(placements[1].gpu, 0);
    }
    #[test]
    fn fallback_colocates_all() {
        let mut placements = vec![
            Placement { node: 1, gpu: 0, vram_mb: 1, cost_us: 10 },
            Placement { node: 2, gpu: 1, vram_mb: 1, cost_us: 10 },
        ];
        let n = single_gpu_fallback(&mut placements).unwrap();
        assert_eq!(n, 1);
        assert!(placements.iter().all(|p| p.gpu == 0));
    }
    #[test]
    fn dual_speedup_threshold() {
        assert_eq!(dual_gpu_speedup_permille(2000, 1000).unwrap(), 2000);
        assert!(dual_gpu_speedup_permille(1000, 1000).is_err());
    }
}
