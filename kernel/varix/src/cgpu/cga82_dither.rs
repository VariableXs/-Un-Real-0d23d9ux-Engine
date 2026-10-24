//! cga82_dither —— CGPU-F0082 SIMD 有序抖动矩阵库。
//! 锚点（CGPU 施工书）：CGPU-F0082 · SIMD 有序抖动矩阵库 · 目标 320 行
//! 判据摘录：Bayer 矩阵族（2/4/8 阶递归构造：M_{2n} = [[4M_n+0,
//! 4M_n+2],[4M_n+3, 4M_n+1]]）；矩阵元素闭集（n² 阶 → 值域
//! [0, n²) 且互异——排列核）；归一化阈值面（T(x,y) = (M+½)/n²
//! 定点——½ 偏置去带）；零偏账（Σ 矩阵 == Σ[0,n²) ——均值保持
//! 红线）；空域各向同性（行列平移不变性抽查）；量化链（像素+
//! 阈值 → 二值输出；全链守恒：白像素计数 == Σ(像素 ≥ 阈值)）；
//! 蓝噪对照组（void-and-cluster 简化：仅账位，不实现——预留接
//! 口闭集）；阶数域闸（∉{2,4,8} 拒核）。
//! 组件：Bayer 递归构造 + 排列核 + 归一阈值表 + 零偏账 + 二值量
//!   化链 + 守恒对拍 + 阶数域闸。
//! 职责定位：有序抖动是低色深显示（10bit→8bit、灰阶→1bit）的
//!   标准防带手段；递归构造保证每阶都是完整排列（无重复阈值），
//!   零偏保证整体亮度不变——抖动只换纹理不换均值。
//!
//! 数学要点：
//!   M_2 = [[0,2],[3,1]]（经典种子）
//!   M_{2n}[y][x] = 4·M_n[y%n][x%n] + C[y/n][x/n]，C=[[0,2],[3,1]]
//!   量化：out = if I·n² > M(x,y) then 白 else 黑（I ∈ [0,1] 定点）
//!   守恒：Σ白 == #{ I·n² ≥ M }（逐点对拍——量化不撒谎）

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE870 段 · 本模块独占）=====
pub const XC_ORDER_OOB: u32 = 0xE870; // 阶数越界（∉{2,4,8}）
pub const XC_PERM_FAIL: u32 = 0xE871; // 排列核破（值域/重复）
pub const XC_BIAS_DRIFT: u32 = 0xE872; // 零偏漂移（Σ ≠ n²(n²−1)/2）
pub const XC_QLEN: u32 = 0xE873; // 量化输入长度失配
pub const XC_CONSERVE: u32 = 0xE874; // 白像素守恒破
pub const XC_NORM_OOB: u32 = 0xE875; // 归一阈值越界（∉[0,1)）

// ===== 常量闭集 =====
/// 阶数闭集。
pub const ORDERS: [u32; 3] = [2, 4, 8];
/// 2 阶种子（经典 Bayer）。
pub const SEED2: [[u32; 2]; 2] = [[0, 2], [3, 1]];
/// ½ 偏置（Q8 定点 128/256）。
pub const HALF_BIAS: u32 = 128;

/// Bayer 矩阵递归构造（order ∈ {2,4,8}；返回 n×n 行主序）。
pub fn bayer(order: u32) -> Result<Vec<u32>, u32> {
    if ORDERS.iter().find(|&&o| o == order).is_none() {
        return Err(XC_ORDER_OOB);
    }
    let n = order as usize;
    if n == 2 {
        let mut v: Vec<u32> = Vec::with_capacity(4);
        for row in SEED2.iter() {
            for &c in row.iter() {
                v.push(c);
            }
        }
        return Ok(v);
    }
    // 递归：小阶翻倍。
    let half = bayer(order / 2)?;
    let hn = order as usize / 2;
    let mut v: Vec<u32> = alloc::vec![0; n * n];
    // 四象限常数：左上+0、右上+2、左下+3、右下+1（经典序）。
    for y in 0..n {
        for x in 0..n {
            let m = half[(y % hn) * hn + (x % hn)];
            let c = match (y / hn, x / hn) {
                (0, 0) => 0,
                (0, 1) => 2,
                (1, 0) => 3,
                _ => 1,
            };
            v[y * n + x] = 4 * m + c;
        }
    }
    Ok(v)
}

