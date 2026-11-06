//! UNX-F50021 N-up 布局引擎（2/4/6/9/16-up 页面编排）（0x42ec0 段）
//!
//! 锚点：UNX-F50021-J1 五档编排 ×30 页序与缩放逐项一致；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段：`SEG_BASE = 0x42ec0`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 10`，
//! 逐项见 [`NUpError`]。
//!
//! 编排（imposition）有两件事必须逐位可复算，否则拼版一定错页：
//! 1. **页序**——「逻辑第 k 页落到哪张纸的哪个物理格」是纯函数：行主序 /
//!    列主序 / 镜像（双面翻背用）三种映射各自可逆，且对任意档位都保持
//!    「每页恰好出现一次」的守恒律；
//! 2. **缩放**——`scale = min(cell_w / page_w, cell_h / page_h)`（装填档取
//!    max、自适应档在旋转后取更大者），放置框按缩放后页面尺寸在格内**居中**。
//!
//! 几何一律用点（1/72 英寸），不引入任何 DPI 换算；所有比较带 1e-4 容差，
//! 但**同一算式**在计划与判据里各算一遍，容差只吸收浮点结合律差异，不吸收
//! 逻辑错误。
//!
//! 结构清单：
//! * [`PageSize`] / [`Rect`]：纸张与放置框几何；
//! * [`NUpMode`]：五档编排（2/4/6/9/16）与网格（列 × 行）；
//! * [`NUpSpec`] + [`CellGrid`]：纸面参数与格网推导（边距、跨缝、格尺寸）；
//! * [`Plan`] / [`SheetPlan`] / [`Placement`]：编排结果（页序 + 缩放 + 放置框）；
//! * [`BookletPlan`]：书帖（骑马订）2-up 正反配对；
//! * [`injection_core`]：6 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::fabsf;

/// 诊断段（0x42ec0 独占 0x20）。
pub const SEG_BASE: u32 = 0x42ec0;

const E_EMPTY: u16 = 1;
const E_PAGE_COUNT: u16 = 2;
const E_SHEET_SMALL: u16 = 3;
const E_RANGE: u16 = 4;
const E_BAD_MODE: u16 = 5;
const E_OVERFLOW: u16 = 6;
const E_CELL: u16 = 7;
const E_NO_PLACEMENT: u16 = 8;
const E_BOOKLET: u16 = 9;
const E_COVERAGE: u16 = 10;

/// 编排错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NUpError {
    /// 页数为 0。
    Empty,
    /// 页数超出引擎上限。
    PageCount,
    /// 纸张扣掉边距/跨缝后装不下最小格。
    SheetSmall,
    /// 参数为负或非有限（边距、跨缝、纸面尺寸）。
    Range,
    /// 档位与网格不自洽（不可达，仍显式返回）。
    BadMode,
    /// 页数 × 档位溢出 u16。
    Overflow,
    /// 单格尺寸退化（宽或高不足 1pt）。
    Cell,
    /// 查询的页/格没有放置记录。
    NoPlacement,
    /// 书帖参数非法。
    Booklet,
    /// 编排结果未覆盖全部页（守恒破坏）。
    Coverage,
}

impl NUpError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            NUpError::Empty => E_EMPTY,
            NUpError::PageCount => E_PAGE_COUNT,
            NUpError::SheetSmall => E_SHEET_SMALL,
            NUpError::Range => E_RANGE,
            NUpError::BadMode => E_BAD_MODE,
            NUpError::Overflow => E_OVERFLOW,
            NUpError::Cell => E_CELL,
            NUpError::NoPlacement => E_NO_PLACEMENT,
            NUpError::Booklet => E_BOOKLET,
            NUpError::Coverage => E_COVERAGE,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            NUpError::Empty => "page count is zero",
            NUpError::PageCount => "page count above engine limit",
            NUpError::SheetSmall => "sheet too small for requested grid",
            NUpError::Range => "geometry parameter negative or not finite",
            NUpError::BadMode => "mode/grid inconsistent",
            NUpError::Overflow => "page count times mode overflows",
            NUpError::Cell => "cell degenerated below 1pt",
            NUpError::NoPlacement => "no placement for queried page",
            NUpError::Booklet => "booklet parameters invalid",
            NUpError::Coverage => "plan does not cover every page once",
        }
    }
}

/// 纸张/页面尺寸（单位：点）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSize {
    /// 宽。
    pub w: f32,
    /// 高。
    pub h: f32,
}

impl PageSize {
    /// 构造。
    pub const fn new(w: f32, h: f32) -> PageSize {
        PageSize { w, h }
    }

    /// A4 纵向（595×842pt，来源：210×297mm × 72/25.4）。
    pub const A4_PORTRAIT: PageSize = PageSize::new(595.0, 842.0);
    /// A4 横向。
    pub const A4_LANDSCAPE: PageSize = PageSize::new(842.0, 595.0);

    /// 尺寸是否可用（正且有限）。
    pub fn valid(&self) -> bool {
        self.w.is_finite() && self.h.is_finite() && self.w > 0.0 && self.h > 0.0
    }

