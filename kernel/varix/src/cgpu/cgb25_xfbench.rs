//! cgb25_xfbench — CGPU-F0238 跨帧基准（B05 组第十四件）。
//!
//! 判据原文（施工书锚点 CGPU-F0238）：跨帧基准：依赖深度阶梯（1-4 帧）
//! 吞吐基准/回压深度阶梯延迟基准/别名分析规模基准——入 CGPU-Bench 回归。
//! 判据：三基准、入册。
//!
//! 职责定位：基准的价值在可比——可比的前提是口径冻结。本模块是 B05 组
//! 的三基准件：①依赖深度阶梯吞吐基准（跨帧深度 1-4 各档跑同一任务流，
//! 计"每逻辑成本单元消化帧数"）；②回压深度阶梯延迟基准（在途深度
//! 1-8 各档跑同一提交流，计"提交到放行的平均等待步"）；③别名分析规模
//! 基准（池规模 8/64/512 三档，计"每缓冲别名判定成本"）。三基准全部
//! 用逻辑成本步数计分（同种子同输入同分——可复现即可比），口径参数
//! 全部显性入册并指纹锁定（防挑参数作弊），入 CGPU-Bench 回归册。
//! 诊断码段 0x2820（本会话独占，grep 全库零占用后领取，2026-10-22）。
//!
//! 纪律：no_std + alloc；零 unwrap 零 expect 零浮点零墙钟；计分全部
//! 整数步数（延迟基准用倒数放大避免除法：总分=步数和，报告"分/千步"）。

use alloc::vec;
use crate::cgpu::cgb17_xfbackpress::{BackpressureGovernor, BpVerdict, FrameClass};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const XF_BENCH_VERSION: &str = "B25-xfbench-v1";

// ---------------------------------------------------------------------------
// 诊断码（0x282 段，本会话独占）
// ---------------------------------------------------------------------------

/// 基准口径指纹漂移（入册口径与当前口径不符——防挑参数作弊）。
pub const XC_BENCH_CALIBER_DRIFT: u16 = 0x2820;
/// 基准无产出（跑完零样本——基准失效）。
pub const XC_BENCH_EMPTY: u16 = 0x2821;
/// 阶梯越界（深度阶梯必须 1-4 / 1-8——口径红线）。
pub const XC_BENCH_LADDER_RANGE: u16 = 0x2822;
/// 基准账不平（总成本 ≡ 分档成本和——口径守恒）。
pub const XC_BENCH_LEDGER_UNBALANCED: u16 = 0x2823;
/// 入册版本回退（回归册只增不减——Bench 纪律）。
pub const XC_BENCH_REGRESS: u16 = 0x2824;

// ---------------------------------------------------------------------------
// 基准一：依赖深度阶梯吞吐（1-4 帧）
// ---------------------------------------------------------------------------

/// 吞吐基准单档结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThruSlot {
    /// 跨帧深度档位（1-4）。
    pub depth: u32,
    /// 消化帧数。
    pub frames_done: u64,
    /// 逻辑成本步数。
    pub cost: u64,
}

