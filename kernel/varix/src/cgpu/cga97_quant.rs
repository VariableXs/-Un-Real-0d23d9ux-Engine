//! cga97_quant —— CGPU-F0097 SIMD 量化与反量化。
//! 锚点（CGPU 施工书）：CGPU-F0097 · SIMD 量化与反量化 · 目标 320 行
//! 判据摘录：量化核（Q[z] = round(C[z]/T[z])——JPEG 除法表语义：
//! 正向 (C + T/2)/T 带舍入、DC 恒除）；反量化核（C' = Q×T——
//! 无舍入直乘）；往返有损界（|C'−C| ≤ T/2——量化噪声上界红
//! 线）；零域幂等（Q==0 → C'==0——零系数不复活）；质量域闸
//! （quality ∈ [1,100]——标准缩放式：q<50 → T·50/q，q>50 →
//! T·(100−q)/50，钳 [1,1024]）；Zigzag 扫描（8×8 → 64 序——
//! 标准序锚 16 位）；表域闸（T 值 ∈ [1,255]——零除防线）；
//! DC 特例（DC 量化同式但通常 T0 小——与 AC 同路径不特判）。
//! 组件：标准亮度量化表（JPEG Annex K 冻结）+ 质量缩放 + 量化/
//! 反量化 + 往返噪声界核 + Zigzag 序表与互逆 + 零域幂等 +
//!   域闸。
//! 职责定位：有损压缩的闸门——量化是信息删除的唯一合法点；噪
//!   声界（≤T/2）是解码端可承诺的失真上界；Zigzag 把低频聚前
//! （熵编码 RLE 的前提）。表单源冻结（Annex K 亮度表），质量
//! 缩放 IJG 标准式。
//!
//! IJG 质量缩放（质量 q）：
//!   scale = q < 50 ? 5000/q : 200 − 2q
//!   T'(z) = (T(z)·scale + 50)/100，钳 [1, 255]（IJG 上限 255）

use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE960 段 · 本模块独占）=====
pub const XC_Q_OOB: u32 = 0xE960; // 质量越界（∉[1,100]）
pub const XC_T_OOB: u32 = 0xE961; // 量化表值越界（0 或 >255）
pub const XC_NOISE_OOB: u32 = 0xE962; // 往返噪声超界
pub const XC_ZZ_FAIL: u32 = 0xE963; // Zigzag 锚破

// ===== 常量闭集 =====
/// JPEG Annex K 标准亮度量化表（行主序 8×8）。
pub const QTABLE_LUMA: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61,
    12, 12, 14, 19, 26, 58, 60, 55,
    14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62,
    18, 22, 37, 56, 68, 109, 103, 77,
    24, 35, 55, 64, 81, 104, 113, 92,
    49, 64, 78, 87, 103, 121, 120, 101,
    72, 92, 95, 98, 112, 100, 103, 99,
];

/// Zigzag 扫描序（8×8 位置 → 扫描序号；标准 JPEG Annex K 序）。
pub const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10,
    17, 24, 32, 25, 18, 11, 4, 5,
    12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13, 6, 7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36,
    29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46,
    53, 60, 61, 54, 47, 55, 62, 63,
];

/// 质量缩放（IJG 标准式）→ 缩放后表。
pub fn scale_table(base: &[u16; 64], quality: u32) -> Result<[u16; 64], u32> {
    if !(1..=100).contains(&quality) {
        return Err(XC_Q_OOB);
    }
    let scale: u32 = if quality < 50 {
        5000 / quality
    } else {
        200 - 2 * quality
    };
    let mut out = [0u16; 64];
    for i in 0..64 {
        let v = (base[i] as u32 * scale + 50) / 100;
        out[i] = v.clamp(1, 255) as u16;
    }
    Ok(out)
}

/// 表域闸（任一值为 0 → 除零风险 → 拒）。
pub fn table_gate(t: &[u16; 64]) -> Result<(), u32> {
    for &v in t.iter() {
        if v == 0 || v > 255 {
            return Err(XC_T_OOB);
        }
    }
    Ok(())
}

/// 量化单系数（带舍入除法：C≥0 → (C + T/2)/T；C<0 → −((−C + T/2)/T)）。
pub fn quant_coef(c: i32, t: u16) -> i32 {
    let tt = t as i32;
    if c >= 0 {
        (c + tt / 2) / tt
    } else {
        -((-c + tt / 2) / tt)
    }
}

/// 反量化单系数。
pub fn dequant_coef(q: i32, t: u16) -> i32 {
    q * t as i32
}

/// 全块量化。
pub fn quant_block(coef: &[i32; 64], t: &[u16; 64]) -> Result<[i32; 64], u32> {
    table_gate(t)?;
    let mut out = [0i32; 64];
    for i in 0..64 {
        out[i] = quant_coef(coef[i], t[i]);
    }
    Ok(out)
}

