//! UNX-F50056 中英文字体回退链（缺字形回退显式记账）（0x43320 段）
//!
//! 锚点：UNX-F50056-J1 回退 ×10 显式记账零豆腐块静默；判据锚定 §AI-63 §2 专题二。
//!
//! 结构清单：
//! * 覆盖区间 `CharRange` + 区间表解析器 `parse_ranges`：把 "0041-005A,0061"
//!   这样的紧凑规格解析成闭区间表，倒置区间与越界码位一律 `Range`；
//! * 字体注册表 `FontFace` + 有序回退链 `FallbackChain`：链序即优先级，
//!   首个覆盖命中即返回（`depth` 即回退层数）；
//! * 查询账本 `LookupRecord`：每次解析——**包括失败**——都落一条记录。
//!   缺字形返回 `Missing` 而不是悄悄画一个 .notdef 方框（豆腐块）；
//! * 审计 `audit_tofu`：回扫账本，凡「声称命中但字体其实不覆盖」的条目计数，
//!   恒为 0 才说明没有静默豆腐块；
//! * 注入核：空链 / 重复字体 / 链深满 / 区间倒置 / 代理区码位 / 缺字形 /
//!   规格格式错 / 账本满 八类坏样本，逐类断言诊断码。
//!
//! 错误纪律：诊断码闭集 `0x43320 + 1..=8`；内核路径零 panic。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x43320 独占 0x20）。
pub const SEG_BASE: u32 = 0x43320;

pub const E_EMPTY: u16 = 1;
pub const E_DUP: u16 = 2;
pub const E_FULL: u16 = 3;
pub const E_RANGE: u16 = 4;
pub const E_CP: u16 = 5;
pub const E_MISSING: u16 = 6;
pub const E_FORMAT: u16 = 7;
pub const E_LEDGER: u16 = 8;

/// Unicode 码位上限（含）。
pub const CP_MAX: u32 = 0x0010_FFFF;
/// 代理区：0xD800..=0xDFFF 不可作为字符编码，任何解析入口都必须拒绝。
pub const SURROGATE_LO: u32 = 0xD800;
pub const SURROGATE_HI: u32 = 0xDFFF;

/// 字体回退错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WmError {
    EmptyChain,
    Dup,
    ChainFull,
    Range,
    Codepoint,
    Missing,
    Format,
    LedgerFull,
}

impl WmError {
    pub fn code(self) -> u32 {
        let n = match self {
            WmError::EmptyChain => E_EMPTY,
            WmError::Dup => E_DUP,
            WmError::ChainFull => E_FULL,
            WmError::Range => E_RANGE,
            WmError::Codepoint => E_CP,
            WmError::Missing => E_MISSING,
            WmError::Format => E_FORMAT,
            WmError::LedgerFull => E_LEDGER,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            WmError::EmptyChain => "fallback chain is empty",
            WmError::Dup => "font id already registered",
            WmError::ChainFull => "fallback chain depth exhausted",
            WmError::Range => "coverage range inverted or beyond U+10FFFF",
            WmError::Codepoint => "codepoint is surrogate or beyond U+10FFFF",
            WmError::Missing => "glyph covered by no font in chain",
            WmError::Format => "coverage spec malformed",
            WmError::LedgerFull => "lookup ledger full, cannot book",
        }
    }
}

/// 闭区间覆盖 [lo, hi]。字段私有：只能经 `new` 构造，从源头保证 lo ≤ hi，
/// 让后面的 `covers` 不必再防倒置区间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharRange {
    lo: u32,
    hi: u32,
}

impl CharRange {
    /// 构造一个覆盖区间：倒置或越过 U+10FFFF 一律 `Range`。
    pub fn new(lo: u32, hi: u32) -> Result<CharRange, WmError> {
        if lo > hi || hi > CP_MAX {
            return Err(WmError::Range);
        }
        Ok(CharRange { lo, hi })
    }

    pub fn lo(&self) -> u32 {
        self.lo
    }

    pub fn hi(&self) -> u32 {
        self.hi
    }

    pub fn covers(&self, cp: u32) -> bool {
        cp >= self.lo && cp <= self.hi
    }
}

fn hex_cp(s: &str) -> Result<u32, WmError> {
    u32::from_str_radix(s, 16).map_err(|_| WmError::Format)
}