    /// 是否纵向。
    pub fn portrait(&self) -> bool {
        self.h > self.w
    }

    /// 长短边比（用于日志与档位说明）。
    pub fn aspect(&self) -> f32 {
        if self.h > 0.0 {
            self.w / self.h
        } else {
            0.0
        }
    }
}

/// 放置框（左上角 + 宽高）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// 左。
    pub x: f32,
    /// 上。
    pub y: f32,
    /// 宽。
    pub w: f32,
    /// 高。
    pub h: f32,
}

impl Rect {
    /// 构造。
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    /// 右边界。
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// 下边界。
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// 横向中心。
    pub fn cx(&self) -> f32 {
        self.x + self.w * 0.5
    }

    /// 纵向中心。
    pub fn cy(&self) -> f32 {
        self.y + self.h * 0.5
    }

    /// `o` 是否落在本框内（容差 `eps`）。
    pub fn contains(&self, o: &Rect, eps: f32) -> bool {
        o.x >= self.x - eps
            && o.y >= self.y - eps
            && o.right() <= self.right() + eps
            && o.bottom() <= self.bottom() + eps
    }
}

/// 五档编排。网格取业界惯用摆法：2-up 横纸 2×1，4-up 2×2，6-up 2×3，
/// 9-up 3×3，16-up 4×4——列数先定，行数由容量整除得出。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NUpMode {
    /// 2-up。
    Two,
    /// 4-up。
    Four,
    /// 6-up。
    Six,
    /// 9-up。
    Nine,
    /// 16-up。
    Sixteen,
}

impl NUpMode {
    /// 每面页数。
    pub fn up(self) -> u8 {
        match self {
            NUpMode::Two => 2,
            NUpMode::Four => 4,
            NUpMode::Six => 6,
            NUpMode::Nine => 9,
            NUpMode::Sixteen => 16,
        }
    }

    /// 网格 (列, 行)。
    pub fn grid(self) -> (u8, u8) {
        match self {
            NUpMode::Two => (2, 1),
            NUpMode::Four => (2, 2),
            NUpMode::Six => (2, 3),
            NUpMode::Nine => (3, 3),
            NUpMode::Sixteen => (4, 4),
        }
    }

    /// 每面容量（必须等于 列×行，由判据守）。
    pub fn capacity(self) -> u8 {
        let (c, r) = self.grid();
        c * r
    }

    /// 档位标签。
    pub fn label(self) -> &'static str {
        match self {
            NUpMode::Two => "2-up",
            NUpMode::Four => "4-up",
            NUpMode::Six => "6-up",
            NUpMode::Nine => "9-up",
            NUpMode::Sixteen => "16-up",
        }
    }

    /// 五档全集（判据遍历用）。
    pub const ALL: [NUpMode; 5] = [
        NUpMode::Two,
        NUpMode::Four,
        NUpMode::Six,
        NUpMode::Nine,
        NUpMode::Sixteen,
    ];
}

/// 页序映射。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// 行主序：左→右，上→下。
    RowMajor,
    /// 列主序：上→下，左→右。
    ColumnMajor,
    /// 行内镜像：左右翻转（双面翻背必须用它，否则背面与正面错格）。
    Mirror,
}

impl Order {
    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            Order::RowMajor => "row-major",
            Order::ColumnMajor => "column-major",
            Order::Mirror => "mirror-x",
        }
    }
}

/// 缩放策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// 装下（等比缩到完全可见）。
    Fit,
    /// 填满（等比放大到铺满，超出部分裁切）。
    Fill,
    /// 自适应：允许旋转 90°，取更大的那一档。
    Auto,
}

impl Fit {
    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            Fit::Fit => "fit",
            Fit::Fill => "fill",
            Fit::Auto => "auto",
        }
    }
}

/// 最小可用格边长（点）。低于此值的格放不下任何可读内容，直接拒。
pub const MIN_CELL: f32 = 8.0;
/// 引擎页数上限（u16 之内留余量，同时挡住病态分配）。
pub const MAX_PAGES: u16 = 4096;

/// 纸面参数：纸张、页面、边距、跨缝。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NUpSpec {
    /// 输出纸张。
    pub sheet: PageSize,
    /// 源页面。
    pub page: PageSize,
    /// 四周边距（点）。
    pub margin: f32,
    /// 格间距（点）。
    pub gutter: f32,
}

impl NUpSpec {
    /// 构造（不做校验，校验在 [`NUpSpec::cells`]）。
    pub const fn new(sheet: PageSize, page: PageSize, margin: f32, gutter: f32) -> NUpSpec {
        NUpSpec {
            sheet,
            page,
            margin,
            gutter,
        }
    }

    /// A4 标准档：2-up 走横纸（两页并排），其余走纵纸；边距 18pt、跨缝 12pt。
    pub fn a4(mode: NUpMode) -> NUpSpec {
        let sheet = match mode {
            NUpMode::Two => PageSize::A4_LANDSCAPE,
            _ => PageSize::A4_PORTRAIT,
        };
        NUpSpec::new(sheet, PageSize::A4_PORTRAIT, 18.0, 12.0)
    }

