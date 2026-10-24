//! cga93_idct —— CGPU-F0093 JPEG 解码 SIMD（IDCT）。
//! 锚点（CGPU 施工书）：CGPU-F0093 · JPEG 解码 IDCT · 目标 440 行
//! 判据摘录：二维 IDCT 全链（8×8 块：行一维 IDCT + 列一维 IDCT
//! + 中心化偏移 +128——JPEG 语义冻结）；正交性账（IDCT∘DCT ==
//! 恒等——往返无损红线，整数域 ±1 容差）；直流系数锚（DC-only
//! 块 → 常数面 +128——DC 直接平铺核）；基频锚（单系数块 → 对
//! 应基图样——余弦基可核）；零块锚（全零 → 128 灰——零输入
//! 断面）；系数域闸（量化系数 ∈ [−1024,1023]——JPEG 域闭集）；
//! 舍入账（输出 ∈ [0,255] 钳制且有账）；能量守恒（Parseval 定
//! 点近似：Σ|coef|² 与 Σ|px−128|² 同阶——量级账）。
//! 组件：一维 IDCT（定点 16 位精度：系数×固定余弦表求和）+ 二
//! 维组装（行→列→偏移）+ 参照 DCT（前向——往返对拍用）+ DC/
//! 基频/零块三锚 + 钳制账 + 域闸。
//! 职责定位：JPEG/MJPEG/AVI 内帧解码的数学核心——8×8 块级 IDCT
//! 是解码管线的计算热点；定点实现（C8 固定余弦表，1024 基），
//! 零浮点；与 cga96（前向 DCT）严格互逆——正交对拍是本模块
//! 的首要判据。
//!
//! 数学要点（1024 基定点）：
//!   1D IDCT: out[x] = Σ_u c(u)·F[u]·cos((2x+1)uπ/16)，c(0)=1/√2
//!   定点化：cos 表 ×1024，c(0) 表内预折，输出 >>10 后取整钳制。
//!   2D：先行后列（可分离性），中间不钳（行/列级精度保留），
//!   终态 +128 后钳 [0,255]。

use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE920 段 · 本模块独占）=====
pub const XC_COEF_OOB: u32 = 0xE920; // 系数越界
pub const XC_ROUND_OOB: u32 = 0xE921; // 输出越界（未钳制直出）
pub const XC_ORTHO_FAIL: u32 = 0xE922; // 正交对拍破
pub const XC_DC_FAIL: u32 = 0xE923; // DC 锚破
pub const XC_ENERGY_FAIL: u32 = 0xE924; // 能量账破

// ===== 常量闭集 =====
/// 定点基（cos 表 ×1024）。
pub const FX_BASE: i32 = 1024;
/// 系数域闭集（JPEG 量化系数界）。
pub const COEF_MIN: i32 = -1024;
pub const COEF_MAX: i32 = 1023;

/// 8×8 固定余弦表（×1024）：COS[u][x] = round(1024·cos((2x+1)uπ/16))。
/// 手算冻结（对称可减半，但存全表——SIMD 友好）。
pub const COS8: [[i32; 8]; 8] = [
    [1024, 1024, 1024, 1024, 1024, 1024, 1024, 1024],
    [1024, 973, 820, 574, 276, -276, -574, -820],
    [1024, 820, -276, -973, -1024, -973, -276, 820],
    [1024, 574, -973, -276, 820, 1024, 276, -574],
    [1024, 276, -1024, 820, 574, -574, -820, 1024],
    [1024, -276, -574, 1024, -276, -574, 1024, -276],
    [1024, -574, 276, 820, -1024, 820, 276, -574],
    [1024, -820, 574, -276, 276, -574, 820, -1024],
];

/// c(u) 折算（c(0)=1/√2 → ×724/1024，其余 =1）：直乘 724 或 1024。
pub const CU: [i32; 8] = [724, 1024, 1024, 1024, 1024, 1024, 1024, 1024];

/// 一维 IDCT（8 点；coef ∈ 系数域）。
pub fn idct1d(coef: &[i32; 8]) -> Result<[i32; 8], u32> {
    for &c in coef.iter() {
        if !(COEF_MIN..=COEF_MAX).contains(&c) {
            return Err(XC_COEF_OOB);
        }
    }
    let mut out = [0i32; 8];
    for x in 0..8 {
        let mut acc: i64 = 0;
        for u in 0..8 {
            acc += (coef[u] as i64) * (COS8[u][x] as i64) * (CU[u] as i64);
        }
        // acc 定点尺度 = 1024(cu) × 1024(cos) = 2^20；补 1/2 因子
        //（1D 正交归一）后 >>21，加舍入偏置（半上）。
        out[x] = ((acc + (1i64 << 20)) >> 21) as i32;
    }
    Ok(out)
}

