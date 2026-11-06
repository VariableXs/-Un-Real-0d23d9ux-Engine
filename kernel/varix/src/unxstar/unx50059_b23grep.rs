//! UNX-F50059 B23 批防重 grep 执行记录（关键词族：watermark/header/footer/overlay）（0x43380 段）
//!
//! 锚点：UNX-F50059-J1 批收口防重留痕；判据锚定 §6.1。
//!
//! 数据段：`SEG_BASE = 0x43380`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 10`，
//! 逐项见 [`GrepError`]。
//!
//! 「防重」在批收口里是两层硬语义：
//! 1. **行级去重**——同一行被多个关键词、多个族命中只计一次；账本维护一条
//!    严格单调的全局并集 `distinct`，每次执行只回报本次**新增**行数 `added`；
//! 2. **执行级留痕**——同一 (族, 模式, 关键词集) 重复执行必须标 `repeat=true`
//!    且 `added==0`，不许把「跑了两遍」伪装成「又发现 N 行」。
//!
//! 扫描是真扫：逐字节大小写不敏感子串匹配（不依赖 `str::contains`，因为判据
//! 要求行为可复算，且对中文语料的 UTF-8 续字节不会误配——ASCII 关键词每字节
//! 都 <0x80，不可能落在多字节序列内部）。族是闭集，同一关键词换族重跑即
//! `FamilyMismatch`：族错标会让收口覆盖统计虚高。
//!
//! 结构清单：
//! * [`Family`] + [`KEYWORDS`]：四族关键词表（8 个关键词，逐族 2 个）；
//! * [`MatchMode`] + [`Query`]：any/all 两种匹配语义；
//! * [`RunRecord`] + [`GrepLedger`]：语料 + 执行账 + 全局并集 + 每行族掩码；
//! * [`ClosureReport`]：收口闸门（四族齐 + 去重行数达阈值才算收口）；
//! * [`injection_core`]：7 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43380 独占 0x20）。
pub const SEG_BASE: u32 = 0x43380;

const E_EMPTY_KEYWORD: u16 = 1;
const E_KEYWORD_LONG: u16 = 2;
const E_NON_ASCII: u16 = 3;
const E_CORPUS_EMPTY: u16 = 4;
const E_LEDGER_FULL: u16 = 5;
const E_FAMILY_MISMATCH: u16 = 6;
const E_UNKNOWN_FAMILY: u16 = 7;
const E_INCOMPLETE: u16 = 8;
const E_QUERY: u16 = 9;
const E_UNKNOWN_KEYWORD: u16 = 10;

/// grep 账错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrepError {
    /// 关键词为空或纯空白。
    EmptyKeyword,
    /// 关键词超长（>32 字节）。
    KeywordLong,
    /// 关键词含非 ASCII 字节（族表只收 ASCII 关键词）。
    NonAscii,
    /// 语料为空，扫无可扫。
    CorpusEmpty,
    /// 执行账已满。
    LedgerFull,
    /// 同一关键词换族重跑。
    FamilyMismatch,
    /// 族标签不认识。
    UnknownFamily,
    /// 收口条件未达成（族不齐或去重行数不足）。
    Incomplete,
    /// 查询本身非法（无关键词 / 超过 2 个关键词）。
    Query,
    /// 关键词不在族表里。
    UnknownKeyword,
}

impl GrepError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            GrepError::EmptyKeyword => E_EMPTY_KEYWORD,
            GrepError::KeywordLong => E_KEYWORD_LONG,
            GrepError::NonAscii => E_NON_ASCII,
            GrepError::CorpusEmpty => E_CORPUS_EMPTY,
            GrepError::LedgerFull => E_LEDGER_FULL,
            GrepError::FamilyMismatch => E_FAMILY_MISMATCH,
            GrepError::UnknownFamily => E_UNKNOWN_FAMILY,
            GrepError::Incomplete => E_INCOMPLETE,
            GrepError::Query => E_QUERY,
            GrepError::UnknownKeyword => E_UNKNOWN_KEYWORD,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            GrepError::EmptyKeyword => "keyword empty",
            GrepError::KeywordLong => "keyword longer than 32 bytes",
            GrepError::NonAscii => "keyword contains non-ascii bytes",
            GrepError::CorpusEmpty => "corpus empty",
            GrepError::LedgerFull => "grep ledger full",
            GrepError::FamilyMismatch => "keyword re-run under another family",
            GrepError::UnknownFamily => "family tag unknown",
            GrepError::Incomplete => "batch closure not reached",
            GrepError::Query => "query must carry 1..=2 keywords",
            GrepError::UnknownKeyword => "keyword not registered in any family",
        }
    }
}

