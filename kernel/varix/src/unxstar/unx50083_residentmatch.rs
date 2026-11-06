//! UNX-F50083 打印机驻留字体匹配器（名字/度量匹配优先级）（0x43680 段）
//!
//! 锚点：UNX-F50083-J1 匹配 ×30 与驻留表一致，未命中显式回退下载；
//! 判据锚定 §AI-63 §2 专题二。
//!
//! 为什么必须把「未命中」写成显式回退而不是默认值：打印机驻留字体表是
//! 设备出厂状态的只读视图，请求的字体如果表里没有，唯一正确的动作是
//! 回退到下载通道（F50084）。若匹配器悄悄挑一个「最像的」顶上，调用方
//! 会以为字体已就绪，实际字形/度量全错，且错误会在排版末端才暴露。
//! 因此本模块的 `MatchOutcome` 用 `Download` 档 + `pick=None` 明示回退。
//!
//! 匹配优先级（严格顺序，先命中先返回）：
//! 1. Exact：全名规范化相等且粗体/斜体位一致；
//! 2. Normalized：家族规范化相等（允许 "MT/PS/Regular" 等后缀）；
//! 3. Metric：同衬线类 + 同字距档 + 度量加权分 ≤ 阈值；
//! 4. Substitute：同衬线类 + 同字距档，度量差距大但形制可替；
//! 5. Download：以上全不命中，显式回退。
//!
//! 结构清单：驻留字体表（注册表）`ResidentTable`、匹配器 `Matcher`、
//! 会话状态机 `MatchSession`（Draft→Validated→Resolved）、
//! 判定账本 `MatchLedger`（上限 + FNV 封账）、注入核 `injection_core`。
//! 诊断码闭集：SEG_BASE = 0x43680，n ∈ [1,12]。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43680 独占 0x20；F50084 从 0x436a0 起顺序占用）。
pub const SEG_BASE: u32 = 0x43680;

pub const E_EMPTY: u16 = 1;
pub const E_NAME: u16 = 2;
pub const E_PITCH: u16 = 3;
pub const E_UPEM: u16 = 4;
pub const E_METRIC: u16 = 5;
pub const E_GLYPH: u16 = 6;
pub const E_TABLE: u16 = 7;
pub const E_DUP: u16 = 8;
pub const E_CAP: u16 = 9;
pub const E_TIER: u16 = 10;
pub const E_STATE: u16 = 11;
pub const E_SEAL: u16 = 12;

/// 驻留匹配错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchError {
    Empty,
    Name,
    Pitch,
    Upem,
    Metric,
    Glyph,
    Table,
    Dup,
    Cap,
    Tier,
    State,
    Seal,
}

impl MatchError {
    fn ordinal(self) -> u16 {
        match self {
            MatchError::Empty => E_EMPTY,
            MatchError::Name => E_NAME,
            MatchError::Pitch => E_PITCH,
            MatchError::Upem => E_UPEM,
            MatchError::Metric => E_METRIC,
            MatchError::Glyph => E_GLYPH,
            MatchError::Table => E_TABLE,
            MatchError::Dup => E_DUP,
            MatchError::Cap => E_CAP,
            MatchError::Tier => E_TIER,
            MatchError::State => E_STATE,
            MatchError::Seal => E_SEAL,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            MatchError::Empty => "empty family name",
            MatchError::Name => "family name too long",
            MatchError::Pitch => "pitch out of legal range",
            MatchError::Upem => "units-per-em out of legal range",
            MatchError::Metric => "ascent/cap inconsistent with upem",
            MatchError::Glyph => "glyph count zero",
            MatchError::Table => "resident table empty",
            MatchError::Dup => "duplicate resident font name",
            MatchError::Cap => "resident table capacity exhausted",
            MatchError::Tier => "match tier code invalid",
            MatchError::State => "illegal match session transition",
            MatchError::Seal => "match ledger already sealed",
        }
    }
}

/// 一条驻留字体记录。度量全部用整数：打印链路不需要浮点，整数也避免了
/// 跨平台 NaN/舍入差异导致的匹配抖动。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResidentFont {
    pub name: &'static str,
    pub family: &'static str,
    pub bold: bool,
    pub italic: bool,
    pub serif: bool,
    /// 字距：0 = 比例字体，>0 = 每 10 字符的字符数 ×10（100 即 10cpi）。
    pub pitch: u16,
    pub upem: u16,
    pub ascent: i16,
    pub cap_height: i16,
    pub glyphs: u16,
}

