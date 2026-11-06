//! UNX-F50061 双面三模语义总账（单面 / 长边翻 / 短边翻）（0x433c0 段）
//!
//! 锚点：UNX-F50061-J1 三模 ×30 页序/翻向逐项一致；判据锚定 §AI-63 §2 专题四。
//!
//! 数据段：`SEG_BASE = 0x433c0`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 11`，
//! 逐项见 [`DuplexError`]。
//!
//! 双面语义只有一个真问题：**同一张纸的两面在翻过之后，哪一页朝哪儿**。
//! 三个模式的区别不在页序，而在翻向：
//! * 单面（Simplex）——每张纸只用正面，背面按定义留白；
//! * 长边翻（LongEdge）——像翻书，绕长边翻页，背面内容在翻过后仍正立，
//!   故背面旋转 `R0`；
//! * 短边翻（ShortEdge）——像翻台历，绕短边翻页，背面在翻过后上下颠倒，
//!   故背面旋转 `R180`。
//!
//! 页序三模相同（第 k 张纸正面 = 2k-1、背面 = 2k），差异全部落在
//! [`Imposition::back_rot`] 上。判据因此分两半：**页序逐项一致**（用闭式参考表
//! 独立复算，不信调度器的自述）与**翻向逐项一致**（30 页逐页比对旋转）。
//!
//! 结构清单：
//! * [`Rot`]：象限旋转，Z4 群（合成 / 逆 / 作用于整点），几何与代数双口径；
//! * [`Mode`] + [`Side`] + [`Slot`] + [`Sheet`]：单张纸的两面布局；
//! * [`Imposition`]：三模拼版调度器（支持「从背面起印」偏移）；
//! * [`FaceRow`] + [`FacingLedger`]：逐页总账（物理键双射 + 阅读序连续 + 封账只读）；
//! * [`injection_core`]：7 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x433c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x433c0;

const E_ZERO_PAGES: u16 = 1;
const E_TOO_MANY: u16 = 2;
const E_MODE: u16 = 3;
const E_ROT: u16 = 4;
const E_CONFLICT: u16 = 5;
const E_GAP: u16 = 6;
const E_BOUNDS: u16 = 7;
const E_FULL: u16 = 8;
const E_DUPLICATE: u16 = 9;
const E_IMMUTABLE: u16 = 10;
const E_BAD_BLANK: u16 = 11;

/// 双面语意错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplexError {
    /// 页数为 0。
    ZeroPages,
    /// 页数超过拼版上限。
    PageOverflow,
    /// 模式码不在闭集 0..=2。
    ModeCode,
    /// 旋转码不在闭集 0..=3。
    RotCode,
    /// 同一物理键（纸号 + 面）被两个页占用。
    SlotConflict,
    /// 阅读序出现跳号（账本必须逐行连续）。
    OrderGap,
    /// 拼版超出纸槽容量（含偏移后的总槽数）。
    Bounds,
    /// 总账容量已满。
    Full,
    /// 同一页被排进两个位置。
    Duplicate,
    /// 封账后只读。
    Immutable,
    /// 空白槽带页码，或非空槽页码为 0。
    BadBlank,
}

impl DuplexError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            DuplexError::ZeroPages => E_ZERO_PAGES,
            DuplexError::PageOverflow => E_TOO_MANY,
            DuplexError::ModeCode => E_MODE,
            DuplexError::RotCode => E_ROT,
            DuplexError::SlotConflict => E_CONFLICT,
            DuplexError::OrderGap => E_GAP,
            DuplexError::Bounds => E_BOUNDS,
            DuplexError::Full => E_FULL,
            DuplexError::Duplicate => E_DUPLICATE,
            DuplexError::Immutable => E_IMMUTABLE,
            DuplexError::BadBlank => E_BAD_BLANK,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            DuplexError::ZeroPages => "page count is zero",
            DuplexError::PageOverflow => "page count above imposition limit",
            DuplexError::ModeCode => "duplex mode code outside closed set",
            DuplexError::RotCode => "rotation code outside closed set",
            DuplexError::SlotConflict => "physical slot already occupied",
            DuplexError::OrderGap => "reading order gap",
            DuplexError::Bounds => "imposition exceeds sheet capacity",
            DuplexError::Full => "facing ledger capacity reached",
            DuplexError::Duplicate => "page imposed twice",
            DuplexError::Immutable => "ledger is sealed read-only",
            DuplexError::BadBlank => "blank slot carries a page number",
        }
    }
}

/// 象限旋转（Z4 群）。只允许 0/90/180/270 四档——任意角旋转在双面上没有
/// 语义（纸只能翻不能转任意角），把它关在闭集里比事后夹取更安全。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rot {
    /// 不正立（0°）。
    R0,
    /// 逆时针 90°。
    R90,
    /// 180°。
    R180,
    /// 逆时针 270°（= 顺时针 90°）。
    R270,
}

impl Rot {
    /// 象限码 0..=3。
    pub fn code(self) -> u8 {
        match self {
            Rot::R0 => 0,
            Rot::R90 => 1,
            Rot::R180 => 2,
            Rot::R270 => 3,
        }
    }

    /// 象限码反解；闭集外返回 [`DuplexError::RotCode`]。
    pub fn from_code(c: u8) -> Result<Rot, DuplexError> {
        match c {
            0 => Ok(Rot::R0),
            1 => Ok(Rot::R90),
            2 => Ok(Rot::R180),
            3 => Ok(Rot::R270),
            _ => Err(DuplexError::RotCode),
        }
    }

    /// 由象限码构造（内部用，码已保证 0..=3）。
    fn from_quad(q: u8) -> Rot {
        match q % 4 {
            1 => Rot::R90,
            2 => Rot::R180,
            3 => Rot::R270,
            _ => Rot::R0,
        }
    }

    /// 合成：`self.compose(other)` = 先转 `other` 再转 `self`（`self ∘ other`）。
    /// Z4 可交换，故顺序对结果无影响，但语义仍按函数复合写清。
    pub fn compose(self, other: Rot) -> Rot {
        Rot::from_quad(self.code() + other.code())
    }

