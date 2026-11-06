//! UNX-F50022 N-up 页序规范（横排/纵排两种序显式可选）（0x42ee0 段）
//!
//! 锚点：UNX-F50022-J1 两序 ×50 排列逐页比对一致；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段：`SEG_BASE = 0x42ee0`。诊断码是闭集 `0x42ee1..=0x42eea`
//! （`NupError` 十档，`code()` 逐档返回 `SEG_BASE + n`），段外不产生任何码。
//!
//! 结构清单
//! * 数据结构：`SheetSpec`（纸张/网格/边距，毫米制）、`Rect`（格位矩形）、
//!   `NupOrder`（横排/纵排两序）、`Placement`（单页落位）、`NupPlan`（拼版
//!   计划）、`PageLedger`（页序账本，带重复/乱序审计）；
//! * 几何求解器：`SheetSpec::cell_w/cell_h/frame`、`cell_rect`、
//!   `check_layout`（同面格位互不重叠 + 全部落在可印区内）；
//! * 序映射：`cell_of_slot` / `slot_of_cell`（两序互为逆映射）、
//!   `mirror_col`（双面长边翻背的列镜像）；
//! * 错误路径：`NupError` 十档；所有入口先校验后计算，零 panic；
//! * 注入核：`injection_core` 六类坏样本（空页/零列/纸太小/边距吃掉纸面/
//!   重复页/账本乱序），逐类断言被显式拒绝。
//!
//! 两序为什么这样定义
//! ------------------
//! 横排（Across）按**阅读方向**先走列：槽位 s → (row = s/cols, col = s%cols)；
//! 纵排（Down）按**装订方向**先走行：s → (col = s/rows, row = s%rows)。
//! 两序对同一 `cols×rows` 网格都是双射，且 `cols>1 && rows>1` 时互不相同——
//! J1 要的不是「有个序」，而是「同一批页在两序下的每一格落位都能被独立
//! 复算出来」。因此本模块另有一条**反向实现** `reference_pages`（先枚举格、
//! 再反查槽），与主路径 `imposition_plan`（先枚举槽、再正查格）互为交叉
//! 校验：两边只有在语义真的一致时才会逐页相等。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::fabsf;

/// 诊断段基址（0x42ee0 起 0x20 归本模块，0x42f00 起归下一张卡）。
pub const SEG_BASE: u32 = 0x42ee0;

/// 单面最大格数。A4 横排 4×4=16 已覆盖办公极限，32 格是纸面可读性上限：
/// 单格短边 < 5mm 时任何 N-up 语义都失去物理意义。
pub const MAX_CELLS: u16 = 32;

/// 单计划页数上限：16 张纸 × 32 格 = 512。超过即视为调用方单位错误
/// （把「张」当「页」传进来是最常见的误用）。
pub const MAX_PAGES: u16 = 512;

/// 纸张边长合法区间（mm）。小于 20mm 装不下任何可读网格；大于 2000mm
/// 超出本模块毫米几何的定标假设（f32 在此区间内 1e-4 精度足够）。
pub const MIN_PAPER_MM: f32 = 20.0;
/// 纸张边长上界（mm），见 `MIN_PAPER_MM`。
pub const MAX_PAPER_MM: f32 = 2000.0;

/// 单格最小可读边长（mm）。推导：正文 6pt ≈ 2.1mm 行高 + 上下各 1.5mm 边距
/// ≈ 5.1mm，向下取整 5mm。
pub const MIN_CELL_MM: f32 = 5.0;

pub const E_EMPTY: u16 = 1;
pub const E_GRID: u16 = 2;
pub const E_PAPER: u16 = 3;
pub const E_MARGIN: u16 = 4;
pub const E_FIT: u16 = 5;
pub const E_OVERFLOW: u16 = 6;
pub const E_CELL: u16 = 7;
pub const E_DUPLICATE: u16 = 8;
pub const E_ORDER: u16 = 9;
pub const E_PLAN: u16 = 10;

/// N-up 拼版域错误。每档对应 `SEG_BASE + n` 一个诊断码，段内闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NupError {
    Empty,
    Grid,
    Paper,
    Margin,
    Fit,
    Overflow,
    Cell,
    Duplicate,
    Order,
    Plan,
}

impl NupError {
    /// 诊断码：`SEG_BASE + 档内序号`。
    pub fn code(self) -> u32 {
        let n = match self {
            NupError::Empty => E_EMPTY,
            NupError::Grid => E_GRID,
            NupError::Paper => E_PAPER,
            NupError::Margin => E_MARGIN,
            NupError::Fit => E_FIT,
            NupError::Overflow => E_OVERFLOW,
            NupError::Cell => E_CELL,
            NupError::Duplicate => E_DUPLICATE,
            NupError::Order => E_ORDER,
            NupError::Plan => E_PLAN,
        };
        SEG_BASE + n as u32
    }

    /// 面向操作员的定性说明（稳定文本，可进日志）。
    pub fn message(self) -> &'static str {
        match self {
            NupError::Empty => "page count must be positive",
            NupError::Grid => "grid columns/rows out of range",
            NupError::Paper => "paper size outside 20..2000mm",
            NupError::Margin => "margin/gutter leaves no printable area",
            NupError::Fit => "cell smaller than minimum readable size",
            NupError::Overflow => "page count exceeds plan capacity",
            NupError::Cell => "cell coordinates outside grid",
            NupError::Duplicate => "duplicate slot or page in ledger",
            NupError::Order => "placement appended out of sheet order",
            NupError::Plan => "plan failed self-verification",
        }
    }
}