/// 12 款出厂驻留字体（模拟典型 PCL/PS 打印机）。名字与度量固定，
/// 匹配器只读不写；替换由 F50084 的下载通道负责。
pub const RESIDENT_FONTS: [ResidentFont; 12] = [
    ResidentFont { name: "Courier", family: "Courier", bold: false, italic: false, serif: true, pitch: 100, upem: 1000, ascent: 629, cap_height: 562, glyphs: 256 },
    ResidentFont { name: "Courier-Bold", family: "Courier", bold: true, italic: false, serif: true, pitch: 100, upem: 1000, ascent: 629, cap_height: 562, glyphs: 256 },
    ResidentFont { name: "Times-Roman", family: "Times", bold: false, italic: false, serif: true, pitch: 0, upem: 1000, ascent: 683, cap_height: 662, glyphs: 512 },
    ResidentFont { name: "Times-Bold", family: "Times", bold: true, italic: false, serif: true, pitch: 0, upem: 1000, ascent: 683, cap_height: 676, glyphs: 512 },
    ResidentFont { name: "Times-Italic", family: "Times", bold: false, italic: true, serif: true, pitch: 0, upem: 1000, ascent: 683, cap_height: 653, glyphs: 512 },
    ResidentFont { name: "Helvetica", family: "Helvetica", bold: false, italic: false, serif: false, pitch: 0, upem: 1000, ascent: 718, cap_height: 718, glyphs: 512 },
    ResidentFont { name: "Helvetica-Bold", family: "Helvetica", bold: true, italic: false, serif: false, pitch: 0, upem: 1000, ascent: 718, cap_height: 718, glyphs: 512 },
    ResidentFont { name: "Helvetica-Oblique", family: "Helvetica", bold: false, italic: true, serif: false, pitch: 0, upem: 1000, ascent: 718, cap_height: 718, glyphs: 512 },
    ResidentFont { name: "Palatino-Roman", family: "Palatino", bold: false, italic: false, serif: true, pitch: 0, upem: 1000, ascent: 692, cap_height: 692, glyphs: 512 },
    ResidentFont { name: "Symbol", family: "Symbol", bold: false, italic: false, serif: false, pitch: 0, upem: 1000, ascent: 673, cap_height: 0, glyphs: 190 },
    ResidentFont { name: "ZapfDingbats", family: "ZapfDingbats", bold: false, italic: false, serif: false, pitch: 0, upem: 1000, ascent: 684, cap_height: 0, glyphs: 202 },
    ResidentFont { name: "LetterGothic", family: "LetterGothic", bold: false, italic: false, serif: false, pitch: 120, upem: 1000, ascent: 600, cap_height: 550, glyphs: 256 },
];

/// 请求侧字体描述。`family` 允许带样式后缀，匹配前统一规范化。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontRequest {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub serif: bool,
    pub pitch: u16,
    pub upem: u16,
    pub ascent: i16,
    pub cap_height: i16,
    pub glyphs: u16,
}

impl FontRequest {
    /// 用驻留字体自身的度量构造请求（自检与回归用）。
    pub fn like(font: &ResidentFont, family: String) -> FontRequest {
        FontRequest {
            family,
            bold: font.bold,
            italic: font.italic,
            serif: font.serif,
            pitch: font.pitch,
            upem: font.upem,
            ascent: font.ascent,
            cap_height: font.cap_height,
            glyphs: font.glyphs,
        }
    }
}

/// 请求合法性闸门：所有上界都来自打印链路实测，不做「宽容修正」。
pub fn validate_request(r: &FontRequest) -> Result<(), MatchError> {
    if r.family.is_empty() {
        return Err(MatchError::Empty);
    }
    if r.family.len() > 64 {
        return Err(MatchError::Name);
    }
    if r.pitch > 300 {
        return Err(MatchError::Pitch);
    }
    if r.upem < 16 || r.upem > 4096 {
        return Err(MatchError::Upem);
    }
    if r.ascent <= 0 || (r.ascent as i32) >= r.upem as i32 {
        return Err(MatchError::Metric);
    }
    if r.cap_height < 0 || (r.cap_height as i32) > r.ascent as i32 {
        return Err(MatchError::Metric);
    }
    if r.glyphs == 0 {
        return Err(MatchError::Glyph);
    }
    Ok(())
}

/// 名字规范化：小写、`-`/`_`/空白统一成单空格、其余标点丢弃、去首尾空格。
/// 打印机侧字体名大小写与连字符写法极不统一（Helvetica-Oblique /
/// helvetica oblique），不做规范化就会出现大量假未命中。
pub fn canon_name(s: &str) -> String {
    let mut out = String::new();
    let mut last_space = true;
    for c in s.chars() {
        let lc = c.to_ascii_lowercase();
        if lc.is_ascii_alphanumeric() {
            out.push(lc);
            last_space = false;
        } else if (lc == ' ' || lc == '-' || lc == '_') && !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    while out.ends_with(' ') {
        out.truncate(out.len() - 1);
    }
    out
}

/// 去样式后缀：反复剥掉末尾的 bold/italic/oblique/mt/ps/regular/roman。
/// 只剥尾词，绝不动中间词——"Palatino Roman Book" 的 Roman 是家族名一部分。
pub fn strip_style(s: &str) -> String {
    let mut words: Vec<&str> = Vec::new();
    let mut it = s.split(' ');
    while let Some(w) = it.next() {
        if !w.is_empty() {
            words.push(w);
        }
    }
    loop {
        let n = words.len();
        if n == 0 {
            break;
        }
        let last = words[n - 1];
        let drop = last == "bold"
            || last == "italic"
            || last == "oblique"
            || last == "mt"
            || last == "ps"
            || last == "regular"
            || last == "roman";
        if drop {
            words.truncate(n - 1);
        } else {
            break;
        }
    }
    let mut out = String::new();
    let mut i = 0usize;
    while i < words.len() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(words[i]);
        i += 1;
    }
    out
}