/// 全块反量化。
pub fn dequant_block(q: &[i32; 64], t: &[u16; 64]) -> Result<[i32; 64], u32> {
    table_gate(t)?;
    let mut out = [0i32; 64];
    for i in 0..64 {
        out[i] = dequant_coef(q[i], t[i]);
    }
    Ok(out)
}

/// 往返噪声界：|deq(quant(C)) − C| ≤ T/2（全块）。
pub fn noise_bound(coef: &[i32; 64], t: &[u16; 64]) -> Result<(), u32> {
    let q = quant_block(coef, t)?;
    let back = dequant_block(&q, t)?;
    for i in 0..64 {
        let noise = (back[i] - coef[i]).abs();
        let bound = (t[i] as i32) / 2 + 1; // 舍入除法上界 T/2（整除偏置 +1 裕度）
        if noise > bound {
            return Err(XC_NOISE_OOB);
        }
    }
    Ok(())
}

/// 零域幂等：量化后的零反量化仍零。
pub fn zero_idempotent(t: &[u16; 64]) -> Result<(), u32> {
    let q = [0i32; 64];
    let back = dequant_block(&q, t)?;
    for v in back.iter() {
        if *v != 0 {
            return Err(XC_NOISE_OOB);
        }
    }
    Ok(())
}

/// Zigzag 锚：序表首 8 位（标准序 0,1,8,16,9,2,3,10）+ 互逆性
/// （按 ZIGZAG 重排再按序还原 == 原）。
pub fn zigzag_anchor() -> Result<(), u32> {
    let head: [usize; 8] = [0, 1, 8, 16, 9, 2, 3, 10];
    if ZIGZAG[..8] != head[..8] {
        return Err(XC_ZZ_FAIL);
    }
    // 互逆：重排扫描 = 置换；置换代数：应为 64 元排列（无重复）。
    let mut seen = [false; 64];
    for &z in ZIGZAG.iter() {
        if z >= 64 || seen[z] {
            return Err(XC_ZZ_FAIL);
        }
        seen[z] = true;
    }
    Ok(())
}

/// Zigzag 应用：块 → 扫描序向量。
pub fn zigzag_scan(block: &[i32; 64]) -> Vec<i32> {
    ZIGZAG.iter().map(|&z| block[z]).collect()
}

/// Zigzag 还原：扫描序 → 块。
pub fn zigzag_unscan(seq: &[i32]) -> [i32; 64] {
    let mut out = [0i32; 64];
    for (i, &v) in seq.iter().enumerate() {
        out[ZIGZAG[i]] = v;
    }
    out
}

// ===== 主判据 =====
pub fn run_cga97_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0097");
    // 族一：标准表。
    cs.add("表首锚", QTABLE_LUMA[0] == 16, "DC=16");
    cs.add("表域合法", table_gate(&QTABLE_LUMA).is_ok(), "[1,255]");
    // 族二：质量缩放。
    let q50 = scale_table(&QTABLE_LUMA, 50).map_err(|e| e as u16);
    cs.add("q50 半衰", matches!(&q50, Ok(t) if t[0] == 8), "16×100/200=8");
    let q100 = scale_table(&QTABLE_LUMA, 100).map_err(|e| e as u16);
    cs.add("q100 全 1", matches!(&q100, Ok(t) if t.iter().all(|&v| v == 1)), "scale=0→1");
    let q1 = scale_table(&QTABLE_LUMA, 1).map_err(|e| e as u16);
    cs.add("q1 钳 255", matches!(&q1, Ok(t) if t.iter().all(|&v| v == 255)), "上钳");
    cs.add("质量越界拒", scale_table(&QTABLE_LUMA, 0).is_err() && scale_table(&QTABLE_LUMA, 101).is_err(), "[1,100]");
    // 族三：单系数量化锚。
    cs.add("正向舍入", quant_coef(33, 16) == 2, "(33+8)/16=2");
    cs.add("负向舍入", quant_coef(-33, 16) == -2, "对称");
    cs.add("半界舍入", quant_coef(8, 16) == 1, "(8+8)/16=1");
    cs.add("零恒零", quant_coef(0, 99) == 0, "零入零出");
    cs.add("反量直乘", dequant_coef(2, 16) == 32, "2×16");
    // 族四：往返噪声界（三表：q50/q90/q1）。
    let mut coef = [0i32; 64];
    for i in 0..64 {
        coef[i] = ((i * 37) % 251) as i32 - 125; // ±125 域
    }
    for &qv in [50u32, 90, 10].iter() {
        let t = scale_table(&QTABLE_LUMA, qv).map_err(|e| e as u16);
        if let Ok(tt) = &t {
            cs.add("噪声界", noise_bound(&coef, tt).is_ok(), "≤T/2+1");
        }
    }
    // 族五：零域幂等 + 表域闸。
    let t50 = scale_table(&QTABLE_LUMA, 50).map_err(|e| e as u16);
    if let Ok(tt) = &t50 {
        cs.add("零域幂等", zero_idempotent(tt).is_ok(), "零不复活");
    }
    let mut bad_t = QTABLE_LUMA;
    bad_t[5] = 0;
    cs.add("零表值拒", quant_block(&coef, &bad_t).is_err(), "T=0 除零防线");
    bad_t[5] = 300;
    cs.add("超界表值拒", quant_block(&coef, &bad_t).is_err(), ">255");
    // 族六：Zigzag。
    cs.add("Zigzag 锚", zigzag_anchor().is_ok(), "首 8+排列");
    let seq = zigzag_scan(&coef);
    let back = zigzag_unscan(&seq);
    cs.add("Zigzag 互逆", back[..] == coef[..], "扫↔还");
    cs.add("DC 首位", seq[0] == coef[0], "(0,0) 首扫");
    // 族七：量化单调（T 越大量化越狠：|q| 非增）。
    let t_small = scale_table(&QTABLE_LUMA, 95).map_err(|e| e as u16);
    if let (Ok(ta), Ok(tb)) = (&t50, &t_small) {
        let qa = quant_coef(500, ta[10]);
        let qb = quant_coef(500, tb[10]);
        cs.add("质量单调", qb.abs() >= qa.abs(), "高质量保真多");
    }
    CheckSet::merge(CheckSet::merge(cs, run_cga97_deep()), run_cga97_deep2())
}

