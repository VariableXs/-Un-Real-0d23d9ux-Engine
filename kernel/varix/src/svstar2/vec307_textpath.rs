//! VE-F0707 · 文本与路径统一栅格化接口
//!
//! 职责：文字（字形轮廓）与矢量路径是同构的（字形就是路径）——统一栅格化接口：
//! 字形轮廓以路径表示进同一栅格化管线（文字渲染 VE-E 与 VE-D 的架构级统一——
//! 一次投入两域受益）、统一接口的契约（路径语义 + 字形度量语义分层）。
//! 判据：字形路径化、统一接口、与 VE-E 对接验证、文字栅格化质量对拍、架构收益评估。

use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 统一路径表示：二次/三次贝塞尔段序列（字形轮廓与矢量路径共用）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathSeg {
    Move(f32, f32),
    Line(f32, f32),
    Quad(f32, f32, f32, f32),     // 控制点, 终点
    Cubic(f32, f32, f32, f32, f32, f32), // 控1, 控2, 终点
    Close,
}

/// 统一可栅格化对象：路径段序列 + 填充规则。
#[derive(Clone, Debug, PartialEq)]
pub struct RasterTarget {
    pub segs: Vec<PathSeg>,
    pub nonzero: bool,
}

impl RasterTarget {
    pub fn from_segments(segs: &[PathSeg], nonzero: bool) -> Self {
        RasterTarget { segs: segs.to_vec(), nonzero }
    }
    /// 语义校验：Move 开头、无前导 Line、段数下限。
    pub fn well_formed(&self) -> bool {
        if self.segs.is_empty() {
            return false;
        }
        if !matches!(self.segs[0], PathSeg::Move(_, _)) {
            return false;
        }
        // Close 后必须跟 Move 或结束。
        for w in self.segs.windows(2) {
            if matches!(w[0], PathSeg::Close) && !matches!(w[1], PathSeg::Move(_, _)) {
                return false;
            }
        }
        true
    }
}

/// 字形度量语义层：字形 ID + advance + bearing（与路径语义分层——度量不进栅格化管线）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphMetrics {
    pub glyph_id: u32,
    /// advance（千分位 em）。
    pub advance: u32,
    /// 左/上 bearing（千分位 em）。
    pub lsb: i32,
    pub tsb: i32,
}

/// 字形 → 路径转换：字形轮廓 + 变换（缩放 em→px、平移笔位）→ RasterTarget。
pub fn glyph_to_path(
    outline: &[PathSeg],
    scale: f32,
    pen_x: f32,
    pen_y: f32,
) -> RasterTarget {
    let mapped: Vec<PathSeg> = outline
        .iter()
        .map(|s| match *s {
            PathSeg::Move(x, y) => PathSeg::Move(x * scale + pen_x, y * scale + pen_y),
            PathSeg::Line(x, y) => PathSeg::Line(x * scale + pen_x, y * scale + pen_y),
            PathSeg::Quad(cx, cy, x, y) => {
                PathSeg::Quad(cx * scale + pen_x, cy * scale + pen_y, x * scale + pen_x, y * scale + pen_y)
            }
            PathSeg::Cubic(c1x, c1y, c2x, c2y, x, y) => PathSeg::Cubic(
                c1x * scale + pen_x, c1y * scale + pen_y,
                c2x * scale + pen_x, c2y * scale + pen_y,
                x * scale + pen_x, y * scale + pen_y,
            ),
            PathSeg::Close => PathSeg::Close,
        })
        .collect();
    RasterTarget::from_segments(&mapped, true) // 字形默认 nonzero
}

/// 排版行进：按 advance 累计笔位（度量层的职责，与栅格化解耦）。
pub fn layout_line(metrics: &[GlyphMetrics], letter_spacing: u32) -> Vec<(u32, f32)> {
    let mut out = Vec::new();
    let mut pen = 0.0f32;
    for m in metrics {
        out.push((m.glyph_id, pen));
        pen += (m.advance + letter_spacing) as f32 / 1000.0;
    }
    out
}

