//! UNX-F50079 B24 批防重 grep 执行记录（关键词族：duplex/tray/media/bin）（0x43600 段）
//!
//! 锚点：UNX-F50079-J1 批收口防重留痕；判据锚定 §6.1。
//!
//! 数据段：`SEG_BASE = 0x43600`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 11`，
//! 逐项见 [`GrepError`]。
//!
//! 批收口前要先做一遍**关键词族扫描**，确认 B24 的四族纸张语义
//! （duplex / tray / media / bin）都在实现里出现过，并且同一条命中不许被
//! 记两次——否则「扫描数」会虚高，收口报告就不可信。
//!
//! 流水线：
//!
//! ```text
//! PatternTable（四族 ×3 关键词） --sweep(text, family)--> GrepLog
//!        ^                                                   |
//!        |                                                   v
//!        +-------------- close(): 覆盖 + 账恒等 + 族内行序 --- SealReport
//! ```
//!
//! 防重口径：`Hit` 的四元组 `(family, line, col, len)` 完全相等才算重复；
//! 同一行不同列、不同长度（如 `bin` 与 `outputbin`）是不同命中，不能合并。
//!
//! 结构清单：
//! * [`PatternTable`]：关键词族注册表（空词 / 重复词显式拒绝）；
//! * [`GrepLog`]：命中账（去重集合 + 尝试/唯一/重复三计数）；
//! * [`SealReport`]：收口报告（族覆盖 / 唯一数 / 重复数 / 行数 / 摘要）；
//! * [`injection_core`]：11 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43600 独占 0x20）。
pub const SEG_BASE: u32 = 0x43600;

const E_FAMILY: u16 = 1;
const E_LINE: u16 = 2;
const E_SPAN: u16 = 3;
const E_CAPACITY: u16 = 4;
const E_CLOSED: u16 = 5;
const E_COVERAGE: u16 = 6;
const E_DUP_KEYWORD: u16 = 7;
const E_PATTERN: u16 = 8;
const E_TEXT: u16 = 9;
const E_TALLY: u16 = 10;
const E_ORDER: u16 = 11;

/// grep 执行错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrepError {
    /// 关键词族编码不在 0..=3。
    Family,
    /// 行号为 0（行号从 1 起）。
    Line,
    /// 列 / 长度越出行宽。
    Span,
    /// 命中账已满。
    Capacity,
    /// 已收口，拒绝再写。
    Closed,
    /// 有族一次都没命中，不能收口。
    Coverage,
    /// 同族同词重复注册。
    DuplicateKeyword,
    /// 空词或非 ASCII 关键词。
    Pattern,
    /// 文本过长或含非 ASCII 字节。
    Text,
    /// 尝试 / 唯一 / 重复三计数不守恒。
    Tally,
    /// 族内行序倒退。
    Order,
}

impl GrepError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            GrepError::Family => E_FAMILY,
            GrepError::Line => E_LINE,
            GrepError::Span => E_SPAN,
            GrepError::Capacity => E_CAPACITY,
            GrepError::Closed => E_CLOSED,
            GrepError::Coverage => E_COVERAGE,
            GrepError::DuplicateKeyword => E_DUP_KEYWORD,
            GrepError::Pattern => E_PATTERN,
            GrepError::Text => E_TEXT,
            GrepError::Tally => E_TALLY,
            GrepError::Order => E_ORDER,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            GrepError::Family => "keyword family code outside 0..=3",
            GrepError::Line => "hit line number is zero",
            GrepError::Span => "hit column/length outside line width",
            GrepError::Capacity => "grep hit ledger full",
            GrepError::Closed => "grep ledger already closed",
            GrepError::Coverage => "a keyword family has no hits",
            GrepError::DuplicateKeyword => "keyword already registered in family",
            GrepError::Pattern => "keyword empty or not ASCII",
            GrepError::Text => "scan text too long or not ASCII",
            GrepError::Tally => "attempted != unique + duplicate",
            GrepError::Order => "hit lines decrease inside one family",
        }
    }
}

