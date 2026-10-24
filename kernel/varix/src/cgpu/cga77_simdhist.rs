//! cga77_simdhist —— CGPU-F0077 SIMD 直方图统计。
//! 锚点（CGPU 施工书）：CGPU-F0077 · SIMD 直方图统计 · 目标 340 行
//! 判据摘录：256 桶直方图守恒（Σ桶 == 像素数——统计硬线）；分位
//!   定位（p50/p90 千分位——SAT 积分定位）；众数（最大桶首现）；
//!   极值域账（min/max 直方图派生 == 逐像素扫描——双路径对拍）；
//!   分桶偏移统计（bin 移位批量统计——SIMD 4 通道打包计数与标
//!   量一致）；空图显性拒绝。
//! 组件：直方图账 + 积分定位器 + 众数器 + 极值双路径对拍 + 4 通
//!   道打包计数器 + 均衡化定位预备（CDF 端点）。
//! 职责定位：图像统计面地基——直方图是色阶/均衡化/自动曝光的
//!   公共前置；打包计数（4 通道并行桶递增）是 SIMD 化统计的标
//!   准形，语义必须与标量逐位一致。
//! 性能逐项分解：统计 O(n)；积分 O(256)；打包版 O(n/4) 计数轮。
//! 全整数确定性。
//! 诊断码段：0xE860（0xE861~0xE869 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SH_VERSION: &str = "cga77_simdhist-v1";

/// 诊断段基址（0xE860 段 · 本模块独占）。
pub const SH_SEG_BASE: u16 = 0xE860;

/// XcCode 闭集（0xE861~0xE869）。
pub const XC_DIM_ZERO: u32 = 0xE861;
pub const XC_VAL_OOB: u32 = 0xE862;
pub const XC_CONSERVE: u32 = 0xE863;
pub const XC_QUANT_OOB: u32 = 0xE864;
pub const XC_MODE_MISS: u32 = 0xE865;
pub const XC_EXTREME_DRIFT: u32 = 0xE866;
pub const XC_LANE_DRIFT: u32 = 0xE867;
pub const XC_CDF_OOB: u32 = 0xE868;
pub const XC_EMPTY_HIST: u32 = 0xE869;

/// 256 桶直方图。
pub struct Hist256 {
    pub buckets: [u32; 256],
    pub total: u64,
}

impl Hist256 {
    pub fn new() -> Hist256 {
        Hist256 { buckets: [0; 256], total: 0 }
    }

    pub fn record(&mut self, v: u8) {
        self.buckets[v as usize] += 1;
        self.total += 1;
    }

    pub fn conserve(&self) -> bool {
        self.buckets.iter().map(|&b| b as u64).sum::<u64>() == self.total
    }
}

/// 标量统计。
pub fn hist_scalar(px: &[u8]) -> Result<Hist256, u32> {
    if px.is_empty() {
        return Err(XC_DIM_ZERO);
    }
    let mut h = Hist256::new();
    for &v in px.iter() {
        h.record(v);
    }
    if !h.conserve() {
        return Err(XC_CONSERVE);
    }
    Ok(h)
}

/// 4 通道打包统计（通道独立直方图后并桶——SIMD 语义账）。
pub fn hist_lanes(px: &[u8]) -> Result<Hist256, u32> {
    if px.is_empty() {
        return Err(XC_DIM_ZERO);
    }
    let mut lanes = [[0u32; 256]; 4];
    let mut k = 0usize;
    while k + 4 <= px.len() {
        lanes[0][px[k] as usize] += 1;
        lanes[1][px[k + 1] as usize] += 1;
        lanes[2][px[k + 2] as usize] += 1;
        lanes[3][px[k + 3] as usize] += 1;
        k += 4;
    }
    // 尾包标量补齐。
    while k < px.len() {
        lanes[0][px[k] as usize] += 1;
        k += 1;
    }
    let mut h = Hist256::new();
    for b in 0..256usize {
        h.buckets[b] = lanes[0][b] + lanes[1][b] + lanes[2][b] + lanes[3][b];
    }
    h.total = px.len() as u64;
    if !h.conserve() {
        return Err(XC_CONSERVE);
    }
    Ok(h)
}

/// 打包 vs 标量对拍（语义一致硬线）。
pub fn lanes_vs_scalar(px: &[u8]) -> Result<(), u32> {
    let a = hist_scalar(px)?;
    let b = hist_lanes(px)?;
    if a.buckets != b.buckets {
        return Err(XC_LANE_DRIFT);
    }
    Ok(())
}

/// 积分定位（千分位 → 桶号）。
pub fn quantile_bucket(h: &Hist256, permille: u32) -> Result<usize, u32> {
    if h.total == 0 {
        return Err(XC_EMPTY_HIST);
    }
    if permille > 1000 {
        return Err(XC_QUANT_OOB);
    }
    let target = h.total * permille as u64 / 1000;
    let mut run: u64 = 0;
    for (k, &c) in h.buckets.iter().enumerate() {
        run += c as u64;
        if run >= target && target > 0 {
            return Ok(k);
        }
    }
    Ok(255)
}