fn abs_i32(v: i32) -> u32 {
    if v < 0 {
        (0i32 - v) as u32
    } else {
        v as u32
    }
}

/// 字距兼容：比例体只与比例体兼容；等宽体允许 ±10 的机械容差
/// （600/1000 upem 换算到实际点数会有 1/10 cpi 级误差）。
fn pitch_compatible(a: u16, b: u16) -> bool {
    if a == b {
        return true;
    }
    if a > 0 && b > 0 {
        let d = if a > b { a - b } else { b - a };
        return d <= 10;
    }
    false
}

/// 度量加权分。权重按「排版可见度」排序：衬线类最重（形制不同一眼可见），
/// 字距次之（列宽直接错位），upem/ascender 再次，字形数最轻（只影响可选集）。
fn metric_score(req: &FontRequest, f: &ResidentFont) -> u32 {
    let mut s = 0u32;
    if req.serif != f.serif {
        s += 900;
    }
    if !pitch_compatible(req.pitch, f.pitch) {
        s += 700;
    }
    if req.bold != f.bold {
        s += 300;
    }
    if req.italic != f.italic {
        s += 300;
    }
    s += abs_i32(req.upem as i32 - f.upem as i32) * 2;
    s += abs_i32(req.ascent as i32 - f.ascent as i32);
    s += abs_i32(req.cap_height as i32 - f.cap_height as i32);
    let gd = abs_i32(req.glyphs as i32 - f.glyphs as i32) / 8;
    s += if gd > 400 { 400 } else { gd };
    s
}

/// 命中档位。`Download` 不是「最差匹配」，而是「无匹配」的显式表达。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchTier {
    Exact,
    Normalized,
    Metric,
    Substitute,
    Download,
}

impl MatchTier {
    pub fn code(self) -> u8 {
        match self {
            MatchTier::Exact => 0,
            MatchTier::Normalized => 1,
            MatchTier::Metric => 2,
            MatchTier::Substitute => 3,
            MatchTier::Download => 4,
        }
    }

    pub fn from_code(c: u8) -> Result<MatchTier, MatchError> {
        match c {
            0 => Ok(MatchTier::Exact),
            1 => Ok(MatchTier::Normalized),
            2 => Ok(MatchTier::Metric),
            3 => Ok(MatchTier::Substitute),
            4 => Ok(MatchTier::Download),
            _ => Err(MatchError::Tier),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MatchTier::Exact => "exact",
            MatchTier::Normalized => "normalized",
            MatchTier::Metric => "metric",
            MatchTier::Substitute => "substitute",
            MatchTier::Download => "download",
        }
    }

    pub fn needs_download(self) -> bool {
        matches!(self, MatchTier::Download)
    }
}

/// 匹配结论。`pick` 是驻留表下标；`Download` 时必为 `None`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchOutcome {
    pub tier: MatchTier,
    pub pick: Option<usize>,
    pub score: u32,
    pub reason: &'static str,
}

impl MatchOutcome {
    pub fn needs_download(&self) -> bool {
        self.tier.needs_download() && self.pick.is_none()
    }

    pub fn font_index(&self) -> Option<usize> {
        self.pick
    }
}

/// 驻留字体表：注册表语义——名字唯一、上限 32、构造时拒绝空表。
#[derive(Clone, Debug)]
pub struct ResidentTable {
    fonts: Vec<ResidentFont>,
    cap: usize,
}

impl ResidentTable {
    pub fn builtin() -> ResidentTable {
        let mut fonts = Vec::with_capacity(RESIDENT_FONTS.len());
        let mut i = 0usize;
        while i < RESIDENT_FONTS.len() {
            fonts.push(RESIDENT_FONTS[i]);
            i += 1;
        }
        ResidentTable { fonts, cap: 32 }
    }

