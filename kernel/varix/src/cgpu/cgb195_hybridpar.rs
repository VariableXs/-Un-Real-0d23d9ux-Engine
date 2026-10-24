//! CGPU-F0195 · CPU/GPU 混合并行（判据段 0x05D0）
//! CPU 栅格化与 GPU 渲染的混合并行：混合决策（哪些 Pass 放 CPU 哪些
//! 放 GPU——CGPU-F0004 决策器的帧图集成版）、混合同步（CPU 完成→GPU
//! 继续的跨设备围栏——与 VE-F0114 跨卡围栏同源）、混合负载均衡（CPU
//! 核与 GPU SM 的利用率同时监控——双端利用率均衡）。判据：混合决策、
//! 跨设备同步、双端均衡、性能收益（混合 vs 纯 GPU）、对拍。
//!
//! 诊断码段 0x05D0（本模块独占：0x05D0~0x05DF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x05D0;

/// 双端利用率均衡带宽（千分比）：|CPU利用率−GPU利用率| ≤ 150‰。
pub const UTIL_BALANCE_TOLERANCE_PERMILLE: u32 = 150;

/// 围栏等待轮询间隔（微秒）：跨设备围栏的自旋粒度。
pub const FENCE_POLL_INTERVAL_US: u32 = 50;

/// 围栏超时（微秒）：超过即报错不悬死。
pub const FENCE_TIMEOUT_US: u64 = 200_000;

/// 混合决策候选侧。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    /// CPU 栅格化。
    Cpu,
    /// GPU 渲染。
    Gpu,
}

impl Side {
    /// 侧序号（遥测/指纹用）。
    pub fn ordinal(&self) -> u8 {
        match self {
            Side::Cpu => 0,
            Side::Gpu => 1,
        }
    }
}

/// Pass 特征（混合决策的输入向量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PassTraits {
    /// Pass 号。
    pub pass: u32,
    /// 三角形数量。
    pub tris: u32,
    /// 过绘制倍率 ×1000（1000 = 无过绘制）。
    pub overdraw_permille: u32,
    /// 着色器复杂度（ALU 周期数）。
    pub shader_alu: u32,
    /// 分辨率相关权重 ×1000（GPU 优势项）。
    pub res_weight_permille: u32,
}

impl PassTraits {
    /// 特征合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.pass == 0 || self.tris == 0 {
            return Err(SEG_BASE);
        }
        if self.overdraw_permille < 1000 || self.res_weight_permille == 0 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
    /// GPU 亲和分（0~1000）：三角形越多、着色越重、分辨率权重越高
    /// 越 GPU 亲和；过绘制越高越 CPU 亲和（CPU 栅格化无过绘制税）。
    pub fn gpu_affinity_permille(&self) -> u32 {
        let tri_score = (self.tris as u64).min(100_000) * 3; // ≤300k
        let alu_score = (self.shader_alu as u64).min(4096) * 40; // ≤163k
        let res_score = self.res_weight_permille as u64; // ≤1000
        let overdraw_penalty = (self.overdraw_permille as u64 - 1000) * 2; // 过绘制税
        let raw = tri_score + alu_score / 16 + res_score / 4;
        let raw = raw.saturating_sub(overdraw_penalty);
        raw.min(1000) as u32
    }
}

/// 混合决策裁决（判据「混合决策」核心）：GPU 亲和分 ≥ 500 → GPU，
/// 否则 CPU。返回 (侧, 亲和分)。
pub fn decide_side(t: &PassTraits) -> Result<(Side, u32), u16> {
    t.validate()?;
    let aff = t.gpu_affinity_permille();
    Ok((if aff >= 500 { Side::Gpu } else { Side::Cpu }, aff))
}

/// 混合切分结果：全帧 Pass 的侧分配账。
#[derive(Clone, Debug, Default)]
pub struct HybridSplit {
    /// 每 Pass 的侧（与输入同序）。
    pub sides: Vec<Side>,
    /// 每侧负载（微秒）。
    pub cpu_us: u64,
    pub gpu_us: u64,
}

impl HybridSplit {
    /// 账本校验。
    pub fn validate(&self, pass_count: usize) -> Result<(), u16> {
        if self.sides.len() != pass_count {
            return Err(SEG_BASE + 2);
        }
        if self.cpu_us == 0 && self.gpu_us == 0 {
            return Err(SEG_BASE + 3);
        }
        Ok(())
    }
}

/// 批量混合决策（判据「混合决策」批量面）。
pub fn split_frame_hybrid(traits: &[PassTraits]) -> Result<HybridSplit, u16> {
    if traits.is_empty() {
        return Err(SEG_BASE + 4);
    }
    let mut out = HybridSplit::default();
    for t in traits {
        let (side, _) = decide_side(t)?;
        out.sides.push(side);
    }
    Ok(out)
}