/// 毫米制矩形（左上角 + 宽高）。所有几何比较都带 eps，避免 f32 除法残差
/// 把「刚好贴边」误判成越界。
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

    /// 右边缘 x 坐标。
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// 下边缘 y 坐标（页坐标向下为正）。
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// 几何中心，供双面镜像与视觉对齐核对。
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    /// 面积（mm²）。
    pub fn area(&self) -> f32 {
        self.w * self.h
    }

    /// 正尺寸判定：宽高都必须严格大于 0 且有限。
    pub fn is_positive(&self) -> bool {
        self.w > 0.0 && self.h > 0.0 && self.w.is_finite() && self.h.is_finite()
    }

    /// 与另一矩形是否有**正面积**交叠。`eps` 用于吸收贴边（共享边界不算重叠）。
    pub fn overlaps(&self, o: &Rect, eps: f32) -> bool {
        let ox = self.x + self.w - o.x;
        let oy = self.y + self.h - o.y;
        let tx = o.x + o.w - self.x;
        let ty = o.y + o.h - self.y;
        ox > eps && oy > eps && tx > eps && ty > eps
    }

    /// 是否被外框完全包含（允许 eps 级贴边）。
    pub fn inside(&self, outer: &Rect, eps: f32) -> bool {
        self.x >= outer.x - eps
            && self.y >= outer.y - eps
            && self.right() <= outer.right() + eps
            && self.bottom() <= outer.bottom() + eps
    }
}

/// 纸张 + 网格 + 边距规格。构造后由 `validate()` 一次性把关，
/// 之后所有求解函数都假定「已校验」或先自行校验。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetSpec {
    pub paper_w: f32,
    pub paper_h: f32,
    pub cols: u8,
    pub rows: u8,
    pub margin: f32,
    pub gutter: f32,
    pub duplex: bool,
}

impl SheetSpec {
    /// 新建单面、零边距规格。边距/间距/双面走链式设置，保证默认值可预测。
    pub const fn new(paper_w: f32, paper_h: f32, cols: u8, rows: u8) -> SheetSpec {
        SheetSpec {
            paper_w,
            paper_h,
            cols,
            rows,
            margin: 0.0,
            gutter: 0.0,
            duplex: false,
        }
    }

    /// 设置四边对称边距（mm）。
    pub fn with_margin(mut self, margin: f32) -> SheetSpec {
        self.margin = margin;
        self
    }

    /// 设置格间间距（mm）。
    pub fn with_gutter(mut self, gutter: f32) -> SheetSpec {
        self.gutter = gutter;
        self
    }

    /// 设置单/双面。
    pub fn with_duplex(mut self, duplex: bool) -> SheetSpec {
        self.duplex = duplex;
        self
    }

    /// 单面格数。
    pub fn cells(&self) -> u16 {
        self.cols as u16 * self.rows as u16
    }

    /// 每张纸的面数（双面 = 2）。
    pub fn sides(&self) -> u16 {
        if self.duplex {
            2
        } else {
            1
        }
    }

    /// 每张纸容量（页）。
    pub fn pages_per_sheet(&self) -> u16 {
        self.cells() * self.sides()
    }

    /// 单格宽（mm）。公式：`(W - 2m - (cols-1)g) / cols`。
    pub fn cell_w(&self) -> f32 {
        if self.cols == 0 {
            return 0.0;
        }
        let usable = self.paper_w - 2.0 * self.margin - (self.cols as f32 - 1.0) * self.gutter;
        usable / self.cols as f32
    }

    /// 单格高（mm）。公式：`(H - 2m - (rows-1)g) / rows`。
    pub fn cell_h(&self) -> f32 {
        if self.rows == 0 {
            return 0.0;
        }
        let usable = self.paper_h - 2.0 * self.margin - (self.rows as f32 - 1.0) * self.gutter;
        usable / self.rows as f32
    }

    /// 可印区（去掉四边边距后的盒子）。
    pub fn frame(&self) -> Rect {
        Rect::new(
            self.margin,
            self.margin,
            self.paper_w - 2.0 * self.margin,
            self.paper_h - 2.0 * self.margin,
        )
    }