/// 关键词族（B23 收口的四根柱子：水印本体 / 页眉 / 页脚 / 叠印层）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// `watermark` 族。
    Watermark,
    /// `header` 族。
    Header,
    /// `footer` 族。
    Footer,
    /// `overlay` 族。
    Overlay,
}

impl Family {
    /// 族下标 0..=3（掩码位与数组下标共用）。
    pub fn index(self) -> usize {
        match self {
            Family::Watermark => 0,
            Family::Header => 1,
            Family::Footer => 2,
            Family::Overlay => 3,
        }
    }

    /// 族标签。
    pub fn tag(self) -> &'static str {
        match self {
            Family::Watermark => "watermark",
            Family::Header => "header",
            Family::Footer => "footer",
            Family::Overlay => "overlay",
        }
    }

    /// 族掩码位（每行一个 u8，位 = 族）。
    pub fn bit(self) -> u8 {
        1u8 << self.index()
    }

    /// 解析族标签（大小写不敏感）；不认识返回 `None`，不兜底到水印族。
    pub fn from_tag(t: &str) -> Option<Family> {
        if eq_ci_str(t, "watermark") || eq_ci_str(t, "wm") {
            Some(Family::Watermark)
        } else if eq_ci_str(t, "header") || eq_ci_str(t, "hdr") {
            Some(Family::Header)
        } else if eq_ci_str(t, "footer") || eq_ci_str(t, "ftr") {
            Some(Family::Footer)
        } else if eq_ci_str(t, "overlay") || eq_ci_str(t, "ovl") {
            Some(Family::Overlay)
        } else {
            None
        }
    }

    /// 全部四族。
    pub const ALL: [Family; 4] = [Family::Watermark, Family::Header, Family::Footer, Family::Overlay];
}

/// 关键词上限（字节）。
pub const KEYWORD_CAP: usize = 32;

/// 单次查询的关键词上限。两词够用（母词 + 变体词），再多就该拆族了。
pub const QUERY_KEYWORDS_MAX: usize = 2;

/// 关键词表：四族共 8 个关键词，每族 2 个（族覆盖不允许挂零）。
///
/// 关键词**只允许出现在一族**里：`watermark` 既算水印又算叠印层的话，
/// 「族覆盖」就退化成「跑过一次就算过」，收口统计失去意义。
pub const KEYWORDS: [(&str, Family); 8] = [
    ("watermark", Family::Watermark),
    ("wmtext", Family::Watermark),
    ("header", Family::Header),
    ("hftpl", Family::Header),
    ("footer", Family::Footer),
    ("hfmirror", Family::Footer),
    ("overlay", Family::Overlay),
    ("ovl", Family::Overlay),
];

/// 收口所需的去重行数下限（语料 21 行，四族齐跑实测 20 行；门槛取 12
/// 留出语料增删余量，同时足以挡掉「只跑一族就收口」的假绿灯）。
pub const CLOSURE_MIN: u16 = 12;

/// 执行账容量（4 族 × 2 模式 × 2 关键词组合 + 重跑留痕，取 64）。
pub const MAX_RUNS: usize = 64;

/// 匹配模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchMode {
    /// 任一关键词命中即算。
    Any,
    /// 所有关键词都必须命中。
    All,
}

impl MatchMode {
    /// 模式码 0/1。
    pub fn code(self) -> u8 {
        match self {
            MatchMode::Any => 0,
            MatchMode::All => 1,
        }
    }

    /// 模式名。
    pub fn label(self) -> &'static str {
        match self {
            MatchMode::Any => "any",
            MatchMode::All => "all",
        }
    }
}

/// 一次查询：族 + 1..=2 个关键词 + 匹配模式。
#[derive(Clone, Copy, Debug)]
pub struct Query<'a> {
    /// 归属族。
    pub family: Family,
    /// 关键词切片（引用，不拷贝）。
    pub keywords: &'a [&'a str],
    /// 匹配模式。
    pub mode: MatchMode,
}

impl<'a> Query<'a> {
    /// 构造查询（不做校验，校验见 [`GrepLedger::run`]）。
    pub fn new(family: Family, keywords: &'a [&'a str], mode: MatchMode) -> Query<'a> {
        Query {
            family,
            keywords,
            mode,
        }
    }

    /// 身份串：族 + 模式 + 关键词（保序）。重跑判定与指纹都吃这一串。
    fn identity(&self) -> String {
        let mut s = String::from(self.family.tag());
        s.push('/');
        s.push_str(self.mode.label());
        let mut i = 0usize;
        while i < self.keywords.len() {
            s.push('|');
            if let Some(k) = self.keywords.get(i) {
                s.push_str(k);
            }
            i += 1;
        }
        s
    }
}

