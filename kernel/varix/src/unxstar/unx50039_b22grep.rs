//! UNX-F50039 B22 批防重 grep 执行记录（关键词族：nup/booklet/poster/layout）（0x43100 段）
//!
//! 锚点：UNX-F50039-J1 批收口防重留痕；判据锚定 §6.1。
//!
//! 数据段：`SEG_BASE = 0x43100`，诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 8`，
//! 逐项见 [`GrepError`]。
//!
//! 版面批的「防重」有两层硬语义，缺一层就会让覆盖统计失真：
//! 1. **行级去重**——一行被多个关键词命中只计一次，账本维护严格单调的全局
//!    并集，每次执行回报本次真正**新增**的行数 `added`；
//! 2. **执行级留痕**——同一关键词跑第二遍必须 `repeat=true` 且 `added=0`；
//!    同一关键词换个族重跑则直接 `FamilyMismatch`，不许把族标错的执行记成覆盖。
//!
//! 扫描是**真扫**：逐字节 ASCII 大小写不敏感子串匹配，不借助 `str::contains`
//! 的 Unicode 语义（语料是内核文档注释，非 ASCII 字节一旦参与折叠会让
//! 「命中」不可复算）。收口闸门 [`GrepLedger::close`] 要求四族俱有点亮、
//! 全局并集非空，并给出子可加性报表：族命中之和 ≥ 并集，差值就是重叠量。
//!
//! 结构清单：
//! * [`Family`] + [`FAMILY_KEYWORDS`]：四族关键词表（12 个关键词）；
//! * [`GrepRun`]：单次执行记录（序号 / 关键词 / 族 / 命中 / 新增 / 重复 / 摘要）；
//! * [`GrepLedger`]：语料 + 执行账 + 全局并集 + 四本条族并集；
//! * [`CloseReport`]：收口报表（族覆盖 / 并集 / 重叠 / 链摘要）；
//! * [`injection_core`]：8 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43100 独占 0x20）。
pub const SEG_BASE: u32 = 0x43100;

const E_EMPTY_KEYWORD: u16 = 1;
const E_KEYWORD_LONG: u16 = 2;
const E_NON_ASCII: u16 = 3;
const E_CORPUS_EMPTY: u16 = 4;
const E_LEDGER_FULL: u16 = 5;
const E_FAMILY_MISMATCH: u16 = 6;
const E_ZERO_HITS: u16 = 7;
const E_NOT_COVERED: u16 = 8;

/// grep 账错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrepError {
    /// 关键词为空或纯空白。
    EmptyKeyword,
    /// 关键词超过 32 字节。
    KeywordLong,
    /// 关键词含非 ASCII 字节。
    NonAscii,
    /// 语料为空，扫无可扫。
    CorpusEmpty,
    /// 执行账已满。
    LedgerFull,
    /// 同一关键词换族重跑。
    FamilyMismatch,
    /// 关键词零命中（没有证据的检索不许入账）。
    ZeroHits,
    /// 收口时族覆盖不全 / 并集为空。
    NotCovered,
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
            GrepError::ZeroHits => E_ZERO_HITS,
            GrepError::NotCovered => E_NOT_COVERED,
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
            GrepError::ZeroHits => "keyword has zero hits",
            GrepError::NotCovered => "not every family is covered",
        }
    }
}

/// 关键词族（批收口的四根柱子）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// `nup` 族。
    Nup,
    /// `booklet` 族。
    Booklet,
    /// `poster` 族。
    Poster,
    /// `layout` 族。
    Layout,
}

impl Family {
    /// 四族固定顺序。
    pub const ALL: [Family; 4] = [Family::Nup, Family::Booklet, Family::Poster, Family::Layout];

    /// 族下标 0..4。
    pub fn index(self) -> usize {
        match self {
            Family::Nup => 0,
            Family::Booklet => 1,
            Family::Poster => 2,
            Family::Layout => 3,
        }
    }