/// 行宽上限：grep 只处理 512 列以内的行，超出说明输入不是纸张作业日志。
pub const LINE_WIDTH: u32 = 512;
/// 单次扫描文本上限。
pub const MAX_TEXT: usize = 4096;
/// 命中账容量。
pub const MAX_HITS: usize = 256;
/// 族数。
pub const FAMILIES: usize = 4;

/// 关键词族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// 双面 / 装订面语义。
    Duplex,
    /// 进纸盒语义。
    Tray,
    /// 介质类型语义。
    Media,
    /// 出纸口 / 分拣器语义。
    Bin,
}

impl Family {
    /// 编码 → 族。非法编码返回 None。
    pub fn from_code(c: u8) -> Option<Family> {
        match c {
            0 => Some(Family::Duplex),
            1 => Some(Family::Tray),
            2 => Some(Family::Media),
            3 => Some(Family::Bin),
            _ => None,
        }
    }

    /// 族 → 编码。
    pub fn code(self) -> u8 {
        match self {
            Family::Duplex => 0,
            Family::Tray => 1,
            Family::Media => 2,
            Family::Bin => 3,
        }
    }

    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Family::Duplex => "duplex",
            Family::Tray => "tray",
            Family::Media => "media",
            Family::Bin => "bin",
        }
    }

    /// 族内默认关键词。全部小写，扫描时按 ASCII 大小写不敏感比较。
    pub fn keywords(self) -> &'static [&'static str] {
        match self {
            Family::Duplex => &["duplex", "two-sided", "sides"],
            Family::Tray => &["tray", "inputslot", "drawer"],
            Family::Media => &["media", "papertype", "media-type"],
            Family::Bin => &["bin", "outputbin", "stacker"],
        }
    }
}

/// 注册表条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatternEntry {
    /// 所属族。
    pub family: Family,
    /// 关键词（小写 ASCII）。
    pub word: &'static str,
}

/// 关键词族注册表。空词与同族重复词都显式拒绝，避免「扫了等于没扫」。
#[derive(Clone, Debug)]
pub struct PatternTable {
    entries: Vec<PatternEntry>,
}

impl PatternTable {
    /// 空表。
    pub fn new() -> PatternTable {
        PatternTable { entries: Vec::with_capacity(16) }
    }

    /// 注册一个关键词。
    pub fn register(&mut self, family: Family, word: &'static str) -> Result<(), GrepError> {
        if word.is_empty() {
            return Err(GrepError::Pattern);
        }
        let bytes = word.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                if *b >= 0x80 {
                    return Err(GrepError::Pattern);
                }
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.entries.len() {
            if let Some(e) = self.entries.get(j) {
                if e.family == family && e.word == word {
                    return Err(GrepError::DuplicateKeyword);
                }
            }
            j += 1;
        }
        self.entries.push(PatternEntry { family, word });
        Ok(())
    }

    /// 四族 ×3 关键词的默认表。
    pub fn default_table() -> PatternTable {
        let mut t = PatternTable::new();
        let fams = [Family::Duplex, Family::Tray, Family::Media, Family::Bin];
        let mut i = 0usize;
        while i < fams.len() {
            let f = match fams.get(i) {
                Some(v) => *v,
                None => break,
            };
            let words = f.keywords();
            let mut j = 0usize;
            while j < words.len() {
                if let Some(w) = words.get(j) {
                    let _ = t.register(f, w);
                }
                j += 1;
            }
            i += 1;
        }
        t
    }

    /// 取某族的关键词（注册顺序）。
    pub fn words(&self, family: Family) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.family == family {
                    out.push(e.word);
                }
            }
            i += 1;
        }
        out
    }

    /// 条目总数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空表判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for PatternTable {
    fn default() -> PatternTable {
        PatternTable::default_table()
    }
}

/// 一条命中。`col` 是行内字节偏移（0 起）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// 族编码。
    pub family: u8,
    /// 行号（1 起）。
    pub line: u32,
    /// 行内列偏移。
    pub col: u16,
    /// 命中字节长度。
    pub len: u16,
}