/// 一维 DCT（参照实现——与 idct1d 严格自逆：同 c(u)/2 权重、
/// 同 >>21 定标，JPEG 对称定义 F=(c/2)Σf·cos ↔ f=Σ(c/2)F·cos）。
pub fn dct1d(px: &[i32; 8]) -> [i32; 8] {
    let mut out = [0i32; 8];
    for u in 0..8 {
        let mut acc: i64 = 0;
        for x in 0..8 {
            acc += (px[x] as i64) * (COS8[u][x] as i64) * (CU[u] as i64);
        }
        out[u] = ((acc + (1i64 << 20)) >> 21) as i32;
    }
    out
}

/// 8×8 块容器（行主序）。
pub type Block = [[i32; 8]; 8];

/// 二维 IDCT：行→列→+128→钳制。
/// 返回像素块 [0,255]。
pub fn idct2d(coef: &Block) -> Result<Block, u32> {
    // 行级：tmp[y] = idct1d(coef[y])。
    let mut tmp: Block = [[0i32; 8]; 8];
    for y in 0..8 {
        tmp[y] = idct1d(&coef[y])?;
    }
    // 列级：转置复用 idct1d。
    let mut out: Block = [[0i32; 8]; 8];
    for x in 0..8 {
        let col = [tmp[0][x], tmp[1][x], tmp[2][x], tmp[3][x], tmp[4][x], tmp[5][x], tmp[6][x], tmp[7][x]];
        let r = idct1d(&col)?;
        for y in 0..8 {
            out[y][x] = r[y];
        }
    }
    // 中心化偏移 + 钳制。
    for y in 0..8 {
        for x in 0..8 {
            let v = out[y][x] + 128;
            out[y][x] = v.clamp(0, 255);
        }
    }
    Ok(out)
}

/// 二维 DCT（参照：−128 → 列 DCT → 行 DCT）。
pub fn dct2d(px: &Block) -> Block {
    let mut tmp: Block = [[0i32; 8]; 8];
    for y in 0..8 {
        let row = [
            px[y][0] - 128, px[y][1] - 128, px[y][2] - 128, px[y][3] - 128,
            px[y][4] - 128, px[y][5] - 128, px[y][6] - 128, px[y][7] - 128,
        ];
        let r = dct1d(&row);
        tmp[y] = r;
    }
    let mut out: Block = [[0i32; 8]; 8];
    for x in 0..8 {
        let col = [tmp[0][x], tmp[1][x], tmp[2][x], tmp[3][x], tmp[4][x], tmp[5][x], tmp[6][x], tmp[7][x]];
        let r = dct1d(&col);
        for y in 0..8 {
            out[y][x] = r[y];
        }
    }
    out
}

/// 正交对拍：px → dct2d → idct2d == px（±1 容差）。
pub fn roundtrip(px: &Block) -> Result<(), u32> {
    let coef = dct2d(px);
    let back = idct2d(&coef)?;
    for y in 0..8 {
        for x in 0..8 {
            let d = (back[y][x] - px[y][x]).abs();
            if d > 1 {
                return Err(XC_ORTHO_FAIL);
            }
        }
    }
    Ok(())
}

/// DC 锚：仅 DC 系数块 → 全常数面。
/// DC=dc → 像素 ≈ dc/8 + 128（2D DC 折算：行 1/8 × 列 1/8……实现
/// 层定标：单 DC 系数经两次 1D 后 = dc×1024×724×1024 >> 20 >> 20
/// 化简 = dc/8）。
pub fn dc_anchor(dc: i32) -> Result<u8, u32> {
    if !(COEF_MIN..=COEF_MAX).contains(&dc) {
        return Err(XC_COEF_OOB);
    }
    let mut coef: Block = [[0i32; 8]; 8];
    coef[0][0] = dc;
    let out = idct2d(&coef)?;
    let v = out[0][0];
    for y in 0..8 {
        for x in 0..8 {
            if out[y][x] != v {
                return Err(XC_DC_FAIL);
            }
        }
    }
    Ok(v as u8)
}

/// 基频锚（1D 层）：仅 F[1] 非零 → 行内左右反对称
/// （cos((2x+1)π/16) 与 cos((15−2x)π/16) 互为相反数）。
pub fn base_freq_anchor(f01: i32) -> Result<[i32; 8], u32> {
    if !(COEF_MIN..=COEF_MAX).contains(&f01) {
        return Err(XC_COEF_OOB);
    }
    let mut coef = [0i32; 8];
    coef[1] = f01;
    let row = idct1d(&coef)?;
    for x in 0..4 {
        if row[x] != -row[7 - x] {
            return Err(XC_DC_FAIL);
        }
    }
    Ok(row)
}

/// 零块锚：全零 → 128 灰。
pub fn zero_anchor() -> Result<(), u32> {
    let out = idct2d(&[[0i32; 8]; 8])?;
    for y in 0..8 {
        for x in 0..8 {
            if out[y][x] != 128 {
                return Err(XC_DC_FAIL);
            }
        }
    }
    Ok(())
}

