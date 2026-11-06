//! UNX-F50038 B22 批向量账骨架（布局用例集生成器 + 校验和）（0x430e0 段）
//!
//! 锚点：UNX-F50038-J1 用例带校验和落账；判据锚定 §AI-63 §5。
//!
//! 数据段：`SEG_BASE = 0x430e0`，诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 10`，
//! 逐项见 [`VecError`]。
//!
//! 批向量账回答的问题很具体：**用例集合被谁改过，账本说不说得清**。
//! 生成器把「纸张 × N-up × 单双面 × 旋转」做成确定性笛卡尔积，每条用例
//! 先算出真实的拼版计划（网格 + 每面页号），再把「规格 + 网格 + 页号」
//! 折进一枚 FNV-1a 校验和；落账只接受自洽条目，入账后整本可用
//! [`VecBook::verify_all`] 全量重算——任何一格页号被改都能定位到条目。
//!
//! 结构清单：
//! * [`LayoutCase`]：用例规格（纸张 / N-up / 单双面 / 旋转 / 页数）；
//! * [`Grid`] / [`Sheet`] / [`Plan`]：拼版计划（格子尺寸 + 每面页号 + 空白数）；
//! * [`VecEntry`]：落账条目（规格 + 面数/格数 + 双指纹）；
//! * [`VecBook`]：定容向量账（严格入账 / 非严格导入 / 全量复核 / 根指纹）；
//! * [`injection_core`]：9 类坏样本注入核。
//!
//! 两条路径是刻意分开的：`admit()` 是「外部提交」的严格入口，先重算再收；
//! `import()` 是「从磁盘读回」的非信任入口，只做结构校验，把内容真伪留给
//! `verify_all()`。只有这样才能测出「账本里本来就躺着一条被改过的记录」。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x430e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x430e0;

const E_EMPTY: u16 = 1;
const E_FULL: u16 = 2;
const E_DUPLICATE: u16 = 3;
const E_BAD_SUM: u16 = 4;
const E_BAD_NUP: u16 = 5;
const E_BAD_ROTATE: u16 = 6;
const E_BAD_PAGES: u16 = 7;
const E_RANGE: u16 = 8;
const E_DRIFT: u16 = 9;
const E_SHEET: u16 = 10;

/// 向量账错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecError {
    /// 空用例集 / 空导入。
    Empty,
    /// 账本容量已满。
    Full,
    /// 用例 id 重复。
    Duplicate,
    /// 校验和与内容不符（入账门）。
    BadSum,
    /// N-up 不在闭集 {2,4,6,9,16}。
    BadNup,
    /// 旋转不在闭集 {0,90,180,270}。
    BadRotate,
    /// 页数为 0 或超过 4096。
    BadPages,
    /// 字段越界（id=0 或格子退化）。
    Range,
    /// 全量复核时条目内容漂移。
    Drift,
    /// 全量复核时面数/格数与重算计划不符。
    Sheet,
}

impl VecError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            VecError::Empty => E_EMPTY,
            VecError::Full => E_FULL,
            VecError::Duplicate => E_DUPLICATE,
            VecError::BadSum => E_BAD_SUM,
            VecError::BadNup => E_BAD_NUP,
            VecError::BadRotate => E_BAD_ROTATE,
            VecError::BadPages => E_BAD_PAGES,
            VecError::Range => E_RANGE,
            VecError::Drift => E_DRIFT,
            VecError::Sheet => E_SHEET,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            VecError::Empty => "empty case set",
            VecError::Full => "vector book full",
            VecError::Duplicate => "case id already present",
            VecError::BadSum => "case checksum mismatch",
            VecError::BadNup => "nup not in {2,4,6,9,16}",
            VecError::BadRotate => "rotation not in {0,90,180,270}",
            VecError::BadPages => "page count 0 or above 4096",
            VecError::Range => "case field out of range",
            VecError::Drift => "ledger entry drifted from its checksum",
            VecError::Sheet => "sheet/cell count disagrees with recomputed plan",
        }
    }
}

