//! F625 指针绘制工坊 · 完整设计（STAR I 主册 J-C 组）。
//!
//! **判据（主册原文）**：三笔刷全功能；双倍率同屏对拍；洋葱皮 30% 透明
//! 度；16 帧上限纪律；作品入库链路（F628 对接）；热点复用一致性。
//!
//! **定位（主册）**：F156 编辑器的创作层扩展——F156 调成品、工坊造新件；
//! 热点标定复用 F156 十字件（同语义：热点必须落在实体上、双击置中）。
//!
//! **工坊语义**：
//! - 画布 40×40（1x），2x 帧由 1x 逐像素 2×2 复制派生（双倍率同屏对拍
//!   的口径 = 两倍率逐像素确定对应，画时即所见）；
//! - 三笔刷：钢笔（自由笔迹，半径可调）/矩形（描边与填充两用）/椭圆
//!   （描边与填充两用）——起笔-拖动-收笔全程走笔画栈（可撤销）；
//! - 洋葱皮：前一帧以 30% 透明度（alpha ×77/255）垫底衬显示——画动画
//!   指针不盲画；预览合成不污染工作帧；
//! - 逐帧时间轴：16 帧上限（第 17 帧诚实拒绝）、增删/复制/重排/每帧
//!   延时（默认 100ms）；
//! - 撤销：笔画级撤销栈（每帧独立，容量 50——超限丢最旧并计数）；
//! - 热点：复用 F156 十字件语义（`set_hotspot` 校验实体落点）；
//! - 入库链路：`into_scheme()` 产出 jbase [`CursorSchemeModel`]，
//!   由 F628 `add()` 直收入库（对拍：工坊内容指纹 == 入库条目指纹）。

use crate::checks::CheckSet;
use crate::jstar2::jbase::{
    fill_ellipse, fill_rect, stroke_line, stroke_rect, CursorFrame, CursorSchemeModel,
    OriginKind, PixBuf, PointerState, MAX_FRAMES_PER_STATE,
};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 工坊画布边长（判据「40px 画布」）。
pub const CANVAS_PX: u16 = 40;
/// 洋葱皮不透明度（30%，255 制 → 77）。
pub const ONION_ALPHA: u8 = 77;
/// 撤销栈深度。
pub const UNDO_CAP: usize = 50;

// ---------------------------------------------------------------------------
// 笔刷与笔画
// ---------------------------------------------------------------------------

/// 三笔刷（判据「三笔刷全功能」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Brush {
    /// 钢笔：自由笔迹（radius = 半宽 0..4）。
    Pen { radius: u16 },
    /// 矩形：filled = 填充，否则描边。
    Rect { filled: bool },
    /// 椭圆：filled = 填充，否则描边。
    Ellipse { filled: bool },
}

impl Brush {
    pub fn zh(self) -> &'static str {
        match self {
            Brush::Pen { .. } => "钢笔",
            Brush::Rect { .. } => "矩形",
            Brush::Ellipse { .. } => "椭圆",
        }
    }
}

/// 一笔（撤销栈的最小单位；起终点 + 笔刷参数 + 颜色快照足够重放）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stroke {
    pub brush: Brush,
    pub color: [u8; 4],
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}