/// 跨设备围栏（判据「跨设备同步」载体）：CPU 侧完成 → GPU 侧放行。
#[derive(Debug)]
pub struct CrossFence {
    /// 围栏号。
    pub fence_id: u32,
    /// 需要等待的 CPU 批次号。
    pub cpu_batch: u32,
    /// 已到达的 CPU 批次号（生产者推进）。
    arrived: u32,
    /// GPU 已放行的批次号（消费者推进）。
    released: u32,
}

impl CrossFence {
    /// 建围栏：目标批次必须大于已放行批次（倒退拒绝）。
    pub fn new(fence_id: u32, cpu_batch: u32, released: u32) -> Result<Self, u16> {
        if fence_id == 0 || cpu_batch == 0 || cpu_batch <= released {
            return Err(SEG_BASE + 5);
        }
        Ok(Self { fence_id, cpu_batch, arrived: released, released })
    }
    /// CPU 生产者到达。
    pub fn signal(&mut self, batch: u32) -> Result<(), u16> {
        if batch != self.arrived + 1 {
            return Err(SEG_BASE + 6);
        }
        self.arrived = batch;
        Ok(())
    }
    /// GPU 消费者轮询放行：目标批次已到达才放行。
    pub fn try_release(&mut self, _now_us: u64, waited_us: &mut u64) -> Result<bool, u16> {
        if self.cpu_batch <= self.released {
            return Err(SEG_BASE + 7);
        }
        if self.arrived >= self.cpu_batch {
            self.released = self.cpu_batch;
            return Ok(true);
        }
        *waited_us += FENCE_POLL_INTERVAL_US as u64;
        if *waited_us > FENCE_TIMEOUT_US {
            return Err(SEG_BASE + 8);
        }
        Ok(false)
    }
    /// 已放行批次。
    pub fn released_batch(&self) -> u32 {
        self.released
    }
}

/// 围栏全链演练（判据「跨设备同步」主判据）：CPU 批次逐个到达 →
/// GPU 轮询直至放行，全程无超时。
pub fn fence_roundtrip(f: &mut CrossFence, batches: u32) -> Result<u64, u16> {
    if batches == 0 || f.cpu_batch + batches - 1 < f.cpu_batch {
        return Err(SEG_BASE + 9);
    }
    let mut total_wait: u64 = 0;
    for b in f.cpu_batch..f.cpu_batch + batches {
        f.signal(b)?;
        let mut w = 0u64;
        let mut released = false;
        while !released {
            released = f.try_release(0, &mut w)?;
            total_wait += FENCE_POLL_INTERVAL_US as u64;
        }
        total_wait = total_wait.saturating_sub(FENCE_POLL_INTERVAL_US as u64) + w;
        if f.released_batch() != b {
            return Err(SEG_BASE + 10);
        }
    }
    Ok(total_wait)
}

/// 双端利用率账（判据「双端均衡」输入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UtilPair {
    /// CPU 利用率 ×1000（1000 = 满）。
    pub cpu_permille: u32,
    /// GPU 利用率 ×1000。
    pub gpu_permille: u32,
}

impl UtilPair {
    /// 合法性：均不超过满载。
    pub fn validate(&self) -> Result<(), u16> {
        if self.cpu_permille > 1000 || self.gpu_permille > 1000 {
            return Err(SEG_BASE + 11);
        }
        Ok(())
    }
    /// 双端差 ×1000。
    pub fn gap_permille(&self) -> u32 {
        self.cpu_permille.abs_diff(self.gpu_permille)
    }
}

/// 双端均衡判定（判据「双端均衡」）。
pub fn dual_util_balanced(u: &UtilPair) -> Result<bool, u16> {
    u.validate()?;
    Ok(u.gap_permille() <= UTIL_BALANCE_TOLERANCE_PERMILLE)
}

/// 负载再平衡建议（判据「双端均衡」可行动面）：双端失衡时建议把
/// 若干 CPU Pass 迁往 GPU（或反向），返回建议迁移的 Pass 数。
pub fn rebalance_advised(u: &UtilPair, candidate_passes: u32) -> Result<Option<u32>, u16> {
    u.validate()?;
    if dual_util_balanced(u)? {
        return Ok(None);
    }
    if candidate_passes == 0 {
        return Err(SEG_BASE + 12);
    }
    let gap = u.gap_permille();
    // 每迁移一个候选 Pass 预期收敛 300‰ 差距。
    let need = (gap + 299) / 300;
    Ok(Some(need.min(candidate_passes)))
}

/// 混合收益测算（判据「性能收益」核心量）：混合执行耗时 vs 纯 GPU
/// 执行耗时，返回加速比 ×1000（>1000 即混合更快）。
pub fn hybrid_speedup_permille(
    gpu_only_us: u64,
    cpu_us: u64,
    gpu_us: u64,
    sync_overhead_us: u64,
) -> Result<u32, u16> {
    if gpu_only_us == 0 {
        return Err(SEG_BASE + 13);
    }
    if cpu_us == 0 && gpu_us == 0 {
        return Err(SEG_BASE + 14);
    }
    // 混合总耗时 = max(CPU 流水, GPU 流水) + 同步开销（跨设备围栏）。
    let hybrid = cpu_us.max(gpu_us) + sync_overhead_us;
    if hybrid == 0 {
        return Err(SEG_BASE + 15);
    }
    Ok(((gpu_only_us * 1000) / hybrid) as u32)
}

