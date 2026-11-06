//! UNX-F50036 海报切割线标记与重叠区可视档（0x430a0 段）
//!
//! 锚点：UNX-F50036-J1 标记 ×2 档渲染一致；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段：`SEG_BASE = 0x430a0`，诊断码闭集 `0x430a0 + 1..=0x430a0 + 9`。
//!
//! 结构清单：
//! - 画布与裁切框：`Rect` 描述海报画布；`bleed` 是出血宽度，裁切框 `trim`
//!   为画布四边内缩 bleed 后的矩形。出血区只允许放标记，不参与印张拼接；
//! - 拼版：海报按 `sheet_w × sheet_h` 印张分块，块间保留 `overlap` 搭边。
//!   `grid_dims` 给出行列数，`tiles_for` 给出逐块矩形（末块按画布裁剪），
//!   `zones_for` 把相邻块的搭边登记为重叠区，供拼接工单与可视档共用；
//! - 标记：`MarkKind::{Crop, Trim, Overlap, Registration}`。裁切线（Crop）
//!   在裁切框四角各两条、共 8 条；裁切参考线（Trim）在四边中线各一条、
//!   共 4 条；每条重叠区登记两条边线；套准标记（Registration）为画布中心
//!   十字，仅印刷档可见；
//! - 可视档：`TIER_SCREEN`（屏显）与 `TIER_PRESS`（印刷）是两位掩码，
//!   可组合为 `TIER_BOTH`。档位只过滤可见性，绝不改写几何：同一计划在
//!   两档下共有的线段逐位相同，这就是 J1 的「×2 档渲染一致」；
//! - 计划与审计：`MarkPlan` 封印摘要 `digest`；`audit_marks` 校验标记
//!   顺序与档位归属；`verify_plan` 先验摘要，再按闭式重建标记逐条比对，
//!   并检查裁切线不侵入裁切框、重叠区为正且落在画布内；
//! - 注入核：`injection_core` 八类坏输入逐类断言诊断码。
//!
//! 为什么档位不能改几何：屏显档要隐藏套准线以免干扰拼版预览，但裁切线
//! 与重叠区必须与印刷档像素级一致；一旦渲染路径按档位「顺手」缩线宽或
//! 平移，输出就对不上印刷机。故档位只在 `render` 里做掩码过滤，几何由
//! `plan` 一次生成、两档共享，任何档位差异都只能是「多一条/少一条」，
//! 不能是「同一位置挪了半格」。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use libm::{ceilf, fabsf, sqrtf};

/// 诊断段基址（0x430a0 段，独占 0x20）。
pub const SEG_BASE: u32 = 0x430a0;

/// 屏显档掩码：预览用，隐藏套准线。
pub const TIER_SCREEN: u8 = 0b01;
/// 印刷档掩码：出片用，包含全部标记。
pub const TIER_PRESS: u8 = 0b10;
/// 两档共有：裁切线、参考线、重叠区线在屏显与印刷档都必须可见。
pub const TIER_BOTH: u8 = 0b11;

const E_POSTER: u16 = 1;
const E_SHEET: u16 = 2;
const E_OVERLAP: u16 = 3;
const E_BLEED: u16 = 4;
const E_MARK: u16 = 5;
const E_TILES: u16 = 6;
const E_DIGEST: u16 = 7;
const E_AUDIT: u16 = 8;
const E_RANGE: u16 = 9;

/// 诊断码闭集：每类坏输入对应一个稳定码，注入核逐类断言，不允许兜底。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CropError {
    /// 画布尺寸非正或非有限。
    BadPoster,
    /// 印张尺寸非正或非有限。
    BadSheet,
    /// 搭边非正或不少于印张边长。
    BadOverlap,
    /// 出血为负或吃掉了整个画布。
    BadBleed,
    /// 标记长度非正 / 间隙为负。
    BadMark,
    /// 行列数超过工程上限（512×512 内）。
    TooManyTiles,
    /// 重算摘要与封印不符。
    DigestDrift,
    /// 标记条数 / 顺序 / 档位归属不符。
    AuditMismatch,
    /// 相邻块或索引越界。
    Range,
}

impl CropError {
    /// 取诊断码（1..=9），与 `SEG_BASE` 相加得完整地址。
    pub fn code(&self) -> u16 {
        match self {
            CropError::BadPoster => E_POSTER,
            CropError::BadSheet => E_SHEET,
            CropError::BadOverlap => E_OVERLAP,
            CropError::BadBleed => E_BLEED,
            CropError::BadMark => E_MARK,
            CropError::TooManyTiles => E_TILES,
            CropError::DigestDrift => E_DIGEST,
            CropError::AuditMismatch => E_AUDIT,
            CropError::Range => E_RANGE,
        }
    }
}

/// 平面点，单位与海报坐标系一致（通常为 pt）。
#[derive(Clone, Copy, PartialEq)]
pub struct Point {
    /// 横坐标。
    pub x: f32,
    /// 纵坐标。
    pub y: f32,
}

impl Point {
    /// 由坐标构造点。
    pub const fn new(x: f32, y: f32) -> Self {
        Point { x, y }
    }
}

/// 轴对齐矩形，`x/y` 为左上角，`w/h` 为宽高。
#[derive(Clone, Copy, PartialEq)]
pub struct Rect {
    /// 左上角横坐标。
    pub x: f32,
    /// 左上角纵坐标。
    pub y: f32,
    /// 宽度。
    pub w: f32,
    /// 高度。
    pub h: f32,
}

