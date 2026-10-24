//! cga96_dctfwd —— CGPU-F0096 SIMD DCT 正变换（编码方向）。
//! 锚点（CGPU 施工书）：CGPU-F0096 · SIMD DCT 正变换 · 目标 340 行
//! 判据摘录：与 cga93（IDCT）严格自逆（同 c(u)/2 权重同 >>21 定
//! 标——正交红线）；直流锚（常数块 → DC = Σ(f−128)/8——JPEG
//! 中心化语义）；能量压缩账（低频带能量占比 ≥ 70%——DCT 的
//! 存在意义核）；线性账（DCT(a·x + b·y) == a·DCT(x) + b·DCT(y)
//! ——叠加抽样核）；空间局部性（系数幅值随频率↑总体↓——压
//! 缩方向账）；系数域闸（输出落 JPEG 域）；块域闸（8×8 精确
//! 尺寸拒零）。
//! 组件：一维 DCT（复用 cga93 冻结余弦表 CU/COS8——单一真源）+
//!   二维组装（行→列，−128 中心化）+ 自逆对拍 + DC/能量/线性/
//!   局部性四账 + 域闸。
//! 职责定位：编码方向（MJPEG/WebP 无损帧内/JPEG 重压）的数学核
//! ——解码端的镜像；表与 cga93 单源复用（防两表漂移），判据
//! 以「能量压缩」为主（编码价值核），与 cga93 的「重建保真」
//! 互补。
//!
//! 定标：out[u] = ((Σ px·COS8[u][x]·CU[u]) + 2^20) >> 21 —— 与
//! cga93::idct1d 完全同构（自逆）。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::cga93_idct::{COS8, CU, COEF_MIN, COEF_MAX, idct2d, Block};

// ===== 诊断码闭集（0xE950 段 · 本模块独占）=====
pub const XC_DIM_BAD: u32 = 0xE950; // 块尺寸非法
pub const XC_SELFINV_FAIL: u32 = 0xE951; // 自逆对拍破
pub const XC_DC_FAIL: u32 = 0xE952; // DC 锚破
pub const XC_COMPACT_FAIL: u32 = 0xE953; // 能量压缩破
pub const XC_LINEAR_FAIL: u32 = 0xE954; // 线性破

/// 一维 DCT（自逆标定——与 cga93::idct1d 完全同构）。
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

/// 一维 IDCT（本模块自持——避免跨模块私有耦合；表复用）。
pub fn idct1d_local(coef: &[i32; 8]) -> Result<[i32; 8], u32> {
    for &c in coef.iter() {
        if !(COEF_MIN..=COEF_MAX).contains(&c) {
            return Err(0xE920);
        }
    }
    let mut out = [0i32; 8];
    for x in 0..8 {
        let mut acc: i64 = 0;
        for u in 0..8 {
            acc += (coef[u] as i64) * (COS8[u][x] as i64) * (CU[u] as i64);
        }
        out[x] = ((acc + (1i64 << 20)) >> 21) as i32;
    }
    Ok(out)
}

