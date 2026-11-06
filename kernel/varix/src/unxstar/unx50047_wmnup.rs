//! UNX-F50047 水印与 N-up 组合（每子页独立 / 整页共享两档）（0x43200 段）
//!
//! 锚点：UNX-F50047-J1 组合 ×2 档 ×10 页序一致；判据锚定 §AI-63 §1 防重声明（B22 联签）。
//!
//! 本模块把「N-up 拼版」与「水印落位」拆成两层：拼版只回答「第几个子页槽落在
//! 纸面的哪块矩形」，水印只回答「印章在这个槽里怎么摆」。两档语义：
//!
//! * `PerSubpage`（每子页独立）：每个子页各有一枚水印，印章按子页槽做等比
//!   收缩并居中，槽与槽之间互不影响——这是「每份都盖同一枚章」的语义；
//! * `SheetShared`（整页共享）：整张纸只定义**一枚**纸面坐标的水印，落到子页
//!   时取「印章矩形 ∩ 子页槽」的可见片段——跨槽的笔画被槽边裁开，视觉上是
//!   一整枚大章被印张切分。
//!
//! 两档必须共用同一套「页 → (张, 槽)」映射：档位只改印章几何与裁切，不许改
//! 拼版结果。J1 的「组合一致」就落在这条不变量上（10 种页序 × 2 档 = 20 组）。
//!
//! 数据段 `SEG_BASE = 0x43200`，诊断码闭集 `0x43201..=0x43209`（9 条，全部
//! 由真实构造点产生）。防重声明：同一页在页序里只许出现一次、同一 (页,张,槽)
//! 只许入账一次，重复即 `Duplicate`，台账不静默吞。
//!
//! 结构清单：`NupGrid`（拼版几何）、`WmStamp`（印章参数）、`Rect`（毫点矩形）、
//! `Placement`/`DocumentPlan`（落位账目）、`NupLedger`（防重台账）、
//! `WmMode`（档位闭集）、`page_orders`（10 种页序发生器）、`injection_core`
//! （10 类坏样本注入核）。
//!
//! 全部坐标用**毫点**（1/1000 pt）整数运算：确定性可复算，避免浮点在
//! no_std 目标上的舍入漂移。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x43200）。
pub const SEG_BASE: u32 = 0x43200;

pub const E_EMPTY: u16 = 1;
pub const E_PPS: u16 = 2;
pub const E_SHEET: u16 = 3;
pub const E_CELL: u16 = 4;
pub const E_DUP: u16 = 5;
pub const E_CAP: u16 = 6;
pub const E_GEOM: u16 = 7;
pub const E_PARAM: u16 = 8;
pub const E_MODE: u16 = 9;

/// N-up 水印组合域错误。`code()` 落在 `SEG_BASE + n` 闭集内。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WmNupError {
    /// 页序为空——空文档没有可拼的东西，不返回空计划糊弄调用方
    Empty,
    /// 每张子页数不在受支持集合 {1,2,4,6,9,16} 内
    PagesPerSheet,
    /// 页序长度换算出的张数超过 u16
    SheetIndex,
    /// 子页槽下标越界
    CellIndex,
    /// 同一页在页序中重复，或同一 (页,张,槽) 重复入账（§AI-63 §1 防重）
    Duplicate,
    /// 台账容量满
    Capacity,
    /// 可印区退化（边距/槽距把纸吃光），或印章收缩后小于 1 毫点
    Geometry,
    /// 参数越界（纸面尺寸、边距、印章宽高/不透明度/角度）
    Param,
    /// 档位码非法
    Mode,
}

impl WmNupError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            WmNupError::Empty => E_EMPTY,
            WmNupError::PagesPerSheet => E_PPS,
            WmNupError::SheetIndex => E_SHEET,
            WmNupError::CellIndex => E_CELL,
            WmNupError::Duplicate => E_DUP,
            WmNupError::Capacity => E_CAP,
            WmNupError::Geometry => E_GEOM,
            WmNupError::Param => E_PARAM,
            WmNupError::Mode => E_MODE,
        };
        SEG_BASE + n as u32
    }

    /// 一句话诊断说明（供日志/上层提示复用）。
    pub fn message(self) -> &'static str {
        match self {
            WmNupError::Empty => "page order empty",
            WmNupError::PagesPerSheet => "pages-per-sheet unsupported",
            WmNupError::SheetIndex => "sheet index overflows u16",
            WmNupError::CellIndex => "subpage cell out of range",
            WmNupError::Duplicate => "duplicate page or slot",
            WmNupError::Capacity => "ledger capacity exhausted",
            WmNupError::Geometry => "printable area degenerate",
            WmNupError::Param => "stamp/grid parameter out of range",
            WmNupError::Mode => "watermark mode code invalid",
        }
    }
}