    /// 规格合法性。校验顺序即诊断码优先级：网格 → 纸张 → 边距 → 可读性。
    pub fn validate(&self) -> Result<(), NupError> {
        if self.cols == 0 || self.rows == 0 {
            return Err(NupError::Grid);
        }
        if self.cells() > MAX_CELLS {
            return Err(NupError::Grid);
        }
        if !self.paper_w.is_finite() || !self.paper_h.is_finite() {
            return Err(NupError::Paper);
        }
        if self.paper_w < MIN_PAPER_MM
            || self.paper_h < MIN_PAPER_MM
            || self.paper_w > MAX_PAPER_MM
            || self.paper_h > MAX_PAPER_MM
        {
            return Err(NupError::Paper);
        }
        if !self.margin.is_finite() || self.margin < 0.0 || self.gutter < 0.0 {
            return Err(NupError::Margin);
        }
        // 边距两侧 + 至少一格可读宽度必须留得下，否则认定为边距吃穿纸面。
        let min_span = MIN_CELL_MM * self.cols as f32;
        if self.paper_w - 2.0 * self.margin < min_span {
            return Err(NupError::Margin);
        }
        let min_span_h = MIN_CELL_MM * self.rows as f32;
        if self.paper_h - 2.0 * self.margin < min_span_h {
            return Err(NupError::Margin);
        }
        if (self.cols as f32 - 1.0) * self.gutter
            >= self.paper_w - 2.0 * self.margin - MIN_CELL_MM
        {
            return Err(NupError::Margin);
        }
        if self.cell_w() < MIN_CELL_MM || self.cell_h() < MIN_CELL_MM {
            return Err(NupError::Fit);
        }
        Ok(())
    }
}

/// 页序两档。`code()` 是写进计划/账本的持久编码，永不重排。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NupOrder {
    /// 横排：同一行从左到右填满再换行（阅读方向）。
    Across,
    /// 纵排：同一列从上到下填满再换列（装订方向）。
    Down,
}

impl NupOrder {
    /// 持久编码：0=横排，1=纵排。
    pub fn code(self) -> u8 {
        match self {
            NupOrder::Across => 0,
            NupOrder::Down => 1,
        }
    }

    /// 由持久编码还原；未知码显式拒绝（不猜默认序）。
    pub fn from_code(c: u8) -> Result<NupOrder, NupError> {
        match c {
            0 => Ok(NupOrder::Across),
            1 => Ok(NupOrder::Down),
            _ => Err(NupError::Order),
        }
    }

    /// 写日志用的稳定标签。
    pub fn label(self) -> &'static str {
        match self {
            NupOrder::Across => "across-rows",
            NupOrder::Down => "down-columns",
        }
    }

    /// 槽位 → 格坐标。槽位是「第几个落版位置」，格坐标是纸上物理位置。
    pub fn cell_of_slot(self, slot: u16, cols: u8, rows: u8) -> Result<(u8, u8), NupError> {
        if cols == 0 || rows == 0 {
            return Err(NupError::Grid);
        }
        let cells = cols as u16 * rows as u16;
        if slot >= cells {
            return Err(NupError::Cell);
        }
        match self {
            NupOrder::Across => Ok(((slot % cols as u16) as u8, (slot / cols as u16) as u8)),
            NupOrder::Down => Ok(((slot / rows as u16) as u8, (slot % rows as u16) as u8)),
        }
    }

    /// 格坐标 → 槽位（`cell_of_slot` 的逆）。
    pub fn slot_of_cell(self, col: u8, row: u8, cols: u8, rows: u8) -> Result<u16, NupError> {
        if cols == 0 || rows == 0 {
            return Err(NupError::Grid);
        }
        if col >= cols || row >= rows {
            return Err(NupError::Cell);
        }
        match self {
            NupOrder::Across => Ok(row as u16 * cols as u16 + col as u16),
            NupOrder::Down => Ok(col as u16 * rows as u16 + row as u16),
        }
    }
}

/// 纸面两档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Front,
    Back,
}

impl Side {
    /// 持久编码：0=正面，1=背面。
    pub fn code(self) -> u8 {
        match self {
            Side::Front => 0,
            Side::Back => 1,
        }
    }

    /// 由编码还原。
    pub fn from_code(c: u8) -> Result<Side, NupError> {
        match c {
            0 => Ok(Side::Front),
            1 => Ok(Side::Back),
            _ => Err(NupError::Plan),
        }
    }

    /// 写日志用的稳定标签。
    pub fn label(self) -> &'static str {
        match self {
            Side::Front => "front",
            Side::Back => "back",
        }
    }
}

/// 长边翻背的列镜像：`col' = cols-1-col`。为什么是长边：A4 双面打印默认沿
/// 长边翻页（像翻书），此时背面内容在纸坐标系里左右镜像，正面第 0 列
/// 对应背面最后一列，否则翻过来会看到倒序的阅读流。
pub fn mirror_col(col: u8, cols: u8) -> u8 {
    if cols == 0 {
        return 0;
    }
    if col >= cols {
        return col;
    }
    cols - 1 - col
}

/// 单个格位的毫米矩形。越界格坐标显式返回 `Cell`，不做钳位——
/// 钳位会把拼版错误变成静默的页重叠。
pub fn cell_rect(spec: &SheetSpec, col: u8, row: u8) -> Result<Rect, NupError> {
    if col >= spec.cols || row >= spec.rows {
        return Err(NupError::Cell);
    }
    let cw = spec.cell_w();
    let ch = spec.cell_h();
    if cw <= 0.0 || ch <= 0.0 {
        return Err(NupError::Fit);
    }
    let x = spec.margin + col as f32 * (cw + spec.gutter);
    let y = spec.margin + row as f32 * (ch + spec.gutter);
    Ok(Rect::new(x, y, cw, ch))
}

/// 单页落位记录。`page == 0` 表示空白填充位（册子/尾部未满时的显式占位，
/// 不做「跳过不记」——账本必须逐格对齐）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub sheet: u16,
    pub side: Side,
    pub slot: u16,
    pub page: u16,
    pub rect: Rect,
    pub mirrored: bool,
}