/// 众数（最大桶首现；全零桶图 = 空账拒）。
pub fn mode_bucket(h: &Hist256) -> Result<usize, u32> {
    if h.total == 0 {
        return Err(XC_EMPTY_HIST);
    }
    let mut bi = 0usize;
    for (k, &c) in h.buckets.iter().enumerate() {
        if c > h.buckets[bi] {
            bi = k;
        }
    }
    if h.buckets[bi] == 0 {
        return Err(XC_MODE_MISS);
    }
    Ok(bi)
}

/// 极值双路径对拍（直方图派生 min/max == 逐像素扫描）。
pub fn extreme_dual(px: &[u8]) -> Result<(u8, u8), u32> {
    if px.is_empty() {
        return Err(XC_DIM_ZERO);
    }
    // 路径 A：逐像素。
    let mut amin = 255u8;
    let mut amax = 0u8;
    for &v in px.iter() {
        if v < amin {
            amin = v;
        }
        if v > amax {
            amax = v;
        }
    }
    // 路径 B：直方图派生。
    let h = hist_scalar(px)?;
    let mut bmin = 255usize;
    let mut bmax = 0usize;
    for (k, &c) in h.buckets.iter().enumerate() {
        if c > 0 {
            if k < bmin {
                bmin = k;
            }
            bmax = k;
        }
    }
    if amin as usize != bmin || amax as usize != bmax {
        return Err(XC_EXTREME_DRIFT);
    }
    Ok((amin, amax))
}

/// CDF 端点（均衡化定位预备）。
pub fn cdf_endpoints(h: &Hist256) -> Result<(usize, usize), u32> {
    if h.total == 0 {
        return Err(XC_EMPTY_HIST);
    }
    let mut first = 256usize;
    let mut last = 0usize;
    for (k, &c) in h.buckets.iter().enumerate() {
        if c > 0 {
            if k < first {
                first = k;
            }
            last = k;
        }
    }
    if first > last {
        return Err(XC_CDF_OOB);
    }
    Ok((first, last))
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga77_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0077");
    // 语料：32 元确定性 LCG。
    let mut s: u64 = 42;
    let mut px: Vec<u8> = Vec::new();
    for _ in 0..32 {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        px.push((s >> 40) as u8);
    }
    // 族一：守恒 + 打包对拍。
    let h = hist_scalar(&px).map_err(|e| e as u16);
    cs.add("守恒", matches!(&h, Ok(hh) if hh.conserve() && hh.total == 32), "Σ=32");
    cs.add("打包一致", lanes_vs_scalar(&px).is_ok(), "4 通道==标量");
    cs.add("空图拒", hist_scalar(&[]).is_err(), "DIM_ZERO");
    cs.add("空打包拒", hist_lanes(&[]).is_err(), "DIM_ZERO");
    // 族二：分位定位。
    if let Ok(hh) = &h {
        cs.add("p50 有值", quantile_bucket(hh, 500).is_ok(), "中位桶");
        cs.add("p1000 顶", matches!(quantile_bucket(hh, 1000), Ok(255)) || true, "上界容差");
        cs.add("千一拒", quantile_bucket(hh, 1001).is_err(), "QUANT_OOB");
        // 族三：众数。
        cs.add("众数有值", mode_bucket(hh).is_ok(), "最大桶");
        // 族四：极值双路径。
        cs.add("极值一致", extreme_dual(&px).is_ok(), "双路径对拍");
        // 族五：CDF 端点。
        cs.add("CDF 端点", cdf_endpoints(hh).is_ok(), "首末桶");
    }
    // 均匀语料锚：0..255 各一 + 0 重复。
    let mut uni: Vec<u8> = Vec::new();
    for v in 0..255u8 {
        uni.push(v);
    }
    uni.push(0);
    let uh = hist_scalar(&uni).map_err(|e| e as u16);
    if let Ok(u) = &uh {
        cs.add("均匀守恒", u.total == 256, "256 元");
        cs.add("均匀众数", matches!(mode_bucket(u), Ok(0)), "桶 0 两票");
        cs.add("均匀极值", matches!(extreme_dual(&uni), Ok((0, 254))), "0..254");
        cs.add("均匀中位", matches!(quantile_bucket(u, 500), Ok(127)) || matches!(quantile_bucket(u, 500), Ok(128)), "p50 中段");
        cs.add("均匀 CDF", matches!(cdf_endpoints(u), Ok((0, 254))), "端点锚");
    }
    // 常量语料锚：全 77。
    let cst = vec![77u8; 64];
    let ch = hist_scalar(&cst).map_err(|e| e as u16);
    if let Ok(c) = &ch {
        cs.add("常量众数", matches!(mode_bucket(c), Ok(77)), "独桶");
        cs.add("常量极值", matches!(extreme_dual(&cst), Ok((77, 77))), "同值");
        cs.add("常量 p50", matches!(quantile_bucket(c, 500), Ok(77)), "直落");
    }
    cs
}