    /// 定容构造（自检用）：容量可小于 32，注册第 cap+1 款时必须报 Cap。
    pub fn with_cap(fonts: Vec<ResidentFont>, cap: usize) -> Result<ResidentTable, MatchError> {
        if fonts.is_empty() {
            return Err(MatchError::Table);
        }
        if cap == 0 || fonts.len() > cap {
            return Err(MatchError::Cap);
        }
        let mut i = 0usize;
        while i < fonts.len() {
            let mut j = i + 1;
            while j < fonts.len() {
                if fonts[i].name == fonts[j].name {
                    return Err(MatchError::Dup);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(ResidentTable { fonts, cap })
    }

    /// 构造即校验：空表 / 超容 / 重名都在这里挡住，避免匹配期才发现
    /// 「表里有两份同名字体，pick 结果取决于遍历顺序」。
    pub fn new(fonts: Vec<ResidentFont>) -> Result<ResidentTable, MatchError> {
        if fonts.is_empty() {
            return Err(MatchError::Table);
        }
        if fonts.len() > 32 {
            return Err(MatchError::Cap);
        }
        let mut i = 0usize;
        while i < fonts.len() {
            let mut j = i + 1;
            while j < fonts.len() {
                if fonts[i].name == fonts[j].name {
                    return Err(MatchError::Dup);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(ResidentTable { fonts, cap: 32 })
    }

    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&ResidentFont> {
        self.fonts.get(i)
    }

    pub fn register(&mut self, f: ResidentFont) -> Result<usize, MatchError> {
        let mut i = 0usize;
        while i < self.fonts.len() {
            if self.fonts[i].name == f.name {
                return Err(MatchError::Dup);
            }
            i += 1;
        }
        if self.fonts.len() >= self.cap {
            return Err(MatchError::Cap);
        }
        self.fonts.push(f);
        Ok(self.fonts.len() - 1)
    }

    /// 在满足 `pred` 的候选里取「度量分最低，同分按名字字典序」的一个。
    /// 字典序兜底让结果与表内顺序无关——换一个插入顺序仍选同一字体。
    fn best_of(
        &self,
        req: &FontRequest,
        tier: MatchTier,
        pred: &dyn Fn(&ResidentFont) -> bool,
        reason: &'static str,
    ) -> Option<MatchOutcome> {
        let mut best: Option<(usize, u32, &'static str)> = None;
        let mut i = 0usize;
        while i < self.fonts.len() {
            let f = self.fonts[i];
            if pred(&f) {
                let sc = metric_score(req, &f);
                let take = match best {
                    None => true,
                    Some((_, bs, bn)) => sc < bs || (sc == bs && f.name < bn),
                };
                if take {
                    best = Some((i, sc, f.name));
                }
            }
            i += 1;
        }
        match best {
            Some((i, sc, _)) => Some(MatchOutcome {
                tier,
                pick: Some(i),
                score: sc,
                reason,
            }),
            None => None,
        }
    }
}

/// 度量档阈值：经验值——衬线/字距同级时，upem+ascent+glyph 的综合偏差
/// 超过这个量，替身字体的行高与断行已经不可接受，必须显式下载。
pub const METRIC_MAX: u32 = 600;

/// 匹配器：持有驻留表，纯函数式匹配，不改表。
#[derive(Clone, Debug)]
pub struct Matcher {
    table: ResidentTable,
}

impl Matcher {
    pub fn new(table: ResidentTable) -> Matcher {
        Matcher { table }
    }

    pub fn builtin() -> Matcher {
        Matcher::new(ResidentTable::builtin())
    }

    pub fn table(&self) -> &ResidentTable {
        &self.table
    }

    /// 主入口：按五档优先级返回结论。返回 `Err` 只表示「请求本身非法」，
    /// 合法请求永远不会因为「没找到」而报错——没找到就是 Download 档。
    pub fn match_font(&self, req: &FontRequest) -> Result<MatchOutcome, MatchError> {
        validate_request(req)?;
        if self.table.is_empty() {
            return Err(MatchError::Table);
        }
        let want_full = canon_name(&req.family);
        let want_fam = strip_style(&want_full);

        // 档 1：全名 + 样式完全一致
        let exact = self.table.best_of(
            req,
            MatchTier::Exact,
            &|f| canon_name(f.name) == want_full && f.bold == req.bold && f.italic == req.italic,
            "resident full-name hit",
        );
        if let Some(o) = exact {
            return Ok(o);
        }

        // 档 2：家族名一致（样式允许被替换，分数里已计罚）
        let fam = self.table.best_of(
            req,
            MatchTier::Normalized,
            &|f| {
                canon_name(f.family) == want_fam || strip_style(&canon_name(f.name)) == want_fam
            },
            "resident family hit",
        );
        if let Some(o) = fam {
            return Ok(o);
        }

        // 档 3/4：同衬线类 + 同字距档；先算替身候选，再看度量分归哪档
        let subst = self.table.best_of(
            req,
            MatchTier::Substitute,
            &|f| f.serif == req.serif && pitch_compatible(req.pitch, f.pitch),
            "resident substitute by class",
        );
        if let Some(o) = subst {
            if o.score <= METRIC_MAX {
                return Ok(MatchOutcome {
                    tier: MatchTier::Metric,
                    pick: o.pick,
                    score: o.score,
                    reason: "resident metric-compatible hit",
                });
            }
            return Ok(o);
        }

        Ok(MatchOutcome {
            tier: MatchTier::Download,
            pick: None,
            score: 0,
            reason: "no resident candidate; explicit download fallback",
        })
    }
}

/// 单条判定账目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchRecord {
    pub tier: MatchTier,
    pub pick: Option<usize>,
    pub score: u32,
}

/// 判定账本：append-only、定容、封账后拒写。
#[derive(Clone, Debug)]
pub struct MatchLedger {
    recs: Vec<MatchRecord>,
    cap: usize,
    sealed: bool,
}

impl MatchLedger {
    pub fn new(cap: usize) -> MatchLedger {
        MatchLedger {
            recs: Vec::new(),
            cap,
            sealed: false,
        }
    }

    pub fn push(&mut self, r: MatchRecord) -> Result<(), MatchError> {
        if self.sealed {
            return Err(MatchError::Seal);
        }
        if self.recs.len() >= self.cap {
            return Err(MatchError::Cap);
        }
        self.recs.push(r);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    pub fn record(&self, i: usize) -> Option<&MatchRecord> {
        self.recs.get(i)
    }

    /// 按档位计数（下标 = MatchTier::code），用于汇报「精确/家族/度量/
    /// 替身/下载」各自多少单。
    pub fn tally(&self) -> [usize; 5] {
        let mut out = [0usize; 5];
        let mut i = 0usize;
        while i < self.recs.len() {
            let c = self.recs[i].tier.code() as usize;
            if c < out.len() {
                out[c] += 1;
            }
            i += 1;
        }
        out
    }

    pub fn fingerprint(&self) -> u32 {
        let mut h = 2166136261u32;
        let mut i = 0usize;
        while i < self.recs.len() {
            let r = self.recs[i];
            h = fnv_u32(h, r.tier.code() as u32);
            h = fnv_u32(h, match r.pick {
                Some(p) => p as u32 + 1,
                None => 0,
            });
            h = fnv_u32(h, r.score);
            i += 1;
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fingerprint()
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }
}

fn fnv_u32(mut h: u32, v: u32) -> u32 {
    let mut i = 0usize;
    while i < 4 {
        h ^= (v >> (i * 8)) & 0xFF;
        h = h.wrapping_mul(16777619);
        i += 1;
    }
    h
}

/// 匹配会话状态机：Draft → Validated → Resolved。请求只能在校验后解析，
/// 防止调用方在 session 上「先解析后填请求」拿到上一单的陈旧结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchState {
    Draft,
    Validated,
    Resolved,
}

#[derive(Clone, Debug)]
pub struct MatchSession {
    state: MatchState,
    req: Option<FontRequest>,
    out: Option<MatchOutcome>,
}

impl MatchSession {
    pub fn new() -> MatchSession {
        MatchSession {
            state: MatchState::Draft,
            req: None,
            out: None,
        }
    }

    pub fn state(&self) -> MatchState {
        self.state
    }

    pub fn set_request(&mut self, r: FontRequest) -> Result<(), MatchError> {
        if self.state != MatchState::Draft {
            return Err(MatchError::State);
        }
        validate_request(&r)?;
        self.req = Some(r);
        self.state = MatchState::Validated;
        Ok(())
    }

    pub fn resolve(&mut self, m: &Matcher) -> Result<MatchOutcome, MatchError> {
        if self.state != MatchState::Validated {
            return Err(MatchError::State);
        }
        let req = match &self.req {
            Some(r) => r.clone(),
            None => return Err(MatchError::State),
        };
        let o = m.match_font(&req)?;
        self.out = Some(o);
        self.state = MatchState::Resolved;
        Ok(o)
    }

    pub fn outcome(&self) -> Option<MatchOutcome> {
        self.out
    }

    pub fn reset(&mut self) {
        if self.state == MatchState::Resolved {
            self.state = MatchState::Draft;
            self.req = None;
            self.out = None;
        }
    }
}

// ---------------------------------------------------------------------------
// 注入核：8 类坏请求/坏表必须全部被抓
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;
    let font = RESIDENT_FONTS[0];

    let mut r = FontRequest::like(&font, String::new());
    if validate_request(&r).err() == Some(MatchError::Empty) {
        caught += 1;
    }
    let mut long = String::new();
    let mut i = 0usize;
    while i < 65 {
        long.push('a');
        i += 1;
    }
    r = FontRequest::like(&font, long);
    if validate_request(&r).err() == Some(MatchError::Name) {
        caught += 1;
    }
    r = FontRequest::like(&font, "X".to_string());
    r.pitch = 301;
    if validate_request(&r).err() == Some(MatchError::Pitch) {
        caught += 1;
    }
    r = FontRequest::like(&font, "X".to_string());
    r.upem = 8;
    if validate_request(&r).err() == Some(MatchError::Upem) {
        caught += 1;
    }
    r = FontRequest::like(&font, "X".to_string());
    r.ascent = 0;
    if validate_request(&r).err() == Some(MatchError::Metric) {
        caught += 1;
    }
    r = FontRequest::like(&font, "X".to_string());
    r.glyphs = 0;
    if validate_request(&r).err() == Some(MatchError::Glyph) {
        caught += 1;
    }
    if MatchTier::from_code(9).err() == Some(MatchError::Tier) {
        caught += 1;
    }
    if ResidentTable::new(Vec::new()).err() == Some(MatchError::Table) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50083_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50083");
    let matcher = Matcher::builtin();
    let table = matcher.table();

    let mut ledger = MatchLedger::new(64);
    let mut cap_exact = 0usize;
    let mut cap_norm = 0usize;
    let mut cap_metric = 0usize;
    let mut cap_subst = 0usize;
    let mut cap_dl = 0usize;
    let mut consistent = true;
    let mut total_matched = 0usize;

    // 组 A：8 单全名精确命中（名字 = 驻留全名）
    let mut i = 0usize;
    while i < 8 {
        let f = RESIDENT_FONTS[i];
        let req = FontRequest::like(&f, f.name.to_string());
        let o = match matcher.match_font(&req) {
            Ok(v) => v,
            Err(_) => {
                consistent = false;
                i += 1;
                continue;
            }
        };
        total_matched += 1;
        if o.tier != MatchTier::Exact {
            consistent = false;
        }
        match o.pick {
            Some(p) => match table.get(p) {
                Some(hit) => {
                    if canon_name(hit.name) != canon_name(&req.family)
                        || hit.bold != req.bold
                        || hit.italic != req.italic
                    {
                        consistent = false;
                    }
                }
                None => consistent = false,
            },
            None => consistent = false,
        }
        if o.tier == MatchTier::Exact {
            cap_exact += 1;
        }
        let _ = ledger.push(MatchRecord { tier: o.tier, pick: o.pick, score: o.score });
        i += 1;
    }

    // 组 B：8 单家族命中（家族名 + " MT" 后缀，迫使走 Normalized）
    let mut i = 0usize;
    while i < 8 {
        let f = RESIDENT_FONTS[i];
        let mut fam = String::new();
        fam.push_str(f.family);
        fam.push_str(" MT");
        let req = FontRequest::like(&f, fam);
        let o = match matcher.match_font(&req) {
            Ok(v) => v,
            Err(_) => {
                consistent = false;
                i += 1;
                continue;
            }
        };
        total_matched += 1;
        if o.tier != MatchTier::Normalized {
            consistent = false;
        }
        match o.pick {
            Some(p) => match table.get(p) {
                Some(hit) => {
                    if canon_name(hit.family) != strip_style(&canon_name(&req.family)) {
                        consistent = false;
                    }
                }
                None => consistent = false,
            },
            None => consistent = false,
        }
        if o.tier == MatchTier::Normalized {
            cap_norm += 1;
        }
        let _ = ledger.push(MatchRecord { tier: o.tier, pick: o.pick, score: o.score });
        i += 1;
    }

    // 组 C：6 单度量命中（未知家族，度量取自驻留字体）
    let metric_targets = [4usize, 5, 6, 2, 3, 0];
    let mut i = 0usize;
    while i < metric_targets.len() {
        let f = RESIDENT_FONTS[metric_targets[i]];
        let req = FontRequest::like(&f, format!("Mystery Metric {}", i));
        let o = match matcher.match_font(&req) {
            Ok(v) => v,
            Err(_) => {
                consistent = false;
                i += 1;
                continue;
            }
        };
        total_matched += 1;
        if o.tier != MatchTier::Metric {
            consistent = false;
        }
        match o.pick {
            Some(p) => match table.get(p) {
                Some(hit) => {
                    if hit.serif != req.serif || !pitch_compatible(hit.pitch, req.pitch) {
                        consistent = false;
                    }
                }
                None => consistent = false,
            },
            None => consistent = false,
        }
        if o.tier == MatchTier::Metric {
            cap_metric += 1;
        }
        let _ = ledger.push(MatchRecord { tier: o.tier, pick: o.pick, score: o.score });
        i += 1;
    }

    // 组 D：2 单度量差距过大 → 替身档
    let mut i = 0usize;
    while i < 2 {
        let mut req = FontRequest::like(&RESIDENT_FONTS[5], format!("Mystery Sub {}", i));
        req.upem = 2048;
        req.ascent = 900;
        req.cap_height = 800;
        req.glyphs = 6000;
        let o = match matcher.match_font(&req) {
            Ok(v) => v,
            Err(_) => {
                consistent = false;
                i += 1;
                continue;
            }
        };
        total_matched += 1;
        if o.tier != MatchTier::Substitute || o.score <= METRIC_MAX {
            consistent = false;
        }
        if o.pick.is_none() {
            consistent = false;
        }
        if o.tier == MatchTier::Substitute {
            cap_subst += 1;
        }
        let _ = ledger.push(MatchRecord { tier: o.tier, pick: o.pick, score: o.score });
        i += 1;
    }

    // 组 E：6 单无同字距候选 → 显式下载回退
    let mut i = 0usize;
    while i < 6 {
        let mut req = FontRequest::like(&RESIDENT_FONTS[5], format!("Ghost {}", i));
        req.pitch = 250;
        req.serif = i % 2 == 0;
        let o = match matcher.match_font(&req) {
            Ok(v) => v,
            Err(_) => {
                consistent = false;
                i += 1;
                continue;
            }
        };
        total_matched += 1;
        if o.tier != MatchTier::Download || o.pick.is_some() || !o.needs_download() {
            consistent = false;
        }
        if o.tier == MatchTier::Download {
            cap_dl += 1;
        }
        let _ = ledger.push(MatchRecord { tier: o.tier, pick: o.pick, score: o.score });
        i += 1;
    }

    // J1-1 30 单全部解析且与驻留表一致
    let ok = consistent
        && total_matched == 30
        && ledger.len() == 30
        && cap_exact == 8
        && cap_norm == 8
        && cap_metric == 6
        && cap_subst == 2
        && cap_dl == 6;
    let det = format!(
        "n={} E{} N{} M{} S{} D{}",
        total_matched, cap_exact, cap_norm, cap_metric, cap_subst, cap_dl
    );
    s.add_dyn("50083-J1-三十单匹配一致", ok, &det);

    // J1-2 未命中显式回退下载（pick 必空、reason 非空）
    let ghost = FontRequest::like(&RESIDENT_FONTS[5], "Absent Family".to_string());
    let mut gp = ghost.clone();
    gp.pitch = 250;
    let ok = match matcher.match_font(&gp) {
        Ok(o) => o.needs_download() && o.pick.is_none() && !o.reason.is_empty(),
        Err(_) => false,
    };
    s.add("50083-J1-未命中回退", ok, if ok { "" } else { "未命中未显式回退" });

    // J1-3 账本计数与逐档分布守恒
    let tally = ledger.tally();
    let ok = tally[0] == 8
        && tally[1] == 8
        && tally[2] == 6
        && tally[3] == 2
        && tally[4] == 6;
    let det = format!("{:?}", tally);
    s.add_dyn("50083-J1-账本分档守恒", ok, &det);

    // J1-4 封账指纹稳定 + 封账拒写
    let f1 = ledger.fingerprint();
    let f2 = ledger.fingerprint();
    let sealed = ledger.seal();
    let after = ledger.push(MatchRecord {
        tier: MatchTier::Exact,
        pick: Some(0),
        score: 0,
    });
    let ok = f1 == f2 && sealed == f1 && after.err() == Some(MatchError::Seal);
    s.add("50083-J2-封账指纹", ok, if ok { "" } else { "指纹漂移或封账可写" });

    // J1-5 确定性：同请求两次逐字段一致
    let req = FontRequest::like(&RESIDENT_FONTS[2], "Times-Extra".to_string());
    let a = matcher.match_font(&req);
    let b = matcher.match_font(&req);
    let ok = match (a, b) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    };
    s.add("50083-J2-匹配确定", ok, if ok { "" } else { "两次结论不同" });

    // J1-6 结果与表内顺序无关（字典序兜底）
    let mut rev = Vec::with_capacity(RESIDENT_FONTS.len());
    let mut i = RESIDENT_FONTS.len();
    while i > 0 {
        i -= 1;
        rev.push(RESIDENT_FONTS[i]);
    }
    let rev_table = match ResidentTable::new(rev) {
        Ok(t) => t,
        Err(_) => {
            s.add("50083-J2-顺序无关", false, "反序表构造失败");
            return s;
        }
    };
    let rev_matcher = Matcher::new(rev_table);
    let ra = matcher.match_font(&req);
    let rb = rev_matcher.match_font(&req);
    let ok = match (ra, rb) {
        (Ok(x), Ok(y)) => x.tier == y.tier && x.score == y.score,
        _ => false,
    };
    s.add("50083-J2-顺序无关", ok, if ok { "" } else { "表序影响结论" });

    // J1-7 名字规范化：大小写/连字符/下划线等价
    let ok = canon_name("Times-Roman") == canon_name("times roman")
        && canon_name("Helvetica__Oblique") == canon_name("helvetica-oblique")
        && strip_style(&canon_name("Courier Bold MT")) == "courier";
    s.add("50083-J2-名字规范化", ok, if ok { "" } else { "规范化不等价" });

    // J1-8 家族命中样式计罚：错误样式仍落同家族但分数更高
    let base = FontRequest::like(&RESIDENT_FONTS[5], "Helvetica MT".to_string());
    let mut wrong = base.clone();
    wrong.bold = true;
    wrong.glyphs = 1000;
    let a = matcher.match_font(&base);
    let b = matcher.match_font(&wrong);
    let ok = match (a, b) {
        (Ok(x), Ok(y)) => {
            x.tier == MatchTier::Normalized
                && y.tier == MatchTier::Normalized
                && y.score > x.score
                && x.pick.is_some()
                && y.pick.is_some()
        }
        _ => false,
    };
    s.add("50083-J2-样式计罚", ok, if ok { "" } else { "样式罚分缺失" });

    // J1-9 度量阈值：同族度量好落 Metric，差距大落 Substitute
    let close = FontRequest::like(&RESIDENT_FONTS[6], "Unknown A".to_string());
    let mut far = close.clone();
    far.upem = 2048;
    far.glyphs = 6000;
    let a = matcher.match_font(&close);
    let b = matcher.match_font(&far);
    let ok = match (a, b) {
        (Ok(x), Ok(y)) => x.tier == MatchTier::Metric && y.tier == MatchTier::Substitute,
        _ => false,
    };
    s.add("50083-J2-度量阈值", ok, if ok { "" } else { "阈值分档错误" });

    // J1-10 会话状态机：乱序解析被拒、重设请求被拒
    let mut sess = MatchSession::new();
    let early = sess.resolve(&matcher).err() == Some(MatchError::State);
    let set = sess.set_request(FontRequest::like(&RESIDENT_FONTS[0], "Courier MT".to_string())).is_ok();
    let twice = sess
        .set_request(FontRequest::like(&RESIDENT_FONTS[1], "Courier MT".to_string()))
        .err()
        == Some(MatchError::State);
    let resolved = sess.resolve(&matcher).is_ok();
    let ok = early && set && twice && resolved;
    s.add("50083-J2-会话状态机", ok, if ok { "" } else { "状态机漏拦" });

    // J1-11 表注册：重名与超容被拒
    let mut reg = match ResidentTable::with_cap(vec_one(), 2) {
        Ok(t) => t,
        Err(_) => {
            s.add("50083-J2-表注册", false, "定容表构造失败");
            return s;
        }
    };
    let dup = reg.register(RESIDENT_FONTS[0]).err() == Some(MatchError::Dup);
    let added = reg.register(RESIDENT_FONTS[3]).is_ok();
    let cap_hit = reg.register(RESIDENT_FONTS[4]).err() == Some(MatchError::Cap);
    let ok = dup && added && cap_hit;
    s.add("50083-J2-表注册", ok, if ok { "" } else { "注册未拦" });

    // J1-12 档位码值往返
    let mut ok = true;
    let mut c = 0u8;
    while c < 5 {
        match MatchTier::from_code(c) {
            Ok(t) => {
                if t.code() != c {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        c += 1;
    }
    if MatchTier::from_code(5).is_ok() {
        ok = false;
    }
    s.add("50083-J2-档位往返", ok, if ok { "" } else { "档位码不闭合" });

    // J1-13 驻留表兜底规模：12 款且名字唯一
    let mut names_ok = table.len() == 12;
    let mut i = 0usize;
    while i < table.len() {
        let mut j = i + 1;
        while j < table.len() {
            match (table.get(i), table.get(j)) {
                (Some(a), Some(b)) => {
                    if a.name == b.name {
                        names_ok = false;
                    }
                }
                _ => names_ok = false,
            }
            j += 1;
        }
        i += 1;
    }
    s.add("50083-J2-驻留表规模", names_ok, if names_ok { "" } else { "表规模或重名" });

    // J1-14 注入核 8 类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50083-J1-注入必抓", ok, &det);

    // J1-15 码值闭集
    let codes = [
        MatchError::Empty,
        MatchError::Name,
        MatchError::Pitch,
        MatchError::Upem,
        MatchError::Metric,
        MatchError::Glyph,
        MatchError::Table,
        MatchError::Dup,
        MatchError::Cap,
        MatchError::Tier,
        MatchError::State,
        MatchError::Seal,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < codes.len() {
        let c = codes[i].code();
        if c <= SEG_BASE || c > SEG_BASE + 12 {
            ok = false;
        }
        i += 1;
    }
    s.add("50083-J1-码值闭集", ok, if ok { "" } else { "码值越段" });

    s
}

/// 供注册表容量测试用的单字体表（容量由 with_cap 单独给 2）。
fn vec_one() -> Vec<ResidentFont> {
    let mut v = Vec::with_capacity(1);
    v.push(RESIDENT_FONTS[0]);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50083_checks_green() {
        let set = run_unx50083_checks();
        assert!(set.all_passed(), "UNX-F50083 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50083_download_is_explicit() {
        let m = Matcher::builtin();
        let mut req = FontRequest::like(&RESIDENT_FONTS[0], "No Such Face".to_string());
        req.pitch = 250;
        let o = m.match_font(&req);
        assert!(o.is_ok());
        if let Ok(v) = o {
            assert!(v.needs_download());
            assert!(v.pick.is_none());
        }
    }

    #[test]
    fn unx50083_injection() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
    }
}
