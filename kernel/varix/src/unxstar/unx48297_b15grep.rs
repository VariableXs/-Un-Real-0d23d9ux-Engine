//! unx48297_b15grep — UNX-F48297 B15 批防重 grep（关键词族：latencyledger/msectionclose）（诊断段 0xC600）。
//!
//! 判据原文：
//!   J1 批收口防重留痕；判据锚定 §6.1。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 关键词族闭集：latencyledger（延迟总账面）/ msectionclose（M 段
//!      收口面）两族，族内子关键词前缀命中；族外关键词不进 grep 面。
//!   2. 语料闭集：只扫登记在册的 B15 批条目；未登记/收口后新增条目
//!      一律拒扫——批账边界即扫描边界。
//!   3. 防重口径：收口后结果冻结留痕（两族计数+批指纹），重复收口/
//!      收口后再登记全拒并逐次留痕——同批结果只出一版。
//!   4. 幂等复核：收口前两次独立 grep 计数逐族全等；收口后指纹可复算
//!      且与留痕一致，漂移必红。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC600;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_KW_UNKNOWN: u16 = SEG_BASE + 1; // 族外关键词
const E_SEALED_EDIT: u16 = SEG_BASE + 2;// 收口后改动
const E_FINGER_DRIFT: u16 = SEG_BASE + 3;// 指纹复算不符
const E_GREP_MISS: u16 = SEG_BASE + 4;  // 批内条目不足
const E_LEDGER_BAD: u16 = SEG_BASE + 5; // 留痕账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 6;  // 写回摘要为空

/// 族闭集（B15 两族）。
pub const FAMILY_COUNT: usize = 2;
pub const FAM_LATENCYLEDGER: usize = 0;
pub const FAM_MSECTIONCLOSE: usize = 1;

pub const FAMILY_NAMES: [&str; FAMILY_COUNT] = ["latencyledger", "msectionclose"];

/// 族子关键词（前缀命中）。
pub const SUB_KEYWORDS: [&[&str]; FAMILY_COUNT] = [
    &["latency_", "ll_", "延迟账"],
    &["mclose_", "msec_", "段收口"],
];

/// 关键词 → 族号。
pub fn family_of(kw: &str) -> Option<usize> {
    let mut f = 0usize;
    while f < FAMILY_COUNT {
        let mut i = 0usize;
        while i < SUB_KEYWORDS[f].len() {
            if kw.starts_with(SUB_KEYWORDS[f][i]) {
                return Some(f);
            }
            i += 1;
        }
        f += 1;
    }
    None
}

/// 一条批内条目。
#[derive(Clone, Copy, Debug)]
pub struct GrepEntry {
    pub seq: u32,
    pub tag: &'static str,
    pub registered: bool,
}

/// 一次扫描结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrepResult {
    pub counts: [u32; FAMILY_COUNT],
    pub fingerprint: u32,
}

/// 收口留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealMark {
    pub result: GrepResult,
    pub entry_total: u32,
}

/// B15 批防重 grep 枢纽。
pub struct B15Grep {
    entries: Vec<GrepEntry>,
    pub reject_total: u32,
    pub sealed: bool,
    pub seal_mark: Option<SealMark>,
}

impl B15Grep {
    pub fn new() -> B15Grep {
        B15Grep { entries: Vec::new(), reject_total: 0, sealed: false, seal_mark: None }
    }

    /// 登记批内条目（收口后拒登，族外关键词拒登）。
    pub fn register(&mut self, seq: u32, tag: &'static str) -> Result<(), u16> {
        if self.sealed {
            self.reject_total += 1;
            return Err(E_SEALED_EDIT);
        }
        if family_of(tag).is_none() {
            self.reject_total += 1;
            return Err(E_KW_UNKNOWN);
        }
        self.entries.push(GrepEntry { seq, tag, registered: true });
        Ok(())
    }