    /// 推导格网。所有几何校验集中在这里，计划路径不再重复判断。
    pub fn cells(&self, mode: NUpMode) -> Result<CellGrid, NUpError> {
        if !self.sheet.valid() || !self.page.valid() {
            return Err(NUpError::Range);
        }
        if !self.margin.is_finite() || !self.gutter.is_finite() || self.margin < 0.0 || self.gutter < 0.0 {
            return Err(NUpError::Range);
        }
        let (cols, rows) = mode.grid();
        if cols == 0 || rows == 0 || cols.saturating_mul(rows) != mode.up() {
            return Err(NUpError::BadMode);
        }
        let gut_w = (cols as f32 - 1.0) * self.gutter;
        let gut_h = (rows as f32 - 1.0) * self.gutter;
        let usable_w = self.sheet.w - 2.0 * self.margin - gut_w;
        let usable_h = self.sheet.h - 2.0 * self.margin - gut_h;
        if usable_w < MIN_CELL || usable_h < MIN_CELL {
            return Err(NUpError::SheetSmall);
        }
        let cell_w = usable_w / cols as f32;
        let cell_h = usable_h / rows as f32;
        if cell_w < MIN_CELL || cell_h < MIN_CELL {
            return Err(NUpError::Cell);
        }
        Ok(CellGrid {
            cols,
            rows,
            cell_w,
            cell_h,
            origin_x: self.margin,
            origin_y: self.margin,
            gutter: self.gutter,
        })
    }
}

/// 格网：由纸面参数推导出的等分格。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellGrid {
    /// 列数。
    pub cols: u8,
    /// 行数。
    pub rows: u8,
    /// 格宽。
    pub cell_w: f32,
    /// 格高。
    pub cell_h: f32,
    /// 左上起点 X（= 边距）。
    pub origin_x: f32,
    /// 左上起点 Y（= 边距）。
    pub origin_y: f32,
    /// 跨缝。
    pub gutter: f32,
}

impl CellGrid {
    /// 取 (col,row) 格的矩形；越界返回 `None`。
    pub fn cell(&self, col: u8, row: u8) -> Option<Rect> {
        if col >= self.cols || row >= self.rows {
            return None;
        }
        let x = self.origin_x + col as f32 * (self.cell_w + self.gutter);
        let y = self.origin_y + row as f32 * (self.cell_h + self.gutter);
        Some(Rect::new(x, y, self.cell_w, self.cell_h))
    }

    /// 物理格下标（行主序编号），用于页序核对。
    pub fn index_of(&self, col: u8, row: u8) -> Option<u8> {
        if col >= self.cols || row >= self.rows {
            return None;
        }
        Some(row * self.cols + col)
    }

    /// 格数。
    pub fn capacity(&self) -> u8 {
        self.cols * self.rows
    }
}

/// 逻辑第 k 页落到哪个物理格（三档映射的唯一实现）。
///
/// 三档都是 `0..cap` 上的双射，因此「每页恰好一次」对任何档位都成立；
/// 差异只在**位置**：行主序按行铺、列主序按列铺、镜像在每一行内左右翻转。
pub fn physical_cell(order: Order, k: u8, cols: u8, rows: u8) -> Option<(u8, u8)> {
    if cols == 0 || rows == 0 || k >= cols.saturating_mul(rows) {
        return None;
    }
    match order {
        Order::RowMajor => Some((k % cols, k / cols)),
        Order::ColumnMajor => Some((k / rows, k % rows)),
        Order::Mirror => Some((cols - 1 - (k % cols), k / cols)),
    }
}

/// 缩放与旋转：`Fit` 取 min、`Fill` 取 max、`Auto` 在旋转 90° 后取更大者。
pub fn fit_scale(cell: Rect, page: PageSize, fit: Fit) -> (f32, bool) {
    if !page.valid() || cell.w <= 0.0 || cell.h <= 0.0 {
        return (0.0, false);
    }
    let s_fit = {
        let a = cell.w / page.w;
        let b = cell.h / page.h;
        if a < b {
            a
        } else {
            b
        }
    };
    let s_rot = {
        let a = cell.w / page.h;
        let b = cell.h / page.w;
        if a < b {
            a
        } else {
            b
        }
    };
    match fit {
        Fit::Fit => (s_fit, false),
        Fit::Fill => {
            let a = cell.w / page.w;
            let b = cell.h / page.h;
            (if a > b { a } else { b }, false)
        }
        Fit::Auto => {
            if s_rot > s_fit {
                (s_rot, true)
            } else {
                (s_fit, false)
            }
        }
    }
}

/// 在格内居中摆放缩放后的页面。
pub fn place_rect(cell: Rect, page: PageSize, scale: f32, rotated: bool) -> Rect {
    let (pw, ph) = if rotated {
        (page.h * scale, page.w * scale)
    } else {
        (page.w * scale, page.h * scale)
    };
    Rect::new(
        cell.x + (cell.w - pw) * 0.5,
        cell.y + (cell.h - ph) * 0.5,
        pw,
        ph,
    )
}