impl Rect {
    /// 由左上角与宽高构造矩形。
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rect { x, y, w, h }
    }

    /// 右边界（不含）。
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// 下边界（不含）。
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// 面积；宽高可能为负（裁剪交集的中间态），此处如实返回。
    pub fn area(&self) -> f32 {
        self.w * self.h
    }

    /// 四边内缩 `d`：用于由画布得到裁切框。
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(self.x + d, self.y + d, self.w - 2.0 * d, self.h - 2.0 * d)
    }

    /// 两矩形交集；无交集时宽高为负，由调用方判定。
    pub fn intersect(&self, other: &Rect) -> Rect {
        let x0 = if self.x > other.x { self.x } else { other.x };
        let y0 = if self.y > other.y { self.y } else { other.y };
        let x1 = if self.right() < other.right() {
            self.right()
        } else {
            other.right()
        };
        let y1 = if self.bottom() < other.bottom() {
            self.bottom()
        } else {
            other.bottom()
        };
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

/// 线段，端点为 `a/b`。
#[derive(Clone, Copy, PartialEq)]
pub struct Segment {
    /// 起点。
    pub a: Point,
    /// 终点。
    pub b: Point,
}

impl Segment {
    /// 由两端点构造线段。
    pub const fn new(ax: f32, ay: f32, bx: f32, by: f32) -> Self {
        Segment {
            a: Point::new(ax, ay),
            b: Point::new(bx, by),
        }
    }

    /// 欧氏长度；标记长度自检用。
    pub fn len(&self) -> f32 {
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        sqrtf(dx * dx + dy * dy)
    }

    /// 轴向：1 水平、2 竖直、0 斜线。裁切线不允许斜线。
    pub fn axis(&self) -> u8 {
        if self.a.y == self.b.y {
            1
        } else if self.a.x == self.b.x {
            2
        } else {
            0
        }
    }
}

/// 标记种类；顺序编码同时用于审计的固定带顺序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    /// 裁切线：四角各两条，落在裁切框外侧。
    Crop,
    /// 裁切参考线：四边中线各一条，跨过裁切框。
    Trim,
    /// 重叠区边线：每条搭边两条，标出可视带。
    Overlap,
    /// 套准标记：画布中心十字，仅印刷档可见。
    Registration,
}

impl MarkKind {
    /// 取种类码（1..=4），参与摘要与审计。
    pub fn code(&self) -> u8 {
        match self {
            MarkKind::Crop => 1,
            MarkKind::Trim => 2,
            MarkKind::Overlap => 3,
            MarkKind::Registration => 4,
        }
    }

    /// 取中文名，供工单与诊断文本引用。
    pub fn label(&self) -> &'static str {
        match self {
            MarkKind::Crop => "裁切线",
            MarkKind::Trim => "参考线",
            MarkKind::Overlap => "重叠区线",
            MarkKind::Registration => "套准线",
        }
    }
}

/// 一条带档位归属的标记。`tiers` 是可见档掩码，不是几何参数。
#[derive(Clone, Copy)]
pub struct Mark {
    /// 标记种类。
    pub kind: MarkKind,
    /// 几何线段。
    pub seg: Segment,
    /// 可见档掩码：屏显 / 印刷 / 两档。
    pub tiers: u8,
}

/// 海报规格：画布、印张、搭边、出血与标记参数。
#[derive(Clone, Copy)]
pub struct PosterSpec {
    /// 画布宽（含出血）。
    pub poster_w: f32,
    /// 画布高（含出血）。
    pub poster_h: f32,
    /// 印张宽。
    pub sheet_w: f32,
    /// 印张高。
    pub sheet_h: f32,
    /// 相邻印张搭边宽。
    pub overlap: f32,
    /// 出血宽：裁切框相对画布内缩的量。
    pub bleed: f32,
    /// 单条标记长度。
    pub mark_len: f32,
    /// 标记与裁切框的间隙。
    pub mark_gap: f32,
}

impl PosterSpec {
    /// 以画布与印张尺寸构造规格，其余参数取海报印刷常用值。
    pub const fn new(poster_w: f32, poster_h: f32, sheet_w: f32, sheet_h: f32) -> Self {
        PosterSpec {
            poster_w,
            poster_h,
            sheet_w,
            sheet_h,
            overlap: 40.0,
            bleed: 3.0,
            mark_len: 12.0,
            mark_gap: 4.0,
        }
    }
}

/// 一块印张在画布坐标系中的落位。
#[derive(Clone, Copy)]
pub struct Tile {
    /// 列号（0 起）。
    pub col: u16,
    /// 行号（0 起）。
    pub row: u16,
    /// 印张矩形；末列/末行按画布裁剪。
    pub rect: Rect,
}

/// 一次生成、两档共享的标记计划。
pub struct MarkPlan {
    /// 画布矩形。
    pub poster: Rect,
    /// 印张宽。
    pub sheet_w: f32,
    /// 印张高。
    pub sheet_h: f32,
    /// 搭边宽。
    pub overlap: f32,
    /// 出血宽。
    pub bleed: f32,
    /// 标记长度。
    pub mark_len: f32,
    /// 标记间隙。
    pub mark_gap: f32,
    /// 裁切框。
    pub trim: Rect,
    /// 标记表：8 裁切、4 参考、2×重叠区、2 套准，顺序固定。
    pub marks: Vec<Mark>,
    /// 重叠区表，行优先登记。
    pub zones: Vec<Rect>,
    /// 计划摘要（FNV-1a 64）。
    pub digest: u64,
}

/// 校验规格：画布/印张为正，搭边小于印张，出血与标记参数合法。
pub fn validate(spec: &PosterSpec) -> Result<(), CropError> {
    if !spec.poster_w.is_finite()
        || !spec.poster_h.is_finite()
        || !(spec.poster_w > 0.0)
        || !(spec.poster_h > 0.0)
    {
        return Err(CropError::BadPoster);
    }
    if !spec.sheet_w.is_finite()
        || !spec.sheet_h.is_finite()
        || !(spec.sheet_w > 0.0)
        || !(spec.sheet_h > 0.0)
    {
        return Err(CropError::BadSheet);
    }
    if !spec.overlap.is_finite()
        || !(spec.overlap > 0.0)
        || spec.overlap >= spec.sheet_w
        || spec.overlap >= spec.sheet_h
    {
        return Err(CropError::BadOverlap);
    }
    if !spec.bleed.is_finite()
        || spec.bleed < 0.0
        || spec.bleed * 2.0 >= spec.poster_w.min(spec.poster_h)
    {
        return Err(CropError::BadBleed);
    }
    if !spec.mark_len.is_finite() || !(spec.mark_len > 0.0) {
        return Err(CropError::BadMark);
    }
    if !spec.mark_gap.is_finite() || spec.mark_gap < 0.0 {
        return Err(CropError::BadMark);
    }
    Ok(())
}

