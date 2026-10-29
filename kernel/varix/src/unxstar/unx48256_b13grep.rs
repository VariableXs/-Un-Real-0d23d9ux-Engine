//! unx48256_b13grep — UNX-F48256 B13 批防重 grep（关键词族：shuangpin/opencc/shortphrase）（诊断段 0xC0E0）。
//!
//! 判据原文：
//!   J1 批收口防重留痕；判据锚定 §6.1。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 关键词族闭集：shuangpin（双拼面）/ opencc（简繁面）/ shortphrase
//!      （短 phrase 面）三族，每族带一组子关键词（前缀匹配族内命中）；
//!      族外关键词不进 grep 面（面界清晰）。
//!   2. 语料闭集：grep 只扫登记在册的 B13 批内条目（每条带正文 tag），
//!      命中计数按族聚合；未登记条目拒扫——防把批外改动算进批账。
//!   3. 防重口径：批收口（seal）后 grep 结果冻结留痕（三族计数+批指纹）；
//!      收口后重复 grep/重收口一律拒收并落拒收账——同批结果只出一版。
//!   4. 幂等复核：收口前两次独立 grep 计数逐族全等（纯扫描无副作用）；
//!      收口后指纹可复算且与留痕一致。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC0E0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_KW_UNKNOWN: u16 = SEG_BASE + 1; // 族外关键词
const E_ENTRY_BAD: u16 = SEG_BASE + 2;  // 未登记条目拒扫
const E_SEALED_EDIT: u16 = SEG_BASE + 3;// 收口后再动批账
const E_FINGER_DRIFT: u16 = SEG_BASE + 4;// 指纹复算不符
const E_GREP_MISS: u16 = SEG_BASE + 5;  // 扫描计数不足
const E_LEDGER_BAD: u16 = SEG_BASE + 6; // 留痕账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7;  // 写回摘要为空

/// 族数。
pub const FAMILY_COUNT: usize = 3;
pub const FAM_SHUANGPIN: usize = 0;
pub const FAM_OPENCC: usize = 1;
pub const FAM_SHORTPHRASE: usize = 2;

pub const FAMILY_NAMES: [&str; FAMILY_COUNT] = ["shuangpin", "opencc", "shortphrase"];

/// 族子关键词（前缀匹配）。
pub const SUB_KEYWORDS: [&[&str]; FAMILY_COUNT] = [
    &["sp_", "shuangpin_", "双拼"],
    &["opencc_", "s2t_", "t2s_", "简繁"],
    &["short_", "phrase_", "短语"],
];

/// 关键词 → 族号（闭集判定）。
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

/// 一条批内条目：登记号 + 正文 tag（以关键词前缀开头即命中）。
#[derive(Clone, Copy, Debug)]
pub struct GrepEntry {
    pub seq: u32,
    pub tag: &'static str,
    pub registered: bool,
}

/// 一次扫描结果（三族计数 + 指纹）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrepResult {
    pub counts: [u32; FAMILY_COUNT],
    pub fingerprint: u32,
}

/// 留痕账：收口时的冻结结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealMark {
    pub result: GrepResult,
    pub entry_total: u32,
}

/// 批防重 grep 枢纽。
pub struct B13Grep {
    entries: Vec<GrepEntry>,
    pub scanned_total: u32,
    pub reject_total: u32,
    pub sealed: bool,
    pub seal_mark: Option<SealMark>,
}

impl B13Grep {
    pub fn new() -> B13Grep {
        B13Grep {
            entries: Vec::new(),
            scanned_total: 0,
            reject_total: 0,
            sealed: false,
            seal_mark: None,
        }
    }

    /// 登记批内条目（收口后拒登）。
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

    /// 单条扫描：族命中 +1（未登记条目拒扫）。
    pub fn scan_entry(&self, e: &GrepEntry, counts: &mut [u32; FAMILY_COUNT]) -> Result<(), u16> {
        if !e.registered {
            return Err(E_ENTRY_BAD);
        }
        match family_of(e.tag) {
            Some(f) => {
                counts[f] += 1;
                Ok(())
            }
            None => Err(E_KW_UNKNOWN),
        }
    }

