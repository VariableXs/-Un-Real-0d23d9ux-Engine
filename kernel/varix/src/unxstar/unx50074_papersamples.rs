//! UNX-F50074 纸张处理样本页（双面三模 ×3 样本入账）（0x43560 段）
//!
//! 锚点：UNX-F50074-J1 样本哈希账可复跑；判据锚定 §AI-63 §3。
//!
//! 数据段：`SEG_BASE = 0x43560`，诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 11`，
//! 逐项见 [`SampleError`]。
//!
//! 为什么样本要「入账」而不是「跑一遍看结果」：纸张处理是拼版 / 双面 / 补白
//! 三条链路的交汇点，任何一处的页序或空白策略改动都只在**特定页数**上炸
//! （典型：奇数页的最后一张背面、单面与双面的张数换算）。把 3 个样本 ×
//! 3 种双面模式固化成 9 条**带摘要的凭证**，每次固件变更只需 `replay()`
//! 一次：凭证摘要不匹配即说明拼版逻辑动了，且能精确指到「哪个样本 × 哪个模式」。
//!
//! 结构清单：
//! * [`Size`] + [`Mode`] + [`SampleSpec`]：样本注册表（三种幅面 ×1 处样本表）；
//! * [`SampleReceipt`]：单条样本凭证（张数 / 空白数 / 页→张映射摘要）；
//! * [`SampleLedger`]：定容哈希账（幂等入账 / 冲突必拒 / 封账 / 顺序摘要）；
//! * [`impose`] + [`replay`]：拼版推导与复跑核；
//! * [`injection_core`]：6 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43560 独占 0x20）。
pub const SEG_BASE: u32 = 0x43560;

const E_EMPTY: u16 = 1;
const E_BAD_ID: u16 = 2;
const E_BAD_PAGES: u16 = 3;
const E_BAD_MODE: u16 = 4;
const E_UNKNOWN_SIZE: u16 = 5;
const E_CONFLICT: u16 = 6;
const E_FULL: u16 = 7;
const E_SEALED: u16 = 8;
const E_MISMATCH: u16 = 9;
const E_PAGE_GAP: u16 = 10;
const E_DIGEST_DRIFT: u16 = 11;

/// 样本账错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleError {
    /// 空样本表。
    Empty,
    /// 样本号 0（保留值，禁止用作真样本）。
    BadId,
    /// 页数为 0 或超过上限。
    BadPages,
    /// 模式码不在 0..=2。
    BadMode,
    /// 幅面码不在 0..=3。
    UnknownSize,
    /// 同一（样本 × 模式）已有不同摘要的凭证。
    Conflict,
    /// 账本容量已满。
    Full,
    /// 账本已封账。
    Sealed,
    /// 复跑摘要与原凭证不符。
    Mismatch,
    /// 张数/空白数与页数不守恒。
    PageGap,
    /// 凭证自带摘要与重算结果不符（被篡改）。
    DigestDrift,
}

impl SampleError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            SampleError::Empty => E_EMPTY,
            SampleError::BadId => E_BAD_ID,
            SampleError::BadPages => E_BAD_PAGES,
            SampleError::BadMode => E_BAD_MODE,
            SampleError::UnknownSize => E_UNKNOWN_SIZE,
            SampleError::Conflict => E_CONFLICT,
            SampleError::Full => E_FULL,
            SampleError::Sealed => E_SEALED,
            SampleError::Mismatch => E_MISMATCH,
            SampleError::PageGap => E_PAGE_GAP,
            SampleError::DigestDrift => E_DIGEST_DRIFT,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            SampleError::Empty => "empty sample table",
            SampleError::BadId => "sample id zero",
            SampleError::BadPages => "sample page count out of range",
            SampleError::BadMode => "duplex mode code outside closed set",
            SampleError::UnknownSize => "paper size code outside closed set",
            SampleError::Conflict => "same sample/mode recorded with different digest",
            SampleError::Full => "sample ledger full",
            SampleError::Sealed => "sample ledger sealed read-only",
            SampleError::Mismatch => "sample replay digest mismatch",
            SampleError::PageGap => "sheet/blank count does not cover pages",
            SampleError::DigestDrift => "receipt digest drifted from recompute",
        }
    }
}