/// 单轴块数：`ceil((total - overlap) / (sheet - overlap))`，至少 1。
fn axis_count(total: f32, sheet: f32, overlap: f32) -> Result<u16, CropError> {
    let pitch = sheet - overlap;
    if !(pitch > 0.0) {
        return Err(CropError::BadOverlap);
    }
    let n = ceilf((total - overlap) / pitch);
    if !(n >= 1.0) {
        return Ok(1);
    }
    if n > 512.0 {
        return Err(CropError::TooManyTiles);
    }
    Ok(n as u16)
}

/// 计算行列数；超过 512×512 或总面积上限即拒绝。
fn grid_dims(
    poster: Rect,
    sheet_w: f32,
    sheet_h: f32,
    overlap: f32,
) -> Result<(u16, u16), CropError> {
    if !poster.w.is_finite() || !poster.h.is_finite() || !(poster.w > 0.0) || !(poster.h > 0.0) {
        return Err(CropError::BadPoster);
    }
    if !sheet_w.is_finite() || !sheet_h.is_finite() || !(sheet_w > 0.0) || !(sheet_h > 0.0) {
        return Err(CropError::BadSheet);
    }
    if !overlap.is_finite() || !(overlap > 0.0) || overlap >= sheet_w || overlap >= sheet_h {
        return Err(CropError::BadOverlap);
    }
    let cols = axis_count(poster.w, sheet_w, overlap)?;
    let rows = axis_count(poster.h, sheet_h, overlap)?;
    if cols as u32 * rows as u32 > 4096 {
        return Err(CropError::TooManyTiles);
    }
    Ok((cols, rows))
}

/// 求拼版行列数：海报按印张切块、块间留搭边后的列数与行数。
pub fn tile_grid(spec: &PosterSpec) -> Result<(u16, u16), CropError> {
    validate(spec)?;
    grid_dims(
        Rect::new(0.0, 0.0, spec.poster_w, spec.poster_h),
        spec.sheet_w,
        spec.sheet_h,
        spec.overlap,
    )
}

/// 逐块生成印张矩形；行优先，末列/末行按画布裁剪。
fn tiles_for(
    poster: Rect,
    sheet_w: f32,
    sheet_h: f32,
    overlap: f32,
) -> Result<Vec<Tile>, CropError> {
    let (cols, rows) = grid_dims(poster, sheet_w, sheet_h, overlap)?;
    let pitch_x = sheet_w - overlap;
    let pitch_y = sheet_h - overlap;
    let mut out: Vec<Tile> = Vec::new();
    let mut r = 0u16;
    while r < rows {
        let mut c = 0u16;
        while c < cols {
            let x = c as f32 * pitch_x;
            let y = r as f32 * pitch_y;
            let mut w = sheet_w;
            let mut h = sheet_h;
            if x + w > poster.w {
                w = poster.w - x;
            }
            if y + h > poster.h {
                h = poster.h - y;
            }
            if !(w > 0.0) || !(h > 0.0) {
                return Err(CropError::Range);
            }
            out.push(Tile {
                col: c,
                row: r,
                rect: Rect::new(x, y, w, h),
            });
            c += 1;
        }
        r += 1;
    }
    Ok(out)
}

/// 按规格列出全部印张落位，供拼版预览与拼接工单消费。
pub fn layout_tiles(spec: &PosterSpec) -> Result<Vec<Tile>, CropError> {
    validate(spec)?;
    tiles_for(
        Rect::new(0.0, 0.0, spec.poster_w, spec.poster_h),
        spec.sheet_w,
        spec.sheet_h,
        spec.overlap,
    )
}

/// 登记相邻印张的搭边：每对左右相邻、上下相邻各一条，行优先。
fn zones_for(
    poster: Rect,
    sheet_w: f32,
    sheet_h: f32,
    overlap: f32,
) -> Result<Vec<Rect>, CropError> {
    let (cols, _rows) = grid_dims(poster, sheet_w, sheet_h, overlap)?;
    let tiles = tiles_for(poster, sheet_w, sheet_h, overlap)?;
    let mut out: Vec<Rect> = Vec::new();
    let mut i = 0usize;
    while i < tiles.len() {
        let t = match tiles.get(i) {
            Some(v) => v,
            None => return Err(CropError::Range),
        };
        if t.col + 1 < cols {
            let right = match tiles.get(i + 1) {
                Some(v) => v,
                None => return Err(CropError::Range),
            };
            let z = t.rect.intersect(&right.rect);
            if !(z.w > 0.0) || !(z.h > 0.0) {
                return Err(CropError::Range);
            }
            out.push(z);
        }
        if let Some(below) = tiles.get(i + cols as usize) {
            if below.row == t.row + 1 {
                let z = t.rect.intersect(&below.rect);
                if !(z.w > 0.0) || !(z.h > 0.0) {
                    return Err(CropError::Range);
                }
                out.push(z);
            }
        }
        i += 1;
    }
    Ok(out)
}

/// 列出全部重叠区矩形；条数为 `(cols-1)×rows + (rows-1)×cols`。
pub fn overlap_zones(spec: &PosterSpec) -> Result<Vec<Rect>, CropError> {
    validate(spec)?;
    zones_for(
        Rect::new(0.0, 0.0, spec.poster_w, spec.poster_h),
        spec.sheet_w,
        spec.sheet_h,
        spec.overlap,
    )
}

/// 四角裁切线：左下/右下…顺序固定为 TL、TR、BL、BR，每角横竖各一条。
pub fn crop_marks(trim: &Rect, len: f32, gap: f32) -> Vec<Segment> {
    let x0 = trim.x;
    let y0 = trim.y;
    let x1 = trim.right();
    let y1 = trim.bottom();
    let mut v: Vec<Segment> = Vec::new();
    // 左上角：横线向左、竖线向上。
    v.push(Segment::new(x0 - gap - len, y0, x0 - gap, y0));
    v.push(Segment::new(x0, y0 - gap - len, x0, y0 - gap));
    // 右上角：横线向右、竖线向上。
    v.push(Segment::new(x1 + gap, y0, x1 + gap + len, y0));
    v.push(Segment::new(x1, y0 - gap - len, x1, y0 - gap));
    // 左下角：横线向左、竖线向下。
    v.push(Segment::new(x0 - gap - len, y1, x0 - gap, y1));
    v.push(Segment::new(x0, y1 + gap, x0, y1 + gap + len));
    // 右下角：横线向右、竖线向下。
    v.push(Segment::new(x1 + gap, y1, x1 + gap + len, y1));
    v.push(Segment::new(x1, y1 + gap, x1, y1 + gap + len));
    v
}

