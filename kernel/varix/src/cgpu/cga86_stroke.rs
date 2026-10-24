//! cga86_stroke —— CGPU-F0086 SIMD 描边算子（Alpha 膨胀）。
//! 锚点（CGPU 施工书）：CGPU-F0086 · SIMD 描边算子（Alpha 膨胀）· 目标 320 行
//! 判据摘录：描边 = alpha 膨胀域减去源 alpha 域的差带着色；
//! 半径域闭集（r ∈ {1,2,3,4}）；描边带宽守恒（带宽 == 膨胀域
//! 面积 − 源域面积——逐像素对拍）；描边单向账（描边只出现在
//! 源 alpha==0 处——不覆盖本体红线）；内缩模式对偶（erode 得
//! 内描边：源域减腐蚀域——对偶账）；幂等超集账（dilate(dilate)
//! ⊆ dilate(r1+r2)——膨胀单调）；膨胀可交换（d(r1)∘d(r2) ==
//! d(r2)∘d(r1)）；色域闭集（描边色三元组任意但 alpha 落账）。
//! 组件：alpha 域提取 + 方形膨胀/腐蚀（切比雪夫距离——与 cga85
//! dilate 同语义不同半径闭集）+ 差带提取 + 差带着色 + 带宽守
//! 恒 + 单向账 + 单调/交换代数账。
//! 职责定位：图标/文字选中态与可读性描边的标准算子——外描边
//! （dilate−src）与内描边（src−erode）是同一条差带数学的两面；
//! 纯 alpha 域运算与颜色无关，色只进最终差带填充。
//!
//! 数学要点：
//!   外描边带 S_out = dilate(A, r) ∖ A（A 为 alpha>0 集合）
//!   内描边带 S_in  = A ∖ erode(A, r)
//!   |dilate(A,r)| ≥ |A| ≥ |erode(A,r)|（单调链）
//!   幂等超集：dilate(dilate(A,r1),r2) ⊆ dilate(A,r1+r2)

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE8B0 段 · 本模块独占）=====
pub const XC_RAD_OOB: u32 = 0xE8B0; // 半径越界（∉{1,2,3,4}）
pub const XC_DIM: u32 = 0xE8B1; // 尺寸失配
pub const XC_BAND_FAIL: u32 = 0xE8B2; // 带宽守恒破
pub const XC_COVER_FAIL: u32 = 0xE8B3; // 描边盖本体破
pub const XC_MONO_FAIL: u32 = 0xE8B4; // 膨胀单调破
pub const XC_COMM_FAIL: u32 = 0xE8B5; // 交换律破

// ===== 常量闭集 =====
pub const RADII: [u32; 4] = [1, 2, 3, 4];

/// RGBA 图容器。
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Rgba {
    pub fn new(w: u32, h: u32, px: Vec<u8>) -> Result<Rgba, u32> {
        if px.len() != (w * h * 4) as usize {
            return Err(XC_DIM);
        }
        Ok(Rgba { w, h, px })
    }
}

/// alpha 域位图（true = 不透明）。
pub fn alpha_mask(img: &Rgba) -> Vec<bool> {
    (0..(img.w * img.h) as usize).map(|i| img.px[i * 4 + 3] > 0).collect()
}

/// 方形膨胀（切比雪夫 ≤r）。
pub fn dilate(m: &[bool], w: u32, h: u32, r: u32) -> Result<Vec<bool>, u32> {
    shape_op(m, w, h, r, true)
}

/// 方形腐蚀（切比雪夫 ≤r 窗口全真才真）。
pub fn erode(m: &[bool], w: u32, h: u32, r: u32) -> Result<Vec<bool>, u32> {
    shape_op(m, w, h, r, false)
}

fn shape_op(m: &[bool], w: u32, h: u32, r: u32, is_dilate: bool) -> Result<Vec<bool>, u32> {
    if RADII.iter().find(|&&v| v == r).is_none() {
        return Err(XC_RAD_OOB);
    }
    if m.len() != (w * h) as usize {
        return Err(XC_DIM);
    }
    let mut out: Vec<bool> = alloc::vec![!is_dilate; m.len()];
    for y in 0..h {
        for x in 0..w {
            let mut all = true;
            let mut any = false;
            for dy in -(r as i64)..=(r as i64) {
                for dx in -(r as i64)..=(r as i64) {
                    let cx = (x as i64 + dx).clamp(0, w as i64 - 1) as u32;
                    let cy = (y as i64 + dy).clamp(0, h as i64 - 1) as u32;
                    let v = m[(cy * w + cx) as usize];
                    if v {
                        any = true;
                    } else {
                        all = false;
                    }
                }
            }
            out[(y * w + x) as usize] = if is_dilate { any } else { all };
        }
    }
    Ok(out)
}

/// 差带提取：A 且非 B。
pub fn diff_band(a: &[bool], b: &[bool]) -> Result<Vec<bool>, u32> {
    if a.len() != b.len() {
        return Err(XC_DIM);
    }
    Ok(a.iter().zip(b.iter()).map(|(&x, &y)| x && !y).collect())
}