    /// 逆元（Z4 里 180° 自逆，90/270 互逆）。
    pub fn inverse(self) -> Rot {
        match self {
            Rot::R0 => Rot::R0,
            Rot::R90 => Rot::R270,
            Rot::R180 => Rot::R180,
            Rot::R270 => Rot::R90,
        }
    }

    /// 作用于整点（毫米坐标按象限旋转）。逆时针为正：
    /// 90° → (x,y) ↦ (-y,x)；180° → (-x,-y)；270° → (y,-x)。
    pub fn apply(self, p: (i32, i32)) -> (i32, i32) {
        let (x, y) = p;
        match self {
            Rot::R0 => (x, y),
            Rot::R90 => (-y, x),
            Rot::R180 => (-x, -y),
            Rot::R270 => (y, -x),
        }
    }

    /// 档名（日志用）。
    pub fn label(self) -> &'static str {
        match self {
            Rot::R0 => "0",
            Rot::R90 => "90",
            Rot::R180 => "180",
            Rot::R270 => "270",
        }
    }

    /// 全部四档（遍历用）。
    pub const ALL: [Rot; 4] = [Rot::R0, Rot::R90, Rot::R180, Rot::R270];
}

/// 面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// 正面（先印）。
    Front,
    /// 背面（翻过之后印）。
    Back,
}

impl Side {
    /// 面序：正面 0、背面 1（打印序按它升序）。
    pub fn index(self) -> u8 {
        match self {
            Side::Front => 0,
            Side::Back => 1,
        }
    }

    /// 面名。
    pub fn label(self) -> &'static str {
        match self {
            Side::Front => "front",
            Side::Back => "back",
        }
    }

    /// 反面。
    pub fn flip(self) -> Side {
        match self {
            Side::Front => Side::Back,
            Side::Back => Side::Front,
        }
    }
}

/// 翻向轴。长边翻 = 绕长边（竖轴）翻，短边翻 = 绕短边（横轴）翻。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlipAxis {
    /// 不翻（单面）。
    None,
    /// 绕长边翻（书本式）。
    Long,
    /// 绕短边翻（台历式）。
    Short,
}

impl FlipAxis {
    /// 轴名。
    pub fn label(self) -> &'static str {
        match self {
            FlipAxis::None => "none",
            FlipAxis::Long => "long",
            FlipAxis::Short => "short",
        }
    }
}

/// 三模。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 单面：每张纸只用正面。
    Simplex,
    /// 双面 · 长边翻。
    LongEdge,
    /// 双面 · 短边翻。
    ShortEdge,
}

impl Mode {
    /// 模式码 0..=2。
    pub fn code(self) -> u8 {
        match self {
            Mode::Simplex => 0,
            Mode::LongEdge => 1,
            Mode::ShortEdge => 2,
        }
    }

    /// 模式码反解；闭集外返回 [`DuplexError::ModeCode`]，不兜底到单面。
    pub fn from_code(c: u8) -> Result<Mode, DuplexError> {
        match c {
            0 => Ok(Mode::Simplex),
            1 => Ok(Mode::LongEdge),
            2 => Ok(Mode::ShortEdge),
            _ => Err(DuplexError::ModeCode),
        }
    }

    /// 模式名。
    pub fn label(self) -> &'static str {
        match self {
            Mode::Simplex => "simplex",
            Mode::LongEdge => "duplex-long-edge",
            Mode::ShortEdge => "duplex-short-edge",
        }
    }

    /// 是否双面。
    pub fn is_duplex(self) -> bool {
        matches!(self, Mode::LongEdge | Mode::ShortEdge)
    }

    /// 每张纸可承载的页数（单面 1，双面 2）。
    pub fn pages_per_sheet(self) -> u8 {
        if self.is_duplex() {
            2
        } else {
            1
        }
    }

    /// 翻向轴。
    pub fn axis(self) -> FlipAxis {
        match self {
            Mode::Simplex => FlipAxis::None,
            Mode::LongEdge => FlipAxis::Long,
            Mode::ShortEdge => FlipAxis::Short,
        }
    }

    /// 背面旋转：长边翻背面仍正立（R0），短边翻背面上下颠倒（R180）。
    /// 这两个值是「翻向」的全部信息量，也是三模唯一的语义差异。
    pub fn back_rot(self) -> Rot {
        match self {
            Mode::Simplex => Rot::R0,
            Mode::LongEdge => Rot::R0,
            Mode::ShortEdge => Rot::R180,
        }
    }

    /// 全部三模。
    pub const ALL: [Mode; 3] = [Mode::Simplex, Mode::LongEdge, Mode::ShortEdge];
}

/// 一个页槽（纸的一面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// 页码（0 表示空白槽）。
    pub page: u16,
    /// 所在面。
    pub side: Side,
    /// 该面内容的旋转。
    pub rot: Rot,
    /// 是否空白（与 `page == 0` 必须同真同假，见 [`Slot::well_formed`]）。
    pub blank: bool,
}

impl Slot {
    /// 有页的槽。
    pub const fn page_slot(page: u16, side: Side, rot: Rot) -> Slot {
        Slot {
            page,
            side,
            rot,
            blank: false,
        }
    }

    /// 空白槽（空白槽不带旋转：给它一个旋转等于「白纸上画了个方向」，无意义）。
    pub const fn blank_slot(side: Side) -> Slot {
        Slot {
            page: 0,
            side,
            rot: Rot::R0,
            blank: true,
        }
    }

    /// 空白标志与页码一致、且空白槽旋转为零。
    pub fn well_formed(&self) -> bool {
        if self.blank {
            self.page == 0 && self.rot == Rot::R0
        } else {
            self.page != 0
        }
    }
}

/// 一张纸（正反两面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sheet {
    /// 纸号（0 起）。
    pub index: u16,
    /// 正面。
    pub front: Slot,
    /// 背面。
    pub back: Slot,
}