/// 裁切参考线：四边中线各一条，跨过裁切框，共 4 条。
pub fn trim_marks(trim: &Rect, len: f32) -> Vec<Segment> {
    let cx = (trim.x + trim.right()) * 0.5;
    let cy = (trim.y + trim.bottom()) * 0.5;
    let half = len * 0.5;
    let mut v: Vec<Segment> = Vec::new();
    v.push(Segment::new(cx, trim.y - half, cx, trim.y + half));
    v.push(Segment::new(cx, trim.bottom() - half, cx, trim.bottom() + half));
    v.push(Segment::new(trim.x - half, cy, trim.x + half, cy));
    v.push(Segment::new(trim.right() - half, cy, trim.right() + half, cy));
    v
}

/// 重叠区可视线：沿窄边方向画两条边线，正好夹出搭边带。
pub fn overlap_marks(zone: &Rect) -> Vec<Segment> {
    let mut v: Vec<Segment> = Vec::new();
    if zone.w <= zone.h {
        v.push(Segment::new(zone.x, zone.y, zone.x, zone.bottom()));
        v.push(Segment::new(zone.right(), zone.y, zone.right(), zone.bottom()));
    } else {
        v.push(Segment::new(zone.x, zone.y, zone.right(), zone.y));
        v.push(Segment::new(zone.x, zone.bottom(), zone.right(), zone.bottom()));
    }
    v
}

/// 套准标记：裁切框中心的十字，臂长为标记长度之半，共 2 条。
pub fn registration_marks(trim: &Rect, len: f32) -> Vec<Segment> {
    let cx = (trim.x + trim.right()) * 0.5;
    let cy = (trim.y + trim.bottom()) * 0.5;
    let arm = len * 0.5;
    let mut v: Vec<Segment> = Vec::new();
    v.push(Segment::new(cx - arm, cy, cx + arm, cy));
    v.push(Segment::new(cx, cy - arm, cx, cy + arm));
    v
}

fn fnv_u64(mut h: u64, v: u64) -> u64 {
    h ^= v;
    h.wrapping_mul(0x0000_0100_0000_01B3)
}

fn fnv_f32(h: u64, v: f32) -> u64 {
    fnv_u64(h, v.to_bits() as u64)
}

fn fnv_rect(h: u64, r: &Rect) -> u64 {
    let h = fnv_f32(h, r.x);
    let h = fnv_f32(h, r.y);
    let h = fnv_f32(h, r.w);
    fnv_f32(h, r.h)
}

fn fnv_seg(h: u64, s: &Segment) -> u64 {
    let h = fnv_f32(h, s.a.x);
    let h = fnv_f32(h, s.a.y);
    let h = fnv_f32(h, s.b.x);
    fnv_f32(h, s.b.y)
}

/// 计划摘要：画布、裁切框、拼版参数、标记（种类/档位/几何）、重叠区全部入账。
pub fn plan_digest(plan: &MarkPlan) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    h = fnv_rect(h, &plan.poster);
    h = fnv_rect(h, &plan.trim);
    h = fnv_f32(h, plan.sheet_w);
    h = fnv_f32(h, plan.sheet_h);
    h = fnv_f32(h, plan.overlap);
    h = fnv_f32(h, plan.bleed);
    h = fnv_f32(h, plan.mark_len);
    h = fnv_f32(h, plan.mark_gap);
    h = fnv_u64(h, plan.marks.len() as u64);
    let mut i = 0usize;
    while i < plan.marks.len() {
        if let Some(m) = plan.marks.get(i) {
            h = fnv_u64(h, m.kind.code() as u64);
            h = fnv_u64(h, m.tiers as u64);
            h = fnv_seg(h, &m.seg);
        }
        i += 1;
    }
    h = fnv_u64(h, plan.zones.len() as u64);
    let mut z = 0usize;
    while z < plan.zones.len() {
        if let Some(r) = plan.zones.get(z) {
            h = fnv_rect(h, r);
        }
        z += 1;
    }
    h
}

/// 重建计划应有的标记序列（闭式），`verify_plan` 用它逐条比对。
fn expected_marks(plan: &MarkPlan) -> Vec<Mark> {
    let mut out: Vec<Mark> = Vec::new();
    let crops = crop_marks(&plan.trim, plan.mark_len, plan.mark_gap);
    let mut i = 0usize;
    while i < crops.len() {
        if let Some(seg) = crops.get(i) {
            out.push(Mark {
                kind: MarkKind::Crop,
                seg: *seg,
                tiers: TIER_BOTH,
            });
        }
        i += 1;
    }
    let trims = trim_marks(&plan.trim, plan.mark_len);
    let mut j = 0usize;
    while j < trims.len() {
        if let Some(seg) = trims.get(j) {
            out.push(Mark {
                kind: MarkKind::Trim,
                seg: *seg,
                tiers: TIER_BOTH,
            });
        }
        j += 1;
    }
    let mut z = 0usize;
    while z < plan.zones.len() {
        if let Some(zone) = plan.zones.get(z) {
            let sides = overlap_marks(zone);
            let mut k = 0usize;
            while k < sides.len() {
                if let Some(seg) = sides.get(k) {
                    out.push(Mark {
                        kind: MarkKind::Overlap,
                        seg: *seg,
                        tiers: TIER_BOTH,
                    });
                }
                k += 1;
            }
        }
        z += 1;
    }
    let regs = registration_marks(&plan.trim, plan.mark_len);
    let mut r = 0usize;
    while r < regs.len() {
        if let Some(seg) = regs.get(r) {
            out.push(Mark {
                kind: MarkKind::Registration,
                seg: *seg,
                tiers: TIER_PRESS,
            });
        }
        r += 1;
    }
    out
}

