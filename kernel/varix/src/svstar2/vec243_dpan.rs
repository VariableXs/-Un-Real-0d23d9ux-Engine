//! VE-F0643 平移模式检测与优化（VE-D 域 · 目标 360 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0643`
//!
//! **判据（锚点原文逐条）**：
//! - **宁严勿松**：内容哈希不变 + 变换为纯平移才判平移；亚像素分量存在即拒绝；
//! - **差集暴露带**：新旧位置差集 = 暴露带，只绘带不整层；
//! - **哈希兜底**：判定误松（伪平移）→ 内容哈希拦截；
//! - **动画联动**：F0608 位移动画每帧插值判纯平移即走本路径；
//! - **判据**：搬移越界裁剪；带计算溢出 → 全层重绘回退。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec241_dmodel::Rect;

/// 变换（本模块只关心平移分量与是否有其它线性分量）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xform {
    /// 平移 (dx, dy)——浮点，含亚像素
    pub dx: f32,
    pub dy: f32,
    /// 缩放/旋转等非平移分量存在标记
    pub nonlinear: bool,
}

/// 内容指纹（哈希兜底：内容不变才许搬移）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentHash(pub u64);

/// 平移判定器（宁严勿松）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanVerdict {
    pub is_pan: bool,
    pub reason: &'static str,
}

pub fn detect_pan(x: Xform, old_hash: ContentHash, new_hash: ContentHash) -> PanVerdict {
    if old_hash != new_hash {
        return PanVerdict { is_pan: false, reason: "内容哈希变化" };
    }
    if x.nonlinear {
        return PanVerdict { is_pan: false, reason: "含缩放/旋转变换" };
    }
    // 亚像素分量存在即拒绝（判定宁严勿松）
    if x.dx.fract() != 0.0 || x.dy.fract() != 0.0 {
        return PanVerdict { is_pan: false, reason: "亚像素平移" };
    }
    if x.dx == 0.0 && x.dy == 0.0 {
        return PanVerdict { is_pan: false, reason: "零位移" };
    }
    PanVerdict { is_pan: true, reason: "纯平移" }
}

/// 位块搬移计划：旧位置平移到新位置（画布内裁剪）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovePlan {
    /// 源区域（画布内有效）
    pub src: Rect,
    /// 目标区域（画布内有效）
    pub dst: Rect,
    /// 被裁剪的位移分量
    pub clipped: bool,
}

/// 搬移协议：画布 (w,h) 内做平移搬移，越界部分裁剪
pub fn plan_move(layer: Rect, dx: i64, dy: i64, canvas: (u32, u32)) -> Option<MovePlan> {
    let w = canvas.0 as i64;
    let h = canvas.1 as i64;
    // 源区域钳到画布
    let sx0 = layer.0.max(0) as i64;
    let sy0 = layer.1.max(0) as i64;
    let sx1 = (layer.2.min(canvas.0)) as i64;
    let sy1 = (layer.3.min(canvas.1)) as i64;
    if sx0 >= sx1 || sy0 >= sy1 {
        return None; // 源在画布外
    }
    let dst_x0 = sx0 + dx;
    let dst_y0 = sy0 + dy;
    let dst_x1 = sx1 + dx;
    let dst_y1 = sy1 + dy;
    // 目标与源的画布内交集（搬移有效区）
    let ix0 = dst_x0.max(0);
    let iy0 = dst_y0.max(0);
    let ix1 = dst_x1.min(w);
    let iy1 = dst_y1.min(h);
    let clipped = ix0 != dst_x0 || iy0 != dst_y0 || ix1 != dst_x1 || iy1 != dst_y1;
    if ix0 >= ix1 || iy0 >= iy1 {
        return None; // 完全出画布
    }
    // 有效搬移的源区间（与目标对应）
    let src = ((ix0 - dx) as u32, (iy0 - dy) as u32, (ix1 - dx) as u32, (iy1 - dy) as u32);
    Some(MovePlan {
        src,
        dst: (ix0 as u32, iy0 as u32, ix1 as u32, iy1 as u32),
        clipped,
    })
}

