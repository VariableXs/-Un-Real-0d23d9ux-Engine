//! UNX-F50023 小册子排版（对折骑马钉序：奇偶面镜像排布）（0x42f00 段）
//!
//! 锚点：UNX-F50023-J1 16 页册序逐页比对一致；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段：`SEG_BASE = 0x42f00`。诊断码是闭集 `0x42f01..=0x42f0a`
//! （`BookletError` 十档，`code() = SEG_BASE + n`）。
//!
//! 结构清单
//! * 数据结构：`BookletSpec`（页数/纸面/边距/爬移）、`Rect`（半版矩形）、
//!   `Slot`（半版页位）、`Spread`（一张纸一面的左右页），`Sheet`（一张纸
//!   的正反两面 + 脊线 + 爬移量），`Booklet`（整册），`StitchLedger`（骑马钉
//!   投递账本）；
//! * 求解器：`page_pair`（骑马钉页对）、`impose`（整册求解）、
//!   `creep_shift`（折缝爬移）、`check_geometry`（脊线/包含/不交叠自检）；
//! * 错误路径：`BookletError` 十档；构造期一次校验，求解期逐项返回；
//! * 注入核：`injection_core` 六类坏样本（空册/超上限/竖纸/边距吃穿/
//!   重复投递/乱序投递）。
//!
//! 骑马钉为什么是「页对」
//! ----------------------
//! 一张纸对折后形成 4 个版位；把整册页序按「首尾配对」切开：
//! `pair(k) = (2k+1, padded-2k)`，k 从 0 起。一张纸的正面印 `pair(2i)`、
//! 背面印 `pair(2i+1)`，装订后从中间折起，页序自然连续。
//! 由此得到本模块反复使用的两条恒等式（J1 的比对口径）：
//! * 同一面 `left + right == padded + 1`；
//! * 背面左页 = 正面右页 + 1，背面右页 = 正面左页 - 1（镜像排布）。
//! 这两条不是"实现巧合"——它们是页对定义的直接推论，任何一面写反都会
//! 在恒等式上立刻现形。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::fabsf;

/// 诊断段基址（0x42f00 起 0x20 归本模块）。
pub const SEG_BASE: u32 = 0x42f00;

/// 骑马钉页数上限：256 页 = 64 张纸。超过 64 张后书脊厚度会把内页顶出，
/// 物理上必须改用胶装，本模块不做「假装还能钉」的降级。
pub const MAX_BOOKLET_PAGES: u16 = 256;

/// 纸面边长合法区间（mm）。40mm 以下对折后单版 < 20mm，放不下页眉页脚。
pub const MIN_PAPER_MM: f32 = 40.0;
/// 纸面边长上界（mm），见 `MIN_PAPER_MM`。
pub const MAX_PAPER_MM: f32 = 2000.0;

/// 折缝安全区（mm，单侧）。装订机折刀压痕宽约 1.5mm，加 0.5mm 余量取 2.0mm；
/// 左右版的内容边界必须各自退让这么多，否则折后露出压痕。
pub const FOLD_CLEARANCE: f32 = 2.0;

/// 爬移上限（mm，每册累计）。经验值：80g 纸 16 页册爬移约 0.6~1.0mm，
/// 3mm 已覆盖 64 页厚册，再多说明纸厚参数给错了。
pub const MAX_CREEP_MM: f32 = 3.0;

pub const E_EMPTY: u16 = 1;
pub const E_TOOMANY: u16 = 2;
pub const E_PAPER: u16 = 3;
pub const E_MARGIN: u16 = 4;
pub const E_CREEP: u16 = 5;
pub const E_SLOT: u16 = 6;
pub const E_DUPLICATE: u16 = 7;
pub const E_ORDER: u16 = 8;
pub const E_PLAN: u16 = 9;
pub const E_LEDGER: u16 = 10;

/// 小册子域错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookletError {
    Empty,
    TooMany,
    Paper,
    Margin,
    Creep,
    Slot,
    Duplicate,
    Order,
    Plan,
    Ledger,
}

