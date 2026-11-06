//! UNX-F50119 B26 批防重 grep 执行记录（关键词族：passthrough/pdf/ps/fallback）（0x43b00 段）
//!
//! 锚点：UNX-F50119-J1 批收口防重留痕；判据锚定 §6.1。
//!
//! B26 批的收口结论必须能回答「这四个关键词族到底在源码里被谁引用过几次」。
//! 手工 grep 的问题不在于漏，而在于**重复**：同一条命中被两个模式在同一行上
//! 各记一次，结论就会被放大成「引用了两次」。本模块把「一次扫描」建模成：
//!
//! * `Family` —— 四个关键词族（passthrough / pdf / ps / fallback），编码固定；
//! * `Pattern` —— 归一化后的模式（ASCII 小写 + 空白折叠），注册期即拒绝空模式；
//! * `PatternSet` —— 定容模式池，同族同文本重复注册一律拒绝；
//! * `TraceLog` —— 留痕账本，(族, 行号) 为唯一键，重复痕必须显式报错而不是静默丢；
//! * `ScanResult` —— 一次扫描的结果（行数、命中、被去重的候选数、摘要）。
//!
//! 去重口径（防重的定义）：**同一行、同一族只留一条痕**；跨族不合并——同一行
//! 同时出现 `pdf` 与 `passthrough` 是两个不同族的证据，合并会丢失语义。
//!
//! 错误路径：段内闭集 `SEG_BASE + n`，全部码值在注入核/判据里被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43b00 独占 0x20）。
pub const SEG_BASE: u32 = 0x43b00;

pub const E_FAMILY: u16 = 1;
pub const E_EMPTY_PATTERN: u16 = 2;
pub const E_PATTERN_LONG: u16 = 3;
pub const E_PATTERN_BAD: u16 = 4;
pub const E_DUP_PATTERN: u16 = 5;
pub const E_CAPACITY: u16 = 6;
pub const E_LINE_RANGE: u16 = 7;
pub const E_DUP_HIT: u16 = 8;
pub const E_EMPTY_CORPUS: u16 = 9;
pub const E_NO_PATTERNS: u16 = 10;

/// grep 执行记录错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrepError {
    Family,
    EmptyPattern,
    PatternLong,
    PatternBad,
    DupPattern,
    Capacity,
    LineRange,
    DupHit,
    EmptyCorpus,
    NoPatterns,
}

impl GrepError {
    pub fn code(self) -> u32 {
        let n = match self {
            GrepError::Family => E_FAMILY,
            GrepError::EmptyPattern => E_EMPTY_PATTERN,
            GrepError::PatternLong => E_PATTERN_LONG,
            GrepError::PatternBad => E_PATTERN_BAD,
            GrepError::DupPattern => E_DUP_PATTERN,
            GrepError::Capacity => E_CAPACITY,
            GrepError::LineRange => E_LINE_RANGE,
            GrepError::DupHit => E_DUP_HIT,
            GrepError::EmptyCorpus => E_EMPTY_CORPUS,
            GrepError::NoPatterns => E_NO_PATTERNS,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            GrepError::Family => "keyword family not in the B26 four",
            GrepError::EmptyPattern => "pattern folds to empty",
            GrepError::PatternLong => "pattern longer than 48 bytes",
            GrepError::PatternBad => "pattern contains control byte",
            GrepError::DupPattern => "same family/text already registered",
            GrepError::Capacity => "pattern pool or trace log full",
            GrepError::LineRange => "line number outside corpus",
            GrepError::DupHit => "same (family,line) trace already present",
            GrepError::EmptyCorpus => "corpus has no non-blank line",
            GrepError::NoPatterns => "pattern pool is empty",
        }
    }
}

pub const MAX_PATTERNS: usize = 8;
pub const MAX_HITS: usize = 32;
pub const MAX_PATTERN_LEN: usize = 48;

/// B26 的四个关键词族。编码固定 1..=4。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Passthrough,
    Pdf,
    Ps,
    Fallback,
}

impl Family {
    pub const ALL: [Family; 4] = [
        Family::Passthrough,
        Family::Pdf,
        Family::Ps,
        Family::Fallback,
    ];

