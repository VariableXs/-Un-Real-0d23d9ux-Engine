//! cga67_tilepipe —— CGPU-F0067 大图分块流水。
//! 锚点（CGPU 施工书）：CGPU-F0067 · 大图分块流水 · 目标 340 行
//! 判据摘录：分块（tile）切分完整覆盖（Σtile 面积 == 图面积，
//!   无缝无叠）；tile 间重叠 Halo 环（半径 1 的边界扩展——卷积
//!   算子跨块一致性硬线：带 Halo 分块卷积 == 整图卷积，逐位对
//!   拍）；流水级依赖闭集（load→compute→store 三级，无跨级反依
//!   赖）；tile 序号账（行主序单调）；零尺寸 tile 显性拒绝。
//! 组件：tile 切分器（tw×th 网格）+ Halo 扩展访问器 + 分块卷
//!   积引擎（Halo 内边界镜像）+ 整图卷积参照（对拍锚）+ 流水
//!   级状态机 + 覆盖守恒核。
//! 职责定位：超大图（>SRAM）处理的地基——分块是缓存友好的唯一
//!   形态，Halo 环保证分块不改变语义（这是与「切块近似」的根
//!   本区别：本模块的切分是精确的）。
//! 性能逐项分解：切分 O(tile 数)；分块卷积 O(9n)（与整图同阶
//!   ——Halo 只加 O(周长)）；对拍 O(9n)。全整数确定性。
//! 诊断码段：0xE6B0（0xE6B1~0xE6B9 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const TP_VERSION: &str = "cga67_tilepipe-v1";

/// 诊断段基址（0xE6B0 段 · 本模块独占）。
pub const TP_SEG_BASE: u16 = 0xE6B0;

/// XcCode 闭集（0xE6B1~0xE6B9）。
pub const XC_DIM_ZERO: u32 = 0xE6B1;
pub const XC_TILE_ZERO: u32 = 0xE6B2;
pub const XC_COVER_FAIL: u32 = 0xE6B3;
pub const XC_HALO_MISS: u32 = 0xE6B4;
pub const XC_PIPE_ORDER: u32 = 0xE6B5;
pub const XC_TILE_SEQ: u32 = 0xE6B6;
pub const XC_CONV_DRIFT: u32 = 0xE6B7;
pub const XC_MIRROR_OOB: u32 = 0xE6B8;
pub const XC_LEN_MISMATCH: u32 = 0xE6B9;

/// 灰度图。
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Img {
    pub fn new(w: usize, h: usize, px: Vec<u32>) -> Result<Img, u32> {
        if w == 0 || h == 0 || px.len() != w * h {
            return Err(XC_DIM_ZERO);
        }
        Ok(Img { w, h, px })
    }

    /// 镜像访问（clamp 反射——与 cga61 同语义独立副本）。
    pub fn at(&self, x: i64, y: i64) -> Result<u32, u32> {
        let m = |i: i64, n: usize| -> Result<usize, u32> {
            let nn = n as i64;
            let mut j = i;
            while j < 0 || j >= nn {
                if j < 0 {
                    j = -j;
                }
                if j >= nn {
                    j = 2 * nn - 2 - j;
                }
                if j < 0 {
                    return Err(XC_MIRROR_OOB);
                }
            }
            Ok(j as usize)
        };
        Ok(self.px[m(y, self.h)? * self.w + m(x, self.w)?])
    }
}