impl BookletError {
    /// 诊断码：`SEG_BASE + 档内序号`。
    pub fn code(self) -> u32 {
        let n = match self {
            BookletError::Empty => E_EMPTY,
            BookletError::TooMany => E_TOOMANY,
            BookletError::Paper => E_PAPER,
            BookletError::Margin => E_MARGIN,
            BookletError::Creep => E_CREEP,
            BookletError::Slot => E_SLOT,
            BookletError::Duplicate => E_DUPLICATE,
            BookletError::Order => E_ORDER,
            BookletError::Plan => E_PLAN,
            BookletError::Ledger => E_LEDGER,
        };
        SEG_BASE + n as u32
    }

    /// 面向操作员的稳定说明文本。
    pub fn message(self) -> &'static str {
        match self {
            BookletError::Empty => "booklet page count must be positive",
            BookletError::TooMany => "page count exceeds saddle-stitch limit",
            BookletError::Paper => "spread sheet must be landscape within 40..2000mm",
            BookletError::Margin => "margin + fold clearance consumes the half sheet",
            BookletError::Creep => "creep compensation outside 0..3mm",
            BookletError::Slot => "page number outside 1..=pages",
            BookletError::Duplicate => "page or slot delivered twice",
            BookletError::Order => "sheet delivery out of order",
            BookletError::Plan => "booklet failed self-verification",
            BookletError::Ledger => "ledger does not match imposing result",
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

    /// 是否正尺寸且有限。
    pub fn is_positive(&self) -> bool {
        self.w > 0.0 && self.h > 0.0 && self.w.is_finite() && self.h.is_finite()
    }

    /// 正面积交叠判定（共享边界不算）。
    pub fn overlaps(&self, o: &Rect, eps: f32) -> bool {
        self.x + self.w - o.x > eps
            && self.y + self.h - o.y > eps
            && o.x + o.w - self.x > eps
            && o.y + o.h - self.y > eps
    }

    /// 是否被外框包含。
    pub fn inside(&self, outer: &Rect, eps: f32) -> bool {
        self.x >= outer.x - eps
            && self.y >= outer.y - eps
            && self.right() <= outer.right() + eps
            && self.bottom() <= outer.bottom() + eps
    }
}

/// 册子规格。`pages` 是**实际**页数，不是补齐后的页数——补齐在求解器里做，
/// 账本里只认实际页数，空白位由 0 显式占位。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BookletSpec {
    pub pages: u16,
    pub paper_w: f32,
    pub paper_h: f32,
    pub margin: f32,
    pub creep_mm: f32,
}

impl BookletSpec {
    /// 新建规格（默认 10mm 边距、零爬移）。
    pub const fn new(pages: u16, paper_w: f32, paper_h: f32) -> BookletSpec {
        BookletSpec {
            pages,
            paper_w,
            paper_h,
            margin: 10.0,
            creep_mm: 0.0,
        }
    }

    /// 设置四边对称边距（mm）。
    pub fn with_margin(mut self, margin: f32) -> BookletSpec {
        self.margin = margin;
        self
    }

    /// 设置爬移补偿（mm，每张纸的等差增量）。
    pub fn with_creep(mut self, creep_mm: f32) -> BookletSpec {
        self.creep_mm = creep_mm;
        self
    }

    /// 补齐后的页数（4 的倍数）。骑马钉的最小装订单元就是「一张纸 4 页」。
    pub fn padded(&self) -> u16 {
        ((self.pages + 3) / 4) * 4
    }

    /// 张数 = 补齐页数 / 4。
    pub fn sheet_count(&self) -> u16 {
        self.padded() / 4
    }

    /// 空白页数（补齐页数 - 实际页数）。
    pub fn blanks(&self) -> u16 {
        self.padded() - self.pages
    }

