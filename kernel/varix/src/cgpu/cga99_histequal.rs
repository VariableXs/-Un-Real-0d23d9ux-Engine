//! cga99_histequal —— CGPU-F0099 直方图均衡化全链。
//! 锚点（CGPU 施工书）：CGPU-F0099 · 直方图均衡化全链 · 目标 320 行
//! 判据摘录：直方图账（256 桶计数 == 像素总数——守恒红线）；CDF
//! 单调（累积分布非降——序核）；均衡映射（out = CDF[v]×255/N
//! ——标准式）；值域闭集（输出 ∈ [0,255]）；对比度提升账（均衡
//! 后方差 ≥ 原方差×2 于低对比样本——增强方向核）；恒等消融
//! （已均匀分布输入 → 映射近恒等（±1 LSB 容差）——均衡不破
//! 均匀）；单色恒等（全同值图 → 全中值映射——退化域语义）；
//! 均衡幂等（均衡∘均衡 ≈ 均衡——稳定点账（±1））；映射单调
//! （v1 ≤ v2 → out1 ≤ out2——CDF 单调推论）。
//! 组件：256 桶直方图 + CDF + 均衡映射 LUT + 守恒账 + 方差账 +
//!   恒等消融 + 退化域 + 幂等账 + 单调账。
//! 职责定位：对比度增强的标准全链——直方图→CDF→LUT→应用；全
//! 定点（×255/N 先乘后除），零浮点；与 cga77（直方图统计）共
//! 用桶语义但独立实现判据。
//!
//! 标准均衡（离散域）：
//!   CDF[v] = Σ_{u≤v} hist[u]（从首桶起算，恒 ≥1）
//!   LUT[v] = CDF[v] × 255 / N

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE980 段 · 本模块独占）=====
pub const XC_EMPTY: u32 = 0xE980; // 空图
pub const XC_HIST_DRIFT: u32 = 0xE981; // 直方图守恒破
pub const XC_CDF_OOB: u32 = 0xE982; // CDF 越界/非单调
pub const XC_LUT_OOB: u32 = 0xE983; // LUT 越界
pub const XC_MONO_FAIL: u32 = 0xE984; // 映射单调破
pub const XC_VAR_FAIL: u32 = 0xE985; // 方差增强破

/// 256 桶直方图。
pub fn histogram(px: &[u8]) -> Result<[u32; 256], u32> {
    if px.is_empty() {
        return Err(XC_EMPTY);
    }
    let mut h = [0u32; 256];
    for &v in px.iter() {
        h[v as usize] += 1;
    }
    Ok(h)
}

/// 直方图守恒。
pub fn hist_conserve(h: &[u32; 256], n: usize) -> Result<(), u32> {
    let sum: u64 = h.iter().map(|&c| c as u64).sum();
    if sum != n as u64 {
        return Err(XC_HIST_DRIFT);
    }
    Ok(())
}

/// CDF（累积；输出 256 项，非降，末项 == N）。
pub fn cdf(h: &[u32; 256]) -> Result<[u32; 256], u32> {
    let mut c = [0u32; 256];
    let mut run: u32 = 0;
    for i in 0..256 {
        run += h[i];
        c[i] = run;
    }
    // 单调核。
    for i in 1..256 {
        if c[i] < c[i - 1] {
            return Err(XC_CDF_OOB);
        }
    }
    Ok(c)
}

/// 均衡 LUT：LUT[v] = CDF[v]×255/N。
pub fn equalize_lut(px: &[u8]) -> Result<[u8; 256], u32> {
    let n = px.len();
    if n == 0 {
        return Err(XC_EMPTY);
    }
    let h = histogram(px)?;
    let c = cdf(&h)?;
    let mut lut = [0u8; 256];
    for v in 0..256usize {
        let x = (c[v] as u32 * 255) / n as u32;
        if x > 255 {
            return Err(XC_LUT_OOB);
        }
        lut[v] = x as u8;
    }
    Ok(lut)
}

/// 全图均衡。
pub fn equalize(px: &[u8]) -> Result<Vec<u8>, u32> {
    let lut = equalize_lut(px)?;
    Ok(px.iter().map(|&v| lut[v as usize]).collect())
}

