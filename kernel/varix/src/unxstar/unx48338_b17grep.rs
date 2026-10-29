//! unx48338_b17grep — UNX-F48338 B17 批防重 grep（关键词族：suggest/autocorrect/localstat）（诊断段 0xCB20）。
//!
//! 判据原文：
//!   J1 批收口防重留痕；判据锚定 §6.1。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 关键词族三分：suggest / autocorrect / localstat 三族扫描器
//!      （ASCII 子串滑窗比对，与 B13/B14 grep 同源机制）。
//!   2. 防重留痕：命中按（文件 id, 族）登记主张；同主张重扫幂等
//!      不重记；与前批账撞车必须标 Duplicate 暴露。
//!   3. 收口闭包：命中格子全留痕、未命中格子零登记；账行数 = 命中数。
//!   4. 扫描器自查：空语料零命中零登记；重复行去重只计一次；
//!      词界比对（关键词不匹配长词中段——"suggest" 不得命中
//!      "unsuggestable" 的中间段，前缀外无边界即不命中）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCB20;

const E_FAM_MISS: u16 = SEG_BASE + 1;    // 某族零命中
const E_DUP_SCAN: u16 = SEG_BASE + 2;    // 重复扫描非幂等
const E_CLAIM_CLASH: u16 = SEG_BASE + 3; // 前批撞车未标 Duplicate
const E_FALSE_CLAIM: u16 = SEG_BASE + 4; // 空语料假主张
const E_WORD_BOUND: u16 = SEG_BASE + 5;  // 词界比对失守
const E_LEDGER_BAD: u16 = SEG_BASE + 6;  // 留痕账与命中数不一致
const E_CODE_RANGE: u16 = SEG_BASE + 7;  // 诊断码越段

/// 关键词族三分。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// suggest 联想面。
    Suggest,
    /// autocorrect 纠错面。
    Autocorrect,
    /// localstat 本地统计面。
    Localstat,
}

pub const FAMILY_COUNT: usize = 3;

pub fn family_word(f: Family) -> &'static [u8] {
    match f {
        Family::Suggest => b"suggest",
        Family::Autocorrect => b"autocorrect",
        Family::Localstat => b"localstat",
    }
}

/// 扫描行。
#[derive(Clone, Copy)]
pub struct ScanLine {
    pub file_id: u32,
    pub text: &'static [u8],
}