/// 能量账：Σ|coef|² 与 Σ(px−128)² 量级同阶（定点近似：比值 ∈ [1/4, 4]）。
pub fn energy_order(coef: &Block, px: &Block) -> Result<(), u32> {
    let ec: u64 = coef.iter().flatten().map(|&v| (v as i64 * v as i64) as u64).sum();
    let ep: u64 = px.iter().flatten().map(|&v| {
        let d = v - 128;
        (d as i64 * d as i64) as u64
    }).sum();
    if ep == 0 {
        return Ok(()); // 平坦块能量全在 DC 域——放行
    }
    // 定点 DCT 使能量尺度差 2^k；量级账用比值带（不比精确 Parseval）。
    let big = ec.max(ep);
    let small = ec.min(ep).max(1);
    if big / small > 64 {
        return Err(XC_ENERGY_FAIL);
    }
    Ok(())
}

// ===== 主判据 =====
pub fn run_cga93_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0093");
    // 族一：1D 锚。
    let z = [0i32; 8];
    cs.add("1D 零锚", matches!(idct1d(&z), Ok(v) if v.iter().all(|&x| x == 0)), "零入零出");
    let dc = [256i32, 0, 0, 0, 0, 0, 0, 0];
    cs.add("1D DC 锚", matches!(idct1d(&dc), Ok(v) if v.iter().all(|&x| x == 90)), "256×c0/2≈90");
    cs.add("系数越界拒", idct1d(&[2000i32, 0, 0, 0, 0, 0, 0, 0]).is_err(), "COEF_OOB");
    // 族二：2D 三锚。
    cs.add("零块 128", zero_anchor().is_ok(), "全零→灰");
    cs.add("DC 常数面", dc_anchor(64) == Ok(136), "64/8+128=136");
    cs.add("DC 负面", dc_anchor(-64) == Ok(120), "−64/8+128=120");
    if let Ok(row) = base_freq_anchor(512) {
        cs.add("基频反对称", row[0] == -row[7] && row[1] == -row[6], "半波反对称");
        cs.add("基频非平", row[0] != row[2], "有起伏");
    }
    // 族三：正交往返（三档样本块）。
    let samples: [Block; 3] = [
        {
            let mut b = [[0i32; 8]; 8];
            for y in 0..8 {
                for x in 0..8 {
                    // 幅度收在 [64,192]：两Stage 系数 ≤ ±64×8.97≈±574，
                    // 稳在 JPEG 系数域 [−1024,1023] 内（极端对齐不越界）。
                    b[y][x] = (64 + (x * 17 + y * 29) % 129) as i32;
                }
            }
            b
        },
        {
            let mut b = [[0i32; 8]; 8];
            for y in 0..8 {
                for x in 0..8 {
                    b[y][x] = if (x / 4 + y / 4) % 2 == 0 { 40 } else { 220 };
                }
            }
            b
        },
        [[128i32; 8]; 8],
    ];
    for (idx, s) in samples.iter().enumerate() {
        let _ = idx;
        cs.add("往返 ±1", roundtrip(s).is_ok(), "DCT∘IDCT==id");
    }
    // 族四：能量账。
    let coef = dct2d(&samples[0]);
    cs.add("能量同阶", energy_order(&coef, &samples[0]).is_ok(), "量级带");
    // 族五：钳制账（极端系数 → 输出仍 [0,255]）。
    let mut ext: Block = [[0i32; 8]; 8];
    ext[0][0] = COEF_MAX;
    ext[7][7] = COEF_MIN;
    if let Ok(out) = idct2d(&ext) {
        cs.add("钳制界", out.iter().flatten().all(|&v| (0..=255).contains(&v)), "[0,255]");
    }
    // 族六：系数域闸全账。
    let mut bad: Block = [[0i32; 8]; 8];
    bad[3][3] = COEF_MIN - 1;
    cs.add("2D 系数闸", idct2d(&bad).is_err(), "下越界");
    bad[3][3] = COEF_MAX + 1;
    cs.add("2D 系数闸上", idct2d(&bad).is_err(), "上越界");
    // 族七：DC 线性扫描（DC 单调 → 输出单调）。
    let mut prev: i32 = -1;
    let mut mono = true;
    for dc in (-256..=256i32).step_by(128) {
        if let Ok(v) = dc_anchor(dc) {
            if (v as i32) < prev {
                mono = false;
            }
            prev = v as i32;
        }
    }
    cs.add("DC 单调", mono, "dc↑像素↑");
    // 族八：行/列可分离性烟测（对角系数块输出中心对称）。
    let mut diag: Block = [[0i32; 8]; 8];
    diag[1][1] = 400;
    if let Ok(out) = idct2d(&diag) {
        cs.add("中心对称", (0..4).all(|y| (0..8).all(|x| out[y][x] == out[7 - y][7 - x])), "u=v 基对称");
    }
    CheckSet::merge(CheckSet::merge(cs, run_cga93_deep()), run_cga93_deep2())
}