/// 生成标记计划：裁切框 + 拼版重叠区 + 四类标记，最后封印摘要。
pub fn plan(spec: &PosterSpec) -> Result<MarkPlan, CropError> {
    validate(spec)?;
    let poster = Rect::new(0.0, 0.0, spec.poster_w, spec.poster_h);
    let trim = poster.inset(spec.bleed);
    let zones = zones_for(poster, spec.sheet_w, spec.sheet_h, spec.overlap)?;

    let mut marks: Vec<Mark> = Vec::new();
    let crops = crop_marks(&trim, spec.mark_len, spec.mark_gap);
    let mut i = 0usize;
    while i < crops.len() {
        if let Some(seg) = crops.get(i) {
            marks.push(Mark {
                kind: MarkKind::Crop,
                seg: *seg,
                tiers: TIER_BOTH,
            });
        }
        i += 1;
    }
    let trims = trim_marks(&trim, spec.mark_len);
    let mut j = 0usize;
    while j < trims.len() {
        if let Some(seg) = trims.get(j) {
            marks.push(Mark {
                kind: MarkKind::Trim,
                seg: *seg,
                tiers: TIER_BOTH,
            });
        }
        j += 1;
    }
    let mut z = 0usize;
    while z < zones.len() {
        if let Some(zone) = zones.get(z) {
            let sides = overlap_marks(zone);
            let mut k = 0usize;
            while k < sides.len() {
                if let Some(seg) = sides.get(k) {
                    marks.push(Mark {
                        kind: MarkKind::Overlap,
                        seg: *seg,
                        tiers: TIER_BOTH,
                    });
                }
                k += 1;
            }
        }
        z += 1;
    }
    let regs = registration_marks(&trim, spec.mark_len);
    let mut r = 0usize;
    while r < regs.len() {
        if let Some(seg) = regs.get(r) {
            marks.push(Mark {
                kind: MarkKind::Registration,
                seg: *seg,
                tiers: TIER_PRESS,
            });
        }
        r += 1;
    }

    let mut p = MarkPlan {
        poster,
        sheet_w: spec.sheet_w,
        sheet_h: spec.sheet_h,
        overlap: spec.overlap,
        bleed: spec.bleed,
        mark_len: spec.mark_len,
        mark_gap: spec.mark_gap,
        trim,
        marks,
        zones,
        digest: 0,
    };
    p.digest = plan_digest(&p);
    Ok(p)
}

/// 重新封印：导入/复原后按当前内容重算摘要（结构核对仍由 `verify_plan` 负责）。
pub fn reseal(plan: &mut MarkPlan) {
    plan.digest = plan_digest(plan);
}

/// 按档位渲染：掩码只决定可见性，返回的线段顺序与计划一致。
pub fn render(plan: &MarkPlan, tier: u8) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut i = 0usize;
    while i < plan.marks.len() {
        if let Some(m) = plan.marks.get(i) {
            if m.tiers & tier != 0 {
                out.push(m.seg);
            }
        }
        i += 1;
    }
    out
}

/// 线段表摘要：用于断言两档渲染在共有标记上逐位一致。
pub fn render_digest(segs: &[Segment]) -> u64 {
    let mut h: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut i = 0usize;
    while i < segs.len() {
        if let Some(s) = segs.get(i) {
            h = fnv_seg(h, s);
        }
        i += 1;
    }
    fnv_u64(h, segs.len() as u64)
}

/// 审计标记表：条数、按带顺序的种类、档位归属；不依赖摘要。
pub fn audit_marks(plan: &MarkPlan) -> Result<u16, CropError> {
    let want = 8usize + 4usize + 2usize * plan.zones.len() + 2usize;
    if plan.marks.len() != want {
        return Err(CropError::AuditMismatch);
    }
    let overlap_end = 12usize + 2usize * plan.zones.len();
    let mut i = 0usize;
    while i < plan.marks.len() {
        let m = match plan.marks.get(i) {
            Some(v) => v,
            None => return Err(CropError::AuditMismatch),
        };
        let kind = if i < 8 {
            MarkKind::Crop
        } else if i < 12 {
            MarkKind::Trim
        } else if i < overlap_end {
            MarkKind::Overlap
        } else {
            MarkKind::Registration
        };
        if m.kind != kind {
            return Err(CropError::AuditMismatch);
        }
        let tiers = if m.kind == MarkKind::Registration {
            TIER_PRESS
        } else {
            TIER_BOTH
        };
        if m.tiers != tiers {
            return Err(CropError::AuditMismatch);
        }
        i += 1;
    }
    Ok(plan.marks.len() as u16)
}

fn same_f32(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits()
}

fn same_rect(a: &Rect, b: &Rect) -> bool {
    same_f32(a.x, b.x) && same_f32(a.y, b.y) && same_f32(a.w, b.w) && same_f32(a.h, b.h)
}

fn seg_same(a: &Segment, b: &Segment) -> bool {
    same_f32(a.a.x, b.a.x)
        && same_f32(a.a.y, b.a.y)
        && same_f32(a.b.x, b.b.x)
        && same_f32(a.b.y, b.b.y)
}

fn point_strict_inside(p: &Point, r: &Rect) -> bool {
    p.x > r.x && p.x < r.right() && p.y > r.y && p.y < r.bottom()
}