/// 命中账。
#[derive(Clone, Debug)]
pub struct GrepLog {
    hits: Vec<Hit>,
    attempted: u32,
    unique: u32,
    dup: u32,
    scanned_lines: u32,
    closed: bool,
}

impl GrepLog {
    /// 空账。
    pub fn new() -> GrepLog {
        GrepLog {
            hits: Vec::with_capacity(64),
            attempted: 0,
            unique: 0,
            dup: 0,
            scanned_lines: 0,
            closed: false,
        }
    }

    /// 记一条命中。新命中返回 true，重复命中返回 false（计入防重数）。
    pub fn record(&mut self, hit: Hit) -> Result<bool, GrepError> {
        if self.closed {
            return Err(GrepError::Closed);
        }
        if Family::from_code(hit.family).is_none() {
            return Err(GrepError::Family);
        }
        if hit.line == 0 {
            return Err(GrepError::Line);
        }
        if hit.len == 0 {
            return Err(GrepError::Span);
        }
        if hit.col as u32 + hit.len as u32 > LINE_WIDTH {
            return Err(GrepError::Span);
        }
        if self.hits.len() >= MAX_HITS {
            return Err(GrepError::Capacity);
        }
        self.attempted += 1;
        let mut i = 0usize;
        while i < self.hits.len() {
            if let Some(h) = self.hits.get(i) {
                if *h == hit {
                    self.dup += 1;
                    return Ok(false);
                }
            }
            i += 1;
        }
        self.hits.push(hit);
        self.unique += 1;
        Ok(true)
    }