/// 毫点矩形（1 毫点 = 1/1000 pt）。左上角 + 宽高，全整数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    /// 右边界（不含）。
    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    /// 下边界（不含）。
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// 面积（i64 防溢出；退化矩形面积为 0）。
    pub fn area(&self) -> i64 {
        if self.w <= 0 || self.h <= 0 {
            0
        } else {
            self.w as i64 * self.h as i64
        }
    }

    /// 是否完全包含另一矩形（边界相接允许）。
    pub fn contains(&self, o: &Rect) -> bool {
        o.x >= self.x && o.y >= self.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }

    /// 交集；只有交面积为正时才返回 `Some`（相切不算相交）。
    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let x0 = if self.x > o.x { self.x } else { o.x };
        let y0 = if self.y > o.y { self.y } else { o.y };
        let x1 = if self.right() < o.right() { self.right() } else { o.right() };
        let y1 = if self.bottom() < o.bottom() { self.bottom() } else { o.bottom() };
        if x1 > x0 && y1 > y0 {
            Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
        } else {
            None
        }
    }
}

/// 印章与槽边的安全内缩（4 pt）。留白是水印不压正文的底线。
pub const INSET: i32 = 4000;

/// 受支持的每张子页数（驱动行/列拆分表）。
pub const SUPPORTED_PPS: [u8; 6] = [1, 2, 4, 6, 9, 16];

/// (pps, rows, cols)：行优先铺槽，列数优先保证横向可读。
const GRID_TABLE: [(u8, u8, u8); 6] = [
    (1, 1, 1),
    (2, 1, 2),
    (4, 2, 2),
    (6, 2, 3),
    (9, 3, 3),
    (16, 4, 4),
];

/// 拼版几何：把整张纸按 行×列 切成子页槽，槽间留 `gutter`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NupGrid {
    pub pps: u8,
    pub rows: u8,
    pub cols: u8,
    pub sheet: Rect,
    pub margin: i32,
    pub gutter: i32,
}

impl NupGrid {
    /// 构造拼版几何；非法 pps / 负边距 / 退化可印区一律显性拒绝。
    pub fn new(
        pps: u8,
        sheet_w: i32,
        sheet_h: i32,
        margin: i32,
        gutter: i32,
    ) -> Result<NupGrid, WmNupError> {
        if sheet_w <= 0 || sheet_h <= 0 || margin < 0 || gutter < 0 {
            return Err(WmNupError::Param);
        }
        let mut picked: Option<(u8, u8)> = None;
        let mut i = 0usize;
        while i < GRID_TABLE.len() {
            if let Some(e) = GRID_TABLE.get(i) {
                if e.0 == pps {
                    picked = Some((e.1, e.2));
                }
            }
            i += 1;
        }
        let (rows, cols) = match picked {
            Some(v) => v,
            None => return Err(WmNupError::PagesPerSheet),
        };
        let inner_w = sheet_w - 2 * margin;
        let inner_h = sheet_h - 2 * margin;
        if inner_w <= 0 || inner_h <= 0 {
            return Err(WmNupError::Geometry);
        }
        // 槽距只吃掉 (cols-1) 份；余量必须还能给每槽至少 2×INSET 的可用宽度，
        // 否则水印无处落，构造期就拒（而不是等到落位时才失败）。
        let cw = (inner_w - gutter * (cols as i32 - 1)) / cols as i32;
        let ch = (inner_h - gutter * (rows as i32 - 1)) / rows as i32;
        if cw <= 2 * INSET || ch <= 2 * INSET {
            return Err(WmNupError::Geometry);
        }
        Ok(NupGrid {
            pps,
            rows,
            cols,
            sheet: Rect::new(0, 0, sheet_w, sheet_h),
            margin,
            gutter,
        })
    }

    /// 槽内可印矩形；`cell` 必须 < pps。列宽整数除法的余数留在右边距里，
    /// 因此最右一列的右边距可能比名义值略大（不越界，只多留白）。
    pub fn cell_rect(&self, cell: u8) -> Result<Rect, WmNupError> {
        if cell >= self.pps {
            return Err(WmNupError::CellIndex);
        }
        let cols = self.cols as i32;
        let inner_w = self.sheet.w - 2 * self.margin;
        let inner_h = self.sheet.h - 2 * self.margin;
        let cw = (inner_w - self.gutter * (cols - 1)) / cols;
        let ch = (inner_h - self.gutter * (self.rows as i32 - 1)) / self.rows as i32;
        let col = cell as i32 % cols;
        let row = cell as i32 / cols;
        let x = self.margin + col * (cw + self.gutter);
        let y = self.margin + row * (ch + self.gutter);
        Ok(Rect::new(x, y, cw, ch))
    }

    /// 槽数（= pps）。
    pub fn cell_count(&self) -> u8 {
        self.pps
    }
}

/// 把页序下标换算成张号；超出 u16 即拒，避免静默回绕。
pub fn sheet_of(index: usize, pps: u8) -> Result<u16, WmNupError> {
    if pps == 0 {
        return Err(WmNupError::PagesPerSheet);
    }
    let q = index / pps as usize;
    if q > u16::MAX as usize {
        return Err(WmNupError::SheetIndex);
    }
    Ok(q as u16)
}

/// 水印印章参数（毫点尺寸 + 角度 + 不透明度千分比）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WmStamp {
    pub w: i32,
    pub h: i32,
    pub angle: i16,
    pub opacity: u16,
}

