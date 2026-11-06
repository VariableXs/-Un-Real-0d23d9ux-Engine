//! UNX-F50024 海报拼接（2×2/3×3 放大切割 + 重叠边）（0x42f20 段）
//!
//! 锚点：UNX-F50024-J1 切割 ×2 档拼回完整断言，重叠边一致；判据锚定
//! §AI-63 §2 专题二。
//!
//! 数据段：`SEG_BASE = 0x42f20`。诊断码是闭集 `0x42f21..=0x42f2a`
//! （`PosterError` 十档，`code() = SEG_BASE + n`）。
//!
//! 结构清单
//! * 数据结构：`PosterGrid`（行列档位）、`PosterSpec`（目标尺寸/源尺寸/
//!   重叠带）、`Rect`、`Tile`（单块：源矩形 + 核心 + 重叠 + 放大后落点）、
//!   `PosterPlan`（整版切割计划）、`OverlapAudit`（重叠带核对账）；
//! * 求解器：`tile_source`（源矩形，含右下重叠）、`plan`（整版）、
//!   `render_direct`（整张直接渲染）、`compose_core`（按裁切线拼回）、
//!   `audit_overlaps`（重叠带逐格比对）；
//! * 错误路径：`PosterError` 十档；任何形状/尺寸错误在分配画布之前返回；
//! * 注入核：`injection_core` 六类坏样本（零行、5×2 超档、目标过小、
//!   重叠过大、源尺寸为零、各向异性放大）。
//!
//! 「拼回完整」到底测什么
//! ----------------------
//! 每块海报纸印的是「核心区 + 一条重叠带」。裁切时重叠带被切掉或压到相邻
//! 纸下方，所以拼回只能拿**核心区**：`compose_core` 按每块自己的核心宽高
//! 还原到海报坐标，逐格与 `render_direct` 比对。若任一块的原点或核心尺寸
//! 算错，拼接图会出现错位或空洞，逐格比对立刻报差异；覆盖计数则保证
//! 「既无空洞也无双重覆盖」。重叠带本身由 `audit_overlaps` 单独核对：
//! 带内每一格必须与相邻块核心在同一海报坐标上取到同一个内容签名。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::{fabsf, floorf};

/// 诊断段基址（0x42f20 起 0x20 归本模块）。
pub const SEG_BASE: u32 = 0x42f20;

/// 单边最大块数。4×4=16 块已是桌面级海报的物理上限（A4 放大 4 倍）。
pub const MAX_TILES_PER_SIDE: u8 = 4;

/// 目标画面边长合法区间（mm）。
pub const MIN_TARGET_MM: f32 = 50.0;
/// 目标画面边长上界（mm）。
pub const MAX_TARGET_MM: f32 = 2000.0;

/// 单块核心区最小边长（mm）。小于 20mm 的块贴不住、也对不齐。
pub const MIN_CORE_MM: f32 = 20.0;

pub const E_EMPTY: u16 = 1;
pub const E_GRID: u16 = 2;
pub const E_TARGET: u16 = 3;
pub const E_OVERLAP: u16 = 4;
pub const E_SOURCE: u16 = 5;
pub const E_ZOOM: u16 = 6;
pub const E_TILE: u16 = 7;
pub const E_AUDIT: u16 = 8;
pub const E_PLAN: u16 = 9;
pub const E_CUT: u16 = 10;

/// 海报拼接域错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PosterError {
    Empty,
    Grid,
    Target,
    Overlap,
    Source,
    Zoom,
    Tile,
    Audit,
    Plan,
    Cut,
}

impl PosterError {
    /// 诊断码：`SEG_BASE + 档内序号`。
    pub fn code(self) -> u32 {
        let n = match self {
            PosterError::Empty => E_EMPTY,
            PosterError::Grid => E_GRID,
            PosterError::Target => E_TARGET,
            PosterError::Overlap => E_OVERLAP,
            PosterError::Source => E_SOURCE,
            PosterError::Zoom => E_ZOOM,
            PosterError::Tile => E_TILE,
            PosterError::Audit => E_AUDIT,
            PosterError::Plan => E_PLAN,
            PosterError::Cut => E_CUT,
        };
        SEG_BASE + n as u32
    }

    /// 面向操作员的稳定说明文本。
    pub fn message(self) -> &'static str {
        match self {
            PosterError::Empty => "no tiles requested",
            PosterError::Grid => "grid must be 1..=4 per side",
            PosterError::Target => "target size outside 50..2000mm or too small per tile",
            PosterError::Overlap => "overlap must be positive but below half a tile",
            PosterError::Source => "source document size must be positive",
            PosterError::Zoom => "anisotropic zoom (width/height scale disagree)",
            PosterError::Tile => "tile geometry degenerate or outside target",
            PosterError::Audit => "overlap strip mismatch",
            PosterError::Plan => "poster plan failed self-verification",
            PosterError::Cut => "cut line outside tile",
        }
    }
}

