//! unx48276_b14grep — UNX-F48276 B14 批防重 grep（关键词族：fcitx5/addon/interconnect）（诊断段 0xC360）。
//!
//! 判据原文：
//!   J1 批收口防重留痕；判据锚定 §6.1。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 关键词族三分：fcitx5 / addon / interconnect 三族扫描器，逐行
//!      分类命中（ASCII 子串滑窗比对，不依赖任何外部正则设施）。
//!   2. 防重留痕：扫描命中按（文件 id, 关键词族）登记为领单主张；
//!      同主张重复扫描幂等不重记；与既有批账（前批主张）同键位撞车
//!      必须标 Duplicate 暴露，不得静默吞并。
//!   3. 收口闭包：本批 20 文件 × 3 族的全组合网格中，命中格子必须
//!      全部留痕、未命中格子零登记——账行数 = 命中数，一一对应。
//!   4. 扫描器自查：空语料零命中零登记（防假留痕）；语料重复行
//!      去重后只计一次（防灌水）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC360;

const E_FAM_MISS: u16 = SEG_BASE + 1;   // 某关键词族零命中（覆盖缺失）
const E_DUP_SCAN: u16 = SEG_BASE + 2;   // 重复扫描非幂等（账行膨胀）
const E_CLAIM_CLASH: u16 = SEG_BASE + 3; // 前批撞车未标 Duplicate
const E_FALSE_CLAIM: u16 = SEG_BASE + 4; // 空语料产生留痕（假主张）
const E_LEDGER_BAD: u16 = SEG_BASE + 5; // 留痕账与命中数不一致
const E_DEWATER: u16 = SEG_BASE + 6;    // 重复行未去重（灌水）
const E_CODE_RANGE: u16 = SEG_BASE + 7; // 诊断码越段

/// 关键词族三分。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// fcitx5 互通面。
    Fcitx5,
    /// addon 引擎面。
    Addon,
    /// interconnect 对接面。
    Interconnect,
}

pub const FAMILY_COUNT: usize = 3;

/// 关键词族 → ASCII 关键词字节串（全小写）。
pub fn family_word(f: Family) -> &'static [u8] {
    match f {
        Family::Fcitx5 => b"fcitx5",
        Family::Addon => b"addon",
        Family::Interconnect => b"interconnect",
    }
}

/// 扫描行：文件 id + 语料行（ASCII 小写正文）。
#[derive(Clone, Copy)]
pub struct ScanLine {
    pub file_id: u32,
    pub text: &'static [u8],
}

/// ASCII 子串滑窗比对（大小写敏感，语料约定全小写）。
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

/// 单行族分类：返回该行命中的族位图（bit0=fcitx5, bit1=addon, bit2=interconnect）。
pub fn classify(text: &[u8]) -> u8 {
    let mut bits = 0u8;
    if contains(text, family_word(Family::Fcitx5)) {
        bits |= 0x1;
    }
    if contains(text, family_word(Family::Addon)) {
        bits |= 0x2;
    }
    if contains(text, family_word(Family::Interconnect)) {
        bits |= 0x4;
    }
    bits
}

/// 领单主张：（文件 id, 族）二元组。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Claim {
    pub file_id: u32,
    pub family: u8, // 0=fcitx5, 1=addon, 2=interconnect
}

/// 主张种类：新主张 / 前批撞车。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ClaimVerdict {
    Fresh,
    Duplicate,
}

/// 防重留痕账：append-only；同主张重复登记幂等不重记；
/// 与前批账同键位撞车 → 标 Duplicate 但不重复登记。
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

    /// 登记主张：新 → Fresh 入账；重放 → 幂等；前批撞车 → Duplicate 留痕不重记。
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

    pub fn rows(&self) -> &[Claim] {
        &self.rows
    }
}

/// 语料行去重：逐行指纹（FNV-1a）入集，重复行丢弃。
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

/// FNV-1a 64（行指纹）。
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