impl WmStamp {
    /// 构造印章；宽高必须为正、角度在 [-180,180]、不透明度 ≤1000‰。
    pub fn new(w: i32, h: i32, angle: i16, opacity: u16) -> Result<WmStamp, WmNupError> {
        if w <= 0 || h <= 0 || opacity > 1000 || angle < -180 || angle > 180 {
            return Err(WmNupError::Param);
        }
        Ok(WmStamp { w, h, angle, opacity })
    }
}

/// 两档落位语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WmMode {
    /// 每子页独立：印章按槽收缩居中，与相邻槽无关
    PerSubpage,
    /// 整页共享：一枚纸面水印被槽边裁切
    SheetShared,
}

impl WmMode {
    /// 档位码闭集：0=独立，1=共享，其余拒绝。
    pub fn from_u8(v: u8) -> Result<WmMode, WmNupError> {
        match v {
            0 => Ok(WmMode::PerSubpage),
            1 => Ok(WmMode::SheetShared),
            _ => Err(WmNupError::Mode),
        }
    }

    /// 档位码。
    pub fn as_u8(self) -> u8 {
        match self {
            WmMode::PerSubpage => 0,
            WmMode::SheetShared => 1,
        }
    }
}

/// 一枚水印的落位记录。`stamp` 是印章本来的纸面矩形，`visible` 是实际可印
/// 部分（独立档两者相等）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub page: u16,
    pub sheet: u16,
    pub cell: u8,
    pub stamp: Rect,
    pub visible: Rect,
    pub opacity: u16,
    pub angle: i16,
    pub shared: bool,
}

/// 一页文档的拼版 + 水印计划。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentPlan {
    pub placements: Vec<Placement>,
    pub clipped: usize,
    pub sheets: u16,
}

const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_step(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(FNV_PRIME)
}

impl DocumentPlan {
    /// 计划指纹：两次同输入必须逐位一致（判据用它做确定性对平）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_step(h, self.sheets as u64);
        h = fnv_step(h, self.clipped as u64);
        let mut i = 0usize;
        while i < self.placements.len() {
            if let Some(p) = self.placements.get(i) {
                h = fnv_step(h, p.page as u64);
                h = fnv_step(h, p.sheet as u64);
                h = fnv_step(h, p.cell as u64);
                h = fnv_step(h, p.stamp.x as u32 as u64);
                h = fnv_step(h, p.stamp.y as u32 as u64);
                h = fnv_step(h, p.stamp.w as u32 as u64);
                h = fnv_step(h, p.stamp.h as u32 as u64);
                h = fnv_step(h, p.visible.x as u32 as u64);
                h = fnv_step(h, p.visible.y as u32 as u64);
                h = fnv_step(h, p.visible.w as u32 as u64);
                h = fnv_step(h, p.visible.h as u32 as u64);
                h = fnv_step(h, p.opacity as u64);
                h = fnv_step(h, p.angle as u16 as u64);
            }
            i += 1;
        }
        h
    }

    /// 计划摘要（调试用，不含堆布局依赖）。
    pub fn summary(&self) -> String {
        format!(
            "sheets={} placements={} clipped={}",
            self.sheets,
            self.placements.len(),
            self.clipped
        )
    }
}

/// 印章等比收缩进槽（保留长宽比），再在槽内居中。收缩比用千分整数，
/// 避免任何浮点：同一输入两次结果的矩形逐位相同。
fn fit_into(stamp: &WmStamp, cell: Rect, inset: i32) -> Result<Rect, WmNupError> {
    if stamp.w <= 0 || stamp.h <= 0 || inset < 0 {
        return Err(WmNupError::Param);
    }
    let max_w = cell.w - 2 * inset;
    let max_h = cell.h - 2 * inset;
    if max_w <= 0 || max_h <= 0 {
        return Err(WmNupError::Geometry);
    }
    let mut w = stamp.w;
    let mut h = stamp.h;
    if w > max_w || h > max_h {
        let sx = max_w as i64 * 1000 / stamp.w as i64;
        let sy = max_h as i64 * 1000 / stamp.h as i64;
        let s = if sx < sy { sx } else { sy };
        w = (stamp.w as i64 * s / 1000) as i32;
        h = (stamp.h as i64 * s / 1000) as i32;
        if w < 1 {
            w = 1;
        }
        if h < 1 {
            h = 1;
        }
    }
    let x = cell.x + (cell.w - w) / 2;
    let y = cell.y + (cell.h - h) / 2;
    Ok(Rect::new(x, y, w, h))
}

/// 整页共享档的纸面印章矩形：以纸面中心为锚。
fn sheet_space_stamp(grid: &NupGrid, stamp: &WmStamp) -> Result<Rect, WmNupError> {
    if stamp.w <= 0 || stamp.h <= 0 {
        return Err(WmNupError::Param);
    }
    let x = grid.sheet.x + (grid.sheet.w - stamp.w) / 2;
    let y = grid.sheet.y + (grid.sheet.h - stamp.h) / 2;
    Ok(Rect::new(x, y, stamp.w, stamp.h))
}