/// 毫米制矩形。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    /// 右边缘。
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// 下边缘。
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// 面积。
    pub fn area(&self) -> f32 {
        self.w * self.h
    }

    /// 是否正尺寸且有限。
    pub fn is_positive(&self) -> bool {
        self.w > 0.0 && self.h > 0.0 && self.w.is_finite() && self.h.is_finite()
    }

    /// 与另一矩形的交叠矩形；无正面积交叠返回 `None`。
    pub fn intersect(&self, o: &Rect, eps: f32) -> Option<Rect> {
        let x0 = if self.x > o.x { self.x } else { o.x };
        let y0 = if self.y > o.y { self.y } else { o.y };
        let x1 = if self.right() < o.right() {
            self.right()
        } else {
            o.right()
        };
        let y1 = if self.bottom() < o.bottom() {
            self.bottom()
        } else {
            o.bottom()
        };
        if x1 - x0 <= eps || y1 - y0 <= eps {
            return None;
        }
        Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    /// 是否包含给定点（左闭右开，避免相邻块重复计入边界点）。
    pub fn contains_point(&self, x: f32, y: f32, eps: f32) -> bool {
        x >= self.x - eps && x < self.right() - eps && y >= self.y - eps && y < self.bottom() - eps
    }

    /// 是否被外框包含。
    pub fn inside(&self, outer: &Rect, eps: f32) -> bool {
        self.x >= outer.x - eps
            && self.y >= outer.y - eps
            && self.right() <= outer.right() + eps
            && self.bottom() <= outer.bottom() + eps
    }
}

/// 行列档位。1..=4，硬上限由 `MAX_TILES_PER_SIDE` 给出。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PosterGrid {
    pub cols: u8,
    pub rows: u8,
}

impl PosterGrid {
    /// 新建档位（构造不做校验，`validate()` 是唯一判定口）。
    pub const fn new(cols: u8, rows: u8) -> PosterGrid {
        PosterGrid { cols, rows }
    }

    /// 2×2 常用档。
    pub const fn two_by_two() -> PosterGrid {
        PosterGrid::new(2, 2)
    }

    /// 3×3 常用档。
    pub const fn three_by_three() -> PosterGrid {
        PosterGrid::new(3, 3)
    }

    /// 块数。
    pub fn cells(&self) -> u16 {
        self.cols as u16 * self.rows as u16
    }

    /// 档位合法性：非零、单边 ≤4、总块数 ≤16。
    pub fn validate(&self) -> Result<(), PosterError> {
        if self.cols == 0 || self.rows == 0 {
            return Err(PosterError::Grid);
        }
        if self.cols > MAX_TILES_PER_SIDE || self.rows > MAX_TILES_PER_SIDE {
            return Err(PosterError::Grid);
        }
        if self.cells() == 0 || self.cells() > 16 {
            return Err(PosterError::Grid);
        }
        Ok(())
    }
}

/// 整版规格。`source_*` 是原始文档尺寸，放大倍数由目标/源之比给出。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PosterSpec {
    pub target_w: f32,
    pub target_h: f32,
    pub source_w: f32,
    pub source_h: f32,
    pub grid: PosterGrid,
    pub overlap_mm: f32,
}

impl PosterSpec {
    /// 新建规格（1×1、零重叠），随后链式设置。
    pub const fn new(target_w: f32, target_h: f32, source_w: f32, source_h: f32) -> PosterSpec {
        PosterSpec {
            target_w,
            target_h,
            source_w,
            source_h,
            grid: PosterGrid::new(1, 1),
            overlap_mm: 0.0,
        }
    }

    /// 设置切割档位。
    pub fn with_grid(mut self, grid: PosterGrid) -> PosterSpec {
        self.grid = grid;
        self
    }

    /// 设置重叠带宽度（mm）。
    pub fn with_overlap(mut self, overlap_mm: f32) -> PosterSpec {
        self.overlap_mm = overlap_mm;
        self
    }

    /// 单块核心宽（mm）。
    pub fn core_w(&self) -> f32 {
        if self.grid.cols == 0 {
            return 0.0;
        }
        self.target_w / self.grid.cols as f32
    }

    /// 单块核心高（mm）。
    pub fn core_h(&self) -> f32 {
        if self.grid.rows == 0 {
            return 0.0;
        }
        self.target_h / self.grid.rows as f32
    }

    /// 放大倍数（按宽）。`validate()` 已保证与高一致。
    pub fn zoom(&self) -> f32 {
        if self.source_w <= 0.0 {
            return 0.0;
        }
        self.target_w / self.source_w
    }

    /// 规格合法性。顺序即诊断码优先级：档位 → 源 → 目标 → 重叠。
    pub fn validate(&self) -> Result<(), PosterError> {
        self.grid.validate()?;
        if !self.source_w.is_finite()
            || !self.source_h.is_finite()
            || self.source_w <= 0.0
            || self.source_h <= 0.0
        {
            return Err(PosterError::Source);
        }
        if !self.target_w.is_finite() || !self.target_h.is_finite() {
            return Err(PosterError::Target);
        }
        if self.target_w < MIN_TARGET_MM
            || self.target_h < MIN_TARGET_MM
            || self.target_w > MAX_TARGET_MM
            || self.target_h > MAX_TARGET_MM
        {
            return Err(PosterError::Target);
        }
        if self.core_w() < MIN_CORE_MM || self.core_h() < MIN_CORE_MM {
            return Err(PosterError::Target);
        }
        // 各向异性放大：宽高倍数必须一致，否则拼回后画面被拉歪。
        let zx = self.target_w / self.source_w;
        let zy = self.target_h / self.source_h;
        if fabsf(zx - zy) > 1e-3 * (if zx > 1.0 { zx } else { 1.0 }) {
            return Err(PosterError::Zoom);
        }
        if !self.overlap_mm.is_finite() || self.overlap_mm < 0.0 {
            return Err(PosterError::Overlap);
        }
        // 重叠必须小于半块核心，否则裁切后核心区所剩无几。
        let half = if self.core_w() < self.core_h() {
            self.core_w() * 0.5
        } else {
            self.core_h() * 0.5
        };
        if self.overlap_mm >= half {
            return Err(PosterError::Overlap);
        }
        if self.overlap_mm > 0.0 && self.grid.cells() == 1 {
            return Err(PosterError::Overlap);
        }
        Ok(())
    }
}