/// FNV-1a 32 位偏移基。
pub const FNV_OFFSET: u32 = 0x811c_9dc5;
/// FNV-1a 32 位质数。
pub const FNV_PRIME: u32 = 0x0100_0193;
/// 单本账容量（40 例生成 + 8 个复测位）。
pub const MAX_CASES: usize = 48;
/// N-up 闭集（卡面档位）。
pub const NUPS: [u8; 5] = [2, 4, 6, 9, 16];
/// 旋转闭集。
pub const ROTATIONS: [u16; 4] = [0, 90, 180, 270];
/// 页数上限：超过 4096 的批量不属于本骨架的验证范围。
pub const MAX_PAGES: u16 = 4096;
/// 页边距（1/10 pt）：18 pt。
pub const MARGIN10: i32 = 180;
/// 格间距（1/10 pt）：6 pt。
pub const GUTTER10: i32 = 60;

fn mix(h: u32, v: u32) -> u32 {
    let a = (h ^ (v & 0xffff)).wrapping_mul(FNV_PRIME);
    (a ^ (v >> 16)).wrapping_mul(FNV_PRIME)
}

/// 纸张三档。尺寸用 1/10 pt 整数，拼版全流程不碰浮点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paper {
    /// A4 595.3 × 841.9 pt。
    A4,
    /// A3 841.9 × 1191.1 pt。
    A3,
    /// Letter 612 × 792 pt。
    Letter,
}

impl Paper {
    /// 纸张三档（顺序即指纹里的 paper 码）。
    pub const ALL: [Paper; 3] = [Paper::A4, Paper::A3, Paper::Letter];

    /// 纵向尺寸 (宽, 高)，单位 1/10 pt。
    pub fn tenths(self) -> (i32, i32) {
        match self {
            Paper::A4 => (5953, 8419),
            Paper::A3 => (8419, 11911),
            Paper::Letter => (6120, 7920),
        }
    }

    /// 指纹码 0..3。
    pub fn code(self) -> u32 {
        match self {
            Paper::A4 => 0,
            Paper::A3 => 1,
            Paper::Letter => 2,
        }
    }

    /// 标签。
    pub fn tag(self) -> &'static str {
        match self {
            Paper::A4 => "a4",
            Paper::A3 => "a3",
            Paper::Letter => "letter",
        }
    }
}

/// 一条布局用例规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutCase {
    /// 用例号（1..=n，0 视为越界）。
    pub id: u16,
    /// 纸张。
    pub paper: Paper,
    /// N-up 档位。
    pub nup: u8,
    /// 是否双面。
    pub duplex: bool,
    /// 旋转角（度）。
    pub rotate: u16,
    /// 文档页数。
    pub pages: u16,
}

/// 单面网格：行列数与格尺寸（1/10 pt）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    /// 列数。
    pub cols: u8,
    /// 行数。
    pub rows: u8,
    /// 格宽（1/10 pt）。
    pub cell_w10: u16,
    /// 格高（1/10 pt）。
    pub cell_h10: u16,
}

/// 一个印面：页号按格排布，0 表示空白。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sheet {
    /// 第几张纸（从 0 起）。
    pub index: u16,
    /// 是否背面。
    pub back: bool,
    /// 每格页号（0 = 空白）。
    pub cells: Vec<i16>,
}

/// 拼版计划。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// 网格。
    pub grid: Grid,
    /// 印面列表（正面在前，背面随其后）。
    pub sheets: Vec<Sheet>,
    /// 已放置页数。
    pub placed: u16,
    /// 空白格数。
    pub blanks: u16,
}

/// 规格校验：不合法规格一律在算计划之前挡回。
pub fn validate_case(c: &LayoutCase) -> Result<(), VecError> {
    if c.id == 0 {
        return Err(VecError::Range);
    }
    if !NUPS.contains(&c.nup) {
        return Err(VecError::BadNup);
    }
    if !ROTATIONS.contains(&c.rotate) {
        return Err(VecError::BadRotate);
    }
    if c.pages == 0 || c.pages > MAX_PAGES {
        return Err(VecError::BadPages);
    }
    Ok(())
}

/// N-up 的基准行列：2→1×2、4→2×2、6→2×3、9→3×3、16→4×4。
fn base_grid(nup: u8) -> Option<(u8, u8)> {
    match nup {
        2 => Some((1, 2)),
        4 => Some((2, 2)),
        6 => Some((2, 3)),
        9 => Some((3, 3)),
        16 => Some((4, 4)),
        _ => None,
    }
}