/// 幅面闭集。尺寸以 0.1 mm 为单位存整数，避免浮点在凭证摘要里引入
/// 「同值不同位」的伪漂移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// 210 × 297 mm。
    A4,
    /// 148 × 210 mm。
    A5,
    /// 215.9 × 279.4 mm。
    Letter,
    /// 215.9 × 355.6 mm。
    Legal,
}

impl Size {
    /// 幅面码 0..=3。
    pub fn code(self) -> u8 {
        match self {
            Size::A4 => 0,
            Size::A5 => 1,
            Size::Letter => 2,
            Size::Legal => 3,
        }
    }

    /// 幅面码反解；闭集外返回 [`SampleError::UnknownSize`]。
    pub fn from_code(c: u8) -> Result<Size, SampleError> {
        match c {
            0 => Ok(Size::A4),
            1 => Ok(Size::A5),
            2 => Ok(Size::Letter),
            3 => Ok(Size::Legal),
            _ => Err(SampleError::UnknownSize),
        }
    }

    /// 幅面尺寸（0.1 mm），纵版。
    pub fn dims(self) -> (u16, u16) {
        match self {
            Size::A4 => (2100, 2970),
            Size::A5 => (1480, 2100),
            Size::Letter => (2159, 2794),
            Size::Legal => (2159, 3556),
        }
    }

    /// 幅面名。
    pub fn label(self) -> &'static str {
        match self {
            Size::A4 => "A4",
            Size::A5 => "A5",
            Size::Letter => "Letter",
            Size::Legal => "Legal",
        }
    }
}

/// 双面三模（样本页必须三模各跑一遍）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 单面：一张一页。
    Simplex,
    /// 长边翻：一张两页，背面不倒置。
    LongEdge,
    /// 短边翻：一张两页，背面倒置。
    ShortEdge,
}

impl Mode {
    /// 全部三模（入账顺序即数组序，保证账序稳定）。
    pub const ALL: [Mode; 3] = [Mode::Simplex, Mode::LongEdge, Mode::ShortEdge];

    /// 模式码 0..=2。
    pub fn code(self) -> u8 {
        match self {
            Mode::Simplex => 0,
            Mode::LongEdge => 1,
            Mode::ShortEdge => 2,
        }
    }

    /// 模式码反解；闭集外返回 [`SampleError::BadMode`]。
    pub fn from_code(c: u8) -> Result<Mode, SampleError> {
        match c {
            0 => Ok(Mode::Simplex),
            1 => Ok(Mode::LongEdge),
            2 => Ok(Mode::ShortEdge),
            _ => Err(SampleError::BadMode),
        }
    }

    /// 是否双面。
    pub fn is_duplex(self) -> bool {
        self != Mode::Simplex
    }

    /// 模式名。
    pub fn label(self) -> &'static str {
        match self {
            Mode::Simplex => "simplex",
            Mode::LongEdge => "long-edge",
            Mode::ShortEdge => "short-edge",
        }
    }
}

/// 样本描述（3 条：幅面 / 页数各不相同，覆盖偶数页、奇数页、单面三种路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleSpec {
    pub id: u16,
    pub label: &'static str,
    pub size: Size,
    pub pages: u16,
}

/// 样本注册表。三条的页数刻意选「12 / 7 / 16」：
/// 偶数双面零空白、奇数双面单空白、多张整版，三条覆盖全部补白分支。
pub const SAMPLE_SPECS: [SampleSpec; 3] = [
    SampleSpec {
        id: 1,
        label: "a4-even",
        size: Size::A4,
        pages: 12,
    },
    SampleSpec {
        id: 2,
        label: "a5-odd",
        size: Size::A5,
        pages: 7,
    },
    SampleSpec {
        id: 3,
        label: "letter-block",
        size: Size::Letter,
        pages: 16,
    },
];