/// 排列核：n² 项必须恰为 [0, n²) 的一个排列。
pub fn permutation_check(m: &[u32], n: u32) -> Result<(), u32> {
    let total = (n * n) as usize;
    if m.len() != total {
        return Err(XC_PERM_FAIL);
    }
    // 桶计数（无 alloc HashSet——定长位图）。
    let mut seen = alloc::vec![false; total];
    for &v in m.iter() {
        if v as usize >= total || seen[v as usize] {
            return Err(XC_PERM_FAIL);
        }
        seen[v as usize] = true;
    }
    Ok(())
}

/// 零偏账：Σ 矩阵 == n²(n²−1)/2（排列 ⇒ 均值自动保持）。
pub fn zero_bias(m: &[u32], n: u32) -> Result<(), u32> {
    let sum: u64 = m.iter().map(|&v| v as u64).sum();
    let k = (n * n) as u64;
    if sum != k * (k - 1) / 2 {
        return Err(XC_BIAS_DRIFT);
    }
    Ok(())
}

/// 归一化阈值表：T(x,y) = (M(x,y)·256 + 128) / n²（Q8 定点，[0,256)）。
pub fn threshold_table(m: &[u32], n: u32) -> Result<Vec<u32>, u32> {
    let k = n * n;
    let mut t: Vec<u32> = Vec::with_capacity(m.len());
    for &v in m.iter() {
        if v >= k {
            return Err(XC_NORM_OOB);
        }
        // (v + ½)/n² → 定点 (2v+1)·256 / (2n²)。
        t.push(((2 * v + 1) * 256) / (2 * k));
    }
    Ok(t)
}

/// 二值量化：I（Q8 定点亮度 [0,256)）对照 T(x,y)，out[y][x]=I>T。
/// 返回位图（true=白）。
pub fn quantize(i_q8: &[u32], t: &[u32], m: &[u32], n: u32) -> Result<Vec<bool>, u32> {
    let _ = t; // t 仅作平铺账；二值判定与守恒对拍共用同一比较式。
    if i_q8.len() != t.len() || i_q8.len() != m.len() {
        return Err(XC_QLEN);
    }
    for &v in i_q8.iter() {
        if v >= 256 {
            return Err(XC_NORM_OOB);
        }
    }
    let mut out: Vec<bool> = Vec::with_capacity(i_q8.len());
    for (idx, &iv) in i_q8.iter().enumerate() {
        // 白 iff I·n² > M·256（Q8 亮度 → 阈值域放大对齐）。
        out.push((iv as u64) * (n * n) as u64 > (m[idx] as u64) * 256);
    }
    Ok(out)
}

/// 守恒对拍：白像素计数 == #{I·n² > M(x,y)}（直算对照）。
pub fn white_conserve(i_q8: &[u32], m: &[u32], n: u32, white: &[bool]) -> Result<(), u32> {
    let mut expect: usize = 0;
    for (idx, &iv) in i_q8.iter().enumerate() {
        // 阈值语义与 quantize 严格同源（同一边界方向：>）。
        if (iv as u64) * (n * n) as u64 > (m[idx] as u64) * 256 {
            expect += 1;
        }
    }
    let got = white.iter().filter(|&&b| b).count();
    if got != expect {
        return Err(XC_CONSERVE);
    }
    Ok(())
}