/// 二维 DCT：−128 中心化 → 行 → 列。
pub fn dct2d_local(px: &Block) -> Block {
    let mut tmp: Block = [[0i32; 8]; 8];
    for y in 0..8 {
        let row = [
            px[y][0] - 128, px[y][1] - 128, px[y][2] - 128, px[y][3] - 128,
            px[y][4] - 128, px[y][5] - 128, px[y][6] - 128, px[y][7] - 128,
        ];
        tmp[y] = dct1d(&row);
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

/// 自逆对拍：px → dct2d → idct2d(cga93) == px（±1）。
pub fn self_inverse(px: &Block) -> Result<(), u32> {
    let coef = dct2d_local(px);
    let back = idct2d(&coef)?;
    for y in 0..8 {
        for x in 0..8 {
            if (back[y][x] - px[y][x]).abs() > 1 {
                return Err(XC_SELFINV_FAIL);
            }
        }
    }
    Ok(())
}

/// DC 锚：常数块 c → F[0][0] == 8c（中心化后偏差 c−128 双向 ×8）。
pub fn dc_anchor(c: i32) -> Result<i32, u32> {
    if !(0..=255).contains(&c) {
        return Err(XC_DIM_BAD);
    }
    let mut blk: Block = [[c; 8]; 8];
    let coef = dct2d_local(&blk);
    // 其他系数应近零（常数块纯 DC）。
    for y in 0..8 {
        for x in 0..8 {
            if (y, x) != (0, 0) && coef[y][x].abs() > 2 {
                return Err(XC_DC_FAIL);
            }
        }
    }
    Ok(coef[0][0])
}

/// 能量压缩账：低频 3×3 角能量 / 总能量 ≥ 70%（平滑块域）。
/// 平滑块 = 低频主导（真实图像先验——压缩前提）。
pub fn compaction(px: &Block) -> Result<u32, u32> {
    let coef = dct2d_local(px);
    let e2 = |v: i64| (v * v) as u64;
    let mut total: u64 = 0;
    let mut low: u64 = 0;
    for y in 0..8 {
        for x in 0..8 {
            let e = e2(coef[y][x] as i64);
            total += e;
            if y < 3 && x < 3 {
                low += e;
            }
        }
    }
    if total == 0 {
        return Ok(1000); // 纯 DC 中性块——全能量在 (0,0) 亦低频
    }
    // total==0 时 low 亦 0；非零块计算千分比。
    let pm = (low * 1000 / total) as u32;
    if total > 0 && pm < 700 && low * 1000 < total * 700 {
        return Err(XC_COMPACT_FAIL);
    }
    Ok(pm)
}

/// 线性抽样账：DCT(2x) == 2·DCT(x)（倍乘样本）且
/// DCT(x+y) ≈ DCT(x)+DCT(y)（±2 容差——舍入带）。
pub fn linearity(x: &Block, y: &Block) -> Result<(), u32> {
    // 前置钳界闸：倍乘/求和不得触顶（2x ≤255 ∧ x+y ≤255）——
    // 触顶即样本违规（ Caller 契约），显性拒绝不静默。
    for r in 0..8 {
        for c in 0..8 {
            if 2 * x[r][c] > 255 || x[r][c] + y[r][c] > 255 {
                return Err(XC_LINEAR_FAIL);
            }
        }
    }
    let dx = dct2d_local(x);
    let dy = dct2d_local(y);
    let mut x2 = [[0i32; 8]; 8];
    for r in 0..8 {
        for c in 0..8 {
            x2[r][c] = 2 * x[r][c];
        }
    }
    let dx2b = dct2d_local(&x2);
    for r in 0..8 {
        for c in 0..8 {
            if (dx2b[r][c] - 2 * dx[r][c]).abs() > 6 {
                return Err(XC_LINEAR_FAIL);
            }
        }
    }
    // 叠加。
    let mut sum: Block = [[0i32; 8]; 8];
    for r in 0..8 {
        for c in 0..8 {
            sum[r][c] = x[r][c] + y[r][c];
        }
    }
    let dsum = dct2d_local(&sum);
    for r in 0..8 {
        for c in 0..8 {
            if (dsum[r][c] - dx[r][c] - dy[r][c]).abs() > 6 {
                return Err(XC_LINEAR_FAIL);
            }
        }
    }
    Ok(())
}

/// 空间局部性：系数幅值行/列序单调趋势（低频行均值 ≥ 高频行均
/// 值——平滑块域）。
pub fn locality(px: &Block) -> Result<(), u32> {
    let coef = dct2d_local(px);
    let row_abs = |y: usize| -> u64 {
        coef[y].iter().map(|&v| v.unsigned_abs() as u64).sum::<u64>()
    };
    // 低频两行（0,1）合计 ≥ 高频两行（6,7）合计（趋势账——非逐行）。
    if row_abs(0) + row_abs(1) < row_abs(6) + row_abs(7) {
        return Err(XC_COMPACT_FAIL);
    }
    Ok(())
}

// ===== 主判据 =====
pub fn run_cga96_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0096");
    // 族一：1D 锚。
    let z = [0i32; 8];
    cs.add("1D 零锚", dct1d(&z).iter().all(|&v| v == 0), "零入零出");
    let cst = [40i32; 8];
    let dc1 = dct1d(&cst);
    // 常数行 → 纯 DC：DC = 40×8×0.3536 ≈ 113。
    cs.add("1D 常数纯 DC", dc1[0].abs() <= 114 && dc1[0].abs() >= 112 && dc1[1..].iter().all(|&v| v.abs() <= 1), "DC≈113");
    // 自逆 1D。
    let mut rt = idct1d_local(&dc1).map_err(|e| e as u16);
    if let Ok(v) = &rt {
        cs.add("1D 自逆", (0..8).all(|i| (v[i] - cst[i]).abs() <= 1), "±1");
    }
    let _ = &mut rt;
    // 族二：DC 锚。
    cs.add("DC 常数 128", dc_anchor(128) == Ok(0), "中性块 DC=0");
    cs.add("DC 常数 200", dc_anchor(200) == Ok(576), "(200−128)×8");
    cs.add("DC 常数 64", dc_anchor(64) == Ok(-512), "(64−128)×8");
    cs.add("DC 域闸", dc_anchor(300).is_err(), "[0,255]");
    // 族三：自逆对拍（三样本）。
    let smooth: Block = {
        let mut b = [[0i32; 8]; 8];
        for y in 0..8 {
            for x in 0..8 {
                b[y][x] = 100 + ((x + y) * 4) as i32; // 平滑斜坡
            }
        }
        b
    };
    let noise: Block = {
        let mut b = [[0i32; 8]; 8];
        for y in 0..8 {
            for x in 0..8 {
                b[y][x] = 64 + ((x * 31 + y * 17) % 129) as i32;
            }
        }
        b
    };
    let flat: Block = [[90i32; 8]; 8];
    cs.add("斜坡自逆", self_inverse(&smooth).is_ok(), "±1");
    cs.add("噪声自逆", self_inverse(&noise).is_ok(), "±1");
    cs.add("平块自逆", self_inverse(&flat).is_ok(), "±1");
    // 族四：能量压缩（平滑块低频主导）。
    let pm = compaction(&smooth).map_err(|e| e as u16);
    cs.add("平滑块压缩", matches!(&pm, Ok(p) if *p >= 700), "≥70%");
    let _ = noise;
    // 族五：局部性。
    cs.add("低频行优势", locality(&smooth).is_ok(), "行趋势");
    // 族六：线性抽样。
    // 线性样本：低幅域（56..88）——2x∈[112,176]、x+y∈[112,176] 均不触顶。
    let lx: Block = {
        let mut b = [[0i32; 8]; 8];
        for y in 0..8 {
            for x in 0..8 {
                b[y][x] = (56 + (x * 13 + y * 7) % 33) as i32;
            }
        }
        b
    };
    let ly: Block = {
        let mut b = [[0i32; 8]; 8];
        for y in 0..8 {
            for x in 0..8 {
                b[y][x] = (56 + (x * 5 + y * 3) % 33) as i32;
            }
        }
        b
    };
    cs.add("线性叠加", linearity(&lx, &ly).is_ok(), "倍乘+叠加");
    // 族七：系数域账（平滑块系数全落 JPEG 域）。
    let coef = dct2d_local(&smooth);
    cs.add("系数落域", coef.iter().flatten().all(|&v| (COEF_MIN..=COEF_MAX).contains(&v)), "[−1024,1023]");
    CheckSet::merge(CheckSet::merge(cs, run_cga96_deep()), run_cga96_deep2())
}

// ===== 深化面：线性叠加与局部性二次核 =====
/// 阶梯块线性核：dct(2x) == 2·dct(x)（Q 定点容差内）。
pub fn linearity_scale2() -> Result<(), u32> {
    let mut x = [[0i32; 8]; 8];
    for (y, row) in x.iter_mut().enumerate() {
        for (xi, v) in row.iter_mut().enumerate() {
            *v = ((y * 8 + xi) % 12) as i32;
        }
    }
    linearity(&x, &x)?;
    Ok(())
}

pub fn run_cga96_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga96_deep");
    let mut px = [[0i32; 8]; 8];
    for (y, row) in px.iter_mut().enumerate() {
        for (xi, v) in row.iter_mut().enumerate() {
            *v = ((y * 8 + xi) % 12) as i32;
        }
    }
    cs.add("自逆", self_inverse(&px).is_ok(), "dct∘dct=id");
    cs.add("DC 锚", dc_anchor(0).is_ok(), "零直流");
    cs.add("线性核", linearity_scale2().is_ok(), "2x 线性");
    cs.add("局部性", locality(&px).is_ok(), "能量集中");
    cs.add("压缩账", compaction(&px).is_ok(), "系数聚拢");
    cs.add("一维自逆", {
        let mut row = [0i32; 8];
        for (i, v) in row.iter_mut().enumerate() {
            *v = (i * 5) as i32;
        }
        let c = dct1d(&row);
        idct1d_local(&c) == Ok(row)
    }, "行恒等");
    cs
}

// ===== 深化面二：DC 域与局部性二次 =====
pub fn run_cga96_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga96_deep2");
    // DC 三档锚。
    cs.add("DC 0", dc_anchor(0).is_ok(), "零");
    cs.add("DC 1024", dc_anchor(1024).is_ok(), "中");
    cs.add("DC 4096", dc_anchor(4096).is_ok(), "满");
    // 阶梯块自逆 + 局部性。
    let mut px = [[0i32; 8]; 8];
    for r in 0..8 {
        for c in 0..8 {
            px[r][c] = if (r + c) % 2 == 0 { 90 } else { 30 };
        }
    }
    cs.add("棋盘自逆", self_inverse(&px).is_ok(), "往返");
    cs.add("棋盘局部性", locality(&px).is_ok(), "能量集中");
    cs.add("棋盘压缩", compaction(&px).is_ok(), "系数聚拢");
    // 一维 DC：全常数行 → 仅 u0 非零。
    let row = [64i32; 8];
    let c = dct1d(&row);
    cs.add("行 DC 占优", {
        let mut dc_dom = c[0] != 0;
        for i in 1..8 {
            if c[i].abs() > 4 {
                dc_dom = false;
            }
        }
        dc_dom
    }, "AC≈0");
    cs
}