/// 本批语料夹具：B14 批 20 文件头注摘录（ASCII 小写正文），
/// 关键词族散布覆盖三族；含 1 条与前批账撞车的主张。
pub fn fixture_corpus() -> Vec<ScanLine> {
    vec![
        ScanLine { file_id: 9001, text: b"fcitx5 interconnect bridge surface b14" },
        ScanLine { file_id: 9002, text: b"addon engine regression sentinel window" },
        ScanLine { file_id: 9003, text: b"dictionary crc guard against corrupt load" },
        ScanLine { file_id: 9004, text: b"fcitx5 addon isolation on crash" },
        ScanLine { file_id: 9005, text: b"interconnect vector ledger checksum" },
        ScanLine { file_id: 9006, text: b"privacy zero outbound assertion x5" },
        ScanLine { file_id: 9007, text: b"ui theme data declared out of ai scope" },
        ScanLine { file_id: 9008, text: b"batch seal stamp twenty items frozen" },
        ScanLine { file_id: 9009, text: b"latency frame four segments tsc source" },
        ScanLine { file_id: 9010, text: b"p95 first display gate threshold" },
        ScanLine { file_id: 9011, text: b"fcitx5 three window class sentinel" },
        ScanLine { file_id: 9012, text: b"addon kill drill fallback zero loss" },
        ScanLine { file_id: 9013, text: b"interconnect privacy l3 signed surface" },
        ScanLine { file_id: 9014, text: b"b14 grep dedup claim ledger" },
        ScanLine { file_id: 9015, text: b"fcitx5 dictionary update l3 gate only" },
        ScanLine { file_id: 9016, text: b"addon heartbeat three ticks dead detect" },
        ScanLine { file_id: 9017, text: b"interconnect batch closure anti duplicate" },
        ScanLine { file_id: 9018, text: b"schema migration cfg pin version" },
        ScanLine { file_id: 9019, text: b"fcitx5 addon interconnect combined" },
        ScanLine { file_id: 9020, text: b"plain line without any keyword here" },
    ]
}

/// 前批（B13）既有主张夹具：与 9019 行的三族主张全部撞车。
pub fn prior_claims() -> Vec<Claim> {
    vec![
        Claim { file_id: 9019, family: 0 },
        Claim { file_id: 9019, family: 1 },
        Claim { file_id: 9019, family: 2 },
    ]
}

/// B14 防重 grep 主巡检：批收口留痕全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三族覆盖：去重语料扫描后每族至少 1 命中。
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
    hits += 1;

    // R2 留痕账与命中一一对应：全主张登记后账行数 = 命中数。
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

    // R3 前批撞车：9019 三族主张必须全部标 Duplicate（防重暴露）。
    //   9019 行文本三词全含 → 三主张全撞前批账 → dup_marks 恰 3。
    if led.dup_len() != 3 {
        return Err(E_CLAIM_CLASH);
    }
    hits += 1;

    // R4 幂等重扫：同语料重扫重登 → 账行数与撞车数均不变。
    i = 0;
    while i < hit_claims.len() {
        let v = led.claim(hit_claims[i], &prior);
        match v {
            ClaimVerdict::Duplicate => { /* 撞车重放：幂等 */ }
            ClaimVerdict::Fresh => {
                // 已在账内的主张重放不得再入账——若账长膨胀即非幂等。
                if led.len() > hit_claims.len() {
                    return Err(E_DUP_SCAN);
                }
            }
        }
        i += 1;
    }
    if led.len() + led.dup_len() != hit_claims.len() {
        return Err(E_DUP_SCAN);
    }
    hits += 1;

    // R5 假主张自查：空语料扫描零命中零登记。
    let empty: Vec<ScanLine> = Vec::new();
    let dedup_empty = dedup_lines(&empty);
    if !dedup_empty.is_empty() {
        return Err(E_FALSE_CLAIM);
    }
    let mut led2 = ClaimLedger::new();
    let none: Vec<Claim> = Vec::new();
    let mut j = 0;
    while j < dedup_empty.len() {
        let bits = classify(dedup_empty[j].text);
        let mut f2 = 0u8;
        while f2 < 3 {
            if bits & (1 << f2) != 0 {
                led2.claim(Claim { file_id: dedup_empty[j].file_id, family: f2 }, &none);
            }
            f2 += 1;
        }
        j += 1;
    }
    if led2.len() != 0 || led2.dup_len() != 0 {
        return Err(E_FALSE_CLAIM);
    }
    hits += 1;

    // R6 灌水自查：重复行必须被去重（重放 9001 行 → 去重后语料不含重复）。
    let mut watery: Vec<ScanLine> = Vec::new();
    let mut k = 0;
    while k < corpus.len() {
        watery.push(corpus[k]);
        k += 1;
    }
    watery.push(ScanLine { file_id: 9001, text: b"fcitx5 interconnect bridge surface b14" });
    let dd = dedup_lines(&watery);
    if dd.len() != dedup.len() {
        return Err(E_DEWATER);
    }
    hits += 1;

    // R7 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 7] = [
        E_FAM_MISS,
        E_DUP_SCAN,
        E_CLAIM_CLASH,
        E_FALSE_CLAIM,
        E_LEDGER_BAD,
        E_DEWATER,
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