    /// 规格校验。顺序即诊断码优先级：页数 → 纸面 → 边距 → 爬移。
    pub fn validate(&self) -> Result<(), BookletError> {
        if self.pages == 0 {
            return Err(BookletError::Empty);
        }
        if self.pages > MAX_BOOKLET_PAGES {
            return Err(BookletError::TooMany);
        }
        if !self.paper_w.is_finite() || !self.paper_h.is_finite() {
            return Err(BookletError::Paper);
        }
        if self.paper_w < MIN_PAPER_MM
            || self.paper_h < MIN_PAPER_MM
            || self.paper_w > MAX_PAPER_MM
            || self.paper_h > MAX_PAPER_MM
        {
            return Err(BookletError::Paper);
        }
        // 对折册是「横向对开」：纸宽必须是两张版并排，所以宽必须大于高。
        // 竖纸喂进来一定是操作员把方向搞错了，宁可拒绝也不偷偷旋转。
        if self.paper_w <= self.paper_h {
            return Err(BookletError::Paper);
        }
        if !self.margin.is_finite() || self.margin < 0.0 {
            return Err(BookletError::Margin);
        }
        if !self.creep_mm.is_finite() || self.creep_mm < 0.0 || self.creep_mm > MAX_CREEP_MM {
            return Err(BookletError::Creep);
        }
        // 半版可用宽 = 纸宽/2 - 边距 - 折缝安全区 - 最大爬移（外层偏移最大）。
        let half = self.paper_w * 0.5;
        let max_shift = self.creep_mm * (self.sheet_count() as f32 - 1.0);
        let usable = half - self.margin - FOLD_CLEARANCE - max_shift;
        if usable < 10.0 {
            return Err(BookletError::Margin);
        }
        if self.paper_h - 2.0 * self.margin < 10.0 {
            return Err(BookletError::Margin);
        }
        Ok(())
    }
}

/// 半版页位。`page == 0` 是空白位（显式占位，不省略）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub page: u16,
    pub rect: Rect,
}

impl Slot {
    /// 新建页位。
    pub const fn new(page: u16, rect: Rect) -> Slot {
        Slot { page, rect }
    }

    /// 是否空白位。
    pub fn is_blank(&self) -> bool {
        self.page == 0
    }
}

/// 一张纸一面的左右页。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spread {
    pub left: Slot,
    pub right: Slot,
}

impl Spread {
    /// 该面两页页号之和；空白位按 0 计。非空白面应恒等于 `padded + 1`。
    pub fn page_sum(&self) -> u16 {
        self.left.page.saturating_add(self.right.page)
    }

    /// 空白位数。
    pub fn blanks(&self) -> u16 {
        let mut n = 0u16;
        if self.left.is_blank() {
            n += 1;
        }
        if self.right.is_blank() {
            n += 1;
        }
        n
    }
}

/// 一张纸：正反两面 + 脊线 + 该张的爬移量。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sheet {
    pub index: u16,
    pub front: Spread,
    pub back: Spread,
    pub fold_x: f32,
    pub creep: f32,
}

impl Sheet {
    /// 该张四个版位的页号（正面左、正面右、背面左、背面右）。
    pub fn four_pages(&self) -> (u16, u16, u16, u16) {
        (
            self.front.left.page,
            self.front.right.page,
            self.back.left.page,
            self.back.right.page,
        )
    }
}

/// 整册拼版结果。
#[derive(Clone, Debug)]
pub struct Booklet {
    pub spec: BookletSpec,
    pub padded: u16,
    pub sheets: Vec<Sheet>,
    pub blanks: u16,
}

impl Booklet {
    /// 张数（与 `spec.sheet_count()` 一致，独立按向量长度复核）。
    pub fn sheet_count(&self) -> u16 {
        self.sheets.len() as u16
    }

    /// 按 (张, 面, 左右) 取页号。`left = true` 取左版。
    pub fn page_of(&self, sheet: u16, front: bool, left: bool) -> Option<u16> {
        let sh = self.sheets.get(sheet as usize)?;
        let spread = if front { sh.front } else { sh.back };
        Some(if left { spread.left.page } else { spread.right.page })
    }

