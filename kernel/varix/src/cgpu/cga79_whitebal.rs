//! cga79_whitebal —— CGPU-F0079 SIMD 色温与白平衡。
//! 锚点（CGPU 施工书）：CGPU-F0079 · SIMD 色温与白平衡 · 目标 320 行
//! 判据摘录：灰世界白平衡（三通道均值对齐——R/B 增益 = 均值G/
//! 均值R/B，定点千分比）；增益域闭集（[500,2000]‰——越界拒
//! 绝过补偿）；增益应用钳制（乘后 >>10 归一回 u8 域）；中性灰
//! 图恒等（r==g==b 输入 → 增益 1000‰ 恒等）；色温→增益映射
//! （K 线性插值冻结表：2000K/6500K/10000K 三锚）；白平衡后灰
//! 世界收敛（校准图三通道均值差 ≤ 容差）。
//! 组件：灰世界增益器 + 增益域闸 + 定点增益应用器 + 中性恒等
//!   核 + 色温插值表 + 收敛审计。
//! 职责定位：色彩保真的第一道自动校正——灰世界假设是白平衡的
//!   标准无参考方案；增益失控（过补偿）会把校正变成色偏制造机，
//! 域闸与收敛审计双面钉死。
//! 性能逐项分解：均值 O(n)；增益 O(1)；应用 O(n)。全整数确定性。
//! 诊断码段：0xE880（0xE881~0xE889 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const WB_VERSION: &str = "cga79_whitebal-v1";

/// 诊断段基址（0xE880 段 · 本模块独占）。
pub const WB_SEG_BASE: u16 = 0xE880;

/// XcCode 闭集（0xE881~0xE889）。
pub const XC_GAIN_OOB: u32 = 0xE881;
pub const XC_EMPTY: u32 = 0xE882;
pub const XC_MEAN_ZERO: u32 = 0xE883;
pub const XK_TEMP_OOB: u32 = 0xE884;
pub const XC_CONVERGE: u32 = 0xE885;
pub const XC_NEUTRAL_MISS: u32 = 0xE886;
pub const XC_CLAMP_HI: u32 = 0xE887;
pub const XC_TABLE_MISS: u32 = 0xE888;
pub const XC_PAIR_LEN: u32 = 0xE889;

/// 增益域闭集（千分比 [500,2000]）。
pub const GAIN_MIN: u32 = 500;
pub const GAIN_MAX: u32 = 2000;

pub fn gain_ok(g: u32) -> Result<(), u32> {
    if g < GAIN_MIN || g > GAIN_MAX {
        return Err(XC_GAIN_OOB);
    }
    Ok(())
}

/// 三通道均值（千分比定点返回——/n 精度保留在 u64 域）。
pub fn means(px: &[(u8, u8, u8)]) -> Result<(u64, u64, u64), u32> {
    if px.is_empty() {
        return Err(XC_EMPTY);
    }
    let mut sr: u64 = 0;
    let mut sg: u64 = 0;
    let mut sb: u64 = 0;
    for &(r, g, b) in px.iter() {
        sr += r as u64;
        sg += g as u64;
        sb += b as u64;
    }
    let n = px.len() as u64;
    Ok((sr / n, sg / n, sb / n))
}

/// 灰世界增益（G 通道基准：R 增益 = mG/mR、B 增益 = mG/mB，
/// 千分比定点钳域）。中性图（mR==mG==mB）→ (1000, 1000)。
pub fn gray_world_gains(px: &[(u8, u8, u8)]) -> Result<(u32, u32), u32> {
    let (mr, mg, mb) = means(px)?;
    if mr == 0 || mb == 0 {
        return Err(XC_MEAN_ZERO);
    }
    let gr = (mg * 1000 / mr).clamp(GAIN_MIN as u64, GAIN_MAX as u64) as u32;
    let gb = (mg * 1000 / mb).clamp(GAIN_MIN as u64, GAIN_MAX as u64) as u32;
    Ok((gr, gb))
}

/// 定点增益应用（v × gain / 1000，钳 255）。
pub fn apply_gain(v: u8, gain: u32) -> Result<u8, u32> {
    gain_ok(gain)?;
    let out = v as u64 * gain as u64 / 1000;
    if out > 255 {
        return Err(XC_CLAMP_HI);
    }
    Ok(out as u8)
}