/// 一次执行的记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRecord {
    /// 执行序（0 起）。
    pub seq: u16,
    /// 族。
    pub family: Family,
    /// 模式。
    pub mode: MatchMode,
    /// 身份串（族/模式/关键词）。
    pub key: String,
    /// 本次命中行数（含已计过的行）。
    pub hits: u16,
    /// 本次新增到全局并集的行数。
    pub added: u16,
    /// 是否为同身份重跑。
    pub repeat: bool,
    /// 执行后全局并集大小。
    pub distinct_after: u16,
}

impl RunRecord {
    /// 一行落账文本（与批日志同构）。
    pub fn ledger_line(&self) -> String {
        format!(
            "#{} {} hits={} added={} repeat={} distinct={}",
            self.seq,
            self.key,
            self.hits,
            self.added,
            self.repeat,
            self.distinct_after
        )
    }
}

/// 收口报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClosureReport {
    /// 去重后的命中行数。
    pub distinct: u16,
    /// 语料总行数。
    pub corpus: u16,
    /// 未命中的行数（`corpus - distinct`）。
    pub pending: u16,
    /// 每族去重命中行数。
    pub per_family: [u16; 4],
}

impl ClosureReport {
    /// 四族是否都有命中（族覆盖挂零即不合格）。
    pub fn all_families(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            match self.per_family.get(i) {
                Some(v) if *v > 0 => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 是否已收口（去重行数达阈值且四族齐）。
    pub fn closed(&self) -> bool {
        self.all_families() && self.distinct >= CLOSURE_MIN
    }
}

/// B23 语料的替身：20 个模块头 + 1 行叠印层旁证。每行都是「模块短名 + 主题词」，
/// 与真实模块名对齐，保证 grep 命中的是收口面而不是虚构文本。
pub const CORPUS: [&str; 21] = [
    "unx50041_wmtext    watermark text stamp",
    "unx50042_wmimage  watermark raster overlay",
    "unx50043_wmvar    watermark variable data",
    "unx50044_hftpl    header footer template",
    "unx50045_wmlayer  watermark layer overlay",
    "unx50046_wmsecret watermark secret overlay",
    "unx50047_wmnup    n-up tiling sheet",
    "unx50048_wmduplex watermark duplex back",
    "unx50049_hfmirror header footer mirror",
    "unx50050_wmtrace  watermark trace audit",
    "unx50051_wmerrs   watermark error mask",
    "unx50052_wmsamples watermark samples sheet",
    "unx50053_wmfreeze watermark freeze api",
    "unx50054_wmfuzz   watermark fuzz injection",
    "unx50055_wmvec    watermark vector ledger",
    "unx50056_fontfallback header footer glyph",
    "unx50057_wmperf   watermark perf budget",
    "unx50058_b23vec   watermark case vector",
    "unx50059_b23grep  watermark grep record",
    "unx50060_b23seal  watermark batch seal",
    "unx50044b_ovljs   overlay json schema",
];

fn eq_ci(a: u8, b: u8) -> bool {
    a.to_ascii_lowercase() == b.to_ascii_lowercase()
}

/// 字符串级大小写不敏感相等（长度先比，避免前缀误判）。
pub fn eq_ci_str(a: &str, b: &str) -> bool {
    let x = a.as_bytes();
    let y = b.as_bytes();
    if x.len() != y.len() {
        return false;
    }
    let mut i = 0usize;
    while i < x.len() {
        match (x.get(i), y.get(i)) {
            (Some(p), Some(q)) => {
                if !eq_ci(*p, *q) {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 字节级大小写不敏感子串判定。
///
/// 非 ASCII 字节（≥0x80）经 `to_ascii_lowercase` 原样保留，而 ASCII 关键词
/// 每字节都 <0x80，所以按字节扫中文语料不会落在多字节序列内部误命中。
pub fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > hay.len() {
        return false;
    }
    let last = hay.len() - needle.len();
    let mut i = 0usize;
    while i <= last {
        let mut j = 0usize;
        let mut hit = true;
        while j < needle.len() {
            match (hay.get(i + j), needle.get(j)) {
                (Some(h), Some(n)) => {
                    if !eq_ci(*h, *n) {
                        hit = false;
                        break;
                    }
                }
                _ => {
                    hit = false;
                    break;
                }
            }
            j += 1;
        }
        if hit {
            return true;
        }
        i += 1;
    }
    false
}

/// 该关键词注册在哪个族（大小写不敏感）；未注册返回 `None`。
pub fn family_of(kw: &str) -> Option<Family> {
    let mut i = 0usize;
    while i < KEYWORDS.len() {
        if let Some((k, f)) = KEYWORDS.get(i) {
            if eq_ci_str(k, kw) {
                return Some(*f);
            }
        }
        i += 1;
    }
    None
}

/// FNV-1a 偏移基。
pub const FNV_OFFSET: u32 = 0x811c_9dc5;
/// FNV-1a 质数。
pub const FNV_PRIME: u32 = 0x0100_0193;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

/// 定容 grep 执行账：语料 + 全局并集 + 每行族掩码 + 执行记录。
pub struct GrepLedger {
    lines: Vec<&'static str>,
    hit: Vec<bool>,
    fam_mask: Vec<u8>,
    runs: Vec<RunRecord>,
    capacity: usize,
    next_seq: u16,
    rejected: u32,
}

impl GrepLedger {
    /// 以固定语料建账；空语料显式失败（空语料上收口恒真，必须挡住）。
    pub fn new(corpus: &'static [&'static str]) -> Result<GrepLedger, GrepError> {
        if corpus.is_empty() {
            return Err(GrepError::CorpusEmpty);
        }
        let n = corpus.len();
        let mut lines = Vec::with_capacity(n);
        let mut hit = Vec::with_capacity(n);
        let mut fam_mask = Vec::with_capacity(n);
        let mut i = 0usize;
        while i < n {
            if let Some(l) = corpus.get(i) {
                lines.push(*l);
                hit.push(false);
                fam_mask.push(0u8);
            }
            i += 1;
        }
        Ok(GrepLedger {
            lines,
            hit,
            fam_mask,
            runs: Vec::with_capacity(MAX_RUNS),
            capacity: MAX_RUNS,
            next_seq: 0,
            rejected: 0,
        })
    }

    /// 以内置 B23 语料建账。
    pub fn b23() -> GrepLedger {
        match GrepLedger::new(&CORPUS) {
            Ok(v) => v,
            Err(_) => GrepLedger {
                lines: Vec::new(),
                hit: Vec::new(),
                fam_mask: Vec::new(),
                runs: Vec::with_capacity(1),
                capacity: 1,
                next_seq: 0,
                rejected: 0,
            },
        }
    }

    fn reject<T>(&mut self, e: GrepError) -> Result<T, GrepError> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    /// 关键词逐项校验：非空 → 长度 → ASCII → 已注册 → 族归属。
    fn check_keyword(&mut self, family: Family, kw: &str) -> Result<(), GrepError> {
        let b = kw.as_bytes();
        if b.is_empty() {
            return self.reject(GrepError::EmptyKeyword);
        }
        if b.len() > KEYWORD_CAP {
            return self.reject(GrepError::KeywordLong);
        }
        let mut i = 0usize;
        while i < b.len() {
            match b.get(i) {
                Some(c) if *c < 0x80 => {}
                _ => return self.reject(GrepError::NonAscii),
            }
            i += 1;
        }
        match family_of(kw) {
            None => self.reject(GrepError::UnknownKeyword),
            Some(f) if f != family => self.reject(GrepError::FamilyMismatch),
            Some(_) => Ok(()),
        }
    }

    /// 执行一次查询。返回本次记录；命中行只入全局并集一次。
    pub fn run(&mut self, q: Query<'_>) -> Result<RunRecord, GrepError> {
        if self.lines.is_empty() {
            return self.reject(GrepError::CorpusEmpty);
        }
        if q.keywords.is_empty() || q.keywords.len() > QUERY_KEYWORDS_MAX {
            return self.reject(GrepError::Query);
        }
        let mut i = 0usize;
        while i < q.keywords.len() {
            match q.keywords.get(i) {
                Some(k) => self.check_keyword(q.family, k)?,
                None => return self.reject(GrepError::Query),
            }
            i += 1;
        }
        if self.runs.len() >= self.capacity {
            return self.reject(GrepError::LedgerFull);
        }
        let key = q.identity();
        let mut repeat = false;
        let mut r = 0usize;
        while r < self.runs.len() {
            if let Some(prev) = self.runs.get(r) {
                if prev.key == key {
                    repeat = true;
                }
            }
            r += 1;
        }

        // 真扫：逐行按 Any/All 语义匹配，命中后合并进全局并集与族掩码。
        let mut hits = 0u16;
        let mut added = 0u16;
        let mut li = 0usize;
        while li < self.lines.len() {
            let line = match self.lines.get(li) {
                Some(l) => *l,
                None => break,
            };
            let hay = line.as_bytes();
            let mut matched = 0usize;
            let mut k = 0usize;
            while k < q.keywords.len() {
                if let Some(kw) = q.keywords.get(k) {
                    if contains_ci(hay, kw.as_bytes()) {
                        matched += 1;
                    }
                }
                k += 1;
            }
            let line_hit = match q.mode {
                MatchMode::Any => matched > 0,
                MatchMode::All => matched == q.keywords.len(),
            };
            if line_hit {
                hits = hits.saturating_add(1);
                let was = match self.hit.get(li) {
                    Some(v) => *v,
                    None => false,
                };
                if !was {
                    added = added.saturating_add(1);
                    if let Some(slot) = self.hit.get_mut(li) {
                        *slot = true;
                    }
                }
                if let Some(m) = self.fam_mask.get_mut(li) {
                    *m |= q.family.bit();
                }
            }
            li += 1;
        }
        let distinct = self.distinct();
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        let rec = RunRecord {
            seq,
            family: q.family,
            mode: q.mode,
            key,
            hits,
            added,
            repeat,
            distinct_after: distinct,
        };
        self.runs.push(rec.clone());
        Ok(rec)
    }

    /// 按族标签执行（族标签先解析，未知即 [`GrepError::UnknownFamily`]）。
    pub fn run_tagged(
        &mut self,
        tag: &str,
        keywords: &[&str],
        mode: MatchMode,
    ) -> Result<RunRecord, GrepError> {
        match Family::from_tag(tag) {
            Some(f) => self.run(Query::new(f, keywords, mode)),
            None => self.reject(GrepError::UnknownFamily),
        }
    }

    /// 全局并集大小。
    pub fn distinct(&self) -> u16 {
        let mut n = 0u16;
        let mut i = 0usize;
        while i < self.hit.len() {
            if let Some(v) = self.hit.get(i) {
                if *v {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 某族去重命中行数。
    pub fn family_distinct(&self, f: Family) -> u16 {
        let bit = f.bit();
        let mut n = 0u16;
        let mut i = 0usize;
        while i < self.fam_mask.len() {
            if let Some(m) = self.fam_mask.get(i) {
                if *m & bit != 0 {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 收口闸门：四族齐 + 去重行数达 [`CLOSURE_MIN`]，否则 [`GrepError::Incomplete`]。
    pub fn closure(&self) -> Result<ClosureReport, GrepError> {
        let distinct = self.distinct();
        let corpus = self.lines.len() as u16;
        let report = ClosureReport {
            distinct,
            corpus,
            pending: corpus.saturating_sub(distinct),
            per_family: [
                self.family_distinct(Family::Watermark),
                self.family_distinct(Family::Header),
                self.family_distinct(Family::Footer),
                self.family_distinct(Family::Overlay),
            ],
        };
        if report.closed() {
            Ok(report)
        } else {
            Err(GrepError::Incomplete)
        }
    }

    /// 执行账条数。
    pub fn runs(&self) -> usize {
        self.runs.len()
    }

    /// 取第 `i` 条记录（越界 `None`）。
    pub fn run_at(&self, i: usize) -> Option<&RunRecord> {
        self.runs.get(i)
    }

    /// 语料行数。
    pub fn corpus_len(&self) -> usize {
        self.lines.len()
    }

    /// 累计被拒次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 账指纹：按执行序把身份串与命中数喂进 FNV（换序即换指纹）。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.runs.len() {
            if let Some(r) = self.runs.get(i) {
                let kb = r.key.as_bytes();
                let mut k = 0usize;
                while k < kb.len() {
                    if let Some(b) = kb.get(k) {
                        h = fnv_byte(h, *b);
                    }
                    k += 1;
                }
                h = fnv_byte(h, r.family.index() as u8);
                h = fnv_byte(h, r.mode.code());
                h = fnv_byte(h, (r.hits >> 8) as u8);
                h = fnv_byte(h, r.hits as u8);
                h = fnv_byte(h, (r.added >> 8) as u8);
                h = fnv_byte(h, r.added as u8);
            }
            i += 1;
        }
        h
    }

    /// 落账文本（一次全导，供批日志粘贴）。
    pub fn ledger_text(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.runs.len() {
            if let Some(r) = self.runs.get(i) {
                out.push_str(&r.ledger_line());
                out.push('\n');
            }
            i += 1;
        }
        out
    }
}

/// 标准收口执行：四族各跑一次母关键词。返回账本（已收口）。
pub fn sealed_scan() -> Result<GrepLedger, GrepError> {
    let mut led = GrepLedger::b23();
    led.run(Query::new(Family::Watermark, &["watermark"], MatchMode::Any))?;
    led.run(Query::new(Family::Header, &["header"], MatchMode::Any))?;
    led.run(Query::new(Family::Footer, &["footer"], MatchMode::Any))?;
    led.run(Query::new(Family::Overlay, &["overlay"], MatchMode::Any))?;
    Ok(led)
}

/// 注入核：7 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 7usize;

    // 1) 空关键词：空串既不是「通配」也不是「全部」，必须显式拒。
    let mut a = GrepLedger::b23();
    if a.run(Query::new(Family::Watermark, &[""], MatchMode::Any)).err()
        == Some(GrepError::EmptyKeyword)
    {
        caught += 1;
    }

    // 2) 关键词超长：33 字节。
    let mut b = GrepLedger::b23();
    let long_kw = "abcdefghijklmnopqrstuvwxyz0123456";
    if b.run(Query::new(Family::Watermark, &[long_kw], MatchMode::Any)).err()
        == Some(GrepError::KeywordLong)
    {
        caught += 1;
    }

    // 3) 非 ASCII 关键词：中文词不该进 ASCII 族表。
    let mut c = GrepLedger::b23();
    if c.run(Query::new(Family::Watermark, &["水印"], MatchMode::Any)).err()
        == Some(GrepError::NonAscii)
    {
        caught += 1;
    }

    // 4) 族错标：watermark 是水印族母词，挂在 overlay 族下重跑必须被拒。
    let mut d = GrepLedger::b23();
    if d.run(Query::new(Family::Overlay, &["watermark"], MatchMode::Any)).err()
        == Some(GrepError::FamilyMismatch)
        && d.runs() == 0
    {
        caught += 1;
    }

    // 5) 空语料：账都建不起来。
    if GrepLedger::new(&[]).err() == Some(GrepError::CorpusEmpty) {
        caught += 1;
    }

    // 6) 未知族标签。
    let mut e = GrepLedger::b23();
    if e.run_tagged("banner", &["header"], MatchMode::Any).err() == Some(GrepError::UnknownFamily) {
        caught += 1;
    }

    // 7) 未收口就取报告：只跑了水印一族，四族不齐 → Incomplete。
    let mut f = GrepLedger::b23();
    let ran = f.run(Query::new(Family::Watermark, &["watermark"], MatchMode::Any)).is_ok();
    if ran && f.closure().err() == Some(GrepError::Incomplete) {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50059 自检：16 条判据。
pub fn run_unx50059_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50059");

    // J1-1 真扫：四族母关键词逐族至少命中 1 行（族覆盖不许挂零）。
    // 注意判据只看本族命中数，不看「新增」——header/footer 共用同一批
    // 页眉页脚行，后跑的那族 added 本就是 0，那是**去重生效**而不是没扫到。
    let mut led = GrepLedger::b23();
    let mut fam_ok = true;
    let mut i = 0usize;
    while i < Family::ALL.len() {
        if let Some(f) = Family::ALL.get(i) {
            let kw = match *f {
                Family::Watermark => "watermark",
                Family::Header => "header",
                Family::Footer => "footer",
                Family::Overlay => "overlay",
            };
            let r = led.run(Query::new(*f, &[kw], MatchMode::Any));
            let ok_run = match r {
                Ok(rec) => {
                    let hits_ok = rec.hits > 0 && led.family_distinct(*f) > 0;
                    if *f == Family::Watermark {
                        hits_ok && rec.hits >= 10
                    } else {
                        hits_ok
                    }
                }
                Err(_) => false,
            };
            if !ok_run {
                fam_ok = false;
            }
        }
        i += 1;
    }
    let det = format!(
        "wm={} hd={} ft={} ov={}",
        led.family_distinct(Family::Watermark),
        led.family_distinct(Family::Header),
        led.family_distinct(Family::Footer),
        led.family_distinct(Family::Overlay)
    );
    s.add_dyn("50059-J1-四族真扫", fam_ok, &det);

    // J1-2 首跑 added == hits（并集为空时全部是新增）。
    let mut fresh = GrepLedger::b23();
    let first = fresh.run(Query::new(Family::Watermark, &["watermark"], MatchMode::Any));
    let ok = match &first {
        Ok(r) => r.added == r.hits && !r.repeat && r.distinct_after == r.hits,
        Err(_) => false,
    };
    let det = match &first {
        Ok(r) => format!("hits={} added={}", r.hits, r.added),
        Err(_) => "run failed".to_string(),
    };
    s.add_dyn("50059-J1-首跑全新增", ok, &det);

    // J1-3 重跑留痕：同身份第二次 added==0、repeat==true、并集不变。
    let before = fresh.distinct();
    let again = fresh.run(Query::new(Family::Watermark, &["watermark"], MatchMode::Any));
    let ok = match (&again, before) {
        (Ok(r), b) => r.added == 0 && r.repeat && r.hits > 0 && fresh.distinct() == b,
        _ => false,
    };
    s.add("50059-J1-重跑留痕", ok, if ok { "" } else { "重跑被记成新增" });

    // J1-4 行级去重：并集大小 = 各族并集之并（不重复计数）；
    // 跑完四族后并集仍等于独立算出的逐行「至少被一族命中」行数。
    let seeded = sealed_scan();
    let (union_ok, unique_after) = match &seeded {
        Ok(l) => {
            let mut expect = 0u16;
            let mut li = 0usize;
            while li < CORPUS.len() {
                let line: &[u8] = match CORPUS.get(li) {
                    Some(v) => v.as_bytes(),
                    None => &b""[..],
                };
                let mut hit = false;
                let mut f = 0usize;
                while f < Family::ALL.len() {
                    let kw: &[u8] = match f {
                        0 => &b"watermark"[..],
                        1 => &b"header"[..],
                        2 => &b"footer"[..],
                        _ => &b"overlay"[..],
                    };
                    if contains_ci(line, kw) {
                        hit = true;
                    }
                    f += 1;
                }
                if hit {
                    expect += 1;
                }
                li += 1;
            }
            (l.distinct() == expect && l.runs() == 4, l.distinct())
        }
        Err(_) => (false, 0),
    };
    let det = format!("distinct={}", unique_after);
    s.add_dyn("50059-J1-行级去重", union_ok, &det);

    // J1-5 All ⊆ Any：同族双关键词 all 命中数 ≤ any 命中数，且 any ≥ 单词最大。
    let mut m = GrepLedger::b23();
    let any_rec = m.run(Query::new(Family::Watermark, &["watermark", "wmtext"], MatchMode::Any));
    let all_rec = m.run(Query::new(Family::Watermark, &["watermark", "wmtext"], MatchMode::All));
    let solo = m.run(Query::new(Family::Watermark, &["wmtext"], MatchMode::Any));
    let ok = match (any_rec, all_rec, solo) {
        (Ok(a), Ok(al), Ok(so)) => al.hits <= a.hits && so.hits <= a.hits && al.hits > 0,
        _ => false,
    };
    let det = format!(
        "any={} all={} solo={}",
        any_rec_hits(&m, 0),
        any_rec_hits(&m, 1),
        any_rec_hits(&m, 2)
    );
    s.add_dyn("50059-J1-模式包含", ok, &det);

    // J1-6 大小写不敏感：WATERMARK 与 watermark 命中同数。
    let mut u = GrepLedger::b23();
    let lower = u.run(Query::new(Family::Watermark, &["watermark"], MatchMode::Any));
    let upper = u.run(Query::new(Family::Watermark, &["WATERMARK"], MatchMode::Any));
    let ok = match (lower, upper) {
        (Ok(l), Ok(h)) => l.hits == h.hits && h.hits > 0 && h.added == 0,
        _ => false,
    };
    s.add("50059-J1-大小写不敏感", ok, if ok { "" } else { "大小写改变命中" });

    // J1-7 族错标必拒且账不变：watermark 挂 footer 族。
    let mut mm = GrepLedger::b23();
    let _ = mm.run(Query::new(Family::Watermark, &["watermark"], MatchMode::Any));
    let runs_before = mm.runs();
    let bad = mm.run(Query::new(Family::Footer, &["watermark"], MatchMode::Any));
    let ok = bad.err() == Some(GrepError::FamilyMismatch)
        && mm.runs() == runs_before
        && mm.rejected() >= 1;
    s.add("50059-J1-族错标必拒", ok, if ok { "" } else { "错族关键词进了账" });

    // J1-8 族标签解析：四族 + 简称可解析，未知标签 None 且走 UnknownFamily。
    let mut t = GrepLedger::b23();
    let tag_ok = Family::from_tag("header") == Some(Family::Header)
        && Family::from_tag("HDR") == Some(Family::Header)
        && Family::from_tag("footer") == Some(Family::Footer)
        && Family::from_tag("Overlay") == Some(Family::Overlay)
        && Family::from_tag("wm") == Some(Family::Watermark)
        && Family::from_tag("banner").is_none();
    let unknown = t.run_tagged("banner", &["header"], MatchMode::Any);
    let ok = tag_ok && unknown.err() == Some(GrepError::UnknownFamily) && t.runs() == 0;
    s.add("50059-J1-族标签闭集", ok, if ok { "" } else { "族标签解析越界" });

    // J1-9 未注册关键词必拒（关键词族是闭集，不许现编）。
    let mut uk = GrepLedger::b23();
    let ok = uk.run(Query::new(Family::Watermark, &["watermak"], MatchMode::Any)).err()
        == Some(GrepError::UnknownKeyword)
        && uk.runs() == 0;
    s.add("50059-J1-关键词闭集", ok, if ok { "" } else { "未注册关键词被接受" });

    // J1-10 查询形状必拒：0 关键词 / 3 关键词都算非法查询。
    let mut qs = GrepLedger::b23();
    let none = qs.run(Query::new(Family::Watermark, &[], MatchMode::Any));
    let three = qs.run(Query::new(
        Family::Watermark,
        &["watermark", "wmtext", "watermark"],
        MatchMode::Any,
    ));
    let ok = none.err() == Some(GrepError::Query)
        && three.err() == Some(GrepError::Query)
        && qs.runs() == 0;
    s.add("50059-J1-查询形状", ok, if ok { "" } else { "非法查询形状被接受" });

    // J1-11 收口闸门：只跑水印一族 → Incomplete；四族齐 → Ok 且 pending 对得上。
    let partial = {
        let mut p = GrepLedger::b23();
        let _ = p.run(Query::new(Family::Watermark, &["watermark"], MatchMode::Any));
        p.closure()
    };
    let full = match &seeded {
        Ok(l) => l.closure(),
        Err(_) => Err(GrepError::Incomplete),
    };
    let ok = partial.err() == Some(GrepError::Incomplete)
        && match full {
            Ok(rep) => {
                rep.closed()
                    && rep.all_families()
                    && rep.distinct >= CLOSURE_MIN
                    && rep.pending == rep.corpus - rep.distinct
                    && rep.corpus == CORPUS.len() as u16
            }
            Err(_) => false,
        };
    let det = match full {
        Ok(rep) => format!("d={} p={} f={:?}", rep.distinct, rep.pending, rep.per_family),
        Err(e) => format!("closure err {:?}", e),
    };
    s.add_dyn("50059-J1-收口闸门", ok, &det);

    // J1-12 收口幂等：收口后再全量重跑四族，并集与报告逐项不变。
    let mut re = match sealed_scan() {
        Ok(l) => l,
        Err(_) => GrepLedger::b23(),
    };
    let d1 = re.distinct();
    let _ = re.run(Query::new(Family::Watermark, &["watermark"], MatchMode::Any));
    let _ = re.run(Query::new(Family::Header, &["header"], MatchMode::Any));
    let _ = re.run(Query::new(Family::Footer, &["footer"], MatchMode::Any));
    let _ = re.run(Query::new(Family::Overlay, &["overlay"], MatchMode::Any));
    let d2 = re.distinct();
    let tail_repeat = match re.run_at(7) {
        Some(r) => r.repeat && r.added == 0,
        None => false,
    };
    let ok = d1 == d2 && tail_repeat && re.runs() == 8;
    let det = format!("d1={} d2={} runs={}", d1, d2, re.runs());
    s.add_dyn("50059-J1-收口幂等", ok, &det);

    // J1-13 定容不破：容量 1 的账第二次执行 LedgerFull，并集不因拒绝而变。
    let mut tiny = GrepLedger::b23();
    tiny.capacity = 1;
    let one = tiny.run(Query::new(Family::Watermark, &["watermark"], MatchMode::Any));
    let d_after_one = tiny.distinct();
    let two = tiny.run(Query::new(Family::Header, &["header"], MatchMode::Any));
    let ok = one.is_ok()
        && two.err() == Some(GrepError::LedgerFull)
        && tiny.runs() == 1
        && tiny.distinct() == d_after_one;
    s.add("50059-J1-定容不破", ok, if ok { "" } else { "容量满仍入账" });

    // J1-14 指纹：同序两次扫描同指纹，多跑一条即变；落账文本行数 = 执行数。
    let mut s1 = match sealed_scan() {
        Ok(l) => l,
        Err(_) => GrepLedger::b23(),
    };
    let f1 = s1.digest();
    let text_lines = s1.ledger_text().lines().count();
    let _ = s1.run(Query::new(Family::Watermark, &["watermark"], MatchMode::Any));
    let f2 = s1.digest();
    let s2 = match sealed_scan() {
        Ok(l) => l,
        Err(_) => GrepLedger::b23(),
    };
    let ok = f1 != FNV_OFFSET && f1 == s2.digest() && f1 != f2 && text_lines == 4;
    let det = format!("f1={:#010x} f2={:#010x} lines={}", f1, f2, text_lines);
    s.add_dyn("50059-J1-指纹留痕", ok, &det);

    // J1-15 七类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50059-J1-注入必抓", ok, &det);

    s
}

fn any_rec_hits(l: &GrepLedger, i: usize) -> u16 {
    match l.run_at(i) {
        Some(r) => r.hits,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50059_checks_green() {
        let set = run_unx50059_checks();
        assert!(set.all_passed(), "UNX-F50059 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50059_closure_reaches_all_families() {
        let led = sealed_scan();
        assert!(led.is_ok());
        if let Ok(l) = led {
            let rep = l.closure();
            assert!(rep.is_ok());
            if let Ok(r) = rep {
                assert!(r.all_families());
                assert_eq!(r.corpus as usize, CORPUS.len());
            }
        }
    }

    #[test]
    fn unx50059_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 7);
    }
}