impl Sheet {
    /// 取某一面。
    pub fn side(&self, s: Side) -> Slot {
        match s {
            Side::Front => self.front,
            Side::Back => self.back,
        }
    }

    /// 本张纸有几页（非空槽数）。
    pub fn pages(&self) -> u8 {
        let mut n = 0u8;
        if !self.front.blank {
            n += 1;
        }
        if !self.back.blank {
            n += 1;
        }
        n
    }
}

/// 页数上限（A4 手册级别的拼版上限；超过即显式失败而不是慢慢吃内存）。
pub const MAX_PAGES: u16 = 10_000;
/// 纸槽上限（单面一页一张，故与页数上限同阶）。
pub const MAX_SHEETS: u16 = 10_000;
/// 总账容量上限。
pub const MAX_ROWS: usize = 16_384;

/// 三模拼版调度器。
pub struct Imposition {
    mode: Mode,
    pages: u16,
    start_on_back: bool,
    sheets: Vec<Sheet>,
}

impl Imposition {
    /// 从正面起印。
    pub fn plan(pages: u16, mode: Mode) -> Result<Imposition, DuplexError> {
        Imposition::plan_offset(pages, mode, false)
    }

    /// 指定是否从背面起印（`start_on_back=true` 时首张纸正面留白）。
    ///
    /// 为什么需要它：双面机在上一任务留下的半张纸上续印、或单面机反装纸时，
    /// 第一页会落在背面；不显式建模就只能让调用方自己「补一页白」，而补页
    /// 会把页序整体推后一页，判据再也对不上。
    pub fn plan_offset(
        pages: u16,
        mode: Mode,
        start_on_back: bool,
    ) -> Result<Imposition, DuplexError> {
        if pages == 0 {
            return Err(DuplexError::ZeroPages);
        }
        if pages > MAX_PAGES {
            return Err(DuplexError::PageOverflow);
        }
        let sheets_needed = Imposition::sheet_count_for(pages, mode, start_on_back);
        if sheets_needed == 0 || sheets_needed > MAX_SHEETS {
            return Err(DuplexError::Bounds);
        }
        let mut sheets = Vec::with_capacity(sheets_needed as usize);
        let mut remaining = pages;
        let mut next_page: u16 = 1;
        let mut skip = if start_on_back { 1u8 } else { 0u8 };
        let mut idx: u16 = 0;
        while remaining > 0 {
            let mut front = Slot::blank_slot(Side::Front);
            let mut back = Slot::blank_slot(Side::Back);
            if skip > 0 {
                skip -= 1;
            } else {
                front = Slot::page_slot(next_page, Side::Front, Rot::R0);
                next_page = next_page.saturating_add(1);
                remaining -= 1;
            }
            if mode.is_duplex() && remaining > 0 {
                if skip > 0 {
                    skip -= 1;
                } else {
                    back = Slot::page_slot(next_page, Side::Back, mode.back_rot());
                    next_page = next_page.saturating_add(1);
                    remaining -= 1;
                }
            }
            sheets.push(Sheet { index: idx, front, back });
            idx = idx.saturating_add(1);
        }
        let imp = Imposition {
            mode,
            pages,
            start_on_back,
            sheets,
        };
        imp.validate()?;
        Ok(imp)
    }

    /// 需要的纸张数（闭式；与 [`Imposition::plan_offset`] 的构表循环必须同解）。
    pub fn sheet_count_for(pages: u16, mode: Mode, start_on_back: bool) -> u16 {
        let sides = pages as u32 + if start_on_back { 1 } else { 0 };
        let sheets = if mode.is_duplex() {
            (sides + 1) / 2
        } else {
            sides
        };
        if sheets > u16::MAX as u32 {
            u16::MAX
        } else {
            sheets as u16
        }
    }

    /// 结构自检：槽良构、页唯一、纸号连续。
    pub fn validate(&self) -> Result<(), DuplexError> {
        let mut seen = Vec::with_capacity(self.pages as usize);
        let mut i = 0usize;
        while i < self.sheets.len() {
            let sh = match self.sheets.get(i) {
                Some(v) => *v,
                None => return Err(DuplexError::BadBlank),
            };
            if sh.index as usize != i {
                return Err(DuplexError::Bounds);
            }
            let sides = [sh.front, sh.back];
            let mut k = 0usize;
            while k < sides.len() {
                if let Some(sl) = sides.get(k) {
                    if !sl.well_formed() {
                        return Err(DuplexError::BadBlank);
                    }
                    if sl.page != 0 {
                        let mut dup = false;
                        let mut j = 0usize;
                        while j < seen.len() {
                            if let Some(p) = seen.get(j) {
                                if *p == sl.page {
                                    dup = true;
                                }
                            }
                            j += 1;
                        }
                        if dup {
                            return Err(DuplexError::Duplicate);
                        }
                        if self.mode.is_duplex() && k == 0 && sl.side != Side::Front {
                            return Err(DuplexError::BadBlank);
                        }
                        seen.push(sl.page);
                    }
                }
                k += 1;
            }
            i += 1;
        }
        if seen.len() != self.pages as usize {
            return Err(DuplexError::Bounds);
        }
        Ok(())
    }

    /// 模式。
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// 页数。
    pub fn pages(&self) -> u16 {
        self.pages
    }

    /// 是否从背面起印。
    pub fn start_on_back(&self) -> bool {
        self.start_on_back
    }

    /// 纸张数。
    pub fn sheet_count(&self) -> usize {
        self.sheets.len()
    }

    /// 取第 `i` 张纸（越界 `None`）。
    pub fn sheet(&self, i: usize) -> Option<&Sheet> {
        self.sheets.get(i)
    }

    /// 背面旋转（三模语义的唯一差异点）。
    pub fn back_rot(&self) -> Rot {
        self.mode.back_rot()
    }