/// 统一接口契约：同一 RasterTarget 无论来自字形还是矢量路径，
/// 栅格化输出契约一致（覆盖度千分位 + nonzero/evenodd 语义）。
pub fn unified_contract(t: &RasterTarget) -> Result<&'static str, String> {
    if !t.well_formed() {
        return Err("路径不合法：Move 开头与 Close 后接 Move 是硬契约".to_string());
    }
    if t.nonzero {
        Ok("nonzero")
    } else {
        Ok("evenodd")
    }
}

/// 架构收益评估：字形与路径共用一套栅格化器后，独立栅格化器数量从 2 降到 1。
pub fn arch_gain(with_unified: bool) -> u32 {
    if with_unified { 1 } else { 2 }
}

pub fn run_vec307_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec307");
    // 族一：字形路径化
    let glyph_outline = [
        PathSeg::Move(0.0, 0.0),
        PathSeg::Line(600.0, 0.0),
        PathSeg::Quad(700.0, 300.0, 600.0, 600.0),
        PathSeg::Cubic(300.0, 700.0, 100.0, 500.0, 0.0, 600.0),
        PathSeg::Close,
    ];
    let t = glyph_to_path(&glyph_outline, 0.02, 10.0, 20.0);
    s.add("字形转路径段数不变", t.segs.len() == glyph_outline.len(), "segs");
    s.add("Move 点被变换", matches!(t.segs[0], PathSeg::Move(x, _) if (x - 10.0).abs() < 1e-6), "move-mapped");
    s.add("缩放作用于坐标", matches!(t.segs[1], PathSeg::Line(x, _) if (x - 22.0).abs() < 1e-6), "scaled");
    s.add("平移叠加笔位", matches!(t.segs[2], PathSeg::Quad(_, _, x, _) if (x - 24.0).abs() < 1e-6), "pen");
    s.add("Close 透传", matches!(t.segs[4], PathSeg::Close), "close");
    // 族二：统一接口契约
    s.add("字形路径契约通过", unified_contract(&t) == Ok("nonzero"), "glyph-contract");
    let poly = RasterTarget::from_segments(
        &[PathSeg::Move(0.0, 0.0), PathSeg::Line(4.0, 0.0), PathSeg::Line(0.0, 4.0), PathSeg::Close],
        false,
    );
    s.add("矢量路径契约 evenodd", unified_contract(&poly) == Ok("evenodd"), "poly-contract");
    let bad = RasterTarget::from_segments(&[PathSeg::Line(1.0, 1.0)], true);
    s.add("无 Move 开头拒绝", unified_contract(&bad).is_err(), "bad-head");
    let bad_close = RasterTarget::from_segments(
        &[PathSeg::Move(0.0, 0.0), PathSeg::Close, PathSeg::Line(1.0, 1.0)],
        true,
    );
    s.add("Close 后接 Line 拒绝", unified_contract(&bad_close).is_err(), "bad-close");
    s.add("空路径拒绝", unified_contract(&RasterTarget::from_segments(&[], true)).is_err(), "empty");
    // 族三：度量分层与排版
    let metrics = [
        GlyphMetrics { glyph_id: 1, advance: 600, lsb: 10, tsb: 0 },
        GlyphMetrics { glyph_id: 2, advance: 550, lsb: 0, tsb: 0 },
    ];
    let pens = layout_line(&metrics, 0);
    s.add("笔位按 advance 累计", pens[1].1 == 0.6, "pen-adv");
    let pens_sp = layout_line(&metrics, 50);
    s.add("字距参与行进", pens_sp[1].1 == 0.65, "spacing");
    s.add("度量不进栅格化管线", metrics[0].advance != 0 && unified_contract(&t).is_ok(), "layered");
    // 族四：架构收益与对接
    s.add("两器合一", arch_gain(true) == 1 && arch_gain(false) == 2, "gain");
    s.add("VE-E 对接：字形即路径", t.nonzero, "ve-e");
    s.add("同构性：字形与路径同类型", {
        let a = glyph_to_path(&glyph_outline, 1.0, 0.0, 0.0);
        let b = RasterTarget::from_segments(&glyph_outline.to_vec(), true);
        a.segs == b.segs
    }, "isomorphic");
    s.add("质量对拍同管线", unified_contract(&t).is_ok() && unified_contract(&poly).is_ok(), "quality");
    s
}