/// Tile 切分（行主序）：返回 (x, y, tw, th) 四元组序列。
pub fn split_tiles(w: usize, h: usize, tw: usize, th: usize) -> Result<Vec<(usize, usize, usize, usize)>, u32> {
    if w == 0 || h == 0 {
        return Err(XC_DIM_ZERO);
    }
    if tw == 0 || th == 0 {
        return Err(XC_TILE_ZERO);
    }
    let mut out: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut y = 0usize;
    while y < h {
        let mut x = 0usize;
        let th2 = if y + th <= h { th } else { h - y };
        while x < w {
            let tw2 = if x + tw <= w { tw } else { w - x };
            out.push((x, y, tw2, th2));
            x += tw2;
        }
        y += th2;
    }
    // 覆盖守恒：Σ面积 == 图面积。
    let total: usize = out.iter().map(|&(_, _, a, b)| a * b).sum();
    if total != w * h {
        return Err(XC_COVER_FAIL);
    }
    // 行主序单调（tile 序号账：x 递增 y 递增）。
    for wd in out.windows(2) {
        let (x0, y0, tw0, th0) = wd[0];
        let (x1, y1, _, _) = wd[1];
        let ok = (y1 == y0 && x1 == x0 + tw0) || (y1 == y0 + th0 && x1 == 0);
        if !ok {
            return Err(XC_TILE_SEQ);
        }
    }
    Ok(out)
}

/// 带 Halo 的 tile 提取：tile 区域外扩 halo 圈（镜像边界）。
pub fn tile_with_halo(img: &Img, x: usize, y: usize, tw: usize, th: usize, halo: usize) -> Result<Img, u32> {
    if halo == 0 || halo > 4 {
        return Err(XC_HALO_MISS);
    }
    let ew = tw + 2 * halo;
    let eh = th + 2 * halo;
    let mut px: Vec<u32> = Vec::with_capacity(ew * eh);
    for ey in 0..eh {
        for ex in 0..ew {
            let sx = x + ex - halo;
            let sy = y + ey - halo;
            px.push(img.at(sx as i64, sy as i64)?);
        }
    }
    Img::new(ew, eh, px)
}

/// 3×3 全 1 盒滤波（Halo 消费者的标准算子——归一化 >>3 不得，
/// 9 除以 9 非移位 ⇒ 用 Σ 不归一的梯度账：盒和）。
pub fn box3_sum(src: &Img) -> Result<Img, u32> {
    let mut px: Vec<u32> = Vec::with_capacity(src.px.len());
    for y in 0..src.h {
        for x in 0..src.w {
            let mut acc: u64 = 0;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    acc += src.at(x as i64 + dx, y as i64 + dy)? as u64;
                }
            }
            px.push(acc as u32);
        }
    }
    Img::new(src.w, src.h, px)
}

/// 分块卷积 == 整图卷积对拍核（语义精确性硬线）。
pub fn tiled_vs_whole(img: &Img, tw: usize, th: usize) -> Result<(), u32> {
    let whole = box3_sum(img)?;
    let tiles = split_tiles(img.w, img.h, tw, th)?;
    let mut out: Vec<u32> = vec![0; img.px.len()];
    for &(x, y, tw2, th2) in tiles.iter() {
        let ext = tile_with_halo(img, x, y, tw2, th2, 1)?;
        // 在扩展块内逐像素求盒和，只回写 tile 核心区。
        for ty in 0..th2 {
            for tx in 0..tw2 {
                let mut acc: u64 = 0;
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        // 扩展块坐标：(tx+1+dx, ty+1+dy)。
                        let ex = (tx as i64 + 1 + dx) as usize;
                        let ey = (ty as i64 + 1 + dy) as usize;
                        acc += ext.px[ey * ext.w + ex] as u64;
                    }
                }
                out[(y + ty) * img.w + (x + tx)] = acc as u32;
            }
        }
    }
    if out != whole.px {
        return Err(XC_CONV_DRIFT);
    }
    Ok(())
}

/// 流水级状态机：load → compute → store 闭集，禁跨级反依赖。
pub const PIPE_LOAD: u8 = 0;
pub const PIPE_COMPUTE: u8 = 1;
pub const PIPE_STORE: u8 = 2;

/// 级推进核（只能前向，回退即破）。
pub fn pipe_advance(cur: u8) -> Result<u8, u32> {
    match cur {
        PIPE_LOAD => Ok(PIPE_COMPUTE),
        PIPE_COMPUTE => Ok(PIPE_STORE),
        PIPE_STORE => Ok(PIPE_LOAD),
        _ => Err(XC_PIPE_ORDER),
    }
}

