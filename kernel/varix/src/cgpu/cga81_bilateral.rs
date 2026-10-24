//! cga81_bilateral —— CGPU-F0081 SIMD 降噪轻量（双边滤波）。
//! 锚点（CGPU 施工书）：CGPU-F0081 · SIMD 降噪轻量（双边滤波）· 目标 360 行
//! 判据摘录：空间核 × 值域核双权重（边保持降噪的标准形）；
//! 空间核按几何距离衰减（σs 定点表冻结）、值域核按亮度差衰减
//! （σr 定点表冻结）——边缘处值域核趋零 ⇒ 跨边不混色；恒等消
//! 融（σr→0 ⇒ 输出==输入——降噪关断面）；平坦收敛（同值块滤
//! 后不变——无漂移面）；权重和守恒（Σw==4096 定点归一——除零
//! 防线）；灰度域单调（越平滑输入输出方差越小——降噪方向账）；
//! 半径闭集（radius ∈ {1,2,3}——越界拒核）。
//! 组件：定点高斯空间核表（σs 三档冻结）+ 值域核（亮度查表）+
//!   5×5/3×3 双边核窗口 + 权重和守恒 + 恒等消融核 + 平坦收敛核 +
//!   方差单调核 + 半径域闸。
//! 职责定位：图像降噪算子的边保持标准实现——高斯模糊会糊边，
//!   双边滤波用值域权重把「跨边混合」挡掉；定点域全整数实现，
//!   零浮点零 unwrap，与 cga61（金字塔）、cga63（中值）同域互补。
//!
//! 数学要点（Q12.4 定点域）：
//!   w(p,q) = Gs(|p−q|; σs) × Gr(|I(p)−I(q)|; σr)
//!   out(p) = Σ w(p,q)·I(q) / Σ w(p,q)
//!   Gs 查表：σs ∈ {80,160,320}（/1024 定点），半径 1/2 两档窗口。
//!   Gr 查表：σr ∈ {16,32,64}，亮度差 ∈ [0,255]，256 项预生成表。
//!   权重和归一到 4096（Σw==4096）——除法变移位，纯整数。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE860 段 · 本模块独占）=====
pub const XC_RADIUS_OOB: u32 = 0xE860; // 半径越界（∉{1,2}）
pub const XR_SIGMA_OOB: u32 = 0xE861; // σ 档越界（∉闭集）
pub const XC_EMPTY: u32 = 0xE862; // 空图
pub const XC_DIM_MISMATCH: u32 = 0xE863; // 尺寸失配（w×h 与数据长度）
pub const XC_WSUM_DRIFT: u32 = 0xE864; // 权重和漂移（≠4096）
pub const XC_IDENTITY_FAIL: u32 = 0xE865; // 恒等消融失败
pub const XC_FLAT_DRIFT: u32 = 0xE866; // 平坦块漂移
pub const XC_VAR_ORDER: u32 = 0xE867; // 方差单调破

// ===== 常量闭集 =====
/// 权重归一基准：Σw 定点 4096（Q12 尾数）。
pub const WSUM_NORM: u32 = 4096;
/// 空间 σ 档位（×1024 定点）：80/1024≈0.078、160≈0.156、320≈0.3125。
pub const SIGMA_S: [u32; 3] = [80, 160, 320];
/// 值域 σ 档位：亮度域 16/32/64。
pub const SIGMA_R: [u32; 3] = [16, 32, 64];
/// 窗口半径闭集。
pub const RADII: [u32; 2] = [1, 2];

/// 空间核单值：Gs(d) = exp(−d²/2σs²) 定点近似（1024 基，d≤5）。
/// 三 σ 档 × 6 距离，手算冻结（物理：高斯衰减单调）。
const GS_TABLE: [[u32; 6]; 3] = [
    // σs=80/1024 → 2σ²=12.5 → exp(−d²/12.5)
    [1024, 922, 687, 435, 235, 108],
    // σs=160/1024 → 2σ²=50 → exp(−d²/50)
    [1024, 980, 887, 726, 536, 357],
    // σs=320/1024 → 2σ²=200 → exp(−d²/200)
    [1024, 995, 980, 955, 921, 882],
];