    pub fn code(self) -> u8 {
        match self {
            Family::Passthrough => 1,
            Family::Pdf => 2,
            Family::Ps => 3,
            Family::Fallback => 4,
        }
    }

    pub fn from_code(c: u8) -> Result<Family, GrepError> {
        match c {
            1 => Ok(Family::Passthrough),
            2 => Ok(Family::Pdf),
            3 => Ok(Family::Ps),
            4 => Ok(Family::Fallback),
            _ => Err(GrepError::Family),
        }
    }

    /// 族名反解：大小写不敏感，不认识就是错——不做「猜成 fallback」。
    pub fn from_name(s: &str) -> Result<Family, GrepError> {
        if s.eq_ignore_ascii_case("passthrough") {
            Ok(Family::Passthrough)
        } else if s.eq_ignore_ascii_case("pdf") {
            Ok(Family::Pdf)
        } else if s.eq_ignore_ascii_case("ps") {
            Ok(Family::Ps)
        } else if s.eq_ignore_ascii_case("fallback") {
            Ok(Family::Fallback)
        } else {
            Err(GrepError::Family)
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Family::Passthrough => "passthrough",
            Family::Pdf => "pdf",
            Family::Ps => "ps",
            Family::Fallback => "fallback",
        }
    }

    pub fn slot(self) -> usize {
        (self.code() - 1) as usize
    }
}

fn is_blank_byte(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' || b == 0x0b || b == 0x0c
}

/// 归一化：ASCII 小写 + 连续空白折成单空格 + 去首尾空白。
///
/// 为什么必须折叠空白：源码里 `pass  through`（对齐填充）与 `pass through`
/// 是同一件事，不折叠就会把同一处引用记成两条痕。
fn fold(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut pending_space = false;
    for b in raw.bytes() {
        if is_blank_byte(b) {
            if !out.is_empty() {
                pending_space = true;
            }
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(b.to_ascii_lowercase() as char);
        }
    }
    out
}

/// 编译期拒绝：含控制字节（除空白）的 pattern 不是正常搜索词，多半是二进制混入。
fn has_control(raw: &str) -> bool {
    for b in raw.bytes() {
        if b < 0x20 && !is_blank_byte(b) {
            return true;
        }
        if b == 0x7f {
            return true;
        }
    }
    false
}

/// 已注册模式：`folded` 是唯一用于匹配的形态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    pub family: Family,
    pub folded: String,
}

/// 定容模式池（同族同文本不可重复注册）。
pub struct PatternSet {
    pats: Vec<Pattern>,
    cap: usize,
}

impl PatternSet {
    pub fn new(cap: usize) -> Result<PatternSet, GrepError> {
        if cap == 0 || cap > MAX_PATTERNS {
            return Err(GrepError::Capacity);
        }
        Ok(PatternSet {
            pats: Vec::with_capacity(cap),
            cap,
        })
    }

    pub fn len(&self) -> usize {
        self.pats.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pats.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&Pattern> {
        self.pats.get(i)
    }

    /// 注册一个模式。先归一化再判重——否则 `PDF` 与 `pdf` 会被当成两个模式，
    /// 同一行就被记两次，正好踩中本模块要防的那个放大错误。
    pub fn register(&mut self, family: Family, raw: &str) -> Result<usize, GrepError> {
        if has_control(raw) {
            return Err(GrepError::PatternBad);
        }
        if raw.len() > MAX_PATTERN_LEN {
            return Err(GrepError::PatternLong);
        }
        let folded = fold(raw);
        if folded.is_empty() {
            return Err(GrepError::EmptyPattern);
        }
        if self.pats.len() >= self.cap {
            return Err(GrepError::Capacity);
        }
        let mut i = 0usize;
        while i < self.pats.len() {
            if let Some(p) = self.pats.get(i) {
                if p.family == family && p.folded == folded {
                    return Err(GrepError::DupPattern);
                }
            }
            i += 1;
        }
        let at = self.pats.len();
        self.pats.push(Pattern { family, folded });
        Ok(at)
    }
}

/// 一条留痕：族 × 行号唯一。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub family: Family,
    pub line: u16,
    pub col: u16,
    pub digest: u32,
}