/// 按页序生成整份计划。`pages[i]` 是第 i 个拼版位置上要印的页号（1 基语义由
/// 调用方决定，模块只做位置计算）；页号在页序中重复即 `Duplicate`。
pub fn plan_document(
    grid: &NupGrid,
    pages: &[u16],
    mode: WmMode,
    stamp: &WmStamp,
) -> Result<DocumentPlan, WmNupError> {
    if pages.is_empty() {
        return Err(WmNupError::Empty);
    }
    let pps = grid.pps as usize;
    if pps == 0 {
        return Err(WmNupError::PagesPerSheet);
    }
    let sheet_count = (pages.len() + pps - 1) / pps;
    if sheet_count > u16::MAX as usize {
        return Err(WmNupError::SheetIndex);
    }
    let shell = sheet_space_stamp(grid, stamp)?;
    let mut placements: Vec<Placement> = Vec::with_capacity(pages.len());
    let mut seen: Vec<u16> = Vec::with_capacity(pages.len());
    let mut clipped = 0usize;
    let mut i = 0usize;
    while i < pages.len() {
        let page = match pages.get(i) {
            Some(v) => *v,
            None => return Err(WmNupError::Empty),
        };
        if seen.contains(&page) {
            return Err(WmNupError::Duplicate);
        }
        seen.push(page);
        let sheet = sheet_of(i, grid.pps)?;
        let cell = (i % pps) as u8;
        let rect = grid.cell_rect(cell)?;
        match mode {
            WmMode::PerSubpage => {
                let fitted = fit_into(stamp, rect, INSET)?;
                placements.push(Placement {
                    page,
                    sheet,
                    cell,
                    stamp: fitted,
                    visible: fitted,
                    opacity: stamp.opacity,
                    angle: stamp.angle,
                    shared: false,
                });
            }
            WmMode::SheetShared => match shell.intersect(&rect) {
                Some(v) => placements.push(Placement {
                    page,
                    sheet,
                    cell,
                    stamp: shell,
                    visible: v,
                    opacity: stamp.opacity,
                    angle: stamp.angle,
                    shared: true,
                }),
                None => clipped += 1,
            },
        }
        i += 1;
    }
    Ok(DocumentPlan {
        placements,
        clipped,
        sheets: sheet_count as u16,
    })
}

/// 防重台账容量（32 条覆盖常见 8 页 × 4-up 的 2 张纸）。
pub const LEDGER_CAP: usize = 32;

/// 落位台账：按 (页, 张, 槽) 去重，重复登记显性拒并计数。
pub struct NupLedger {
    entries: [Option<Placement>; LEDGER_CAP],
    len: usize,
    rejected: u16,
    digest: u64,
}

impl NupLedger {
    /// 空台账。
    pub const fn new() -> NupLedger {
        NupLedger {
            entries: [None; LEDGER_CAP],
            len: 0,
            rejected: 0,
            digest: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// 入账一条落位；同 (页,张,槽) 重复 → `Duplicate`，容量满 → `Capacity`。
    pub fn record(&mut self, p: Placement) -> Result<(), WmNupError> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.entries.get(i).and_then(|v| *v) {
                if e.page == p.page && e.sheet == p.sheet && e.cell == p.cell {
                    self.rejected += 1;
                    return Err(WmNupError::Duplicate);
                }
            }
            i += 1;
        }
        if self.len >= LEDGER_CAP {
            self.rejected += 1;
            return Err(WmNupError::Capacity);
        }
        if let Some(slot) = self.entries.get_mut(self.len) {
            *slot = Some(p);
            self.len += 1;
        }
        self.digest = fnv_step(self.digest, p.page as u64);
        self.digest = fnv_step(self.digest, p.cell as u64);
        Ok(())
    }

    /// 已入账条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 被拒条数（重复 + 容量）。
    pub fn rejected(&self) -> u16 {
        self.rejected
    }

    /// 台账指纹（入账序敏感）。
    pub fn digest(&self) -> u64 {
        self.digest
    }
}

/// 10 种页序发生器：正序 / 逆序 / 偶先 / 奇先 / 小册子 / 逆小册子 /
/// 轮转 3 / 轮转 5 / LCG 洗牌两种子。种子写死，保证跨机器复算一致。
pub fn page_orders(n: u16) -> Vec<Vec<u16>> {
    let mut out: Vec<Vec<u16>> = Vec::new();
    if n == 0 {
        return out;
    }
    let mut base: Vec<u16> = Vec::with_capacity(n as usize);
    let mut i = 0u16;
    while i < n {
        base.push(i);
        i += 1;
    }
    out.push(base.clone());
    let mut rev = Vec::with_capacity(base.len());
    let mut k = base.len();
    while k > 0 {
        k -= 1;
        if let Some(v) = base.get(k) {
            rev.push(*v);
        }
    }
    out.push(rev);
    out.push(even_odd(&base, true));
    out.push(even_odd(&base, false));
    let book = booklet(&base);
    out.push(book.clone());
    let mut rbook = Vec::with_capacity(book.len());
    let mut j = book.len();
    while j > 0 {
        j -= 1;
        if let Some(v) = book.get(j) {
            rbook.push(*v);
        }
    }
    out.push(rbook);
    out.push(rotate(&base, 3));
    out.push(rotate(&base, 5));
    out.push(lcg_shuffle(&base, 0x43200));
    out.push(lcg_shuffle(&base, 0x4320b));
    out
}