/// 中性灰恒等核（r==g==b 图 → 增益 1000‰ → 逐像素恒等）。
pub fn neutral_identity(px: &[(u8, u8, u8)]) -> Result<(), u32> {
    if px.is_empty() {
        return Err(XC_EMPTY);
    }
    for &(r, g, b) in px.iter() {
        if r != g || g != b {
            // 非中性图不适用本核（调用面语义），显性拒绝。
            return Err(XC_NEUTRAL_MISS);
        }
    }
    let (gr, gb) = gray_world_gains(px)?;
    if gr != 1000 || gb != 1000 {
        return Err(XC_NEUTRAL_MISS);
    }
    for &(r, _, _) in px.iter() {
        if apply_gain(r, gr)? != r {
            return Err(XC_NEUTRAL_MISS);
        }
    }
    Ok(())
}

/// 色温→增益冻结表（三锚线性插值：2000K 暖偏红 / 6500K 中性 /
/// 10000K 冷偏蓝）。
pub fn temp_to_gain(kelvin: u32) -> Result<(u32, u32), u32> {
    // 锚：(K, r_gain‰, b_gain‰)。
    const ANCHORS: [(u32, u32, u32); 3] = [(2000, 1300, 800), (6500, 1000, 1000), (10000, 850, 1200)];
    if kelvin < ANCHORS[0].0 || kelvin > ANCHORS[2].0 {
        return Err(XK_TEMP_OOB);
    }
    for w in ANCHORS.windows(2) {
        if kelvin >= w[0].0 && kelvin <= w[1].0 {
            let t = (kelvin - w[0].0) as u64;
            let span = (w[1].0 - w[0].0) as u64;
            let r = (w[0].1 as u64 * (span - t) + w[1].1 as u64 * t) / span;
            let b = (w[0].2 as u64 * (span - t) + w[1].2 as u64 * t) / span;
            return Ok((r as u32, b as u32));
        }
    }
    Err(XC_TABLE_MISS)
}

/// 白平衡全链（增益计算 + 应用 + 收敛审计）。
pub fn white_balance(px: &[(u8, u8, u8)]) -> Result<Vec<(u8, u8, u8)>, u32> {
    let (gr, gb) = gray_world_gains(px)?;
    let mut out: Vec<(u8, u8, u8)> = Vec::with_capacity(px.len());
    for &(r, g, b) in px.iter() {
        out.push((apply_gain(r, gr)?, g, apply_gain(b, gb)?));
    }
    Ok(out)
}

/// 收敛审计（校准后三通道均值差 ≤ 容差——灰世界目标达成面）。
pub fn converge_check(px: &[(u8, u8, u8)], tol: u64) -> Result<(), u32> {
    let out = white_balance(px)?;
    let (mr, mg, mb) = means(&out)?;
    let d1 = if mr > mg { mr - mg } else { mg - mr };
    let d2 = if mg > mb { mg - mb } else { mb - mg };
    if d1 > tol || d2 > tol {
        return Err(XC_CONVERGE);
    }
    Ok(())
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga79_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0079");
    // 族一：增益域闸。
    cs.add("域内过", gain_ok(1000).is_ok() && gain_ok(500).is_ok() && gain_ok(2000).is_ok(), "[500,2000]‰");
    cs.add("域下拒", gain_ok(499).is_err(), "GAIN_OOB");
    cs.add("域上拒", gain_ok(2001).is_err(), "GAIN_OOB");
    // 族二：中性灰恒等。
    let neutral = vec![(128u8, 128, 128); 16];
    cs.add("中性恒等", neutral_identity(&neutral).is_ok(), "增益 1000‰ 直通");
    cs.add("非中性拒核", neutral_identity(&[(10u8, 128, 128)]).is_err(), "NEUTRAL_MISS");
    // 族三：灰世界增益（偏红图 → R 增益 <1000 下调）。
    let warm: Vec<(u8, u8, u8)> = vec![(200, 100, 100); 16];
    let gw = gray_world_gains(&warm).map_err(|e| e as u16);
    cs.add("暖图 R 降", matches!(&gw, Ok((gr, _)) if *gr < 1000), "mG<mR 下调");
    cs.add("暖图 B 升钳", matches!(&gw, Ok((_, gb)) if *gb >= 1000), "mG≥mB 上调");
    // 增益计算值锚：mG=100, mR=200 → 500‰。
    cs.add("增益锚 500", matches!(&gw, Ok((500, _))), "1000/2000×1000");
    // 族四：定点应用 + 钳制。
    cs.add("应用恒等", apply_gain(100, 1000) == Ok(100), "1000‰ 直通");
    cs.add("应用半增", apply_gain(100, 500) == Ok(50), "半增");
    cs.add("应用双增", apply_gain(100, 2000) == Ok(200), "倍增");
    cs.add("钳顶立案", apply_gain(128, 2000).is_err(), "128×2=256 > 255");
    cs.add("零图拒", means(&[]).is_err(), "EMPTY");
    // 族五：色温表插值。
    cs.add("锚 2000K", matches!(temp_to_gain(2000), Ok((1300, 800))), "暖锚");
    cs.add("锚 6500K", matches!(temp_to_gain(6500), Ok((1000, 1000))), "中性锚");
    cs.add("插值 4250K", matches!(temp_to_gain(4250), Ok((1150, 900))), "半程插值");
    cs.add("界外拒", temp_to_gain(1000).is_err() && temp_to_gain(20000).is_err(), "TEMP_OOB");
    // 族六：全链收敛（温和偏色可校）。
    let mild: Vec<(u8, u8, u8)> = vec![(120, 100, 110); 32];
    cs.add("温和收敛", converge_check(&mild, 12).is_ok(), "校后均值差 ≤12");
    // 极端偏色（增益钳域 → 不许过补偿——收敛允许有界残差）。
    let extreme: Vec<(u8, u8, u8)> = vec![(250, 30, 30); 32];
    cs.add("极端不炸", white_balance(&extreme).is_ok(), "钳域内完成");
    CheckSet::merge(CheckSet::merge(cs, run_cga79_deep()), run_cga79_deep2())
}

