//! UNX-F50019 B21 批防重 grep 执行记录（关键词族：color/gamut/icc/intent）（0x42e80 段）
//!
//! 锚点：UNX-F50019-J1 批收口防重留痕；判据锚定 §6.1。
//!
//! 数据段：`SEG_BASE = 0x42e80`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 8`，
//! 逐项见 [`GrepError`]。
//!
//! 「防重」在这里有两层硬语义：
//! 1. **行级去重**——同一行被多个关键词命中只计一次，账本维护一条严格递增的
//!    全局并集 `union`，每次执行回报本次**新增**行数 `added`；
//! 2. **执行级留痕**——同一关键词重复执行必须被标记 `repeat=true` 且
//!    `added==0`，不许把「跑了两遍」伪装成「又发现 N 行」。
//!
//! 扫描是**真扫**：逐字节大小写不敏感子串匹配（不依赖 `str::contains`，
//! 因为契约要求行为可复算且对非 ASCII 语料不发生误配）。族（color/gamut/icc/
//! intent）是闭集，同一关键词换族重跑即 `FamilyMismatch`——族错标会让批收口
//! 的覆盖统计虚高。
//!
//! 结构清单：
//! * [`Family`] + [`FAMILY_KEYWORDS`]：四族关键词表（8 个关键词，逐族 ≥1）；
//! * [`GrepRun`]：单次执行记录（关键词 / 族 / 命中行 / 新增行 / 是否重复）；
//! * [`GrepLedger`]：语料 + 执行账 + 全局并集 + 族并集（四本小账）；
//! * [`injection_core`]：7 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x42e80 独占 0x20）。
pub const SEG_BASE: u32 = 0x42e80;

const E_EMPTY_KEYWORD: u16 = 1;
const E_KEYWORD_LONG: u16 = 2;
const E_NON_ASCII: u16 = 3;
const E_CORPUS_EMPTY: u16 = 4;
const E_LEDGER_FULL: u16 = 5;
const E_FAMILY_MISMATCH: u16 = 6;
const E_UNKNOWN_FAMILY: u16 = 7;
const E_BAD_SEQ: u16 = 8;

/// grep 账错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrepError {
    /// 关键词为空或纯空白。
    EmptyKeyword,
    /// 关键词超长。
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
    /// 执行序号越界。
    BadSeq,
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
            GrepError::BadSeq => E_BAD_SEQ,
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
            GrepError::BadSeq => "run sequence out of range",
        }
    }
}

/// 关键词族（批收口的四根柱子）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// `color` 族。
    Color,
    /// `gamut` 族。
    Gamut,
    /// `icc` 族。
    Icc,
    /// `intent` 族。
    Intent,
}

impl Family {
    /// 族下标 0..4，用于族并集数组。
    pub fn index(self) -> usize {
        match self {
            Family::Color => 0,
            Family::Gamut => 1,
            Family::Icc => 2,
            Family::Intent => 3,
        }
    }

    /// 族标签。
    pub fn tag(self) -> &'static str {
        match self {
            Family::Color => "color",
            Family::Gamut => "gamut",
            Family::Icc => "icc",
            Family::Intent => "intent",
        }
    }

    /// 解析族标签；不认识返回 `None`（不兜底到任意族）。
    pub fn from_tag(t: &str) -> Option<Family> {
        if eq_ci_str(t, "color") {
            Some(Family::Color)
        } else if eq_ci_str(t, "gamut") {
            Some(Family::Gamut)
        } else if eq_ci_str(t, "icc") {
            Some(Family::Icc)
        } else if eq_ci_str(t, "intent") {
            Some(Family::Intent)
        } else {
            None
        }
    }

    /// 全部四族（遍历用）。
    pub const ALL: [Family; 4] = [Family::Color, Family::Gamut, Family::Icc, Family::Intent];
}

/// 关键词上限（字节）。
pub const KEYWORD_CAP: usize = 32;