    /// 全量自检：页集合守恒、两面页和、空白计数、脊线居中、几何不交叠。
    pub fn verify(&self) -> Result<(), BookletError> {
        self.spec.validate()?;
        if self.sheets.len() != self.spec.sheet_count() as usize {
            return Err(BookletError::Plan);
        }
        if self.padded != self.spec.padded() {
            return Err(BookletError::Plan);
        }
        let mut counts = [0u8; (MAX_BOOKLET_PAGES as usize) + 1];
        let mut blanks = 0u16;
        let paper = Rect::new(0.0, 0.0, self.spec.paper_w, self.spec.paper_h);
        let mut i = 0usize;
        while i < self.sheets.len() {
            let sh = match self.sheets.get(i) {
                Some(v) => *v,
                None => return Err(BookletError::Plan),
            };
            if fabsf(sh.fold_x - self.spec.paper_w * 0.5) > 1e-3 {
                return Err(BookletError::Plan);
            }
            if sh.front.left.rect.overlaps(&sh.front.right.rect, 1e-3)
                || sh.back.left.rect.overlaps(&sh.back.right.rect, 1e-3)
            {
                return Err(BookletError::Plan);
            }
            if !sh.front.left.rect.inside(&paper, 1e-3)
                || !sh.front.right.rect.inside(&paper, 1e-3)
                || !sh.back.left.rect.inside(&paper, 1e-3)
                || !sh.back.right.rect.inside(&paper, 1e-3)
            {
                return Err(BookletError::Plan);
            }
            let page = sh.four_pages();
            let mut k = 0usize;
            while k < 4 {
                let p = match k {
                    0 => page.0,
                    1 => page.1,
                    2 => page.2,
                    _ => page.3,
                };
                if p == 0 {
                    blanks += 1;
                } else {
                    if p > self.spec.pages {
                        return Err(BookletError::Slot);
                    }
                    match counts.get_mut(p as usize) {
                        Some(c) => {
                            if *c > 0 {
                                return Err(BookletError::Duplicate);
                            }
                            *c += 1;
                        }
                        None => return Err(BookletError::Slot),
                    }
                }
                k += 1;
            }
            i += 1;
        }
        if blanks != self.blanks {
            return Err(BookletError::Plan);
        }
        let mut p = 1usize;
        while p <= self.spec.pages as usize {
            if counts.get(p) != Some(&1u8) {
                return Err(BookletError::Plan);
            }
            p += 1;
        }
        Ok(())
    }
}

/// 骑马钉页对：`k` 从 0 起，返回 `(低页, 高页)`，定义为 `(k+1, padded-k)`。
///
/// `pair(0) = (1, padded)`，最后一对是 `(padded/2, padded/2+1)`：首尾相接，
/// 全部页对无重无漏地覆盖 `1..=padded`（J1-4 直接测这条）。
/// 一张纸的正面印 `pair(2i)`、背面印 `pair(2i+1)`，因此
/// `pair(1) = (2, padded-1)` 正是第 0 张的背面。
pub fn page_pair(k: u16, padded: u16) -> Result<(u16, u16), BookletError> {
    if padded == 0 || padded % 4 != 0 {
        return Err(BookletError::Plan);
    }
    let pairs = padded / 2;
    if k >= pairs {
        return Err(BookletError::Slot);
    }
    let low = k + 1;
    let high = padded - k;
    if low == 0 || high == 0 || low > padded || high > padded || low == high {
        return Err(BookletError::Plan);
    }
    Ok((low, high))
}

/// 折缝爬移：第 `sheet` 张（0 基）向内收的毫米数。
///
/// 为什么外层收得最多：骑马钉的册子外层纸要绕过整个书脊，折后外层的
/// 内页会被推得离折缝更远，裁切后边缘先被切到，必须预先向内收。
/// 内外层收量差恒等于 `creep_mm`（J1-7 的等差断言）。
pub fn creep_shift(sheet: u16, sheets: u16, creep_mm: f32) -> f32 {
    if sheets == 0 || sheet >= sheets {
        return 0.0;
    }
    let c = if creep_mm < 0.0 {
        0.0
    } else if creep_mm > MAX_CREEP_MM {
        MAX_CREEP_MM
    } else {
        creep_mm
    };
    c * (sheets - 1 - sheet) as f32
}

/// 整册求解。页对 → 每张四版 → 几何（边距 / 折缝安全区 / 爬移）→ 自检。
pub fn impose(spec: BookletSpec) -> Result<Booklet, BookletError> {
    spec.validate()?;
    let padded = spec.padded();
    let sheets_n = spec.sheet_count();
    let half = spec.paper_w * 0.5;
    let mut sheets = Vec::with_capacity(sheets_n as usize);
    let mut i = 0u16;
    while i < sheets_n {
        let shift = creep_shift(i, sheets_n, spec.creep_mm);
        let w = half - spec.margin - FOLD_CLEARANCE - shift;
        if w <= 0.0 {
            return Err(BookletError::Margin);
        }
        let h = spec.paper_h - 2.0 * spec.margin;
        if h <= 0.0 {
            return Err(BookletError::Margin);
        }
        let front_rect_l = Rect::new(spec.margin, spec.margin, w, h);
        let front_rect_r = Rect::new(half + FOLD_CLEARANCE + shift, spec.margin, w, h);
        // 正反两面的版位几何完全相同：折后背面翻过来正好对位。
        let pair_f = page_pair(2 * i, padded)?;
        let pair_b = page_pair(2 * i + 1, padded)?;
        let clamp = |p: u16| -> u16 {
            if p <= spec.pages {
                p
            } else {
                0
            }
        };
        // 正面：左放偶数页（外页），右放奇数页（内页）；背面镜像。
        let front = Spread {
            left: Slot::new(clamp(pair_f.1), front_rect_l),
            right: Slot::new(clamp(pair_f.0), front_rect_r),
        };
        let back = Spread {
            left: Slot::new(clamp(pair_b.0), front_rect_l),
            right: Slot::new(clamp(pair_b.1), front_rect_r),
        };
        sheets.push(Sheet {
            index: i,
            front,
            back,
            fold_x: half,
            creep: shift,
        });
        i += 1;
    }
    let booklet = Booklet {
        spec,
        padded,
        sheets,
        blanks: padded - spec.pages,
    };
    booklet.verify()?;
    Ok(booklet)
}