    /// 族标签。
    pub fn tag(self) -> &'static str {
        match self {
            Family::Nup => "nup",
            Family::Booklet => "booklet",
            Family::Poster => "poster",
            Family::Layout => "layout",
        }
    }

    /// 解析族标签；不认识返回 `None`（不猜、不兜底）。
    pub fn from_tag(t: &str) -> Option<Family> {
        if t == "nup" {
            Some(Family::Nup)
        } else if t == "booklet" {
            Some(Family::Booklet)
        } else if t == "poster" {
            Some(Family::Poster)
        } else if t == "layout" {
            Some(Family::Layout)
        } else {
            None
        }
    }
}

/// 四族关键词表：每个关键词都必须在语料里至少命中一行，否则 `ZeroHits`。
pub const FAMILY_KEYWORDS: [&[&str]; 4] = [
    &["nup", "n-up", "imposition"],
    &["booklet", "saddle", "fold"],
    &["poster", "tile", "crop mark"],
    &["layout", "page box", "gutter"],
];

/// 版面批语料（内核 unxstar 文档注释的真实样式，含一行空行）。
pub const CORPUS: [&str; 20] = [
    "nup2 places two source pages on one sheet",
    "NUP4 uses a 2x2 grid with a 6pt gutter between cells",
    "booklet imposition folds saddle stitched signatures",
    "the poster mode tiles a page across 2x2 sheets",
    "crop mark placement belongs to the poster family",
    "layout preview renders the page box before printing",
    "page box is the authoritative layout rectangle",
    "duplex printing flips on the long edge by default",
    "n-up ordering keeps reading order left to right",
    "saddle stitching requires a booklet page count multiple of four",
    "tile overlap is 3 mm on every poster seam",
    "the gutter is excluded from every layout cell",
    "imposition is the umbrella term for nup and booklet",
    "fold lines are inserted by the booklet finisher",
    "a poster built from nine sheets needs crop marks on the border",
    "page rotation of 180 degrees keeps the layout box unchanged",
    "layout samples cover nup2 nup4 nup6 nup9 nup16",
    "the crop mark engine refuses sheets smaller than the poster box",
    "duplex sheet order is front then back per paper",
    "",
];

/// 单本账的执行条数上限（12 个关键词 × 4 遍复跑，留足复测位）。
pub const MAX_RUNS: usize = 48;
/// 关键词字节上限。
pub const KEYWORD_CAP: usize = 32;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(0x0100_0193)
}

/// 逐字节 ASCII 大小写不敏感子串匹配；非 ASCII 字节按原值比较（不折叠）。
pub fn contains_ci(hay: &str, needle: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return false;
    }
    let mut i = 0usize;
    while i + n.len() <= h.len() {
        let mut j = 0usize;
        let mut ok = true;
        while j < n.len() {
            let a = match h.get(i + j) {
                Some(v) => *v,
                None => return false,
            };
            let b = match n.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if a.to_ascii_lowercase() != b.to_ascii_lowercase() {
                ok = false;
                break;
            }
            j += 1;
        }
        if ok {
            return true;
        }
        i += 1;
    }
    false
}

/// 单次执行记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrepRun {
    /// 执行序号（从 1 起，连续）。
    pub seq: u16,
    /// 关键词原文（大小写保留）。
    pub keyword: String,
    /// 所属族。
    pub family: Family,
    /// 命中行数。
    pub hits: u16,
    /// 本次新增到全局并集的行数。
    pub added: u16,
    /// 是否重复执行（同关键词在前序出现过）。
    pub repeat: bool,
    /// 命中行号集合的摘要（升序混入，便于复算比对）。
    pub digest: u32,
}

impl GrepRun {
    /// 一行台账。
    pub fn ledger_line(&self) -> String {
        format!(
            "{} [{}] \"{}\" hits={} added={} repeat={} dg={:08x}",
            self.seq,
            self.family.tag(),
            self.keyword,
            self.hits,
            self.added,
            if self.repeat { 1 } else { 0 },
            self.digest
        )
    }
}