/// 覆盖守恒独立复核。
pub fn cover_audit(tiles: &[(usize, usize, usize, usize)], w: usize, h: usize) -> Result<(), u32> {
    let total: usize = tiles.iter().map(|&(_, _, a, b)| a * b).sum();
    if total != w * h {
        return Err(XC_COVER_FAIL);
    }
    Ok(())
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga67_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0067");
    // 族一：tile 切分覆盖 + 序号账。
    let t1 = split_tiles(8, 8, 4, 4).map_err(|e| e as u16);
    cs.add("整除切分", matches!(&t1, Ok(v) if v.len() == 4 && v[0] == (0, 0, 4, 4)), "2×2 网格");
    let t2 = split_tiles(10, 8, 4, 4).map_err(|e| e as u16);
    cs.add("余数吸收", matches!(&t2, Ok(v) if v.len() == 6), "3×2 网格含窄带");
    if let Ok(v) = &t2 {
        cs.add("覆盖复核", cover_audit(v, 10, 8).is_ok(), "Σ=80");
    }
    cs.add("零 tile 拒", split_tiles(8, 8, 0, 4).is_err(), "TILE_ZERO");
    cs.add("空图拒", split_tiles(0, 0, 4, 4).is_err(), "DIM_ZERO");
    // 族二：Halo 提取（4×4 tile + halo1 → 6×6 扩展块）。
    let img = Img::new(8, 8, (0..64u32).collect()).map_err(|e| e as u16);
    if let Ok(g) = &img {
        let ext = tile_with_halo(g, 0, 0, 4, 4, 1).map_err(|e| e as u16);
        cs.add("Halo 尺寸", matches!(&ext, Ok(e) if e.w == 6 && e.h == 6), "外扩一圈");
        cs.add("Halo 镜像", matches!(&ext, Ok(e) if e.px[0] == g.px[0] + 1 + 8), "(-1,-1) 镜像=px[9]");
        cs.add("Halo 核心区", matches!(&ext, Ok(e) if e.px[1 * 6 + 1] == 0 && e.px[4 * 6 + 4] == 4 * 8 + 3), "核心区值不变");
        cs.add("Halo 零拒", tile_with_halo(g, 0, 0, 4, 4, 0).is_err(), "HALO_MISS");
        cs.add("Halo 超限拒", tile_with_halo(g, 0, 0, 4, 4, 5).is_err(), ">4 拒");
        // 族三：分块 == 整图（双形态对拍——语义精确性硬线）。
        cs.add("4×4 精确", tiled_vs_whole(g, 4, 4).is_ok(), "分块==整图");
        cs.add("3×3 精确", tiled_vs_whole(g, 3, 3).is_ok(), "非整除网格");
        cs.add("8×8 单块", tiled_vs_whole(g, 8, 8).is_ok(), "单 tile 退化");
        // 族四：流水级闭集。
        cs.add("load→compute", matches!(pipe_advance(PIPE_LOAD), Ok(PIPE_COMPUTE)), "前向一级");
        cs.add("compute→store", matches!(pipe_advance(PIPE_COMPUTE), Ok(PIPE_STORE)), "前向二级");
        cs.add("store→load", matches!(pipe_advance(PIPE_STORE), Ok(PIPE_LOAD)), "轮转回卷");
        cs.add("级越界拒", pipe_advance(9).is_err(), "PIPE_ORDER");
        // 族五：盒滤波值锚（中心 4 邻 8 元和）。
        let bs = box3_sum(g).map_err(|e| e as u16);
        cs.add("盒和角点", matches!(&bs, Ok(m) if m.px[0] == 54), "镜像 9 元 = 9+8+9+1+0+1+9+8+9");
        cs.add("盒和中心", matches!(&bs, Ok(m) if m.px[3 * 8 + 3] == (18 + 19 + 20 + 26 + 27 + 28 + 34 + 35 + 36) as u32), "中心 9 元 243");
    } else {
        cs.add("夹具构建", false, "img 构建失败");
    }
    cs
}