/// 几何自检（可单独调用于反序列化后的册子）。
pub fn check_geometry(booklet: &Booklet) -> Result<(), BookletError> {
    let paper = Rect::new(0.0, 0.0, booklet.spec.paper_w, booklet.spec.paper_h);
    let mut i = 0usize;
    while i < booklet.sheets.len() {
        let sh = match booklet.sheets.get(i) {
            Some(v) => *v,
            None => return Err(BookletError::Plan),
        };
        if fabsf(sh.fold_x - booklet.spec.paper_w * 0.5) > 1e-3 {
            return Err(BookletError::Plan);
        }
        if !sh.front.left.rect.inside(&paper, 1e-3) || !sh.front.right.rect.inside(&paper, 1e-3)
        {
            return Err(BookletError::Plan);
        }
        if sh.front.left.rect.right() > sh.front.right.rect.x + 1e-3 {
            return Err(BookletError::Margin);
        }
        if !sh.front.left.rect.is_positive() || !sh.front.right.rect.is_positive() {
            return Err(BookletError::Margin);
        }
        i += 1;
    }
    Ok(())
}

/// 骑马钉投递账本：按 (张, 面, 左右) 记账，独立发现重复与乱序。
///
/// 与 `Booklet` 的区别：册子是**输出**，账本是**输入侧的收货记录**
/// （拼版工作站逐张投递时写入）。两者必须对得上，所以账本自带回放与对账。
#[derive(Clone, Debug)]
pub struct StitchLedger {
    entries: Vec<(u16, bool, bool, u16)>,
    limit: usize,
}

impl StitchLedger {
    /// 建账，`limit` 为硬容量。
    pub fn new(limit: usize) -> StitchLedger {
        StitchLedger {
            entries: Vec::new(),
            limit,
        }
    }

    /// 当前条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 追加一条投递。拒绝：容量满、页号越界、重复页、重复版位、张号倒退。
    pub fn push(&mut self, sheet: u16, front: bool, left: bool, page: u16) -> Result<(), BookletError> {
        if self.entries.len() >= self.limit {
            return Err(BookletError::Ledger);
        }
        if page > MAX_BOOKLET_PAGES {
            return Err(BookletError::Slot);
        }
        if let Some(last) = self.entries.last() {
            if sheet < last.0 {
                return Err(BookletError::Order);
            }
            if sheet == last.0 {
                if front == last.1 && left == last.2 {
                    return Err(BookletError::Duplicate);
                }
                // 同一张内允许的顺序：正面左 → 正面右 → 背面左 → 背面右。
                let ord = |f: bool, l: bool| -> u8 {
                    match (f, l) {
                        (true, true) => 0,
                        (true, false) => 1,
                        (false, true) => 2,
                        _ => 3,
                    }
                };
                if ord(front, left) < ord(last.1, last.2) {
                    return Err(BookletError::Order);
                }
            }
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.0 == sheet && e.1 == front && e.2 == left {
                    return Err(BookletError::Duplicate);
                }
                if page > 0 && e.3 == page {
                    return Err(BookletError::Duplicate);
                }
            }
            i += 1;
        }
        self.entries.push((sheet, front, left, page));
        Ok(())
    }

    /// 回放：按记账顺序的页号串（含空白 0）。
    pub fn replay(&self) -> Vec<u16> {
        let mut out = Vec::with_capacity(self.entries.len());
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                out.push(e.3);
            }
            i += 1;
        }
        out
    }

    /// 按册子整批装载（正→背，左→右）。
    pub fn load(&mut self, b: &Booklet) -> Result<usize, BookletError> {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < b.sheets.len() {
            if let Some(sh) = b.sheets.get(i) {
                self.push(sh.index, true, true, sh.front.left.page)?;
                self.push(sh.index, true, false, sh.front.right.page)?;
                self.push(sh.index, false, true, sh.back.left.page)?;
                self.push(sh.index, false, false, sh.back.right.page)?;
                n += 4;
            }
            i += 1;
        }
        Ok(n)
    }

    /// 与册子逐位对账，返回不一致条数（0 = 完全一致）。
    pub fn audit(&self, b: &Booklet) -> usize {
        let expect = b.sheets.len() * 4;
        if self.entries.len() != expect {
            return expect.max(self.entries.len());
        }
        let mut bad = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                let sheet = b.sheets.get(e.0 as usize);
                match sheet {
                    Some(sh) => {
                        let want = if e.1 {
                            if e.2 {
                                sh.front.left.page
                            } else {
                                sh.front.right.page
                            }
                        } else if e.2 {
                            sh.back.left.page
                        } else {
                            sh.back.right.page
                        };
                        if want != e.3 {
                            bad += 1;
                        }
                    }
                    None => bad += 1,
                }
            }
            i += 1;
        }
        bad
    }
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