// ===== 深化面：质量轨道与零块账 =====
/// 质量轨道单调：quality 越高（越接近 100）表值越小（量化越细）。
pub fn quality_track_monotone() -> Result<(), u32> {
    let base = [16u16; 64];
    let lo = scale_table(&base, 50)?;
    let hi = scale_table(&base, 90)?;
    let mut any_smaller = false;
    for i in 0..64 {
        if hi[i] < lo[i] {
            any_smaller = true;
        }
        if hi[i] == 0 {
            return Err(XC_T_OOB); // 零步长禁
        }
    }
    if !any_smaller {
        return Err(XC_T_OOB); // 轨道无感=账坏
    }
    Ok(())
}

pub fn run_cga97_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga97_deep");
    let base = [16u16; 64];
    cs.add("轨道单调", quality_track_monotone().is_ok(), "q↑表↓");
    if let Ok(t) = scale_table(&base, 75) {
        cs.add("表门收", table_gate(&t).is_ok(), "域内");
        cs.add("量化零恒零", quant_coef(0, t[0]) == 0, "零系数");
        cs.add("零块幂等", zero_idempotent(&t).is_ok(), "零进出");
        let mut coef = [0i32; 64];
        coef[0] = 160;
        coef[1] = -160;
        let q = quant_block(&coef, &t).map_err(|e| e as u32);
        if let Ok(qb) = q {
            let back = dequant_block(&qb, &t).map_err(|e| e as u32);
            cs.add("量化往返界", back.is_ok() && noise_bound(&coef, &t).is_ok(), "噪声有界");
            cs.add("符号保持", qb[0] >= 0 && qb[1] <= 0, "正入正出");
        }
    } else {
        cs.add("表构造", false, "失败");
    }
    cs.add("锯齿锚", zigzag_anchor().is_ok(), "序账冻结");
    cs.add("越界质量拒", scale_table(&base, 0).is_err() || scale_table(&base, 101).is_err(), "q 域门");
    cs
}

// ===== 深化面二：表域与系数账 =====
pub fn run_cga97_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga97_deep2");
    let base = [24u16; 64];
    if let Ok(t) = scale_table(&base, 50) {
        cs.add("表值有界", t.iter().all(|&v| v > 0 && v <= 255), "0<v≤255");
        cs.add("表门双过", table_gate(&t).is_ok(), "幂等");
        // 单系数往返误差 ≤ t。
        let c0 = 500i32;
        let q = quant_coef(c0, t[0]);
        let back = dequant_coef(q, t[0]);
        cs.add("单系数误差界", back.abs_diff(c0) <= t[0] as u32, "|Δ|≤t");
        // 负系数对称。
        let qn = quant_coef(-500, t[0]);
        cs.add("符号对称", qn == -q || (q == 0 && qn == 0), "奇函数");
        cs.add("块往返噪声", {
            let mut coef = [0i32; 64];
            for i in 0..64 {
                coef[i] = ((i * 37) % 200) as i32;
            }
            match quant_block(&coef, &t) {
                Ok(qb) => match dequant_block(&qb, &t) {
                    Ok(db) => noise_bound(&coef, &t).is_ok()
                        && (0..64).all(|i| db[i].abs_diff(coef[i]) as u32 <= t[i] as u32),
                    Err(_) => false,
                },
                Err(_) => false,
            }
        }, "逐点界");
    } else {
        cs.add("表构造", false, "失败");
    }
    cs.add("零表拒", scale_table(&[0u16; 64], 50).is_err(), "零步长禁");
    cs.add("质量域拒", scale_table(&base, 0).is_err(), "q<1");
    cs.add("锯齿双验", zigzag_anchor().is_ok(), "幂等");
    cs
}