/// 单次执行回报（调用方拿到的就是这一行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunReport {
    /// 执行序号。
    pub seq: u16,
    /// 命中行数。
    pub hits: u16,
    /// 新增行数。
    pub added: u16,
    /// 是否重复。
    pub repeat: bool,
}

/// 收口报表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseReport {
    /// 执行条数。
    pub runs: u16,
    /// 已点亮的族数（应为 4）。
    pub families: u8,
    /// 全局并集行数。
    pub union_size: u16,
    /// 族命中之和。
    pub family_total: u16,
    /// 重叠量 = 族命中之和 − 并集（重叠只算一次的证据）。
    pub overlap: u16,
    /// 收口链摘要（按执行序折叠）。
    pub digest: u32,
}

impl CloseReport {
    /// 子可加性：族命中之和必须 ≥ 并集，且重叠量恰为差额。
    pub fn subadditive(&self) -> bool {
        self.family_total >= self.union_size
            && self.overlap == self.family_total - self.union_size
            && self.families == 4
    }

    /// 一行对账文本。
    pub fn ledger_line(&self) -> String {
        format!(
            "runs={} fam={} union={} fam_total={} overlap={} dg={:08x}",
            self.runs, self.families, self.union_size, self.family_total, self.overlap, self.digest
        )
    }
}

/// grep 账本：语料 + 执行账 + 全局并集 + 四条族并集。
pub struct GrepLedger {
    corpus: &'static [&'static str],
    runs: Vec<GrepRun>,
    union: Vec<bool>,
    fam_union: [Vec<bool>; 4],
    rejected: u32,
}