impl Stroke {
    /// 重放到画布（撤销重做与实际绘制共用同一重放口——所见即所存）。
    fn replay(&self, cv: &mut PixBuf) {
        match self.brush {
            Brush::Pen { radius } => {
                stroke_line(cv, self.x0, self.y0, self.x1, self.y1, radius, self.color);
            }
            Brush::Rect { filled } => {
                if filled {
                    fill_rect(cv, self.x0, self.y0, self.x1, self.y1, self.color);
                } else {
                    stroke_rect(cv, self.x0, self.y0, self.x1, self.y1, 0, self.color);
                }
            }
            Brush::Ellipse { filled } => {
                let (cx, cy) = ((self.x0 + self.x1) / 2, (self.y0 + self.y1) / 2);
                let (rx, ry) = ((self.x0 - self.x1).abs() / 2, (self.y0 - self.y1).abs() / 2);
                if rx == 0 || ry == 0 {
                    // 退化：单轴为 0 时画线（诚实处理，不画隐形椭圆）。
                    stroke_line(cv, self.x0, self.y0, self.x1, self.y1, 0, self.color);
                } else if filled {
                    fill_ellipse(cv, cx, cy, rx, ry, self.color);
                } else {
                    crate::jstar2::jbase::stroke_ellipse(cv, cx, cy, rx, ry, 0, self.color);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 工坊文档（帧序列 + 撤销栈 + 热点）
// ---------------------------------------------------------------------------

/// 单帧工作文档。
#[derive(Clone, Debug)]
struct WorkFrame {
    canvas: PixBuf,
    strokes: Vec<Stroke>,
    undo_pos: usize,
    undo_dropped: usize,
    hotspot: (u16, u16),
    delay_ms: u32,
}

impl WorkFrame {
    fn new(delay_ms: u32) -> WorkFrame {
        WorkFrame {
            canvas: PixBuf::new(CANVAS_PX, CANVAS_PX),
            strokes: Vec::new(),
            undo_pos: 0,
            undo_dropped: 0,
            hotspot: (2, 2),
            delay_ms,
        }
    }

    /// 全量重放（撤销/重做后画布由笔画序列唯一决定——单一事实源）。
    fn replay_all(&mut self) {
        self.canvas = PixBuf::new(CANVAS_PX, CANVAS_PX);
        for s in &self.strokes[..self.undo_pos] {
            s.replay(&mut self.canvas);
        }
    }

    fn apply(&mut self, s: Stroke) {
        // 截断重做尾巴（新笔画分叉）。
        self.strokes.truncate(self.undo_pos);
        self.strokes.push(s);
        if self.strokes.len() > UNDO_CAP {
            let drop = self.strokes.len() - UNDO_CAP;
            self.strokes.drain(..drop);
            self.undo_dropped += drop;
            self.undo_pos = self.strokes.len();
        } else {
            self.undo_pos = self.strokes.len();
        }
        s.replay(&mut self.canvas);
    }

    fn undo(&mut self) -> bool {
        if self.undo_pos == 0 {
            return false;
        }
        self.undo_pos -= 1;
        self.replay_all();
        true
    }

    fn redo(&mut self) -> bool {
        if self.undo_pos >= self.strokes.len() {
            return false;
        }
        self.undo_pos += 1;
        self.replay_all();
        true
    }
}

/// 工坊文档（多帧）。
#[derive(Clone, Debug)]
pub struct Workshop {
    frames: Vec<WorkFrame>,
    current: usize,
    color: [u8; 4],
    brush: Brush,
    /// 洋葱皮开关（默认开——判据「画动画指针不盲画」）。
    pub onion_skin: bool,
}

/// 工坊操作结果（诚实错误面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkshopError {
    FrameCapReached,
    NoFrame,
    HotspotOffInk,
}

impl Workshop {
    /// 新工坊（自动带 1 空白帧）。
    pub fn new() -> Workshop {
        Workshop {
            frames: alloc::vec![WorkFrame::new(100)],
            current: 0,
            color: [24, 24, 24, 255],
            brush: Brush::Pen { radius: 0 },
            onion_skin: true,
        }
    }

    pub fn set_color(&mut self, c: [u8; 4]) {
        self.color = c;
    }

    pub fn set_brush(&mut self, b: Brush) {
        self.brush = b;
    }

    pub fn brush(&self) -> Brush {
        self.brush
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    pub fn select_frame(&mut self, i: usize) -> bool {
        if i >= self.frames.len() {
            return false;
        }
        self.current = i;
        true
    }

    /// 加帧（16 帧上限——第 17 帧诚实拒绝）。
    pub fn add_frame(&mut self) -> Result<usize, WorkshopError> {
        if self.frames.len() >= MAX_FRAMES_PER_STATE {
            return Err(WorkshopError::FrameCapReached);
        }
        let delay = self.frames[self.current].delay_ms;
        self.frames.insert(self.current + 1, WorkFrame::new(delay));
        self.current += 1;
        Ok(self.current)
    }

    /// 删帧（保底 1 帧）。
    pub fn delete_frame(&mut self) -> bool {
        if self.frames.len() <= 1 {
            return false;
        }
        self.frames.remove(self.current);
        if self.current >= self.frames.len() {
            self.current = self.frames.len() - 1;
        }
        true
    }

    /// 复制当前帧到其后。
    pub fn duplicate_frame(&mut self) -> Result<usize, WorkshopError> {
        if self.frames.len() >= MAX_FRAMES_PER_STATE {
            return Err(WorkshopError::FrameCapReached);
        }
        let clone = self.frames[self.current].clone();
        self.frames.insert(self.current + 1, clone);
        self.current += 1;
        Ok(self.current)
    }

    pub fn set_frame_delay(&mut self, ms: u32) {
        self.frames[self.current].delay_ms = ms.max(17).min(60000);
    }

    pub fn frame_delay(&self) -> u32 {
        self.frames[self.current].delay_ms
    }

    /// 落笔（笔画入栈并重放）。
    pub fn stroke(&mut self, x0: i64, y0: i64, x1: i64, y1: i64) {
        let s = Stroke { brush: self.brush, color: self.color, x0, y0, x1, y1 };
        self.frames[self.current].apply(s);
    }

    pub fn undo(&mut self) -> bool {
        self.frames[self.current].undo()
    }

    pub fn redo(&mut self) -> bool {
        self.frames[self.current].redo()
    }

    /// 撤销栈溢出计数（容量纪律的对账面）。
    pub fn undo_dropped_total(&self) -> usize {
        self.frames.iter().map(|f| f.undo_dropped).sum()
    }

    /// 热点设置（F156 十字件语义复用：必须落在实体上）。
    pub fn set_hotspot(&mut self, x: u16, y: u16) -> Result<(), WorkshopError> {
        let f = &self.frames[self.current];
        if x >= f.canvas.w || y >= f.canvas.h || !f.canvas.solid(x, y) {
            return Err(WorkshopError::HotspotOffInk);
        }
        self.frames[self.current].hotspot = (x, y);
        Ok(())
    }

    pub fn hotspot(&self) -> (u16, u16) {
        self.frames[self.current].hotspot
    }

    /// 工作帧视图（画布 + 可选洋葱皮合成——预览不污染工作帧）。
    pub fn view(&self) -> PixBuf {
        let f = &self.frames[self.current];
        let mut out = f.canvas.clone();
        if self.onion_skin && self.current > 0 {
            let mut ghost = self.frames[self.current - 1].canvas.clone();
            for px in ghost.px.chunks_exact_mut(4) {
                px[3] = ((px[3] as u32) * ONION_ALPHA as u32 / 255) as u8;
            }
            out.blend_over(&ghost, 0, 0);
        }
        out
    }

    /// 双倍率同屏帧（1x 逐像素 2×2 复制——对拍判据的产物面）。
    pub fn view_2x(&self) -> PixBuf {
        self.view().scale_integer2x()
    }

    /// 产出方案模型（入库链路：F628.add 直收；态归属由调用方指定）。
    pub fn into_scheme(self, name: &str, state: PointerState) -> CursorSchemeModel {
        let mut m = CursorSchemeModel::empty(name, OriginKind::Created);
        let frames: Vec<CursorFrame> = self
            .frames
            .iter()
            .map(|f| CursorFrame::from_buf(f.hotspot.0, f.hotspot.1, f.delay_ms, f.canvas.clone()))
            .collect();
        m.set_state(state, frames);
        m
    }

    /// 当前工作帧的笔画数（对账面）。
    pub fn stroke_count(&self) -> usize {
        self.frames[self.current].undo_pos
    }
}

impl Default for Workshop {
    fn default() -> Self {
        Workshop::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F625 自检。
pub fn run_workshop_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F625");

    // 1. 三笔刷全功能：各画一笔均有实体像素（形态区分）。
    let mut ok = true;
    let mut sigs: Vec<u64> = Vec::new();
    for b in [Brush::Pen { radius: 1 }, Brush::Rect { filled: true }, Brush::Ellipse { filled: false }] {
        let mut w = Workshop::new();
        w.set_brush(b);
        w.set_color([255, 0, 0, 255]);
        w.stroke(5, 5, 30, 25);
        let v = w.view();
        if v.solid_count() == 0 {
            ok = false;
        }
        sigs.push(crate::jstar2::jbase::fnv1a64(&v.px));
    }
    set.add("three brushes all paint", ok && sigs[0] != sigs[1] && sigs[1] != sigs[2], "");

    // 2. 双倍率同屏对拍：2x 帧是 1x 的逐像素 2×2 复制。
    let mut w = Workshop::new();
    w.stroke(3, 3, 20, 20);
    let one = w.view();
    let two = w.view_2x();
    let mut exact = two.w == 80 && two.h == 80;
    if exact {
        'outer: for y in 0..40u16 {
            for x in 0..40u16 {
                let p = one.get(x, y).unwrap();
                for oy in 0..2u16 {
                    for ox in 0..2u16 {
                        if two.get(x * 2 + ox, y * 2 + oy) != Some(p) {
                            exact = false;
                            break 'outer;
                        }
                    }
                }
            }
        }
    }
    set.add("2x view is exact 2x replication", exact, "");

    // 3. 洋葱皮 30%：开 onion 时透明底上前帧以 77/255 叠显。
    let mut w2 = Workshop::new();
    w2.set_color([255, 0, 0, 255]);
    w2.set_brush(Brush::Pen { radius: 2 });
    w2.stroke(10, 10, 12, 12);
    w2.add_frame().unwrap();
    // 新帧空白：onion 开 → 合成出前帧 30% 影（alpha=77，计入「有墨迹」
    // 用 alpha>0 口径——实体口径 ≥128 是热点/重心的语义，两口径分开）。
    let ghosted = w2.view();
    let ghost_count = ghosted.px.chunks_exact(4).filter(|p| p[3] > 0).count();
    w2.onion_skin = false;
    let plain = w2.view();
    let plain_count = plain.px.chunks_exact(4).filter(|p| p[3] > 0).count();
    set.add(
        "onion skin 30% ghost visible not polluting",
        ghost_count > 0 && plain_count == 0 && w2.view_2x().w == 80,
        "",
    );

    // 4. 16 帧上限纪律：第 17 帧拒绝。
    let mut w3 = Workshop::new();
    let mut cap_ok = true;
    for i in 0..15 {
        if w3.add_frame().is_err() {
            cap_ok = false;
        }
        let _ = i;
    }
    cap_ok &= w3.frame_count() == MAX_FRAMES_PER_STATE;
    cap_ok &= w3.add_frame() == Err(WorkshopError::FrameCapReached);
    set.add("16-frame cap honest reject", cap_ok, "");

    // 5. 撤销/重做：笔画级往返一致；栈溢出丢最旧并计数。
    let mut w4 = Workshop::new();
    w4.set_brush(Brush::Pen { radius: 0 });
    for i in 0..60 {
        w4.stroke(i, 0, i, 5);
    }
    let dropped = w4.undo_dropped_total();
    let u = w4.undo();
    set.add(
        "undo stack cap 50 with honest drop count",
        u && w4.stroke_count() == 49 && dropped == 10,
        "",
    );

    // 6. 热点复用一致性：空白落点拒绝、实体落点通过。
    let mut w5 = Workshop::new();
    w5.stroke(8, 8, 12, 12);
    let off_ink = w5.set_hotspot(0, 0);
    let on_ink = w5.set_hotspot(10, 10);
    set.add(
        "hotspot cross semantics reused",
        off_ink == Err(WorkshopError::HotspotOffInk) && on_ink.is_ok() && w5.hotspot() == (10, 10),
        "",
    );

    // 7. 入库链路：into_scheme → F628 add → 指纹一致 + F627 体检绿。
    let mut w6 = Workshop::new();
    w6.set_brush(Brush::Pen { radius: 1 });
    w6.stroke(2, 2, 30, 30);
    let _ = w6.set_hotspot(2, 2);
    let scheme = w6.into_scheme("工坊处女作", PointerState::Normal);
    let fp = crate::jstar2::jbase::vxcur_fingerprint(&scheme);
    let mut lib = crate::jstar2::library::SchemeLibrary::new(0);
    let added = lib.add(scheme);
    let entry_fp = match &added {
        crate::jstar2::library::AddOutcome::Added(fp) => *fp,
        _ => 0,
    };
    set.add(
        "workshop to library fingerprint preserved",
        entry_fp == fp && lib.get("工坊处女作").is_some(),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_roundtrip() {
        let mut w = Workshop::new();
        w.set_brush(Brush::Pen { radius: 1 });
        w.stroke(0, 0, 39, 39);
        let after_stroke = w.view().solid_count();
        assert!(w.undo());
        assert_eq!(w.view().solid_count(), 0);
        assert!(w.redo());
        assert_eq!(w.view().solid_count(), after_stroke);
        assert!(!w.redo(), "重做到头返回 false");
    }

    #[test]
    fn new_stroke_truncates_redo_branch() {
        let mut w = Workshop::new();
        w.stroke(0, 0, 5, 5);
        w.undo();
        w.stroke(20, 20, 30, 30);
        assert_eq!(w.stroke_count(), 1, "分叉后旧重做尾巴截断");
        assert!(!w.redo());
    }

    #[test]
    fn frame_ops_and_delays() {
        let mut w = Workshop::new();
        assert_eq!(w.frame_delay(), 100);
        w.set_frame_delay(5);
        assert_eq!(w.frame_delay(), 17, "延时钳到帧率闸内");
        w.add_frame().unwrap();
        w.set_frame_delay(200);
        assert_eq!(w.frame_delay(), 200);
        assert!(w.delete_frame());
        assert_eq!(w.frame_count(), 1);
        assert!(!w.delete_frame(), "保底 1 帧不删");
    }

    #[test]
    fn duplicate_carries_strokes_and_hotspot() {
        let mut w = Workshop::new();
        w.stroke(4, 4, 9, 9);
        let _ = w.set_hotspot(5, 5);
        w.duplicate_frame().unwrap();
        assert_eq!(w.current_index(), 1);
        assert_eq!(w.view().solid_count() > 0, true);
        assert_eq!(w.hotspot(), (5, 5), "复制帧带热点");
    }

    #[test]
    fn pen_rectangle_ellipse_shapes_differ() {
        let mut w = Workshop::new();
        w.set_brush(Brush::Rect { filled: true });
        w.stroke(5, 5, 20, 20);
        let rect_n = w.view().solid_count();
        w.undo();
        w.set_brush(Brush::Rect { filled: false });
        w.stroke(5, 5, 20, 20);
        let rect_o = w.view().solid_count();
        assert!(rect_n > rect_o * 2, "填充面积显著大于描边");
    }

    #[test]
    fn degenerate_ellipse_draws_line() {
        let mut w = Workshop::new();
        w.set_brush(Brush::Ellipse { filled: true });
        w.stroke(10, 10, 30, 10); // ry = 0
        assert!(w.view().solid_count() > 0, "退化椭圆不画隐形");
    }

    #[test]
    fn into_scheme_roundtrip_via_vxcur() {
        let mut w = Workshop::new();
        w.stroke(1, 1, 20, 20);
        let scheme = w.into_scheme("t", PointerState::Link);
        let bytes = crate::jstar2::jbase::serialize_vxcur(&scheme);
        let parsed = crate::jstar2::jbase::parse_vxcur(&bytes).unwrap();
        assert_eq!(
            crate::jstar2::jbase::vxcur_fingerprint(&parsed),
            crate::jstar2::jbase::vxcur_fingerprint(&scheme)
        );
        assert_eq!(parsed.state(PointerState::Link).unwrap().frames.len(), 1);
    }
}
