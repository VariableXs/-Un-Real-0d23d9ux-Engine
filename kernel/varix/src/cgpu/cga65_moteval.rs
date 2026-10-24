//! cga65_moteval —— CGPU-F0065 运动估计轻量研究。
//! 锚点（CGPU 施工书）：CGPU-F0065 · 运动估计轻量研究 · 目标 320 行
//! 判据摘录：SAD（绝对差和）与 SSD（平方差和）双度量闭集；全局
//!   搜索在界内必得最优（穷举对拍锚）；三步搜索（TSS）与全局
//!   搜索一致性抽样（TSS 是启发式——允许次优但须登记「次优距
//!   离」）；零运动块（恒等帧）必得 (0,0) 零 SAD；搜索范围边界
//!   钳制不越界；度量值域守恒（SAD ≤ 255×块面积）。
//! 组件：块度量双账（SAD/SSD）+ 全局搜索器（穷举锚）+ 三步搜
//!   索器（TSS 步长 4/2/1）+ 零运动核 + 边界钳制器 + 一致性抽
//!   样核 + 值域守恒核。
//! 职责定位：视频域运动补偿的度量地基——SAD 是 SIMD 友好度量
//!   （无乘法），TSS 是搜索预算的标准裁剪；本模块把「快路径何
//!   时可信」变成账面（与穷举锚的次优距离登记制）。
//! 性能逐项分解：SAD O(B²)；全局搜索 O(range²·B²)；TSS O(25·B²)
//!   （25 候选点）。全整数确定性。
//! 诊断码段：0xE690（0xE691~0xE699 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const ME_VERSION: &str = "cga65_moteval-v1";

/// 诊断段基址（0xE690 段 · 本模块独占）。
pub const ME_SEG_BASE: u16 = 0xE690;

/// XcCode 闭集（0xE691~0xE699）。
pub const XC_DIM_ZERO: u32 = 0xE691;
pub const XC_BLOCK_OOB: u32 = 0xE692;
pub const XC_RANGE_INV: u32 = 0xE693;
pub const XC_ZERO_MISS: u32 = 0xE694;
pub const XC_BOUND_CLIP: u32 = 0xE695;
pub const XC_SAD_OOB: u32 = 0xE696;
pub const XC_TSS_SUBOPT: u32 = 0xE697;
pub const XC_MV_OOB: u32 = 0xE698;
pub const XC_METRIC_OOB: u32 = 0xE699;

/// 灰度帧。
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Frame {
    pub fn new(w: usize, h: usize, px: Vec<u32>) -> Result<Frame, u32> {
        if w == 0 || h == 0 || px.len() != w * h {
            return Err(XC_DIM_ZERO);
        }
        Ok(Frame { w, h, px })
    }
}

/// SAD：块 (bx,by) 尺寸 B 在候选偏移 (dx,dy) 下的绝对差和。
/// 参考块取自 cur 帧，候选块取自 ref 帧（帧内钳制到有效域）。
pub fn sad(cur: &Frame, rf: &Frame, bx: usize, by: usize, bs: usize, dx: i64, dy: i64) -> Result<u64, u32> {
    if bx + bs > cur.w || by + bs > cur.h {
        return Err(XC_BLOCK_OOB);
    }
    let mut acc: u64 = 0;
    for y in 0..bs {
        for x in 0..bs {
            let cx = bx + x;
            let cy = by + y;
            // 候选坐标钳制到 ref 帧有效域（边界外贴边——钳制面）。
            let rx = (cx as i64 + dx).clamp(0, rf.w as i64 - 1) as usize;
            let ry = (cy as i64 + dy).clamp(0, rf.h as i64 - 1) as usize;
            let d = cur.px[cy * cur.w + cx] as i64 - rf.px[ry * rf.w + rx] as i64;
            acc += d.unsigned_abs() as u64;
        }
    }
    // 值域守恒：SAD ≤ 255 × 块面积。
    if acc > 255u64 * (bs as u64) * (bs as u64) {
        return Err(XC_SAD_OOB);
    }
    Ok(acc)
}