/// 由规格推网格。90°/270° 视为整张纸转置：行列互换、可用宽高互换，
/// 因此 `grid(90)` 必是 `grid(0)` 的转置——这条等式就是旋转语义的定义。
pub fn grid_for(c: &LayoutCase) -> Result<Grid, VecError> {
    let (bc, br) = match base_grid(c.nup) {
        Some(v) => v,
        None => return Err(VecError::BadNup),
    };
    let swapped = c.rotate == 90 || c.rotate == 270;
    let (cols, rows) = if swapped { (br, bc) } else { (bc, br) };
    let (pw, ph) = c.paper.tenths();
    let (uw, uh) = if swapped { (ph, pw) } else { (pw, ph) };
    let cols_i = cols as i32;
    let rows_i = rows as i32;
    let avail_w = uw - 2 * MARGIN10 - (cols_i - 1) * GUTTER10;
    let avail_h = uh - 2 * MARGIN10 - (rows_i - 1) * GUTTER10;
    if avail_w <= 0 || avail_h <= 0 || cols_i <= 0 || rows_i <= 0 {
        return Err(VecError::Range);
    }
    let cw = avail_w / cols_i;
    let ch = avail_h / rows_i;
    if cw <= 0 || ch <= 0 || cw > u16::MAX as i32 || ch > u16::MAX as i32 {
        return Err(VecError::Range);
    }
    Ok(Grid {
        cols,
        rows,
        cell_w10: cw as u16,
        cell_h10: ch as u16,
    })
}

/// 算拼版计划：页号顺序铺格，最后一格不满即留 0，双面则在正面满后翻背。
pub fn plan(c: &LayoutCase) -> Result<Plan, VecError> {
    validate_case(c)?;
    let grid = grid_for(c)?;
    let per_side = c.nup as usize;
    let sides = if c.duplex { 2usize } else { 1usize };
    let pages = c.pages as usize;
    let capacity_per_sheet = per_side * sides;
    let sheet_count = (pages + capacity_per_sheet - 1) / capacity_per_sheet;
    let mut sheets: Vec<Sheet> = Vec::with_capacity(sheet_count * sides);
    let mut next: usize = 1;
    let mut blanks = 0u16;
    let mut s = 0usize;
    while s < sheet_count {
        let mut side = 0usize;
        while side < sides {
            let mut cells: Vec<i16> = Vec::with_capacity(per_side);
            let mut k = 0usize;
            while k < per_side {
                if next <= pages {
                    cells.push(next as i16);
                    next += 1;
                } else {
                    cells.push(0);
                    blanks = blanks.saturating_add(1);
                }
                k += 1;
            }
            sheets.push(Sheet {
                index: s as u16,
                back: side == 1,
                cells,
            });
            side += 1;
        }
        s += 1;
    }
    Ok(Plan {
        grid,
        sheets,
        placed: c.pages,
        blanks,
    })
}

/// 规格指纹：只串规格字段（用于把「规格被改」与「内容被改」分开）。
pub fn spec_head(c: &LayoutCase) -> u32 {
    let mut h = FNV_OFFSET;
    h = mix(h, c.id as u32);
    h = mix(h, c.paper.code());
    h = mix(h, c.nup as u32);
    h = mix(h, if c.duplex { 1 } else { 0 });
    h = mix(h, c.rotate as u32);
    h = mix(h, c.pages as u32);
    h
}

/// 计划校验和：规格 + 网格 + 每面每格页号，全部按整数混入。
pub fn plan_sum(c: &LayoutCase, p: &Plan) -> u32 {
    let mut h = spec_head(c);
    h = mix(h, p.grid.cols as u32);
    h = mix(h, p.grid.rows as u32);
    h = mix(h, p.grid.cell_w10 as u32);
    h = mix(h, p.grid.cell_h10 as u32);
    let mut i = 0usize;
    while i < p.sheets.len() {
        if let Some(sh) = p.sheets.get(i) {
            h = mix(h, sh.index as u32);
            h = mix(h, if sh.back { 1 } else { 0 });
            let mut k = 0usize;
            while k < sh.cells.len() {
                if let Some(v) = sh.cells.get(k) {
                    h = mix(h, *v as u32);
                }
                k += 1;
            }
        }
        i += 1;
    }
    h
}

/// 向量账条目：规格 + 面数/格数 + 双指纹。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VecEntry {
    /// 用例规格。
    pub case: LayoutCase,
    /// 印面数。
    pub sheets: u16,
    /// 格数（所有印面之和）。
    pub cells: u16,
    /// 计划校验和。
    pub sum: u32,
    /// 规格指纹。
    pub head: u32,
}