/// 单块切割计划。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub col: u8,
    pub row: u8,
    /// 该块在海报坐标系里覆盖的源矩形（核心 + 向右/向下的重叠带）。
    pub src: Rect,
    /// 裁切线以内的核心尺寸（mm），拼回时只取这一块。
    pub core_w: f32,
    pub core_h: f32,
    /// 右/下重叠带宽度；最后一列/行恒为 0。
    pub overlap_right: f32,
    pub overlap_bottom: f32,
    /// 该块在**源文档**坐标系里的对应矩形（src / zoom），用于从原稿取图。
    pub dst: Rect,
}

impl Tile {
    /// 割线位置（海报坐标）：右割线、下割线。
    pub fn cut_lines(&self) -> (f32, f32) {
        (self.src.x + self.core_w, self.src.y + self.core_h)
    }
}

/// 整版切割计划。
#[derive(Clone, Debug)]
pub struct PosterPlan {
    pub spec: PosterSpec,
    pub tiles: Vec<Tile>,
}

impl PosterPlan {
    /// 块数。
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    /// 按行列取块。
    pub fn tile(&self, col: u8, row: u8) -> Option<&Tile> {
        let mut i = 0usize;
        while i < self.tiles.len() {
            if let Some(t) = self.tiles.get(i) {
                if t.col == col && t.row == row {
                    return Some(t);
                }
            }
            i += 1;
        }
        None
    }