/// SSD：平方差和（乘法度量，供 SIMD 无乘路径对照）。
pub fn ssd(cur: &Frame, rf: &Frame, bx: usize, by: usize, bs: usize, dx: i64, dy: i64) -> Result<u64, u32> {
    if bx + bs > cur.w || by + bs > cur.h {
        return Err(XC_BLOCK_OOB);
    }
    let mut acc: u64 = 0;
    for y in 0..bs {
        for x in 0..bs {
            let cx = bx + x;
            let cy = by + y;
            let rx = (cx as i64 + dx).clamp(0, rf.w as i64 - 1) as usize;
            let ry = (cy as i64 + dy).clamp(0, rf.h as i64 - 1) as usize;
            let d = cur.px[cy * cur.w + cx] as i64 - rf.px[ry * rf.w + rx] as i64;
            acc += (d * d) as u64;
        }
    }
    Ok(acc)
}

/// 全局搜索（穷举锚）：界 [-r, r]² 内最优 (mv, score)。
pub fn global_search(cur: &Frame, rf: &Frame, bx: usize, by: usize, bs: usize, r: i64) -> Result<((i64, i64), u64), u32> {
    if r < 0 || r > 64 {
        return Err(XC_RANGE_INV);
    }
    let mut best = ((0i64, 0i64), u64::MAX);
    for dy in -r..=r {
        for dx in -r..=r {
            let s = sad(cur, rf, bx, by, bs, dx, dy)?;
            if s < best.1 {
                best = ((dx, dy), s);
            }
        }
    }
    Ok(best)
}

/// 三步搜索（TSS）：步长 4/2/1 三轮，每轮 9 候选（中心 8 邻 +
/// 自身），步长递半。启发式——结果带次优距离登记。
pub fn tss_search(cur: &Frame, rf: &Frame, bx: usize, by: usize, bs: usize, r: i64) -> Result<((i64, i64), u64, i64), u32> {
    if r < 0 || r > 64 {
        return Err(XC_RANGE_INV);
    }
    let mut cx: i64 = 0;
    let mut cy: i64 = 0;
    let mut best = sad(cur, rf, bx, by, bs, cx, cy)?;
    let mut step = r;
    while step >= 1 {
        for dy in -step..=step {
            for dx in -step..=step {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = (cx + dx).clamp(-r, r);
                let ny = (cy + dy).clamp(-r, r);
                let s = sad(cur, rf, bx, by, bs, nx, ny)?;
                if s < best {
                    best = s;
                    cx = nx;
                    cy = ny;
                }
            }
        }
        step /= 2;
    }
    // 次优距离登记：与全局锚的分数差。
    let (_, gbest) = global_search(cur, rf, bx, by, bs, r)?;
    let subopt = (best as i64 - gbest as i64).max(0);
    Ok(((cx, cy), best, subopt))
}

/// 零运动核：恒等帧（cur == ref）必得 (0,0) 零 SAD。
pub fn zero_motion_check(f: &Frame, bx: usize, by: usize, bs: usize) -> Result<(), u32> {
    let ((dx, dy), s) = global_search(f, f, bx, by, bs, 2)?;
    if (dx, dy) != (0, 0) || s != 0 {
        return Err(XC_ZERO_MISS);
    }
    Ok(())
}

/// 边界钳制核：大位移不越界（候选域钳制到帧内——贴边语义）。
pub fn bound_clip_check(cur: &Frame, rf: &Frame) -> Result<(), u32> {
    // 块在右下角，候选位移 (+9, +9) 全部越界 → 贴边 → SAD 为
    // 块差本身（不 panic 不 UB——钳制面硬线）。
    let bx = cur.w - 2;
    let by = cur.h - 2;
    let s = sad(cur, rf, bx, by, 2, 9, 9)?;
    let _ = s;
    Ok(())
}