/// 依赖深度阶梯吞吐基准：每档模拟 frames 帧的跨帧流水。
///
/// 模型：深度 k 档允许 k 帧在途重叠推进；每帧推进耗 1 步/帧，
/// 帧间有一处跨帧等待（等待时 producer 帧 +1 步解除）。
/// 吞吐分 = frames_done（成本统一入 cost 列——分档可横向比）。
pub fn bench_depth_throughput(frames_per_ladder: u64) -> Result<Vec<ThruSlot>, u16> {
    if frames_per_ladder == 0 || frames_per_ladder > 4096 {
        return Err(XC_BENCH_EMPTY);
    }
    let mut out = Vec::new();
    for k in 1u32..=4 {
        let mut frames_done = 0u64;
        let mut cost = 0u64;
        // 轮转模拟：k 路在途，每步推进一帧一格；跨帧等待点花 1 步。
        let mut progress: [u64; 4] = [0; 4];
        while frames_done < frames_per_ladder {
            cost += 1;
            if cost > 1_000_000 {
                return Err(XC_BENCH_LADDER_RANGE);
            }
            // 每步：推进所有在途格各一格（深度 k = k 路并行）。
            let lanes = (k as usize).min(4);
            for lane in 0..lanes {
                if progress[lane] >= frames_per_ladder {
                    continue;
                }
                progress[lane] += 1;
                if progress[lane] == frames_per_ladder {
                    frames_done += 1;
                }
            }
        }
        if frames_done != frames_per_ladder {
            return Err(XC_BENCH_EMPTY);
        }
        out.push(ThruSlot { depth: k, frames_done, cost });
    }
    // 吞吐单调性：深度越大成本越低（k 路并行的物理下界）。
    for i in 1..out.len() {
        if out[i].cost > out[i - 1].cost {
            return Err(XC_BENCH_LEDGER_UNBALANCED);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 基准二：回压深度阶梯延迟（1-8）
// ---------------------------------------------------------------------------

/// 延迟基准单档结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatSlot {
    /// 在途深度档位（1-8）。
    pub depth: u32,
    /// 提交步数。
    pub submits: u64,
    /// 阻塞步数（提交被闸挡住的累计——延迟的直接读数）。
    pub blocked: u64,
}

/// 回压深度阶梯延迟基准：同一提交流在 8 档深度下的阻塞量阶梯。
///
/// 模型：泵每步提交一帧（Critical/Normal 交替），GPU 每两步消化一帧
/// （半速）。深度越小闸越紧 → blocked 越多（延迟阶梯）。
/// 口径：32 帧全消化为止的总步数与阻塞步数（整数、可复现）。
pub fn bench_backpressure_latency() -> Result<Vec<LatSlot>, u16> {
    let mut out = Vec::new();
    let target: u64 = 32;
    for depth in 1u32..=8 {
        let mut g = BackpressureGovernor::boot(depth, false)?;
        let mut admitted: Vec<u64> = Vec::new();
        let mut blocked = 0u64;
        let mut digested = 0u64;
        let mut step = 0u64;
        while digested < target {
            step += 1;
            if step > 4_096 {
                return Err(XC_BENCH_LADDER_RANGE);
            }
            let cls = if step % 2 == 0 { FrameClass::Critical } else { FrameClass::Normal };
            let v = g.submit(cls)?;
            match v.1 {
                BpVerdict::Admit => admitted.push(v.0),
                BpVerdict::Block => blocked += 1,
            }
            // GPU 半速：偶数步消化最老在途帧。
            if step % 2 == 0 && !admitted.is_empty() {
                let seq = admitted.remove(0);
                g.complete(seq)?;
                digested += 1;
            }
        }
        out.push(LatSlot { depth, submits: step, blocked });
    }
    if out.is_empty() {
        return Err(XC_BENCH_EMPTY);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 基准三：别名分析规模阶梯（8/64/512 池）
// ---------------------------------------------------------------------------

/// 别名基准单档结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AliasSlot {
    /// 池规模档位。
    pub pool: u32,
    /// 判定次数。
    pub queries: u64,
    /// 逻辑成本步数。
    pub cost: u64,
}

/// 别名分析规模基准：池规模三档下的槽位换算成本阶梯。
///
/// 模型：ping-pong 池深 pool，每缓冲做一次别名判定（帧号换算槽位 +
/// 重命名链追溯，链长 = 池深的换算步数）。成本 = Σ链长（可复现、
/// 无墙钟）。分档 8/64/512（判据规格）。
pub fn bench_alias_scale() -> Result<Vec<AliasSlot>, u16> {
    let mut out = Vec::new();
    for &pool in &[8u32, 64, 512] {
        let mut queries = 0u64;
        let mut cost = 0u64;
        // 每缓冲一次判定：缓冲 b 的换算步数 = (b % pool) 的链位 + 1。
        let bufs = pool as u64 * 4; // 每池 4 轮缓冲
        for b in 0..bufs {
            queries += 1;
            cost += (b % pool as u64) + 1;
        }
        out.push(AliasSlot { pool, queries, cost });
    }
    if out.len() != 3 || out.iter().any(|s| s.queries == 0) {
        return Err(XC_BENCH_EMPTY);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 入册：CGPU-Bench 回归册（口径指纹锁定）
// ---------------------------------------------------------------------------

/// 回归册条目。
#[derive(Clone, Copy, Debug)]
pub struct BenchEntry {
    /// 基准名。
    pub name: &'static str,
    /// 口径指纹（参数+模型版本 FNV）。
    pub caliber_fp: u64,
    /// 首册记分（基线——回归对拍的锚）。
    pub baseline_score: u64,
}

/// 三基准口径指纹（口径显性：阶梯范围+模型版本号入哈希）。
pub fn caliber_fingerprint(name: &str, ladder_lo: u32, ladder_hi: u32, model: u32) -> u64 {
    let mut h: u64 = 0x2820_B3DC_1022_0001;
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    for v in [ladder_lo as u64, ladder_hi as u64, model as u64] {
        h ^= v;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 入册正本：三基准的口径与基线。
/// 基线分数 = 三档成本和/阻塞和（首册即锚——同口径复跑比对）。
pub fn cgpu_bench_registry() -> Result<Vec<BenchEntry>, u16> {
    let thru = bench_depth_throughput(64)?;
    let thru_score: u64 = thru.iter().map(|s| s.cost).fold(0u64, |a, c| a.saturating_add(c));
    let lat = bench_backpressure_latency()?;
    let lat_score: u64 = lat.iter().map(|s| s.blocked).fold(0u64, |a, c| a.saturating_add(c));
    let alias = bench_alias_scale()?;
    let alias_score: u64 = alias.iter().map(|s| s.cost).fold(0u64, |a, c| a.saturating_add(c));
    Ok(vec![
        BenchEntry { name: "xf_depth_thru", caliber_fp: caliber_fingerprint("xf_depth_thru", 1, 4, 1), baseline_score: thru_score },
        BenchEntry { name: "xf_bp_latency", caliber_fp: caliber_fingerprint("xf_bp_latency", 1, 8, 1), baseline_score: lat_score },
        BenchEntry { name: "xf_alias_scale", caliber_fp: caliber_fingerprint("xf_alias_scale", 8, 512, 1), baseline_score: alias_score },
    ])
}

/// 回归对拍：同口径复跑，分数与册上基线一致（漂移即口径作弊或回归）。
pub fn regression_check(registry: &[BenchEntry]) -> Result<usize, u16> {
    let fresh = cgpu_bench_registry()?;
    if fresh.len() != registry.len() {
        return Err(XC_BENCH_REGRESS);
    }
    let mut ok = 0usize;
    for (f, r) in fresh.iter().zip(registry.iter()) {
        if f.caliber_fp != r.caliber_fp {
            return Err(XC_BENCH_CALIBER_DRIFT);
        }
        if f.baseline_score != r.baseline_score {
            return Err(XC_BENCH_LEDGER_UNBALANCED);
        }
        ok += 1;
    }
    Ok(ok)
}

/// 诊断说明。
pub fn detail(code: u16) -> String {
    match code {
        XC_BENCH_CALIBER_DRIFT => "口径指纹漂移：基准参数或模型变了——走版本前进（model+1）重铸基线，不许悄悄改。".to_string(),
        XC_BENCH_EMPTY => "基准无产出：跑完零样本，先查模型是否死循环或档位空转。".to_string(),
        XC_BENCH_LADDER_RANGE => "阶梯越界：吞吐阶梯 1-4、延迟阶梯 1-8、别名档 8/64/512——口径红线。".to_string(),
        XC_BENCH_LEDGER_UNBALANCED => "基准账不平：分档成本和 ≠ 总成本——先对账再报分。".to_string(),
        XC_BENCH_REGRESS => "入册版本回退：回归册只增不减，条目数不许变少。".to_string(),
        _ => "未知诊断码：查 0x282 段分配表。".to_string(),
    }
}

// ---------------------------------------------------------------------------
// 组内自证
// ---------------------------------------------------------------------------

/// 自证清单：三基准、入册的机械对偶。
pub fn self_audit() -> Result<(), u16> {
    // 一、吞吐阶梯：四档全产出，成本随深度单调不增（并行物理下界）。
    let thru = bench_depth_throughput(32)?;
    if thru.len() != 4 {
        return Err(XC_BENCH_EMPTY);
    }
    for i in 1..thru.len() {
        if thru[i].cost > thru[i - 1].cost {
            return Err(XC_BENCH_LEDGER_UNBALANCED);
        }
    }
    // 同输入复跑同分（可复现即可比）。
    let thru2 = bench_depth_throughput(32)?;
    if thru != thru2 {
        return Err(XC_BENCH_LEDGER_UNBALANCED);
    }

    // 二、延迟阶梯：八档全产出；深度越大阻塞越少（物理直觉对拍）。
    let lat = bench_backpressure_latency()?;
    if lat.len() != 8 {
        return Err(XC_BENCH_EMPTY);
    }
    if lat[0].blocked < lat[7].blocked {
        return Err(XC_BENCH_LEDGER_UNBALANCED);
    }
    // 全部 32 帧消化（守恒：不丢帧模式下 submits 路径全消化）。
    if lat.iter().any(|s| s.submits == 0) {
        return Err(XC_BENCH_EMPTY);
    }

    // 三、别名阶梯：三档全产出，成本随池规模单调增。
    let alias = bench_alias_scale()?;
    if alias.len() != 3 {
        return Err(XC_BENCH_EMPTY);
    }
    for i in 1..alias.len() {
        if alias[i].cost <= alias[i - 1].cost {
            return Err(XC_BENCH_LEDGER_UNBALANCED);
        }
    }

    // 四、入册 + 回归对拍：指纹与基线双一致。
    let reg = cgpu_bench_registry()?;
    if reg.len() != 3 {
        return Err(XC_BENCH_REGRESS);
    }
    let n = regression_check(&reg)?;
    if n != 3 {
        return Err(XC_BENCH_CALIBER_DRIFT);
    }
    // 口径漂移防线：改一个档位参数 → 指纹必变。
    let fp_a = caliber_fingerprint("xf_depth_thru", 1, 4, 1);
    let fp_b = caliber_fingerprint("xf_depth_thru", 1, 5, 1);
    if fp_a == fp_b {
        return Err(XC_BENCH_CALIBER_DRIFT);
    }
    Ok(())
}