/// 解析覆盖规格：逗号分隔的区间，区间可写成 `LO-HI` 或单个码位 `CP`，
/// 全部十六进制。空段、多余连字符、非十六进制字符都是 `Format`。
///
/// 不接受十进制：字体覆盖表惯例是十六进制，混入十进制会让 "10" 到底是
/// 0x10 还是 10 产生歧义，宁可统一拒绝也不做猜测。
pub fn parse_ranges(spec: &str) -> Result<Vec<CharRange>, WmError> {
    let mut out = Vec::new();
    for part in spec.split(',') {
        let t = part.trim();
        if t.is_empty() {
            return Err(WmError::Format);
        }
        let mut it = t.split('-');
        let lo_s = match it.next() {
            Some(v) => v.trim(),
            None => return Err(WmError::Format),
        };
        let lo = hex_cp(lo_s)?;
        let hi = match it.next() {
            Some(v) => hex_cp(v.trim())?,
            None => lo,
        };
        if it.next().is_some() {
            return Err(WmError::Format);
        }
        out.push(CharRange::new(lo, hi)?);
    }
    if out.is_empty() {
        return Err(WmError::Format);
    }
    Ok(out)
}

/// 一个字体面：标签 + 覆盖区间表。链序由加入顺序决定。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFace {
    pub id: u16,
    pub name: &'static str,
    ranges: Vec<CharRange>,
}

impl FontFace {
    pub fn new(id: u16, name: &'static str, ranges: Vec<CharRange>) -> FontFace {
        FontFace { id, name, ranges }
    }

    pub fn ranges(&self) -> &[CharRange] {
        &self.ranges
    }