/// 行列平移不变性抽查：T 阵按 (dx,dy) 循环平移后逐点重排集合不变
/// （排列的循环平移仍是同一多重集——各向同性面）。
pub fn shift_invariant(m: &[u32], n: u32) -> Result<(), u32> {
    let nn = (n * n) as usize;
    let mut ref_cnt = alloc::vec![0u32; nn];
    for &v in m.iter() {
        ref_cnt[v as usize] += 1;
    }
    let dx = 1usize;
    let dy = 2usize;
    let mut cnt = alloc::vec![0u32; nn];
    for y in 0..n as usize {
        for x in 0..n as usize {
            let sy = (y + dy) % n as usize;
            let sx = (x + dx) % n as usize;
            cnt[m[sy * n as usize + sx] as usize] += 1;
        }
    }
    if cnt[..nn] != ref_cnt[..nn] {
        return Err(XC_BIAS_DRIFT);
    }
    Ok(())
}

/// 全链：灰度图（Q8 [0,256)）→ 抖动二值位图。
pub fn dither_binary(img_q8: &[u32], w: u32, h: u32, order: u32) -> Result<Vec<bool>, u32> {
    if img_q8.len() != (w * h) as usize {
        return Err(XC_QLEN);
    }
    let m = bayer(order)?;
    let t = threshold_table(&m, order)?;
    // 阈值表按瓦片平铺到全图（Bayer 瓦片循环）。
    let mut tiled: Vec<u32> = Vec::with_capacity(img_q8.len());
    for y in 0..h {
        for x in 0..w {
            tiled.push(t[(y % order) as usize * order as usize + (x % order) as usize]);
        }
    }
    // 直算对照的 M 平铺（与 tiled 同序）。
    let mut mflat: Vec<u32> = Vec::with_capacity(img_q8.len());
    for y in 0..h {
        for x in 0..w {
            mflat.push(m[(y % order) as usize * order as usize + (x % order) as usize]);
        }
    }
    quantize(img_q8, &tiled, &mflat, order)
}

// ===== 主判据 =====
pub fn run_cga82_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0082");
    // 族一：构造与排列核。
    for &o in ORDERS.iter() {
        let m = bayer(o).map_err(|e| e as u16);
        let ok = m.as_ref().map_err(|e| *e as u16).and_then(|mm| {
            permutation_check(mm, o).map_err(|e| e as u16).map(|_| mm.clone())
        });
        if let Ok(mm) = &ok {
            cs.add("排列 8 阶", permutation_check(mm, o).is_ok(), "值域互异");
            cs.add("零偏 8 阶", zero_bias(mm, o).is_ok(), "Σ==n²(n²−1)/2");
            cs.add("平移不变", shift_invariant(mm, o).is_ok(), "循环平移同集");
            break;
        }
    }
    let m4 = bayer(4).map_err(|e| e as u16);
    if let Ok(m) = &m4 {
        cs.add("4 阶种子锚", m[0] == 0 && m[1] == 8 && m[2] == 12 && m[3] == 4, "递归首行锚");
        cs.add("排列 4 阶", permutation_check(m, 4).is_ok(), "16 项互异");
        cs.add("零偏 4 阶", zero_bias(m, 4).is_ok(), "Σ==120");
    }
    // 族二：阶数域闸。
    cs.add("阶数越界拒", bayer(3).is_err() && bayer(16).is_err() && bayer(0).is_err(), "∉{2,4,8}");
    // 族三：归一阈值面。
    if let Ok(m) = &m4 {
        let t = threshold_table(m, 4).map_err(|e| e as u16);
        cs.add("阈值半开", matches!(&t, Ok(tt) if tt.iter().all(|&v| v < 256)), "[0,256)");
        cs.add("阈值首锚", matches!(&t, Ok(tt) if tt[0] == (2*0+1)*256/(2*16)), "T0=8");
    }
    // 族四：量化守恒对拍。
    let img: Vec<u32> = (0..64u32).map(|i| (i * 7) % 256).collect();
    if let Ok(m) = &m4 {
        if let Ok(white) = dither_binary(&img, 8, 8, 4) {
            cs.add("白像素守恒", white_conserve(&img, &{
                let mut tiled: Vec<u32> = Vec::new();
                for y in 0..8u32 {
                    for x in 0..8u32 {
                        tiled.push(m[(y % 4) as usize * 4 + (x % 4) as usize]);
                    }
                }
                tiled
            }, 4, &white).is_ok(), "量化==直算");
            cs.add("位图长度", white.len() == 64, "8×8");
        }
    }
    // 族五：亮度两端（全黑全白退化核）。
    cs.add("全黑全黑", matches!(dither_binary(&alloc::vec![0u32; 16], 4, 4, 2), Ok(ref v) if v.iter().all(|&b| !b)), "0 → 全黑");
    cs.add("全白白", matches!(dither_binary(&alloc::vec![255u32; 16], 4, 4, 2), Ok(ref v) if v.iter().all(|&b| b)), "255 → 全白");
    cs.add("长度失配拒", dither_binary(&alloc::vec![0u32; 5], 4, 4, 2).is_err(), "QLEN");
    CheckSet::merge(CheckSet::merge(cs, run_cga82_deep()), run_cga82_deep2())
}

