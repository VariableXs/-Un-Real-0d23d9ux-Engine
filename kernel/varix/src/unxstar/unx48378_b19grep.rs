//! unx48378_b19grep — UNX-F48378 B19 批防重 grep（关键词族：explog/rageclick/storyline）（诊断段 0xD020）。
//!
//! 判据原文：
//!   J1 批收口防重留痕；判据锚定 §6.1。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 关键词族封闭：防重扫描只认三个关键词——explog / rageclick /
//!      storyline，全部折成 u64 FNV 指印参与匹配，禁字符串比较；
//!      三枚指印两两互异、恒非零，指印族是本卡唯一匹配口径。
//!   2. 语料 token 化：语料行 = 有序 token 指印向量，行指纹 = 全行
//!      FNV 折叠；命中判据 = 行内存在 token 指印等于关键词指印。
//!   3. 防重留痕：命中账按（关键词序号, 行指纹）二元组记帐，同一
//!      二元组重复命中不重记（幂等）——重复内容行、重复扫描都不得
//!      让账目膨胀，留痕恰等于去重后的命中面。
//!   4. 增量敏感：新出现的唯一命中必须入账（漏记=废账），而重复命中
//!      必须被拒（重记=废账）——防重器双向都要灵。
//!   5. 扫描确定性：同语料重扫命中数与账面逐位一致，零真随机。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xD020;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_KW_BAD: u16 = SEG_BASE + 1;    // 关键词指印族违规（重/零）
const E_CORPUS_BAD: u16 = SEG_BASE + 2; // 语料形违规（空行/超界）
const E_SCAN_DRIFT: u16 = SEG_BASE + 3; // 重扫命中数漂移
const E_DUP_GROW: u16 = SEG_BASE + 4;   // 重复命中重记（防重失效）
const E_FRESH_MISS: u16 = SEG_BASE + 5; // 新唯一命中漏记
const E_LINE_FP_BAD: u16 = SEG_BASE + 6; // 行指纹碰撞/为零
const E_LEDGER_BAD: u16 = SEG_BASE + 7; // 命中账面与去重命中面不符
const E_CODE_RANGE: u16 = SEG_BASE + 8; // 诊断码越段

/// 关键词族规模。
pub const KW_N: usize = 3;

/// 语料行数（夹具基线）。
pub const LINES: usize = 24;

/// FNV-1a 字节折叠。
pub fn fnv_bytes(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 词 → u64 指印。
pub fn kw_hash(w: &[u8]) -> u64 {
    fnv_bytes(0xCBF2_9CE4_8422_2325, w)
}

/// 关键词族指印：explog / rageclick / storyline。
pub fn kw_family() -> [u64; 3] {
    [kw_hash(b"explog"), kw_hash(b"rageclick"), kw_hash(b"storyline")]
}

/// FNV-1a 全折叠（行指纹）。
pub fn fnv_stream(tokens: &[u64]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < tokens.len() {
        h ^= tokens[i];
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h | (1 << 63) // 行指纹恒非零
}

/// 行内是否含指印 t。
pub fn line_contains(line: &[u64], t: u64) -> bool {
    let mut i = 0;
    while i < line.len() {
        if line[i] == t {
            return true;
        }
        i += 1;
    }
    false
}

/// 语料夹具：24 行 × 6 token；行 l 的 token 值 = l*16+t，
/// 行 l%4==0 在 t=2 处放关键词 kw[l%3]；行 19 复制行 4 内容（重复行探针）。
pub fn corpus() -> Vec<Vec<u64>> {
    let kws = kw_family();
    let mut lines: Vec<Vec<u64>> = Vec::new();
    let mut l = 0usize;
    while l < LINES {
        let mut row: Vec<u64> = Vec::new();
        let mut t = 0usize;
        while t < 6 {
            if l % 4 == 0 && t == 2 {
                row.push(kws[l % 3]);
            } else {
                let v = (l as u64) * 16 + t as u64;
                row.push(fnv_bytes(0x9E37_79B9_7F4A_7C15, &[v as u8, (v >> 8) as u8, (v >> 16) as u8]));
            }
            t += 1;
        }
        if l == 19 {
            // 复制行 4 全部 token（行 4 含 rageclick）→ 重复内容行探针。
            row.clear();
            let mut t = 0usize;
            while t < 6 {
                row.push(lines[4][t]);
                t += 1;
            }
        }
        lines.push(row);
        l += 1;
    }
    lines
}

/// 防重命中账：（关键词序号, 行指纹）二元组，幂等不重记。
pub struct GrepLedger {
    entries: Vec<(u8, u64)>,
}

impl GrepLedger {
    pub fn new() -> Self {
        GrepLedger { entries: Vec::new() }
    }

    /// 返回 true = 新留痕；false = 重复命中被防重吸收。
    pub fn record(&mut self, kw: u8, line_fp: u64) -> bool {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].0 == kw && self.entries[i].1 == line_fp {
                return false;
            }
            i += 1;
        }
        self.entries.push((kw, line_fp));
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn entries(&self) -> &[(u8, u64)] {
        &self.entries
    }
}

/// 扫描：逐行逐关键词匹配，命中计数（含重复）+ 留账（去重）。
pub fn scan(corpus: &[Vec<u64>], kws: &[u64; 3], led: &mut GrepLedger) -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut l = 0usize;
    while l < corpus.len() {
        if corpus[l].is_empty() {
            return Err(E_CORPUS_BAD);
        }
        let fp = fnv_stream(&corpus[l]);
        let mut k = 0usize;
        while k < KW_N {
            if line_contains(&corpus[l], kws[k]) {
                hits += 1;
                led.record(k as u8, fp);
            }
            k += 1;
        }
        l += 1;
    }
    Ok(hits)
}