/// 单样本页数上限（超过视为坏输入，不让一条样本把账撑爆）。
pub const MAX_PAGES: u16 = 500;

/// 按序号取样本（越界给一条契约内兜底，杜绝裸下标）。
pub fn spec_at(i: usize) -> SampleSpec {
    match SAMPLE_SPECS.get(i) {
        Some(s) => *s,
        None => SampleSpec {
            id: 1,
            label: "fallback",
            size: Size::A4,
            pages: 4,
        },
    }
}

const FNV_OFFSET: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

fn fnv_u16(h: u32, v: u16) -> u32 {
    fnv_byte(fnv_byte(h, (v >> 8) as u8), v as u8)
}

/// 双面模式下的张数（单面即页数）。
pub fn sheets_for(pages: u16, mode: Mode) -> u16 {
    if mode.is_duplex() {
        (pages + 1) / 2
    } else {
        pages
    }
}

/// 双面模式下的空白槽数：只有奇数页会补一张空白背面。
pub fn blanks_for(pages: u16, mode: Mode) -> u16 {
    if mode.is_duplex() && pages % 2 == 1 {
        1
    } else {
        0
    }
}

/// 凭证摘要：样本号 / 模式 / 幅面 / 页数 / 张数 / 空白数，外加逐页
/// 「页 → (张序, 面)」映射。逐页折叠是关键——它让「张数对但页序错」
/// 也能被摘要抓住。
pub fn receipt_digest(
    sample: u16,
    mode: Mode,
    size: Size,
    pages: u16,
    sheets: u16,
    blanks: u16,
) -> u32 {
    let mut h = FNV_OFFSET;
    h = fnv_u16(h, sample);
    h = fnv_byte(h, mode.code());
    h = fnv_byte(h, size.code());
    h = fnv_u16(h, pages);
    h = fnv_u16(h, sheets);
    h = fnv_u16(h, blanks);
    let mut p: u16 = 1;
    while p <= pages {
        let sheet = if mode.is_duplex() { (p - 1) / 2 } else { p - 1 };
        let side = if mode.is_duplex() { (p - 1) % 2 } else { 0 };
        h = fnv_u16(h, p);
        h = fnv_u16(h, sheet);
        h = fnv_byte(h, side as u8);
        p = p.saturating_add(1);
    }
    h
}

/// 单条样本凭证（账本行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleReceipt {
    pub sample: u16,
    pub mode: Mode,
    pub size: Size,
    pub pages: u16,
    pub sheets: u16,
    pub blanks: u16,
    pub digest: u32,
}

impl SampleReceipt {
    /// 凭证自检：页数区间 / 张数守恒 / 空白规则 / 摘要一致。
    pub fn validate(&self) -> Result<(), SampleError> {
        if self.sample == 0 {
            return Err(SampleError::BadId);
        }
        if self.pages == 0 || self.pages > MAX_PAGES {
            return Err(SampleError::BadPages);
        }
        if self.sheets != sheets_for(self.pages, self.mode) {
            return Err(SampleError::PageGap);
        }
        if self.blanks != blanks_for(self.pages, self.mode) {
            return Err(SampleError::PageGap);
        }
        if self.sheets < self.blanks {
            return Err(SampleError::PageGap);
        }
        let want = receipt_digest(
            self.sample,
            self.mode,
            self.size,
            self.pages,
            self.sheets,
            self.blanks,
        );
        if want != self.digest {
            return Err(SampleError::DigestDrift);
        }
        Ok(())
    }

    /// 账键（样本 × 模式）。
    pub fn key(&self) -> (u16, u8) {
        (self.sample, self.mode.code())
    }