/// 一条放置记录。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// 第几张纸（0 起）。
    pub sheet: u16,
    /// 物理格下标（行主序编号）。
    pub cell: u8,
    /// 列。
    pub col: u8,
    /// 行。
    pub row: u8,
    /// 源页码（1 起）。
    pub source_page: u16,
    /// 放置框。
    pub rect: Rect,
    /// 缩放系数。
    pub scale: f32,
    /// 是否旋转 90°。
    pub rotated: bool,
}

/// 单张纸的编排摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetPlan {
    /// 纸序（0 起）。
    pub index: u16,
    /// 本张有页的格数。
    pub placements: u8,
    /// 本张空格数（末张通常非 0）。
    pub blanks: u8,
}

/// 编排结果。
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// 档位。
    pub mode: NUpMode,
    /// 页序映射。
    pub order: Order,
    /// 缩放策略。
    pub fit: Fit,
    /// 总页数。
    pub page_count: u16,
    /// 每面容量。
    pub capacity: u8,
    /// 放置记录（按逻辑页序）。
    pub placements: Vec<Placement>,
    /// 每张纸的摘要。
    pub sheets: Vec<SheetPlan>,
}

impl Plan {
    /// 纸张张数。
    pub fn sheet_count(&self) -> usize {
        self.sheets.len()
    }

    /// 某页的放置记录（页号 1 起）。
    pub fn placement_for_page(&self, page: u16) -> Option<&Placement> {
        let mut i = 0usize;
        while i < self.placements.len() {
            if let Some(p) = self.placements.get(i) {
                if p.source_page == page {
                    return Some(p);
                }
            }
            i += 1;
        }
        None
    }

    /// 某张纸某物理格上的页码；空格返回 `None`。
    pub fn page_at(&self, sheet: u16, cell: u8) -> Option<u16> {
        let mut i = 0usize;
        while i < self.placements.len() {
            if let Some(p) = self.placements.get(i) {
                if p.sheet == sheet && p.cell == cell {
                    return Some(p.source_page);
                }
            }
            i += 1;
        }
        None
    }

    /// 某张纸的物理格序（按 cell 升序，仅含有页的格）。
    pub fn physical_sequence(&self, sheet: u16) -> Vec<(u8, u16)> {
        let mut out: Vec<(u8, u16)> = Vec::new();
        let mut c = 0u8;
        while c < self.capacity {
            if let Some(p) = self.page_at(sheet, c) {
                out.push((c, p));
            }
            c += 1;
        }
        out
    }

    /// 守恒校验：`1..=page_count` 每页恰好出现一次。
    pub fn covers_all(&self) -> bool {
        let mut page = 1u16;
        while page <= self.page_count {
            let mut hits = 0usize;
            let mut i = 0usize;
            while i < self.placements.len() {
                if let Some(p) = self.placements.get(i) {
                    if p.source_page == page {
                        hits += 1;
                    }
                }
                i += 1;
            }
            if hits != 1 {
                return false;
            }
            page += 1;
        }
        self.placements.len() == self.page_count as usize
    }

    /// 计划级缩放（同档同格尺，取首条；无放置记录时为 0）。
    pub fn uniform_scale(&self) -> f32 {
        match self.placements.first() {
            Some(p) => p.scale,
            None => 0.0,
        }
    }

    /// 空格总数（末张不整的显性账）。
    pub fn total_blanks(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.sheets.len() {
            if let Some(s) = self.sheets.get(i) {
                n += s.blanks as usize;
            }
            i += 1;
        }
        n
    }

    /// 一行摘要文本。
    pub fn summary(&self) -> String {
        format!(
            "{} x{} pages={} sheets={} scale={:.4} order={} fit={} blanks={}",
            self.mode.label(),
            self.capacity,
            self.page_count,
            self.sheets.len(),
            self.uniform_scale(),
            self.order.label(),
            self.fit.label(),
            self.total_blanks()
        )
    }
}