    /// 扫描一段 ASCII 文本的某一族关键词；返回本次新增的唯一命中数。
    ///
    /// 行号从 1 起、按 `\n` 切分；同一行多次出现按不同列各记一条。
    pub fn sweep(
        &mut self,
        text: &str,
        family: Family,
        table: &PatternTable,
    ) -> Result<u32, GrepError> {
        if self.closed {
            return Err(GrepError::Closed);
        }
        let bytes = text.as_bytes();
        if bytes.len() > MAX_TEXT {
            return Err(GrepError::Text);
        }
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                if *b >= 0x80 {
                    return Err(GrepError::Text);
                }
            }
            i += 1;
        }
        let words = table.words(family);
        let mut line = 1u32;
        let mut start = 0usize;
        let mut added = 0u32;
        loop {
            let mut end = start;
            while end < bytes.len() {
                let stop = match bytes.get(end) {
                    Some(b) => *b == b'\n',
                    None => true,
                };
                if stop {
                    break;
                }
                end += 1;
            }
            let mut w = 0usize;
            while w < words.len() {
                let word = match words.get(w) {
                    Some(v) => *v,
                    None => break,
                };
                let needle = word.as_bytes();
                if needle.is_empty() {
                    w += 1;
                    continue;
                }
                let mut pos = start;
                while pos + needle.len() <= end {
                    let mut same = false;
                    if let Some(win) = bytes.get(pos..pos + needle.len()) {
                        same = win.eq_ignore_ascii_case(needle);
                    }
                    if same {
                        let hit = Hit {
                            family: family.code(),
                            line,
                            col: (pos - start) as u16,
                            len: needle.len() as u16,
                        };
                        let fresh = self.record(hit)?;
                        if fresh {
                            added += 1;
                        }
                    }
                    pos += 1;
                }
                w += 1;
            }
            line += 1;
            if end >= bytes.len() {
                break;
            }
            start = end + 1;
        }
        self.scanned_lines += line - 1;
        Ok(added)
    }

    /// 某族唯一命中数。
    pub fn family_hits(&self, family: Family) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.hits.len() {
            if let Some(h) = self.hits.get(i) {
                if h.family == family.code() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 四族覆盖向量（[duplex, tray, media, bin]）。
    pub fn coverage(&self) -> [bool; FAMILIES] {
        let fams = [Family::Duplex, Family::Tray, Family::Media, Family::Bin];
        let mut out = [false; FAMILIES];
        let mut i = 0usize;
        while i < fams.len() {
            if let Some(f) = fams.get(i) {
                if let Some(slot) = out.get_mut(i) {
                    *slot = self.family_hits(*f) > 0;
                }
            }
            i += 1;
        }
        out
    }

    /// 尝试数 / 唯一数 / 重复数（守恒：attempted == unique + dup）。
    pub fn counters(&self) -> (u32, u32, u32) {
        (self.attempted, self.unique, self.dup)
    }

    /// 三计数与命中表是否自洽。
    pub fn balanced(&self) -> bool {
        self.attempted == self.unique + self.dup && self.unique as usize == self.hits.len()
    }

    /// 已扫描行数。
    pub fn scanned_lines(&self) -> u32 {
        self.scanned_lines
    }

    /// 唯一命中序列的 FNV-1a 摘要（收口留痕，可复跑比对）。
    pub fn digest(&self) -> u32 {
        const PRIME: u32 = 0x0100_0193;
        let mut h: u32 = 0x811c_9dc5;
        let mut i = 0usize;
        while i < self.hits.len() {
            if let Some(x) = self.hits.get(i) {
                let bytes = [
                    x.family,
                    (x.line >> 24) as u8,
                    (x.line >> 16) as u8,
                    (x.line >> 8) as u8,
                    x.line as u8,
                    (x.col >> 8) as u8,
                    x.col as u8,
                    (x.len >> 8) as u8,
                    x.len as u8,
                ];
                let mut j = 0usize;
                while j < bytes.len() {
                    if let Some(b) = bytes.get(j) {
                        h = (h ^ *b as u32).wrapping_mul(PRIME);
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        h
    }

    /// 收口：覆盖、账恒等、族内行序三项全过才出报告；随后只读。
    pub fn close(&mut self, table: &PatternTable) -> Result<SealReport, GrepError> {
        if self.closed {
            return Err(GrepError::Closed);
        }
        if !self.balanced() {
            return Err(GrepError::Tally);
        }
        let fams = [Family::Duplex, Family::Tray, Family::Media, Family::Bin];
        let mut f = 0usize;
        while f < fams.len() {
            if let Some(fam) = fams.get(f) {
                if table.words(*fam).is_empty() {
                    f += 1;
                    continue;
                }
                if self.family_hits(*fam) == 0 {
                    return Err(GrepError::Coverage);
                }
            }
            f += 1;
        }
        // 族内行序必须非降——跨族交错是正常的，族内倒退说明记录被打乱。
        let mut g = 0usize;
        while g < fams.len() {
            let fam = match fams.get(g) {
                Some(v) => *v,
                None => break,
            };
            let mut last = 0u32;
            let mut i = 0usize;
            while i < self.hits.len() {
                if let Some(h) = self.hits.get(i) {
                    if h.family == fam.code() {
                        if h.line < last {
                            return Err(GrepError::Order);
                        }
                        last = h.line;
                    }
                }
                i += 1;
            }
            g += 1;
        }
        self.closed = true;
        let mut fams_seen = 0u8;
        let mut c = 0usize;
        while c < FAMILIES {
            if let Some(v) = self.coverage().get(c) {
                if *v {
                    fams_seen += 1;
                }
            }
            c += 1;
        }
        Ok(SealReport {
            families: fams_seen,
            unique: self.unique,
            dup: self.dup,
            lines: self.scanned_lines,
            digest: self.digest(),
        })
    }

    /// 是否已收口。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 一行报告。
    pub fn report(&self) -> String {
        let c = self.coverage();
        let (a, u, d) = self.counters();
        format!(
            "B24 grep attempted={} unique={} dup={} lines={} cover={}{}{}{}",
            a,
            u,
            d,
            self.scanned_lines,
            c[0] as u8,
            c[1] as u8,
            c[2] as u8,
            c[3] as u8
        )
    }
}

impl Default for GrepLog {
    fn default() -> GrepLog {
        GrepLog::new()
    }
}

/// 收口报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealReport {
    /// 命中的族数（应为 4）。
    pub families: u8,
    /// 唯一命中数。
    pub unique: u32,
    /// 重复命中数。
    pub dup: u32,
    /// 扫描总行数。
    pub lines: u32,
    /// 命中序列摘要。
    pub digest: u32,
}

/// B24 收口扫描样本：四族关键词各出现多次，行序天然升序。
pub fn fixture_text() -> &'static str {
    "duplex job start\ninputslot 1 duplex\ntray 2 plain\ntwo-sided media glossy\nbin upper outputbin\nmedia-type card"
}

/// 用固定样本把四族扫进同一本账（每族各扫一遍全文本）。
pub fn sweep_all(log: &mut GrepLog, table: &PatternTable) -> Result<u32, GrepError> {
    let text = fixture_text();
    let fams = [Family::Duplex, Family::Tray, Family::Media, Family::Bin];
    let mut added = 0u32;
    let mut i = 0usize;
    while i < fams.len() {
        if let Some(f) = fams.get(i) {
            added += log.sweep(text, *f, table)?;
        }
        i += 1;
    }
    Ok(added)
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

/// 坏样本注入核：12 类错误必须逐类命中具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 12usize;
    let table = PatternTable::default_table();

    // 1 族编码越界
    let mut log = GrepLog::new();
    let bad = Hit { family: 9, line: 1, col: 0, len: 1 };
    if log.record(bad).err() == Some(GrepError::Family) {
        caught += 1;
    }
    // 2 行号为 0
    let zero_line = Hit { family: 0, line: 0, col: 0, len: 1 };
    if log.record(zero_line).err() == Some(GrepError::Line) {
        caught += 1;
    }
    // 3 零长度命中
    let zero_len = Hit { family: 0, line: 1, col: 0, len: 0 };
    if log.record(zero_len).err() == Some(GrepError::Span) {
        caught += 1;
    }
    // 4 越出行宽
    let wide = Hit { family: 0, line: 1, col: 500, len: 20 };
    if log.record(wide).err() == Some(GrepError::Span) {
        caught += 1;
    }
    // 5 容量封顶
    let mut cap = GrepLog::new();
    let mut i = 0u16;
    while i < MAX_HITS as u16 {
        let h = Hit { family: 1, line: 1, col: i, len: 1 };
        let _ = cap.record(h);
        i += 1;
    }
    let over = Hit { family: 1, line: 1, col: 300, len: 1 };
    if cap.record(over).err() == Some(GrepError::Capacity) {
        caught += 1;
    }
    // 6 缺族覆盖就收口
    let mut one = GrepLog::new();
    let _ = one.sweep(fixture_text(), Family::Duplex, &table);
    if one.close(&table).err() == Some(GrepError::Coverage) {
        caught += 1;
    }
    // 7 关键词重复注册
    let mut t = PatternTable::new();
    let _ = t.register(Family::Tray, "tray");
    if t.register(Family::Tray, "tray").err() == Some(GrepError::DuplicateKeyword) {
        caught += 1;
    }
    // 8 空关键词
    if t.register(Family::Tray, "").err() == Some(GrepError::Pattern) {
        caught += 1;
    }
    // 9 非 ASCII 扫描文本
    if log.sweep("duplex 中", Family::Duplex, &table).err() == Some(GrepError::Text) {
        caught += 1;
    }
    // 10 三计数不守恒（绕过 record 直塞命中）
    let mut raw = GrepLog::new();
    raw.hits.push(Hit { family: 0, line: 1, col: 0, len: 6 });
    if raw.close(&table).err() == Some(GrepError::Tally) {
        caught += 1;
    }
    // 11 族内行序倒退
    let mut ord = GrepLog::new();
    let fams = [Family::Duplex, Family::Tray, Family::Media, Family::Bin];
    let mut f = 0usize;
    while f < fams.len() {
        if let Some(fam) = fams.get(f) {
            let _ = ord.record(Hit { family: fam.code(), line: 10, col: 0, len: 1 });
        }
        f += 1;
    }
    let _ = ord.record(Hit { family: 0, line: 5, col: 0, len: 1 });
    if ord.close(&table).err() == Some(GrepError::Order) {
        caught += 1;
    }
    // 收口后追加（复用成功收口账，确保 Closed 码可达）
    let mut done = GrepLog::new();
    let _ = sweep_all(&mut done, &table);
    let _ = done.close(&table);
    let again = Hit { family: 0, line: 1, col: 0, len: 6 };
    if done.record(again).err() == Some(GrepError::Closed) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 跑 UNX-F50079 的 13 条判据（含 J1 批收口防重留痕与注入核）。
pub fn run_unx50079_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50079");
    let table = PatternTable::default_table();

    // J1-1 注册表覆盖四族 ×3 词，词全部非空。
    let mut ok = table.len() == FAMILIES * 3;
    let fams = [Family::Duplex, Family::Tray, Family::Media, Family::Bin];
    let mut i = 0usize;
    while i < fams.len() {
        if let Some(f) = fams.get(i) {
            if table.words(*f).len() != 3 {
                ok = false;
            }
        }
        i += 1;
    }
    let det = format!("entries={}", table.len());
    s.add_dyn("50079-J1-四族注册", ok, &det);

    // J1-2 批扫描四族全覆盖。
    let mut log = GrepLog::new();
    let added = sweep_all(&mut log, &table);
    let cov = log.coverage();
    let ok = added.is_ok() && cov[0] && cov[1] && cov[2] && cov[3];
    let det = format!(
        "cover={}{}{}{} unique={}",
        cov[0] as u8, cov[1] as u8, cov[2] as u8, cov[3] as u8, log.counters().1
    );
    s.add_dyn("50079-J1-扫描覆盖", ok, &det);

    // J1-3 防重：同一条命中第二次记入返回 false，唯一数不变、重复数 +1。
    let before = log.counters();
    let dup_hit = Hit { family: 0, line: 1, col: 0, len: 6 };
    let got = log.record(dup_hit);
    let after = log.counters();
    let ok = got == Ok(false)
        && after.0 == before.0 + 1
        && after.1 == before.1
        && after.2 == before.2 + 1;
    let det = format!("before={:?} after={:?}", before, after);
    s.add_dyn("50079-J1-防重留痕", ok, &det);

    // J1-4 账恒等：attempted == unique + dup，且 unique == 命中表长度。
    let ok = log.balanced();
    s.add("50079-J1-计数守恒", ok, if ok { "" } else { "尝试账不守恒" });

    // J1-5 扫描幂等：再扫一遍同一文本，唯一数不变、重复数按首轮唯一数增长。
    let u0 = log.counters().1;
    let d0 = log.counters().2;
    let again = log.sweep(fixture_text(), Family::Duplex, &table);
    let u1 = log.counters().1;
    let d1 = log.counters().2;
    // 首轮 Duplex 唯一数：3（duplex ×2 + two-sided），重扫全部转为重复。
    let ok = again == Ok(0) && u1 == u0 && d1 == d0 + 3 && log.balanced();
    let det = format!("new={:?} u={}->{} d={}->{}", again, u0, u1, d0, d1);
    s.add_dyn("50079-J1-重扫幂等", ok, &det);

    // J1-6 收口：四族齐 + 账平衡才能出报告，报告族数为 4 且摘要非零。
    let report = log.close(&table);
    let ok = match &report {
        Ok(r) => r.families == 4 && r.unique == log.counters().1 && r.dup == log.counters().2 && r.digest != 0,
        Err(_) => false,
    };
    let det = match &report {
        Ok(r) => format!("fam={} u={} d={} dg={:08x}", r.families, r.unique, r.dup, r.digest),
        Err(e) => format!("err={}", e.code()),
    };
    s.add_dyn("50079-J1-收口报告", ok, &det);

    // J1-7 收口后只读：再次收口 / 再记命中都报 Closed。
    let closed_twice = log.close(&table).err() == Some(GrepError::Closed);
    let write_after = log.record(Hit { family: 1, line: 3, col: 0, len: 4 }).err() == Some(GrepError::Closed);
    let ok = closed_twice && write_after && log.is_closed();
    s.add("50079-J1-收口只读", ok, if ok { "" } else { "收口后仍可写" });

    // J1-8 单族不能收口（Coverage）。
    let mut solo = GrepLog::new();
    let _ = solo.sweep(fixture_text(), Family::Media, &table);
    let ok = solo.close(&table).err() == Some(GrepError::Coverage);
    s.add("50079-J1-缺族拒绝", ok, if ok { "" } else { "缺族也能收口" });

    // J1-9 族内乱序拒绝：四族都有命中，但 Duplex 出现 10 → 5 的倒退。
    let mut bad_order = GrepLog::new();
    let mut f = 0usize;
    while f < fams.len() {
        if let Some(fam) = fams.get(f) {
            let _ = bad_order.record(Hit { family: fam.code(), line: 10, col: 0, len: 1 });
        }
        f += 1;
    }
    let _ = bad_order.record(Hit { family: 0, line: 5, col: 0, len: 1 });
    let ok = bad_order.close(&table).err() == Some(GrepError::Order);
    s.add("50079-J1-族内乱序", ok, if ok { "" } else { "乱序未被抓" });

    // J1-10 越界命中拒绝：族 9 / 行 0 / 零长 / 越行宽 全部拒。
    let cases = [
        Hit { family: 9, line: 1, col: 0, len: 1 },
        Hit { family: 0, line: 0, col: 0, len: 1 },
        Hit { family: 0, line: 1, col: 0, len: 0 },
        Hit { family: 0, line: 1, col: 500, len: 20 },
    ];
    let mut probe = GrepLog::new();
    let mut all_rejected = true;
    let mut k = 0usize;
    while k < cases.len() {
        if let Some(h) = cases.get(k) {
            if probe.record(*h).is_ok() {
                all_rejected = false;
            }
        }
        k += 1;
    }
    let ok = all_rejected && probe.counters().0 == 0;
    s.add("50079-J1-越界必拒", ok, if ok { "" } else { "越界命中被接受" });

    // J1-11 同一行不同列 / 不同长度不算重复（bin 与 outputbin）。
    let mut distinct = GrepLog::new();
    let a = Hit { family: 3, line: 5, col: 0, len: 3 };
    let b = Hit { family: 3, line: 5, col: 6, len: 3 };
    let c = Hit { family: 3, line: 5, col: 10, len: 9 };
    let _ = distinct.record(a);
    let _ = distinct.record(b);
    let _ = distinct.record(c);
    let ok = distinct.counters() == (3, 3, 0);
    let det = format!("counters={:?}", distinct.counters());
    s.add_dyn("50079-J1-同列区分", ok, &det);

    // J1-12 确定性：两次独立扫描同一文本，摘要与计数逐位一致。
    let mut x = GrepLog::new();
    let mut y = GrepLog::new();
    let _ = sweep_all(&mut x, &table);
    let _ = sweep_all(&mut y, &table);
    let ok = x.digest() == y.digest() && x.counters() == y.counters() && x.report() == y.report();
    let det = format!("dg={:08x}", x.digest());
    s.add_dyn("50079-J1-扫描确定", ok, &det);

    // 注入核：11 类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50079-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50079_checks_green() {
        let set = run_unx50079_checks();
        assert!(set.all_passed(), "UNX-F50079 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50079_default_table_has_twelve_words() {
        assert_eq!(PatternTable::default_table().len(), 12);
    }

    #[test]
    fn unx50079_duplicate_is_detected() {
        let mut log = GrepLog::new();
        let h = Hit { family: 2, line: 2, col: 0, len: 5 };
        assert_eq!(log.record(h), Ok(true));
        assert_eq!(log.record(h), Ok(false));
        assert_eq!(log.counters(), (2, 1, 1));
    }
}