    /// 空白槽数。
    pub fn blank_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.sheets.len() {
            if let Some(sh) = self.sheets.get(i) {
                if sh.front.blank {
                    n += 1;
                }
                if sh.back.blank {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 页码 → (纸号, 面)。闭式反查，用于与账本交叉校验。
    pub fn slot_of(&self, page: u16) -> Option<(u16, Side)> {
        if page == 0 || page > self.pages {
            return None;
        }
        let extra = if self.start_on_back { 1u16 } else { 0 };
        if self.mode.is_duplex() {
            let k = page - 1 + extra;
            let sheet = k / 2;
            let side = if k % 2 == 0 { Side::Front } else { Side::Back };
            Some((sheet, side))
        } else {
            Some((page - 1 + extra, Side::Front))
        }
    }

    /// 打印序：按纸号升序、正面先于背面，导出有页的槽的页码序列。
    pub fn emit_order(&self) -> Vec<u16> {
        let mut out = Vec::with_capacity(self.pages as usize);
        let mut i = 0usize;
        while i < self.sheets.len() {
            if let Some(sh) = self.sheets.get(i) {
                if !sh.front.blank {
                    out.push(sh.front.page);
                }
                if !sh.back.blank {
                    out.push(sh.back.page);
                }
            }
            i += 1;
        }
        out
    }

    /// 拼版指纹：按纸序串 正/背面页码与旋转码。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.sheets.len() {
            if let Some(sh) = self.sheets.get(i) {
                h = fnv_u16(h, sh.index);
                h = fnv_u16(h, sh.front.page);
                h = fnv_byte(h, sh.front.rot.code());
                h = fnv_byte(h, sh.front.blank as u8);
                h = fnv_u16(h, sh.back.page);
                h = fnv_byte(h, sh.back.rot.code());
                h = fnv_byte(h, sh.back.blank as u8);
            }
            i += 1;
        }
        h
    }

    /// 落账文本（逐张纸一行）。
    pub fn describe(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.sheets.len() {
            if let Some(sh) = self.sheets.get(i) {
                out.push_str(&format!(
                    "sheet{} {}({}) {}({})\n",
                    sh.index,
                    if sh.front.blank {
                        "--".to_string()
                    } else {
                        sh.front.page.to_string()
                    },
                    sh.front.rot.label(),
                    if sh.back.blank {
                        "--".to_string()
                    } else {
                        sh.back.page.to_string()
                    },
                    sh.back.rot.label()
                ));
            }
            i += 1;
        }
        out
    }
}

/// 总账一行：一页落在哪张纸的哪一面、旋转多少、阅读序第几。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceRow {
    /// 页码（1 起）。
    pub page: u16,
    /// 纸号。
    pub sheet: u16,
    /// 面。
    pub side: Side,
    /// 旋转。
    pub rot: Rot,
    /// 阅读序（0 起，必须与入账位置一致）。
    pub order: u16,
}

impl FaceRow {
    /// 物理键：纸号 × 2 + 面序。双射判据就比这个键。
    pub fn key(&self) -> u32 {
        (self.sheet as u32) * 2 + self.side.index() as u32
    }

    /// 一行落账文本。
    pub fn ledger_line(&self) -> String {
        format!(
            "#{} p{} s{} {} rot{}",
            self.order,
            self.page,
            self.sheet,
            self.side.label(),
            self.rot.label()
        )
    }
}

/// 逐页总账：物理键双射 + 阅读序连续 + 页唯一 + 封账只读。
pub struct FacingLedger {
    rows: Vec<FaceRow>,
    capacity: usize,
    sealed: bool,
    rejected: u32,
}

impl FacingLedger {
    /// 建空账；容量 0 按 1 处理，上限 [`MAX_ROWS`]。
    pub fn new(capacity: usize) -> FacingLedger {
        let cap = if capacity == 0 { 1 } else { capacity.min(MAX_ROWS) };
        FacingLedger {
            rows: Vec::with_capacity(cap),
            capacity: cap,
            sealed: false,
            rejected: 0,
        }
    }

    fn reject<T>(&mut self, e: DuplexError) -> Result<T, DuplexError> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    /// 入账一行：封账 → 容量 → 页码 → 阅读序连续 → 物理键唯一 → 页唯一。
    pub fn insert(&mut self, row: FaceRow) -> Result<(), DuplexError> {
        if self.sealed {
            return self.reject(DuplexError::Immutable);
        }
        if self.rows.len() >= self.capacity {
            return self.reject(DuplexError::Full);
        }
        if row.page == 0 {
            return self.reject(DuplexError::BadBlank);
        }
        if row.order as usize != self.rows.len() {
            return self.reject(DuplexError::OrderGap);
        }
        let key = row.key();
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.key() == key {
                    return self.reject(DuplexError::SlotConflict);
                }
                if r.page == row.page {
                    return self.reject(DuplexError::Duplicate);
                }
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 全表复核：阅读序连续、物理键唯一、页唯一、键落在纸槽范围内。
    pub fn verify(&self) -> Result<usize, DuplexError> {
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DuplexError::OrderGap),
            };
            if r.order as usize != i || r.page == 0 {
                return Err(DuplexError::OrderGap);
            }
            let mut j = i + 1;
            while j < self.rows.len() {
                if let Some(o) = self.rows.get(j) {
                    if o.key() == r.key() {
                        return Err(DuplexError::SlotConflict);
                    }
                    if o.page == r.page {
                        return Err(DuplexError::Duplicate);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(self.rows.len())
    }

    /// 封账：之后入账一律 [`DuplexError::Immutable`]。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否已封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 累计被拒次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 取第 `i` 行（越界 `None`）。
    pub fn row(&self, i: usize) -> Option<&FaceRow> {
        self.rows.get(i)
    }

    /// 按页查行。
    pub fn by_page(&self, page: u16) -> Option<&FaceRow> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.page == page {
                    return Some(r);
                }
            }
            i += 1;
        }
        None
    }

    /// 翻向分布：四档旋转各多少页（判据用它证明翻向一致）。
    pub fn rot_histogram(&self) -> [u16; 4] {
        let mut hist = [0u16; 4];
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if let Some(slot) = hist.get_mut(r.rot.code() as usize) {
                    *slot = slot.saturating_add(1);
                }
            }
            i += 1;
        }
        hist
    }

    /// 账面指纹：按入账序串 页 / 键 / 旋转。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fnv_u16(h, r.page);
                h = fnv_u16(h, r.sheet);
                h = fnv_byte(h, r.side.index());
                h = fnv_byte(h, r.rot.code());
                h = fnv_u16(h, r.order);
            }
            i += 1;
        }
        h
    }

    /// 落账文本。
    pub fn ledger_text(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                out.push_str(&r.ledger_line());
                out.push('\n');
            }
            i += 1;
        }
        out
    }
}