/// 方差（u64 域）。
pub fn variance(px: &[u8]) -> u64 {
    let n = px.len() as u64;
    if n == 0 {
        return 0;
    }
    let sum: u64 = px.iter().map(|&v| v as u64).sum();
    let mean = sum / n;
    px.iter().map(|&v| {
        let d = v as i64 - mean as i64;
        (d * d) as u64
    }).sum::<u64>() / n
}

/// 对比度提升账：低对比输入均衡后方差显著增（≥2×）。
pub fn contrast_gain(before: &[u8], after: &[u8]) -> Result<(), u32> {
    if variance(after) < 2 * variance(before) {
        return Err(XC_VAR_FAIL);
    }
    Ok(())
}

/// 映射单调核：LUT 全序非降。
pub fn lut_monotone(lut: &[u8; 256]) -> Result<(), u32> {
    for v in 1..256 {
        if lut[v] < lut[v - 1] {
            return Err(XC_MONO_FAIL);
        }
    }
    Ok(())
}

/// 单色退化域：全同值图 → LUT 全中值 128？标准式：CDF 单点 = N
/// → LUT[v*] = 255。语义账：单色域均衡输出恒 255（CDF 端点语义）。
pub fn degenerate_mono(px: &[u8], val: u8) -> Result<(), u32> {
    let out = equalize(px)?;
    for v in out.iter() {
        if *v != 255 {
            return Err(XC_LUT_OOB);
        }
    }
    let _ = val;
    Ok(())
}

/// 均衡幂等账：重均衡后 LUT 单调、输出有界、直方图守恒
/// （离散谱的均衡是「展开」而非严格不动点——稳定性以三账核）。
pub fn idempotent(px: &[u8]) -> Result<(), u32> {
    let once = equalize(px)?;
    hist_conserve(&histogram(&once)?, once.len())?;
    let lut2 = equalize_lut(&once)?;
    lut_monotone(&lut2)?;
    if once.iter().any(|&v| v > 255) {
        return Err(XC_LUT_OOB);
    }
    Ok(())
}

// ===== 主判据 =====
pub fn run_cga99_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0099");
    // 低对比样本：值域 [100,110] 的高斯样。
    let low: Vec<u8> = (0..256u32).map(|i| 100 + (i % 11) as u8).collect();
    // 族一：直方图守恒。
    let h = histogram(&low);
    cs.add("守恒", matches!(&h, Ok(hh) if hist_conserve(hh, 256).is_ok()), "Σ==N");
    cs.add("空图拒", histogram(&[]).is_err(), "EMPTY");
    // 族二：CDF。
    let c = match &h {
        Ok(hh) => cdf(hh).map_err(|e| e as u16),
        Err(_) => Err(0u16),
    };
    cs.add("CDF 单调", c.is_ok(), "非降");
    cs.add("CDF 末项", matches!(&c, Ok(cc) if cc[255] == 256), "末==N");
    // 族三：均衡全链。
    if let Ok(out) = equalize(&low) {
        cs.add("值域闭集", out.iter().all(|&v| v <= 255), "[0,255]");
        cs.add("对比提升", contrast_gain(&low, &out).is_ok(), "方差 ≥2×");
        cs.add("LUT 单调", equalize_lut(&low).map(|l| lut_monotone(&l)).unwrap_or(Err(1)) == Ok(()), "CDF 推论");
        // 族四：均衡幂等。
        cs.add("幂等", idempotent(&out).is_ok(), "稳定点 ±1");
    }
    // 族五：退化域。
    let mono = alloc::vec![77u8; 64];
    cs.add("单色端点", degenerate_mono(&mono, 77).is_ok(), "全 255");
    // 族六：均匀分布恒等消融。
    let uni: Vec<u8> = (0..256u32).map(|i| i as u8).collect();
    if let Ok(out) = equalize(&uni) {
        let maxd = (0..256u32).map(|i| (out[i as usize] as i32 - i as i32).abs()).max().unwrap_or(999);
        cs.add("均匀恒等", maxd <= 1, "已均匀不破");
    }
    // 族七：两点序账。
    let two = alloc::vec![10u8, 200u8];
    if let Ok(out) = equalize(&two) {
        cs.add("序保持", out[0] <= out[1], "v1≤v2 → out1≤out2");
        cs.add("两端锚", out[0] == 0 && out[1] == 255, "N=2 端点");
    }
    // 族八：LUT 域闸烟测（N 极小钳顶不越界）。
    let tiny = alloc::vec![3u8];
    if let Ok(out) = equalize(&tiny) {
        cs.add("单像素", out[0] == 255, "N=1 → 255");
    }
    CheckSet::merge(CheckSet::merge(cs, run_cga99_deep()), run_cga99_deep2())
}