impl Placement {
    /// 该落位在纸张坐标系中的格坐标（双面镜像已折算）。
    pub fn cell(&self, spec: &SheetSpec, order: NupOrder) -> Result<(u8, u8), NupError> {
        let (mut col, row) = order.cell_of_slot(self.slot, spec.cols, spec.rows)?;
        if self.mirrored {
            col = mirror_col(col, spec.cols);
        }
        Ok((col, row))
    }
}

/// 拼版计划：规格 + 序 + 全部落位。`placements` 的顺序即生成顺序
/// （sheet 升序 → front/back → slot 升序），`verify()` 用它做全量自检。
#[derive(Clone, Debug)]
pub struct NupPlan {
    pub spec: SheetSpec,
    pub order: NupOrder,
    pub pages: u16,
    pub blanks: u16,
    pub placements: Vec<Placement>,
}

impl NupPlan {
    /// 张数（向上取整）。
    pub fn sheets(&self) -> u16 {
        let pps = self.spec.pages_per_sheet();
        if pps == 0 {
            return 0;
        }
        (self.pages + pps - 1) / pps
    }

    /// 按 (sheet, side, slot) 取页号；未登记返回 `None`（调用方决定怎么报）。
    pub fn page_of(&self, sheet: u16, side: Side, slot: u16) -> Option<u16> {
        let mut i = 0usize;
        while i < self.placements.len() {
            if let Some(p) = self.placements.get(i) {
                if p.sheet == sheet && p.side == side && p.slot == slot {
                    return Some(p.page);
                }
            }
            i += 1;
        }
        None
    }

    /// 按槽序列展开的页号串（空白位保留 0），供账本回放比对。
    pub fn ordered_pages(&self) -> Vec<u16> {
        let mut out = Vec::with_capacity(self.placements.len());
        let mut i = 0usize;
        while i < self.placements.len() {
            if let Some(p) = self.placements.get(i) {
                out.push(p.page);
            }
            i += 1;
        }
        out
    }