fn fnv32(seed: u32, bytes: &[u8]) -> u32 {
    let mut h = seed;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 留痕账本：按 (族, 行) 去重，重复写入显式报错并计数。
pub struct TraceLog {
    hits: Vec<Hit>,
    cap: usize,
    dup_seen: u16,
    range_rejects: u16,
}

impl TraceLog {
    pub fn new(cap: usize) -> Result<TraceLog, GrepError> {
        if cap == 0 || cap > MAX_HITS {
            return Err(GrepError::Capacity);
        }
        Ok(TraceLog {
            hits: Vec::with_capacity(cap),
            cap,
            dup_seen: 0,
            range_rejects: 0,
        })
    }

    pub fn len(&self) -> usize {
        self.hits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }

    pub fn dup_seen(&self) -> u16 {
        self.dup_seen
    }

    pub fn range_rejects(&self) -> u16 {
        self.range_rejects
    }

    pub fn get(&self, i: usize) -> Option<&Hit> {
        self.hits.get(i)
    }

    /// 写入一条痕迹。行号必须落在语料内；重复键拒绝但**计数**，
    /// 这样「防重」的动作本身也留痕（静默丢等于把证据抹了）。
    pub fn record(&mut self, h: Hit, corpus_lines: u16) -> Result<(), GrepError> {
        if h.line >= corpus_lines {
            self.range_rejects += 1;
            return Err(GrepError::LineRange);
        }
        if self.hits.len() >= self.cap {
            return Err(GrepError::Capacity);
        }
        let mut i = 0usize;
        while i < self.hits.len() {
            if let Some(x) = self.hits.get(i) {
                if x.family == h.family && x.line == h.line {
                    self.dup_seen += 1;
                    return Err(GrepError::DupHit);
                }
            }
            i += 1;
        }
        self.hits.push(h);
        Ok(())
    }

    /// 按族统计：[passthrough, pdf, ps, fallback]。
    pub fn coverage(&self) -> [u32; 4] {
        let mut cov = [0u32; 4];
        let mut i = 0usize;
        while i < self.hits.len() {
            if let Some(h) = self.hits.get(i) {
                let s = h.family.slot();
                if let Some(c) = cov.get_mut(s) {
                    *c += 1;
                }
            }
            i += 1;
        }
        cov
    }

    /// 未命中的族位图（bit0=passthrough … bit3=fallback）。诚实地把「零命中」
    /// 报出来，而不是让调用方以为「没覆盖 = 没问题」。
    pub fn uncovered_mask(&self) -> u8 {
        let cov = self.coverage();
        let mut mask = 0u8;
        let mut i = 0usize;
        while i < 4 {
            if let Some(c) = cov.get(i) {
                if *c == 0 {
                    mask |= 1u8 << i;
                }
            }
            i += 1;
        }
        mask
    }

    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.hits.len() as u64;
        let mut i = 0usize;
        while i < self.hits.len() {
            if let Some(x) = self.hits.get(i) {
                h ^= x.family.code() as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
                h ^= x.line as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
                h ^= x.col as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
                h ^= x.digest as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
            }
            i += 1;
        }
        h
    }
}

/// 一次扫描的结果。
#[derive(Clone, Debug, PartialEq)]
pub struct ScanResult {
    pub lines: u16,
    pub hits: Vec<Hit>,
    /// 被去重掉的候选数（同族同行多模式命中的次数）。
    pub deduped: u16,
    pub digest: u64,
}

impl ScanResult {
    /// 原始候选数 = 留痕数 + 去重数（守恒）。
    pub fn raw_candidates(&self) -> u32 {
        self.hits.len() as u32 + self.deduped as u32
    }

    pub fn coverage(&self) -> [u32; 4] {
        let mut cov = [0u32; 4];
        let mut i = 0usize;
        while i < self.hits.len() {
            if let Some(h) = self.hits.get(i) {
                let s = h.family.slot();
                if let Some(c) = cov.get_mut(s) {
                    *c += 1;
                }
            }
            i += 1;
        }
        cov
    }
}