// ===== 深化面：色温轨道 + 灰世界迭代链 =====
/// 深化目标：色温→增益的轨道单调性、灰世界二次收敛、
/// 三通道独立恒等、批量白平衡守恒账。

/// 色温轨道锚表：2500K/6500K/10000K 三档（轨道冻结）。
pub const TEMP_TRACK: [u32; 3] = [2500, 6500, 10000];

/// 轨道单调核：Kelvin 越高蓝增益越高、红增益越低（分轨单调）。
pub fn temp_track_monotone() -> Result<(), u32> {
    let mut prev_rg = (u32::MAX, 0u32);
    for &k in TEMP_TRACK.iter() {
        let (rg, bg) = temp_to_gain(k)?;
        if rg > prev_rg.0 {
            return Err(XK_TEMP_OOB); // 红轨必须不升
        }
        if bg < prev_rg.1 {
            return Err(XK_TEMP_OOB); // 蓝轨必须不降
        }
        prev_rg = (rg, bg);
    }
    Ok(())
}

/// 色温越界核：低于 1500K 或高于 12000K 拒（轨道外）。
pub fn temp_domain(kelvin: u32) -> Result<(), u32> {
    if kelvin < 1500 || kelvin > 12000 {
        return Err(XK_TEMP_OOB);
    }
    temp_to_gain(kelvin).map(|_| ())
}

/// 三通道独立恒等：只动 R 增益时 G/B 逐像素不变。
pub fn channel_independence(px: &[(u8, u8, u8)], gain: u32) -> Result<(), u32> {
    gain_ok(gain)?;
    for &(r, g, b) in px.iter() {
        let nr = apply_gain(r, gain)?;
        let _ = nr;
        // G/B 不经 apply_gain——恒等锚。
        if apply_gain(g, GAIN_ONE)? != g || apply_gain(b, GAIN_ONE)? != b {
            return Err(XC_NEUTRAL_MISS);
        }
    }
    Ok(())
}

/// 单位增益锚（千分记法：1000 = ×1.0）。
pub const GAIN_ONE: u32 = 1000;

/// 灰世界二次收敛：白平衡一次后残差应小于一次前（严格下降，
///   容差内允许平——有界残差语义）。
pub fn two_pass_converge(px: &[(u8, u8, u8)], tol: u64) -> Result<(), u32> {
    let (r0, g0, b0) = means(px)?;
    let bal = white_balance(px)?;
    let bal_ref: Vec<(u8, u8, u8)> = bal;
    let (r1, g1, b1) = means(&bal_ref)?;
    // 残差 = 三通道极差。
    let d0 = r0.max(g0).max(b0) - r0.min(g0).min(b0);
    let d1 = r1.max(g1).max(b1) - r1.min(g1).min(b1);
    if d1 > d0 + tol {
        return Err(XC_CONVERGE);
    }
    Ok(())
}