/// 自例程判据：逐条落实 J1（批收口防重留痕），返回命中组数。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let kws = kw_family();

    // R1 指印族规约：三枚指印两两互异、恒非零。
    if kws[0] == 0 || kws[1] == 0 || kws[2] == 0 {
        return Err(E_KW_BAD);
    }
    if kws[0] == kws[1] || kws[0] == kws[2] || kws[1] == kws[2] {
        return Err(E_KW_BAD);
    }
    hits += 1;

    // R2 语料形：24 行全非空；行指纹两两可辨（重点行不碰撞）且非零。
    let corpus = corpus();
    if corpus.len() != LINES {
        return Err(E_CORPUS_BAD);
    }
    let mut l = 0usize;
    while l < LINES {
        if corpus[l].len() != 6 {
            return Err(E_CORPUS_BAD);
        }
        l += 1;
    }
    let fp4 = fnv_stream(&corpus[4]);
    let fp19 = fnv_stream(&corpus[19]);
    if fp4 == 0 || fp19 != fp4 {
        return Err(E_LINE_FP_BAD); // 行 19 必须与行 4 同纹（重复行探针前置）
    }
    let fp3 = fnv_stream(&corpus[3]);
    if fp3 == fp4 {
        return Err(E_LINE_FP_BAD);
    }
    hits += 1;

    // R3 首扫留痕：基线语料关键词行 = 0,4,8,12,16,20（行 19 与 4 同纹
    //    同命中），去重留痕恰 6 条；命中账与去重命中面全等。
    let mut led = GrepLedger::new();
    let h1 = scan(&corpus, &kws, &mut led)?;
    if h1 != 7 {
        return Err(E_SCAN_DRIFT); // 6 个关键词行 + 1 个重复行命中 = 7
    }
    if led.len() != 6 {
        return Err(E_LEDGER_BAD);
    }
    // 留痕三元组逐一核对：kw 序号必须落在对应行。
    let mut e = 0usize;
    while e < led.entries().len() {
        let (kw, fp) = led.entries()[e];
        if kw >= 3 {
            return Err(E_KW_BAD);
        }
        // 该指纹必须真是含关键词 kw 的行。
        let mut found = false;
        let mut l = 0usize;
        while l < corpus.len() {
            if fnv_stream(&corpus[l]) == fp && line_contains(&corpus[l], kws[kw as usize]) {
                found = true;
                break;
            }
            l += 1;
        }
    if !found {
        return Err(E_LEDGER_BAD);
    }
        e += 1;
    }
    // 基线快照：把首扫去重留痕面定格，供 R7 复现对拍（后续增量不动它）。
    let mut baseline: Vec<(u8, u64)> = Vec::new();
    let mut e = 0usize;
    while e < led.entries().len() {
        baseline.push(led.entries()[e]);
        e += 1;
    }
    hits += 1;

    // R4 重扫防重：同语料再扫，命中数一致、账面零膨胀。
    let h2 = scan(&corpus, &kws, &mut led)?;
    if h2 != h1 || led.len() != 6 {
        return Err(E_DUP_GROW);
    }
    hits += 1;

    // R5 重复行注入：再贴一份行 4 内容 → 命中 +1 但留痕零增长。
    let mut corpus2: Vec<Vec<u64>> = Vec::new();
    let mut l = 0usize;
    while l < corpus.len() {
        corpus2.push(corpus[l].clone());
        l += 1;
    }
    corpus2.push(corpus[4].clone());
    let h3 = scan(&corpus2, &kws, &mut led)?;
    if h3 != h1 + 1 {
        return Err(E_SCAN_DRIFT);
    }
    if led.len() != 6 {
        return Err(E_DUP_GROW);
    }
    hits += 1;

    // R6 新唯一命中：追加一行含 rageclick 的新内容 → 留痕恰 +1。
    let mut fresh: Vec<u64> = Vec::new();
    let mut t = 0usize;
    while t < 6 {
        let v = 9_999 + t as u64;
        fresh.push(fnv_bytes(0x9E37_79B9_7F4A_7C15, &[v as u8, (v >> 8) as u8, (v >> 16) as u8]));
        t += 1;
    }
    fresh[1] = kws[1]; // rageclick
    corpus2.push(fresh);
    let h4 = scan(&corpus2, &kws, &mut led)?;
    if h4 != h3 + 1 {
        return Err(E_FRESH_MISS);
    }
    if led.len() != 7 {
        return Err(E_FRESH_MISS);
    }
    hits += 1;

    // R7 扫描确定性：全新账重扫基线语料，命中数与留痕面逐位复现。
    let mut led2 = GrepLedger::new();
    let h5 = scan(&corpus, &kws, &mut led2)?;
    if h5 != 7 || led2.len() != 6 {
        return Err(E_SCAN_DRIFT);
    }
    let mut e = 0usize;
    while e < led2.entries().len() {
        if led2.entries()[e] != baseline[e] {
            return Err(E_SCAN_DRIFT);
        }
        e += 1;
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_KW_BAD,
        E_CORPUS_BAD,
        E_SCAN_DRIFT,
        E_DUP_GROW,
        E_FRESH_MISS,
        E_LINE_FP_BAD,
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