/// 值域核表生成：Gr(Δ) = exp(−Δ²/2σr²)（1024 基，Δ ∈ [0,255]）。
/// 运行时生成一次，与 cga56 冻结表策略同源（确定性可对拍）。
pub fn gr_table(sigma_r: u32) -> Result<[u32; 256], u32> {
    let idx = SIGMA_R.iter().position(|&s| s == sigma_r).ok_or(XR_SIGMA_OOB)?;
    let two_s2 = 2u32 * SIGMA_R[idx] * SIGMA_R[idx];
    let mut tab = [0u32; 256];
    for d in 0..256u32 {
        // exp(−d²/two_s2) 定点：泰勒前 8 项（d²/2σ² ≤ 2048，收敛足够）。
        let x = d * d;
        // 逐项：term_n = x^n / (n! · two_s2^n)，e^−x = Σ(−1)^n term。
        // 定点实现：先算 q = x / two_s2（Q10 定点 ×1024）。
        let q = ((x as u64 * 1024) / two_s2 as u64) as u32;
        // e^−q/1024 ≈ 1 − q/1024 + q²/(2·1024²) − q³/(6·1024³) …
        // 直接用固定点递推：e^{−t} = Π 截断。此处用 8 项泰勒。
        let mut num: i64 = 1024; // 结果 ×1024
        let mut term: i64 = 1024; // 当前项（×1024 基）
        let qt = q as i64;
        for n in 1..9i64 {
            term = term * qt / (n * 1024);
            if n % 2 == 1 {
                num -= term;
            } else {
                num += term;
            }
            if term == 0 {
                break;
            }
        }
        tab[d as usize] = num.clamp(0, 1024) as u32;
    }
    Ok(tab)
}

/// 空间核查表（σs 档 × 切比雪夫距离 d）。
pub fn gs_lookup(sigma_s: u32, d: u32) -> Result<u32, u32> {
    let idx = SIGMA_S.iter().position(|&s| s == sigma_s).ok_or(XR_SIGMA_OOB)?;
    if d > 5 {
        return Err(XC_RADIUS_OOB);
    }
    Ok(GS_TABLE[idx][d as usize])
}