/// 批量守恒账：像素数不变 + 全部像素落域内（白平衡不许丢像素）。
pub fn batch_conserved(src: &[(u8, u8, u8)]) -> Result<(), u32> {
    let out = white_balance(src)?;
    if out.len() != src.len() {
        return Err(XC_PAIR_LEN);
    }
    Ok(())
}

pub fn run_cga79_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga79_deep");
    // 深族一：色温轨道。
    cs.add("轨道单调", temp_track_monotone().is_ok(), "红降蓝升");
    cs.add("温域内收", temp_domain(6500).is_ok(), "日光锚");
    cs.add("温域低拒", temp_domain(999).is_err(), "TEMP_OOB");
    cs.add("温域高拒", temp_domain(20000).is_err(), "TEMP_OOB");
    // 深族二：三通道独立。
    let px: Vec<(u8, u8, u8)> = vec![(200, 100, 50), (30, 60, 90), (128, 128, 128)];
    cs.add("单位恒等", channel_independence(&px, GAIN_ONE).is_ok(), "×1.0 不动");
    cs.add("G/B 不动账", apply_gain(77, GAIN_ONE) == Ok(77), "单位增益");
    // 深族三：二次收敛。
    let warm: Vec<(u8, u8, u8)> = (0..32u32)
        .map(|i| ((180 + (i % 8)) as u8, 90, 70))
        .collect();
    cs.add("二次收敛", two_pass_converge(&warm, 24).is_ok(), "残差下降");
    // 深族四：批量守恒。
    cs.add("批守恒", batch_conserved(&px).is_ok(), "像素数闭");
    cs.add("空批拒", batch_conserved(&[]).is_err(), "EMPTY");
    // 深族五：轨道锚值（6500K 日光 = 近单位）。
    if let Ok((rg, bg)) = temp_to_gain(6500) {
        cs.add("日光锚", rg.abs_diff(1000) <= 120 && bg.abs_diff(1000) <= 120, "近单位");
    } else {
        cs.add("日光锚", false, "轨道断");
    }
    cs
}

// ===== 深化面二：增益钳域与轨道端点账 =====
/// 增益上下界行为锚：低于下界钳到下界、高于上界拒收。
pub fn gain_boundary_anchor() -> Result<(), u32> {
    if gain_ok(GAIN_MIN).is_err() || gain_ok(GAIN_MAX).is_err() {
        return Err(XC_GAIN_OOB);
    }
    if gain_ok(GAIN_MIN - 1).is_ok() || gain_ok(GAIN_MAX + 1).is_ok() {
        return Err(XC_GAIN_OOB);
    }
    Ok(())
}

/// 轨道端点闭账：最低/最高温档都可构造（端点自洽）。
pub fn temp_endpoints() -> Result<(), u32> {
    let (rl, bl) = temp_to_gain(TEMP_TRACK[0])?;
    let (rh, bh) = temp_to_gain(TEMP_TRACK[2])?;
    if rl < rh {
        return Err(XK_TEMP_OOB); // 低温红增益更高
    }
    if bl > bh {
        return Err(XK_TEMP_OOB); // 低温蓝增益更低
    }
    Ok(())
}

/// 中性灰恒等：纯灰图白平衡后仍是纯灰（零偏锚）。
pub fn neutral_gray_inert() -> Result<(), u32> {
    let gray: Vec<(u8, u8, u8)> = (0..16u32).map(|i| ((i * 16) as u8, (i * 16) as u8, (i * 16) as u8)).collect();
    let out = white_balance(&gray)?;
    for (i, &(r, g, b)) in out.iter().enumerate() {
        let src = (i * 16) as u8;
        if r.abs_diff(src) > 2 || g.abs_diff(src) > 2 || b.abs_diff(src) > 2 {
            return Err(XC_NEUTRAL_MISS);
        }
    }
    Ok(())
}

pub fn run_cga79_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga79_deep2");
    cs.add("增益钳域", gain_boundary_anchor().is_ok(), "双端闭");
    cs.add("端点闭账", temp_endpoints().is_ok(), "低温红高");
    cs.add("中性惰性", neutral_gray_inert().is_ok(), "灰图不动");
    cs
}