impl VecEntry {
    /// 由规格封装条目（重算校验和）；规格非法即拒。
    pub fn seal(c: LayoutCase) -> Result<VecEntry, VecError> {
        let p = plan(&c)?;
        let cells = p.sheets.len() as u32 * c.nup as u32;
        if cells > u16::MAX as u32 {
            return Err(VecError::Range);
        }
        Ok(VecEntry {
            case: c,
            sheets: p.sheets.len() as u16,
            cells: cells as u16,
            sum: plan_sum(&c, &p),
            head: spec_head(&c),
        })
    }

    /// 一行台账。
    pub fn ledger_line(&self) -> String {
        format!(
            "#{} {} nup={} dup={} rot={} pages={} sheets={} cells={} sum={:08x}",
            self.case.id,
            self.case.paper.tag(),
            self.case.nup,
            if self.case.duplex { 1 } else { 0 },
            self.case.rotate,
            self.case.pages,
            self.sheets,
            self.cells,
            self.sum
        )
    }
}

/// 定容向量账。
pub struct VecBook {
    entries: Vec<VecEntry>,
    capacity: usize,
    rejected: u32,
}

impl VecBook {
    /// 建空账。
    pub fn new(capacity: usize) -> VecBook {
        let cap = if capacity == 0 { 1 } else { capacity };
        VecBook {
            entries: Vec::with_capacity(cap),
            capacity: cap,
            rejected: 0,
        }
    }

    /// 默认容量账本。
    pub fn with_default_capacity() -> VecBook {
        VecBook::new(MAX_CASES)
    }

    fn reject<T>(&mut self, e: VecError) -> Result<T, VecError> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    fn find(&self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.case.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 账本条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 累计被拒次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 按用例号取条目。
    pub fn entry(&self, id: u16) -> Option<&VecEntry> {
        match self.find(id) {
            Some(i) => self.entries.get(i),
            None => None,
        }
    }

    /// 严格入账：结构 + 重算校验和，全部自洽才收。
    pub fn admit(&mut self, e: VecEntry) -> Result<(), VecError> {
        if self.entries.len() >= self.capacity {
            return self.reject(VecError::Full);
        }
        if self.find(e.case.id).is_some() {
            return self.reject(VecError::Duplicate);
        }
        let p = match plan(&e.case) {
            Ok(v) => v,
            Err(err) => return self.reject(err),
        };
        let cells = p.sheets.len() as u32 * e.case.nup as u32;
        if e.sheets as u32 != p.sheets.len() as u32 || e.cells as u32 != cells {
            return self.reject(VecError::Sheet);
        }
        if e.sum != plan_sum(&e.case, &p) || e.head != spec_head(&e.case) {
            return self.reject(VecError::BadSum);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 非信任导入：只查结构（空 / 重号 / 超容），内容真伪留给 `verify_all`。
    pub fn import(&mut self, items: &[VecEntry]) -> Result<usize, VecError> {
        if items.is_empty() {
            return self.reject(VecError::Empty);
        }
        let mut i = 0usize;
        while i < items.len() {
            let e = match items.get(i) {
                Some(v) => v,
                None => return self.reject(VecError::Empty),
            };
            if self.entries.len() >= self.capacity {
                return self.reject(VecError::Full);
            }
            if self.find(e.case.id).is_some() {
                return self.reject(VecError::Duplicate);
            }
            self.entries.push(e.clone());
            i += 1;
        }
        Ok(self.entries.len())
    }

    /// 全量复核：逐条重算计划，先比面数/格数，再比双指纹。
    pub fn verify_all(&self) -> Result<usize, VecError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                let p = plan(&e.case)?;
                let cells = p.sheets.len() as u32 * e.case.nup as u32;
                if e.sheets as u32 != p.sheets.len() as u32 || e.cells as u32 != cells {
                    return Err(VecError::Sheet);
                }
                if e.sum != plan_sum(&e.case, &p) || e.head != spec_head(&e.case) {
                    return Err(VecError::Drift);
                }
            }
            i += 1;
        }
        Ok(self.entries.len())
    }

