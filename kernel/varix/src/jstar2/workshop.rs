//! F625 指针绘制工坊 · 完整设计（STAR I 主册 J-C 组）· v2 深化版。
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
//! - 笔刷族：钢笔（自由笔迹，半径可调）/矩形/椭圆（描边与填充两用）/
//!   橡皮（写透明——改错是画的一部分）/水彩（alpha 混合笔——叠加出
//!   过渡色，不覆盖底色）+ 取色器（从画布取色，非笔画）；
//! - 自由笔迹捕获状态机：起笔-拖动-收笔三段式（Idle/Drawing），采样点
//!   上限 512（超限诚实计数不静默丢）、<1px 抖动点过滤、收笔时三点
//!   滑动窗平滑（折线毛刺→顺滑曲线）——重放与预览共用同一重放口；
//! - 出界钳制诚实：画布外采样点钳到边界并计数（不丢弃不崩溃）；
//! - 对称模式：水平镜像（指针常左右对称——一笔出双份，镜像轴为画布
//!   垂直中线）；
//! - 洋葱皮：前一帧以 30% 透明度（alpha ×77/255）垫底衬显示——画动画
//!   指针不盲画；预览合成不污染工作帧；热点校验只认当前帧实体、
//!   幽灵墨迹不作数（边界语义：能对准的才准画热点）；
//! - 逐帧时间轴：16 帧上限（第 17 帧诚实拒绝）、增删/复制/重排/每帧
//!   延时（默认 100ms，钳 17..=60000 帧率闸内）；帧缩略图（8×8 盒式
//!   降采样——时间轴一眼看全）；总时长 = Σ帧延时；
//! - 试玩直通（判据无感标准「画完试玩一键」）：注入钟驱动的播放头
//!   状态机——play/tick/stop 全确定复现，循环计数留对账；
//! - 撤销：笔画级撤销/重做栈（每帧独立，容量 50——超限丢最旧并计数）；
//!   清空帧/整帧填充同为可撤销笔画（重放模型的一等公民）；
//! - 热点：复用 F156 十字件语义（`set_hotspot` 校验实体落点）；
//! - 入库链路：`into_scheme()` 产出 jbase [`CursorSchemeModel`]（F628
//!   add 直收）；`into_scheme_checked()` 空帧诚实拒绝（空白帧进方案
//!   是隐形缺陷——拒绝并列出帧号）；`into_scheme_multi()` 一稿多态
//!   导出（同一作品铺 Normal/Link/… 多态，内容逐字节同源）。

use crate::checks::CheckSet;
use crate::jstar2::jbase::{
    fill_ellipse, fill_rect, stroke_line, stroke_rect, CursorFrame, CursorSchemeModel,
    OriginKind, PixBuf, PointerState, MAX_FRAMES_PER_STATE,
};
use alloc::string::String;
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
/// 自由笔迹单笔采样点上限（超限诚实计数——不静默丢、不崩溃）。
pub const FREEHAND_POINT_CAP: usize = 512;
/// 帧缩略图边长（时间轴小图）。
pub const THUMB_PX: u16 = 8;

// ---------------------------------------------------------------------------
// 笔刷与笔画
// ---------------------------------------------------------------------------

/// 笔刷族（判据「三笔刷全功能」起步 + 工坊成熟版两笔一器）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Brush {
    /// 钢笔：自由笔迹（radius = 半宽 0..4）。
    Pen { radius: u16 },
    /// 矩形：filled = 填充，否则描边。
    Rect { filled: bool },
    /// 椭圆：filled = 填充，否则描边。
    Ellipse { filled: bool },
    /// 橡皮：写全透明（改错是画的一部分）。
    Eraser { radius: u16 },
    /// 水彩：alpha 混合笔（src-over 叠加——重叠处出过渡色不覆盖）。
    Watercolor { radius: u16, alpha: u8 },
}

impl Brush {
    pub fn zh(self) -> &'static str {
        match self {
            Brush::Pen { .. } => "钢笔",
            Brush::Rect { .. } => "矩形",
            Brush::Ellipse { .. } => "椭圆",
            Brush::Eraser { .. } => "橡皮",
            Brush::Watercolor { .. } => "水彩",
        }
    }
}

/// 单像素 alpha 混合（src-over，与 `PixBuf::blend_over` 同公式——
/// 水彩笔的像素级形态；jbase 主笔语义保持覆盖写不动）。
fn blend_pixel(cv: &mut PixBuf, x: i64, y: i64, rgba: [u8; 4]) {
    if x < 0 || y < 0 || x >= cv.w as i64 || y >= cv.h as i64 {
        return;
    }
    let (dx, dy) = (x as u16, y as u16);
    let dst = cv.get(dx, dy).unwrap_or([0, 0, 0, 0]);
    let sa = rgba[3] as u32;
    let da = dst[3] as u32;
    let oa = sa + da * (255 - sa) / 255;
    let mix = |s: u8, d: u8| -> u8 {
        if oa == 0 {
            0
        } else {
            ((s as u32 * sa + d as u32 * da * (255 - sa) / 255) / oa).clamp(0, 255) as u8
        }
    };
    cv.set(dx, dy, [mix(rgba[0], dst[0]), mix(rgba[1], dst[1]), mix(rgba[2], dst[2]), oa as u8]);
}