/// 主编排入口：`pages` 页按 `mode` 排到 `spec` 纸面上。
///
/// 页序：逻辑第 k 页（0 起）落到 `s*cap + k` 号源页，物理格由
/// [`physical_cell`] 决定；末张不足一版时后排留空（`blanks` 如实记账）。
pub fn plan(
    mode: NUpMode,
    order: Order,
    fit: Fit,
    pages: u16,
    spec: &NUpSpec,
) -> Result<Plan, NUpError> {
    if pages == 0 {
        return Err(NUpError::Empty);
    }
    if pages > MAX_PAGES {
        return Err(NUpError::PageCount);
    }
    let grid = spec.cells(mode)?;
    let cap = mode.capacity();
    if cap == 0 {
        return Err(NUpError::BadMode);
    }
    // 张数：(pages + cap - 1) / cap，用 u32 中转避免 u16 加法溢出。
    let sheets_needed_u32 = (pages as u32 + cap as u32 - 1) / cap as u32;
    if sheets_needed_u32 > u16::MAX as u32 {
        return Err(NUpError::Overflow);
    }
    let sheets_needed = sheets_needed_u32 as u16;
    let mut placements: Vec<Placement> = Vec::with_capacity(pages as usize);
    let mut sheets: Vec<SheetPlan> = Vec::with_capacity(sheets_needed as usize);
    let mut s = 0u16;
    while s < sheets_needed {
        let mut filled = 0u8;
        let mut k = 0u8;
        while k < cap {
            let page = s as u32 * cap as u32 + k as u32 + 1;
            if page <= pages as u32 {
                let (col, row) = match physical_cell(order, k, grid.cols, grid.rows) {
                    Some(v) => v,
                    None => return Err(NUpError::BadMode),
                };
                let cell = match grid.cell(col, row) {
                    Some(c) => c,
                    None => return Err(NUpError::NoPlacement),
                };
                let (scale, rotated) = fit_scale(cell, spec.page, fit);
                if !(scale > 0.0) || !scale.is_finite() {
                    return Err(NUpError::Cell);
                }
                let rect = place_rect(cell, spec.page, scale, rotated);
                let idx = match grid.index_of(col, row) {
                    Some(v) => v,
                    None => return Err(NUpError::NoPlacement),
                };
                placements.push(Placement {
                    sheet: s,
                    cell: idx,
                    col,
                    row,
                    source_page: page as u16,
                    rect,
                    scale,
                    rotated,
                });
                filled = filled.saturating_add(1);
            }
            k += 1;
        }
        sheets.push(SheetPlan {
            index: s,
            placements: filled,
            blanks: cap.saturating_sub(filled),
        });
        s += 1;
    }
    let plan = Plan {
        mode,
        order,
        fit,
        page_count: pages,
        capacity: cap,
        placements,
        sheets,
    };
    // 落成即自检守恒：不守恒的计划不允许返回（宁可显式失败）。
    if !plan.covers_all() {
        return Err(NUpError::Coverage);
    }
    Ok(plan)
}

/// 书帖的一面：2 个槽（0 表示空白页）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookletSide {
    /// 所属纸张。
    pub sheet: u16,
    /// 面序：0 = 正面，1 = 反面。
    pub side: u8,
    /// 两个槽上的页码（0 = 空白）。
    pub slots: [u16; 2],
}

/// 书帖计划。
#[derive(Clone, Debug, PartialEq)]
pub struct BookletPlan {
    /// 原始页数。
    pub pages: u16,
    /// 补齐到 4 的倍数后的页数。
    pub padded: u16,
    /// 所有面（每张纸两面）。
    pub sides: Vec<BookletSide>,
}