    /// 根指纹：按入账序把 (id, sum, head) 折叠进一条链，顺序敏感。
    pub fn root(&self) -> u32 {
        let mut h = FNV_OFFSET ^ 0x0000_5006;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = mix(h, e.case.id as u32);
                h = mix(h, e.sum);
                h = mix(h, e.head);
            }
            i += 1;
        }
        h
    }

    /// 计划放置的文档页数合计。
    pub fn total_pages(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                n = n.saturating_add(e.case.pages as u32);
            }
            i += 1;
        }
        n
    }

    /// 全表台账文本。
    pub fn ledger_text(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                out.push_str(&e.ledger_line());
                out.push('\n');
            }
            i += 1;
        }
        out
    }
}

/// 确定性用例集生成器：纸张固定，遍历 N-up × 单双面 × 旋转。
pub fn generate_cases(paper: Paper, pages: u16) -> Result<Vec<LayoutCase>, VecError> {
    if pages == 0 || pages > MAX_PAGES {
        return Err(VecError::BadPages);
    }
    let mut out: Vec<LayoutCase> = Vec::with_capacity(NUPS.len() * 2 * ROTATIONS.len());
    let mut id: u16 = 1;
    let mut ni = 0usize;
    while ni < NUPS.len() {
        let mut di = 0usize;
        while di < 2 {
            let mut ri = 0usize;
            while ri < ROTATIONS.len() {
                if let (Some(nup), Some(rot)) = (NUPS.get(ni), ROTATIONS.get(ri)) {
                    out.push(LayoutCase {
                        id,
                        paper,
                        nup: *nup,
                        duplex: di == 1,
                        rotate: *rot,
                        pages,
                    });
                    id += 1;
                }
                ri += 1;
            }
            di += 1;
        }
        ni += 1;
    }
    Ok(out)
}

/// 按生成器建账（严格入账 40 条）。
pub fn sealed_book(pages: u16) -> VecBook {
    let mut b = VecBook::with_default_capacity();
    match generate_cases(Paper::A4, pages) {
        Ok(cases) => {
            let mut i = 0usize;
            while i < cases.len() {
                if let Some(c) = cases.get(i) {
                    if let Ok(e) = VecEntry::seal(*c) {
                        let _ = b.admit(e);
                    }
                }
                i += 1;
            }
        }
        Err(_) => {}
    }
    b
}