/// 差集暴露带：旧区域与新区域的对称差（只绘带不整层）
pub fn expose_bands(old: Rect, new: Rect) -> Vec<Rect> {
    let mut bands = Vec::new();
    let (ox0, oy0, ox1, oy1) = old;
    let (nx0, ny0, nx1, ny1) = new;
    // 水平差带
    if ox0 != nx0 || ox1 != nx1 {
        let l = ox0.min(nx0);
        let r = ox1.max(nx1);
        let overlap_l = ox0.max(nx0);
        let overlap_r = ox1.min(nx1);
        if overlap_l >= overlap_r {
            bands.push((l, oy0.min(ny0), r, oy1.max(ny1)));
        } else {
            if l < overlap_l {
                bands.push((l, oy0.min(ny0), overlap_l, oy1.max(ny1)));
            }
            if overlap_r < r {
                bands.push((overlap_r, oy0.min(ny0), r, oy1.max(ny1)));
            }
        }
    }
    // 垂直差带（水平重叠段内）
    let l = ox0.max(nx0);
    let r = ox1.min(nx1);
    if l < r {
        let t = oy0.min(ny0);
        let b = oy1.max(ny1);
        let overlap_t = oy0.max(ny0);
        let overlap_b = oy1.min(ny1);
        if overlap_t >= overlap_b {
            bands.push((l, t, r, b));
        } else {
            if t < overlap_t {
                bands.push((l, t, r, overlap_t));
            }
            if overlap_b < b {
                bands.push((l, overlap_b, r, b));
            }
        }
    }
    bands
}

/// 带计算溢出回退：暴露带总面积超过层面积 → 全层重绘
pub fn bands_fallback(layer: Rect, bands: &[Rect]) -> bool {
    let layer_area = (layer.2 - layer.0) as u64 * (layer.3 - layer.1) as u64;
    let band_area: u64 = bands
        .iter()
        .map(|&(l, t, r, b)| (r.saturating_sub(l) as u64) * (b.saturating_sub(t) as u64))
        .sum();
    band_area >= layer_area
}

pub fn run_vec243_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec243");
    let h1 = ContentHash(123);
    // 族一：宁严勿松
    let v1 = detect_pan(Xform { dx: 5.0, dy: 0.0, nonlinear: false }, h1, h1);
    s.add("平移-纯平移过", v1.is_pan, "clean pan");
    let v2 = detect_pan(Xform { dx: 5.5, dy: 0.0, nonlinear: false }, h1, h1);
    s.add("平移-亚像素拒", !v2.is_pan && v2.reason == "亚像素平移", "subpixel");
    let v3 = detect_pan(Xform { dx: 5.0, dy: 0.0, nonlinear: true }, h1, h1);
    s.add("平移-缩放伴生拒", !v3.is_pan, "scale reject");
    let v4 = detect_pan(Xform { dx: 5.0, dy: 0.0, nonlinear: false }, h1, ContentHash(456));
    s.add("平移-哈希兜底", !v4.is_pan && v4.reason == "内容哈希变化", "hash guard");
    let v5 = detect_pan(Xform { dx: 0.0, dy: 0.0, nonlinear: false }, h1, h1);
    s.add("平移-零位移拒", !v5.is_pan, "no-op");
    // 族二：搬移协议（画布裁剪）
    let m1 = plan_move((0, 0, 100, 100), 20, 0, (200, 200)).expect("m1");
    s.add("搬移-框随动", m1.dst == (20, 0, 120, 100), "moved box");
    s.add("搬移-未裁剪", !m1.clipped, "no clip");
    let m2 = plan_move((0, 0, 100, 100), 150, 0, (200, 200)).expect("m2");
    s.add("搬移-越界裁剪", m2.clipped && m2.dst == (150, 0, 200, 50), "clipped");
    s.add("搬移-源对应", m2.src == (0, 0, 50, 50), "src shrink");
    s.add("搬移-完全出画布", plan_move((0, 0, 100, 100), 500, 0, (200, 200)).is_none(), "gone");
    s.add("搬移-负向", plan_move((100, 100, 200, 200), -50, 0, (200, 200))
        .expect("neg").dst == (50, 100, 150, 200), "left move");
    // 族三：差集暴露带
    let bands = expose_bands((0, 0, 100, 100), (20, 0, 120, 100));
    s.add("暴露带-两条带", bands.len() == 2, "left+right bands");
    s.add("暴露带-左带", bands.contains(&(0, 0, 20, 100)), "left");
    s.add("暴露带-右带", bands.contains(&(100, 0, 120, 100)), "right");
    // 同框零差
    s.add("暴露带-零差无带", expose_bands((0, 0, 10, 10), (0, 0, 10, 10)).is_empty(), "no bands");
    // 族四：溢出回退
    s.add("回退-小带不全绘", !bands_fallback((0, 0, 1000, 1000), &[(0, 0, 20, 100)]), "small ok");
    let big_bands = expose_bands((0, 0, 100, 100), (0, 0, 100, 100));
    let _ = big_bands;
    s.add("回退-带超层回退", bands_fallback((0, 0, 10, 10), &[(0, 0, 10, 10), (0, 0, 10, 10)]), "≥ layer → full");
    // 族五：动画联动语义（每帧整像素位移可走本路径）
    for f in 1..4u32 {
        let v = detect_pan(Xform { dx: f as f32 * 8.0, dy: 0.0, nonlinear: false }, h1, h1);
        s.add("动画-帧平移可用", v.is_pan, "frame pan ok");
    }
    s
}