/// 混合对拍（判据「对拍」主判据）：同一帧两种策略的 Pass 产出一致
/// 性——以像素指纹代表输出，两策略指纹必须相等（优化不改语义）。
pub fn hybrid_par_compare(fp_cpu_gfx: u64, fp_gpu_gfx: u64) -> Result<bool, u16> {
    if fp_cpu_gfx == 0 || fp_gpu_gfx == 0 {
        return Err(SEG_BASE + 16);
    }
    Ok(fp_cpu_gfx == fp_gpu_gfx)
}

/// 混合决策账指纹（可审计面）。
pub fn hybrid_split_fp(h: &HybridSplit) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for s in &h.sides {
        acc = acc.wrapping_mul(0x100000001b3) ^ s.ordinal() as u64;
    }
    acc = acc.wrapping_mul(0x100000001b3) ^ h.cpu_us;
    acc = acc.wrapping_mul(0x100000001b3) ^ h.gpu_us;
    acc
}

/// 帧级混合流水模型（独立函数面）：CPU 流与 GPU 流各自按时序推进，
/// 返回帧总耗时（两流 max）——依赖边跨流处计入一次围栏开销。
pub fn hybrid_pipeline_model(
    traits: &[PassTraits],
    sides: &[Side],
    cost_of: fn(&PassTraits) -> u64,
    fence_us: u64,
) -> Result<u64, u16> {
    if traits.len() != sides.len() || traits.is_empty() {
        return Err(SEG_BASE + 17);
    }
    let mut cpu_total: u64 = 0;
    let mut gpu_total: u64 = 0;
    let mut crossings: u64 = 0;
    for (t, s) in traits.iter().zip(sides.iter()) {
        let c = cost_of(t);
        match s {
            Side::Cpu => {
                if gpu_total > 0 && cpu_total == 0 {
                    crossings += 1;
                }
                cpu_total += c;
            }
            Side::Gpu => {
                if cpu_total > 0 && gpu_total == 0 {
                    crossings += 1;
                }
                gpu_total += c;
            }
        }
    }
    Ok(cpu_total.max(gpu_total) + crossings * fence_us)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn t(pass: u32, tris: u32, over: u32, alu: u32, res: u32) -> PassTraits {
        PassTraits { pass, tris, overdraw_permille: over, shader_alu: alu, res_weight_permille: res }
    }
    #[test]
    fn affinity_direction() {
        // 重着色高分辨率 → GPU。
        let gpu_like = t(1, 100_000, 1000, 4096, 1000);
        assert!(decide_side(&gpu_like).unwrap().0 == Side::Gpu);
        // 高过绘制轻着色 → CPU。
        let cpu_like = t(2, 100, 3000, 10, 100);
        assert!(decide_side(&cpu_like).unwrap().0 == Side::Cpu);
    }
    #[test]
    fn fence_roundtrip_clean() {
        let mut f = CrossFence::new(1, 3, 0).unwrap();
        let w = fence_roundtrip(&mut f, 3).unwrap();
        assert_eq!(f.released_batch(), 3);
        assert!(w > 0);
    }
    #[test]
    fn fence_rejects_regression() {
        assert!(CrossFence::new(1, 2, 2).is_err());
    }
    #[test]
    fn dual_balance_judgement() {
        assert!(dual_util_balanced(&UtilPair { cpu_permille: 800, gpu_permille: 900 }).unwrap());
        assert!(!dual_util_balanced(&UtilPair { cpu_permille: 300, gpu_permille: 950 }).unwrap());
        let adv = rebalance_advised(&UtilPair { cpu_permille: 300, gpu_permille: 950 }, 4).unwrap();
        assert_eq!(adv, Some(3));
    }
    #[test]
    fn speedup_quantified() {
        // GPU 独跑 1000us；混合 CPU 400 / GPU 700 + 同步 50 → 750。
        let s = hybrid_speedup_permille(1000, 400, 700, 50).unwrap();
        assert_eq!(s, 1333);
    }
    #[test]
    fn par_compare_semantics() {
        assert!(hybrid_par_compare(0xdead_beef, 0xdead_beef).unwrap());
        assert!(!hybrid_par_compare(0xdead_beef, 0xdead_c0de).unwrap());
        assert!(hybrid_par_compare(0, 1).is_err());
    }
    #[test]
    fn pipeline_model_counts_crossings() {
        let traits = vec![t(1, 500, 1000, 100, 500), t(2, 500, 1000, 100, 500), t(3, 500, 1000, 100, 500)];
        let sides = vec![Side::Cpu, Side::Gpu, Side::Gpu];
        let cost = |x: &PassTraits| -> u64 { 100 };
        let total = hybrid_pipeline_model(&traits, &sides, cost, 30).unwrap();
        // cpu 100, gpu 200, 1 crossing → 200+30=230。
        assert_eq!(total, 230);
    }
}