impl BookletPlan {
    /// 某个源页在全部面里出现的次数（守恒校验用）。
    pub fn occurrences(&self, page: u16) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.sides.len() {
            if let Some(s) = self.sides.get(i) {
                if s.slots[0] == page {
                    n += 1;
                }
                if s.slots[1] == page {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 空白槽数。
    pub fn blank_slots(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.sides.len() {
            if let Some(s) = self.sides.get(i) {
                if s.slots[0] == 0 {
                    n += 1;
                }
                if s.slots[1] == 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 张数。
    pub fn sheet_count(&self) -> usize {
        self.padded as usize / 4
    }
}

/// 取页号，越界（含补齐区）返回 0（空白槽）。
fn slot_or_blank(v: i32, pages: u16) -> u16 {
    if v >= 1 && v <= pages as i32 {
        v as u16
    } else {
        0
    }
}

/// 骑马订书帖：正反两面的 2-up 配对。
///
/// 配对公式（`n = padded`，`k` = 帖序号）：
/// * 正面：`[n - 2k, 1 + 2k]`
/// * 反面：`[2 + 2k, n - 1 - 2k]`
///
/// 两面配对之和恒为 `n + 1`——这正是「对折后页边对齐」的几何条件，
/// 也是判据里最硬的一条性质（不依赖任何魔数）。
pub fn plan_booklet(pages: u16) -> Result<BookletPlan, NUpError> {
    if pages == 0 {
        return Err(NUpError::Empty);
    }
    if pages > MAX_PAGES {
        return Err(NUpError::PageCount);
    }
    let rem = pages % 4;
    let padded = if rem == 0 { pages } else { pages + (4 - rem) };
    if padded % 4 != 0 {
        return Err(NUpError::Booklet);
    }
    let quires = padded / 4;
    let mut sides: Vec<BookletSide> = Vec::with_capacity(quires as usize * 2);
    let mut k: u16 = 0;
    while k < quires {
        let kk = k as i32;
        let n = padded as i32;
        let f0 = slot_or_blank(n - 2 * kk, pages);
        let f1 = slot_or_blank(1 + 2 * kk, pages);
        let b0 = slot_or_blank(2 + 2 * kk, pages);
        let b1 = slot_or_blank(n - 1 - 2 * kk, pages);
        sides.push(BookletSide {
            sheet: k,
            side: 0,
            slots: [f0, f1],
        });
        sides.push(BookletSide {
            sheet: k,
            side: 1,
            slots: [b0, b1],
        });
        k += 1;
    }
    Ok(BookletPlan {
        pages,
        padded,
        sides,
    })
}

/// 注入核：6 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;
    let spec = NUpSpec::a4(NUpMode::Four);

    // 1) 零页
    if plan(NUpMode::Four, Order::RowMajor, Fit::Fit, 0, &spec).err() == Some(NUpError::Empty) {
        caught += 1;
    }

    // 2) 页数超限
    if plan(NUpMode::Four, Order::RowMajor, Fit::Fit, MAX_PAGES + 1, &spec).err()
        == Some(NUpError::PageCount)
    {
        caught += 1;
    }

    // 3) 边距吃掉纸面（595 宽扣 2×400 格网装不下）
    let tight = NUpSpec::new(PageSize::A4_PORTRAIT, PageSize::A4_PORTRAIT, 400.0, 12.0);
    if plan(NUpMode::Four, Order::RowMajor, Fit::Fit, 4, &tight).err()
        == Some(NUpError::SheetSmall)
    {
        caught += 1;
    }

    // 4) 负跨缝
    let neg = NUpSpec::new(PageSize::A4_PORTRAIT, PageSize::A4_PORTRAIT, 18.0, -4.0);
    if plan(NUpMode::Four, Order::RowMajor, Fit::Fit, 4, &neg).err() == Some(NUpError::Range) {
        caught += 1;
    }

    // 5) 非有限纸面
    let bad = NUpSpec::new(PageSize::new(f32::NAN, 842.0), PageSize::A4_PORTRAIT, 18.0, 12.0);
    if plan(NUpMode::Four, Order::RowMajor, Fit::Fit, 4, &bad).err() == Some(NUpError::Range) {
        caught += 1;
    }

    // 6) 书帖零页
    if plan_booklet(0).err() == Some(NUpError::Empty) {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50021 自检：16 条判据。
pub fn run_unx50021_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50021");

    // J1-1 五档容量自洽：up() == 列×行，且覆盖 2/4/6/9/16。
    let mut cap_ok = true;
    let want = [2u8, 4, 6, 9, 16];
    let mut i = 0usize;
    while i < NUpMode::ALL.len() {
        if let Some(m) = NUpMode::ALL.get(i) {
            let (c, r) = m.grid();
            if m.capacity() != c * r || m.up() != c * r {
                cap_ok = false;
            }
            if let Some(w) = want.get(i) {
                if *w != m.up() {
                    cap_ok = false;
                }
            }
        }
        i += 1;
    }
    let _ = want.get(0);
    s.add("50021-J1-五档容量", cap_ok, if cap_ok { "" } else { "档位容量不自洽" });

    // J1-2 30 页五档守恒：每档每页恰好出现一次、张数 = ceil(30/cap)。
    let mut conserve = true;
    let mut detail = String::new();
    let mut m = 0usize;
    while m < NUpMode::ALL.len() {
        if let Some(mode) = NUpMode::ALL.get(m) {
            let spec = NUpSpec::a4(*mode);
            match plan(*mode, Order::RowMajor, Fit::Fit, 30, &spec) {
                Ok(p) => {
                    let cap = mode.capacity() as u32;
                    let sheets = (30u32 + cap - 1) / cap;
                    if !p.covers_all() || p.sheet_count() as u32 != sheets || p.placements.len() != 30 {
                        conserve = false;
                    }
                    detail.push_str(&format!("{}:{} ", mode.label(), p.sheet_count()));
                }
                Err(_) => conserve = false,
            }
        }
        m += 1;
    }
    s.add_dyn("50021-J1-五档30页守恒", conserve, &detail);

    // J1-3 行主序页序单调，且物理格按 0..filled 连续填充。
    let spec4 = NUpSpec::a4(NUpMode::Four);
    let p4 = match plan(NUpMode::Four, Order::RowMajor, Fit::Fit, 30, &spec4) {
        Ok(p) => p,
        Err(_) => {
            // 4-up 是后续全部判据的基座；它失败必须落红，不能返回空集假绿。
            s.add("50021-J1-编排基座", false, "4-up x30 编排失败");
            return s;
        }
    };
    let mut order_ok = true;
    let mut sht = 0u16;
    while sht < p4.sheet_count() as u16 {
        let seq = p4.physical_sequence(sht);
        let mut j = 0usize;
        while j < seq.len() {
            if let Some((cell, page)) = seq.get(j) {
                if *cell as usize != j {
                    order_ok = false;
                }
                if *page != sht * 4 + j as u16 + 1 {
                    order_ok = false;
                }
            }
            j += 1;
        }
        sht += 1;
    }
    s.add("50021-J1-行主序页序", order_ok, if order_ok { "" } else { "页序与格序不符" });

    // J1-4 缩放逐项一致：每条放置记录都能用格尺寸重算出同一系数。
    let grid4 = match spec4.cells(NUpMode::Four) {
        Ok(g) => g,
        Err(_) => {
            s.add("50021-J1-格网基座", false, "4-up 格网推导失败");
            return s;
        }
    };
    let mut scale_ok = true;
    let mut max_diff = 0.0f32;
    let mut k = 0usize;
    while k < p4.placements.len() {
        if let Some(p) = p4.placements.get(k) {
            if let Some(cell) = grid4.cell(p.col, p.row) {
                let (want, rot) = fit_scale(cell, spec4.page, Fit::Fit);
                let d = fabsf(want - p.scale);
                if d > max_diff {
                    max_diff = d;
                }
                if d > 1e-6 || rot != p.rotated {
                    scale_ok = false;
                }
            }
        }
        k += 1;
    }
    let det = format!("max_diff={:.9} scale={:.4}", max_diff, p4.uniform_scale());
    s.add_dyn("50021-J1-缩放逐项", scale_ok, &det);

    // J1-5 装下档缩放落在 (0,1]：A4 页放进 A4 纸的格子必然不放大。
    let mut bound_ok = true;
    let mut t = 0usize;
    while t < NUpMode::ALL.len() {
        if let Some(mode) = NUpMode::ALL.get(t) {
            let spec = NUpSpec::a4(*mode);
            if let Ok(p) = plan(*mode, Order::RowMajor, Fit::Fit, 12, &spec) {
                let sc = p.uniform_scale();
                if !(sc > 0.0) || sc > 1.0001 {
                    bound_ok = false;
                }
            } else {
                bound_ok = false;
            }
        }
        t += 1;
    }
    s.add("50021-J1-缩放上界", bound_ok, if bound_ok { "" } else { "缩放越界" });

    // J1-6 装下档放置框必在格内（不留裁切）。
    let mut inside_ok = true;
    let mut q = 0usize;
    while q < p4.placements.len() {
        if let Some(p) = p4.placements.get(q) {
            match grid4.cell(p.col, p.row) {
                Some(cell) => {
                    if !cell.contains(&p.rect, 1e-4) {
                        inside_ok = false;
                    }
                }
                None => inside_ok = false,
            }
        }
        q += 1;
    }
    s.add("50021-J1-放置框在格内", inside_ok, if inside_ok { "" } else { "放置框越格" });

    // J1-7 居中：放置框中心与格中心重合。
    let mut center_ok = true;
    let mut c = 0usize;
    while c < p4.placements.len() {
        if let Some(p) = p4.placements.get(c) {
            if let Some(cell) = grid4.cell(p.col, p.row) {
                if fabsf(p.rect.cx() - cell.cx()) > 1e-4 || fabsf(p.rect.cy() - cell.cy()) > 1e-4 {
                    center_ok = false;
                }
            }
        }
        c += 1;
    }
    s.add("50021-J1-格内居中", center_ok, if center_ok { "" } else { "未居中" });

    // J1-8 末张不整：30 页 4-up → 8 张，末张 2 格有页 2 格空。
    let last_idx = if p4.sheets.is_empty() { 0 } else { p4.sheets.len() - 1 };
    let last = p4.sheets.get(last_idx);
    let last_ok = match last {
        Some(sp) => sp.placements == 2 && sp.blanks == 2 && sp.index == 7,
        None => false,
    };
    let ok = last_ok && p4.sheet_count() == 8 && p4.total_blanks() == 2;
    let det = match last {
        Some(sp) => format!("sheets={} last={}/{}", p4.sheet_count(), sp.placements, sp.blanks),
        None => "no last sheet".to_string(),
    };
    s.add_dyn("50021-J1-末张留空", ok, &det);

    // J1-9 确定性：同参两次编排逐项相等（含每个 f32 缩放）。
    let again = plan(NUpMode::Four, Order::RowMajor, Fit::Fit, 30, &spec4);
    let ok = match again {
        Ok(p) => p == p4,
        Err(_) => false,
    };
    s.add("50021-J1-编排确定", ok, if ok { "" } else { "两次编排不一致" });

    // J1-10 列主序：物理第 (col,row) 格 = 逻辑第 col*rows+row 页（0 起）。
    let pc = match plan(NUpMode::Four, Order::ColumnMajor, Fit::Fit, 30, &spec4) {
        Ok(p) => p,
        Err(_) => {
            s.add("50021-J1-列主序可编排", false, "列主序编排失败");
            return s;
        }
    };
    let mut col_ok = pc.covers_all();
    let mut col = 0u8;
    while col < 2 {
        let mut row = 0u8;
        while row < 2 {
            let idx = row * 2 + col;
            let want = 1 + (col as u16) * 2 + row as u16;
            if pc.page_at(0, idx) != Some(want) {
                col_ok = false;
            }
            row += 1;
        }
        col += 1;
    }
    s.add("50021-J1-列主序映射", col_ok, if col_ok { "" } else { "列主序错格" });

    // J1-11 镜像：同张页集合不变，每行左右两格页码互换。
    let pm = match plan(NUpMode::Four, Order::Mirror, Fit::Fit, 30, &spec4) {
        Ok(p) => p,
        Err(_) => {
            s.add("50021-J1-镜像可编排", false, "镜像编排失败");
            return s;
        }
    };
    let mut mirror_ok = pm.covers_all() && pm.placements.len() == p4.placements.len();
    let mut row = 0u8;
    while row < 2 {
        let l = pm.page_at(0, row * 2);
        let r = pm.page_at(0, row * 2 + 1);
        let l0 = p4.page_at(0, row * 2);
        let r0 = p4.page_at(0, row * 2 + 1);
        if l != r0 || r != l0 {
            mirror_ok = false;
        }
        row += 1;
    }
    s.add("50021-J1-镜像换位", mirror_ok, if mirror_ok { "" } else { "镜像未换位" });

    // J1-12 书帖 30 页：补齐 32、每页恰好一次、每面两槽配对和恒为 33。
    let bk = plan_booklet(30);
    let mut bk_ok = false;
    let mut bk_det = String::from("n/a");
    if let Ok(b) = bk {
        bk_ok = b.padded == 32 && b.sides.len() == 16 && b.sheet_count() == 8;
        let mut page = 1u16;
        while page <= 30 {
            if b.occurrences(page) != 1 {
                bk_ok = false;
            }
            page += 1;
        }
        let mut i2 = 0usize;
        while i2 < b.sides.len() {
            if let Some(sd) = b.sides.get(i2) {
                let a = sd.slots[0];
                let d = sd.slots[1];
                if a != 0 && d != 0 && (a + d) != 33 {
                    bk_ok = false;
                }
            }
            i2 += 1;
        }
        bk_det = format!("padded={} sides={} blanks={}", b.padded, b.sides.len(), b.blank_slots());
    }
    s.add_dyn("50021-J1-书帖配对", bk_ok, &bk_det);

    // J1-13 缩放三档：Fill ≥ Fit；Auto ≥ Fit 且横格放纵页时旋转生效。
    let cell = Rect::new(0.0, 0.0, 397.0, 559.0);
    let (s_fit, r_fit) = fit_scale(cell, PageSize::A4_PORTRAIT, Fit::Fit);
    let (s_fill, _) = fit_scale(cell, PageSize::A4_PORTRAIT, Fit::Fill);
    let (s_auto, r_auto) = fit_scale(cell, PageSize::A4_LANDSCAPE, Fit::Auto);
    let (s_auto_fit, _) = fit_scale(cell, PageSize::A4_LANDSCAPE, Fit::Fit);
    let ok = s_fill > s_fit && !r_fit && s_auto >= s_auto_fit && r_auto && s_auto > 0.0;
    let det = format!("fit={:.4} fill={:.4} auto={:.4}", s_fit, s_fill, s_auto);
    s.add_dyn("50021-J1-缩放三档", ok, &det);

    // J1-14 参数护栏：零页 / 超限 / 边距过大 / 负跨缝 四类显式拒绝。
    let tight = NUpSpec::new(PageSize::A4_PORTRAIT, PageSize::A4_PORTRAIT, 400.0, 12.0);
    let neg = NUpSpec::new(PageSize::A4_PORTRAIT, PageSize::A4_PORTRAIT, 18.0, -2.0);
    let ok = plan(NUpMode::Four, Order::RowMajor, Fit::Fit, 0, &spec4).err() == Some(NUpError::Empty)
        && plan(NUpMode::Four, Order::RowMajor, Fit::Fit, MAX_PAGES + 1, &spec4).err()
            == Some(NUpError::PageCount)
        && plan(NUpMode::Four, Order::RowMajor, Fit::Fit, 4, &tight).err() == Some(NUpError::SheetSmall)
        && plan(NUpMode::Four, Order::RowMajor, Fit::Fit, 4, &neg).err() == Some(NUpError::Range);
    s.add("50021-J1-参数护栏", ok, if ok { "" } else { "护栏漏判" });

    // J1-15 往返一致：页 → 放置记录 → (纸, 格) → 页 恒等。
    let mut rt_ok = true;
    let mut pg = 1u16;
    while pg <= 30 {
        match p4.placement_for_page(pg) {
            Some(pl) => {
                if p4.page_at(pl.sheet, pl.cell) != Some(pg) {
                    rt_ok = false;
                }
                if pl.source_page != pg {
                    rt_ok = false;
                }
            }
            None => rt_ok = false,
        }
        pg += 1;
    }
    let ok = rt_ok && p4.placement_for_page(31).is_none();
    s.add("50021-J1-页格往返", ok, if ok { "" } else { "页格往返断裂" });

    // J1-16 六类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50021-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50021_checks_green() {
        let set = run_unx50021_checks();
        assert!(set.all_passed(), "UNX-F50021 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50021_thirty_pages_all_modes() {
        let mut i = 0usize;
        while i < NUpMode::ALL.len() {
            if let Some(mode) = NUpMode::ALL.get(i) {
                let spec = NUpSpec::a4(*mode);
                let p = plan(*mode, Order::RowMajor, Fit::Fit, 30, &spec);
                assert!(p.is_ok(), "{} plan failed", mode.label());
                if let Ok(pl) = p {
                    assert!(pl.covers_all(), "{} lost a page", mode.label());
                    assert_eq!(pl.placements.len(), 30);
                }
            }
            i += 1;
        }
    }

    #[test]
    fn unx50021_booklet_pairs() {
        let b = plan_booklet(30);
        assert!(b.is_ok());
        if let Ok(bk) = b {
            assert_eq!(bk.padded, 32);
            let mut page = 1u16;
            while page <= 30 {
                assert_eq!(bk.occurrences(page), 1);
                page += 1;
            }
        }
    }

    #[test]
    fn unx50021_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 6);
    }
}