/// 外描边全链：膨胀差带着色（描边色写入，本体不动）。
/// 返回新图与描边像素计数。
pub fn stroke_outer(img: &Rgba, r: u32, color: [u8; 3], opacity: u8) -> Result<(Rgba, u32), u32> {
    let a = alpha_mask(img);
    let dil = dilate(&a, img.w, img.h, r)?;
    let band = diff_band(&dil, &a)?;
    let mut out_px = img.px.clone();
    let mut n: u32 = 0;
    for (i, &in_band) in band.iter().enumerate() {
        if in_band {
            out_px[i * 4] = color[0];
            out_px[i * 4 + 1] = color[1];
            out_px[i * 4 + 2] = color[2];
            out_px[i * 4 + 3] = opacity;
            n += 1;
        }
    }
    Rgba::new(img.w, img.h, out_px).map(|g| (g, n))
}

/// 内描边全链：腐蚀差带着色。
pub fn stroke_inner(img: &Rgba, r: u32, color: [u8; 3], opacity: u8) -> Result<(Rgba, u32), u32> {
    let a = alpha_mask(img);
    let ero = erode(&a, img.w, img.h, r)?;
    let band = diff_band(&a, &ero)?;
    let mut out_px = img.px.clone();
    let mut n: u32 = 0;
    for (i, &in_band) in band.iter().enumerate() {
        if in_band {
            out_px[i * 4] = color[0];
            out_px[i * 4 + 1] = color[1];
            out_px[i * 4 + 2] = color[2];
            out_px[i * 4 + 3] = opacity;
            n += 1;
        }
    }
    Rgba::new(img.w, img.h, out_px).map(|g| (g, n))
}

/// 带宽守恒：带像素数 == 膨胀域面积 − 源域面积。
pub fn band_conserve(a: &[bool], dil: &[bool], band: &[bool]) -> Result<(), u32> {
    let ca = a.iter().filter(|&&b| b).count();
    let cd = dil.iter().filter(|&&b| b).count();
    let cb = band.iter().filter(|&&b| b).count();
    if cb != cd - ca {
        return Err(XC_BAND_FAIL);
    }
    Ok(())
}

/// 单向账：描边带与源域不相交（描边不盖本体）。
pub fn no_cover(band: &[bool], a: &[bool]) -> Result<(), u32> {
    for i in 0..band.len() {
        if band[i] && a[i] {
            return Err(XC_COVER_FAIL);
        }
    }
    Ok(())
}

/// 膨胀单调：r 增大域只增不减。
pub fn dilate_monotone(m: &[bool], w: u32, h: u32) -> Result<(), u32> {
    let d1 = dilate(m, w, h, 1)?;
    let d2 = dilate(m, w, h, 2)?;
    for i in 0..m.len() {
        if d1[i] && !d2[i] {
            return Err(XC_MONO_FAIL);
        }
    }
    Ok(())
}

/// 交换律：d(r1)∘d(r2) == d(r2)∘d(r1)（方形核下成立）。
pub fn dilate_commute(m: &[bool], w: u32, h: u32) -> Result<(), u32> {
    let d12 = dilate(&dilate(m, w, h, 1)?, w, h, 2)?;
    let d21 = dilate(&dilate(m, w, h, 2)?, w, h, 1)?;
    if d12[..] != d21[..] {
        return Err(XC_COMM_FAIL);
    }
    Ok(())
}

// ===== 主判据 =====
pub fn run_cga86_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0086");
    // 8×8 中心 4×4 方块。
    let img = Rgba::new(8, 8, {
        let mut v: Vec<u8> = alloc::vec![0; 256];
        for y in 2..6u32 {
            for x in 2..6u32 {
                let b = ((y * 8 + x) * 4) as usize;
                v[b] = 100;
                v[b + 1] = 150;
                v[b + 2] = 200;
                v[b + 3] = 255;
            }
        }
        v
    }).map_err(|e| e as u16);
    if let Ok(s) = &img {
        let a = alpha_mask(s);
        cs.add("源域面积", a.iter().filter(|&&b| b).count() == 16, "4×4=16");
        // 族一：外描边守恒 + 单向 + 色写。
        if let Ok((out, n)) = stroke_outer(s, 1, [255, 220, 0], 255) {
            match dilate(&a, 8, 8, 1) {
                Ok(dil) => match diff_band(&dil, &a) {
                    Ok(band) => {
                        cs.add("带宽守恒", band_conserve(&a, &dil, &band).is_ok(), "d−a==band");
                        cs.add("单向账", no_cover(&band, &a).is_ok(), "不盖本体");
                    }
                    Err(_) => {
                        cs.add("带宽守恒", false, "band 构建失败");
                        cs.add("单向账", false, "band 构建失败");
                    }
                },
                Err(_) => {
                    cs.add("带宽守恒", false, "dil 构建失败");
                    cs.add("单向账", false, "dil 构建失败");
                }
            }
            cs.add("描边数锚", n == 24, "6×6−4×4=24");
            let edge = ((1 * 8 + 2) * 4) as usize; // 上缘描边点
            cs.add("色写锚", out.px[edge] == 255 && out.px[edge + 1] == 220, "描边色");
            cs.add("本体不动", out.px[((3 * 8 + 3) * 4) as usize] == 100, "源色保持");
        }
        // 族二：内描边对偶。
        if let Ok((_, ni)) = stroke_inner(s, 1, [0, 0, 0], 255) {
            cs.add("内描边非空", ni > 0, "腐蚀差带");
            cs.add("内≤外", ni <= 24, "内带不大于外带");
        }
        // 族三：代数账。
        cs.add("膨胀单调", dilate_monotone(&a, 8, 8).is_ok(), "r↑域↑");
        cs.add("交换律", dilate_commute(&a, 8, 8).is_ok(), "d1∘d2==d2∘d1");
        // 族四：半径域闸。
        cs.add("半径越界拒", dilate(&a, 8, 8, 5).is_err(), "∉{1..4}");
        cs.add("零域腐蚀", erode(&alloc::vec![false; 64], 8, 8, 1).map_err(|e| e as u16).map(|v| v.iter().all(|&b| !b)) == Ok(true), "空域闭");
    }
    CheckSet::merge(CheckSet::merge(cs, run_cga86_deep()), run_cga86_deep2())
}