    /// 全量自检：块数、核心尺寸、重叠宽度、落点、放大往返。
    pub fn verify(&self) -> Result<(), PosterError> {
        self.spec.validate()?;
        if self.tiles.len() != self.spec.grid.cells() as usize {
            return Err(PosterError::Plan);
        }
        let target = Rect::new(0.0, 0.0, self.spec.target_w, self.spec.target_h);
        let source = Rect::new(0.0, 0.0, self.spec.source_w, self.spec.source_h);
        let zoom = self.spec.zoom();
        let mut i = 0usize;
        while i < self.tiles.len() {
            let t = match self.tiles.get(i) {
                Some(v) => v,
                None => return Err(PosterError::Plan),
            };
            if t.core_w <= 0.0 || t.core_h <= 0.0 {
                return Err(PosterError::Tile);
            }
            if fabsf(t.core_w - self.spec.core_w()) > 1e-3
                || fabsf(t.core_h - self.spec.core_h()) > 1e-3
            {
                return Err(PosterError::Cut);
            }
            let want_r = if t.col + 1 < self.spec.grid.cols {
                self.spec.overlap_mm
            } else {
                0.0
            };
            let want_b = if t.row + 1 < self.spec.grid.rows {
                self.spec.overlap_mm
            } else {
                0.0
            };
            if fabsf(t.overlap_right - want_r) > 1e-4 || fabsf(t.overlap_bottom - want_b) > 1e-4
            {
                return Err(PosterError::Plan);
            }
            if fabsf(t.src.w - (t.core_w + t.overlap_right)) > 1e-3
                || fabsf(t.src.h - (t.core_h + t.overlap_bottom)) > 1e-3
            {
                return Err(PosterError::Plan);
            }
            if !t.src.is_positive() || !t.src.inside(&target, 1e-3) || !t.dst.inside(&source, 1e-3)
            {
                return Err(PosterError::Tile);
            }
            // 放大往返：源文档矩形 × zoom 必须回到海报矩形。
            if fabsf(t.dst.x * zoom - t.src.x) > 1e-2
                || fabsf(t.dst.y * zoom - t.src.y) > 1e-2
                || fabsf(t.dst.w * zoom - t.src.w) > 1e-2
            {
                return Err(PosterError::Zoom);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 求单块源矩形。内部边向右/向下多吃一条重叠带，最后一行列不吃。
/// 不能越出目标边界：`x1` 先算再加重叠，最后按目标宽钳位。
pub fn tile_source(spec: &PosterSpec, col: u8, row: u8) -> Result<Rect, PosterError> {
    spec.validate()?;
    if col >= spec.grid.cols || row >= spec.grid.rows {
        return Err(PosterError::Tile);
    }
    let cw = spec.core_w();
    let ch = spec.core_h();
    let x0 = col as f32 * cw;
    let y0 = row as f32 * ch;
    let mut x1 = (col as f32 + 1.0) * cw;
    let mut y1 = (row as f32 + 1.0) * ch;
    if col + 1 < spec.grid.cols {
        x1 += spec.overlap_mm;
    }
    if row + 1 < spec.grid.rows {
        y1 += spec.overlap_mm;
    }
    if x1 > spec.target_w {
        x1 = spec.target_w;
    }
    if y1 > spec.target_h {
        y1 = spec.target_h;
    }
    if x1 <= x0 || y1 <= y0 {
        return Err(PosterError::Tile);
    }
    Ok(Rect::new(x0, y0, x1 - x0, y1 - y0))
}

/// 生成整版切割计划。
pub fn plan(spec: PosterSpec) -> Result<PosterPlan, PosterError> {
    spec.validate()?;
    if spec.grid.cells() == 0 {
        return Err(PosterError::Empty);
    }
    let zoom = spec.zoom();
    let mut tiles = Vec::with_capacity(spec.grid.cells() as usize);
    let mut row = 0u8;
    while row < spec.grid.rows {
        let mut col = 0u8;
        while col < spec.grid.cols {
            let src = tile_source(&spec, col, row)?;
            let core_w = spec.core_w();
            let core_h = spec.core_h();
            let overlap_right = src.w - core_w;
            let overlap_bottom = src.h - core_h;
            let dst = Rect::new(
                src.x / zoom,
                src.y / zoom,
                src.w / zoom,
                src.h / zoom,
            );
            tiles.push(Tile {
                col,
                row,
                src,
                core_w,
                core_h,
                overlap_right,
                overlap_bottom,
                dst,
            });
            col += 1;
        }
        row += 1;
    }
    let p = PosterPlan { spec, tiles };
    p.verify()?;
    Ok(p)
}

// ---------------------------------------------------------------------------
// 渲染 / 拼回（模拟器档：以整数毫米格上的内容签名代替真实像素）
// ---------------------------------------------------------------------------

/// 海报坐标 (ix, iy) 的内容签名。选素数模 251 是为了让相邻格的签名
/// 几乎不相等，错位一格必现差异；同时保证 0..=250 落在 u8 内。
pub fn signature(ix: i32, iy: i32) -> u8 {
    let cross = (ix * iy).rem_euclid(13);
    let v = ix * 37 + iy * 17 + cross;
    v.rem_euclid(251) as u8
}

/// 毫米坐标 → 格索引。`+1e-4` 吸收除法残差（1mm 格，残差恒 < 1e-3）。
fn idx_of(mm: f32, step: f32) -> i32 {
    floorf(mm / step + 1e-4) as i32
}

/// 从**源文档**取样：海报坐标 → 源文档坐标（除以放大倍数）→ 源像素签名。
///
/// 所有渲染都从源文档取内容，而不是各自编一份——这样「块拼回来对不对」
/// 才等价于「dst 原点与 zoom 是否一致」，而不是自己跟自己比。
fn sample_source(zoom: f32, src_step: f32, gx_mm: f32, gy_mm: f32) -> u8 {
    let sx = gx_mm / zoom;
    let sy = gy_mm / zoom;
    signature(idx_of(sx, src_step), idx_of(sy, src_step))
}

/// 从某一块的**局部版面**取样：局部毫米 = 海报坐标 - 块原点，再按 dst 落回源文档。
///
/// 与 `sample_source` 走的是两条不同的算术路径（局部坐标 + dst 原点 vs 全局
/// 坐标直接除 zoom）。两条路径只在 `dst = src / zoom` 成立时才会逐格相等。
fn sample_tile(tile: &Tile, zoom: f32, src_step: f32, gx_mm: f32, gy_mm: f32) -> u8 {
    let local_x = gx_mm - tile.src.x;
    let local_y = gy_mm - tile.src.y;
    let sx = tile.dst.x + local_x / zoom;
    let sy = tile.dst.y + local_y / zoom;
    signature(idx_of(sx, src_step), idx_of(sy, src_step))
}

/// 画布：逐格内容 + 覆盖计数。
#[derive(Clone, Debug)]
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub step: f32,
    pub cells: Vec<u8>,
    pub counts: Vec<u8>,
}

impl Canvas {
    /// 取某格内容；越界返回 `None`。
    pub fn at(&self, ix: usize, iy: usize) -> Option<u8> {
        if ix >= self.w || iy >= self.h {
            return None;
        }
        self.cells.get(iy * self.w + ix).copied()
    }

    /// 取某格覆盖计数；越界返回 `None`。
    pub fn count_at(&self, ix: usize, iy: usize) -> Option<u8> {
        if ix >= self.w || iy >= self.h {
            return None;
        }
        self.counts.get(iy * self.w + ix).copied()
    }

    /// 内容为 0 的格数（签名可以为 0，所以这项只作诊断，不作判据）。
    pub fn zero_cells(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.cells.len() {
            if self.cells.get(i) == Some(&0u8) {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

fn canvas_dims(spec: &PosterSpec, step: f32) -> Result<(usize, usize), PosterError> {
    if step <= 0.0 || !step.is_finite() || step > 10.0 {
        return Err(PosterError::Plan);
    }
    let w = (spec.target_w / step).ceil() as usize;
    let h = (spec.target_h / step).ceil() as usize;
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return Err(PosterError::Plan);
    }
    Ok((w, h))
}

/// 整张直接渲染（不切块），作为拼回的对照组。
///
/// 取样点在格中心 `(ix+0.5)*step`，避免边界上 f32 取整分歧。
pub fn render_direct(spec: &PosterSpec, step: f32) -> Result<Canvas, PosterError> {
    spec.validate()?;
    let (w, h) = canvas_dims(spec, step)?;
    let zoom = spec.zoom();
    if zoom <= 0.0 {
        return Err(PosterError::Zoom);
    }
    let src_step = step / zoom;
    let mut cells = Vec::with_capacity(w * h);
    let mut counts = Vec::with_capacity(w * h);
    let mut iy = 0usize;
    while iy < h {
        let mut ix = 0usize;
        while ix < w {
            let gx = (ix as f32 + 0.5) * step;
            let gy = (iy as f32 + 0.5) * step;
            cells.push(sample_source(zoom, src_step, gx, gy));
            counts.push(1u8);
            ix += 1;
        }
        iy += 1;
    }
    Ok(Canvas {
        w,
        h,
        step,
        cells,
        counts,
    })
}

/// 按裁切线拼回：每块只贴**核心区**（重叠带被切掉），贴到海报坐标上。
///
/// 内容来自各块自己的局部版面（`sample_tile`，经 dst + zoom 落回源文档），
/// 覆盖计数是**数出来的**而不是计划自述，因此空洞与双重覆盖都能独立发现。
pub fn compose_core(plan: &PosterPlan, step: f32) -> Result<Canvas, PosterError> {
    plan.verify()?;
    let (w, h) = canvas_dims(&plan.spec, step)?;
    let zoom = plan.spec.zoom();
    if zoom <= 0.0 {
        return Err(PosterError::Zoom);
    }
    let src_step = step / zoom;
    let mut cells = Vec::with_capacity(w * h);
    let mut counts = Vec::with_capacity(w * h);
    let mut i = 0usize;
    while i < w * h {
        cells.push(0u8);
        counts.push(0u8);
        i += 1;
    }
    let mut t = 0usize;
    while t < plan.tiles.len() {
        let tile = match plan.tiles.get(t) {
            Some(v) => v,
            None => return Err(PosterError::Plan),
        };
        let ix0 = idx_of(tile.src.x, step);
        let iy0 = idx_of(tile.src.y, step);
        let ix1 = idx_of(tile.src.x + tile.core_w, step);
        let iy1 = idx_of(tile.src.y + tile.core_h, step);
        if ix0 < 0 || iy0 < 0 || ix1 as usize > w || iy1 as usize > h || ix1 <= ix0 || iy1 <= iy0
        {
            return Err(PosterError::Cut);
        }
        let mut gy = iy0;
        while gy < iy1 {
            let mut gx = ix0;
            while gx < ix1 {
                let px = (gx as f32 + 0.5) * step;
                let py = (gy as f32 + 0.5) * step;
                let v = sample_tile(tile, zoom, src_step, px, py);
                let at = (gy as usize) * w + gx as usize;
                if let Some(c) = cells.get_mut(at) {
                    *c = v;
                }
                if let Some(c) = counts.get_mut(at) {
                    *c = c.saturating_add(1);
                }
                gx += 1;
            }
            gy += 1;
        }
        t += 1;
    }
    Ok(Canvas {
        w,
        h,
        step,
        cells,
        counts,
    })
}

/// 重叠带核对结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlapAudit {
    /// 参与核对的相邻块对数（水平 + 垂直）。
    pub pairs: usize,
    /// 带内抽样格数。
    pub samples: usize,
    /// 两块的局部取样与原稿直接取样不一致的格数（0 = 完全一致）。
    pub mismatches: usize,
    /// 几何不符的边数（带宽不对、带不在左块版面内、带起点不等于右块原点）。
    pub width_bad: usize,
}

/// 重叠带逐格核对。
///
/// 对每条内部边取「右块（下块）的左/上重叠带」为几何基准，逐格做三件事：
/// 1. 带宽必须等于 `overlap_mm`，带必须完整落在左块（上块）版面内，且带起点
///    必须正好是右块的原点——这三条任一破了都说明邻块偏移算错；
/// 2. 左块用**局部坐标 + dst 原点**取样，右块同法取样，两者必须相等；
/// 3. 再与**原稿全局坐标**直接取样对齐，防止两块"一起错"。
pub fn audit_overlaps(plan: &PosterPlan, step: f32) -> Result<OverlapAudit, PosterError> {
    plan.verify()?;
    if step <= 0.0 || !step.is_finite() {
        return Err(PosterError::Plan);
    }
    let zoom = plan.spec.zoom();
    if zoom <= 0.0 {
        return Err(PosterError::Zoom);
    }
    let src_step = step / zoom;
    let mut audit = OverlapAudit {
        pairs: 0,
        samples: 0,
        mismatches: 0,
        width_bad: 0,
    };
    let spec = plan.spec;
    let ov = spec.overlap_mm;
    let mut i = 0usize;
    while i < plan.tiles.len() {
        let a = match plan.tiles.get(i) {
            Some(v) => *v,
            None => return Err(PosterError::Plan),
        };
        if a.col + 1 < spec.grid.cols {
            let b = match plan.tile(a.col + 1, a.row) {
                Some(v) => *v,
                None => return Err(PosterError::Plan),
            };
            audit.pairs += 1;
            // 基准带：右块的左缘 + 一个重叠宽度，高度取右块核心高。
            let strip = Rect::new(b.src.x, b.src.y, ov, b.core_h);
            if !strip.inside(&a.src, 1e-3) || fabsf(strip.right() - a.src.right()) > 1e-3 {
                audit.width_bad += 1;
            }
            let gx0 = idx_of(strip.x, step);
            let gx1 = idx_of(strip.right(), step);
            let gy0 = idx_of(strip.y, step);
            let gy1 = idx_of(strip.bottom(), step);
            let mut gy = gy0;
            while gy < gy1 {
                let mut gx = gx0;
                while gx < gx1 {
                    let px = (gx as f32 + 0.5) * step;
                    let py = (gy as f32 + 0.5) * step;
                    let va = sample_tile(&a, zoom, src_step, px, py);
                    let vb = sample_tile(&b, zoom, src_step, px, py);
                    let vd = sample_source(zoom, src_step, px, py);
                    audit.samples += 1;
                    if va != vb || va != vd {
                        audit.mismatches += 1;
                    }
                    gx += 1;
                }
                gy += 1;
            }
        }
        if a.row + 1 < spec.grid.rows {
            let b = match plan.tile(a.col, a.row + 1) {
                Some(v) => *v,
                None => return Err(PosterError::Plan),
            };
            audit.pairs += 1;
            // 基准带：下块的上缘 + 一个重叠高度，宽度取下块核心宽。
            let strip = Rect::new(b.src.x, b.src.y, b.core_w, ov);
            if !strip.inside(&a.src, 1e-3) || fabsf(strip.bottom() - a.src.bottom()) > 1e-3 {
                audit.width_bad += 1;
            }
            let gx0 = idx_of(strip.x, step);
            let gx1 = idx_of(strip.right(), step);
            let gy0 = idx_of(strip.y, step);
            let gy1 = idx_of(strip.bottom(), step);
            let mut gy = gy0;
            while gy < gy1 {
                let mut gx = gx0;
                while gx < gx1 {
                    let px = (gx as f32 + 0.5) * step;
                    let py = (gy as f32 + 0.5) * step;
                    let va = sample_tile(&a, zoom, src_step, px, py);
                    let vb = sample_tile(&b, zoom, src_step, px, py);
                    let vd = sample_source(zoom, src_step, px, py);
                    audit.samples += 1;
                    if va != vb || va != vd {
                        audit.mismatches += 1;
                    }
                    gx += 1;
                }
                gy += 1;
            }
        }
        i += 1;
    }
    Ok(audit)
}

/// 拼回完整性：返回 `(差异格数, 空洞格数, 重复格数)`。
/// 三项都为 0 才算「拼回完整」。
pub fn stitch_report(plan: &PosterPlan, step: f32) -> Result<(usize, usize, usize), PosterError> {
    let direct = render_direct(&plan.spec, step)?;
    let rebuilt = compose_core(plan, step)?;
    if direct.w != rebuilt.w || direct.h != rebuilt.h {
        return Err(PosterError::Plan);
    }
    let mut diff = 0usize;
    let mut holes = 0usize;
    let mut dup = 0usize;
    let mut i = 0usize;
    while i < direct.cells.len() {
        match (direct.cells.get(i), rebuilt.cells.get(i)) {
            (Some(a), Some(b)) => {
                if a != b {
                    diff += 1;
                }
            }
            _ => diff += 1,
        }
        match rebuilt.counts.get(i) {
            Some(0) => holes += 1,
            Some(c) => {
                if *c > 1 {
                    dup += 1;
                }
            }
            None => holes += 1,
        }
        i += 1;
    }
    Ok((diff, holes, dup))
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

/// 六类坏样本：零行、5×2 超档、目标过小、重叠过大、源尺寸为零、各向异性。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;
    let base = PosterSpec::new(240.0, 180.0, 120.0, 90.0).with_overlap(10.0);

    if plan(base.with_grid(PosterGrid::new(2, 0))).err() == Some(PosterError::Grid) {
        caught += 1;
    }
    if plan(base.with_grid(PosterGrid::new(5, 2))).err() == Some(PosterError::Grid) {
        caught += 1;
    }
    if plan(PosterSpec::new(240.0, 30.0, 120.0, 30.0).with_grid(PosterGrid::new(2, 2)))
        .err()
        == Some(PosterError::Target)
    {
        caught += 1;
    }
    if plan(base.with_grid(PosterGrid::new(2, 2)).with_overlap(70.0)).err()
        == Some(PosterError::Overlap)
    {
        caught += 1;
    }
    if plan(PosterSpec::new(240.0, 180.0, 0.0, 90.0).with_grid(PosterGrid::two_by_two()))
        .err()
        == Some(PosterError::Source)
    {
        caught += 1;
    }
    if plan(PosterSpec::new(240.0, 180.0, 120.0, 60.0).with_grid(PosterGrid::two_by_two()))
        .err()
        == Some(PosterError::Zoom)
    {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50024 判据集（15 条）。
pub fn run_unx50024_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50024");
    let spec2 = PosterSpec::new(240.0, 180.0, 120.0, 90.0)
        .with_grid(PosterGrid::two_by_two())
        .with_overlap(10.0);
    let spec3 = PosterSpec::new(240.0, 180.0, 80.0, 60.0)
        .with_grid(PosterGrid::three_by_three())
        .with_overlap(8.0);
    let p2 = plan(spec2);
    let p3 = plan(spec3);

    // J1-1 2×2 拼回完整（逐格 0 差异、0 空洞、0 重复）
    let mut ok = false;
    let mut detail = format!("plan_fail");
    if let Ok(p) = &p2 {
        match stitch_report(p, 1.0) {
            Ok((diff, holes, dup)) => {
                ok = diff == 0 && holes == 0 && dup == 0;
                detail = format!("2x2 diff={} holes={} dup={}", diff, holes, dup);
            }
            Err(e) => detail = format!("2x2 err={}", e.code()),
        }
    }
    s.add_dyn("50024-J1-2x2拼回", ok, &detail);

    // J1-2 3×3 拼回完整
    let mut ok = false;
    let mut detail = format!("plan_fail");
    if let Ok(p) = &p3 {
        match stitch_report(p, 1.0) {
            Ok((diff, holes, dup)) => {
                ok = diff == 0 && holes == 0 && dup == 0;
                detail = format!("3x3 diff={} holes={} dup={}", diff, holes, dup);
            }
            Err(e) => detail = format!("3x3 err={}", e.code()),
        }
    }
    s.add_dyn("50024-J1-3x3拼回", ok, &detail);

    // J1-3 重叠带一致（2×2 与 3×3 各一遍，mismatch 必须为 0）
    let a2 = p2.as_ref().ok().and_then(|p| audit_overlaps(p, 1.0).ok());
    let a3 = p3.as_ref().ok().and_then(|p| audit_overlaps(p, 1.0).ok());
    let ok = match (&a2, &a3) {
        (Some(x), Some(y)) => {
            x.mismatches == 0
                && y.mismatches == 0
                && x.width_bad == 0
                && y.width_bad == 0
                && x.pairs == 4
                && y.pairs == 12
                && x.samples > 0
                && y.samples > 0
        }
        _ => false,
    };
    let detail = match (&a2, &a3) {
        (Some(x), Some(y)) => format!(
            "pairs={}/{} mis={}/{} badw={}/{}",
            x.pairs, y.pairs, x.mismatches, y.mismatches, x.width_bad, y.width_bad
        ),
        _ => format!("audit_fail"),
    };
    s.add_dyn("50024-J1-重叠一致", ok, &detail);

    // J1-4 重叠带宽 = 规格值；末列/末行带为 0
    let mut ok = false;
    let mut bad = 0usize;
    if let Ok(p) = &p2 {
        let mut i = 0usize;
        while i < p.tiles.len() {
            if let Some(t) = p.tiles.get(i) {
                let want_r = if t.col + 1 < p.spec.grid.cols { p.spec.overlap_mm } else { 0.0 };
                let want_b = if t.row + 1 < p.spec.grid.rows { p.spec.overlap_mm } else { 0.0 };
                if fabsf(t.overlap_right - want_r) > 1e-4 || fabsf(t.overlap_bottom - want_b) > 1e-4
                {
                    bad += 1;
                }
                // 末列/末行的源矩形必须正好贴到目标边
                if t.col + 1 == p.spec.grid.cols && fabsf(t.src.right() - p.spec.target_w) > 1e-3 {
                    bad += 1;
                }
                if t.row + 1 == p.spec.grid.rows && fabsf(t.src.bottom() - p.spec.target_h) > 1e-3 {
                    bad += 1;
                }
            }
            i += 1;
        }
        ok = bad == 0;
    }
    let detail = format!("bad={}", bad);
    s.add_dyn("50024-J1-重叠带宽", ok, &detail);

    // J1-5 放大倍数 ×2/×3 与规则一致（target/source）
    let ok = match (&p2, &p3) {
        (Ok(a), Ok(b)) => {
            fabsf(a.spec.zoom() - 2.0) < 1e-4
                && fabsf(b.spec.zoom() - 3.0) < 1e-4
                && fabsf(a.spec.core_w() * 2.0 - a.spec.target_w) < 1e-3
        }
        _ => false,
    };
    s.add("50024-J1-放大倍数", ok, if ok { "" } else { "倍数不符" });

    // J1-6 覆盖恰一次：拼回画布每格 count 恒为 1
    let mut ok = false;
    let mut max_count = 0u8;
    let mut min_count = 255u8;
    if let Ok(p) = &p3 {
        if let Ok(c) = compose_core(p, 1.0) {
            let mut i = 0usize;
            while i < c.counts.len() {
                if let Some(v) = c.counts.get(i) {
                    if *v > max_count {
                        max_count = *v;
                    }
                    if *v < min_count {
                        min_count = *v;
                    }
                }
                i += 1;
            }
            ok = min_count == 1 && max_count == 1;
        }
    }
    let detail = format!("min={} max={}", min_count, max_count);
    s.add_dyn("50024-J1-覆盖唯一", ok, &detail);

    // J1-7 裁切后核心尺寸相加 = 目标尺寸（几何守恒）
    let mut ok = false;
    if let Ok(p) = &p2 {
        let mut sum_w = 0.0f32;
        let mut sum_h = 0.0f32;
        let mut c = 0u8;
        while c < p.spec.grid.cols {
            if let Some(t) = p.tile(c, 0) {
                sum_w += t.core_w;
            }
            c += 1;
        }
        let mut r = 0u8;
        while r < p.spec.grid.rows {
            if let Some(t) = p.tile(0, r) {
                sum_h += t.core_h;
            }
            r += 1;
        }
        ok = fabsf(sum_w - p.spec.target_w) < 1e-3 && fabsf(sum_h - p.spec.target_h) < 1e-3;
    }
    s.add("50024-J1-尺寸守恒", ok, if ok { "" } else { "核心尺寸不等于目标" });

    // J1-8 块源矩形互不越出目标，且都为正
    let mut ok = true;
    if let Ok(p) = &p3 {
        let target = Rect::new(0.0, 0.0, p.spec.target_w, p.spec.target_h);
        let mut i = 0usize;
        while i < p.tiles.len() {
            if let Some(t) = p.tiles.get(i) {
                if !t.src.is_positive() || !t.src.inside(&target, 1e-3) {
                    ok = false;
                }
            }
            i += 1;
        }
    } else {
        ok = false;
    }
    s.add("50024-J1-块不越界", ok, if ok { "" } else { "块越出目标" });

    // J1-9 非法档位：5×2 与 0 列
    let e1 = plan(spec2.with_grid(PosterGrid::new(5, 2))).err() == Some(PosterError::Grid);
    let e2 = plan(spec2.with_grid(PosterGrid::new(0, 2))).err() == Some(PosterError::Grid);
    let ok = e1 && e2;
    s.add("50024-J1-非法档位", ok, if ok { "" } else { "非法档位未拒" });

    // J1-10 重叠过大拒绝（70mm ≥ 半块 110mm? 2×2 核心 120×90，半块下限 45）
    let ok = plan(spec2.with_overlap(70.0)).err() == Some(PosterError::Overlap)
        && plan(spec2.with_overlap(-1.0)).err() == Some(PosterError::Overlap);
    s.add("50024-J1-重叠拒", ok, if ok { "" } else { "非法重叠未拒" });

    // J1-11 目标过小 / 各向异性源
    let e1 = plan(PosterSpec::new(40.0, 180.0, 20.0, 90.0).with_grid(PosterGrid::two_by_two()))
        .err()
        == Some(PosterError::Target);
    let e2 = plan(PosterSpec::new(240.0, 180.0, 120.0, 60.0).with_grid(PosterGrid::two_by_two()))
        .err()
        == Some(PosterError::Zoom);
    let ok = e1 && e2;
    s.add("50024-J1-尺寸拒", ok, if ok { "" } else { "非法尺寸未拒" });

    // J1-12 确定性：两次求解逐块一致
    let again = plan(spec3);
    let ok = match (&p3, &again) {
        (Ok(a), Ok(b)) => a.tiles == b.tiles,
        _ => false,
    };
    s.add("50024-J1-确定性", ok, if ok { "" } else { "两次计划不一致" });

    // J1-13 放大往返：源文档矩形 × zoom 回到海报矩形（误差 < 0.01mm）
    let mut ok = false;
    let mut max_err = 0.0f32;
    if let Ok(p) = &p3 {
        let z = p.spec.zoom();
        let mut i = 0usize;
        while i < p.tiles.len() {
            if let Some(t) = p.tiles.get(i) {
                let e = fabsf(t.dst.x * z - t.src.x) + fabsf(t.dst.y * z - t.src.y);
                if e > max_err {
                    max_err = e;
                }
            }
            i += 1;
        }
        ok = max_err < 1e-2;
    }
    let detail = format!("max_err={:.6}", max_err);
    s.add_dyn("50024-J1-放大往返", ok, &detail);

    // J1-14 注入核
    let (caught, total) = injection_core();
    let ok = caught == total;
    let detail = format!("注入 {}/{}", caught, total);
    s.add_dyn("50024-J1-注入必抓", ok, &detail);

    // J1-15 重叠带抽样量足够（带宽 10mm、2×2 → 4 对边 × 10 列 × 120 行）
    let mut ok = false;
    if let Some(a) = &a2 {
        // 水平两对边：每对 10×90 格；垂直两对边：每对 10×120 格 → 4200
        ok = a.samples == 4200 && a.pairs == 4;
    }
    let detail = match &a2 {
        Some(a) => format!("samples={} pairs={}", a.samples, a.pairs),
        None => format!("audit_fail"),
    };
    s.add_dyn("50024-J1-抽样账", ok, &detail);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50024_checks_green() {
        let set = run_unx50024_checks();
        assert!(set.all_passed(), "UNX-F50024 red: {}", set.red_items_text());
    }

    /// 4×4 档也应当拼回完整（每块 60×45mm，重叠 5mm）。
    #[test]
    fn unx50024_four_by_four() {
        let spec = PosterSpec::new(240.0, 180.0, 60.0, 45.0)
            .with_grid(PosterGrid::new(4, 4))
            .with_overlap(5.0);
        let p = plan(spec);
        assert!(p.is_ok());
        if let Ok(p) = p {
            let r = stitch_report(&p, 1.0);
            assert_eq!(r.ok(), Some((0usize, 0usize, 0usize)));
        }
    }

    /// 相邻块的重叠带在几何上必须真的相交，宽度等于规格。
    #[test]
    fn unx50024_overlap_geometry() {
        let spec = PosterSpec::new(240.0, 180.0, 120.0, 90.0)
            .with_grid(PosterGrid::new(3, 3))
            .with_overlap(8.0);
        let p = plan(spec);
        let p = match p {
            Ok(v) => v,
            Err(_) => return,
        };
        let a = p.tile(0, 0);
        let b = p.tile(1, 0);
        match (a, b) {
            (Some(a), Some(b)) => {
                let strip = a.src.intersect(&b.src, 1e-4);
                match strip {
                    Some(r) => assert!(fabsf(r.w - 8.0) < 1e-3),
                    None => panic!("no strip"),
                }
            }
            _ => panic!("missing tile"),
        }
    }
}