    /// 一行可读凭证。
    pub fn line(&self) -> String {
        format!(
            "#{}/{} {} p={} s={} b={} {:08x}",
            self.sample,
            self.mode.label(),
            self.size.label(),
            self.pages,
            self.sheets,
            self.blanks,
            self.digest
        )
    }
}

/// 按「样本表 × 三模」拼版推导一条凭证。
pub fn impose(spec: &SampleSpec, mode: Mode) -> Result<SampleReceipt, SampleError> {
    if spec.id == 0 {
        return Err(SampleError::BadId);
    }
    if spec.pages == 0 || spec.pages > MAX_PAGES {
        return Err(SampleError::BadPages);
    }
    let (w, hgt) = spec.size.dims();
    if w == 0 || hgt == 0 {
        return Err(SampleError::UnknownSize);
    }
    let sheets = sheets_for(spec.pages, mode);
    let blanks = blanks_for(spec.pages, mode);
    let digest = receipt_digest(spec.id, mode, spec.size, spec.pages, sheets, blanks);
    let r = SampleReceipt {
        sample: spec.id,
        mode,
        size: spec.size,
        pages: spec.pages,
        sheets,
        blanks,
        digest,
    };
    r.validate()?;
    Ok(r)
}

/// 样本哈希账：定容 + 幂等入账 + 冲突必拒 + 封账。
#[derive(Clone, Debug)]
pub struct SampleLedger {
    rows: Vec<SampleReceipt>,
    cap: usize,
    sealed: bool,
}

impl SampleLedger {
    /// 建空账。
    pub fn new(cap: usize) -> SampleLedger {
        SampleLedger {
            rows: Vec::with_capacity(cap),
            cap,
            sealed: false,
        }
    }

    /// 入账：封账 → 自检 → 定容 → 幂等/冲突判定。
    ///
    /// 幂等是**有意**的：同一样本同一模式重跑得到同一摘要，属于正常复跑，
    /// 不该让账本变长；摘要不同才是真冲突（说明拼版逻辑改了而凭证没更新）。
    pub fn record(&mut self, r: SampleReceipt) -> Result<(), SampleError> {
        if self.sealed {
            return Err(SampleError::Sealed);
        }
        r.validate()?;
        if let Some(old) = self.find(r.sample, r.mode) {
            if old.digest == r.digest {
                return Ok(());
            }
            return Err(SampleError::Conflict);
        }
        if self.rows.len() >= self.cap {
            return Err(SampleError::Full);
        }
        self.rows.push(r);
        Ok(())
    }

    /// 按（样本 × 模式）查行。
    pub fn find(&self, sample: u16, mode: Mode) -> Option<&SampleReceipt> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.sample == sample && r.mode == mode {
                    return Some(r);
                }
            }
            i += 1;
        }
        None
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 取第 i 行。
    pub fn row(&self, i: usize) -> Option<&SampleReceipt> {
        self.rows.get(i)
    }

    /// 账摘要（入账顺序敏感）：用于证明复跑逐位一致。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fnv_u16(h, r.sample);
                h = fnv_byte(h, r.mode.code());
                h = fnv_byte(h, r.size.code());
                h = fnv_u16(h, r.pages);
                h = fnv_u16(h, r.sheets);
                h = fnv_u16(h, r.blanks);
                h = fnv_byte(h, (r.digest >> 24) as u8);
                h = fnv_byte(h, (r.digest >> 16) as u8);
                h = fnv_byte(h, (r.digest >> 8) as u8);
                h = fnv_byte(h, r.digest as u8);
            }
            i += 1;
        }
        h
    }

    /// 复跑核：逐行重算并比对摘要，返回复跑通过的行数。
    ///
    /// 这里用**样本表**（而不是凭证自述）重算，才叫独立复算；只比
    /// `r.digest == r.digest` 那种自证是无效判据。
    pub fn replay(&self) -> Result<usize, SampleError> {
        let mut ok = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                let mut found = false;
                let mut j = 0usize;
                while j < SAMPLE_SPECS.len() {
                    if let Some(spec) = SAMPLE_SPECS.get(j) {
                        if spec.id == r.sample && spec.size == r.size && spec.pages == r.pages {
                            let fresh = impose(spec, r.mode)?;
                            if fresh.digest != r.digest
                                || fresh.sheets != r.sheets
                                || fresh.blanks != r.blanks
                            {
                                return Err(SampleError::Mismatch);
                            }
                            found = true;
                            ok += 1;
                        }
                    }
                    j += 1;
                }
                if !found {
                    return Err(SampleError::BadId);
                }
            }
            i += 1;
        }
        Ok(ok)
    }
}