/// 混合写线段（水彩笔用；Bresenham 主循环 + 半径方刷，与
/// `jbase::stroke_line` 同轨迹不同写语义）。
fn blend_line(cv: &mut PixBuf, x0: i64, y0: i64, x1: i64, y1: i64, radius: u16, rgba: [u8; 4]) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx: i64 = if x0 < x1 { 1 } else { -1 };
    let sy: i64 = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut x = x0;
    let mut y = y0;
    let r = radius as i64;
    loop {
        for oy in -r..=r {
            for ox in -r..=r {
                blend_pixel(cv, x + ox, y + oy, rgba);
            }
        }
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// 一笔（撤销栈的最小单位——重放与实际绘制共用同一重放口）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stroke {
    /// 单段笔画（几何笔刷 + 钢笔点按）。
    Segment {
        brush: Brush,
        color: [u8; 4],
        x0: i64,
        y0: i64,
        x1: i64,
        y1: i64,
    },
    /// 自由笔迹（多点折线——收笔时已平滑）。
    Freehand {
        points: Vec<(i64, i64)>,
        radius: u16,
        color: [u8; 4],
        erase: bool,
        blend: bool,
    },
    /// 清空整帧（可撤销——与手动逐笔擦除区分）。
    ClearFrame,
    /// 整帧填充（可撤销）。
    FillAll { color: [u8; 4] },
}

impl Stroke {
    /// 镜像本笔（对称模式：水平翻转 x——画布垂直中线为轴）。
    fn mirrored(&self) -> Stroke {
        let flip = |x: i64| (CANVAS_PX as i64 - 1) - x;
        match self {
            Stroke::Segment { brush, color, x0, y0, x1, y1 } => Stroke::Segment {
                brush: *brush,
                color: *color,
                x0: flip(*x0),
                y0: *y0,
                x1: flip(*x1),
                y1: *y1,
            },
            Stroke::Freehand { points, radius, color, erase, blend } => Stroke::Freehand {
                points: points.iter().map(|(x, y)| (flip(*x), *y)).collect(),
                radius: *radius,
                color: *color,
                erase: *erase,
                blend: *blend,
            },
            other => other.clone(),
        }
    }