    /// 是否覆盖某码位。区间表按加入顺序扫描，命中即返回。
    pub fn covers(&self, cp: u32) -> bool {
        let mut i = 0usize;
        while i < self.ranges.len() {
            if let Some(r) = self.ranges.get(i) {
                if r.covers(cp) {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
}

/// 一次查询的落账记录。`missing == true` 时 `font_id` 无意义（记 0），
/// `depth` 记「扫完了几层才放弃」，便于事后判断是链太短还是字体没装。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LookupRecord {
    pub seq: u32,
    pub cp: u32,
    pub font_id: u16,
    pub depth: u16,
    pub missing: bool,
}

/// 解析结果：命中字体的 id 与回退层数（0 = 首选字体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub cp: u32,
    pub font_id: u16,
    pub depth: u16,
}

/// 有序回退链 + 查询账本。
#[derive(Clone, Debug)]
pub struct FallbackChain {
    fonts: Vec<FontFace>,
    ledger: Vec<LookupRecord>,
    hits: Vec<u32>,
    max_fonts: usize,
    ledger_cap: usize,
}

impl FallbackChain {
    /// `max_fonts` 限制回退深度（链太长会拖慢每个缺字查询）；
    /// `ledger_cap` 限制记账条数，满了显式 `LedgerFull`——不能为了继续
    /// 解析而丢掉记账，那正是「静默豆腐块」的滋生土壤。
    pub fn new(max_fonts: usize, ledger_cap: usize) -> FallbackChain {
        FallbackChain {
            fonts: Vec::new(),
            ledger: Vec::new(),
            hits: Vec::new(),
            max_fonts,
            ledger_cap,
        }
    }

    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }

    pub fn fonts(&self) -> &[FontFace] {
        &self.fonts
    }

    pub fn ledger(&self) -> &[LookupRecord] {
        &self.ledger
    }

    /// 追加一个字体到链尾。重复 id、空覆盖表、链深满都显式拒绝。
    pub fn add(&mut self, f: FontFace) -> Result<(), WmError> {
        if f.ranges.is_empty() {
            return Err(WmError::Range);
        }
        if self.fonts.len() >= self.max_fonts {
            return Err(WmError::ChainFull);
        }
        let mut i = 0usize;
        while i < self.fonts.len() {
            if let Some(x) = self.fonts.get(i) {
                if x.id == f.id {
                    return Err(WmError::Dup);
                }
            }
            i += 1;
        }
        self.fonts.push(f);
        self.hits.push(0);
        Ok(())
    }

    /// 解析一个码位：按链序找第一个覆盖它的字体。
    ///
    /// 失败路径同样落账——这是本模块的核心纪律：缺字形必须留下「我查过、
    /// 没有」的记录，否则渲染层只能画豆腐块，而豆腐块是不可审计的。
    pub fn resolve(&mut self, cp: u32) -> Result<Resolution, WmError> {
        if cp > CP_MAX || (cp >= SURROGATE_LO && cp <= SURROGATE_HI) {
            return Err(WmError::Codepoint);
        }
        if self.fonts.is_empty() {
            return Err(WmError::EmptyChain);
        }
        if self.ledger.len() >= self.ledger_cap {
            return Err(WmError::LedgerFull);
        }
        let mut found: Option<usize> = None;
        let mut i = 0usize;
        while i < self.fonts.len() {
            if let Some(f) = self.fonts.get(i) {
                if f.covers(cp) {
                    found = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let seq = self.ledger.len() as u32;
        match found {
            Some(idx) => {
                let id = match self.fonts.get(idx) {
                    Some(f) => f.id,
                    None => 0,
                };
                self.ledger.push(LookupRecord {
                    seq,
                    cp,
                    font_id: id,
                    depth: idx as u16,
                    missing: false,
                });
                if let Some(h) = self.hits.get_mut(idx) {
                    *h = h.saturating_add(1);
                }
                Ok(Resolution {
                    cp,
                    font_id: id,
                    depth: idx as u16,
                })
            }
            None => {
                let depth = self.fonts.len() as u16;
                self.ledger.push(LookupRecord {
                    seq,
                    cp,
                    font_id: 0,
                    depth,
                    missing: true,
                });
                Err(WmError::Missing)
            }
        }
    }

    /// 某字体累计命中次数（不是解析次数——失败不记在字体头上）。
    pub fn hits_of(&self, id: u16) -> u32 {
        let mut i = 0usize;
        while i < self.fonts.len() {
            if let Some(f) = self.fonts.get(i) {
                if f.id == id {
                    return match self.hits.get(i) {
                        Some(v) => *v,
                        None => 0,
                    };
                }
            }
            i += 1;
        }
        0
    }

    pub fn resolved_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.ledger.len() {
            if let Some(r) = self.ledger.get(i) {
                if !r.missing {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn missing_count(&self) -> usize {
        self.ledger.len() - self.resolved_count()
    }

    /// 静默豆腐块审计：账本里凡「声称命中某字体但该字体其实不覆盖此码位」
    /// 的条目计数。实现正确时恒为 0。
    pub fn audit_tofu(&self) -> usize {
        let mut bad = 0usize;
        let mut i = 0usize;
        while i < self.ledger.len() {
            if let Some(r) = self.ledger.get(i) {
                if !r.missing {
                    let mut covered = false;
                    let mut k = 0usize;
                    while k < self.fonts.len() {
                        if let Some(f) = self.fonts.get(k) {
                            if f.id == r.font_id && f.covers(r.cp) {
                                covered = true;
                            }
                        }
                        k += 1;
                    }
                    if !covered {
                        bad += 1;
                    }
                }
            }
            i += 1;
        }
        bad
    }

    /// 账本序号连续性（追加式账本必须严格 +1，不得出现空洞）。
    pub fn ledger_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.ledger.len() {
            match self.ledger.get(i) {
                Some(r) => {
                    if r.seq != i as u32 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

/// 内置回退链：拉丁 → 中日韩 → 符号/emoji → 兜底。
///
/// 兜底字体刻意不覆盖私用区（E000–F8FF）：私用区没有标准字形，
/// 让兜底字体"接住"它才是真正的豆腐块来源——必须显式报缺字。
pub fn builtin_chain() -> Result<FallbackChain, WmError> {
    let mut c = FallbackChain::new(4, 64);
    c.add(FontFace::new(
        1,
        "LatinCore",
        parse_ranges("0020-007E,00A0-00FF,0100-017F")?,
    ))?;
    c.add(FontFace::new(
        2,
        "CJKSans",
        parse_ranges("3000-303F,4E00-9FFF,FF00-FFEF")?,
    ))?;
    c.add(FontFace::new(
        3,
        "Symbols",
        parse_ranges("2000-27BF,1F300-1FAFF")?,
    ))?;
    c.add(FontFace::new(
        4,
        "LastResort",
        parse_ranges("0000-D7FF,F900-10FFFF")?,
    ))?;
    Ok(c)
}

/// J1 的十个代表码位：拉丁字母/数字/重音、CJK 汉字、CJK 标点、全角逗号、
/// 数学求和、emoji、私用区（唯一预期缺字）。
pub const PROBE_CODEPOINTS: [u32; 10] = [
    0x0041, // A  → LatinCore
    0x0030, // 0  → LatinCore
    0x00E9, // é  → LatinCore
    0x4E2D, // 中 → CJKSans
    0x6587, // 文 → CJKSans
    0x3001, // 、 → CJKSans
    0xFF0C, // ， → CJKSans
    0x2211, // ∑  → Symbols
    0x1F600, // 😀 → Symbols
    0xE123, // 私用区 → 显式缺字
];

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

fn small_face(id: u16, spec: &str) -> Result<FontFace, WmError> {
    Ok(FontFace::new(id, "probe", parse_ranges(spec)?))
}

/// 八类坏样本。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;

    // 1 空链解析
    let mut c = FallbackChain::new(4, 8);
    if c.resolve(0x41).err() == Some(WmError::EmptyChain) {
        caught += 1;
    }

    // 2 重复字体 id
    let mut c = FallbackChain::new(4, 8);
    if let Ok(f) = small_face(1, "0041-005A") {
        let _ = c.add(f);
    }
    if let Ok(f) = small_face(1, "0061-007A") {
        if c.add(f).err() == Some(WmError::Dup) {
            caught += 1;
        }
    }

    // 3 链深满
    let mut c = FallbackChain::new(1, 8);
    if let Ok(f) = small_face(1, "0041-005A") {
        let _ = c.add(f);
    }
    if let Ok(f) = small_face(2, "0061-007A") {
        if c.add(f).err() == Some(WmError::ChainFull) {
            caught += 1;
        }
    }

    // 4 区间倒置
    if CharRange::new(0x50, 0x40).err() == Some(WmError::Range) {
        caught += 1;
    }

    // 5 代理区码位
    let mut c = match builtin_chain() {
        Ok(v) => v,
        Err(_) => FallbackChain::new(4, 8),
    };
    if c.resolve(0xD800).err() == Some(WmError::Codepoint) {
        caught += 1;
    }

    // 6 缺字形（私用区）
    if c.resolve(0xE123).err() == Some(WmError::Missing) {
        caught += 1;
    }

    // 7 规格格式错
    if parse_ranges("XY").err() == Some(WmError::Format) {
        caught += 1;
    }

    // 8 账本满（容量 2，第三次解析被拒且不落账）
    let mut c = FallbackChain::new(4, 2);
    if let Ok(f) = small_face(1, "0041-005A") {
        let _ = c.add(f);
    }
    let _ = c.resolve(0x41);
    let _ = c.resolve(0x42);
    if c.resolve(0x43).err() == Some(WmError::LedgerFull) && c.ledger().len() == 2 {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50056_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50056");

    // J1-1 拉丁直取：A/0/é 在首选字体命中，depth = 0
    let mut c = match builtin_chain() {
        Ok(v) => v,
        Err(_) => FallbackChain::new(4, 64),
    };
    let a = c.resolve(0x0041);
    let d0 = c.resolve(0x0030);
    let e = c.resolve(0x00E9);
    let ok = match (&a, &d0, &e) {
        (Ok(x), Ok(y), Ok(z)) => {
            x.depth == 0 && y.depth == 0 && z.depth == 0 && x.font_id == 1 && z.font_id == 1
        }
        _ => false,
    };
    let det = format!(
        "A={} é={}",
        match &a {
            Ok(x) => x.font_id,
            Err(_) => 0,
        },
        match &e {
            Ok(z) => z.font_id,
            Err(_) => 0,
        }
    );
    s.add_dyn("50056-J1-拉丁直取", ok, &det);

    // J1-2 CJK 回退：中/文/、/， 落到第二字体，depth = 1
    let mut chain = match builtin_chain() {
        Ok(v) => v,
        Err(_) => FallbackChain::new(4, 64),
    };
    let mut cjk_ok = true;
    let mut i = 3usize;
    while i < 7 {
        if let Some(cp) = PROBE_CODEPOINTS.get(i) {
            match chain.resolve(*cp) {
                Ok(r) => {
                    if r.font_id != 2 || r.depth != 1 {
                        cjk_ok = false;
                    }
                }
                Err(_) => cjk_ok = false,
            }
        }
        i += 1;
    }
    s.add(
        "50056-J1-CJK回退",
        cjk_ok,
        if cjk_ok { "" } else { "CJK 未在第二层命中" },
    );

    // J1-3 符号/emoji 回退：∑ 与 😀 落到第三字体，depth = 2
    let mut sym_ok = true;
    let mut i = 7usize;
    while i < 9 {
        if let Some(cp) = PROBE_CODEPOINTS.get(i) {
            match chain.resolve(*cp) {
                Ok(r) => {
                    if r.font_id != 3 || r.depth != 2 {
                        sym_ok = false;
                    }
                }
                Err(_) => sym_ok = false,
            }
        }
        i += 1;
    }
    s.add(
        "50056-J1-符号回退",
        sym_ok,
        if sym_ok { "" } else { "符号未在第三层命中" },
    );

    // J1-4 私用区显式缺字：Err(Missing)，且账本留下 missing 记录
    let mut c = match builtin_chain() {
        Ok(v) => v,
        Err(_) => FallbackChain::new(4, 64),
    };
    let miss = c.resolve(0xE123).err() == Some(WmError::Missing);
    let booked = match c.ledger().last() {
        Some(r) => r.missing && r.cp == 0xE123 && r.depth == c.len() as u16,
        None => false,
    };
    let ok = miss && booked;
    s.add("50056-J1-缺字记账", ok, if ok { "" } else { "缺字未落账" });

    // J1-5 十码点守恒：resolved + missing == 10，缺字恰 1 条，零静默豆腐块
    let mut c = match builtin_chain() {
        Ok(v) => v,
        Err(_) => FallbackChain::new(4, 64),
    };
    let mut i = 0usize;
    while i < PROBE_CODEPOINTS.len() {
        if let Some(cp) = PROBE_CODEPOINTS.get(i) {
            let _ = c.resolve(*cp);
        }
        i += 1;
    }
    let ok = c.ledger().len() == PROBE_CODEPOINTS.len()
        && c.resolved_count() + c.missing_count() == PROBE_CODEPOINTS.len()
        && c.missing_count() == 1
        && c.audit_tofu() == 0;
    let det = format!(
        "hit={} miss={} tofu={}",
        c.resolved_count(),
        c.missing_count(),
        c.audit_tofu()
    );
    s.add_dyn("50056-J1-十码点守恒", ok, &det);

    // J1-6 覆盖真值：每条命中记录都指向一个真的覆盖该码位的字体
    let ok = c.audit_tofu() == 0 && c.ledger_ok();
    s.add(
        "50056-J1-覆盖真值",
        ok,
        if ok { "" } else { "命中记录与覆盖表不符" },
    );

    // J1-7 代理区码位必拒
    let mut c = match builtin_chain() {
        Ok(v) => v,
        Err(_) => FallbackChain::new(4, 64),
    };
    let ok = c.resolve(0xD800).err() == Some(WmError::Codepoint)
        && c.resolve(0xDFFF).err() == Some(WmError::Codepoint);
    s.add("50056-J1-代理区必拒", ok, if ok { "" } else { "代理码位被放行" });

    // J1-8 超界码位必拒
    let mut c = FallbackChain::new(4, 8);
    let ok = c.resolve(CP_MAX + 1).err() == Some(WmError::Codepoint);
    s.add("50056-J1-超界必拒", ok, if ok { "" } else { "越界码位被放行" });

    // J1-9 空链必拒
    let mut c = FallbackChain::new(4, 8);
    let ok = c.resolve(0x41).err() == Some(WmError::EmptyChain);
    s.add("50056-J1-空链必拒", ok, if ok { "" } else { "空链仍返回结果" });

    // J1-10 重复字体必拒
    let mut c = FallbackChain::new(4, 8);
    if let Ok(f) = small_face(9, "0041-005A") {
        let _ = c.add(f);
    }
    let dup = match small_face(9, "0061-007A") {
        Ok(f) => c.add(f).err() == Some(WmError::Dup),
        Err(_) => false,
    };
    s.add("50056-J1-重复字体必拒", dup, if dup { "" } else { "重复 id 被放行" });

    // J1-11 区间倒置必拒
    let ok = CharRange::new(0x50, 0x40).err() == Some(WmError::Range)
        && CharRange::new(0x40, 0x40).is_ok();
    s.add("50056-J1-区间倒置必拒", ok, if ok { "" } else { "倒置区间被接受" });

    // J1-12 链深上限必拒
    let mut c = FallbackChain::new(2, 8);
    if let Ok(f) = small_face(1, "0041-005A") {
        let _ = c.add(f);
    }
    if let Ok(f) = small_face(2, "0061-007A") {
        let _ = c.add(f);
    }
    let full = match small_face(3, "0030-0039") {
        Ok(f) => c.add(f).err() == Some(WmError::ChainFull),
        Err(_) => false,
    };
    s.add("50056-J1-链深满必拒", full, if full { "" } else { "链深上限失效" });

    // J1-13 账本满必拒且不落账
    let mut c = FallbackChain::new(4, 3);
    if let Ok(f) = small_face(1, "0041-007A") {
        let _ = c.add(f);
    }
    let _ = c.resolve(0x41);
    let _ = c.resolve(0x42);
    let _ = c.resolve(0x43);
    let ok = c.resolve(0x44).err() == Some(WmError::LedgerFull) && c.ledger().len() == 3;
    s.add("50056-J1-账本满必拒", ok, if ok { "" } else { "账本满仍记账/仍解析" });

    // J1-14 优先级：两个字体都覆盖时取链上靠前者
    let mut c = FallbackChain::new(4, 8);
    if let Ok(f) = small_face(7, "0041-005A") {
        let _ = c.add(f);
    }
    if let Ok(f) = small_face(8, "0041") {
        let _ = c.add(f);
    }
    let ok = match c.resolve(0x41) {
        Ok(r) => r.font_id == 7 && r.depth == 0,
        Err(_) => false,
    };
    s.add("50056-J1-优先级", ok, if ok { "" } else { "回退顺序被颠倒" });

    // J1-15 解析确定性 + 命中计数单调：同码位两次解析同字体/同深度
    let mut c = match builtin_chain() {
        Ok(v) => v,
        Err(_) => FallbackChain::new(4, 64),
    };
    let r1 = c.resolve(0x4E2D);
    let r2 = c.resolve(0x4E2D);
    let ok = match (&r1, &r2) {
        (Ok(x), Ok(y)) => x == y && c.hits_of(2) == 2 && c.hits_of(1) == 0,
        _ => false,
    };
    let det = format!("hits2={}", c.hits_of(2));
    s.add_dyn("50056-J1-确定性单调", ok, &det);

    // J1-16 区间解析：两点区间 + 单点区间；格式错必拒
    let parsed = parse_ranges("0041-005A,0061");
    let ok = match &parsed {
        Ok(v) => {
            v.len() == 2
                && match (v.get(0), v.get(1)) {
                    (Some(a), Some(b)) => {
                        a.lo() == 0x41
                            && a.hi() == 0x5A
                            && b.lo() == 0x61
                            && b.hi() == 0x61
                    }
                    _ => false,
                }
                && parse_ranges("XY").err() == Some(WmError::Format)
                && parse_ranges("0041-").err() == Some(WmError::Format)
                && parse_ranges("").err() == Some(WmError::Format)
        }
        Err(_) => false,
    };
    s.add("50056-J1-区间解析", ok, if ok { "" } else { "规格解析不符预期" });

    // J1-17 账本序号严格递增（追加式账本无空洞）
    let ok = c.ledger_ok() && c.ledger().len() == 2;
    s.add("50056-J1-账本连续", ok, if ok { "" } else { "账本序号有空洞" });

    // J1-18 注入核八类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50056-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50056_checks_green() {
        let set = run_unx50056_checks();
        assert!(set.all_passed(), "UNX-F50056 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50056_missing_glyph_is_booked() {
        let mut c = builtin_chain();
        assert!(c.is_ok());
        if let Ok(chain) = c.as_mut() {
            assert_eq!(chain.resolve(0xE123).err(), Some(WmError::Missing));
            assert_eq!(chain.missing_count(), 1);
            assert_eq!(chain.audit_tofu(), 0);
        }
    }
}