fn even_odd(base: &[u16], even_first: bool) -> Vec<u16> {
    let mut out = Vec::with_capacity(base.len());
    let mut pass = 0u8;
    while pass < 2 {
        let want_even = if even_first { pass == 0 } else { pass == 1 };
        let mut i = 0usize;
        while i < base.len() {
            if let Some(v) = base.get(i) {
                let is_even = i % 2 == 0;
                if is_even == want_even {
                    out.push(*v);
                }
            }
            i += 1;
        }
        pass += 1;
    }
    out
}

fn booklet(base: &[u16]) -> Vec<u16> {
    let n = base.len();
    let mut out = Vec::with_capacity(n);
    let mut lo = 0usize;
    let mut hi = n;
    while lo < hi {
        hi -= 1;
        if let Some(v) = base.get(lo) {
            out.push(*v);
        }
        if lo < hi {
            if let Some(v) = base.get(hi) {
                out.push(*v);
            }
        }
        lo += 1;
    }
    out
}

fn rotate(base: &[u16], by: usize) -> Vec<u16> {
    let n = base.len();
    let mut out = Vec::with_capacity(n);
    if n == 0 {
        return out;
    }
    let mut i = 0usize;
    while i < n {
        let idx = (i + by) % n;
        if let Some(v) = base.get(idx) {
            out.push(*v);
        }
        i += 1;
    }
    out
}

fn lcg_shuffle(base: &[u16], seed: u32) -> Vec<u16> {
    let mut v: Vec<u16> = Vec::with_capacity(base.len());
    let mut i = 0usize;
    while i < base.len() {
        if let Some(x) = base.get(i) {
            v.push(*x);
        }
        i += 1;
    }
    let mut st = seed | 1;
    let mut k = v.len();
    while k > 1 {
        st = st.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let j = (st >> 8) as usize % k;
        k -= 1;
        v.swap(k, j);
    }
    v
}

/// 页 → (张, 槽) 映射指纹。两档必须给出同一指纹（档位不改拼版）。
pub fn slot_digest(plan: &DocumentPlan) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < plan.placements.len() {
        if let Some(p) = plan.placements.get(i) {
            h = fnv_step(h, p.page as u64);
            h = fnv_step(h, p.sheet as u64);
            h = fnv_step(h, p.cell as u64);
        }
        i += 1;
    }
    h
}

/// 整页共享档的裁切数对平：暴力遍历所有槽统计交面积为空的个数。
pub fn brute_clipped(grid: &NupGrid, pages: &[u16], stamp: &WmStamp) -> usize {
    let shell = match sheet_space_stamp(grid, stamp) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let mut n = 0usize;
    let mut i = 0usize;
    while i < pages.len() {
        let cell = (i % grid.pps as usize) as u8;
        match grid.cell_rect(cell) {
            Ok(r) => {
                if shell.intersect(&r).is_none() {
                    n += 1;
                }
            }
            Err(_) => n += 1,
        }
        i += 1;
    }
    n
}

/// J1 组合矩阵：10 页序 × 2 档，返回 (通过组数, 总组数) = (20, 20)。
///
/// 每种页序贡献两组（独立 / 共享各一组），每组要同时满足：① 页在计划中恰
/// 出现一次且落位条数等于页数；② 该档的落位语义自洽（独立档印章在槽内，
/// 共享档可见区 ⊆ 槽且同张共用一枚印章）；③ 跨档的 (页 → 张,槽) 指纹一致
/// ——档位只许改印章几何，不许改拼版。
pub fn j1_matrix_result() -> (usize, usize) {
    let grid = match NupGrid::new(4, 595_000, 842_000, 20_000, 6_000) {
        Ok(g) => g,
        Err(_) => return (0, 20),
    };
    // 印章取 260×120 pt：大于单槽可用区，必然触发等比收缩路径。
    let stamp = match WmStamp::new(260_000, 120_000, 45, 180) {
        Ok(s) => s,
        Err(_) => return (0, 20),
    };
    let orders = page_orders(10);
    let mut ok = 0usize;
    let mut total = 0usize;
    let mut oi = 0usize;
    while oi < orders.len() {
        let order = match orders.get(oi) {
            Some(v) => v,
            None => break,
        };
        total += 2;
        let ind = plan_document(&grid, order, WmMode::PerSubpage, &stamp);
        let shr = plan_document(&grid, order, WmMode::SheetShared, &stamp);
        let (a, b) = match (ind, shr) {
            (Ok(a), Ok(b)) => (a, b),
            _ => {
                oi += 1;
                continue;
            }
        };
        let ind_ok = covers_each_page_once(&a, 10)
            && a.placements.len() == order.len()
            && all_inside_cells(&grid, &a)
            && all_mode_flag(&a, false);
        let shr_ok = covers_each_page_once(&b, 10)
            && b.placements.len() == order.len()
            && all_visible_inside_cells(&grid, &b)
            && all_mode_flag(&b, true)
            && shared_stamp_uniform(&b);
        let same_slots = slot_digest(&a) == slot_digest(&b);
        if ind_ok {
            ok += 1;
        }
        if shr_ok && same_slots {
            ok += 1;
        }
        oi += 1;
    }
    (ok, total)
}