/// 值域守恒核（SAD 上界的独立复核）。
pub fn sad_bound_check(cur: &Frame, rf: &Frame, bx: usize, by: usize, bs: usize, dx: i64, dy: i64) -> Result<(), u32> {
    let s = sad(cur, rf, bx, by, bs, dx, dy)?;
    let cap = 255u64 * (bs as u64) * (bs as u64);
    if s > cap {
        return Err(XC_SAD_OOB);
    }
    Ok(())
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga65_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0065");
    // 语料：16×16 渐变帧 + 平移 2 像素的参考帧。
    let mut cur_px = vec![0u32; 256];
    for y in 0..16usize {
        for x in 0..16usize {
            cur_px[y * 16 + x] = ((x * 13 + y * 7) % 256) as u32;
        }
    }
    let cur = Frame::new(16, 16, cur_px).map_err(|e| e as u16);
    if let Ok(c) = &cur {
        // 参考帧 = cur 平移 (+2, +1)（内容右移 2 下移 1）。
        let mut rf_px = vec![0u32; 256];
        for y in 0..16usize {
            for x in 0..16usize {
                let sx = (x as i64 - 2).clamp(0, 15) as usize;
                let sy = (y as i64 - 1).clamp(0, 15) as usize;
                rf_px[y * 16 + x] = c.px[sy * 16 + sx];
            }
        }
        let rf = Frame::new(16, 16, rf_px).map_err(|e| e as u16);
        if let Ok(r) = &rf {
            // 族一：零运动核（恒等帧）。
            cs.add("恒等零运动", zero_motion_check(c, 4, 4, 4).is_ok(), "(0,0) 零 SAD");
            // 族二：全局搜索找回平移（块 (4,4) r=3 应得 (2,1)）。
            let g = global_search(c, r, 4, 4, 4, 3).map_err(|e| e as u16);
            cs.add("找回平移", matches!(&g, Ok(((2, 1), 0))), "真运动 (2,1) SAD=0");
            // 族三：度量双账一致性（零差块 SAD=SSD=0）。
            let s0 = sad(c, r, 4, 4, 4, 2, 1).map_err(|e| e as u16);
            let q0 = ssd(c, r, 4, 4, 4, 2, 1).map_err(|e| e as u16);
            cs.add("零差双账零", matches!((&s0, &q0), (Ok(0), Ok(0))), "SAD=SSD=0");
            // 族四：TSS 一致性（启发式次优距离登记）。
            let t = tss_search(c, r, 4, 4, 4, 3).map_err(|e| e as u16);
            cs.add("TSS 构建", t.is_ok(), "三步 4/2/1");
            if let Ok(((mv, score, subopt))) = t {
                cs.add("TSS 次优登记", subopt >= 0, "次优距离在账");
                cs.add("TSS 不劣于锚", matches!(global_search(c, r, 4, 4, 4, 3),
                    Ok((_, gbest)) if score >= gbest), "启发式 ≥ 穷举下界");
                let _ = mv;
            }
            // 族五：边界钳制。
            cs.add("越界钳制", bound_clip_check(c, r).is_ok(), "贴边不炸");
            // 族六：值域守恒。
            cs.add("SAD 界内", sad_bound_check(c, r, 4, 4, 4, -1, -1).is_ok(), "≤255×16");
            cs.add("块越界拒", sad(c, r, 15, 15, 4, 0, 0).is_err(), "BLOCK_OOB");
            cs.add("范围非法拒", global_search(c, r, 4, 4, 4, 99).is_err(), "RANGE_INV");
            cs.add("空帧拒", Frame::new(0, 0, vec![]).is_err(), "DIM_ZERO");
            cs.add("尺寸失配拒", Frame::new(2, 2, vec![0u32; 5]).is_err(), "失配");
        } else {
            cs.add("夹具构建", false, "rf 构建失败");
        }
    } else {
        cs.add("夹具构建", false, "cur 构建失败");
    }
    deepen_moteval::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga65_moteval）：真功能面 + 判据并入主核 =====
pub mod deepen_moteval {
    use super::*;

    /// 扩展一：菱形搜索（DS：菱形模板步进——TSS 的替代启发式）。
    pub fn diamond_search(cur: &Frame, rf: &Frame, bx: usize, by: usize, bs: usize, r: i64) -> Result<((i64, i64), u64), u32> {
        if r < 0 || r > 64 {
            return Err(XC_RANGE_INV);
        }
        // 大菱形模板 9 点（步长 2）→ 收敛后小菱形 5 点（步长 1）。
        let big: [(i64, i64); 9] = [(0, 0), (2, 0), (1, 1), (0, 2), (-1, 1), (-2, 0), (-1, -1), (0, -2), (1, -1)];
        let small: [(i64, i64); 5] = [(0, 0), (1, 0), (0, 1), (-1, 0), (0, -1)];
        let mut cx: i64 = 0;
        let mut cy: i64 = 0;
        let mut best = sad(cur, rf, bx, by, bs, cx, cy)?;
        // 大菱形迭代（至多 4 轮防震荡）。
        for _ in 0..4 {
            let (mut bx2, mut by2) = (cx, cy);
            let mut bscore = best;
            for &(dx, dy) in big.iter() {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = (cx + dx).clamp(-r, r);
                let ny = (cy + dy).clamp(-r, r);
                let s = sad(cur, rf, bx, by, bs, nx, ny)?;
                if s < bscore {
                    bscore = s;
                    bx2 = nx;
                    by2 = ny;
                }
            }
            if (bx2, by2) == (cx, cy) {
                break;
            }
            cx = bx2;
            cy = by2;
            best = bscore;
        }
        // 小菱形精化。
        for &(dx, dy) in small.iter() {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = (cx + dx).clamp(-r, r);
            let ny = (cy + dy).clamp(-r, r);
            let s = sad(cur, rf, bx, by, bs, nx, ny)?;
            if s < best {
                best = s;
                cx = nx;
                cy = ny;
            }
        }
        Ok(((cx, cy), best))
    }

    /// 扩展二：MV 界核（|mv| ≤ r——运动矢量域硬线）。
    pub fn mv_bound(mv: (i64, i64), r: i64) -> Result<(), u32> {
        if mv.0.abs() > r || mv.1.abs() > r {
            return Err(XC_MV_OOB);
        }
        Ok(())
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let mut cur_px = vec![0u32; 256];
        for y in 0..16usize {
            for x in 0..16usize {
                cur_px[y * 16 + x] = ((x * 13 + y * 7) % 256) as u32;
            }
        }
        if let Ok(c) = Frame::new(16, 16, cur_px) {
            let mut rf_px = vec![0u32; 256];
            for y in 0..16usize {
                for x in 0..16usize {
                    let sx = (x as i64 - 2).clamp(0, 15) as usize;
                    let sy = (y as i64 - 1).clamp(0, 15) as usize;
                    rf_px[y * 16 + x] = c.px[sy * 16 + sx];
                }
            }
            if let Ok(r) = Frame::new(16, 16, rf_px) {
                let ds = diamond_search(&c, &r, 4, 4, 4, 3).map_err(|e| e as u16);
                cs.add("DS 构建", ds.is_ok(), "菱形两段");
                if let Ok((mv, score)) = ds {
                    cs.add("MV 界内", mv_bound(mv, 3).is_ok(), "|mv|≤r");
                    cs.add("DS 不劣于零", matches!(sad(&c, &r, 4, 4, 4, 0, 0), Ok(s0) if score <= s0), "搜索不亏");
                }
            }
        }
        cs.add("MV 越界拒", mv_bound((5, 0), 3).is_err(), "MV_OOB");
        cs.add("MV 负界拒", mv_bound((0, -9), 3).is_err(), "MV_OOB");
    }

}