/// 关键词表：四族共 8 个关键词，每族 ≥1（族覆盖不允许挂零）。
pub const FAMILY_KEYWORDS: [(&str, Family); 8] = [
    ("color", Family::Color),
    ("gray", Family::Color),
    ("gamut", Family::Gamut),
    ("knee", Family::Gamut),
    ("icc", Family::Icc),
    ("curv", Family::Icc),
    ("intent", Family::Intent),
    ("perceptual", Family::Intent),
];

/// 扫描语料（真实模块名 + 主题词，逐行一条）。
pub const CORPUS: [&str; 21] = [
    "unx50001_gamutmap   sRGB gamut 映射内核",
    "unx50002_color_icc_loader  加载 ICC profile 头",
    "unx50003_intent_dispatch  four rendering intents",
    "unx50004_gamut_boundary   边界色度二分",
    "unx50005_COLOR_fallback   内置 sRGB 兜底",
    "unx50006_icc_tagtable     标签表区间校验",
    "unx50007_INTENT_absolute  绝对比色档",
    "unx50008_gamut_clip       裁剪路径",
    "unx50009_color_gray       BT.709 灰度",
    "unx50010_icc_curve        curv TRC 解析",
    "unx50011_intent_perceptual 感知压缩",
    "unx50012_gamut_knee       knee=0.82*边界",
    "unx50013_color_duotone    双色分色",
    "unx50014_icc_matrix       色料矩阵求逆",
    "unx50015_intent_saturation 饱和度档",
    "unx50016_gamut_report     100 色块统计",
    "unx50017_colort1          T1 真机对照槽位",
    "unx50018_b21vec           用例集生成器",
    "unx50019_b21grep          防重 grep 记录",
    "unx50020_b21seal          批收官印",
    "unx50021_nup              N-up 布局引擎",
];

fn eq_ci(a: u8, b: u8) -> bool {
    a.to_ascii_lowercase() == b.to_ascii_lowercase()
}

/// 字符串级大小写不敏感相等（长度必须先相等，避免前缀误判）。
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
/// 非 ASCII 尾字节（≥0x80）经 `to_ascii_lowercase` 原样保留，而 ASCII 关键词
/// 的每个字节都 <0x80，因此不可能在 UTF-8 多字节序列内部误命中——这是本实现
/// 敢直接按字节扫中文语料的依据。
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

/// 在有序向量里插入并保持严格递增；返回是否真的新增。
fn insert_sorted(v: &mut Vec<u16>, x: u16) -> bool {
    match v.binary_search(&x) {
        Ok(_) => false,
        Err(pos) => {
            v.insert(pos, x);
            true
        }
    }
}