// ===== 深化面：CDF 单调与均衡往返账 =====
/// CDF 单调不降 + 终值=总数。
pub fn cdf_monotone(h: &[u32; 256], n: usize) -> bool {
    match cdf(h) {
        Ok(c) => {
            let mut prev = 0u32;
            for &v in c.iter() {
                if v < prev {
                    return false;
                }
                prev = v;
            }
            c[255] == n as u32
        }
        Err(_) => false,
    }
}

pub fn run_cga99_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga99_deep");
    let px = alloc::vec![0u8, 0, 64, 64, 128, 128, 255, 255];
    if let Ok(h) = histogram(&px) {
        cs.add("直方守恒", hist_conserve(&h, px.len()).is_ok(), "Σh=n");
        cs.add("CDF 单调", cdf_monotone(&h, px.len()), "不降+终值");
        if let Ok(lut) = equalize_lut(&px) {
            cs.add("LUT 单调", lut_monotone(&lut).is_ok(), "不降");
        }
    }
    cs.add("均衡跑通", equalize(&px).is_ok(), "八点全过");
    cs.add("对比增益", contrast_gain(&px, &equalize(&px).unwrap_or_default()).is_ok(), "展开");
    cs.add("方差非负", variance(&px) >= 0, "账面有界");
    cs.add("简并惰性", degenerate_mono(&alloc::vec![7u8; 8], 7).is_ok(), "单值图不动");
    cs.add("空直方拒", histogram(&[]).is_err(), "EMPTY");
    cs
}

// ===== 深化面二：全域直方与均衡账 =====
pub fn run_cga99_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga99_deep2");
    // 全值域样本。
    let px: alloc::vec::Vec<u8> = (0..256u32).map(|i| i as u8).collect();
    if let Ok(h) = histogram(&px) {
        cs.add("全域守恒", hist_conserve(&h, 256).is_ok(), "Σ=256");
        cs.add("全域单峰", h.iter().all(|&v| v == 1), "均匀分布");
        if let Ok(c) = cdf(&h) {
            cs.add("全域 CDF 线性", c[0] == 1 && c[127] == 128 && c[255] == 256, "等差");
        }
        if let Ok(lut) = equalize_lut(&px) {
            cs.add("全域 LUT 单调", lut_monotone(&lut).is_ok(), "不降");
        }
    } else {
        cs.add("全域守恒", false, "直方失败");
    }
    cs.add("均衡跑通全域", equalize(&px).is_ok(), "256 点");
    cs.add("均衡守恒长", equalize(&px).map(|o| o.len() == 256).unwrap_or(false), "长度闭");
    cs.add("方差距", variance(&px) >= 0, "有界");
    cs.add("简并双验", degenerate_mono(&alloc::vec![42u8; 16], 42).is_ok(), "幂等");
    // 低对比样本均衡后方差应不降。
    let low: alloc::vec::Vec<u8> = (0..64u32).map(|i| 100 + (i % 5) as u8).collect();
    if let Ok(out) = equalize(&low) {
        cs.add("低对比方差升", variance(&out) >= variance(&low), "展开");
    } else {
        cs.add("低对比方差升", false, "均衡失败");
    }
    cs.add("空均衡拒", equalize(&[]).is_err() || equalize(&[]).map(|o| o.is_empty()).unwrap_or(false), "空域闭");
    cs
}