    /// 全量扫描（纯函数：不改批账）。
    pub fn grep_all(&self) -> Result<GrepResult, u16> {
        let mut counts: [u32; FAMILY_COUNT] = [0, 0];
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if !e.registered {
                return Err(E_KW_UNKNOWN);
            }
            match family_of(e.tag) {
                Some(f) => counts[f] += 1,
                None => return Err(E_KW_UNKNOWN),
            }
            i += 1;
        }
        let fingerprint = self.fingerprint(&counts);
        Ok(GrepResult { counts, fingerprint })
    }

    /// 批指纹：条目总数 + 两族计数 XOR 折叠。
    pub fn fingerprint(&self, counts: &[u32; FAMILY_COUNT]) -> u32 {
        let mut fp: u32 = 0xC601;
        fp = fp.wrapping_mul(0x9E3779B1) ^ self.entries.len() as u32;
        let mut f = 0usize;
        while f < FAMILY_COUNT {
            fp = fp.wrapping_mul(0x85EBCA6B) ^ counts[f];
            f += 1;
        }
        fp
    }

    /// 批收口：冻结留痕（空批拒收口）。
    pub fn seal(&mut self) -> Result<SealMark, u16> {
        if self.sealed {
            self.reject_total += 1;
            return Err(E_SEALED_EDIT);
        }
        if self.entries.is_empty() {
            return Err(E_GREP_MISS);
        }
        let result = self.grep_all()?;
        let mark = SealMark { result, entry_total: self.entries.len() as u32 };
        self.sealed = true;
        self.seal_mark = Some(mark);
        Ok(mark)
    }

    /// 收口复核：条目数不变 + 指纹复算一致。
    pub fn verify_seal(&self) -> Result<(), u16> {
        let mark = match self.seal_mark {
            Some(m) => m,
            None => return Err(E_LEDGER_BAD),
        };
        if !self.sealed || self.entries.len() as u32 != mark.entry_total {
            return Err(E_SEALED_EDIT);
        }
        let counts = mark.result.counts;
        if self.fingerprint(&counts) != mark.result.fingerprint {
            return Err(E_FINGER_DRIFT);
        }
        Ok(())
    }

    /// ×N 覆盖核验。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if (self.entries.len() as u32) < want {
            return Err(E_GREP_MISS);
        }
        Ok(())
    }

    pub fn entries_len(&self) -> usize {
        self.entries.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || !self.sealed {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（批收口防重留痕），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主流程：登记 20 条（两族各 10）→ 幂等双扫全等 → 收口留痕
    //    → 指纹复核一致。
    let mut g = B15Grep::new();
    let tags_ll: [&str; 10] = [
        "latency_p50", "ll_p95", "latency_p99", "延迟账_r", "ll_mem",
        "latency_boot", "ll_w22", "延迟账_m", "latency_z", "ll_k",
    ];
    let tags_mc: [&str; 10] = [
        "mclose_293", "msec_294", "mclose_295", "段收口_296", "msec_297",
        "mclose_298", "段收口_299", "msec_300", "mclose_301", "段收口_302",
    ];
    let mut seq = 0u32;
    let mut i = 0usize;
    while i < 10 {
        g.register(seq, tags_ll[i])?;
        seq += 1;
        g.register(seq, tags_mc[i])?;
        seq += 1;
        i += 1;
    }
    g.coverage(20)?;
    let r1 = g.grep_all()?;
    let r2 = g.grep_all()?;
    if r1 != r2 {
        return Err(E_LEDGER_BAD);
    }
    if r1.counts[FAM_LATENCYLEDGER] != 10 || r1.counts[FAM_MSECTIONCLOSE] != 10 {
        return Err(E_GREP_MISS);
    }
    let mark = g.seal()?;
    if !g.sealed || g.seal_mark != Some(mark) || mark.entry_total != 20 {
        return Err(E_LEDGER_BAD);
    }
    g.verify_seal()?;
    hits += 1;

    // R2 防重拦截：收口后重复 seal 拒、再登记拒，拒收逐次留痕；
    //    留痕复核仍绿。
    if g.seal().is_ok() {
        return Err(E_SEALED_EDIT);
    }
    if g.register(99, "ll_new").is_ok() {
        return Err(E_SEALED_EDIT);
    }
    if g.reject_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    g.verify_seal()?;
    hits += 1;

    // R3 族判定边界：子关键词前缀命中，族外拒。
    if family_of("latency_x") != Some(FAM_LATENCYLEDGER)
        || family_of("ll_y") != Some(FAM_LATENCYLEDGER)
        || family_of("延迟账_z") != Some(FAM_LATENCYLEDGER)
    {
        return Err(E_KW_UNKNOWN);
    }
    if family_of("mclose_a") != Some(FAM_MSECTIONCLOSE)
        || family_of("msec_b") != Some(FAM_MSECTIONCLOSE)
        || family_of("段收口_c") != Some(FAM_MSECTIONCLOSE)
    {
        return Err(E_KW_UNKNOWN);
    }
    if family_of("unrelated").is_some() || family_of("latenc").is_some() {
        return Err(E_KW_UNKNOWN); // 前缀不匹配不命中
    }
    hits += 1;

    // R4 指纹敏感：同计数复算同值、计数变动指纹必变。
    let mut g2 = B15Grep::new();
    g2.register(0, "ll_a")?;
    let fa = g2.fingerprint(&[1, 0]);
    if fa != g2.fingerprint(&[1, 0]) {
        return Err(E_FINGER_DRIFT);
    }
    if fa == g2.fingerprint(&[2, 0]) {
        return Err(E_FINGER_DRIFT);
    }
    hits += 1;

    // R5 族外关键词拒登：拒收留痕、账零污染。
    let mut g = B15Grep::new();
    if g.register(0, "unrelated").is_ok() {
        return Err(E_KW_UNKNOWN);
    }
    if g.register(1, "latenc").is_ok() {
        return Err(E_KW_UNKNOWN);
    }
    if g.reject_total != 2 || g.entries_len() != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 空批收口拒：零条目批不得收口（防空印）。
    let mut g = B15Grep::new();
    if g.seal().is_ok() {
        return Err(E_GREP_MISS);
    }
    if g.sealed {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 指纹漂移抓拍：伪造留痕指纹与重算不符必红（可红判据）。
    let mut g = B15Grep::new();
    g.register(0, "ll_a")?;
    g.register(1, "mclose_b")?;
    g.seal()?;
    if let Some(m) = g.seal_mark.as_mut() {
        m.result.fingerprint ^= 0xBEEF;
    }
    if g.verify_seal().is_ok() {
        return Err(E_FINGER_DRIFT);
    }
    hits += 1;

    // R8 补封路径与写回（判据 J2）：重置留痕后补收口放行；
    //    空摘要与未收口写回拒，正常批写回放行。
    g.seal_mark = None;
    g.sealed = false;
    g.reject_total = 0;
    g.seal()?;
    if g.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    g.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