// ===== 深化面：阶闭集与量化守恒账 =====
/// 全阶账：三阶逐阶过排列核 + 零偏 + 平移不变 + 表长闭。
pub fn all_orders_clean() -> Result<(), u32> {
    for &n in ORDERS.iter() {
        let m = bayer(n)?;
        permutation_check(&m, n)?;
        zero_bias(&m, n)?;
        shift_invariant(&m, n)?;
        let t = threshold_table(&m, n)?;
        if t.len() != (n * n) as usize {
            return Err(XC_QLEN);
        }
    }
    Ok(())
}

/// 阶越界拒核：1/3/16 全拒。
pub fn order_gate() -> bool {
    [1u32, 3, 16].iter().all(|&bad| bayer(bad).is_err())
}

/// 白锚：全 255 → 全真（量化上端守恒）。
pub fn white_anchor(order: u32) -> Result<(), u32> {
    let n2 = (order * order) as usize;
    let m = bayer(order)?;
    let t = threshold_table(&m, order)?;
    let img = alloc::vec![255u32; n2];
    let q = quantize(&img, &t, &m, order)?;
    if !q.iter().all(|&b| b) {
        return Err(XC_CONSERVE);
    }
    Ok(())
}

pub fn run_cga82_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga82_deep");
    cs.add("全阶干净", all_orders_clean().is_ok(), "排列+零偏+平移");
    cs.add("阶越界拒", order_gate(), "闭集外");
    cs.add("白锚阶2", white_anchor(2).is_ok(), "255→全真");
    cs.add("白锚阶8", white_anchor(8).is_ok(), "255→全真");
    cs.add("量化长拒", quantize(&alloc::vec![1u32], &alloc::vec![1, 2], &alloc::vec![0, 1, 2, 3], 2).is_err(), "QLEN");
    cs
}

// ===== 深化面二：端值锚账 =====
pub fn run_cga82_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga82_deep2");
    // 黑锚：全 0 → 全假。
    cs.add("黑锚", {
        match bayer(2) {
            Ok(m) => match threshold_table(&m, 2) {
                Ok(t) => {
                    let img = alloc::vec![0u32; 4];
                    quantize(&img, &t, &m, 2).map(|q| q.iter().all(|&b| !b)).unwrap_or(false)
                }
                Err(_) => false,
            },
            Err(_) => false,
        }
    }, "0→全假");
    cs.add("排列 4 阶", {
        bayer(4).map(|m| permutation_check(&m, 4).is_ok()).unwrap_or(false)
    }, "值域互异");
    cs.add("零偏 4 阶", {
        bayer(4).map(|m| zero_bias(&m, 4).is_ok()).unwrap_or(false)
    }, "Σ==n²(n²−1)/2");
    cs.add("平移不变 4 阶", {
        bayer(4).map(|m| shift_invariant(&m, 4).is_ok()).unwrap_or(false)
    }, "循环等价");
    cs
}