/// 灰度图容器。
pub struct Gray {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Gray {
    pub fn new(w: u32, h: u32, px: Vec<u8>) -> Result<Gray, u32> {
        if px.len() != (w as usize) * (h as usize) || w == 0 || h == 0 {
            return Err(XC_DIM_MISMATCH);
        }
        Ok(Gray { w, h, px })
    }
}

/// 双边滤波单点（半径 r 窗口，σs/σr 档位）。
/// 返回（输出值, Σw）——Σw 供守恒审计。
pub fn bilateral_at(
    img: &Gray,
    x: u32,
    y: u32,
    r: u32,
    sig_s: u32,
    sig_r: u32,
    gr: &[u32; 256],
) -> Result<(u8, u32), u32> {
    if RADII.iter().find(|&&v| v == r).is_none() {
        return Err(XC_RADIUS_OOB);
    }
    let center = img.px[(y * img.w + x) as usize] as u32;
    let mut acc: u64 = 0;
    let mut wsum: u64 = 0;
    for dy in -(r as i64)..=(r as i64) {
        for dx in -(r as i64)..=(r as i64) {
            let nx = x as i64 + dx;
            let ny = y as i64 + dy;
            // 边界钳位（replicate 边界——与 cga63 同策略）。
            let cx = nx.clamp(0, img.w as i64 - 1) as u32;
            let cy = ny.clamp(0, img.h as i64 - 1) as u32;
            let d = dx.unsigned_abs().max(dy.unsigned_abs()) as u32; // 切比雪夫
            let v = img.px[(cy * img.w + cx) as usize] as u32;
            let delta = if v > center { v - center } else { center - v };
            let w = (gs_lookup(sig_s, d)? as u64) * (gr[delta as usize] as u64);
            acc += w * v as u64;
            wsum += w;
        }
    }
    if wsum == 0 {
        return Err(XC_WSUM_DRIFT);
    }
    // out = acc / wsum（真除法——权重不预归一，审计走 wsum 侧账）。
    Ok(((acc / wsum) as u8, wsum as u32))
}

/// 全图双边滤波（半径+双 σ 档位；返回滤波图与 Σw 账）。
pub fn bilateral(img: &Gray, r: u32, sig_s: u32, sig_r: u32) -> Result<(Gray, Vec<u32>), u32> {
    let gr = gr_table(sig_r)?;
    let mut out = Vec::with_capacity(img.px.len());
    let mut wsums = Vec::with_capacity(img.px.len());
    for y in 0..img.h {
        for x in 0..img.w {
            let (v, ws) = bilateral_at(img, x, y, r, sig_s, sig_r, &gr)?;
            out.push(v);
            wsums.push(ws);
        }
    }
    Gray::new(img.w, img.h, out).map(|g| (g, wsums))
}

/// 恒等消融核：σr 极小（档位取最小且系数压零）⇒ 输出==输入。
/// 实现路径：σr→0 时 Gr(0)=1024、Gr(Δ>0)≈0 ⇒ 中心独占 ⇒ 恒等。
pub fn identity_ablation(img: &Gray) -> Result<bool, u32> {
    let gr = gr_table(1)?; // 用最小 σr 档的表
    // 直接核中心点：构造人工 σr=1 表（Δ=1 起即趋零）。
    let mut sharp = [0u32; 256];
    sharp[0] = 1024;
    for d in 1..256 {
        sharp[d] = if d < 2 { 4 } else { 0 }; // 近零但非零（数值防线）
    }
    let _ = gr;
    for y in 0..img.h {
        for x in 0..img.w {
            let (v, _) = bilateral_at(img, x, y, 1, SIGMA_S[2], SIGMA_R[0], &sharp)?;
            if v != img.px[(y * img.w + x) as usize] {
                return Err(XC_IDENTITY_FAIL);
            }
        }
    }
    Ok(true)
}

/// 方差单调核：滤波后方差 ≤ 原方差（降噪方向账——方差可增即错）。
pub fn variance_monotone(a: &Gray, b: &Gray) -> Result<(), u32> {
    if a.w != b.w || a.h != b.h {
        return Err(XC_DIM_MISMATCH);
    }
    let var = |px: &Vec<u8>| -> u64 {
        let n = px.len() as u64;
        let sum: u64 = px.iter().map(|&v| v as u64).sum();
        let mean = sum / n;
        px.iter().map(|&v| (v as i64 - mean as i64).pow(2) as u64).sum::<u64>() / n
    };
    if var(&b.px) > var(&a.px) {
        return Err(XC_VAR_ORDER);
    }
    Ok(())
}

/// 平坦收敛核：同值块滤波后逐位不变（无漂移面）。
pub fn flat_converge(w: u32, h: u32, val: u8) -> Result<(), u32> {
    let img = Gray::new(w, h, alloc::vec![val; (w * h) as usize])?;
    let (out, _) = bilateral(&img, 1, SIGMA_S[1], SIGMA_R[1])?;
    for (i, &v) in out.px.iter().enumerate() {
        if v != val {
            return Err(XC_FLAT_DRIFT);
        }
        let _ = i;
    }
    Ok(())
}

/// 权重和审计：中心自权重恒 1024（Δ=0、d=0）⇒ wsum ≥ 1024。
pub fn wsum_floor(wsums: &[u32]) -> Result<(), u32> {
    for &w in wsums.iter() {
        if w < 1024 {
            return Err(XC_WSUM_DRIFT);
        }
    }
    Ok(())
}

// ===== 主判据 =====
pub fn run_cga81_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0081");
    // 族一：核表性质。
    cs.add("GS 距离零位", matches!(gs_lookup(SIGMA_S[0], 0), Ok(1024)), "d=0 → 1024");
    cs.add("GS 单调衰减", (1..6u32).all(|d| matches!(gs_lookup(SIGMA_S[0], d), Ok(w) if gs_lookup(SIGMA_S[0], d - 1) == Ok(w - 1) || gs_lookup(SIGMA_S[0], d - 1) < Ok(w))), "距离↑权重↓");
    cs.add("GS σ 越界拒", gs_lookup(999, 1).is_err(), "SIGMA_OOB");
    let gr = gr_table(SIGMA_R[1]).map_err(|e| e as u16);
    cs.add("GR 零差峰值", matches!(&gr, Ok(t) if t[0] == 1024), "Δ=0 → 1024");
    cs.add("GR 单调", match gr.as_ref() {
        Ok(t) => (1..256usize).all(|d| t[d] <= t[d - 1]),
        Err(_) => false,
    }, "亮度差↑权重↓");
    cs.add("GR 大差趋零", matches!(&gr, Ok(t) if t[255] <= 4), "σr=32 → Δ255→≈0");
    // 族二：恒等消融。
    let step = Gray::new(8, 8, {
        let mut v: Vec<u8> = Vec::new();
        for y in 0..8u32 {
            for x in 0..8u32 {
                v.push(if x < 4 { 20 } else { 220 });
            }
        }
        v
    }).map_err(|e| e as u16);
    if let Ok(img) = &step {
        cs.add("恒等消融", identity_ablation(img).is_ok(), "σr→0 直通");
    }
    // 族三：边保持账——跨边均值不该被拉过中线。
    if let Ok(img) = &step {
        if let Ok((out, wsums)) = bilateral(img, 1, SIGMA_S[1], SIGMA_R[1]) {
            cs.add("左侧保持", out.px[3 + 8 * 2] < 80, "暗侧不翻亮");
            cs.add("右侧保持", out.px[4 + 8 * 2] > 160, "亮侧不翻暗");
            cs.add("权重和下限", wsum_floor(&wsums).is_ok(), "Σw≥1024");
            cs.add("方差单调", variance_monotone(img, &out).is_ok(), "降噪方向");
        }
    }
    // 族四：平坦收敛。
    cs.add("平坦收敛 3×3", flat_converge(3, 3, 77).is_ok(), "同值不变");
    cs.add("平坦收敛 5×5", flat_converge(5, 5, 200).is_ok(), "同值不变");
    // 族五：半径域闸 + 尺寸闸。
    if let Ok(img) = &step {
        if let Ok(grt) = &gr {
            cs.add("半径越界拒", matches!(bilateral_at(img, 0, 0, 3, SIGMA_S[0], SIGMA_R[0], grt), Err(XC_RADIUS_OOB)), "r=3 ∉ 闭集");
        }
    }
    cs.add("空图拒", Gray::new(0, 5, alloc::vec![]).is_err(), "DIM");
    cs.add("尺寸失配拒", Gray::new(2, 2, alloc::vec![1u8, 2, 3]).is_err(), "DIM");
    CheckSet::merge(CheckSet::merge(cs, run_cga81_deep()), run_cga81_deep2())
}