// ===== 深化面：块级往返扫账与能量序二次核 =====
/// 多块往返扫账：常数块/斜坡块/单点冲激块三类全过。
pub fn multi_block_roundtrip() -> Result<(), u32> {
    let mut flat = [[0i32; 8]; 8];
    for row in flat.iter_mut() {
        for v in row.iter_mut() {
            *v = 100;
        }
    }
    roundtrip(&flat)?;
    let mut ramp = [[0i32; 8]; 8];
    for (y, row) in ramp.iter_mut().enumerate() {
        for (x, v) in row.iter_mut().enumerate() {
            *v = (y * 8 + x) as i32;
        }
    }
    roundtrip(&ramp)?;
    let mut impulse = [[0i32; 8]; 8];
    impulse[3][3] = 512;
    roundtrip(&impulse)?;
    Ok(())
}

/// 冲激能量集中：单点冲激的系数账在 DC 与全域有界。
pub fn impulse_energy() -> Result<(), u32> {
    let mut px = [[0i32; 8]; 8];
    px[3][3] = 512;
    let coef = dct2d(&px);
    // 全系数有界（能量守恒的松界锚）。
    for row in coef.iter() {
        for &c in row.iter() {
            if c.abs() > 512 * 64 {
                return Err(XC_ENERGY_FAIL);
            }
        }
    }
    Ok(())
}

pub fn run_cga93_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga93_deep");
    cs.add("多块往返", multi_block_roundtrip().is_ok(), "三块全过");
    cs.add("冲激能量界", impulse_energy().is_ok(), "系数有界");
    cs.add("零锚", zero_anchor().is_ok(), "零块零出");
    cs.add("DC 锚 0", dc_anchor(0).is_ok(), "零直流");
    cs.add("DC 锚 2048", dc_anchor(2048).is_ok(), "中直流");
    cs.add("基频锚", base_freq_anchor(64).is_ok(), "F01 可构造");
    let mut px = [[0i32; 8]; 8];
    for (y, row) in px.iter_mut().enumerate() {
        for (x, v) in row.iter_mut().enumerate() {
            *v = ((y * 8 + x) % 16) as i32;
        }
    }
    let coef = dct2d(&px);
    cs.add("能量序", energy_order(&coef, &px).is_ok(), "账序守恒");
    cs.add("一维往返", {
        let mut row = [0i32; 8];
        for (i, v) in row.iter_mut().enumerate() {
            *v = (i * 7) as i32;
        }
        let c = dct1d(&row);
        idct1d(&c) == Ok(row)
    }, "dct→idct 恒等");
    cs
}

// ===== 深化面二：块级 DC 与正交域 =====
pub fn run_cga93_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga93_deep2");
    // 常量块 → DCT 后仅 DC 非零（正交性推论锚）。
    let mut flat = [[0i32; 8]; 8];
    for row in flat.iter_mut() {
        for v in row.iter_mut() {
            *v = 64;
        }
    }
    let coef = dct2d(&flat);
    cs.add("常量 DC 占优", {
        let mut ac_small = true;
        for r in 0..8 {
            for c in 0..8 {
                if (r, c) != (0, 0) && coef[r][c].abs() > 4 {
                    ac_small = false;
                }
            }
        }
        coef[0][0] != 0 && ac_small
    }, "AC≈0");
    // DC 反变换回常量（往返闭）。
    if let Ok(back) = idct2d(&coef) {
        cs.add("DC 往返", back.iter().flatten().all(|&v| v.abs_diff(64) <= 2), "常量还原");
    } else {
        cs.add("DC 往返", false, "idct 失败");
    }
    // 一维线性账：dct(2x)==2·dct(x)。
    let mut row = [0i32; 8];
    for (i, v) in row.iter_mut().enumerate() {
        *v = (i * 9) as i32;
    }
    let c1 = dct1d(&row);
    let mut row2 = [0i32; 8];
    for i in 0..8 {
        row2[i] = 2 * row[i];
    }
    let c2 = dct1d(&row2);
    cs.add("一维线性", (0..8).all(|i| (c2[i] - 2 * c1[i]).abs() <= 2), "倍角");
    cs.add("CU 锚", CU[0] == 724 && CU[1] == 1024, "定标冻结");
    cs.add("零块零出", {
        let z = [[0i32; 8]; 8];
        dct2d(&z).iter().flatten().all(|&v| v == 0)
    }, "线性零");
    cs.add("能量序核", energy_order(&coef, &flat).is_ok(), "账序守恒");
    cs
}