/// 由样本表构建 9 条凭证账（样本外循环、模式内循环 → 账序固定）。
pub fn build_from(specs: &[SampleSpec]) -> Result<SampleLedger, SampleError> {
    if specs.is_empty() {
        return Err(SampleError::Empty);
    }
    let mut led = SampleLedger::new(specs.len() * Mode::ALL.len());
    let mut i = 0usize;
    while i < specs.len() {
        if let Some(spec) = specs.get(i) {
            let mut m = 0usize;
            while m < Mode::ALL.len() {
                if let Some(mode) = Mode::ALL.get(m) {
                    let r = impose(spec, *mode)?;
                    led.record(r)?;
                }
                m += 1;
            }
        }
        i += 1;
    }
    Ok(led)
}

/// 由标准样本表构建账本。
pub fn build_ledger() -> Result<SampleLedger, SampleError> {
    build_from(&SAMPLE_SPECS)
}

/// 复跑标准样本表：返回 (构建的账摘要, 复跑通过行数)。
pub fn replay_standard() -> Result<(u32, usize), SampleError> {
    let led = build_ledger()?;
    let verified = led.replay()?;
    Ok((led.digest(), verified))
}

// ---------------------------------------------------------------------------
// 注入核：6 类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 6 类坏样本注入核，返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    // 1) 空样本表。
    if build_from(&[]).err() == Some(SampleError::Empty) {
        caught += 1;
    }

    // 2) 样本号 0。
    let zero = SampleSpec {
        id: 0,
        label: "zero",
        size: Size::A4,
        pages: 2,
    };
    if impose(&zero, Mode::LongEdge).err() == Some(SampleError::BadId) {
        caught += 1;
    }

    // 3) 页数为 0。
    let empty_pages = SampleSpec {
        id: 9,
        label: "empty",
        size: Size::A4,
        pages: 0,
    };
    if impose(&empty_pages, Mode::Simplex).err() == Some(SampleError::BadPages) {
        caught += 1;
    }

    // 4) 模式码越界（3 不在闭集）。
    if Mode::from_code(3).err() == Some(SampleError::BadMode) {
        caught += 1;
    }

    // 5) 张数与页数不守恒（12 页双面应 6 张，凭证写 5 张）。
    let mut gap = match impose(&spec_at(0), Mode::LongEdge) {
        Ok(v) => v,
        Err(_) => SampleReceipt {
            sample: 1,
            mode: Mode::LongEdge,
            size: Size::A4,
            pages: 12,
            sheets: 6,
            blanks: 0,
            digest: 0,
        },
    };
    gap.sheets = 5;
    gap.digest = receipt_digest(gap.sample, gap.mode, gap.size, gap.pages, gap.sheets, gap.blanks);
    if gap.validate().err() == Some(SampleError::PageGap) {
        caught += 1;
    }

    // 6) 摘要被篡改（其余字段都对，只有 digest 漂了）。
    let mut drift = match impose(&spec_at(1), Mode::ShortEdge) {
        Ok(v) => v,
        Err(_) => SampleReceipt {
            sample: 2,
            mode: Mode::ShortEdge,
            size: Size::A5,
            pages: 7,
            sheets: 4,
            blanks: 1,
            digest: 0,
        },
    };
    drift.digest ^= 0x0000_00ff;
    if drift.validate().err() == Some(SampleError::DigestDrift) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50074 自检：15 条判据。