/// 计划自检：摘要、标记账、裁切框、闭式重建、裁切线界外、重叠区界内。
pub fn verify_plan(plan: &MarkPlan) -> Result<(), CropError> {
    if plan_digest(plan) != plan.digest {
        return Err(CropError::DigestDrift);
    }
    audit_marks(plan)?;

    let want_trim = plan.poster.inset(plan.bleed);
    if !same_rect(&want_trim, &plan.trim) {
        return Err(CropError::BadMark);
    }
    let want = expected_marks(plan);
    if want.len() != plan.marks.len() {
        return Err(CropError::AuditMismatch);
    }
    let mut i = 0usize;
    while i < want.len() {
        let got = match plan.marks.get(i) {
            Some(v) => v,
            None => return Err(CropError::AuditMismatch),
        };
        let exp = match want.get(i) {
            Some(v) => v,
            None => return Err(CropError::AuditMismatch),
        };
        if got.kind != exp.kind || got.tiers != exp.tiers {
            return Err(CropError::AuditMismatch);
        }
        if !seg_same(&got.seg, &exp.seg) {
            return Err(CropError::BadMark);
        }
        i += 1;
    }

    // 裁切线必须落在裁切框外（含边界），不得侵入成品区。
    let mut k = 0usize;
    while k < 8 {
        if let Some(m) = plan.marks.get(k) {
            if point_strict_inside(&m.seg.a, &plan.trim) || point_strict_inside(&m.seg.b, &plan.trim)
            {
                return Err(CropError::BadMark);
            }
        }
        k += 1;
    }

    // 重叠区必须为正且落在画布内。
    let mut z = 0usize;
    while z < plan.zones.len() {
        if let Some(r) = plan.zones.get(z) {
            if !(r.w > 0.0) || !(r.h > 0.0) {
                return Err(CropError::BadMark);
            }
            if r.x < plan.poster.x - 1.0e-3
                || r.y < plan.poster.y - 1.0e-3
                || r.right() > plan.poster.right() + 1.0e-3
                || r.bottom() > plan.poster.bottom() + 1.0e-3
            {
                return Err(CropError::BadMark);
            }
        }
        z += 1;
    }
    Ok(())
}

fn near(a: f32, b: f32) -> bool {
    fabsf(a - b) <= 1.0e-2
}

fn dist(a: &Point, b: &Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    sqrtf(dx * dx + dy * dy)
}

fn expect_code(spec: &PosterSpec, want: CropError, hits: &mut usize, misses: &mut usize) {
    match plan(spec) {
        Ok(_) => *misses += 1,
        Err(e) => {
            if e == want {
                *hits += 1;
            } else {
                *misses += 1;
            }
        }
    }
}

/// 注入核：八类坏输入（画布/印张/搭边×2/出血/标记×2/超量）逐类断言诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut hits = 0usize;
    let mut misses = 0usize;
    let base = PosterSpec::new(2920.0, 1960.0, 1000.0, 1000.0);

    let mut a = base;
    a.poster_w = 0.0;
    expect_code(&a, CropError::BadPoster, &mut hits, &mut misses);

    let mut b = base;
    b.sheet_w = 0.0;
    expect_code(&b, CropError::BadSheet, &mut hits, &mut misses);

    let mut c = base;
    c.overlap = 0.0;
    expect_code(&c, CropError::BadOverlap, &mut hits, &mut misses);

    let mut d = base;
    d.overlap = d.sheet_w;
    expect_code(&d, CropError::BadOverlap, &mut hits, &mut misses);

    let mut e = base;
    e.bleed = -1.0;
    expect_code(&e, CropError::BadBleed, &mut hits, &mut misses);

    let mut f = base;
    f.mark_len = 0.0;
    expect_code(&f, CropError::BadMark, &mut hits, &mut misses);

    let mut g = base;
    g.mark_gap = -1.0;
    expect_code(&g, CropError::BadMark, &mut hits, &mut misses);

    let mut k = base;
    k.poster_w = 1_000_000.0;
    k.sheet_w = 100.0;
    k.sheet_h = 100.0;
    k.overlap = 10.0;
    k.bleed = 0.0;
    k.mark_len = 1.0;
    k.mark_gap = 0.0;
    expect_code(&k, CropError::TooManyTiles, &mut hits, &mut misses);

    (hits, misses)
}