    /// 全量扫描（纯函数：不改批账、不置收口位）。
    pub fn grep_all(&self) -> Result<GrepResult, u16> {
        let mut counts: [u32; FAMILY_COUNT] = [0, 0, 0];
        let mut i = 0usize;
        while i < self.entries.len() {
            self.scan_entry(&self.entries[i], &mut counts)?;
            i += 1;
        }
        let fingerprint = self.fingerprint(&counts);
        Ok(GrepResult { counts, fingerprint })
    }

    /// 批指纹：三族计数 XOR 折叠 + 条目总数掺入。
    pub fn fingerprint(&self, counts: &[u32; FAMILY_COUNT]) -> u32 {
        let mut fp: u32 = 0xC0E1;
        fp = fp.wrapping_mul(0x9E3779B1) ^ self.entries.len() as u32;
        let mut f = 0usize;
        while f < FAMILY_COUNT {
            fp = fp.wrapping_mul(0x85EBCA6B) ^ counts[f];
            f += 1;
        }
        fp
    }

    /// 批收口：冻结留痕（一次定终身）。
    pub fn seal(&mut self) -> Result<SealMark, u16> {
        if self.sealed {
            self.reject_total += 1;
            return Err(E_SEALED_EDIT);
        }
        let result = self.grep_all()?;
        if self.entries.is_empty() {
            return Err(E_GREP_MISS); // 空批不得收口
        }
        let mark = SealMark { result, entry_total: self.entries.len() as u32 };
        self.sealed = true;
        self.seal_mark = Some(mark);
        Ok(mark)
    }

    /// 收口后复核：重算指纹与留痕一致（防重留痕的回放口径）。
    pub fn verify_seal(&self) -> Result<(), u16> {
        let mark = match self.seal_mark {
            Some(m) => m,
            None => return Err(E_LEDGER_BAD),
        };
        if !self.sealed {
            return Err(E_LEDGER_BAD);
        }
        if self.entries.len() as u32 != mark.entry_total {
            return Err(E_SEALED_EDIT); // 收口后条目数变动 = 互斥失守
        }
        let counts: [u32; FAMILY_COUNT] = mark.result.counts;
        if self.fingerprint(&counts) != mark.result.fingerprint {
            return Err(E_FINGER_DRIFT);
        }
        Ok(())
    }

    /// 扫描覆盖核验。
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

    // R1 主流程：登记 30 条（三族各 10）→ 幂等双扫全等 → 收口留痕
    //    → 收口复核指纹一致。
    let mut g = B13Grep::new();
    let tags_sp: [&str; 10] = [
        "sp_vec", "sp_key", "shuangpin_tab", "sp_fuzzy", "sp_perf",
        "双拼_a", "双拼_b", "sp_conc", "shuangpin_x", "sp_z",
    ];
    let tags_oc: [&str; 10] = [
        "opencc_core", "s2t_map", "t2s_map", "opencc_edge", "简繁_x",
        "s2t_y", "t2s_z", "opencc_w", "简繁_v", "s2t_u",
    ];
    let tags_short: [&str; 10] = [
        "short_tab", "phrase_lib", "shortphrase_c", "短语_k", "short_m",
        "phrase_n", "shortphrase_p", "短语_q", "short_r", "phrase_s",
    ];
    let mut seq = 0u32;
    let mut i = 0usize;
    while i < 10 {
        g.register(seq, tags_sp[i])?;
        seq += 1;
        g.register(seq, tags_oc[i])?;
        seq += 1;
        g.register(seq, tags_short[i])?;
        seq += 1;
        i += 1;
    }
    g.coverage(30)?;
    // 幂等双扫。
    let r1 = g.grep_all()?;
    let r2 = g.grep_all()?;
    if r1 != r2 {
        return Err(E_LEDGER_BAD);
    }
    if r1.counts[FAM_SHUANGPIN] != 10
        || r1.counts[FAM_OPENCC] != 10
        || r1.counts[FAM_SHORTPHRASE] != 10
    {
        return Err(E_GREP_MISS);
    }
    // 收口。
    let mark = g.seal()?;
    if !g.sealed || g.seal_mark != Some(mark) {
        return Err(E_LEDGER_BAD);
    }
    if mark.entry_total != 30 {
        return Err(E_LEDGER_BAD);
    }
    g.verify_seal()?;
    hits += 1;