/// 扫描一遍语料，返回命中行的下标（升序，天然去重）。
pub fn scan(lines: &[&'static str], needle: &str) -> Vec<u16> {
    let mut out = Vec::new();
    let nb = needle.as_bytes();
    if nb.is_empty() {
        return out;
    }
    let mut i = 0usize;
    while i < lines.len() {
        match lines.get(i) {
            Some(l) => {
                if contains_ci(l.as_bytes(), nb) {
                    out.push(i as u16);
                }
            }
            None => {}
        }
        i += 1;
    }
    out
}

/// 一次 grep 执行的记录。
#[derive(Clone, Debug, PartialEq)]
pub struct GrepRun {
    /// 执行序号（本账内自 0 递增）。
    pub seq: u16,
    /// 关键词（保留首次执行的原始大小写）。
    pub keyword: String,
    /// 归属族。
    pub family: Family,
    /// 命中行下标（升序）。
    pub hits: Vec<u16>,
    /// 本次为全局并集**新增**的行数。
    pub added: u16,
    /// 是否与已有执行重复（关键词相同）。
    pub repeat: bool,
}

impl GrepRun {
    /// 本次命中数。
    pub fn hit_count(&self) -> usize {
        self.hits.len()
    }

    /// 一行落账文本。
    pub fn ledger_line(&self) -> String {
        format!(
            "#{} {} [{}] hits={} added={} repeat={}",
            self.seq,
            self.keyword,
            self.family.tag(),
            self.hits.len(),
            self.added,
            if self.repeat { "yes" } else { "no" }
        )
    }
}

/// grep 执行账：语料 + 执行记录 + 全局并集 + 四族并集。
pub struct GrepLedger {
    lines: Vec<&'static str>,
    runs: Vec<GrepRun>,
    union: Vec<u16>,
    family_union: [Vec<u16>; 4],
    capacity: usize,
    rejected: u32,
}

impl GrepLedger {
    /// 建账；`capacity` 为执行记录上限（语料行数不限，但只收非空语料）。
    pub fn new(corpus: &[&'static str], capacity: usize) -> GrepLedger {
        let cap = if capacity == 0 { 1 } else { capacity.min(256) };
        let mut lines = Vec::with_capacity(corpus.len());
        let mut i = 0usize;
        while i < corpus.len() {
            if let Some(l) = corpus.get(i) {
                if !l.trim().is_empty() {
                    lines.push(*l);
                }
            }
            i += 1;
        }
        GrepLedger {
            lines,
            runs: Vec::with_capacity(cap),
            union: Vec::new(),
            family_union: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            capacity: cap,
            rejected: 0,
        }
    }

    fn reject<T>(&mut self, e: GrepError) -> Result<T, GrepError> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    /// 执行一次 grep：关键词先过四道闸（空 / 超长 / 非 ASCII / 容量），
    /// 再查族一致性，最后才真扫——任何一关不过都不留执行记录。
    pub fn run(&mut self, keyword: &str, family: Family) -> Result<u16, GrepError> {
        if self.lines.is_empty() {
            return self.reject(GrepError::CorpusEmpty);
        }
        let kw = keyword.trim();
        if kw.is_empty() {
            return self.reject(GrepError::EmptyKeyword);
        }
        if kw.len() > KEYWORD_CAP {
            return self.reject(GrepError::KeywordLong);
        }
        if !kw.is_ascii() {
            return self.reject(GrepError::NonAscii);
        }
        if self.runs.len() >= self.capacity {
            return self.reject(GrepError::LedgerFull);
        }
        // 族一致性：该关键词若已执行过，族必须一致（否则收口统计会重复计族）。
        let mut repeat = false;
        let mut conflict = false;
        let mut i = 0usize;
        while i < self.runs.len() {
            if let Some(r) = self.runs.get(i) {
                if eq_ci_str(&r.keyword, kw) {
                    if r.family != family {
                        conflict = true;
                    }
                    repeat = true;
                }
            }
            i += 1;
        }
        if conflict {
            return self.reject(GrepError::FamilyMismatch);
        }
        let hits = scan(&self.lines, kw);
        let mut added = 0u16;
        let fi = family.index();
        let mut k = 0usize;
        while k < hits.len() {
            if let Some(line) = hits.get(k) {
                if insert_sorted(&mut self.union, *line) {
                    added = added.saturating_add(1);
                }
                if let Some(fu) = self.family_union.get_mut(fi) {
                    insert_sorted(fu, *line);
                }
            }
            k += 1;
        }
        let seq = self.runs.len() as u16;
        self.runs.push(GrepRun {
            seq,
            keyword: kw.to_string(),
            family,
            hits,
            added,
            repeat,
        });
        Ok(seq)
    }

    /// 按关键词表整批执行（8 个关键词，族序固定）；返回成功执行条数。
    pub fn run_table(&mut self) -> Result<usize, GrepError> {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < FAMILY_KEYWORDS.len() {
            match FAMILY_KEYWORDS.get(i) {
                Some((kw, fam)) => {
                    self.run(kw, *fam)?;
                    n += 1;
                }
                None => break,
            }
            i += 1;
        }
        Ok(n)
    }

    /// 按序号取执行记录（越界返回 `None`，不 panic）。
    pub fn run_of(&self, seq: u16) -> Option<&GrepRun> {
        self.runs.get(seq as usize)
    }

    /// 严格取记录：越界返回 [`GrepError::BadSeq`]（对账路径不许「查不到就当没有」）。
    pub fn require_run(&self, seq: u16) -> Result<&GrepRun, GrepError> {
        match self.runs.get(seq as usize) {
            Some(r) => Ok(r),
            None => Err(GrepError::BadSeq),
        }
    }

    /// 执行记录条数。
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    /// 是否尚未执行过任何关键词。
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// 执行记录容量。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 语料行数（空行已剔除）。
    pub fn corpus_len(&self) -> usize {
        self.lines.len()
    }

    /// 全局去重命中行数。
    pub fn union_len(&self) -> usize {
        self.union.len()
    }

    /// 全局并集快照（升序）。
    pub fn union_snapshot(&self) -> Vec<u16> {
        let mut out = Vec::with_capacity(self.union.len());
        let mut i = 0usize;
        while i < self.union.len() {
            if let Some(x) = self.union.get(i) {
                out.push(*x);
            }
            i += 1;
        }
        out
    }

    /// 族并集快照（升序）。
    pub fn family_snapshot(&self, f: Family) -> Vec<u16> {
        let mut out = Vec::new();
        if let Some(v) = self.family_union.get(f.index()) {
            let mut i = 0usize;
            while i < v.len() {
                if let Some(x) = v.get(i) {
                    out.push(*x);
                }
                i += 1;
            }
        }
        out
    }

    /// 族覆盖行数。
    pub fn family_covered(&self, f: Family) -> usize {
        match self.family_union.get(f.index()) {
            Some(v) => v.len(),
            None => 0,
        }
    }

    /// 已有命中的族数（0..=4）。批收口要求 4/4。
    pub fn coverage(&self) -> u8 {
        let mut n = 0u8;
        let mut i = 0usize;
        while i < Family::ALL.len() {
            if let Some(f) = Family::ALL.get(i) {
                if self.family_covered(*f) > 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 累计新增行数（应恒等于 `union_len`）。
    pub fn added_total(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.runs.len() {
            if let Some(r) = self.runs.get(i) {
                n += r.added as usize;
            }
            i += 1;
        }
        n
    }

    /// 重复执行条数。
    pub fn repeats(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.runs.len() {
            if let Some(r) = self.runs.get(i) {
                if r.repeat {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 累计被拒次数（显性记账）。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }
}

/// 造一本已整批执行过的账（8 关键词，四族全覆盖）。
pub fn standard_ledger() -> GrepLedger {
    let mut l = GrepLedger::new(&CORPUS, 32);
    let _ = l.run_table();
    l
}

/// 注入核：8 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;

    // 1) 空关键词
    let mut a = GrepLedger::new(&CORPUS, 4);
    if a.run("", Family::Color).err() == Some(GrepError::EmptyKeyword) {
        caught += 1;
    }

    // 2) 纯空白关键词（trim 后为空）
    let mut b = GrepLedger::new(&CORPUS, 4);
    if b.run("   ", Family::Color).err() == Some(GrepError::EmptyKeyword) {
        caught += 1;
    }

    // 3) 非 ASCII 关键词
    let mut c = GrepLedger::new(&CORPUS, 4);
    if c.run("色域", Family::Gamut).err() == Some(GrepError::NonAscii) {
        caught += 1;
    }

    // 4) 超长关键词
    let mut d = GrepLedger::new(&CORPUS, 4);
    let long = "gamutgamutgamutgamutgamutgamutgamut"; // 35 字节 > 32
    if d.run(long, Family::Gamut).err() == Some(GrepError::KeywordLong) {
        caught += 1;
    }

    // 5) 空语料
    let mut e = GrepLedger::new(&[], 4);
    if e.run("color", Family::Color).err() == Some(GrepError::CorpusEmpty) {
        caught += 1;
    }

    // 6) 族错标：color 首跑属 Color，二跑报 Gamut
    let mut f = GrepLedger::new(&CORPUS, 4);
    let first = f.run("color", Family::Color).is_ok();
    if first && f.run("COLOR", Family::Gamut).err() == Some(GrepError::FamilyMismatch) {
        caught += 1;
    }

    // 7) 容量满：容量 1 的账跑第二个关键词
    let mut g = GrepLedger::new(&CORPUS, 1);
    let ok_first = g.run("color", Family::Color).is_ok();
    if ok_first && g.run("gamut", Family::Gamut).err() == Some(GrepError::LedgerFull) {
        caught += 1;
    }

    // 8) 严格取记录时序号越界
    if g.require_run(9).err() == Some(GrepError::BadSeq) && g.require_run(0).is_ok() {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50019 自检：12 条判据。
pub fn run_unx50019_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50019");

    // J1-1 大小写不敏感：同一关键词两种写法命中集逐项相等且非空。
    let lower = scan(&CORPUS, "color");
    let upper = scan(&CORPUS, "COLOR");
    let mixed = scan(&CORPUS, "CoLoR");
    let ok = !lower.is_empty() && lower == upper && lower == mixed;
    let det = format!("n={}", lower.len());
    s.add_dyn("50019-J1-大小写不敏感", ok, &det);

    // J1-2 匹配真实性（双向）：命中集中每一行确实含词，未命中的每一行确实不含。
    let gamut_hits = scan(&CORPUS, "gamut");
    let mut truth = !gamut_hits.is_empty();
    let mut i = 0usize;
    while i < CORPUS.len() {
        if let Some(line) = CORPUS.get(i) {
            let listed = gamut_hits.binary_search(&(i as u16)).is_ok();
            if contains_ci(line.as_bytes(), "gamut".as_bytes()) != listed {
                truth = false;
            }
        }
        i += 1;
    }
    s.add("50019-J1-命中双向核", truth, if truth { "" } else { "命中集与文本矛盾" });

    // J1-3 防重：同关键词二跑 repeat=true、added=0、并集不变、账长 +1。
    let mut l = standard_ledger();
    let before_union = l.union_len();
    let before_runs = l.len();
    let seq2 = l.run("gamut", Family::Gamut);
    let repeat_ok = match seq2 {
        Ok(seq) => match l.run_of(seq) {
            Some(r) => r.repeat && r.added == 0 && r.hits.len() > 0,
            None => false,
        },
        Err(_) => false,
    };
    let ok = repeat_ok && l.union_len() == before_union && l.len() == before_runs + 1 && l.repeats() == 1;
    let det = format!("u={}/{} runs={}", l.union_len(), before_union, l.len());
    s.add_dyn("50019-J1-防重留痕", ok, &det);

    // J1-4 并集守恒：全局并集 == 各次执行命中集的合并（重算比较）。
    let mut recomputed: Vec<u16> = Vec::new();
    let mut k = 0usize;
    while k < l.len() {
        if let Some(r) = l.run_of(k as u16) {
            let mut h = 0usize;
            while h < r.hits.len() {
                if let Some(x) = r.hits.get(h) {
                    insert_sorted(&mut recomputed, *x);
                }
                h += 1;
            }
        }
        k += 1;
    }
    let ok = recomputed == l.union_snapshot() && l.added_total() == l.union_len();
    let det = format!("re={} u={} add={}", recomputed.len(), l.union_len(), l.added_total());
    s.add_dyn("50019-J1-并集守恒", ok, &det);

    // J1-5 四族覆盖 4/4，且每族并集严格递增无重复。
    let mut cov_ok = l.coverage() == 4;
    let mut f = 0usize;
    while f < Family::ALL.len() {
        if let Some(fam) = Family::ALL.get(f) {
            let snap = l.family_snapshot(*fam);
            if snap.is_empty() {
                cov_ok = false;
            }
            let mut j = 1usize;
            while j < snap.len() {
                if let (Some(a), Some(b)) = (snap.get(j - 1), snap.get(j)) {
                    if a >= b {
                        cov_ok = false;
                    }
                }
                j += 1;
            }
        }
        f += 1;
    }
    let det = format!("cov={}/4 c={} g={} i={} n={}", l.coverage(), l.family_covered(Family::Color), l.family_covered(Family::Gamut), l.family_covered(Family::Icc), l.family_covered(Family::Intent));
    s.add_dyn("50019-J1-四族覆盖", cov_ok, &det);

    // J1-6 族错标必拒，且不落执行记录。
    let mut m = standard_ledger();
    let runs_before = m.len();
    let ok = m.run("GAMUT", Family::Color).err() == Some(GrepError::FamilyMismatch)
        && m.len() == runs_before;
    s.add("50019-J1-族错标拒", ok, if ok { "" } else { "错标进了账" });

    // J1-7 顺序无关：不同执行顺序得到同一并集（集合语义）。
    let mut x = GrepLedger::new(&CORPUS, 16);
    let mut y = GrepLedger::new(&CORPUS, 16);
    let _ = x.run("gamut", Family::Gamut);
    let _ = x.run("intent", Family::Intent);
    let _ = y.run("intent", Family::Intent);
    let _ = y.run("gamut", Family::Gamut);
    let ok = x.union_snapshot() == y.union_snapshot() && x.union_len() > 0;
    let det = format!("ux={} uy={}", x.union_len(), y.union_len());
    s.add_dyn("50019-J1-顺序无关", ok, &det);

    // J1-8 确定性：同入参两本账，执行记录逐条相等。
    let a = standard_ledger();
    let b = standard_ledger();
    let mut same = a.len() == b.len() && a.len() == 8;
    let mut t = 0usize;
    while t < a.len() {
        match (a.run_of(t as u16), b.run_of(t as u16)) {
            (Some(p), Some(q)) => {
                if p != q {
                    same = false;
                }
            }
            _ => same = false,
        }
        t += 1;
    }
    s.add("50019-J1-执行确定", same, if same { "" } else { "两次执行不一致" });

    // J1-9 空语料 / 空关键词 / 非 ASCII 三类显式拒绝，且都不落记录。
    let mut e = GrepLedger::new(&[], 4);
    let c1 = e.run("color", Family::Color).err() == Some(GrepError::CorpusEmpty) && e.is_empty();
    let mut z = GrepLedger::new(&CORPUS, 4);
    let c2 = z.run("  ", Family::Color).err() == Some(GrepError::EmptyKeyword);
    let c3 = z.run("色域", Family::Gamut).err() == Some(GrepError::NonAscii);
    let ok = c1 && c2 && c3 && z.is_empty() && z.rejected() == 2;
    let det = format!("rej={}", z.rejected());
    s.add_dyn("50019-J1-三类入参拒", ok, &det);

    // J1-10 序号查询：越界返回 BadSeq/None，合法序号取回同一记录（往返一致）。
    let l2 = standard_ledger();
    let roundtrip = match l2.require_run(3) {
        Ok(r) => {
            let again = match l2.require_run(r.seq) {
                Ok(x) => x.keyword == r.keyword && x.family == r.family,
                Err(_) => false,
            };
            r.seq == 3 && again
        }
        Err(_) => false,
    };
    let ok = roundtrip
        && l2.run_of(999).is_none()
        && l2.require_run(999).err() == Some(GrepError::BadSeq);
    s.add("50019-J1-序号往返", ok, if ok { "" } else { "序号查询不闭环" });

    // J1-11 账实相符：执行条数 == 关键词表条数，且每族至少一条执行。
    let mut per_family = [0u8; 4];
    let mut i2 = 0usize;
    while i2 < l2.len() {
        if let Some(r) = l2.run_of(i2 as u16) {
            if let Some(c) = per_family.get_mut(r.family.index()) {
                *c = c.saturating_add(1);
            }
        }
        i2 += 1;
    }
    let mut all_families = true;
    let mut q = 0usize;
    while q < per_family.len() {
        if let Some(v) = per_family.get(q) {
            if *v == 0 {
                all_families = false;
            }
        }
        q += 1;
    }
    let ok = l2.len() == FAMILY_KEYWORDS.len() && all_families && l2.repeats() == 0;
    let det = format!("runs={} kw={}", l2.len(), FAMILY_KEYWORDS.len());
    s.add_dyn("50019-J1-账实相符", ok, &det);

    // J1-12 七类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50019-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50019_checks_green() {
        let set = run_unx50019_checks();
        assert!(set.all_passed(), "UNX-F50019 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50019_dedup_is_stable() {
        let mut l = standard_ledger();
        let u0 = l.union_len();
        let _ = l.run("color", Family::Color);
        let _ = l.run("COLOR", Family::Color);
        assert_eq!(l.union_len(), u0);
        assert_eq!(l.added_total(), u0);
        assert_eq!(l.repeats(), 2);
    }

    #[test]
    fn unx50019_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 8);
    }
}