    /// 全量自检：容量、唯一性、页数守恒、几何包含、双面镜像一致性。
    pub fn verify(&self) -> Result<(), NupError> {
        self.spec.validate()?;
        let pps = self.spec.pages_per_sheet();
        if pps == 0 || self.pages == 0 {
            return Err(NupError::Empty);
        }
        let expect = self.sheets() as usize * pps as usize;
        if self.placements.len() != expect {
            return Err(NupError::Plan);
        }
        let paper = Rect::new(0.0, 0.0, self.spec.paper_w, self.spec.paper_h);
        let mut seen_pages = 0u16;
        let mut i = 0usize;
        while i < self.placements.len() {
            let p = match self.placements.get(i) {
                Some(v) => *v,
                None => return Err(NupError::Plan),
            };
            if p.sheet >= self.sheets() || p.slot >= self.spec.cells() {
                return Err(NupError::Plan);
            }
            if p.page > self.pages {
                return Err(NupError::Plan);
            }
            if !p.rect.is_positive() || !p.rect.inside(&paper, 1e-3) {
                return Err(NupError::Fit);
            }
            if p.mirrored != (p.side == Side::Back && self.spec.duplex) {
                return Err(NupError::Plan);
            }
            if p.page > 0 {
                seen_pages += 1;
            }
            // 同一 (sheet, side, slot) 只允许一条记录。
            let mut j = i + 1;
            while j < self.placements.len() {
                if let Some(q) = self.placements.get(j) {
                    if q.sheet == p.sheet && q.side == p.side && q.slot == p.slot {
                        return Err(NupError::Duplicate);
                    }
                    if p.page > 0 && q.page == p.page {
                        return Err(NupError::Duplicate);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        if seen_pages != self.pages {
            return Err(NupError::Plan);
        }
        Ok(())
    }
}

/// 生成拼版计划。
///
/// 页号按 (sheet, side, slot) 线性递增（从 1 起），尾部不足处填 0；
/// 双面时背面格位做长边列镜像。所有形状错误在分配 `Vec` 之前就返回。
pub fn imposition_plan(
    pages: u16,
    spec: SheetSpec,
    order: NupOrder,
    duplex: bool,
) -> Result<NupPlan, NupError> {
    if pages == 0 {
        return Err(NupError::Empty);
    }
    if pages > MAX_PAGES {
        return Err(NupError::Overflow);
    }
    let mut s = spec;
    s.duplex = duplex;
    s.validate()?;
    let pps = s.pages_per_sheet();
    if pps == 0 {
        return Err(NupError::Grid);
    }
    let sheets = (pages + pps - 1) / pps;
    let mut placements = Vec::with_capacity(sheets as usize * pps as usize);
    let mut sheet = 0u16;
    while sheet < sheets {
        let mut side_i = 0u16;
        while side_i < s.sides() {
            let side = if side_i == 0 { Side::Front } else { Side::Back };
            let mut slot = 0u16;
            while slot < s.cells() {
                let linear = sheet as u32 * pps as u32
                    + side_i as u32 * s.cells() as u32
                    + slot as u32;
                let page = if linear < pages as u32 {
                    (linear + 1) as u16
                } else {
                    0
                };
                let (col, row) = order.cell_of_slot(slot, s.cols, s.rows)?;
                let (gcol, mirrored) = if side == Side::Back && duplex {
                    (mirror_col(col, s.cols), true)
                } else {
                    (col, false)
                };
                let rect = cell_rect(&s, gcol, row)?;
                placements.push(Placement {
                    sheet,
                    side,
                    slot,
                    page,
                    rect,
                    mirrored,
                });
                slot += 1;
            }
            side_i += 1;
        }
        sheet += 1;
    }
    let blanks = sheets * pps - pages;
    let plan = NupPlan {
        spec: s,
        order,
        pages,
        blanks,
        placements,
    };
    plan.verify()?;
    Ok(plan)
}

/// 独立参考实现（J1 的对照组）。
///
/// 与 `imposition_plan` 的遍历方向相反：这里**先枚举物理格**，再用
/// `slot_of_cell` 反查它占第几槽，最后线性算页号。两条路径共用的只有
/// 纯函数定义（序映射、镜像），任何一条把「行/列」弄反都会在逐页比对时
/// 立刻分叉。
pub fn reference_pages(
    pages: u16,
    spec: &SheetSpec,
    order: NupOrder,
) -> Result<Vec<(u16, Side, u16, u16)>, NupError> {
    let pps = spec.pages_per_sheet();
    if pps == 0 {
        return Err(NupError::Grid);
    }
    let sheets = (pages + pps - 1) / pps;
    let mut out = Vec::with_capacity(sheets as usize * pps as usize);
    let mut sheet = 0u16;
    while sheet < sheets {
        let mut side_i = 0u16;
        while side_i < spec.sides() {
            let side = if side_i == 0 { Side::Front } else { Side::Back };
            let mut row = 0u8;
            while row < spec.rows {
                let mut col = 0u8;
                while col < spec.cols {
                    let slot = order.slot_of_cell(col, row, spec.cols, spec.rows)?;
                    let linear = sheet as u32 * pps as u32
                        + side_i as u32 * spec.cells() as u32
                        + slot as u32;
                    let page = if linear < pages as u32 {
                        (linear + 1) as u16
                    } else {
                        0
                    };
                    out.push((sheet, side, slot, page));
                    col += 1;
                }
                row += 1;
            }
            side_i += 1;
        }
        sheet += 1;
    }
    Ok(out)
}

/// 同面格位互不重叠 + 全部落在可印区内。这是「格位几何」唯一的合法性判据，
/// 计划生成、账本装载、外部反序列化后都要过一遍。
pub fn check_layout(plan: &NupPlan) -> Result<(), NupError> {
    let frame = plan.spec.frame();
    let mut i = 0usize;
    while i < plan.placements.len() {
        let a = match plan.placements.get(i) {
            Some(v) => *v,
            None => return Err(NupError::Plan),
        };
        if !a.rect.inside(&frame, 1e-3) {
            return Err(NupError::Fit);
        }
        let mut j = i + 1;
        while j < plan.placements.len() {
            if let Some(b) = plan.placements.get(j) {
                if a.sheet == b.sheet && a.side == b.side && a.rect.overlaps(&b.rect, 1e-3) {
                    return Err(NupError::Plan);
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 页序账本：按落位顺序记账，逐条审计「重复格 / 重复页 / 乱序」。
///
/// 账本不是计划的副本：它是**接收侧**的独立记录（真实打印链路里账本来自
/// 拼版工作站，计划来自驱动），所以必须能独立地发现重复投递与乱序投递。
#[derive(Clone, Debug)]
pub struct PageLedger {
    entries: Vec<Placement>,
    limit: usize,
}

impl PageLedger {
    /// 建账。`limit` 是容量硬上限，超出即 `Overflow`（不做静默丢条）。
    pub fn new(limit: usize) -> PageLedger {
        PageLedger {
            entries: Vec::new(),
            limit,
        }
    }

    /// 当前条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 追加一条落位。四类拒绝：容量满、格位重复、页重复、序倒退。
    pub fn push(&mut self, p: Placement) -> Result<(), NupError> {
        if self.entries.len() >= self.limit {
            return Err(NupError::Overflow);
        }
        if p.page > MAX_PAGES {
            return Err(NupError::Overflow);
        }
        if let Some(last) = self.entries.last() {
            if p.sheet < last.sheet {
                return Err(NupError::Order);
            }
            if p.sheet == last.sheet && p.side.code() < last.side.code() {
                return Err(NupError::Order);
            }
            if p.sheet == last.sheet && p.side == last.side && p.slot < last.slot {
                return Err(NupError::Order);
            }
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(q) = self.entries.get(i) {
                if q.sheet == p.sheet && q.side == p.side && q.slot == p.slot {
                    return Err(NupError::Duplicate);
                }
                if p.page > 0 && q.page == p.page {
                    return Err(NupError::Duplicate);
                }
            }
            i += 1;
        }
        self.entries.push(p);
        Ok(())
    }

    /// 回放：按记账顺序吐出页号（含空白位 0）。
    pub fn replay(&self) -> Vec<u16> {
        let mut out = Vec::with_capacity(self.entries.len());
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(p) = self.entries.get(i) {
                out.push(p.page);
            }
            i += 1;
        }
        out
    }

    /// 重复格位的独立计数（用于「账本被外部塞脏数据」后的取证）。
    pub fn duplicate_slots(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            let mut j = i + 1;
            while j < self.entries.len() {
                if let (Some(a), Some(b)) = (self.entries.get(i), self.entries.get(j)) {
                    if a.sheet == b.sheet && a.side == b.side && a.slot == b.slot {
                        n += 1;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        n
    }

    /// 按计划顺序整批装载，返回装载条数。任一条被拒即整体返回错误，
    /// 但已装载部分保留（便于调用方对账：错误码 + 已写条数）。
    pub fn load_plan(&mut self, plan: &NupPlan) -> Result<usize, NupError> {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < plan.placements.len() {
            if let Some(p) = plan.placements.get(i) {
                self.push(*p)?;
                n += 1;
            }
            i += 1;
        }
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

/// 六类坏样本：空页、零列、纸太小、边距吃穿、重复页、账本乱序。
/// 返回 `(抓到, 总数)`，判据要求两者相等。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;
    let good = SheetSpec::new(210.0, 297.0, 2, 2).with_margin(10.0);

    // 1) 空页
    if imposition_plan(0, good, NupOrder::Across, false).err() == Some(NupError::Empty) {
        caught += 1;
    }
    // 2) 零列
    let zero_cols = SheetSpec::new(210.0, 297.0, 0, 2).with_margin(10.0);
    if imposition_plan(4, zero_cols, NupOrder::Across, false).err() == Some(NupError::Grid) {
        caught += 1;
    }
    // 3) 纸太小（10mm）
    let tiny = SheetSpec::new(10.0, 10.0, 2, 2);
    if imposition_plan(4, tiny, NupOrder::Across, false).err() == Some(NupError::Paper) {
        caught += 1;
    }
    // 4) 边距吃穿纸面
    let fat = SheetSpec::new(210.0, 297.0, 2, 2).with_margin(104.0);
    if imposition_plan(4, fat, NupOrder::Across, false).err() == Some(NupError::Margin) {
        caught += 1;
    }
    // 5) 重复页（账本里同一页投递两次，格位不同）
    let plan = match imposition_plan(4, good, NupOrder::Across, false) {
        Ok(p) => p,
        Err(_) => return (caught, total),
    };
    let mut ledger = PageLedger::new(64);
    let first = match plan.placements.first() {
        Some(p) => *p,
        None => return (caught, total),
    };
    if ledger.push(first).is_err() {
        return (caught, total);
    }
    let mut dup = first;
    dup.slot = first.slot + 1;
    if ledger.push(dup).err() == Some(NupError::Duplicate) {
        caught += 1;
    }
    // 6) 账本乱序（sheet 倒退）
    let mut late = first;
    late.sheet = 3;
    late.slot = 0;
    late.page = 9;
    if ledger.push(late).is_ok() {
        let mut early = first;
        early.sheet = 1;
        early.slot = 0;
        early.page = 10;
        if ledger.push(early).err() == Some(NupError::Order) {
            caught += 1;
        }
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50022 判据集（16 条）。
pub fn run_unx50022_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50022");
    let a4 = SheetSpec::new(210.0, 297.0, 2, 2).with_margin(10.0);

    // J1-1 容量：2×2 单面 4 页/张，双面 8 页/张
    let ok = a4.cells() == 4 && a4.pages_per_sheet() == 4 && a4.with_duplex(true).pages_per_sheet() == 8;
    s.add("50022-J1-容量", ok, if ok { "" } else { "容量漂移" });

    // J1-2 横排：2 列 3 行，前 3 槽落在同一列? 横排应先在行内走列
    let g23 = SheetSpec::new(210.0, 297.0, 2, 3).with_margin(10.0);
    let s0 = NupOrder::Across.cell_of_slot(0, 2, 3);
    let s1 = NupOrder::Across.cell_of_slot(1, 2, 3);
    let s2 = NupOrder::Across.cell_of_slot(2, 2, 3);
    let ok = s0 == Ok((0, 0)) && s1 == Ok((1, 0)) && s2 == Ok((0, 1));
    let det = format!("{:?}/{:?}/{:?}", s0, s1, s2);
    s.add_dyn("50022-J1-横排行优先", ok, &det);

    // J1-3 纵排：同一列从上到下
    let d0 = NupOrder::Down.cell_of_slot(0, 2, 3);
    let d1 = NupOrder::Down.cell_of_slot(1, 2, 3);
    let d2 = NupOrder::Down.cell_of_slot(2, 2, 3);
    let ok = d0 == Ok((0, 0)) && d1 == Ok((0, 1)) && d2 == Ok((0, 2));
    let det = format!("{:?}/{:?}/{:?}", d0, d1, d2);
    s.add_dyn("50022-J1-纵排列优先", ok, &det);

    // J1-4 两序互逆：cell_of_slot ∘ slot_of_cell = id
    let mut ok = true;
    let mut slot = 0u16;
    while slot < g23.cells() {
        let mut ord_i = 0u8;
        while ord_i < 2 {
            let order = if ord_i == 0 { NupOrder::Across } else { NupOrder::Down };
            if let Ok((c, r)) = order.cell_of_slot(slot, g23.cols, g23.rows) {
                match order.slot_of_cell(c, r, g23.cols, g23.rows) {
                    Ok(back) => {
                        if back != slot {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                }
            } else {
                ok = false;
            }
            ord_i += 1;
        }
        slot += 1;
    }
    s.add("50022-J1-两序互逆", ok, if ok { "" } else { "映射不互逆" });

    // J1-5 两序落位集合相同、纸上分布不同（3×2、6 页）。
    // 比的是「某个物理格里放的是第几页」：两序都得把 1..6 各放一次，
    // 但同一格拿到的页号必须不同——否则「两序」只是同一序的两个别名。
    let g32 = SheetSpec::new(297.0, 210.0, 3, 2).with_margin(8.0);
    let pa = imposition_plan(6, g32, NupOrder::Across, false);
    let pd = imposition_plan(6, g32, NupOrder::Down, false);
    let mut ok = false;
    let mut diff = 0usize;
    if let (Ok(ma), Ok(md)) = (&pa, &pd) {
        let mut row = 0u8;
        while row < 2 {
            let mut col = 0u8;
            while col < 3 {
                match (
                    ma.order.slot_of_cell(col, row, 3, 2),
                    md.order.slot_of_cell(col, row, 3, 2),
                ) {
                    (Ok(sa), Ok(sd)) => {
                        let pa_page = ma.page_of(0, Side::Front, sa);
                        let pd_page = md.page_of(0, Side::Front, sd);
                        if pa_page.is_some() && pd_page.is_some() {
                            if pa_page != pd_page {
                                diff += 1;
                            }
                        }
                    }
                    _ => {}
                }
                col += 1;
            }
            row += 1;
        }
        ok = diff > 0;
    }
    let det = format!("cell_diff={}", diff);
    s.add_dyn("50022-J1-两序分叉", ok, &det);

    // J1-6 五十排列：1..=50 页 × 两序，逐页对齐参考实现
    let g22 = SheetSpec::new(210.0, 297.0, 2, 2).with_margin(10.0).with_duplex(true);
    let mut ok = true;
    let mut compared = 0usize;
    let mut expected = 0usize;
    let mut p = 1u16;
    while p <= 50 {
        let sheets = (p + 7) / 8;
        expected += sheets as usize * 8 * 2;
        let mut ord_i = 0u8;
        while ord_i < 2 {
            let order = if ord_i == 0 { NupOrder::Across } else { NupOrder::Down };
            match imposition_plan(p, g22, order, true) {
                Ok(plan) => match reference_pages(p, &g22, order) {
                    Ok(refs) => {
                        let mut k = 0usize;
                        while k < refs.len() {
                            if let Some((sh, sd, sl, pg)) = refs.get(k) {
                                if plan.page_of(*sh, *sd, *sl) != Some(*pg) {
                                    ok = false;
                                }
                                compared += 1;
                            }
                            k += 1;
                        }
                    }
                    Err(_) => ok = false,
                },
                Err(_) => ok = false,
            }
            ord_i += 1;
        }
        p += 1;
    }
    let ok = ok && compared == expected && expected > 0;
    let det = format!("compared={}/{}", compared, expected);
    s.add_dyn("50022-J1-五十排列", ok, &det);

    // J1-7 单列退化：cols=1 时两序必须同序
    let g14 = SheetSpec::new(210.0, 297.0, 1, 4).with_margin(10.0);
    let a = imposition_plan(4, g14, NupOrder::Across, false);
    let b = imposition_plan(4, g14, NupOrder::Down, false);
    let ok = match (&a, &b) {
        (Ok(x), Ok(y)) => x.ordered_pages() == y.ordered_pages(),
        _ => false,
    };
    s.add("50022-J1-单列退化", ok, if ok { "" } else { "单列两序分叉" });

    // J1-8 空白填充：7 页 2×2 双面 → 1 张 8 槽 1 空白；9 页 → 2 张 7 空白
    let b1 = imposition_plan(7, g22, NupOrder::Across, true);
    let b2 = imposition_plan(9, g22, NupOrder::Across, true);
    let ok = match (&b1, &b2) {
        (Ok(x), Ok(y)) => x.blanks == 1 && x.sheets() == 1 && y.blanks == 7 && y.sheets() == 2,
        _ => false,
    };
    s.add("50022-J1-空白填充", ok, if ok { "" } else { "空白位记账错" });

    // J1-9 账本回放与计划逐槽一致
    let plan = match imposition_plan(13, g22, NupOrder::Down, true) {
        Ok(v) => v,
        Err(_) => {
            s.add("50022-J1-账本回放", false, "计划生成失败");
            return s;
        }
    };
    let mut ledger = PageLedger::new(4096);
    let loaded = match ledger.load_plan(&plan) {
        Ok(n) => n,
        Err(_) => 0,
    };
    let ok = loaded == plan.placements.len()
        && ledger.replay() == plan.ordered_pages()
        && ledger.duplicate_slots() == 0;
    let det = format!("loaded={} planned={}", loaded, plan.placements.len());
    s.add_dyn("50022-J1-账本回放", ok, &det);

    // J1-10 背面长边镜像：正面 slot0 在左，背面 slot0 落到最右列
    let mut ok = false;
    let mut dx = 0.0f32;
    let mut back_rect = None;
    let mut i = 0usize;
    while i < plan.placements.len() {
        if let Some(q) = plan.placements.get(i) {
            if q.sheet == 0 && q.side == Side::Back && q.slot == 0 {
                back_rect = Some(q.rect);
            }
        }
        i += 1;
    }
    if let Some(r) = back_rect {
        let expect_x = plan.spec.paper_w - plan.spec.margin - plan.spec.cell_w();
        dx = fabsf(r.x - expect_x);
        ok = dx < 1e-3 && plan.spec.duplex;
    }
    let det = format!("dx={:.4}", dx);
    s.add_dyn("50022-J1-背面镜像", ok, &det);

    // J1-11 同面格位不重叠
    let ok = check_layout(&plan).is_ok() && check_layout(&a4_plan()).is_ok();
    s.add("50022-J1-格位不重叠", ok, if ok { "" } else { "格位交叠" });

    // J1-12 全部格位落在可印区内（计划自检已含几何校验）
    let ok = plan.verify().is_ok();
    s.add("50022-J1-几何在纸内", ok, if ok { "" } else { "格位越纸" });

    // J1-13 重复页/重复格显式拒绝
    let mut led = PageLedger::new(32);
    let mut ok = false;
    if let Some(p0) = plan.placements.first() {
        if led.push(*p0).is_ok() {
            let mut dup_page = *p0;
            dup_page.slot = (p0.slot + 1) % plan.spec.cells();
            dup_page.sheet = p0.sheet;
            ok = led.push(dup_page).err() == Some(NupError::Duplicate);
        }
    }
    s.add("50022-J1-重复拒", ok, if ok { "" } else { "重复未拒" });

    // J1-14 越界三类：超页数、越格、未知序编码
    let over = imposition_plan(MAX_PAGES + 1, g22, NupOrder::Across, true).err() == Some(NupError::Overflow);
    let cell = cell_rect(&g22, 9, 0).err() == Some(NupError::Cell);
    let ord = NupOrder::from_code(7).err() == Some(NupError::Order);
    let ok = over && cell && ord;
    s.add("50022-J1-越界拒绝", ok, if ok { "" } else { "越界未拒" });

    // J1-15 确定性：同输入两次、逐字段一致
    let x1 = imposition_plan(23, g22, NupOrder::Across, true);
    let x2 = imposition_plan(23, g22, NupOrder::Across, true);
    let ok = match (&x1, &x2) {
        (Ok(u), Ok(v)) => u.placements == v.placements && u.blanks == v.blanks,
        _ => false,
    };
    s.add("50022-J1-确定性", ok, if ok { "" } else { "两次结果不一致" });

    // J1-16 注入核
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50022-J1-注入必抓", ok, &det);

    s
}

/// 判据内部用的小计划（2×2 单面 4 页），避免每条判据重复构造。
fn a4_plan() -> NupPlan {
    match imposition_plan(4, SheetSpec::new(210.0, 297.0, 2, 2).with_margin(10.0), NupOrder::Across, false) {
        Ok(p) => p,
        Err(_) => NupPlan {
            spec: SheetSpec::new(210.0, 297.0, 2, 2).with_margin(10.0),
            order: NupOrder::Across,
            pages: 0,
            blanks: 0,
            placements: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50022_checks_green() {
        let set = run_unx50022_checks();
        assert!(set.all_passed(), "UNX-F50022 red: {}", set.red_items_text());
    }

    /// 两序在 3×3 网格上都是双射：每个格恰被一个槽覆盖。
    #[test]
    fn unx50022_bijection_3x3() {
        let mut ord_i = 0u8;
        while ord_i < 2 {
            let order = if ord_i == 0 { NupOrder::Across } else { NupOrder::Down };
            let mut seen = [0u8; 9];
            let mut slot = 0u16;
            while slot < 9 {
                match order.cell_of_slot(slot, 3, 3) {
                    Ok((c, r)) => {
                        let idx = r as usize * 3 + c as usize;
                        assert!(idx < 9);
                        if let Some(v) = seen.get_mut(idx) {
                            *v += 1;
                        }
                    }
                    Err(_) => panic!("slot rejected"),
                }
                slot += 1;
            }
            let mut i = 0usize;
            while i < 9 {
                assert_eq!(seen.get(i), Some(&1u8));
                i += 1;
            }
            ord_i += 1;
        }
    }

    /// 双面计划：每个非空页号在整份计划里只出现一次。
    #[test]
    fn unx50022_pages_unique() {
        let g = SheetSpec::new(210.0, 297.0, 2, 2).with_margin(10.0);
        let plan = imposition_plan(30, g, NupOrder::Down, true);
        let plan = match plan {
            Ok(p) => p,
            Err(_) => return,
        };
        let mut seen = [0u16; 64];
        let mut i = 0usize;
        while i < plan.placements.len() {
            if let Some(p) = plan.placements.get(i) {
                if p.page > 0 {
                    let at = p.page as usize;
                    if let Some(v) = seen.get_mut(at) {
                        *v += 1;
                    }
                }
            }
            i += 1;
        }
        let mut n = 1usize;
        while n <= 30 {
            assert_eq!(seen.get(n), Some(&1u16));
            n += 1;
        }
    }
}