    // R2 防重拦截：收口后重复 seal 拒、再登记拒、拒收账逐次留痕。
    if g.seal().is_ok() {
        return Err(E_SEALED_EDIT);
    }
    if g.register(99, "sp_new").is_ok() {
        return Err(E_SEALED_EDIT);
    }
    if g.reject_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    // 留痕结果未被污染：复核仍绿。
    g.verify_seal()?;
    hits += 1;

    // R3 族判定边界：子关键词前缀命中、族外关键词判 None。
    if family_of("sp_a") != Some(FAM_SHUANGPIN)
        || family_of("shuangpin_z") != Some(FAM_SHUANGPIN)
        || family_of("双拼_q") != Some(FAM_SHUANGPIN)
    {
        return Err(E_KW_UNKNOWN);
    }
    if family_of("opencc_t2s") != Some(FAM_OPENCC)
        || family_of("简繁_w") != Some(FAM_OPENCC)
    {
        return Err(E_KW_UNKNOWN);
    }
    if family_of("shortphrase_q") != Some(FAM_SHORTPHRASE)
        || family_of("短语_zz") != Some(FAM_SHORTPHRASE)
    {
        return Err(E_KW_UNKNOWN);
    }
    if family_of("unrelated").is_some() || family_of("spx").is_some() {
        return Err(E_KW_UNKNOWN); // 前缀不匹配不得命中
    }
    hits += 1;

    // R4 指纹可复算：同计数两次指纹同值；计数变动指纹必变（防漂移）。
    let mut g2 = B13Grep::new();
    g2.register(0, "sp_a")?;
    let fp_a = g2.fingerprint(&[1, 0, 0]);
    let fp_b = g2.fingerprint(&[1, 0, 0]);
    if fp_a != fp_b {
        return Err(E_FINGER_DRIFT);
    }
    let fp_c = g2.fingerprint(&[2, 0, 0]);
    if fp_a == fp_c {
        return Err(E_FINGER_DRIFT);
    }
    hits += 1;

    // R5 族外关键词拒登：登记面闭集外 tag 拒收且拒收留痕。
    let mut g = B13Grep::new();
    if g.register(0, "unrelated").is_ok() {
        return Err(E_KW_UNKNOWN);
    }
    if g.register(1, "sp").is_ok() {
        return Err(E_GREP_MISS); // 恰为前缀 "sp_"？不：tag "sp" 不含下划线，不匹配
    }
    if g.reject_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    if g.entries_len() != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 空批收口拒：零条目批不得收口（防空印）。
    let mut g = B13Grep::new();
    if g.seal().is_ok() {
        return Err(E_GREP_MISS);
    }
    if g.sealed {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 混族聚合：同一族多前缀交叉命中计数正确（12 条三族混排）。
    let mut g = B13Grep::new();
    g.register(0, "opencc_a")?;
    g.register(1, "sp_b")?;
    g.register(2, "s2t_c")?;
    g.register(3, "short_d")?;
    g.register(4, "shuangpin_e")?;
    g.register(5, "短语_f")?;
    g.register(6, "t2s_g")?;
    g.register(7, "phrase_h")?;
    g.register(8, "简繁_i")?;
    g.register(9, "双拼_j")?;
    g.register(10, "short_k")?;
    g.register(11, "opencc_l")?;
    let r = g.grep_all()?;
    if r.counts[FAM_SHUANGPIN] != 3 || r.counts[FAM_OPENCC] != 4 || r.counts[FAM_SHORTPHRASE] != 4 {
        return Err(E_GREP_MISS);
    }
    hits += 1;

    // R8 指纹漂移抓拍与写回（判据 J2）：伪造留痕指纹与重算不符必红；
    //    正常批写回放行。
    let mut g = B13Grep::new();
    g.register(0, "sp_a")?;
    g.register(1, "opencc_b")?;
    g.register(2, "short_c")?;
    g.seal()?;
    if let Some(m) = g.seal_mark.as_mut() {
        m.result.fingerprint ^= 0xBEEF; // 伪造漂移
    }
    if g.verify_seal().is_ok() {
        return Err(E_FINGER_DRIFT);
    }
    // 恢复留痕后写回。
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