/// 共享档只要求**可见片段**落在槽内：印章本体本来就跨槽。
fn all_visible_inside_cells(grid: &NupGrid, plan: &DocumentPlan) -> bool {
    let mut i = 0usize;
    while i < plan.placements.len() {
        if let Some(p) = plan.placements.get(i) {
            let cell = match grid.cell_rect(p.cell) {
                Ok(r) => r,
                Err(_) => return false,
            };
            if !cell.contains(&p.visible) || !p.stamp.contains(&p.visible) {
                return false;
            }
        }
        i += 1;
    }
    true
}

fn all_mode_flag(plan: &DocumentPlan, shared: bool) -> bool {
    let mut i = 0usize;
    while i < plan.placements.len() {
        if let Some(p) = plan.placements.get(i) {
            if p.shared != shared {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 共享档：同一张纸上所有落位必须引用同一枚纸面印章矩形。
fn shared_stamp_uniform(plan: &DocumentPlan) -> bool {
    let mut i = 0usize;
    while i < plan.placements.len() {
        if let Some(p) = plan.placements.get(i) {
            let mut j = i + 1;
            while j < plan.placements.len() {
                if let Some(q) = plan.placements.get(j) {
                    if q.sheet == p.sheet && q.stamp != p.stamp {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    true
}

fn covers_each_page_once(plan: &DocumentPlan, n: u16) -> bool {
    let mut seen: Vec<u16> = Vec::with_capacity(plan.placements.len());
    let mut i = 0usize;
    while i < plan.placements.len() {
        if let Some(p) = plan.placements.get(i) {
            if p.page >= n || seen.contains(&p.page) {
                return false;
            }
            seen.push(p.page);
        }
        i += 1;
    }
    seen.len() == n as usize
}

fn all_inside_cells(grid: &NupGrid, plan: &DocumentPlan) -> bool {
    let mut i = 0usize;
    while i < plan.placements.len() {
        if let Some(p) = plan.placements.get(i) {
            let cell = match grid.cell_rect(p.cell) {
                Ok(r) => r,
                Err(_) => return false,
            };
            if !cell.contains(&p.visible) || !cell.contains(&p.stamp) {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 注入核：10 类坏样本必须逐类命中指定诊断码
// ---------------------------------------------------------------------------

/// 构造 10 类坏样本并逐类断言诊断码，返回 (命中数, 总数)。
pub fn injection_core() -> (usize, usize) {
    let total = 10usize;
    let mut caught = 0usize;

    let grid_ok = NupGrid::new(4, 595_000, 842_000, 20_000, 6_000);
    let stamp_ok = WmStamp::new(120_000, 60_000, 0, 200);

    // ① 空页序
    if let (Ok(g), Ok(st)) = (grid_ok, stamp_ok) {
        if plan_document(&g, &[], WmMode::PerSubpage, &st).err() == Some(WmNupError::Empty) {
            caught += 1;
        }
    }
    // ② pps 不在闭集
    if NupGrid::new(0, 595_000, 842_000, 20_000, 6_000).err() == Some(WmNupError::PagesPerSheet) {
        caught += 1;
    }
    if NupGrid::new(7, 595_000, 842_000, 20_000, 6_000).err() == Some(WmNupError::PagesPerSheet) {
        caught += 1;
    }
    // ③ 可印区退化（边距吃掉整张纸）
    if NupGrid::new(4, 595_000, 842_000, 300_000, 6_000).err() == Some(WmNupError::Geometry) {
        caught += 1;
    }
    // ④ 槽下标越界
    if let Ok(g) = grid_ok {
        if g.cell_rect(9).err() == Some(WmNupError::CellIndex) {
            caught += 1;
        }
    }
    // ⑤ 张号溢出
    if sheet_of(70_000, 1).err() == Some(WmNupError::SheetIndex) {
        caught += 1;
    }
    // ⑥ 页序重复（页 3 出现两次）
    if let (Ok(g), Ok(st)) = (grid_ok, stamp_ok) {
        let order = [1u16, 2, 3, 3];
        if plan_document(&g, &order, WmMode::PerSubpage, &st).err() == Some(WmNupError::Duplicate) {
            caught += 1;
        }
    }
    // ⑦ 印章参数越界
    if WmStamp::new(0, 10, 0, 100).err() == Some(WmNupError::Param) {
        caught += 1;
    }
    // ⑧ 档位码越界
    if WmMode::from_u8(9).err() == Some(WmNupError::Mode) {
        caught += 1;
    }
    // ⑨ 台账容量：32 容量灌 33 条
    if let Ok(g) = grid_ok {
        let mut led = NupLedger::new();
        let mut i = 0u16;
        let mut cap_hit = false;
        while i < 33 {
            let p = Placement {
                page: i,
                sheet: i / 8,
                cell: (i % 8) as u8,
                stamp: Rect::new(0, 0, 10, 10),
                visible: Rect::new(0, 0, 10, 10),
                opacity: 200,
                angle: 0,
                shared: false,
            };
            match led.record(p) {
                Err(WmNupError::Capacity) => {
                    cap_hit = true;
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        let _ = g;
        if cap_hit && led.len() == LEDGER_CAP && led.rejected() == 1 {
            caught += 1;
        }
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50047 的 14 条判据。
pub fn run_unx50047_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50047");
    let grid = match NupGrid::new(4, 595_000, 842_000, 20_000, 6_000) {
        Ok(g) => g,
        Err(_) => {
            s.add("50047-网格构造", false, "构造失败");
            return s;
        }
    };
    let stamp = match WmStamp::new(260_000, 120_000, 45, 180) {
        Ok(v) => v,
        Err(_) => {
            s.add("50047-印章构造", false, "参数被误拒");
            return s;
        }
    };

    // J1-1 槽数 = pps，且槽两两不相交
    let mut disjoint = true;
    let mut i = 0u8;
    while i < grid.pps {
        let a = match grid.cell_rect(i) {
            Ok(r) => r,
            Err(_) => Rect::new(0, 0, 0, 0),
        };
        let mut j = i + 1;
        while j < grid.pps {
            let b = match grid.cell_rect(j) {
                Ok(r) => r,
                Err(_) => Rect::new(0, 0, 0, 0),
            };
            if a.intersect(&b).is_some() {
                disjoint = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "50047-槽不相交",
        disjoint && grid.cell_count() == grid.pps,
        "槽重叠或槽数漂移",
    );

    // J1-2 1-up 槽 == 可印区
    let one_up_ok = match NupGrid::new(1, 595_000, 842_000, 20_000, 0) {
        Ok(g) => match g.cell_rect(0) {
            Ok(r) => r.x == 20_000 && r.y == 20_000 && r.w == 555_000 && r.h == 802_000,
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("50047-1up槽", one_up_ok, "1-up 槽未等于可印区");

    // J1-3 10 页序 × 2 档组合一致
    let (ok, total) = j1_matrix_result();
    let det = format!("{}/{}", ok, total);
    s.add_dyn("50047-J1-组合一致", ok == 20 && total == 20, &det);

    // J1-4 页序发生器给足 10 种且都是排列
    let orders = page_orders(10);
    let mut perm_ok = orders.len() == 10;
    let mut oi = 0usize;
    while oi < orders.len() {
        if let Some(o) = orders.get(oi) {
            let mut seen: Vec<u16> = Vec::with_capacity(10);
            let mut k = 0usize;
            while k < o.len() {
                if let Some(p) = o.get(k) {
                    if seen.contains(p) {
                        perm_ok = false;
                    }
                    seen.push(*p);
                }
                k += 1;
            }
            if seen.len() != 10 {
                perm_ok = false;
            }
        }
        oi += 1;
    }
    s.add("50047-页序排列", perm_ok, "页序不是 10 个排列");

    // J1-5 独立档：印章完全落在自己槽内
    let order10: Vec<u16> = match orders.get(0) {
        Some(v) => v.clone(),
        None => Vec::new(),
    };
    let ind = plan_document(&grid, &order10, WmMode::PerSubpage, &stamp);
    let ind_ok = match &ind {
        Ok(p) => all_inside_cells(&grid, p) && p.placements.len() == 10,
        Err(_) => false,
    };
    s.add("50047-独立槽内", ind_ok, "独立档印章越出槽");

    // J1-6 独立档：不透明度/角度原样透传
    let prop_ok = match &ind {
        Ok(p) => {
            let mut ok = true;
            let mut k = 0usize;
            while k < p.placements.len() {
                if let Some(e) = p.placements.get(k) {
                    if e.opacity != stamp.opacity || e.angle != stamp.angle || e.shared {
                        ok = false;
                    }
                }
                k += 1;
            }
            ok
        }
        Err(_) => false,
    };
    s.add("50047-透传属性", prop_ok, "档位属性被改写");

    // J1-7 共享档：可见区 ⊆ 印章 且 ⊆ 槽
    let shr = plan_document(&grid, &order10, WmMode::SheetShared, &stamp);
    let shr_ok = match &shr {
        Ok(p) => {
            let mut ok = true;
            let mut k = 0usize;
            while k < p.placements.len() {
                if let Some(e) = p.placements.get(k) {
                    if !e.stamp.contains(&e.visible) || !e.shared {
                        ok = false;
                    }
                    if let Ok(c) = grid.cell_rect(e.cell) {
                        if !c.contains(&e.visible) {
                            ok = false;
                        }
                    }
                }
                k += 1;
            }
            ok && !p.placements.is_empty()
        }
        Err(_) => false,
    };
    s.add("50047-共享裁切", shr_ok, "共享档可见区越界");

    // J1-8 共享档：同张内所有落位共用同一枚纸面印章
    let mut shared_same = true;
    if let Ok(p) = &shr {
        let mut base: Option<Rect> = None;
        let mut k = 0usize;
        while k < p.placements.len() {
            if let Some(e) = p.placements.get(k) {
                match base {
                    None => base = Some(e.stamp),
                    Some(b) => {
                        if b != e.stamp {
                            shared_same = false;
                        }
                    }
                }
            }
            k += 1;
        }
    } else {
        shared_same = false;
    }
    s.add("50047-共享同章", shared_same, "同张出现多枚印章");

    // J1-9 共享档裁切数与暴力统计对平（含真实发生裁切的样本）
    let small = match WmStamp::new(30_000, 20_000, 0, 120) {
        Ok(v) => v,
        Err(_) => stamp,
    };
    let clip_ok = match plan_document(&grid, &order10, WmMode::SheetShared, &small) {
        Ok(p) => {
            let brute = brute_clipped(&grid, &order10, &small);
            let mut any_cut = false;
            let mut k = 0usize;
            while k < p.placements.len() {
                if let Some(e) = p.placements.get(k) {
                    if e.visible.area() < e.stamp.area() {
                        any_cut = true;
                    }
                }
                k += 1;
            }
            p.clipped == brute && any_cut
        }
        Err(_) => false,
    };
    let det = match plan_document(&grid, &order10, WmMode::SheetShared, &small) {
        Ok(p) => format!("clipped={} expect={}", p.clipped, brute_clipped(&grid, &order10, &small)),
        Err(_) => String::from("plan failed"),
    };
    s.add_dyn("50047-裁切对平", clip_ok, &det);

    // J1-10 确定性：两次计划指纹一致
    let d1 = match plan_document(&grid, &order10, WmMode::PerSubpage, &stamp) {
        Ok(p) => p.digest(),
        Err(_) => 0,
    };
    let d2 = match plan_document(&grid, &order10, WmMode::PerSubpage, &stamp) {
        Ok(p) => p.digest(),
        Err(_) => 1,
    };
    s.add("50047-确定性", d1 == d2 && d1 != 0, "两次计划不一致");

    // J1-11 台账去重：同一落位重复登记必拒，计数不涨
    let dedup_ok = match &ind {
        Ok(p) => match p.placements.get(0) {
            Some(first) => {
                let mut led = NupLedger::new();
                let r1 = led.record(*first);
                let before = led.len();
                let r2 = led.record(*first);
                r1.is_ok()
                    && r2.err() == Some(WmNupError::Duplicate)
                    && led.len() == before
                    && led.rejected() == 1
            }
            None => false,
        },
        Err(_) => false,
    };
    s.add("50047-台账去重", dedup_ok, "重复落位未被拒");

    // J1-12 台账容量：第 33 条显性拒
    let mut led = NupLedger::new();
    let mut cap_ok = true;
    let mut k = 0u16;
    while k < LEDGER_CAP as u16 {
        let p = Placement {
            page: k,
            sheet: 0,
            cell: (k % 4) as u8,
            stamp: Rect::new(0, 0, 5, 5),
            visible: Rect::new(0, 0, 5, 5),
            opacity: 100,
            angle: 0,
            shared: false,
        };
        if led.record(p).is_err() {
            cap_ok = false;
        }
        k += 1;
    }
    let overflow = Placement {
        page: 900,
        sheet: 0,
        cell: 0,
        stamp: Rect::new(0, 0, 5, 5),
        visible: Rect::new(0, 0, 5, 5),
        opacity: 100,
        angle: 0,
        shared: false,
    };
    cap_ok = cap_ok && led.record(overflow).err() == Some(WmNupError::Capacity);
    let det = format!("len={} rej={}", led.len(), led.rejected());
    s.add_dyn("50047-台账容量", cap_ok, &det);

    // J1-13 印章参数逐项拒
    let params_ok = WmStamp::new(0, 10, 0, 100).err() == Some(WmNupError::Param)
        && WmStamp::new(10, -1, 0, 100).err() == Some(WmNupError::Param)
        && WmStamp::new(10, 10, 0, 1001).err() == Some(WmNupError::Param)
        && WmStamp::new(10, 10, 181, 100).err() == Some(WmNupError::Param);
    s.add("50047-印章参数", params_ok, "越界参数未被拒");

    // J1-14 注入核 10/10
    let (caught, total) = injection_core();
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50047-注入必抓", caught == total, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50047_checks_green() {
        let set = run_unx50047_checks();
        assert!(set.all_passed(), "UNX-F50047 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50047_slot_map_is_order_independent_of_mode() {
        let grid = match NupGrid::new(4, 595_000, 842_000, 20_000, 6_000) {
            Ok(g) => g,
            Err(_) => return,
        };
        let stamp = match WmStamp::new(200_000, 80_000, 30, 250) {
            Ok(s) => s,
            Err(_) => return,
        };
        let order: Vec<u16> = (0..10u16).collect();
        let a = plan_document(&grid, &order, WmMode::PerSubpage, &stamp);
        let b = plan_document(&grid, &order, WmMode::SheetShared, &stamp);
        match (a, b) {
            (Ok(a), Ok(b)) => assert_eq!(slot_digest(&a), slot_digest(&b)),
            _ => assert!(false, "plan failed"),
        }
    }

    #[test]
    fn unx50047_ten_orders_two_modes() {
        let (ok, total) = j1_matrix_result();
        assert_eq!(ok, 20);
        assert_eq!(total, 20);
    }
}