/// FNV-1a 偏移基。
pub const FNV_OFFSET: u32 = 0x811c_9dc5;
/// FNV-1a 质数。
pub const FNV_PRIME: u32 = 0x0100_0193;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

fn fnv_u16(h: u32, v: u16) -> u32 {
    fnv_byte(fnv_byte(h, (v >> 8) as u8), v as u8)
}

/// 把拼版逐页落账（按页码升序入账，故阅读序与入账序同构）。
pub fn build_ledger(imp: &Imposition) -> Result<FacingLedger, DuplexError> {
    let mut led = FacingLedger::new(imp.pages() as usize + 1);
    let mut page: u16 = 1;
    while page <= imp.pages() {
        let (sheet, side) = match imp.slot_of(page) {
            Some(v) => v,
            None => return Err(DuplexError::Bounds),
        };
        let sh = match imp.sheet(sheet as usize) {
            Some(v) => *v,
            None => return Err(DuplexError::Bounds),
        };
        let sl = sh.side(side);
        if sl.page != page || sl.blank {
            return Err(DuplexError::BadBlank);
        }
        led.insert(FaceRow {
            page,
            sheet,
            side,
            rot: sl.rot,
            order: page - 1,
        })?;
        page = page.saturating_add(1);
    }
    Ok(led)
}

/// 闭式参考表：由「页号 → 物理键」的定义直接算，用于独立复算调度器。
///
/// 与 [`Imposition::plan_offset`] 是两条独立推导：调度器逐张纸消耗页号，
/// 这里对每页直接解 k = page-1+offset。两者必须逐项同解，否则说明
/// 「顺序填充」与「闭式映射」有一个错了。
pub fn reference_slot(page: u16, mode: Mode, start_on_back: bool) -> Option<(u16, Side)> {
    if page == 0 {
        return None;
    }
    let extra = if start_on_back { 1u16 } else { 0 };
    let k = page - 1 + extra;
    if mode.is_duplex() {
        Some((k / 2, if k % 2 == 0 { Side::Front } else { Side::Back }))
    } else {
        Some((k, Side::Front))
    }
}