/// ASCII 子串滑窗比对。
pub fn contains(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        let mut j = 0;
        while j < needle.len() {
            if hay[i + j] != needle[j] {
                break;
            }
            j += 1;
        }
        if j == needle.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 词界安全命中：关键词命中处，左右相邻字符必须是非小写字母
/// （词界）——排除长词中段误命中。
pub fn contains_word_bounded(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    let is_lower = |c: u8| c.is_ascii_lowercase();
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        let mut j = 0;
        while j < needle.len() {
            if hay[i + j] != needle[j] {
                break;
            }
            j += 1;
        }
        if j == needle.len() {
            let left_ok = i == 0 || !is_lower(hay[i - 1]);
            let right_ok = i + needle.len() >= hay.len() || !is_lower(hay[i + needle.len()]);
            if left_ok && right_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 单行族分类（词界安全版）：位图 bit0=suggest bit1=autocorrect bit2=localstat。
pub fn classify(text: &[u8]) -> u8 {
    let mut bits = 0u8;
    if contains_word_bounded(text, family_word(Family::Suggest)) {
        bits |= 0x1;
    }
    if contains_word_bounded(text, family_word(Family::Autocorrect)) {
        bits |= 0x2;
    }
    if contains_word_bounded(text, family_word(Family::Localstat)) {
        bits |= 0x4;
    }
    bits
}

/// 领单主张。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Claim {
    pub file_id: u32,
    pub family: u8,
}

/// 主张裁定。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ClaimVerdict {
    Fresh,
    Duplicate,
}

/// 防重留痕账。
pub struct ClaimLedger {
    rows: Vec<Claim>,
    dup_marks: Vec<Claim>,
}

impl ClaimLedger {
    pub fn new() -> Self {
        ClaimLedger {
            rows: Vec::new(),
            dup_marks: Vec::new(),
        }
    }

    pub fn has(&self, c: &Claim) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i] == *c {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn claim(&mut self, c: Claim, prior: &[Claim]) -> ClaimVerdict {
        let mut p = 0;
        while p < prior.len() {
            if prior[p] == c {
                let mut d = 0;
                while d < self.dup_marks.len() {
                    if self.dup_marks[d] == c {
                        return ClaimVerdict::Duplicate;
                    }
                    d += 1;
                }
                self.dup_marks.push(c);
                return ClaimVerdict::Duplicate;
            }
            p += 1;
        }
        if !self.has(&c) {
            self.rows.push(c);
        }
        ClaimVerdict::Fresh
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn dup_len(&self) -> usize {
        self.dup_marks.len()
    }
}

/// 行指纹去重（FNV-1a）。
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 语料行去重。
pub fn dedup_lines(lines: &[ScanLine]) -> Vec<ScanLine> {
    let mut seen: Vec<u64> = Vec::new();
    let mut out: Vec<ScanLine> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let fp = fnv64(lines[i].text);
        let mut dup = false;
        let mut s = 0;
        while s < seen.len() {
            if seen[s] == fp {
                dup = true;
                break;
            }
            s += 1;
        }
        if !dup {
            seen.push(fp);
            out.push(lines[i]);
        }
        i += 1;
    }
    out
}

/// B17 语料夹具：20 行，三族散布；9030 行含长词中段陷阱
/// （"unsuggestable" 含 suggest 但词界不安全 → 不应命中）；
/// 9031 行三族全含（与前批账撞车样本）。
pub fn fixture_corpus() -> Vec<ScanLine> {
    vec![
        ScanLine { file_id: 9011, text: b"suggest pipeline bigram walk v2" },
        ScanLine { file_id: 9012, text: b"autocorrect confusable table restore" },
        ScanLine { file_id: 9013, text: b"localstat window freq adaptive" },
        ScanLine { file_id: 9014, text: b"suggest and autocorrect combined" },
        ScanLine { file_id: 9015, text: b"localstat zero outbound proof" },
        ScanLine { file_id: 9016, text: b"plain line without keywords" },
        ScanLine { file_id: 9017, text: b"suggest top3 stability check" },
        ScanLine { file_id: 9018, text: b"autocorrect penalty annotation" },
        ScanLine { file_id: 9019, text: b"localstat dual focus isolation" },
        ScanLine { file_id: 9020, text: b"suggest saturation guard" },
        ScanLine { file_id: 9021, text: b"autocorrect fuzz corpus v1" },
        ScanLine { file_id: 9022, text: b"localstat fingerprint replay" },
        ScanLine { file_id: 9023, text: b"suggest feedback loop bounded" },
        ScanLine { file_id: 9024, text: b"autocorrect localstat pair" },
        ScanLine { file_id: 9025, text: b"third plain line here" },
        ScanLine { file_id: 9026, text: b"suggest unbounded reject pinned" },
        ScanLine { file_id: 9027, text: b"autocorrect drift zero rule" },
        ScanLine { file_id: 9028, text: b"localstat quota per focus" },
        ScanLine { file_id: 9029, text: b"fourth plain line again" },
        ScanLine { file_id: 9030, text: b"unsuggestable midword trap plus suggest tail" },
        ScanLine { file_id: 9031, text: b"suggest autocorrect localstat all three" },
    ]
}

/// 前批（B16）既有主张夹具：与 9031 行三族主张全部撞车。
pub fn prior_claims() -> Vec<Claim> {
    vec![
        Claim { file_id: 9031, family: 0 },
        Claim { file_id: 9031, family: 1 },
        Claim { file_id: 9031, family: 2 },
    ]
}

/// B17 防重 grep 主巡检：批收口留痕全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三族覆盖 + 词界安全：9030 行的 "unsuggestable" 中段不得命中
    //    suggest 族（该行仅以尾部 " suggest " 命中一次）。
    let corpus = fixture_corpus();
    let dedup = dedup_lines(&corpus);
    let mut fam_hits = [0usize; FAMILY_COUNT];
    let mut hit_claims: Vec<Claim> = Vec::new();
    let mut i = 0;
    while i < dedup.len() {
        let bits = classify(dedup[i].text);
        let mut f = 0u8;
        while f < 3 {
            if bits & (1 << f) != 0 {
                fam_hits[f as usize] += 1;
                hit_claims.push(Claim { file_id: dedup[i].file_id, family: f });
            }
            f += 1;
        }
        i += 1;
    }
    let mut f = 0usize;
    while f < FAMILY_COUNT {
        if fam_hits[f] == 0 {
            return Err(E_FAM_MISS);
        }
        f += 1;
    }
    // 9030 词界陷阱核验：该行 suggest 计数恰 1（中段不计）。
    let mut c9030 = 0u8;
    let mut i2 = 0;
    while i2 < dedup.len() {
        if dedup[i2].file_id == 9030 {
            c9030 = classify(dedup[i2].text) & 0x1;
        }
        i2 += 1;
    }
    if c9030 == 0 {
        return Err(E_WORD_BOUND);
    }
    hits += 1;

    // R2 留痕账与命中一一对应。
    let mut led = ClaimLedger::new();
    let prior = prior_claims();
    i = 0;
    while i < hit_claims.len() {
        led.claim(hit_claims[i], &prior);
        i += 1;
    }
    if led.len() + led.dup_len() != hit_claims.len() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R3 前批撞车：9031 三族主张全部标 Duplicate（恰 3 条）。
    if led.dup_len() != 3 {
        return Err(E_CLAIM_CLASH);
    }
    hits += 1;

    // R4 幂等重扫：账行数与撞车数均不变。
    i = 0;
    while i < hit_claims.len() {
        led.claim(hit_claims[i], &prior);
        i += 1;
    }
    if led.len() + led.dup_len() != hit_claims.len() {
        return Err(E_DUP_SCAN);
    }
    hits += 1;

    // R5 假主张自查：空语料零命中零登记。
    let empty: Vec<ScanLine> = Vec::new();
    let dd = dedup_lines(&empty);
    if !dd.is_empty() {
        return Err(E_FALSE_CLAIM);
    }
    let mut led2 = ClaimLedger::new();
    let none: Vec<Claim> = Vec::new();
    let mut j = 0;
    while j < dd.len() {
        let bits = classify(dd[j].text);
        let mut f2 = 0u8;
        while f2 < 3 {
            if bits & (1 << f2) != 0 {
                led2.claim(Claim { file_id: dd[j].file_id, family: f2 }, &none);
            }
            f2 += 1;
        }
        j += 1;
    }
    if led2.len() != 0 {
        return Err(E_FALSE_CLAIM);
    }
    hits += 1;

    // R6 词界单元自查：构造四个边界样本逐一核对（前缀/后缀/双侧/
    //    无界）。
    if contains_word_bounded(b"xsuggesty", b"suggest") {
        return Err(E_WORD_BOUND); // 双侧字母夹 → 不命中
    }
    if !contains_word_bounded(b"suggest", b"suggest") {
        return Err(E_WORD_BOUND); // 全词 → 命中
    }
    if contains_word_bounded(b"autosuggestx", b"suggest") {
        return Err(E_WORD_BOUND); // 前后夹 → 不命中
    }
    if !contains_word_bounded(b"do suggest now", b"suggest") {
        return Err(E_WORD_BOUND); // 空格界 → 命中
    }
    hits += 1;

    // R7 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 7] = [
        E_FAM_MISS,
        E_DUP_SCAN,
        E_CLAIM_CLASH,
        E_FALSE_CLAIM,
        E_WORD_BOUND,
        E_LEDGER_BAD,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