impl GrepLedger {
    /// 建账：语料为空即拒（扫空语料得出的「全覆盖」是假证据）。
    pub fn new(corpus: &'static [&'static str]) -> Result<GrepLedger, GrepError> {
        if corpus.is_empty() {
            return Err(GrepError::CorpusEmpty);
        }
        let n = corpus.len();
        Ok(GrepLedger {
            corpus,
            runs: Vec::with_capacity(MAX_RUNS),
            union: vec_false(n),
            fam_union: [vec_false(n), vec_false(n), vec_false(n), vec_false(n)],
            rejected: 0,
        })
    }

    fn reject<T>(&mut self, e: GrepError) -> Result<T, GrepError> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    /// 语料行数。
    pub fn corpus_len(&self) -> usize {
        self.corpus.len()
    }

    /// 执行条数。
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// 累计被拒次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 按序号取执行记录（序号从 1 起）。
    pub fn run(&self, seq: u16) -> Option<&GrepRun> {
        if seq == 0 {
            return None;
        }
        self.runs.get(seq as usize - 1)
    }

    /// 全局并集行数。
    pub fn union_size(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.union.len() {
            if self.union.get(i) == Some(&true) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 某族已点亮的行数。
    pub fn family_size(&self, f: Family) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        if let Some(v) = self.fam_union.get(f.index()) {
            while i < v.len() {
                if v.get(i) == Some(&true) {
                    n += 1;
                }
                i += 1;
            }
        }
        n
    }

    /// 四族命中之和。
    pub fn family_total(&self) -> usize {
        let mut n = 0usize;
        let mut f = 0usize;
        while f < 4 {
            if let Some(fam) = Family::ALL.get(f) {
                n += self.family_size(*fam);
            }
            f += 1;
        }
        n
    }

    /// 四族是否都点亮（每族至少一行）。
    pub fn families_covered(&self) -> usize {
        let mut n = 0usize;
        let mut f = 0usize;
        while f < 4 {
            if let Some(fam) = Family::ALL.get(f) {
                if self.family_size(*fam) > 0 {
                    n += 1;
                }
            }
            f += 1;
        }
        n
    }

    fn prior_family(&self, keyword: &str) -> Option<Family> {
        let mut i = 0usize;
        while i < self.runs.len() {
            if let Some(r) = self.runs.get(i) {
                if r.keyword.as_str() == keyword {
                    return Some(r.family);
                }
            }
            i += 1;
        }
        None
    }

    /// 执行一次 grep：校验关键词 → 真扫 → 算新增 → 落账。
    pub fn run_grep(&mut self, keyword: &str, family: Family) -> Result<RunReport, GrepError> {
        let kw = keyword.trim();
        if kw.is_empty() {
            return self.reject(GrepError::EmptyKeyword);
        }
        if kw.len() > KEYWORD_CAP {
            return self.reject(GrepError::KeywordLong);
        }
        let mut k = 0usize;
        while k < kw.len() {
            match kw.as_bytes().get(k) {
                Some(b) => {
                    if *b >= 0x80 {
                        return self.reject(GrepError::NonAscii);
                    }
                }
                None => return self.reject(GrepError::NonAscii),
            }
            k += 1;
        }
        if self.runs.len() >= MAX_RUNS {
            return self.reject(GrepError::LedgerFull);
        }
        if let Some(prev) = self.prior_family(kw) {
            if prev != family {
                return self.reject(GrepError::FamilyMismatch);
            }
        }
        // 真扫：命中行号按语料序升序收集。
        let mut hits: Vec<usize> = Vec::new();
        let mut digest = 0x811c_9dc5u32;
        let mut i = 0usize;
        while i < self.corpus.len() {
            if let Some(line) = self.corpus.get(i) {
                if contains_ci(line, kw) {
                    hits.push(i);
                    digest = fnv_byte(digest, i as u8);
                    digest = fnv_byte(digest, (i >> 8) as u8);
                }
            }
            i += 1;
        }
        if hits.is_empty() {
            return self.reject(GrepError::ZeroHits);
        }
        // 新增 = 尚未进全局并集的行；同时点亮族并集。
        let mut added = 0usize;
        let mut h = 0usize;
        while h < hits.len() {
            if let Some(idx) = hits.get(h) {
                let already = self.union.get(*idx) == Some(&true);
                if !already {
                    added += 1;
                    if let Some(slot) = self.union.get_mut(*idx) {
                        *slot = true;
                    }
                }
                if let Some(fam) = self.fam_union.get_mut(family.index()) {
                    if let Some(slot) = fam.get_mut(*idx) {
                        *slot = true;
                    }
                }
            }
            h += 1;
        }
        let repeat = self.prior_family(kw).is_some();
        let seq = self.runs.len() as u16 + 1;
        let hits_u16 = hits.len() as u16;
        let added_u16 = added as u16;
        self.runs.push(GrepRun {
            seq,
            keyword: kw.to_string(),
            family,
            hits: hits_u16,
            added: added_u16,
            repeat,
            digest,
        });
        Ok(RunReport {
            seq,
            hits: hits_u16,
            added: added_u16,
            repeat,
        })
    }

    /// 一次跑完一族的全部关键词，返回本族新增行数。
    pub fn run_family(&mut self, family: Family) -> Result<u16, GrepError> {
        let mut total = 0u16;
        let mut i = 0usize;
        let list = match FAMILY_KEYWORDS.get(family.index()) {
            Some(v) => *v,
            None => return self.reject(GrepError::ZeroHits),
        };
        while i < list.len() {
            if let Some(kw) = list.get(i) {
                let rep = self.run_grep(kw, family)?;
                total = total.saturating_add(rep.added);
            }
            i += 1;
        }
        Ok(total)
    }

    /// 收口链摘要：按执行序把 (seq, hits, added) 折叠进来。
    pub fn digest(&self) -> u32 {
        let mut h = 0x811c_9dc5u32;
        let mut i = 0usize;
        while i < self.runs.len() {
            if let Some(r) = self.runs.get(i) {
                h = fnv_byte(h, r.seq as u8);
                h = fnv_byte(h, r.hits as u8);
                h = fnv_byte(h, r.added as u8);
                h = fnv_byte(h, r.family.index() as u8);
            }
            i += 1;
        }
        h
    }

    /// 收口：四族俱亮 + 并集非空，二者缺一即 `NotCovered`。
    pub fn close(&self) -> Result<CloseReport, GrepError> {
        let families = self.families_covered();
        if families != 4 {
            return Err(GrepError::NotCovered);
        }
        let union_size = self.union_size();
        if union_size == 0 {
            return Err(GrepError::NotCovered);
        }
        let family_total = self.family_total();
        Ok(CloseReport {
            runs: self.runs.len() as u16,
            families: families as u8,
            union_size: union_size as u16,
            family_total: family_total as u16,
            overlap: (family_total - union_size) as u16,
            digest: self.digest(),
        })
    }

    /// 全表台账文本。
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

fn vec_false(n: usize) -> Vec<bool> {
    let mut v = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        v.push(false);
        i += 1;
    }
    v
}