    /// 重放到画布（撤销重做与实际绘制共用——所见即所存）。
    fn replay(&self, cv: &mut PixBuf) {
        match self {
            Stroke::Segment { brush, color, x0, y0, x1, y1 } => {
                let (c, erase) = if matches!(brush, Brush::Eraser { .. }) {
                    ([0, 0, 0, 0], true)
                } else {
                    (*color, false)
                };
                match brush {
                    Brush::Pen { radius } | Brush::Eraser { radius } => {
                        stroke_line(cv, *x0, *y0, *x1, *y1, *radius, c);
                    }
                    Brush::Watercolor { radius, alpha } => {
                        let mut wc = c;
                        wc[3] = *alpha;
                        blend_line(cv, *x0, *y0, *x1, *y1, *radius, wc);
                    }
                    Brush::Rect { filled } => {
                        if *filled && !erase {
                            fill_rect(cv, *x0, *y0, *x1, *y1, c);
                        } else {
                            stroke_rect(cv, *x0, *y0, *x1, *y1, 0, c);
                        }
                    }
                    Brush::Ellipse { filled } => {
                        let (cx, cy) = ((*x0 + *x1) / 2, (*y0 + *y1) / 2);
                        let (rx, ry) = ((*x0 - *x1).abs() / 2, (*y0 - *y1).abs() / 2);
                        if rx == 0 || ry == 0 {
                            // 退化：单轴为 0 时画线（诚实处理，不画隐形椭圆）。
                            stroke_line(cv, *x0, *y0, *x1, *y1, 0, c);
                        } else if *filled && !erase {
                            fill_ellipse(cv, cx, cy, rx, ry, c);
                        } else {
                            crate::jstar2::jbase::stroke_ellipse(cv, cx, cy, rx, ry, 0, c);
                        }
                    }
                }
            }
            Stroke::Freehand { points, radius, color, erase, blend } => {
                let mut c = *color;
                if *erase {
                    c = [0, 0, 0, 0];
                }
                for w in points.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    if *blend && !*erase {
                        let mut ac = c;
                        ac[3] = 255;
                        blend_line(cv, a.0, a.1, b.0, b.1, *radius, ac);
                    } else {
                        stroke_line(cv, a.0, a.1, b.0, b.1, *radius, c);
                    }
                }
                if points.len() == 1 {
                    let p = points[0];
                    if *blend && !*erase {
                        let mut ac = c;
                        ac[3] = 255;
                        blend_pixel(cv, p.0, p.1, ac);
                    } else {
                        stroke_line(cv, p.0, p.1, p.0, p.1, *radius, c);
                    }
                }
            }
            Stroke::ClearFrame => {
                *cv = PixBuf::new(CANVAS_PX, CANVAS_PX);
            }
            Stroke::FillAll { color } => {
                fill_rect(cv, 0, 0, CANVAS_PX as i64 - 1, CANVAS_PX as i64 - 1, *color);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 自由笔迹捕获状态机（起笔-拖动-收笔三段式）
// ---------------------------------------------------------------------------

/// 笔迹捕获状态（交互状态机：第三章「每个可交互元素都是一台完整的状态机」）。
#[derive(Clone, Debug, PartialEq, Eq)]
enum CaptureState {
    Idle,
    Drawing,
}

/// 一次捕获会话的采样账（诚实计量面）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureMetrics {
    /// 原始采样点数（含抖动）。
    pub raw_points: usize,
    /// <1px 抖动过滤掉的分立点数。
    pub jitter_filtered: usize,
    /// 出界钳制到画布内的点数。
    pub clamped: usize,
    /// 超出单笔上限被诚实拒绝的点数。
    pub cap_dropped: usize,
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
        s.replay(&mut self.canvas);
        self.strokes.push(s);
        if self.strokes.len() > UNDO_CAP {
            let drop = self.strokes.len() - UNDO_CAP;
            self.strokes.drain(..drop);
            self.undo_dropped += drop;
            self.undo_pos = self.strokes.len();
        } else {
            self.undo_pos = self.strokes.len();
        }
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

/// 试玩播放头（注入钟驱动——全确定复现，不持真实时钟）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playback {
    /// 播放中（stop 才回 Idle——播放中再 play 是重放行为）。
    pub playing: bool,
    /// 播放头所在帧。
    pub frame: usize,
    /// 当前帧内已消耗毫秒。
    pub elapsed_ms: u64,
    /// 完成循环数。
    pub loops: usize,
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
    /// 对称模式（水平镜像——一笔出双份）。
    pub mirror_x: bool,
    /// 自由笔迹捕获状态机。
    capture: CaptureState,
    capture_points: Vec<(i64, i64)>,
    capture_metrics: CaptureMetrics,
    /// 试玩播放头。
    playback: Playback,
    /// 文档元数据（入库与分享的署名面）。
    pub title: String,
    pub author: String,
}

/// 工坊操作结果（诚实错误面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkshopError {
    FrameCapReached,
    NoFrame,
    HotspotOffInk,
    /// 空白帧导出拒绝（帧号从 0 计——诚实列位不静默放行）。
    EmptyFrame(usize),
    /// 取色落点无墨（alpha=0——如实返回 None 的枚举面）。
    NothingToPick,
    /// 播放头越界（帧被删后旧播放头失效的防御面）。
    PlayheadOutOfRange,
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
            mirror_x: false,
            capture: CaptureState::Idle,
            capture_points: Vec::new(),
            capture_metrics: CaptureMetrics::default(),
            playback: Playback { playing: false, frame: 0, elapsed_ms: 0, loops: 0 },
            title: String::new(),
            author: String::new(),
        }
    }

    pub fn set_color(&mut self, c: [u8; 4]) {
        self.color = c;
    }

    pub fn color(&self) -> [u8; 4] {
        self.color
    }

    pub fn set_brush(&mut self, b: Brush) {
        self.brush = b;
    }

    pub fn brush(&self) -> Brush {
        self.brush
    }

    // -- 帧管理 ----------------------------------------------------------

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

    /// 删帧（保底 1 帧；删除后播放头越界防御钳位）。
    pub fn delete_frame(&mut self) -> bool {
        if self.frames.len() <= 1 {
            return false;
        }
        self.frames.remove(self.current);
        if self.current >= self.frames.len() {
            self.current = self.frames.len() - 1;
        }
        if self.playback.frame >= self.frames.len() {
            self.playback.frame = 0;
            self.playback.elapsed_ms = 0;
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

    /// 帧重排（from 移到 to——时间轴拖拽排序的模型面；越界诚实拒绝）。
    pub fn move_frame(&mut self, from: usize, to: usize) -> bool {
        if from >= self.frames.len() || to >= self.frames.len() || from == to {
            return false;
        }
        let f = self.frames.remove(from);
        self.frames.insert(to, f);
        // 播放头与工作帧跟随语义：当前帧被移走则跟到新位置。
        if self.current == from {
            self.current = to;
        } else if from < self.current && to >= self.current {
            self.current -= 1;
        } else if from > self.current && to <= self.current {
            self.current += 1;
        }
        true
    }

    pub fn set_frame_delay(&mut self, ms: u32) {
        self.frames[self.current].delay_ms = ms.max(17).min(60000);
    }

    pub fn frame_delay(&self) -> u32 {
        self.frames[self.current].delay_ms
    }

    /// 总时长 = Σ帧延时（时间轴标尺的数字面）。
    pub fn total_duration_ms(&self) -> u64 {
        self.frames.iter().map(|f| f.delay_ms as u64).sum()
    }

    /// 帧缩略图：THUMB_PX×THUMB_PX 盒式降采样（40/8=5 盒边）——
    /// 块内有实体（alpha≥128）则取实体像素 RGB 均值 + alpha 取块内最大，
    /// 全空块透明（缩略图不说谎：空白帧缩略图就是空白）。
    pub fn frame_thumbnail(&self, i: usize) -> Option<PixBuf> {
        let f = self.frames.get(i)?;
        let step = (CANVAS_PX / THUMB_PX) as i64;
        let mut out = PixBuf::new(THUMB_PX, THUMB_PX);
        for ty in 0..THUMB_PX as i64 {
            for tx in 0..THUMB_PX as i64 {
                let mut sr: u32 = 0;
                let mut sg: u32 = 0;
                let mut sb: u32 = 0;
                let mut n: u32 = 0;
                let mut max_a: u8 = 0;
                for oy in 0..step {
                    for ox in 0..step {
                        let sx = (tx * step + ox) as u16;
                        let sy = (ty * step + oy) as u16;
                        if let Some(p) = f.canvas.get(sx, sy) {
                            if p[3] >= 128 {
                                sr += p[0] as u32;
                                sg += p[1] as u32;
                                sb += p[2] as u32;
                                n += 1;
                                max_a = max_a.max(p[3]);
                            }
                        }
                    }
                }
                if n > 0 {
                    out.set(
                        tx as u16,
                        ty as u16,
                        [(sr / n) as u8, (sg / n) as u8, (sb / n) as u8, max_a],
                    );
                }
            }
        }
        Some(out)
    }

    // -- 试玩直通（判据「画完试玩一键」） --------------------------------

    /// 开始播放（从当前帧起；播放中再 play = 从头重放）。
    pub fn play(&mut self) {
        self.playback = Playback {
            playing: true,
            frame: self.current,
            elapsed_ms: 0,
            loops: 0,
        };
    }

    /// 注入钟步进（dt 毫秒——全确定，宿主测试与实机共用同一推进逻辑）。
    pub fn tick(&mut self, dt_ms: u64) {
        if !self.playback.playing {
            return;
        }
        self.playback.elapsed_ms += dt_ms;
        while self.playback.elapsed_ms >= self.frames[self.playback.frame].delay_ms as u64 {
            self.playback.elapsed_ms -= self.frames[self.playback.frame].delay_ms as u64;
            self.playback.frame += 1;
            if self.playback.frame >= self.frames.len() {
                self.playback.frame = 0;
                self.playback.loops += 1;
            }
        }
    }

    /// 停止播放（播放头停在暂停位——不跳回，续播从暂停位继续）。
    pub fn stop(&mut self) {
        self.playback.playing = false;
    }

    /// 播放头视图（只读面）。
    pub fn playback(&self) -> &Playback {
        &self.playback
    }

    /// 播放头越界自检（删帧后旧播放头的防御断言面）。
    pub fn playback_sane(&self) -> bool {
        self.playback.frame < self.frames.len()
    }

    // -- 绘制交互 --------------------------------------------------------

    /// 落笔（几何笔刷的单段快捷口——起终点一次性给齐）。
    pub fn stroke(&mut self, x0: i64, y0: i64, x1: i64, y1: i64) {
        let s = Stroke::Segment { brush: self.brush, color: self.color, x0, y0, x1, y1 };
        self.apply_with_symmetry(s);
    }

    /// 自由笔迹：起笔（Idle→Drawing；重复起笔 = 上笔自动收笔）。
    pub fn begin_stroke(&mut self, x: i64, y: i64) {
        if self.capture == CaptureState::Drawing {
            self.end_stroke();
        }
        self.capture = CaptureState::Drawing;
        self.capture_points.clear();
        self.capture_metrics = CaptureMetrics::default();
        self.capture_point(x, y);
    }

    /// 自由笔迹：拖动采样（仅 Drawing 态有效——Idle 采样被诚实忽略）。
    pub fn extend_stroke(&mut self, x: i64, y: i64) {
        if self.capture != CaptureState::Drawing {
            return;
        }
        self.capture_point(x, y);
    }

    /// 单点采样：钳界 → 抖动过滤 → 上限诚实计数。
    fn capture_point(&mut self, x: i64, y: i64) {
        let m = &mut self.capture_metrics;
        m.raw_points += 1;
        let max = CANVAS_PX as i64 - 1;
        let (cx, cy) = (x.clamp(0, max), y.clamp(0, max));
        if cx != x || cy != y {
            m.clamped += 1;
        }
        if let Some(&(lx, ly)) = self.capture_points.last() {
            let d2 = (cx - lx) * (cx - lx) + (cy - ly) * (cy - ly);
            if d2 == 0 {
                m.jitter_filtered += 1;
                return;
            }
        }
        if self.capture_points.len() >= FREEHAND_POINT_CAP {
            m.cap_dropped += 1;
            return;
        }
        self.capture_points.push((cx, cy));
    }

    /// 自由笔迹：收笔（三点滑动窗平滑 → 入栈重放；Idle 态收笔为 no-op）。
    pub fn end_stroke(&mut self) -> bool {
        if self.capture != CaptureState::Drawing || self.capture_points.is_empty() {
            self.capture = CaptureState::Idle;
            return false;
        }
        // 三点滑动窗平滑（首尾点保留——折线毛刺顺滑，形状不走样）。
        let raw = core::mem::take(&mut self.capture_points);
        let mut pts = Vec::with_capacity(raw.len());
        pts.push(raw[0]);
        for w in raw[1..raw.len().saturating_sub(1)].windows(3) {
            pts.push(((w[0].0 + w[1].0 + w[2].0) / 3, (w[0].1 + w[1].1 + w[2].1) / 3));
        }
        if raw.len() > 1 {
            pts.push(raw[raw.len() - 1]);
        }
        let (erase, blend, radius) = match self.brush {
            Brush::Eraser { radius } => (true, false, radius),
            Brush::Watercolor { radius, .. } => (false, true, radius),
            Brush::Pen { radius } => (false, false, radius),
            // 几何笔刷误入自由手捕获：按钢笔语义落地（状态机不吞事件）。
            _ => (false, false, 0),
        };
        let s = Stroke::Freehand { points: pts, radius, color: self.color, erase, blend };
        self.apply_with_symmetry(s);
        self.capture = CaptureState::Idle;
        true
    }

    /// 笔画应用（对称模式：一笔镜像出双份——两份都是可撤销单元的一部分）。
    fn apply_with_symmetry(&mut self, s: Stroke) {
        if self.mirror_x {
            let m = s.mirrored();
            self.frames[self.current].apply(s);
            self.frames[self.current].apply(m);
        } else {
            self.frames[self.current].apply(s);
        }
    }

    /// 当前捕获指标（诚实计量面——原始/过滤/钳制/超限四数全露出）。
    pub fn capture_metrics(&self) -> CaptureMetrics {
        self.capture_metrics
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

    /// 清空当前帧（可撤销——入栈为一笔）。
    pub fn clear_frame(&mut self) {
        self.apply_with_symmetry(Stroke::ClearFrame);
    }

    /// 整帧填充（可撤销）。
    pub fn fill_all(&mut self) {
        self.apply_with_symmetry(Stroke::FillAll { color: self.color });
    }

    /// 取色器：从当前帧画布取色（无墨诚实拒绝—— onion 幽灵不作数）。
    pub fn pick_color(&mut self, x: u16, y: u16) -> Result<[u8; 4], WorkshopError> {
        let f = &self.frames[self.current];
        match f.canvas.get(x, y) {
            Some(p) if p[3] > 0 => {
                self.color = p;
                Ok(p)
            }
            _ => Err(WorkshopError::NothingToPick),
        }
    }

    // -- 热点（F156 十字件语义复用） --------------------------------------

    /// 热点设置（F156 十字件语义：必须落在当前帧实体上——洋葱皮幽灵
    /// 墨迹不作数，边界语义：能对准的才准画热点）。
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

    // -- 预览 -------------------------------------------------------------

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

    /// 播放头所在帧的 2x 视图（试玩面——与工作面独立，互不污染）。
    pub fn playback_view_2x(&self) -> PixBuf {
        self.frames[self.playback.frame].canvas.scale_integer2x()
    }

    // -- 入库链路 ----------------------------------------------------------

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

    /// 署名版产出（文档元数据参与命名：「标题 · 作者」；无作者退化为标题）。
    pub fn into_scheme_authored(self, state: PointerState) -> CursorSchemeModel {
        let name = if self.author.is_empty() || self.title.is_empty() {
            if self.title.is_empty() { String::from("未命名工坊作品") } else { self.title.clone() }
        } else {
            let mut n = self.title.clone();
            n.push_str(" · ");
            n.push_str(&self.author);
            n
        };
        self.into_scheme(&name, state)
    }

    /// 空帧检查（导出前诚实体检——返回首个空白帧号）。
    pub fn first_empty_frame(&self) -> Option<usize> {
        self.frames.iter().position(|f| f.canvas.solid_count() == 0)
    }

    /// 带检查的产出（空白帧诚实拒绝并列出帧号——不静默放行隐形缺陷）。
    pub fn into_scheme_checked(
        self,
        name: &str,
        state: PointerState,
    ) -> Result<CursorSchemeModel, WorkshopError> {
        match self.first_empty_frame() {
            Some(i) => Err(WorkshopError::EmptyFrame(i)),
            None => Ok(self.into_scheme(name, state)),
        }
    }

    /// 一稿多态导出（同一作品铺多态——帧内容逐字节同源，热点延时共用）。
    pub fn into_scheme_multi(
        self,
        name: &str,
        states: &[PointerState],
    ) -> Result<CursorSchemeModel, WorkshopError> {
        if let Some(i) = self.first_empty_frame() {
            return Err(WorkshopError::EmptyFrame(i));
        }
        let frames: Vec<CursorFrame> = self
            .frames
            .iter()
            .map(|f| CursorFrame::from_buf(f.hotspot.0, f.hotspot.1, f.delay_ms, f.canvas.clone()))
            .collect();
        let mut m = CursorSchemeModel::empty(name, OriginKind::Created);
        for st in states {
            m.set_state(*st, frames.clone());
        }
        Ok(m)
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

    // 8. 自由笔迹捕获状态机：起-拖-收三段式产出平滑折线。
    let mut w7 = Workshop::new();
    w7.set_brush(Brush::Pen { radius: 0 });
    w7.begin_stroke(2, 2);
    for i in 0..20u64 {
        w7.extend_stroke(2 + i as i64, 2 + (i * i / 40) as i64);
    }
    w7.end_stroke();
    let m = w7.capture_metrics();
    set.add(
        "freehand capture state machine paints smoothed polyline",
        w7.view().solid_count() > 0 && m.raw_points == 21 && m.cap_dropped == 0,
        "",
    );
    // Idle 态 extend 被诚实忽略（状态机不吞事件也不误收）。
    let mut w7b = Workshop::new();
    w7b.extend_stroke(10, 10);
    set.add(
        "freehand idle extend honestly ignored",
        w7b.capture_metrics().raw_points == 0 && w7b.view().solid_count() == 0,
        "",
    );

    // 9. 单笔采样上限：600 点诚实截到 512 并计数（不静默丢、不崩溃）。
    let mut w8 = Workshop::new();
    w8.begin_stroke(0, 0);
    for i in 0..600u64 {
        w8.extend_stroke((i % 40) as i64, (i / 40 % 40) as i64);
    }
    w8.end_stroke();
    let m8 = w8.capture_metrics();
    set.add(
        "freehand point cap 512 honest drop",
        m8.cap_dropped == 88 && m8.raw_points == 601,
        "",
    );

    // 10. 出界钳制诚实：画布外点钳到边界并计数（不丢弃不崩溃）。
    let mut w9 = Workshop::new();
    w9.begin_stroke(-5, -3);
    w9.extend_stroke(50, 60);
    w9.end_stroke();
    let m9 = w9.capture_metrics();
    set.add(
        "out-of-canvas points clamped and counted",
        m9.clamped == 2 && w9.view().solid_count() > 0,
        "",
    );

    // 11. 橡皮：画了再擦，墨迹归零。
    let mut w10 = Workshop::new();
    w10.set_brush(Brush::Pen { radius: 2 });
    w10.stroke(5, 5, 30, 30);
    w10.set_brush(Brush::Eraser { radius: 3 });
    w10.stroke(5, 5, 30, 30);
    set.add("eraser removes ink", w10.view().solid_count() == 0, "");

    // 12. 水彩：叠加出过渡色（混合不覆盖）——同点两笔 alpha 叠加，
    //     结果色 ≠ 任一单笔的覆盖写结果。
    let mut w11 = Workshop::new();
    w11.set_brush(Brush::Watercolor { radius: 0, alpha: 128 });
    w11.set_color([255, 0, 0, 128]);
    w11.stroke(10, 10, 10, 10);
    let once = w11.view().get(10, 10).unwrap();
    w11.stroke(10, 10, 10, 10);
    let twice = w11.view().get(10, 10).unwrap();
    set.add(
        "watercolor blends not replaces",
        twice[3] > once[3] && twice[3] < 255,
        "",
    );

    // 13. 取色器：有墨取色、无墨诚实拒绝；取到的色进当前色。
    let mut w12 = Workshop::new();
    w12.set_color([10, 200, 30, 255]);
    w12.set_brush(Brush::Pen { radius: 1 });
    w12.stroke(7, 7, 8, 8);
    let picked = w12.pick_color(7, 7);
    let picked_empty = w12.pick_color(39, 39);
    set.add(
        "eyedropper picks ink and honestly refuses blank",
        picked == Ok([10, 200, 30, 255])
            && w12.color() == [10, 200, 30, 255]
            && picked_empty == Err(WorkshopError::NothingToPick),
        "",
    );

    // 14. 对称模式：一笔镜像出双份（左右各一墨团）。
    let mut w13 = Workshop::new();
    w13.mirror_x = true;
    w13.set_brush(Brush::Pen { radius: 0 });
    w13.stroke(3, 20, 3, 20);
    let v13 = w13.view();
    set.add(
        "mirror mode paints symmetric pair",
        v13.solid(3, 20) && v13.solid(36, 20) && v13.solid_count() == 2,
        "",
    );

    // 15. 帧重排：内容跟随移动，工作帧/播放头跟随不越界。
    let mut w14 = Workshop::new();
    w14.set_brush(Brush::Pen { radius: 1 });
    w14.stroke(2, 2, 8, 8); // 帧 0 有墨
    w14.add_frame().unwrap();
    w14.add_frame().unwrap();
    w14.select_frame(2);
    w14.stroke(20, 20, 26, 26);
    let moved = w14.move_frame(2, 0);
    let fp0 = w14.frame_thumbnail(0).unwrap();
    let ok15 = moved
        && w14.current_index() == 0
        && fp0.get(4, 4).unwrap()[3] >= 128
        && w14.playback_sane();
    set.add("frame reorder follows content", ok15, "");
    set.add("frame reorder rejects out of range", !w14.move_frame(0, 9) && !w14.move_frame(9, 0), "");

    // 16. 帧缩略图：8×8 盒式降采样——有墨块取实体均值，空白块透明。
    let mut w15 = Workshop::new();
    w15.set_brush(Brush::Pen { radius: 2 });
    w15.stroke(18, 18, 21, 21); // 画布中心 → 缩略图 (4,4) 块
    let th = w15.frame_thumbnail(0).unwrap();
    let th_blank = Workshop::new().frame_thumbnail(0).unwrap();
    set.add(
        "frame thumbnail 8x8 honest box sample",
        th.w == 8 && th.h == 8 && th.solid(4, 4) && th_blank.solid_count() == 0,
        "",
    );

    // 17. 总时长 = Σ帧延时。
    let mut w16 = Workshop::new();
    w16.set_frame_delay(100);
    w16.add_frame().unwrap();
    w16.set_frame_delay(200);
    w16.add_frame().unwrap();
    w16.set_frame_delay(50);
    set.add("total duration sums delays", w16.total_duration_ms() == 350, "");

    // 18. 试玩直通：注入钟推进 + 循环计数 + 停在暂停位。
    let mut w17 = Workshop::new();
    w17.set_frame_delay(100);
    w17.add_frame().unwrap();
    w17.set_frame_delay(200);
    w17.select_frame(0);
    w17.play();
    w17.tick(150); // 帧 0（100ms）耗尽 → 进帧 1 余 50ms
    let at1 = w17.playback().frame == 1 && w17.playback().elapsed_ms == 50;
    w17.tick(200); // 帧 1（200ms）耗尽 → 回帧 0 计一循环
    let looped = w17.playback().frame == 0 && w17.playback().loops == 1;
    w17.stop();
    w17.tick(999); // 停止后 tick 不推进
    let stopped = w17.playback().frame == 0 && w17.playback().loops == 1;
    set.add(
        "playback playhead injected-clock loops and pauses",
        at1 && looped && stopped && w17.playback_view_2x().w == 80,
        "",
    );

    // 19. 清空帧/整帧填充可撤销（重放模型一等公民）。
    let mut w18 = Workshop::new();
    w18.set_brush(Brush::Pen { radius: 1 });
    w18.stroke(3, 3, 9, 9);
    w18.clear_frame();
    set.add("clear frame wipes and undoes", w18.view().solid_count() == 0 && w18.undo() && w18.view().solid_count() > 0, "");
    let mut w19 = Workshop::new();
    w19.set_color([1, 2, 3, 255]);
    w19.fill_all();
    let full = w19.view().solid_count();
    w19.undo();
    set.add("fill all paints and undoes", full == 1600 && w19.view().solid_count() == 0, "");

    // 20. 空帧导出诚实拒绝 + 一稿多态导出内容同源。
    let mut w20 = Workshop::new();
    w20.stroke(1, 1, 5, 5);
    w20.add_frame().unwrap(); // 帧 1 空白
    let rejected = w20.into_scheme_checked("t", PointerState::Normal);
    set.add(
        "empty frame export honestly rejected with index",
        matches!(rejected, Err(WorkshopError::EmptyFrame(1))),
        "",
    );
    let mut w21 = Workshop::new();
    w21.set_brush(Brush::Pen { radius: 1 });
    w21.stroke(1, 1, 9, 9);
    let multi = w21
        .into_scheme_multi("多态稿", &[PointerState::Normal, PointerState::Link])
        .unwrap();
    let (fa, fb) = (
        multi.state(PointerState::Normal).unwrap().frames.len(),
        multi.state(PointerState::Link).unwrap().frames.len(),
    );
    set.add(
        "multi-state export shares content",
        fa == 1 && fb == 1 && multi.state(PointerState::Link).is_some(),
        "",
    );

    // 21. 热点幽灵边界：洋葱皮幽灵墨迹不作热点落点（只认当前帧实体）。
    let mut w22 = Workshop::new();
    w22.set_brush(Brush::Pen { radius: 2 });
    w22.stroke(10, 10, 12, 12);
    w22.add_frame().unwrap(); // 当前帧空白、幽灵在前帧 (10..12)
    let ghost_rejected = w22.set_hotspot(11, 11) == Err(WorkshopError::HotspotOffInk);
    w22.onion_skin = false;
    let still_rejected = w22.set_hotspot(11, 11) == Err(WorkshopError::HotspotOffInk);
    set.add("hotspot ignores onion ghost ink", ghost_rejected && still_rejected, "");

    // 22. 署名导出：标题 · 作者 参与命名。
    let mut w23 = Workshop::new();
    w23.title = String::from("夜行箭");
    w23.author = String::from("林间");
    w23.set_brush(Brush::Pen { radius: 1 });
    w23.stroke(2, 2, 8, 8);
    let named = w23.into_scheme_authored(PointerState::Normal);
    set.add(
        "authored export composes title and author",
        named.name == "夜行箭 · 林间",
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

    #[test]
    fn freehand_smooth_preserves_endpoints() {
        let mut w = Workshop::new();
        w.begin_stroke(0, 0);
        for i in 1..10 {
            w.extend_stroke(i, i);
        }
        w.end_stroke();
        assert!(w.view().solid(0, 0), "首点保留");
        assert!(w.view().solid(9, 9), "尾点保留");
        assert!(w.view().solid(5, 5), "中段平滑后仍在轨迹附近");
    }

    #[test]
    fn freehand_begin_during_drawing_auto_ends() {
        let mut w = Workshop::new();
        w.begin_stroke(1, 1);
        w.extend_stroke(5, 5);
        w.begin_stroke(30, 30); // 未收笔再起笔 → 上笔自动收
        assert!(w.view().solid(1, 1) || w.view().solid(2, 2), "上一笔已落地");
        w.extend_stroke(35, 35);
        assert!(w.end_stroke(), "第二笔正常收");
    }

    #[test]
    fn watercolor_stack_three_passes() {
        let mut w = Workshop::new();
        w.set_brush(Brush::Watercolor { radius: 0, alpha: 85 });
        w.set_color([0, 0, 255, 85]);
        for _ in 0..3 {
            w.stroke(12, 12, 12, 12);
        }
        let p = w.view().get(12, 12).unwrap();
        assert!(p[3] > 85 * 2, "三层叠加显著浓于一层");
        assert!(p[3] < 255, "半透明水彩不顶格");
    }

    #[test]
    fn mirror_freehand_symmetric() {
        let mut w = Workshop::new();
        w.mirror_x = true;
        w.begin_stroke(5, 5);
        w.extend_stroke(8, 9);
        w.end_stroke();
        let v = w.view();
        assert!(v.solid(5, 5) && v.solid(34, 5), "镜像双份同时落地");
        assert_eq!(v.solid_count() % 2, 0, "对称墨量成对");
    }

    #[test]
    fn playback_never_out_of_range_after_delete() {
        let mut w = Workshop::new();
        w.add_frame().unwrap();
        w.play();
        assert!(w.delete_frame());
        assert!(w.playback_sane(), "删帧后播放头防御钳位");
        w.tick(1000);
        assert!(w.playback_sane());
    }

    #[test]
    fn authored_export_without_author_falls_back() {
        let mut w = Workshop::new();
        w.title = String::from("无名稿");
        w.set_brush(Brush::Pen { radius: 0 });
        w.stroke(3, 3, 4, 4);
        let m = w.into_scheme_authored(PointerState::Text);
        assert_eq!(m.name, "无名稿");
    }

    #[test]
    fn capture_metrics_full_accounting() {
        let mut w = Workshop::new();
        w.begin_stroke(-2, -3); // 钳制点 1（→ 0,0）
        w.extend_stroke(50, 60); // 钳制点 2（→ 39,39）
        for i in 0..600 {
            w.extend_stroke((i % 40) as i64, ((i / 40) % 40) as i64);
        }
        w.end_stroke();
        let m = w.capture_metrics();
        assert_eq!(m.clamped, 2, "两个出界采样点各计一次");
        assert!(m.cap_dropped > 0, "超限诚实计数");
        assert_eq!(m.raw_points, 602);
    }
}