/// 朴素子串查找（返回字节下标）。模式都很短（≤48），朴素扫描比建自动机更省。
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    let last = hay.len() - needle.len();
    let mut i = 0usize;
    while i <= last {
        let mut j = 0usize;
        let mut ok = true;
        while j < needle.len() {
            let a = hay.get(i + j);
            let b = needle.get(j);
            if a != b {
                ok = false;
                break;
            }
            j += 1;
        }
        if ok {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 扫描语料。每行每族最多留一条痕（防重），去重次数如实记在 `deduped` 里。
pub fn scan(doc: &str, pats: &PatternSet) -> Result<ScanResult, GrepError> {
    if doc.trim().is_empty() {
        return Err(GrepError::EmptyCorpus);
    }
    if pats.is_empty() {
        return Err(GrepError::NoPatterns);
    }
    let mut hits: Vec<Hit> = Vec::with_capacity(MAX_HITS);
    let mut deduped = 0u16;
    let mut line_no: u16 = 0;
    for line in doc.lines() {
        let folded = fold(line);
        let fb = folded.as_bytes();
        let mut fam_taken = [false; 4];
        let mut p = 0usize;
        while p < pats.len() {
            if let Some(pat) = pats.get(p) {
                if let Some(col) = find(fb, pat.folded.as_bytes()) {
                    let slot = pat.family.slot();
                    let already = match fam_taken.get(slot) {
                        Some(v) => *v,
                        None => true,
                    };
                    if already {
                        deduped += 1;
                    } else {
                        if let Some(cell) = fam_taken.get_mut(slot) {
                            *cell = true;
                        }
                        let digest = fnv32(0x811c_9dc5, line.as_bytes());
                        hits.push(Hit {
                            family: pat.family,
                            line: line_no,
                            col: col as u16,
                            digest,
                        });
                    }
                }
            }
            p += 1;
        }
        // 行号用饱和加法：超长语料截断在 u16::MAX，而不是回绕成 0 制造假痕。
        line_no = line_no.saturating_add(1);
    }
    let mut probe = match TraceLog::new(MAX_HITS) {
        Ok(t) => t,
        Err(_) => return Err(GrepError::Capacity),
    };
    let mut i = 0usize;
    while i < hits.len() {
        if let Some(h) = hits.get(i) {
            let _ = probe.record(*h, line_no);
        }
        i += 1;
    }
    let digest = probe.digest();
    Ok(ScanResult {
        lines: line_no,
        hits,
        deduped,
        digest,
    })
}

/// 诊断文本。
pub fn render(r: &ScanResult) -> String {
    let c = r.coverage();
    format!(
        "grep lines={} pt={} pdf={} ps={} fb={} dedup={}",
        r.lines, c[0], c[1], c[2], c[3], r.deduped
    )
}

// ---------------------------------------------------------------------------
// 注入核：六类坏样本必须全部被拒
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    // 1 未知族名
    if Family::from_name("postscript2").err() == Some(GrepError::Family) {
        caught += 1;
    }

    let mut ps = match PatternSet::new(4) {
        Ok(p) => p,
        Err(_) => return (0, total),
    };

    // 2 空模式（只有空白，折叠后为空）
    if ps.register(Family::Pdf, "   ").err() == Some(GrepError::EmptyPattern) {
        caught += 1;
    }

    // 3 重复模式（大小写不同、文本相同）
    if ps.register(Family::Pdf, "PDF").is_ok() {
        if ps.register(Family::Pdf, "pdf").err() == Some(GrepError::DupPattern) {
            caught += 1;
        }
    }

    // 4 模式池容量
    let mut small = match PatternSet::new(1) {
        Ok(p) => p,
        Err(_) => return (caught, total),
    };
    if small.register(Family::Ps, "ps").is_ok() {
        if small.register(Family::Fallback, "fallback").err() == Some(GrepError::Capacity) {
            caught += 1;
        }
    }

    // 5 重复痕迹
    let mut log = match TraceLog::new(4) {
        Ok(l) => l,
        Err(_) => return (caught, total),
    };
    let h = Hit {
        family: Family::Passthrough,
        line: 1,
        col: 3,
        digest: 7,
    };
    if log.record(h, 10).is_ok() {
        if log.record(h, 10).err() == Some(GrepError::DupHit) {
            caught += 1;
        }
    }

    // 6 行号越界
    let mut log2 = match TraceLog::new(4) {
        Ok(l) => l,
        Err(_) => return (caught, total),
    };
    let far = Hit {
        family: Family::Ps,
        line: 99,
        col: 0,
        digest: 1,
    };
    if log2.record(far, 10).err() == Some(GrepError::LineRange) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50119_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50119");

    // 族表：四个名字可反解，编码可往返
    let mut ok = true;
    let mut i = 0usize;
    while i < Family::ALL.len() {
        if let Some(f) = Family::ALL.get(i) {
            if Family::from_name(f.label()).ok() != Some(*f) {
                ok = false;
            }
            if Family::from_code(f.code()).ok() != Some(*f) {
                ok = false;
            }
        }
        i += 1;
    }
    // J1-1 族名/编码往返
    s.add("50119-J1-族表往返", ok, if ok { "" } else { "族名或编码不闭合" });

    // J1-2 归一化：大小写 + 空白折叠一致
    let a = fold("  PASS   Through ");
    let b = fold("pass through");
    let ok = a == b && a == "pass through";
    let det = format!("{}", a);
    s.add_dyn("50119-J1-归一折叠", ok, &det);

    // 主场景语料：四族各一行，另加一行同族双命中
    let doc = "fn a() { passthrough_write(); }\n\
               load_pdf_profile();\n\
               emit_ps_header();\n\
               fallback_to_raster();\n\
               pdf and pdfx both here\n\
               nothing relevant\n";
    let mut pats = match PatternSet::new(8) {
        Ok(p) => p,
        Err(_) => return s,
    };
    let _ = pats.register(Family::Passthrough, "passthrough");
    let _ = pats.register(Family::Pdf, "pdf");
    let _ = pats.register(Family::Pdf, "pdfx");
    let _ = pats.register(Family::Ps, "ps");
    let _ = pats.register(Family::Fallback, "fallback");

    // J1-3 注册守恒：五个模式全部入池
    let ok = pats.len() == 5;
    let det = format!("n={}", pats.len());
    s.add_dyn("50119-J1-注册守恒", ok, &det);

    let r = match scan(doc, &pats) {
        Ok(r) => r,
        Err(_) => {
            s.add("50119-J1-扫描可用", false, "合法语料被拒");
            return s;
        }
    };

    // J1-4 四族各留一痕（pdf 行有两处命中但只留一痕）
    let cov = r.coverage();
    let ok = cov[0] == 1 && cov[1] == 2 && cov[2] == 1 && cov[3] == 1;
    let det = format!("{}/{}/{}/{}", cov[0], cov[1], cov[2], cov[3]);
    s.add_dyn("50119-J1-各族命中", ok, &det);

    // J1-5 防重：同族同行第二条被去重，原始候选 = 留痕 + 去重
    let ok = r.deduped == 1 && r.raw_candidates() == r.hits.len() as u32 + 1;
    let det = format!("dedup={} raw={} kept={}", r.deduped, r.raw_candidates(), r.hits.len());
    s.add_dyn("50119-J1-防重留痕", ok, &det);

    // J1-6 跨族不合并：同一行两个不同族 -> 两条痕
    let cross = "pdf passthrough\n";
    let cross_r = scan(cross, &pats);
    let ok = match cross_r {
        Ok(x) => x.hits.len() == 2 && x.deduped == 0,
        Err(_) => false,
    };
    s.add("50119-J1-跨族不合并", ok, if ok { "" } else { "跨族被误合并" });

    // J1-7 未覆盖族如实上报：只有 pdf 命中时，bit0/bit2/bit3 置位（bit1=pdf 覆盖）
    let only_pdf = "just a pdf line\n";
    let ok = match scan(only_pdf, &pats) {
        Ok(x) => {
            let mut probe = match TraceLog::new(8) {
                Ok(t) => t,
                Err(_) => return s,
            };
            let mut k = 0usize;
            while k < x.hits.len() {
                if let Some(h) = x.hits.get(k) {
                    let _ = probe.record(*h, x.lines);
                }
                k += 1;
            }
            probe.uncovered_mask() == 0b1101
        }
        Err(_) => false,
    };
    let det = format!("mask={:04b}", 0b1101);
    s.add_dyn("50119-J1-未覆盖如实", ok, &det);

    // J1-8 覆盖单调：追加一行不减少任何族的命中数
    let longer = "fn a() { passthrough_write(); }\nload_pdf_profile();\npdf and pdfx both here\nfallback_to_raster();\nps again\n";
    let r2 = scan(longer, &pats);
    let ok = match r2 {
        Ok(x) => {
            let c2 = x.coverage();
            c2[0] >= cov[0] && c2[1] >= cov[1] && c2[2] >= cov[2] && c2[3] >= cov[3]
        }
        Err(_) => false,
    };
    s.add("50119-J1-覆盖单调", ok, if ok { "" } else { "覆盖随语料减少" });

    // J1-9 确定性：同输入两次扫描摘要一致
    let again = scan(doc, &pats);
    let ok = match again {
        Ok(x) => x.digest == r.digest && x == r,
        Err(_) => false,
    };
    s.add("50119-J1-扫描确定", ok, if ok { "" } else { "同语料摘要漂移" });

    // J1-10 行号与列号在界内（留痕可回放）
    let mut ok = true;
    let mut k = 0usize;
    while k < r.hits.len() {
        if let Some(h) = r.hits.get(k) {
            if h.line >= r.lines {
                ok = false;
            }
        }
        k += 1;
    }
    s.add("50119-J1-行号在界", ok, if ok { "" } else { "留痕行号越界" });

    // J1-11 空语料 / 空模式池必拒
    let empty_set = match PatternSet::new(2) {
        Ok(p) => p,
        Err(_) => return s,
    };
    let ok = scan("   \n\t\n", &pats).err() == Some(GrepError::EmptyCorpus)
        && scan("anything\n", &empty_set).err() == Some(GrepError::NoPatterns);
    s.add("50119-J1-空输入必拒", ok, if ok { "" } else { "空输入被当成零命中" });

    // J1-12 模式编译闭集：控制字节 / 超长 / 非法容量
    let mut bad = match PatternSet::new(4) {
        Ok(p) => p,
        Err(_) => return s,
    };
    let mut long_pat = String::new();
    let mut n = 0usize;
    while n <= MAX_PATTERN_LEN {
        long_pat.push('a');
        n += 1;
    }
    let ok = bad.register(Family::Ps, "a\u{0001}b").err() == Some(GrepError::PatternBad)
        && bad.register(Family::Ps, &long_pat).err() == Some(GrepError::PatternLong)
        && bad.register(Family::Ps, "").err() == Some(GrepError::EmptyPattern)
        && PatternSet::new(0).err() == Some(GrepError::Capacity)
        && TraceLog::new(0).err() == Some(GrepError::Capacity);
    s.add("50119-J1-模式闭集", ok, if ok { "" } else { "坏模式被放行" });

    // J1-13 注入核：六类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50119-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50119_checks_green() {
        let set = run_unx50119_checks();
        assert!(set.all_passed(), "UNX-F50119 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50119_dedup_counts_once() {
        let mut p = PatternSet::new(4);
        assert!(p.is_ok());
        if let Ok(set) = p.as_mut() {
            assert!(set.register(Family::Pdf, "pdf").is_ok());
            assert!(set.register(Family::Pdf, "PDFX").is_ok());
            let r = scan("pdfx pdf\n", set);
            assert!(r.is_ok());
            if let Ok(x) = r {
                assert_eq!(x.hits.len(), 1);
                assert_eq!(x.deduped, 1);
            }
        }
    }

    #[test]
    fn unx50119_error_codes_in_segment() {
        assert_eq!(GrepError::Family.code(), SEG_BASE + 1);
        assert_eq!(GrepError::NoPatterns.code(), SEG_BASE + 10);
    }
}