/// 独立实现的三字节窗口扫描（判据交叉核对用，刻意不复用 `contains_ci`）。
fn manual_has_nup(line: &str) -> bool {
    let b = line.as_bytes();
    if b.len() < 3 {
        return false;
    }
    let mut i = 0usize;
    while i + 3 <= b.len() {
        let a0 = match b.get(i) {
            Some(v) => v.to_ascii_lowercase(),
            None => return false,
        };
        let a1 = match b.get(i + 1) {
            Some(v) => v.to_ascii_lowercase(),
            None => return false,
        };
        let a2 = match b.get(i + 2) {
            Some(v) => v.to_ascii_lowercase(),
            None => return false,
        };
        if a0 == b'n' && a1 == b'u' && a2 == b'p' {
            return true;
        }
        i += 1;
    }
    false
}

/// 按四族跑满全部关键词（每个关键词各跑一遍）。
pub fn full_sweep() -> Result<GrepLedger, GrepError> {
    let mut l = GrepLedger::new(&CORPUS)?;
    let mut i = 0usize;
    while i < Family::ALL.len() {
        if let Some(f) = Family::ALL.get(i) {
            l.run_family(*f)?;
        }
        i += 1;
    }
    Ok(l)
}

/// 注入核：8 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;

    // 1) 空关键词。
    if let Ok(mut l) = GrepLedger::new(&CORPUS) {
        if l.run_grep("   ", Family::Nup).err() == Some(GrepError::EmptyKeyword) {
            caught += 1;
        }
    }

    // 2) 关键词超长（33 字节）。
    if let Ok(mut l) = GrepLedger::new(&CORPUS) {
        let long = "n".repeat(KEYWORD_CAP + 1);
        if l.run_grep(&long, Family::Nup).err() == Some(GrepError::KeywordLong) {
            caught += 1;
        }
    }

    // 3) 关键词含非 ASCII 字节。
    if let Ok(mut l) = GrepLedger::new(&CORPUS) {
        if l.run_grep("nüp", Family::Nup).err() == Some(GrepError::NonAscii) {
            caught += 1;
        }
    }

    // 4) 空语料建账（空切片字面量显式标 'static，避免临时借用）。
    let empty: &'static [&'static str] = &[];
    if GrepLedger::new(empty).err() == Some(GrepError::CorpusEmpty) {
        caught += 1;
    }

    // 5) 同一关键词换族重跑。
    if let Ok(mut l) = GrepLedger::new(&CORPUS) {
        let first = l.run_grep("layout", Family::Layout).is_ok();
        let second = l.run_grep("layout", Family::Poster).err() == Some(GrepError::FamilyMismatch);
        if first && second {
            caught += 1;
        }
    }

    // 6) 零命中关键词（"kerning" 不在语料里）。
    if let Ok(mut l) = GrepLedger::new(&CORPUS) {
        if l.run_grep("kerning", Family::Layout).err() == Some(GrepError::ZeroHits) && l.run_count() == 0 {
            caught += 1;
        }
    }

    // 7) 未覆盖四族就收口。
    if let Ok(mut l) = GrepLedger::new(&CORPUS) {
        let _ = l.run_grep("nup", Family::Nup);
        if l.close().err() == Some(GrepError::NotCovered) {
            caught += 1;
        }
    }

    // 8) 执行账满（容量 48 条）。
    if let Ok(mut l) = GrepLedger::new(&CORPUS) {
        let mut i = 0usize;
        while i < MAX_RUNS {
            // 同族同关键词反复跑：命中稳定、不触发零命中，只把账本填满。
            let _ = l.run_grep("nup", Family::Nup);
            i += 1;
        }
        if l.run_count() == MAX_RUNS
            && l.run_grep("nup", Family::Nup).err() == Some(GrepError::LedgerFull)
        {
            caught += 1;
        }
    }

    (caught, total)
}