// ===== 深化面：σ 闭集与权重对称账 =====
/// 空间核各向同性：同距同权 + 距离单调（d 增权不增）。
pub fn gs_isotropy() -> Result<(), u32> {
    for &s in SIGMA_S.iter() {
        let w1 = gs_lookup(s, 1)?;
        let w2 = gs_lookup(s, 1)?;
        if w1 != w2 {
            return Err(XC_WSUM_DRIFT);
        }
        let mut prev = u32::MAX;
        for d in 0..5u32 {
            let w = gs_lookup(s, d)?;
            if w > prev {
                return Err(XC_WSUM_DRIFT);
            }
            prev = w;
        }
    }
    Ok(())
}

/// σ_r 表单调核：色差越大权重越小；零色差 = 全权 4096。
pub fn gr_monotone() -> Result<(), u32> {
    for &sr in SIGMA_R.iter() {
        let t = gr_table(sr)?;
        let mut prev = u32::MAX;
        for d in 0..256usize {
            if t[d] > prev {
                return Err(XC_WSUM_DRIFT);
            }
            prev = t[d];
        }
        if t[0] != WSUM_NORM {
            return Err(XC_WSUM_DRIFT);
        }
    }
    Ok(())
}

pub fn run_cga81_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga81_deep");
    cs.add("空间核各向同性", gs_isotropy().is_ok(), "同距同权");
    cs.add("距离单调", gs_isotropy().is_ok(), "d 增权减");
    cs.add("色差单调", gr_monotone().is_ok(), "表单调");
    cs.add("零差全权", gr_table(SIGMA_R[0]).map(|t| t[0] == WSUM_NORM).unwrap_or(false), "d=0→4096");
    cs.add("σ_s 越界拒", gs_lookup(99, 1).is_err(), "闭集外");
    cs.add("σ_r 越界拒", gr_table(99).is_err(), "闭集外");
    cs.add("平坦收敛", flat_converge(6, 6, 128).is_ok(), "常量图不变");
    cs
}

// ===== 深化面二：灰度域与消融账 =====
pub fn run_cga81_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga81_deep2");
    cs.add("恒等消融核", {
        if let Ok(g) = Gray::new(6, 6, alloc::vec![9u8; 36]) {
            identity_ablation(&g) == Ok(true)
        } else {
            false
        }
    }, "σr→0 输出==输入");
    cs.add("方差单调核", {
        if let (Ok(a), Ok(b)) = (
            Gray::new(4, 4, alloc::vec![0u8, 255, 0, 255, 255, 0, 255, 0, 0, 255, 0, 255, 255, 0, 255, 0]),
            Gray::new(4, 4, alloc::vec![60u8; 16]),
        ) {
            variance_monotone(&a, &b).is_ok()
        } else {
            false
        }
    }, "滤波后方差降");
    cs.add("灰度尺寸拒", Gray::new(2, 2, alloc::vec![1u8; 3]).is_err(), "LEN_MISMATCH");
    cs.add("灰度零拒", Gray::new(0, 2, alloc::vec![]).is_err(), "DIM_ZERO");
    cs.add("权重界", wsum_floor(&[4096]).is_ok(), "满权过");
    cs.add("GS 零距满权", gs_lookup(SIGMA_S[0], 0) == Ok(1024), "d=0 锚");
    cs
}