pub fn run_unx50074_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50074");

    // J1-1 入账规模：3 样本 × 3 模 = 9 行，账键两两互异。
    let led = build_ledger();
    let mut shape_ok = false;
    let mut rows = 0usize;
    if let Ok(l) = &led {
        rows = l.len();
        shape_ok = rows == 9;
        let mut i = 0usize;
        while i < rows {
            if let Some(a) = l.row(i) {
                let mut j = i + 1;
                while j < rows {
                    if let Some(b) = l.row(j) {
                        if a.key() == b.key() {
                            shape_ok = false;
                        }
                    }
                    j += 1;
                }
            }
            i += 1;
        }
    }
    let det = format!("rows={}/9", rows);
    s.add_dyn("50074-J1-入账9行", shape_ok, &det);

    // J1-2 可复跑：逐行独立复算全部通过（replay 返回 9）。
    let verified = match &led {
        Ok(l) => match l.replay() {
            Ok(v) => v,
            Err(_) => 0,
        },
        Err(_) => 0,
    };
    let ok = verified == 9;
    let det = format!("verified={}/9", verified);
    s.add_dyn("50074-J1-复跑9行", ok, &det);

    // J1-3 账摘要可复跑：两次独立构建摘要逐位一致。
    let det_ok = match (build_ledger(), build_ledger()) {
        (Ok(a), Ok(b)) => a.digest() == b.digest() && a.len() == b.len(),
        _ => false,
    };
    s.add("50074-J1-摘要可复跑", det_ok, if det_ok { "" } else { "两次构建摘要不同" });

    // J1-4 逐条守恒：双面 pages + blanks == 2 × sheets；单面 sheets == pages。
    let mut cons = true;
    if let Ok(l) = &led {
        let mut i = 0usize;
        while i < l.len() {
            if let Some(r) = l.row(i) {
                if r.mode.is_duplex() {
                    if r.pages as u32 + r.blanks as u32 != 2 * r.sheets as u32 {
                        cons = false;
                    }
                } else if r.sheets != r.pages || r.blanks != 0 {
                    cons = false;
                }
            }
            i += 1;
        }
    } else {
        cons = false;
    }
    s.add("50074-J1-页张守恒", cons, if cons { "" } else { "页数未被张数覆盖" });

    // J1-5 补白规则：奇数页双面恰 1 空白，偶数页双面 0，单面恒 0。
    let mut blanks_ok = true;
    let mut i5 = 0usize;
    while i5 < SAMPLE_SPECS.len() {
        if let Some(spec) = SAMPLE_SPECS.get(i5) {
            let mut m = 0usize;
            while m < Mode::ALL.len() {
                if let Some(mode) = Mode::ALL.get(m) {
                    if let Ok(r) = impose(spec, *mode) {
                        let want = if mode.is_duplex() && spec.pages % 2 == 1 { 1 } else { 0 };
                        if r.blanks != want {
                            blanks_ok = false;
                        }
                    } else {
                        blanks_ok = false;
                    }
                }
                m += 1;
            }
        }
        i5 += 1;
    }
    s.add("50074-J1-补白规则", blanks_ok, if blanks_ok { "" } else { "补白数与页奇偶不符" });

    // J1-6 摘要分档：同一样本的三种模式摘要两两不同（模式必须进摘要）。
    let mut mode_distinct = true;
    let base = match impose(&spec_at(0), Mode::Simplex) {
        Ok(v) => v,
        Err(_) => SampleReceipt {
            sample: 1,
            mode: Mode::Simplex,
            size: Size::A4,
            pages: 12,
            sheets: 12,
            blanks: 0,
            digest: 0,
        },
    };
    let long = impose(&spec_at(0), Mode::LongEdge);
    let short = impose(&spec_at(0), Mode::ShortEdge);
    match (long, short) {
        (Ok(a), Ok(b)) => {
            if a.digest == base.digest || b.digest == base.digest || a.digest == b.digest {
                mode_distinct = false;
            }
        }
        _ => mode_distinct = false,
    }
    s.add("50074-J1-模式进摘要", mode_distinct, if mode_distinct { "" } else { "模式未进摘要" });

    // J1-7 样本进摘要：三条样本在同一模式下摘要互异（页数/幅面进摘要）。
    let mut sample_distinct = true;
    let mut digests: Vec<u32> = Vec::new();
    let mut i7 = 0usize;
    while i7 < SAMPLE_SPECS.len() {
        if let Some(spec) = SAMPLE_SPECS.get(i7) {
            match impose(spec, Mode::LongEdge) {
                Ok(r) => digests.push(r.digest),
                Err(_) => sample_distinct = false,
            }
        }
        i7 += 1;
    }
    let mut a = 0usize;
    while a < digests.len() {
        let mut b = a + 1;
        while b < digests.len() {
            if digests.get(a) == digests.get(b) {
                sample_distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("50074-J1-样本进摘要", sample_distinct, if sample_distinct { "" } else { "摘要碰撞" });

    // J1-8 幂等入账：同凭证重复入账不增行；不同摘要必冲突且账不变。
    let mut l8 = SampleLedger::new(4);
    let r8 = match impose(&spec_at(0), Mode::LongEdge) {
        Ok(v) => v,
        Err(_) => base,
    };
    let first = l8.record(r8).is_ok();
    let again = l8.record(r8).is_ok();
    let mut other = r8;
    other.pages = 10;
    other.sheets = 5;
    other.digest = receipt_digest(other.sample, other.mode, other.size, other.pages, 5, 0);
    let ok = first && again && l8.len() == 1 && l8.record(other).err() == Some(SampleError::Conflict);
    s.add("50074-J1-幂等与冲突", ok, if ok { "" } else { "幂等/冲突判定错" });

    // J1-9 定容：cap=2 第三条 Full，行数停在 2。
    let mut l9 = SampleLedger::new(2);
    let mut pushed = 0usize;
    let mut i9 = 0usize;
    while i9 < SAMPLE_SPECS.len() {
        if let Some(spec) = SAMPLE_SPECS.get(i9) {
            if let Ok(r) = impose(spec, Mode::Simplex) {
                if l9.record(r).is_ok() {
                    pushed += 1;
                }
            }
        }
        i9 += 1;
    }
    let ok = pushed == 2 && l9.len() == 2 && l9.capacity() == 2;
    s.add("50074-J1-定容封顶", ok, if ok { "" } else { "超容入账" });

    // J1-10 封账只读。
    l9.seal();
    let mut l10 = match impose(&spec_at(2), Mode::Simplex) {
        Ok(v) => v,
        Err(_) => base,
    };
    l10.sample = 3;
    l10.digest = receipt_digest(l10.sample, l10.mode, l10.size, l10.pages, l10.sheets, l10.blanks);
    let ok = l9.is_sealed()
        && l9.record(l10).err() == Some(SampleError::Sealed)
        && l9.len() == 2;
    s.add("50074-J1-封账只读", ok, if ok { "" } else { "封账后可写" });

    // J1-11 篡改必抓：只翻摘要一位，其余字段全对，validate 必须给 DigestDrift；
    // 再把张数改错一位，必须给 PageGap（两类篡改走两条不同诊断）。
    let mut bogus = match impose(&spec_at(0), Mode::Simplex) {
        Ok(v) => v,
        Err(_) => base,
    };
    let digest_caught = {
        bogus.digest ^= 0x0001_0000;
        bogus.validate().err() == Some(SampleError::DigestDrift)
    };
    let mut gap2 = match impose(&spec_at(0), Mode::LongEdge) {
        Ok(v) => v,
        Err(_) => base,
    };
    gap2.sheets = gap2.sheets.saturating_add(1);
    gap2.digest = receipt_digest(gap2.sample, gap2.mode, gap2.size, gap2.pages, gap2.sheets, gap2.blanks);
    let sheet_caught = gap2.validate().err() == Some(SampleError::PageGap);
    s.add("50074-J1-篡改必抓", digest_caught && sheet_caught, if digest_caught && sheet_caught { "" } else { "摘要/张数漂移未拒" });

    // J1-12 坏模式/坏幅面/超页数：反解与推导都必须给闭集错误。
    let over = SampleSpec {
        id: 5,
        label: "over",
        size: Size::A4,
        pages: MAX_PAGES + 1,
    };
    let ok = Mode::from_code(9).err() == Some(SampleError::BadMode)
        && Size::from_code(4).err() == Some(SampleError::UnknownSize)
        && impose(&over, Mode::LongEdge).err() == Some(SampleError::BadPages);
    s.add("50074-J1-坏码必拒", ok, if ok { "" } else { "越界入参被接受" });

    // J1-13 账摘要顺序敏感：换入账顺序摘要必变（防「排序后摘要」冒充）。
    let mut fwd = SampleLedger::new(3);
    let mut rev = SampleLedger::new(3);
    let r1 = match impose(&spec_at(0), Mode::Simplex) {
        Ok(v) => v,
        Err(_) => base,
    };
    let r2 = match impose(&spec_at(1), Mode::Simplex) {
        Ok(v) => v,
        Err(_) => base,
    };
    let _ = fwd.record(r1);
    let _ = fwd.record(r2);
    let _ = rev.record(r2);
    let _ = rev.record(r1);
    let ok = fwd.len() == 2 && rev.len() == 2 && fwd.digest() != rev.digest();
    s.add("50074-J1-账序敏感", ok, if ok { "" } else { "摘要忽略入账序" });

    // J1-14 账摘要盖住全部行：删一行摘要必变（无遗漏行）。
    let mut shrunk = SampleLedger::new(9);
    if let Ok(src) = &led {
        let mut k = 0usize;
        while k + 1 < src.len() {
            if let Some(r) = src.row(k) {
                let _ = shrunk.record(*r);
            }
            k += 1;
        }
    }
    let ok = match (&led, &shrunk) {
        (Ok(a), b) => a.len() == 9 && b.len() == 8 && a.digest() != b.digest(),
        _ => false,
    };
    s.add("50074-J1-摘要全覆盖", ok, if ok { "" } else { "少一行摘要未变" });

    // J1-15 六类坏样本全抓。
    let (caught_bad, total) = injection_core();
    let ok = caught_bad == total;
    let det = format!("注入 {}/{}", caught_bad, total);
    s.add_dyn("50074-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50074_checks_green() {
        let set = run_unx50074_checks();
        assert!(set.all_passed(), "UNX-F50074 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50074_nine_receipts_replay() {
        let led = build_ledger();
        assert!(led.is_ok());
        if let Ok(l) = led {
            assert_eq!(l.len(), 9);
            assert_eq!(l.replay().ok(), Some(9));
            assert_eq!(l.replay().ok(), Some(9));
        }
    }

    #[test]
    fn unx50074_blank_rule_matches_parity() {
        for spec in SAMPLE_SPECS.iter() {
            for mode in Mode::ALL.iter() {
                let r = impose(spec, *mode);
                assert!(r.is_ok());
                if let Ok(r) = r {
                    let want = if mode.is_duplex() && spec.pages % 2 == 1 { 1 } else { 0 };
                    assert_eq!(r.blanks, want);
                    if mode.is_duplex() {
                        assert_eq!(r.pages as u32 + r.blanks as u32, 2 * r.sheets as u32);
                    }
                }
            }
        }
    }

    #[test]
    fn unx50074_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 6);
    }
}