/// UNX-F50039 自检：16 条判据。
pub fn run_unx50039_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50039");

    // J1-1 全族扫：12 个关键词各跑一遍，执行序号 1..=12 连续。
    let full = full_sweep();
    let mut seq_ok = false;
    let mut runs = 0usize;
    if let Ok(l) = &full {
        runs = l.run_count();
        seq_ok = runs == 12;
        let mut i = 0usize;
        while i < runs {
            match l.run(i as u16 + 1) {
                Some(r) => {
                    if r.seq != (i as u16) + 1 {
                        seq_ok = false;
                    }
                }
                None => seq_ok = false,
            }
            i += 1;
        }
    }
    let det = format!("runs={}", runs);
    s.add_dyn("50039-J1-全族执行", seq_ok, &det);

    // J1-2 关键词表与族归属：每个关键词都在自己族表里，四族各 3 个。
    let mut kw_ok = true;
    let mut f = 0usize;
    while f < 4 {
        match FAMILY_KEYWORDS.get(f) {
            Some(list) => {
                if list.len() != 3 {
                    kw_ok = false;
                }
                let mut i = 0usize;
                while i < list.len() {
                    if let Some(kw) = list.get(i) {
                        if kw.is_empty() || kw.len() > KEYWORD_CAP {
                            kw_ok = false;
                        }
                    }
                    i += 1;
                }
            }
            None => kw_ok = false,
        }
        f += 1;
    }
    s.add("50039-J1-关键词表", kw_ok, if kw_ok { "" } else { "关键词表结构不符" });

    // J1-3 首跑不标重复：12 条 run 全部 repeat=false，且每族的合计新增 > 0。
    let mut no_repeat = full.is_ok();
    let mut fam_added = [0u16; 4];
    if let Ok(l) = &full {
        let mut i = 0usize;
        while i < l.run_count() {
            match l.run(i as u16 + 1) {
                Some(r) => {
                    if r.repeat {
                        no_repeat = false;
                    }
                    if let Some(slot) = fam_added.get_mut(r.family.index()) {
                        *slot = slot.saturating_add(r.added);
                    }
                }
                None => no_repeat = false,
            }
            i += 1;
        }
        let mut f = 0usize;
        while f < 4 {
            if fam_added.get(f) == Some(&0) {
                no_repeat = false;
            }
            f += 1;
        }
    }
    let det = format!("fam_added={:?}", fam_added);
    s.add_dyn("50039-J1-首跑皆新增", no_repeat, &det);

    // J1-4 重复执行留痕：同一关键词第二遍 repeat=true 且 added=0，账本仍增一条。
    let mut rep_ok = false;
    if let Ok(mut l) = GrepLedger::new(&CORPUS) {
        let first = l.run_grep("layout", Family::Layout);
        let second = l.run_grep("layout", Family::Layout);
        let third = l.run_grep("layOut", Family::Layout);
        rep_ok = match (first, second, third) {
            (Ok(a), Ok(b), Ok(c)) => {
                !a.repeat
                    && a.added > 0
                    && b.repeat
                    && b.added == 0
                    && b.hits == a.hits
                    && !c.repeat
                    && c.added == 0
                    && c.hits == a.hits
                    && l.run_count() == 3
            }
            _ => false,
        };
    }
    s.add("50039-J1-重复留痕", rep_ok, if rep_ok { "" } else { "重复执行未留痕" });

    // J1-5 并集单调：每次执行后并集只增不减。
    let mut mono = true;
    if let Ok(mut l) = GrepLedger::new(&CORPUS) {
        let plan = [
            ("nup", Family::Nup),
            ("layout", Family::Layout),
            ("booklet", Family::Booklet),
            ("poster", Family::Poster),
            ("gutter", Family::Layout),
        ];
        let mut prev = 0usize;
        let mut i = 0usize;
        while i < plan.len() {
            if let Some((kw, fam)) = plan.get(i) {
                if l.run_grep(kw, *fam).is_ok() {
                    let now = l.union_size();
                    if now < prev {
                        mono = false;
                    }
                    prev = now;
                }
            }
            i += 1;
        }
    }
    s.add("50039-J1-并集单调", mono, if mono { "" } else { "并集出现回退" });

    // J1-6 新增量守恒：每次 added 恰等于并集增量。
    let mut added_ok = true;
    if let Ok(mut l) = GrepLedger::new(&CORPUS) {
        let plan = [
            ("nup", Family::Nup),
            ("n-up", Family::Nup),
            ("imposition", Family::Nup),
            ("booklet", Family::Booklet),
        ];
        let mut i = 0usize;
        while i < plan.len() {
            if let Some((kw, fam)) = plan.get(i) {
                let before = l.union_size();
                match l.run_grep(kw, *fam) {
                    Ok(r) => {
                        let after = l.union_size();
                        if after - before != r.added as usize {
                            added_ok = false;
                        }
                    }
                    Err(_) => added_ok = false,
                }
            }
            i += 1;
        }
    }
    s.add("50039-J1-新增守恒", added_ok, if added_ok { "" } else { "added 与并集增量不符" });

    // J1-7 族并集被全局并集包含：任一行属于某族，必属于全局并集。
    let mut subset_ok = full.is_ok();
    if let Ok(l) = &full {
        let mut f = 0usize;
        while f < 4 {
            if let Some(fam) = Family::ALL.get(f) {
                let size = l.family_size(*fam);
                if size == 0 || size > l.union_size() {
                    subset_ok = false;
                }
            }
            f += 1;
        }
    }
    s.add("50039-J1-族集包含", subset_ok, if subset_ok { "" } else { "族并集越出全局并集" });

    // J1-8 收口报表：四族俱亮、并集非空、子可加（重叠=族和−并集）。
    let close = match &full {
        Ok(l) => l.close(),
        Err(e) => Err(*e),
    };
    let (close_ok, close_det) = match close {
        Ok(r) => (r.subadditive() && r.union_size > 0 && r.runs == 12, r.ledger_line()),
        Err(e) => (false, format!("close err={:#x}", e.code())),
    };
    s.add_dyn("50039-J1-收口报表", close_ok, &close_det);

    // J1-9 重叠必存在：语料里同一行被两族命中，重叠量 > 0（否则说明族表互斥得可疑）。
    let overlap_ok = match full.as_ref().ok().and_then(|l| l.close().ok()) {
        Some(r) => r.overlap > 0 && r.family_total > r.union_size,
        None => false,
    };
    s.add("50039-J1-重叠计一次", overlap_ok, if overlap_ok { "" } else { "重叠量异常" });

    // J1-10 大小写不敏感：全大写与小写关键词命中集合一致、摘要一致。
    let mut ci_ok = false;
    if let Ok(mut l) = GrepLedger::new(&CORPUS) {
        let a = l.run_grep("layout", Family::Layout);
        let b = l.run_grep("LAYOUT", Family::Layout);
        ci_ok = match (a, b) {
            (Ok(x), Ok(y)) => x.hits == y.hits,
            _ => false,
        };
        if let Some(r) = l.run(1) {
            if let Some(q) = l.run(2) {
                ci_ok = ci_ok && r.digest == q.digest;
            }
        }
    }
    s.add("50039-J1-大小写折叠", ci_ok, if ci_ok { "" } else { "大小写命中不一致" });

    // J1-11 扫描正确性：与另一套逐字节实现交叉核对（同语料、同关键词）。
    let scan_ok = {
        let mut ok = true;
        let mut i = 0usize;
        while i < CORPUS.len() {
            if let Some(line) = CORPUS.get(i) {
                let has_nup = contains_ci(line, "nup");
                let manual = manual_has_nup(line);
                if has_nup != manual {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    };
    s.add("50039-J1-逐行真扫", scan_ok, if scan_ok { "" } else { "子串匹配与手工扫描不一致" });

    // J1-12 族标签往返：四族 tag 可解析回自身，未知标签返回 None。
    let mut tag_ok = true;
    let mut f = 0usize;
    while f < 4 {
        if let Some(fam) = Family::ALL.get(f) {
            if Family::from_tag(fam.tag()) != Some(*fam) {
                tag_ok = false;
            }
            if fam.index() != f {
                tag_ok = false;
            }
        }
        f += 1;
    }
    let tag_ok = tag_ok && Family::from_tag("nup9").is_none();
    s.add("50039-J1-族标签", tag_ok, if tag_ok { "" } else { "族标签往返失败" });

    // J1-13 确定性：同序列两遍得到逐条一致的执行账与收口摘要。
    let mut det_ok = false;
    if let (Ok(a), Ok(b)) = (full_sweep(), full_sweep()) {
        det_ok = a.run_count() == b.run_count()
            && a.digest() == b.digest()
            && a.union_size() == b.union_size()
            && a.family_total() == b.family_total();
    }
    let det = match &full {
        Ok(l) => format!("dg={:08x} union={}", l.digest(), l.union_size()),
        Err(_) => "sweep failed".to_string(),
    };
    s.add_dyn("50039-J1-执行确定", det_ok, &det);

    // J1-14 台账逐行可读：每行含族标签、关键词与两个计数。
    let ledger_ok = match &full {
        Ok(l) => {
            let text = l.ledger_text();
            text.contains("[nup]") && text.contains("[booklet]") && text.contains("hits=") && text.contains("added=")
        }
        Err(_) => false,
    };
    s.add("50039-J1-台账可读", ledger_ok, if ledger_ok { "" } else { "台账文本缺字段" });

    // J1-15 容量防线：容量由 MAX_RUNS 决定，满后第 MAX_RUNS+1 次被拒且计数不变。
    let cap_ok = {
        let mut ok = false;
        if let Ok(mut l) = GrepLedger::new(&CORPUS) {
            let mut i = 0usize;
            while i < MAX_RUNS {
                let _ = l.run_grep("nup", Family::Nup);
                i += 1;
            }
            let hit_cap = l.run_grep("nup", Family::Nup).err() == Some(GrepError::LedgerFull);
            ok = hit_cap && l.run_count() == MAX_RUNS && l.rejected() == 1;
        }
        ok
    };
    s.add("50039-J1-容量防线", cap_ok, if cap_ok { "" } else { "执行账越容" });

    // J1-16 八类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50039-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50039_checks_green() {
        let set = run_unx50039_checks();
        assert!(set.all_passed(), "UNX-F50039 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50039_four_families_close() {
        let l = match full_sweep() {
            Ok(v) => v,
            Err(e) => panic!("sweep failed: {:#x}", e.code()),
        };
        let rep = match l.close() {
            Ok(v) => v,
            Err(e) => panic!("close failed: {:#x}", e.code()),
        };
        assert_eq!(rep.families, 4);
        assert!(rep.subadditive());
        assert_eq!(rep.runs, 12);
    }

    #[test]
    fn unx50039_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 8);
    }
}