/// 六类坏样本：空册、超上限、竖纸、边距吃穿、重复投递、乱序投递。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;
    let good = BookletSpec::new(16, 420.0, 297.0).with_margin(10.0);

    if impose(BookletSpec::new(0, 420.0, 297.0)).err() == Some(BookletError::Empty) {
        caught += 1;
    }
    if impose(BookletSpec::new(MAX_BOOKLET_PAGES + 4, 420.0, 297.0)).err()
        == Some(BookletError::TooMany)
    {
        caught += 1;
    }
    if impose(BookletSpec::new(16, 297.0, 420.0)).err() == Some(BookletError::Paper) {
        caught += 1;
    }
    if impose(BookletSpec::new(16, 420.0, 297.0).with_margin(200.0)).err()
        == Some(BookletError::Margin)
    {
        caught += 1;
    }
    let b = match impose(good) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let mut led = StitchLedger::new(4096);
    let first_page = match b.page_of(0, true, true) {
        Some(p) => p,
        None => return (caught, total),
    };
    if led.push(0, true, true, first_page).is_err() {
        return (caught, total);
    }
    // 同一版位再投一次 = 重复
    if led.push(0, true, true, first_page).err() == Some(BookletError::Duplicate) {
        caught += 1;
    }
    // 张号倒退 = 乱序
    if led.push(1, true, true, 1).is_ok() {
        if led.push(0, false, true, 2).err() == Some(BookletError::Order) {
            caught += 1;
        }
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50023 判据集（16 条）。
pub fn run_unx50023_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50023");
    let spec = BookletSpec::new(16, 420.0, 297.0).with_margin(10.0);
    let book = match impose(spec) {
        Ok(b) => b,
        Err(_) => {
            s.add("50023-J1-16页册序", false, "求解失败");
            return s;
        }
    };

    // J1-1 16 页册序逐页比对（显式表：4 张 × 正反 × 左右）
    // 期望来自页对公式 pair(k) 的手工展开，不是实现回调：
    // 正面 (16,1)(14,3)(12,5)(10,7)；背面 (2,15)(4,13)(6,11)(8,9)。
    let want: [(u16, u16, u16, u16); 4] =
        [(16, 1, 2, 15), (14, 3, 4, 13), (12, 5, 6, 11), (10, 7, 8, 9)];
    let mut ok = book.sheets.len() == 4;
    let mut mism = 0usize;
    let mut i = 0usize;
    while i < want.len() {
        if let Some(sh) = book.sheets.get(i) {
            if sh.four_pages() != want[i] {
                mism += 1;
            }
        } else {
            mism += 1;
        }
        i += 1;
    }
    ok = ok && mism == 0;
    let det = format!("mismatch={}", mism);
    s.add_dyn("50023-J1-16页册序", ok, &det);

    // J1-2 页集合守恒：1..=16 每页恰好一次
    let mut seen = [0u8; 17];
    let mut j = 0usize;
    while j < book.sheets.len() {
        if let Some(sh) = book.sheets.get(j) {
            let four = sh.four_pages();
            let arr = [four.0, four.1, four.2, four.3];
            let mut k = 0usize;
            while k < 4 {
                let p = arr[k] as usize;
                if p > 0 && p < seen.len() {
                    if let Some(v) = seen.get_mut(p) {
                        *v += 1;
                    }
                }
                k += 1;
            }
        }
        j += 1;
    }
    let mut ok = true;
    let mut n = 1usize;
    while n <= 16 {
        if seen.get(n) != Some(&1u8) {
            ok = false;
        }
        n += 1;
    }
    s.add("50023-J1-页集合守恒", ok, if ok { "" } else { "页号重复或缺失" });

    // J1-3 两面页和恒为 padded+1
    let mut ok = true;
    let mut j = 0usize;
    while j < book.sheets.len() {
        if let Some(sh) = book.sheets.get(j) {
            if sh.front.page_sum() != book.padded + 1 || sh.back.page_sum() != book.padded + 1 {
                ok = false;
            }
        }
        j += 1;
    }
    s.add("50023-J1-两面页和", ok, if ok { "" } else { "页和对不上" });

    // J1-4 页对覆盖：pair(0..padded/2) 恰好铺满 1..=padded
    let mut cover = [0u8; 17];
    let mut k = 0u16;
    let mut ok = true;
    while k < book.padded / 2 {
        match page_pair(k, book.padded) {
            Ok((a, b)) => {
                if a as usize >= cover.len() || b as usize >= cover.len() {
                    ok = false;
                } else {
                    if let Some(v) = cover.get_mut(a as usize) {
                        *v += 1;
                    }
                    if let Some(v) = cover.get_mut(b as usize) {
                        *v += 1;
                    }
                }
            }
            Err(_) => ok = false,
        }
        k += 1;
    }
    let mut n = 1usize;
    while n <= 16 {
        if cover.get(n) != Some(&1u8) {
            ok = false;
        }
        n += 1;
    }
    s.add("50023-J1-页对覆盖", ok, if ok { "" } else { "页对不铺满" });

    // J1-5 13 页补齐：padded=16、空白=3、张数=4
    let b13 = impose(BookletSpec::new(13, 420.0, 297.0).with_margin(10.0));
    let ok = match &b13 {
        Ok(b) => b.padded == 16 && b.blanks == 3 && b.sheet_count() == 4 && b.verify().is_ok(),
        Err(_) => false,
    };
    s.add("50023-J1-补齐计数", ok, if ok { "" } else { "补齐记账错" });

    // J1-6 脊线居中：每张 fold_x == 纸宽/2
    let mut ok = true;
    let mut j = 0usize;
    while j < book.sheets.len() {
        if let Some(sh) = book.sheets.get(j) {
            if fabsf(sh.fold_x - 210.0) > 1e-4 {
                ok = false;
            }
        }
        j += 1;
    }
    s.add("50023-J1-脊线居中", ok, if ok { "" } else { "脊线偏离纸中线" });

    // J1-7 爬移等差单调：外层多内层少，相邻差恒为 creep_mm
    let cspec = BookletSpec::new(16, 420.0, 297.0).with_margin(10.0).with_creep(0.25);
    let mut ok = false;
    let mut step_err = 0.0f32;
    if let Ok(cb) = impose(cspec) {
        let mut j = 0usize;
        let mut mono = true;
        while j + 1 < cb.sheets.len() {
            if let (Some(a), Some(b)) = (cb.sheets.get(j), cb.sheets.get(j + 1)) {
                let d = a.creep - b.creep;
                if d < 0.0 {
                    mono = false;
                }
                step_err += fabsf(d - 0.25);
                if a.creep <= 0.0 {
                    mono = false;
                }
            }
            j += 1;
        }
        ok = mono && step_err < 1e-4;
    }
    let det = format!("step_err={:.6}", step_err);
    s.add_dyn("50023-J1-爬移等差", ok, &det);

    // J1-8 爬移不越脊：左版右缘 < 脊线 < 右版左缘
    let ok = match impose(cspec) {
        Ok(cb) => {
            let mut good = true;
            let mut j = 0usize;
            while j < cb.sheets.len() {
                if let Some(sh) = cb.sheets.get(j) {
                    if sh.front.left.rect.right() >= sh.fold_x || sh.front.right.rect.x <= sh.fold_x
                    {
                        good = false;
                    }
                }
                j += 1;
            }
            good
        }
        Err(_) => false,
    };
    s.add("50023-J1-不越脊", ok, if ok { "" } else { "版位压到脊线" });

    // J1-9 账本回放 + 对账一致
    let mut led = StitchLedger::new(4096);
    let loaded = match led.load(&book) {
        Ok(n) => n,
        Err(_) => 0,
    };
    let ok = loaded == book.sheets.len() * 4
        && led.audit(&book) == 0
        && led.replay().len() == loaded;
    let det = format!("loaded={} audit={}", loaded, led.audit(&book));
    s.add_dyn("50023-J1-账本对账", ok, &det);

    // J1-10 重复页拒（同页投到另一版位）
    let mut led2 = StitchLedger::new(64);
    let mut ok = false;
    if let Some(p0) = book.page_of(0, true, true) {
        if led2.push(0, true, true, p0).is_ok() {
            ok = led2.push(0, true, false, p0).err() == Some(BookletError::Duplicate);
        }
    }
    s.add("50023-J1-重复拒", ok, if ok { "" } else { "重复页未拒" });

    // J1-11 乱序拒：同一张内背面先于正面
    let mut led3 = StitchLedger::new(64);
    let mut ok = false;
    if led3.push(0, false, true, 2).is_ok() {
        ok = led3.push(0, true, true, 16).err() == Some(BookletError::Order);
    }
    s.add("50023-J1-乱序拒", ok, if ok { "" } else { "乱序未拒" });

    // J1-12 页数非法：0 页与超上限
    let e0 = impose(BookletSpec::new(0, 420.0, 297.0)).err() == Some(BookletError::Empty);
    let e1 = impose(BookletSpec::new(300, 420.0, 297.0)).err() == Some(BookletError::TooMany);
    let ok = e0 && e1;
    s.add("50023-J1-页数非法", ok, if ok { "" } else { "非法页数未拒" });

    // J1-13 竖纸拒绝（对折册必须横向对开）
    let ok = impose(BookletSpec::new(16, 297.0, 420.0)).err() == Some(BookletError::Paper);
    s.add("50023-J1-竖纸拒", ok, if ok { "" } else { "竖纸未被拒" });

    // J1-14 几何自检：脊线/包含/不交叠
    let ok = check_geometry(&book).is_ok() && book.verify().is_ok();
    s.add("50023-J1-几何自检", ok, if ok { "" } else { "几何自检失败" });

    // J1-15 确定性：同规格两次求解逐版一致
    let again = impose(spec);
    let ok = match &again {
        Ok(b) => b.sheets == book.sheets && b.blanks == book.blanks,
        Err(_) => false,
    };
    s.add("50023-J1-确定性", ok, if ok { "" } else { "两次求解不一致" });

    // J1-16 注入核
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50023-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50023_checks_green() {
        let set = run_unx50023_checks();
        assert!(set.all_passed(), "UNX-F50023 red: {}", set.red_items_text());
    }

    /// 32 页册：八张纸，每张四页互不相同且页和恒为 33。
    #[test]
    fn unx50023_thirty_two_pages() {
        let b = impose(BookletSpec::new(32, 420.0, 297.0).with_margin(10.0));
        let b = match b {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(b.sheets.len(), 8);
        let mut i = 0usize;
        while i < b.sheets.len() {
            if let Some(sh) = b.sheets.get(i) {
                assert_eq!(sh.front.page_sum(), 33);
                assert_eq!(sh.back.page_sum(), 33);
                assert_eq!(sh.back.left.page, sh.front.right.page + 1);
            }
            i += 1;
        }
    }

    /// 爬移为 0 时所有版位几何完全一致。
    #[test]
    fn unx50023_zero_creep_uniform() {
        let b = impose(BookletSpec::new(20, 420.0, 297.0).with_margin(10.0));
        let b = match b {
            Ok(v) => v,
            Err(_) => return,
        };
        let first = b.sheets.first().map(|s| s.front.left.rect);
        let mut i = 0usize;
        while i < b.sheets.len() {
            if let (Some(sh), Some(r0)) = (b.sheets.get(i), first) {
                assert_eq!(sh.front.left.rect, r0);
                assert_eq!(sh.creep, 0.0);
            }
            i += 1;
        }
    }
}