/// 注入核：9 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 9usize;
    let base = LayoutCase {
        id: 1,
        paper: Paper::A4,
        nup: 4,
        duplex: true,
        rotate: 0,
        pages: 9,
    };

    // 1) N-up 不在闭集。
    let bad = LayoutCase { nup: 3, ..base };
    if VecEntry::seal(bad).err() == Some(VecError::BadNup) {
        caught += 1;
    }

    // 2) 旋转不在闭集。
    let bad = LayoutCase { rotate: 45, ..base };
    if VecEntry::seal(bad).err() == Some(VecError::BadRotate) {
        caught += 1;
    }

    // 3) 页数为 0。
    let bad = LayoutCase { pages: 0, ..base };
    if VecEntry::seal(bad).err() == Some(VecError::BadPages) {
        caught += 1;
    }

    // 4) 用例号为 0。
    let bad = LayoutCase { id: 0, ..base };
    if VecEntry::seal(bad).err() == Some(VecError::Range) {
        caught += 1;
    }

    // 5) 校验和被翻转。
    let mut book = VecBook::with_default_capacity();
    if let Ok(mut e) = VecEntry::seal(LayoutCase { id: 5, ..base }) {
        e.sum ^= 1;
        if book.admit(e).err() == Some(VecError::BadSum) {
            caught += 1;
        }
    }

    // 6) 重复用例号。
    let mut book6 = VecBook::with_default_capacity();
    if let Ok(e) = VecEntry::seal(base) {
        let _ = book6.admit(e.clone());
        if book6.admit(e).err() == Some(VecError::Duplicate) {
            caught += 1;
        }
    }

    // 7) 导入一条规格指纹被改的记录，全量复核抓 Drift。
    if let Ok(mut e) = VecEntry::seal(LayoutCase { id: 2, ..base }) {
        e.head = e.head ^ 0xFFFF;
        let mut b2 = VecBook::with_default_capacity();
        let _ = b2.import(&[e]);
        if b2.verify_all().err() == Some(VecError::Drift) {
            caught += 1;
        }
    }

    // 8) 导入一条面数被改的记录，全量复核抓 Sheet。
    if let Ok(mut e) = VecEntry::seal(LayoutCase { id: 3, ..base }) {
        e.sheets = e.sheets.saturating_add(1);
        let mut b3 = VecBook::with_default_capacity();
        let _ = b3.import(&[e]);
        if b3.verify_all().err() == Some(VecError::Sheet) {
            caught += 1;
        }
    }

    // 9) 空导入。
    let mut b4 = VecBook::with_default_capacity();
    let empty: [VecEntry; 0] = [];
    if b4.import(&empty).err() == Some(VecError::Empty) {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50038 自检：15 条判据。
pub fn run_unx50038_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50038");

    // J1-1 生成器闭集：5 档 N-up × 2 单双面 × 4 旋转 = 40 条，id 连续。
    let cases = match generate_cases(Paper::A4, 9) {
        Ok(v) => v,
        Err(_) => Vec::new(),
    };
    let mut contiguous = cases.len() == NUPS.len() * 2 * ROTATIONS.len();
    let mut i = 0usize;
    while i < cases.len() {
        match cases.get(i) {
            Some(c) => {
                if c.id != (i as u16) + 1 {
                    contiguous = false;
                }
            }
            None => contiguous = false,
        }
        i += 1;
    }
    let det = format!("cases={}", cases.len());
    s.add_dyn("50038-J1-用例生成", contiguous, &det);

    // J1-2 网格分解：每档 N-up 的行列乘积恰等于 N-up 值。
    let mut product_ok = true;
    let mut ni = 0usize;
    while ni < NUPS.len() {
        if let Some(nup) = NUPS.get(ni) {
            let c = LayoutCase { id: 1, paper: Paper::A4, nup: *nup, duplex: false, rotate: 0, pages: 9 };
            match grid_for(&c) {
                Ok(g) => {
                    if (g.cols as u16) * (g.rows as u16) != *nup as u16 {
                        product_ok = false;
                    }
                }
                Err(_) => product_ok = false,
            }
        }
        ni += 1;
    }
    s.add("50038-J1-网格分档", product_ok, if product_ok { "" } else { "行列乘积与 N-up 不符" });

    // J1-3 旋转语义：90° 网格是 0° 的转置，180° 与 0° 完全一致。
    let c0 = LayoutCase { id: 1, paper: Paper::A4, nup: 6, duplex: false, rotate: 0, pages: 8 };
    let c90 = LayoutCase { rotate: 90, ..c0 };
    let c180 = LayoutCase { rotate: 180, ..c0 };
    let g0 = grid_for(&c0).ok();
    let g90 = grid_for(&c90).ok();
    let g180 = grid_for(&c180).ok();
    let ok = match (g0, g90, g180) {
        (Some(a), Some(b), Some(c)) => {
            a.cols == b.rows && a.rows == b.cols && a.cell_w10 == b.cell_h10 && a.cell_h10 == b.cell_w10 && a == c
        }
        _ => false,
    };
    s.add("50038-J1-旋转转置", ok, if ok { "" } else { "旋转网格不是转置" });

    // J1-4 格尺寸守恒：cols*cell + (cols-1)*gutter + 2*margin ≤ 纸宽，且余量 < cols。
    let mut fit_ok = true;
    let mut k = 0usize;
    while k < cases.len() {
        if let Some(c) = cases.get(k) {
            match grid_for(c) {
                Ok(g) => {
                    let (pw, ph) = c.paper.tenths();
                    let swapped = c.rotate == 90 || c.rotate == 270;
                    let (uw, uh) = if swapped { (ph, pw) } else { (pw, ph) };
                    let used_w = g.cols as i32 * g.cell_w10 as i32 + (g.cols as i32 - 1) * GUTTER10 + 2 * MARGIN10;
                    let used_h = g.rows as i32 * g.cell_h10 as i32 + (g.rows as i32 - 1) * GUTTER10 + 2 * MARGIN10;
                    let slack_w = uw - used_w;
                    let slack_h = uh - used_h;
                    if used_w > uw || used_h > uh || slack_w >= g.cols as i32 || slack_h >= g.rows as i32 {
                        fit_ok = false;
                    }
                }
                Err(_) => fit_ok = false,
            }
        }
        k += 1;
    }
    s.add("50038-J1-格子守恒", fit_ok, if fit_ok { "" } else { "格子溢出或余量异常" });

    // J1-5 页号守恒：计划里 1..=pages 每页恰出现一次。
    let mut perm_ok = cases.len() == 40;
    let mut ci = 0usize;
    while ci < cases.len() {
        if let Some(c) = cases.get(ci) {
            match plan(c) {
                Ok(p) => {
                    let mut seen: Vec<u16> = Vec::with_capacity(c.pages as usize);
                    let mut sh = 0usize;
                    while sh < p.sheets.len() {
                        if let Some(sheet) = p.sheets.get(sh) {
                            let mut q = 0usize;
                            while q < sheet.cells.len() {
                                if let Some(v) = sheet.cells.get(q) {
                                    if *v > 0 {
                                        seen.push(*v as u16);
                                    }
                                }
                                q += 1;
                            }
                        }
                        sh += 1;
                    }
                    if seen.len() != c.pages as usize {
                        perm_ok = false;
                    }
                    let mut want = 1u16;
                    while want <= c.pages {
                        let mut hit = 0usize;
                        let mut z = 0usize;
                        while z < seen.len() {
                            if seen.get(z) == Some(&want) {
                                hit += 1;
                            }
                            z += 1;
                        }
                        if hit != 1 {
                            perm_ok = false;
                        }
                        want += 1;
                    }
                }
                Err(_) => perm_ok = false,
            }
        }
        ci += 1;
    }
    let det = format!("checked={}", cases.len());
    s.add_dyn("50038-J1-页号守恒", perm_ok, &det);

    // J1-6 空白只在尾部：同一印面出现 0 之后不得再有页号。
    let tail_ok = {
        let mut good = true;
        let mut a = 0usize;
        while a < cases.len() {
            if let Some(c) = cases.get(a) {
                if let Ok(p) = plan(c) {
                    let mut sh = 0usize;
                    while sh < p.sheets.len() {
                        if let Some(sheet) = p.sheets.get(sh) {
                            let mut blank_seen = false;
                            let mut q = 0usize;
                            while q < sheet.cells.len() {
                                if let Some(v) = sheet.cells.get(q) {
                                    if *v == 0 {
                                        blank_seen = true;
                                    } else if blank_seen {
                                        good = false;
                                    }
                                }
                                q += 1;
                            }
                        }
                        sh += 1;
                    }
                }
            }
            a += 1;
        }
        good
    };
    s.add("50038-J1-空白在尾", tail_ok, if tail_ok { "" } else { "空白格夹在页号之间" });

    // J1-7 印面数公式：纸数 = ceil(pages/(nup×面数))，印面数 = 纸数 × 面数。
    let mut sheet_ok = true;
    let mut x = 0usize;
    while x < cases.len() {
        if let Some(c) = cases.get(x) {
            if let Ok(p) = plan(c) {
                let sides = if c.duplex { 2 } else { 1 };
                let capacity = c.nup as usize * sides;
                let sheets_of_paper = (c.pages as usize + capacity - 1) / capacity;
                let want = sheets_of_paper * sides;
                if p.sheets.len() != want {
                    sheet_ok = false;
                }
            }
        }
        x += 1;
    }
    s.add("50038-J1-印面公式", sheet_ok, if sheet_ok { "" } else { "印面数与公式不符" });

    // J1-8 双面翻背前提：背面只要有页号，同一张纸的正面必须已满。
    let mut front_ok = true;
    let mut y = 0usize;
    while y < cases.len() {
        if let Some(c) = cases.get(y) {
            if let Ok(p) = plan(c) {
                let mut sh = 0usize;
                while sh < p.sheets.len() {
                    if let Some(sheet) = p.sheets.get(sh) {
                        if sheet.back {
                            let mut back_has_page = false;
                            let mut q = 0usize;
                            while q < sheet.cells.len() {
                                if let Some(v) = sheet.cells.get(q) {
                                    if *v > 0 {
                                        back_has_page = true;
                                    }
                                }
                                q += 1;
                            }
                            if back_has_page {
                                let mut front_idx: Option<&Sheet> = None;
                                let mut m = 0usize;
                                while m < p.sheets.len() {
                                    if let Some(f) = p.sheets.get(m) {
                                        if f.index == sheet.index && !f.back {
                                            front_idx = Some(f);
                                        }
                                    }
                                    m += 1;
                                }
                                let mut all_full = false;
                                if let Some(f) = front_idx {
                                    all_full = true;
                                    let mut z = 0usize;
                                    while z < f.cells.len() {
                                        if f.cells.get(z) == Some(&0) {
                                            all_full = false;
                                        }
                                        z += 1;
                                    }
                                }
                                if !all_full {
                                    front_ok = false;
                                }
                            }
                        }
                    }
                    sh += 1;
                }
            }
        }
        y += 1;
    }
    s.add("50038-J1-翻背有序", front_ok, if front_ok { "" } else { "正面未满就翻背" });

    // J1-9 用例带校验和落账：40 条全部 admit，账本 40 条、根指纹非零。
    let book = sealed_book(9);
    let ok = book.len() == 40 && book.rejected() == 0 && book.root() != 0 && book.total_pages() == 40 * 9;
    let det = format!("len={} root={:08x}", book.len(), book.root());
    s.add_dyn("50038-J1-带校验和落账", ok, &det);

    // J1-10 全量复核通过：40 条重算后逐条一致。
    let verified = book.verify_all();
    let ok = match verified {
        Ok(n) => n == 40,
        Err(_) => false,
    };
    let det = match verified {
        Ok(n) => format!("verified={}", n),
        Err(e) => format!("verify err={:#x}", e.code()),
    };
    s.add_dyn("50038-J1-全量复核", ok, &det);

    // J1-11 指纹确定性：同规格两次封装，sum/head 逐位一致。
    let c = LayoutCase { id: 7, paper: Paper::A3, nup: 9, duplex: true, rotate: 270, pages: 17 };
    let a = VecEntry::seal(c);
    let b = VecEntry::seal(c);
    let ok = match (a, b) {
        (Ok(e1), Ok(e2)) => e1.sum == e2.sum && e1.head == e2.head && e1.sheets == e2.sheets,
        _ => false,
    };
    s.add("50038-J1-指纹确定", ok, if ok { "" } else { "同规格指纹不一致" });

    // J1-12 根指纹敏感：换掉一条用例的页数，根必须变。
    let mut alt = VecBook::with_default_capacity();
    let mut j = 0usize;
    while j < cases.len() {
        if let Some(cc) = cases.get(j) {
            let mutated = if cc.id == 1 { LayoutCase { pages: 10, ..*cc } } else { *cc };
            if let Ok(e) = VecEntry::seal(mutated) {
                let _ = alt.admit(e);
            }
        }
        j += 1;
    }
    let ok = alt.len() == book.len() && alt.root() != book.root() && alt.verify_all().is_ok();
    let det = format!("root={:08x} alt={:08x}", book.root(), alt.root());
    s.add_dyn("50038-J1-根指纹敏感", ok, &det);

    // J1-13 篡改必拒：翻转一位校验和即 BadSum，账本长度不变。
    let mut tamper = VecBook::with_default_capacity();
    let mut caught_bad = false;
    if let Ok(e) = VecEntry::seal(c) {
        let mut e2 = e.clone();
        let _ = tamper.admit(e);
        e2.case.id = e2.case.id.saturating_add(1);
        e2.sum = e2.sum.wrapping_add(0x0100_0000);
        caught_bad = tamper.admit(e2).err() == Some(VecError::BadSum);
    }
    let ok = caught_bad && tamper.len() == 1 && tamper.verify_all().is_ok();
    s.add("50038-J1-篡改必拒", ok, if ok { "" } else { "被改的条目进了账" });

    // J1-14 容量防线：容量 3 的账本第 4 条被 Full 挡回，前 3 条完好。
    let mut small = VecBook::new(3);
    let mut admitted = 0usize;
    let mut t = 0usize;
    while t < cases.len() && admitted < 3 {
        if let Some(cc) = cases.get(t) {
            if let Ok(e) = VecEntry::seal(*cc) {
                if small.admit(e).is_ok() {
                    admitted += 1;
                }
            }
        }
        t += 1;
    }
    let overflow = match cases.get(t) {
        Some(cc) => match VecEntry::seal(*cc) {
            Ok(e) => small.admit(e).err() == Some(VecError::Full),
            Err(_) => false,
        },
        None => false,
    };
    let ok = admitted == 3 && small.len() == 3 && overflow && small.rejected() == 1;
    let det = format!("len={} rej={}", small.len(), small.rejected());
    s.add_dyn("50038-J1-容量防线", ok, &det);

    // J1-15 九类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50038-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50038_checks_green() {
        let set = run_unx50038_checks();
        assert!(set.all_passed(), "UNX-F50038 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50038_book_verifies() {
        let b = sealed_book(9);
        assert_eq!(b.len(), 40);
        assert_eq!(b.verify_all().ok(), Some(40));
        assert_eq!(b.total_pages(), 360);
    }

    #[test]
    fn unx50038_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 9);
    }
}