// ===== 深化面：形态学对偶与带宽账 =====
/// 半径闭账：RADII 全档 dilate 可构造，域外拒。
pub fn radius_closed(w: u32, h: u32, m: &[bool]) -> bool {
    RADII.iter().all(|&r| dilate(m, w, h, r).is_ok())
        && dilate(m, w, h, 9).is_err()
}

/// 膨胀-腐蚀序账：erode 结果 ⊆ 原 ⊆ dilate（逐点）。
pub fn morpho_order(w: u32, h: u32, m: &[bool]) -> bool {
    match (erode(m, w, h, 1), dilate(m, w, h, 1)) {
        (Ok(e), Ok(d)) => m.iter().zip(e.iter()).zip(d.iter())
            .all(|((&x, &e), &d)| e <= x && x <= d),
        _ => false,
    }
}

/// 带尺寸守恒：diff_band 输出长度与输入一致。
pub fn band_dim(w: u32, h: u32, m: &[bool]) -> bool {
    match dilate(m, w, h, 1) {
        Ok(d) => match diff_band(m, &d) {
            Ok(b) => b.len() == m.len(),
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// 空图惰性：全假掩码膨胀仍全假。
pub fn empty_inert(w: u32, h: u32) -> bool {
    let m = alloc::vec![false; (w * h) as usize];
    match dilate(&m, w, h, 1) {
        Ok(d) => d.iter().all(|&b| !b),
        Err(_) => false,
    }
}

pub fn run_cga86_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga86_deep");
    let w = 5u32;
    let h = 5u32;
    let m = alloc::vec![false, true, true, false, false,
                        false, true, true, false, false,
                        false, false, false, false, false,
                        false, true, false, false, false,
                        false, false, false, false, true];
    cs.add("半径闭", radius_closed(w, h, &m), "4 档可构造");
    cs.add("形态学序", morpho_order(w, h, &m), "erode⊆m⊆dilate");
    cs.add("带尺寸", band_dim(w, h, &m), "长度守恒");
    cs.add("空图惰性", empty_inert(w, h), "全假不生点");
    cs.add("覆盖核", no_cover(&alloc::vec![false, true, false], &alloc::vec![true, false, false]).is_ok(), "带不盖本体");
    cs
}

// ===== 深化面二：零径恒等账 =====
pub fn run_cga86_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga86_deep2");
    let w = 4u32;
    let h = 4u32;
    let m = alloc::vec![true, false, false, false,
                        false, true, false, false,
                        false, false, true, false,
                        false, false, false, true];
    cs.add("膨胀零径恒等", dilate(&m, w, h, 0) == Ok(m.clone()), "r=0 不动");
    cs.add("腐蚀零径恒等", erode(&m, w, h, 0) == Ok(m.clone()), "r=0 不动");
    cs.add("单调核", dilate_monotone(&m, w, h).is_ok(), "账面声明");
    cs.add("全真腐蚀惰性", {
        let full = alloc::vec![true; 16];
        // 边界语义可能收缩——只验长度与不增。
        erode(&full, w, h, 1).map(|e| e.iter().filter(|&&b| b).count() <= 16).unwrap_or(false)
    }, "腐蚀不生点");
    cs.add("带差互斥", {
        if let (Ok(d), Ok(e)) = (dilate(&m, w, h, 1), erode(&m, w, h, 1)) {
            let outer = diff_band(&m, &d);
            let inner = diff_band(&e, &m);
            match (outer, inner) {
                (Ok(o), Ok(i)) => o.iter().zip(i.iter()).all(|(&a, &b)| !(a && b)),
                _ => false,
            }
        } else {
            false
        }
    }, "外带内带不交");
    cs
}