/// 注入核：7 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 7usize;

    // 1) 零页：拼版不接受空作业。
    if Imposition::plan(0, Mode::LongEdge).err() == Some(DuplexError::ZeroPages) {
        caught += 1;
    }

    // 2) 页数超上限。
    if Imposition::plan(MAX_PAGES + 1, Mode::Simplex).err() == Some(DuplexError::PageOverflow) {
        caught += 1;
    }

    // 3) 模式码越界：3 不在闭集里。
    if Mode::from_code(3).err() == Some(DuplexError::ModeCode) {
        caught += 1;
    }

    // 4) 旋转码越界：4 不在 Z4 里。
    if Rot::from_code(4).err() == Some(DuplexError::RotCode) {
        caught += 1;
    }

    // 5) 物理键冲突：两页占同一张纸的同一面。
    let mut led = FacingLedger::new(8);
    let a = FaceRow {
        page: 1,
        sheet: 0,
        side: Side::Front,
        rot: Rot::R0,
        order: 0,
    };
    let b = FaceRow {
        page: 2,
        sheet: 0,
        side: Side::Front,
        rot: Rot::R0,
        order: 1,
    };
    let first = led.insert(a).is_ok();
    if first && led.insert(b).err() == Some(DuplexError::SlotConflict) && led.len() == 1 {
        caught += 1;
    }

    // 6) 阅读序缺口：第二行 order 跳到 5。
    let mut gap = FacingLedger::new(8);
    let g0 = FaceRow {
        page: 1,
        sheet: 0,
        side: Side::Front,
        rot: Rot::R0,
        order: 0,
    };
    let g1 = FaceRow {
        page: 2,
        sheet: 0,
        side: Side::Back,
        rot: Rot::R0,
        order: 5,
    };
    let ok0 = gap.insert(g0).is_ok();
    if ok0 && gap.insert(g1).err() == Some(DuplexError::OrderGap) && gap.len() == 1 {
        caught += 1;
    }

    // 7) 空白槽带页码：page=0 的账行本身就是「把白纸当成一页」。
    let mut blank = FacingLedger::new(8);
    let bad = FaceRow {
        page: 0,
        sheet: 3,
        side: Side::Back,
        rot: Rot::R0,
        order: 0,
    };
    if blank.insert(bad).err() == Some(DuplexError::BadBlank) {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50061 自检：19 条判据。
pub fn run_unx50061_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50061");
    const N: u16 = 30;

    // J1-1 三模 ×30 页守恒：每页恰好一行、页集合 = 1..=30、阅读序连续。
    let mut conserved = true;
    let mut total_rows = 0usize;
    let mut mode_i = 0usize;
    while mode_i < Mode::ALL.len() {
        if let Some(m) = Mode::ALL.get(mode_i) {
            match Imposition::plan(N, *m) {
                Ok(imp) => match build_ledger(&imp) {
                    Ok(led) => {
                        total_rows += led.len();
                        if led.len() != N as usize {
                            conserved = false;
                        }
                        let mut p: u16 = 1;
                        while p <= N {
                            match led.by_page(p) {
                                Some(r) if r.page == p && r.order == p - 1 => {}
                                _ => conserved = false,
                            }
                            p += 1;
                        }
                        if led.verify() != Ok(N as usize) {
                            conserved = false;
                        }
                    }
                    Err(_) => conserved = false,
                },
                Err(_) => conserved = false,
            }
        }
        mode_i += 1;
    }
    let det = format!("rows={}/{}", total_rows, N as usize * 3);
    s.add_dyn("50061-J1-三模守恒", conserved, &det);

    // J1-2 页序逐项一致：调度器 vs 闭式参考表，三模 ×30 页逐项比对。
    let mut order_ok = true;
    let mut mismatches = 0u16;
    let mut mi = 0usize;
    while mi < Mode::ALL.len() {
        if let Some(m) = Mode::ALL.get(mi) {
            if let Ok(imp) = Imposition::plan(N, *m) {
                let mut p: u16 = 1;
                while p <= N {
                    let got = imp.slot_of(p);
                    let want = reference_slot(p, *m, false);
                    if got != want {
                        order_ok = false;
                        mismatches = mismatches.saturating_add(1);
                    }
                    p += 1;
                }
            } else {
                order_ok = false;
            }
        }
        mi += 1;
    }
    let det = format!("mismatch={}", mismatches);
    s.add_dyn("50061-J1-页序逐项", order_ok, &det);

    // J1-3 翻向逐项一致：长边翻 30 页背面全 R0、短边翻全 R180、正面恒 R0。
    let mut flip_ok = true;
    let mut mi2 = 0usize;
    while mi2 < Mode::ALL.len() {
        if let Some(m) = Mode::ALL.get(mi2) {
            if let Ok(imp) = Imposition::plan(N, *m) {
                let want = m.back_rot();
                let mut p: u16 = 1;
                while p <= N {
                    match imp.slot_of(p) {
                        Some((sheet, side)) => {
                            let sh = match imp.sheet(sheet as usize) {
                                Some(v) => *v,
                                None => {
                                    flip_ok = false;
                                    continue;
                                }
                            };
                            let sl = sh.side(side);
                            let expect_rot = if side == Side::Back { want } else { Rot::R0 };
                            if sl.rot != expect_rot {
                                flip_ok = false;
                            }
                            if *m == Mode::Simplex && side != Side::Front {
                                flip_ok = false;
                            }
                        }
                        None => flip_ok = false,
                    }
                    p += 1;
                }
            } else {
                flip_ok = false;
            }
        }
        mi2 += 1;
    }
    s.add("50061-J1-翻向逐项", flip_ok, if flip_ok { "" } else { "背面旋转与模式不符" });

    // J1-4 纸张数：单面 30 张、双面两模各 15 张；双面每张 2 页、单面 1 页。
    let mut count_ok = true;
    let mut mi3 = 0usize;
    while mi3 < Mode::ALL.len() {
        if let Some(m) = Mode::ALL.get(mi3) {
            match Imposition::plan(N, *m) {
                Ok(imp) => {
                    let want = Imposition::sheet_count_for(N, *m, false);
                    if imp.sheet_count() != want as usize {
                        count_ok = false;
                    }
                    if *m == Mode::Simplex && imp.sheet_count() != N as usize {
                        count_ok = false;
                    }
                    if m.is_duplex() && imp.sheet_count() != (N as usize + 1) / 2 {
                        count_ok = false;
                    }
                    let mut i = 0usize;
                    while i < imp.sheet_count() {
                        if let Some(sh) = imp.sheet(i) {
                            if sh.pages() != m.pages_per_sheet() {
                                count_ok = false;
                            }
                        }
                        i += 1;
                    }
                }
                Err(_) => count_ok = false,
            }
        }
        mi3 += 1;
    }
    s.add("50061-J1-纸张守恒", count_ok, if count_ok { "" } else { "纸张数与模式不符" });

    // J1-5 奇数页：31 页双面 → 16 张、恰 1 个空白（末张背面），页 31 在末张正面。
    let mut odd_ok = false;
    let mut odd_detail = String::new();
    if let Ok(imp) = Imposition::plan(31, Mode::ShortEdge) {
        let blanks = imp.blank_count();
        let last = imp.sheet(imp.sheet_count() - 1);
        match last {
            Some(sh) => {
                odd_ok = imp.sheet_count() == 16
                    && blanks == 1
                    && sh.back.blank
                    && sh.back.page == 0
                    && sh.front.page == 31
                    && sh.back.rot == Rot::R0;
                odd_detail = format!("sheets={} blanks={} lastfront={}", imp.sheet_count(), blanks, sh.front.page);
            }
            None => odd_ok = false,
        }
    }
    s.add_dyn("50061-J1-奇数页补白", odd_ok, &odd_detail);

    // J1-6 空白账：单面 30 页 → 30 个空白背面；双面偶数页 → 0 空白。
    let simp_blanks = match Imposition::plan(N, Mode::Simplex) {
        Ok(v) => v.blank_count(),
        Err(_) => 0,
    };
    let long_blanks = match Imposition::plan(N, Mode::LongEdge) {
        Ok(v) => v.blank_count(),
        Err(_) => 1,
    };
    let ok = simp_blanks == N as usize && long_blanks == 0;
    let det = format!("simplex={} duplex={}", simp_blanks, long_blanks);
    s.add_dyn("50061-J1-空白显式", ok, &det);

    // J1-7 确定性：同参两次拼版逐项相等、指纹相同；换模式指纹必变。
    let a = Imposition::plan(N, Mode::LongEdge);
    let b = Imposition::plan(N, Mode::LongEdge);
    let c = Imposition::plan(N, Mode::ShortEdge);
    let ok = match (&a, &b, &c) {
        (Ok(x), Ok(y), Ok(z)) => {
            x.digest() == y.digest()
                && x.sheets == y.sheets
                && z.digest() != x.digest()
                && x.sheet_count() == z.sheet_count()
        }
        _ => false,
    };
    s.add("50061-J1-拼版确定", ok, if ok { "" } else { "两次拼版不一致" });

    // J1-8 打印序：三模的发射序列都恰为 1..=30 严格递增（页序不因翻向改变）。
    let mut emit_ok = true;
    let mut mi4 = 0usize;
    while mi4 < Mode::ALL.len() {
        if let Some(m) = Mode::ALL.get(mi4) {
            match Imposition::plan(N, *m) {
                Ok(imp) => {
                    let seq = imp.emit_order();
                    if seq.len() != N as usize {
                        emit_ok = false;
                    }
                    let mut k = 0usize;
                    while k < seq.len() {
                        if let Some(v) = seq.get(k) {
                            if *v as usize != k + 1 {
                                emit_ok = false;
                            }
                        }
                        k += 1;
                    }
                }
                Err(_) => emit_ok = false,
            }
        }
        mi4 += 1;
    }
    s.add("50061-J1-打印序", emit_ok, if emit_ok { "" } else { "发射序非 1..n" });

    // J1-9 旋转群：4×4 合成表闭合、单位元、逆元、结合律（全表穷举）。
    let mut group_ok = true;
    let mut i = 0usize;
    while i < Rot::ALL.len() {
        if let Some(x) = Rot::ALL.get(i) {
            if x.compose(Rot::R0) != *x || Rot::R0.compose(*x) != *x {
                group_ok = false;
            }
            if x.compose(x.inverse()) != Rot::R0 {
                group_ok = false;
            }
            let mut j = 0usize;
            while j < Rot::ALL.len() {
                if let Some(y) = Rot::ALL.get(j) {
                    if x.compose(*y) != y.compose(*x) {
                        group_ok = false;
                    }
                    let mut k = 0usize;
                    while k < Rot::ALL.len() {
                        if let Some(z) = Rot::ALL.get(k) {
                            if x.compose(*y).compose(*z) != x.compose(y.compose(*z)) {
                                group_ok = false;
                            }
                        }
                        k += 1;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("50061-J1-旋转群公理", group_ok, if group_ok { "" } else { "Z4 公理不成立" });

    // J1-10 旋转几何：四次回原、合成与逐次作用一致（4 基点 × 16 对）。
    let mut geo_ok = true;
    let points = [(1, 0), (0, 1), (3, -2), (-5, 7)];
    let mut p = 0usize;
    while p < points.len() {
        if let Some(pt) = points.get(p) {
            let mut m = 0usize;
            while m < Rot::ALL.len() {
                if let Some(r) = Rot::ALL.get(m) {
                    let mut acc = *pt;
                    let mut t = 0usize;
                    while t < 4 {
                        acc = r.apply(acc);
                        t += 1;
                    }
                    if acc != *pt {
                        geo_ok = false;
                    }
                    let mut n = 0usize;
                    while n < Rot::ALL.len() {
                        if let Some(r2) = Rot::ALL.get(n) {
                            if r.compose(*r2).apply(*pt) != r.apply(r2.apply(*pt)) {
                                geo_ok = false;
                            }
                        }
                        n += 1;
                    }
                }
                m += 1;
            }
        }
        p += 1;
    }
    s.add("50061-J1-旋转几何", geo_ok, if geo_ok { "" } else { "作用与合成不一致" });

    // J1-11 账本双射：物理键唯一、页唯一、键数 = 页数（三模各一遍）。
    let mut bij = true;
    let mut mi5 = 0usize;
    while mi5 < Mode::ALL.len() {
        if let Some(m) = Mode::ALL.get(mi5) {
            if let Ok(imp) = Imposition::plan(N, *m) {
                if let Ok(led) = build_ledger(&imp) {
                    if led.verify() != Ok(N as usize) {
                        bij = false;
                    }
                    let mut keys = Vec::with_capacity(led.len());
                    let mut k = 0usize;
                    while k < led.len() {
                        if let Some(r) = led.row(k) {
                            keys.push(r.key());
                        }
                        k += 1;
                    }
                    if keys.len() != N as usize {
                        bij = false;
                    }
                } else {
                    bij = false;
                }
            } else {
                bij = false;
            }
        }
        mi5 += 1;
    }
    s.add("50061-J1-物理键双射", bij, if bij { "" } else { "物理键冲突或漏页" });

    // J1-12 翻向分布：长边翻两侧都不转 → 全 R0；短边翻是「正面 R0、背面 R180」，
    // 所以直方图必须是 {张数,0,张数,0}，且两项之和回到总页数。
    let long_hist = match Imposition::plan(N, Mode::LongEdge) {
        Ok(imp) => match build_ledger(&imp) {
            Ok(led) => led.rot_histogram(),
            Err(_) => [0u16; 4],
        },
        Err(_) => [0u16; 4],
    };
    let short_hist = match Imposition::plan(N, Mode::ShortEdge) {
        Ok(imp) => match build_ledger(&imp) {
            Ok(led) => led.rot_histogram(),
            Err(_) => [0u16; 4],
        },
        Err(_) => [0u16; 4],
    };
    let half = N / 2; // N=30 双面 → 15 张纸，正面 15 条 R0 + 背面 15 条 R180
    let ok = long_hist == [N, 0, 0, 0]
        && short_hist == [half, 0, half, 0]
        && short_hist[0] as u32 + short_hist[2] as u32 == N as u32;
    let det = format!("long={:?} short={:?}", long_hist, short_hist);
    s.add_dyn("50061-J1-翻向分布", ok, &det);

    // J1-13 重复物理键必拒且账不变。
    let mut dup = FacingLedger::new(8);
    let r0 = FaceRow {
        page: 1,
        sheet: 0,
        side: Side::Front,
        rot: Rot::R0,
        order: 0,
    };
    let r1 = FaceRow {
        page: 2,
        sheet: 0,
        side: Side::Front,
        rot: Rot::R0,
        order: 1,
    };
    let ok = dup.insert(r0).is_ok()
        && dup.insert(r1).err() == Some(DuplexError::SlotConflict)
        && dup.len() == 1
        && dup.rejected() == 1;
    s.add("50061-J1-槽冲突必拒", ok, if ok { "" } else { "同槽两页进了账" });

    // J1-14 页重复必拒（同一页排到两个不同槽）。
    let mut twice = FacingLedger::new(8);
    let t0 = FaceRow {
        page: 7,
        sheet: 0,
        side: Side::Front,
        rot: Rot::R0,
        order: 0,
    };
    let t1 = FaceRow {
        page: 7,
        sheet: 1,
        side: Side::Back,
        rot: Rot::R180,
        order: 1,
    };
    let ok = twice.insert(t0).is_ok()
        && twice.insert(t1).err() == Some(DuplexError::Duplicate)
        && twice.len() == 1;
    s.add("50061-J1-重页必拒", ok, if ok { "" } else { "同一页排了两次" });

    // J1-15 阅读序缺口必拒。
    let mut gap = FacingLedger::new(8);
    let g0 = FaceRow {
        page: 1,
        sheet: 0,
        side: Side::Front,
        rot: Rot::R0,
        order: 0,
    };
    let g1 = FaceRow {
        page: 2,
        sheet: 0,
        side: Side::Back,
        rot: Rot::R0,
        order: 4,
    };
    let ok = gap.insert(g0).is_ok()
        && gap.insert(g1).err() == Some(DuplexError::OrderGap)
        && gap.verify() == Ok(1);
    s.add("50061-J1-序缺口必拒", ok, if ok { "" } else { "跳号被接受" });

    // J1-16 定容 + 封账只读：容量 2 收满后 Full；封账后一律 Immutable。
    let mut tiny = FacingLedger::new(2);
    let x0 = FaceRow {
        page: 1,
        sheet: 0,
        side: Side::Front,
        rot: Rot::R0,
        order: 0,
    };
    let x1 = FaceRow {
        page: 2,
        sheet: 0,
        side: Side::Back,
        rot: Rot::R180,
        order: 1,
    };
    let x2 = FaceRow {
        page: 3,
        sheet: 1,
        side: Side::Front,
        rot: Rot::R0,
        order: 2,
    };
    let full = tiny.insert(x0).is_ok()
        && tiny.insert(x1).is_ok()
        && tiny.insert(x2).err() == Some(DuplexError::Full)
        && tiny.len() == tiny.capacity();
    tiny.seal();
    let x3 = FaceRow {
        page: 4,
        sheet: 1,
        side: Side::Back,
        rot: Rot::R0,
        order: 2,
    };
    let ok = full
        && tiny.is_sealed()
        && tiny.insert(x3).err() == Some(DuplexError::Immutable)
        && tiny.len() == 2;
    let det = format!("n={} cap={} sealed={}", tiny.len(), tiny.capacity(), tiny.is_sealed());
    s.add_dyn("50061-J1-定容封账", ok, &det);

    // J1-17 起印偏移：从背面起印时页 1 落首张背面、首张正面留白；
    // 10 页占 11 个槽位 → 6 张纸，末张背面也空，故空白槽恰为 2，页序仍连续。
    let mut offset_ok = false;
    let mut offset_detail = String::new();
    if let Ok(imp) = Imposition::plan_offset(10, Mode::LongEdge, true) {
        let first = imp.sheet(0);
        let led = build_ledger(&imp);
        match (first, led) {
            (Some(sh), Ok(l)) => {
                let p1 = l.by_page(1);
                offset_ok = sh.front.blank
                    && sh.front.page == 0
                    && sh.back.page == 1
                    && sh.back.side == Side::Back
                    && imp.start_on_back()
                    && match p1 {
                        Some(r) => r.sheet == 0 && r.side == Side::Back,
                        None => false,
                    }
                    && l.len() == 10
                    && imp.blank_count() == 2
                    && imp.slot_of(1) == reference_slot(1, Mode::LongEdge, true);
                offset_detail = format!(
                    "front={} back={} blanks={}",
                    sh.front.page,
                    sh.back.page,
                    imp.blank_count()
                );
            }
            _ => offset_ok = false,
        }
    }
    s.add_dyn("50061-J1-起印偏移", offset_ok, &offset_detail);

    // J1-18 非法必拒六连：0 页 / 超上限 / 模式码 3 / 旋转码 4 / 空白槽带页 / 槽超限。
    let bad_slot = Slot {
        page: 9,
        side: Side::Back,
        rot: Rot::R0,
        blank: true,
    };
    let nonblank = Slot {
        page: 0,
        side: Side::Front,
        rot: Rot::R0,
        blank: false,
    };
    let ok = Imposition::plan(0, Mode::Simplex).err() == Some(DuplexError::ZeroPages)
        && Imposition::plan(MAX_PAGES + 1, Mode::LongEdge).err() == Some(DuplexError::PageOverflow)
        && Mode::from_code(3).err() == Some(DuplexError::ModeCode)
        && Rot::from_code(9).err() == Some(DuplexError::RotCode)
        && !bad_slot.well_formed()
        && !nonblank.well_formed()
        && Imposition::plan_offset(MAX_PAGES, Mode::Simplex, true).err() == Some(DuplexError::Bounds);
    s.add("50061-J1-非法必拒", ok, if ok { "" } else { "非法入参被接受" });

    // J1-19 七类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50061-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50061_checks_green() {
        let set = run_unx50061_checks();
        assert!(set.all_passed(), "UNX-F50061 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50061_three_modes_same_page_order() {
        let a = Imposition::plan(30, Mode::LongEdge);
        let b = Imposition::plan(30, Mode::ShortEdge);
        assert!(a.is_ok() && b.is_ok());
        if let (Ok(x), Ok(y)) = (a, b) {
            for p in 1..=30u16 {
                assert_eq!(x.slot_of(p), y.slot_of(p));
            }
            assert_eq!(x.back_rot(), Rot::R0);
            assert_eq!(y.back_rot(), Rot::R180);
        }
    }

    #[test]
    fn unx50061_rotation_is_z4() {
        for r in Rot::ALL {
            assert_eq!(r.compose(Rot::R0), r);
            assert_eq!(r.compose(r.inverse()), Rot::R0);
            let mut acc = (3, 5);
            for _ in 0..4 {
                acc = r.apply(acc);
            }
            assert_eq!(acc, (3, 5));
        }
    }

    #[test]
    fn unx50061_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 7);
    }
}