/// 运行本卡全部自检（17 项），返回绿色/红色明细。
pub fn run_unx50036_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50036");
    let spec = PosterSpec::new(2920.0, 1960.0, 1000.0, 1000.0);

    let grid = match tile_grid(&spec) {
        Ok(g) => g,
        Err(_) => (0, 0),
    };
    s.add(
        "50036-J1-拼版格数",
        grid == (3, 2),
        "2920×1960 画布按 1000 方张、40 搭边应切 3 列×2 行",
    );

    let tiles = match layout_tiles(&spec) {
        Ok(t) => t,
        Err(_) => Vec::new(),
    };
    let zones = match overlap_zones(&spec) {
        Ok(z) => z,
        Err(_) => Vec::new(),
    };
    let base_plan = match plan(&spec) {
        Ok(p) => p,
        Err(_) => {
            s.add("50036-J1-计划可解", false, "基准海报应一次解出标记计划");
            return s;
        }
    };

    // 1) 步距与首尾：每块原点 = 行列号×960，尺寸不裁剪。
    let mut step_ok = tiles.len() == 6;
    let mut i = 0usize;
    while i < tiles.len() {
        if let Some(t) = tiles.get(i) {
            let px = t.col as f32 * 960.0;
            let py = t.row as f32 * 960.0;
            if !near(t.rect.x, px)
                || !near(t.rect.y, py)
                || !near(t.rect.w, 1000.0)
                || !near(t.rect.h, 1000.0)
            {
                step_ok = false;
            }
        }
        i += 1;
    }
    s.add(
        "50036-J1-拼版步距",
        step_ok,
        "每块原点 = 行列号×960（1000 张 − 40 搭边），尺寸 1000 方张",
    );

    let mut ends_ok = false;
    if let (Some(first), Some(last)) = (tiles.first(), tiles.get(5)) {
        ends_ok = near(first.rect.x, 0.0)
            && near(first.rect.y, 0.0)
            && near(last.rect.right(), 2920.0)
            && near(last.rect.bottom(), 1960.0);
    }
    s.add(
        "50036-J1-拼版首尾",
        ends_ok,
        "首块贴画布原点，末块右/下边正好收在 2920×1960",
    );

    // 2) 重叠区账：横向 4 条、纵向 3 条，面积 40×1000。
    let mut wide = 0usize;
    let mut tall = 0usize;
    let mut area_ok = true;
    let mut z = 0usize;
    while z < zones.len() {
        if let Some(r) = zones.get(z) {
            if near(r.w, 40.0) && near(r.h, 1000.0) {
                wide += 1;
            } else if near(r.h, 40.0) && near(r.w, 1000.0) {
                tall += 1;
            }
            if !near(r.area(), 40000.0) {
                area_ok = false;
            }
        }
        z += 1;
    }
    let zone_det = format!(
        "搭边 {} 条：横向 {} 条、纵向 {} 条，面积均为 40×1000",
        zones.len(),
        wide,
        tall
    );
    s.add_dyn(
        "50036-J1-重叠区账",
        zones.len() == 7 && wide == 4 && tall == 3 && area_ok,
        &zone_det,
    );

    let mut inside_ok = zones.len() == 7;
    let mut z = 0usize;
    while z < zones.len() {
        if let Some(r) = zones.get(z) {
            if r.x < -1.0e-3
                || r.y < -1.0e-3
                || r.right() > 2920.0 + 1.0e-3
                || r.bottom() > 1960.0 + 1.0e-3
            {
                inside_ok = false;
            }
        }
        z += 1;
    }
    s.add(
        "50036-J1-重叠区界内",
        inside_ok,
        "七条搭边全部落在画布内，不越出血边",
    );

    // 3) 裁切线：8 条、等长、轴向正确、距角 gap、且在裁切框外。
    let mut crops = 0usize;
    let mut crop_len_ok = true;
    let mut i = 0usize;
    while i < base_plan.marks.len() {
        if let Some(m) = base_plan.marks.get(i) {
            if m.kind == MarkKind::Crop {
                crops += 1;
                if !near(m.seg.len(), 12.0) {
                    crop_len_ok = false;
                }
            }
        }
        i += 1;
    }
    s.add(
        "50036-J1-裁切线数",
        crops == 8 && crop_len_ok,
        "裁切框四角各 2 条、共 8 条，长度均为 mark_len=12",
    );

    let mut h_cnt = 0usize;
    let mut v_cnt = 0usize;
    let mut axis_ok = true;
    let mut i = 0usize;
    while i < 12 {
        if let Some(m) = base_plan.marks.get(i) {
            match m.seg.axis() {
                1 => h_cnt += 1,
                2 => v_cnt += 1,
                _ => axis_ok = false,
            }
        }
        i += 1;
    }
    s.add(
        "50036-J1-裁切线轴向",
        axis_ok && h_cnt == 6 && v_cnt == 6,
        "前 12 条标记（8 裁切 + 4 参考）全为水平/竖直，各 6 条",
    );

    let corners = [
        Point::new(3.0, 3.0),
        Point::new(2917.0, 3.0),
        Point::new(3.0, 1957.0),
        Point::new(2917.0, 1957.0),
    ];
    let mut gap_ok = true;
    let mut c = 0usize;
    while c < 4 {
        let corner = match corners.get(c) {
            Some(p) => *p,
            None => Point::new(0.0, 0.0),
        };
        let mut k = 0usize;
        while k < 2 {
            let idx = c * 2 + k;
            if let Some(m) = base_plan.marks.get(idx) {
                let d0 = dist(&m.seg.a, &corner);
                let d1 = dist(&m.seg.b, &corner);
                let near_d = if d0 < d1 { d0 } else { d1 };
                let far_d = if d0 < d1 { d1 } else { d0 };
                if !near(near_d, 4.0) || !near(far_d, 16.0) {
                    gap_ok = false;
                }
            }
            k += 1;
        }
        c += 1;
    }
    s.add(
        "50036-J1-裁切线距角",
        gap_ok,
        "每角两条标记近端距角 gap=4、远端 4+12=16",
    );

    let mut outside_ok = true;
    let mut i = 0usize;
    while i < 8 {
        if let Some(m) = base_plan.marks.get(i) {
            if point_strict_inside(&m.seg.a, &base_plan.trim)
                || point_strict_inside(&m.seg.b, &base_plan.trim)
            {
                outside_ok = false;
            }
        }
        i += 1;
    }
    let trim_ok = near(base_plan.trim.x, 3.0)
        && near(base_plan.trim.y, 3.0)
        && near(base_plan.trim.right(), 2917.0)
        && near(base_plan.trim.bottom(), 1957.0);
    s.add(
        "50036-J1-裁切内侧",
        outside_ok && trim_ok,
        "裁切线不侵入裁切框；裁切框四边内缩出血 3",
    );

    let bleed_ok = near(base_plan.bleed, 3.0)
        && near(base_plan.mark_gap, 4.0)
        && base_plan.mark_gap >= base_plan.bleed;
    s.add(
        "50036-J1-裁切余量",
        bleed_ok,
        "mark_gap=4 ≥ bleed=3，标记带远离成品边",
    );

    // 4) 两档渲染：共有标记逐位一致，印刷档仅多出套准十字。
    let screen = render(&base_plan, TIER_SCREEN);
    let press = render(&base_plan, TIER_PRESS);
    let mut two_ok = screen.len() == 26 && press.len() == 28;
    if two_ok {
        let mut i = 0usize;
        while i < screen.len() {
            match (screen.get(i), press.get(i)) {
                (Some(a), Some(b)) => {
                    if !seg_same(a, b) {
                        two_ok = false;
                    }
                }
                _ => two_ok = false,
            }
            i += 1;
        }
        let mut head: Vec<Segment> = Vec::new();
        let mut j = 0usize;
        while j < 26 {
            match press.get(j) {
                Some(seg) => head.push(*seg),
                None => two_ok = false,
            }
            j += 1;
        }
        if render_digest(&screen) != render_digest(&head) {
            two_ok = false;
        }
    }
    s.add(
        "50036-J1-两档一致",
        two_ok,
        "屏显 26 条与印刷前 26 条逐位相同，线段摘要一致",
    );

    let mut cover_ok = screen.len() == 26 && press.len() == 28;
    let mut i = 0usize;
    while i < base_plan.marks.len() {
        if let Some(m) = base_plan.marks.get(i) {
            if m.kind == MarkKind::Registration {
                if m.tiers != TIER_PRESS {
                    cover_ok = false;
                }
            } else if m.tiers != TIER_BOTH {
                cover_ok = false;
            }
        }
        i += 1;
    }
    s.add(
        "50036-J1-两档覆盖",
        cover_ok,
        "印刷档 = 屏显档 + 2 条套准线；裁切/参考/搭边线两档共有",
    );

    let audit = audit_marks(&base_plan);
    let count_formula = 8usize + 4usize + 2usize * base_plan.zones.len() + 2usize;
    let ledger_ok = match audit {
        Ok(n) => n as usize == count_formula && base_plan.marks.len() == count_formula,
        Err(_) => false,
    };
    let verify_ok = verify_plan(&base_plan).is_ok();
    let ledger_det = format!(
        "标记 {} 条 = 8 裁切 + 4 参考 + 2×{} 搭边 + 2 套准，审计{}、核对{}",
        base_plan.marks.len(),
        base_plan.zones.len(),
        if ledger_ok { "通过" } else { "未过" },
        if verify_ok { "通过" } else { "未过" }
    );
    s.add_dyn("50036-J1-标记账", ledger_ok && verify_ok, &ledger_det);

    // 5) 确定性：两次成计划摘要、渲染摘要一致。
    let mut det_ok = false;
    match plan(&spec) {
        Ok(p2) => {
            det_ok = p2.digest == base_plan.digest
                && verify_plan(&p2).is_ok()
                && render_digest(&render(&p2, TIER_SCREEN)) == render_digest(&screen);
        }
        Err(_) => {}
    }
    s.add(
        "50036-J1-渲染确定",
        det_ok,
        "同规格两次成计划摘要一致，屏显渲染摘要一致",
    );

    // 6) 篡改必抓：改端点/换类/删条/未重封四路。
    let mut tamper_ok = true;
    let mut t1 = match plan(&spec) {
        Ok(p) => p,
        Err(_) => return s,
    };
    if let Some(m) = t1.marks.get_mut(0) {
        m.seg.b.x += 5.0;
    }
    reseal(&mut t1);
    if verify_plan(&t1).err() != Some(CropError::BadMark) {
        tamper_ok = false;
    }

    let mut t2 = match plan(&spec) {
        Ok(p) => p,
        Err(_) => return s,
    };
    if let Some(m) = t2.marks.get_mut(0) {
        m.kind = MarkKind::Trim;
    }
    if audit_marks(&t2).err() != Some(CropError::AuditMismatch) {
        tamper_ok = false;
    }

    let mut t3 = match plan(&spec) {
        Ok(p) => p,
        Err(_) => return s,
    };
    let _ = t3.marks.pop();
    reseal(&mut t3);
    if verify_plan(&t3).err() != Some(CropError::AuditMismatch) {
        tamper_ok = false;
    }

    let mut t4 = match plan(&spec) {
        Ok(p) => p,
        Err(_) => return s,
    };
    if let Some(m) = t4.marks.get_mut(3) {
        m.seg.a.y -= 2.0;
    }
    if verify_plan(&t4).err() != Some(CropError::DigestDrift) {
        tamper_ok = false;
    }
    s.add(
        "50036-J1-篡改必抓",
        tamper_ok,
        "改端点/删条重封落 BadMark/AuditMismatch，换类落 AuditMismatch，未重封落 DigestDrift",
    );

    let (hits, misses) = injection_core();
    s.add(
        "50036-J1-非法必拒",
        hits == 8 && misses == 0,
        "画布/印张/搭边×2/出血/标记×2/超量八类坏输入各命中专属码",
    );

    let codes = [
        CropError::BadPoster.code(),
        CropError::BadSheet.code(),
        CropError::BadOverlap.code(),
        CropError::BadBleed.code(),
        CropError::BadMark.code(),
        CropError::TooManyTiles.code(),
        CropError::DigestDrift.code(),
        CropError::AuditMismatch.code(),
        CropError::Range.code(),
    ];
    let mut set_ok = true;
    let mut i = 0usize;
    while i < codes.len() {
        let c = match codes.get(i) {
            Some(v) => *v,
            None => 0,
        };
        if c < 1 || c > 9 || SEG_BASE + c as u32 > SEG_BASE + 9 {
            set_ok = false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if let Some(d) = codes.get(j) {
                if *d == c {
                    set_ok = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "50036-J1-码集闭包",
        set_ok,
        "九个诊断码互异且全部落在 0x430a1..=0x430a9",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50036_checks_green() {
        let set = run_unx50036_checks();
        assert!(
            set.all_passed(),
            "UNX-F50036 red: {}",
            set.red_items_text()
        );
    }

    #[test]
    fn unx50036_two_tier_identity() {
        let spec = PosterSpec::new(2920.0, 1960.0, 1000.0, 1000.0);
        match plan(&spec) {
            Ok(p) => {
                let a = render(&p, TIER_SCREEN);
                let b = render(&p, TIER_PRESS);
                assert_eq!(a.len(), 26);
                assert_eq!(b.len(), 28);
                let mut i = 0usize;
                while i < a.len() {
                    match (a.get(i), b.get(i)) {
                        (Some(x), Some(y)) => assert!(seg_same(x, y), "两档第 {} 条不一致", i),
                        _ => panic!("渲染条数不足"),
                    }
                    i += 1;
                }
                let mut head: Vec<Segment> = Vec::new();
                let mut j = 0usize;
                while j < 26 {
                    match b.get(j) {
                        Some(seg) => head.push(*seg),
                        None => panic!("印刷档条数不足"),
                    }
                    j += 1;
                }
                assert_eq!(render_digest(&a), render_digest(&head));
            }
            Err(e) => panic!("plan rejected: {}", e.code()),
        }
    }

    #[test]
    fn unx50036_injection_closed() {
        assert_eq!(injection_core(), (8, 0));
        let spec = PosterSpec::new(2920.0, 1960.0, 1000.0, 1000.0);
        match plan(&spec) {
            Ok(mut p) => {
                let _ = p.marks.pop();
                reseal(&mut p);
                assert_eq!(verify_plan(&p).err(), Some(CropError::AuditMismatch));
            }
            Err(e) => panic!("plan rejected: {}", e.code()),
        }
    }
}
