//! UNX-F48500 —— E25 批收官印（20 条冻结/累计 150,000 行）
//! 判据原文：UNX-F48500-J1 E25 批收官以冻结印结算：
//! 收官账恰 20 条冻结条目、累计行数恰 150,000 行；条目一旦入印
//! 只读（冻结），补录/改写必须被拒；条目行数账守恒、收官指纹
//! 与呈现行可复核、未满额不得出印。
//! 判据落地（drill 场景）：R1 条目闭集 20 条；R2 累计行数恰 150,000；
//! R3 冻结后补录拒收；R4 未满额拒印；R5 行数账守恒；
//! R6 指纹区分扰动；R7 零条目拒印；R8 fuzz 入账流零错态；
//! R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE920;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_SEAL: u16 = SEG_BASE + 1; // 条目数非 20
const E_ROWS: u16 = SEG_BASE + 2; // 累计行数非 150,000
const E_WRITABLE: u16 = SEG_BASE + 3; // 冻结后仍可补录
const E_EARLY: u16 = SEG_BASE + 4; // 未满额出印
const E_CONSERV: u16 = SEG_BASE + 5; // 行数账不守恒
const E_DISTURB: u16 = SEG_BASE + 6; // 指纹未区分扰动
const E_EMPTY: u16 = SEG_BASE + 7; // 零条目出印
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 入账流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 收官条目数。
pub const SEAL_N: u32 = 20;
/// 收官累计行数下限。
pub const SEAL_ROWS: u32 = 150_000;

/// 一条收官条目：模块名 + 行数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealEntry {
    pub code: u32,
    pub rows: u32,
}

/// 收官印：条目账 + 冻结态 + 累计行数。
pub struct SealLedger {
    pub entries: Vec<SealEntry>,
    pub frozen: bool,
    pub total_rows: u32,
}

impl SealLedger {
    pub fn new() -> SealLedger {
        SealLedger {
            entries: vec![],
            frozen: false,
            total_rows: 0,
        }
    }

    /// 补录一条：冻结后拒收；行数为 0 拒收。
    pub fn add(&mut self, e: SealEntry) -> Result<(), u16> {
        if self.frozen {
            return Err(E_WRITABLE);
        }
        if e.rows == 0 {
            return Err(E_CONSERV);
        }
        self.entries.push(e);
        self.total_rows += e.rows;
        Ok(())
    }

    /// 出印条件：条目数 == SEAL_N 且累计行数 ≥ SEAL_ROWS。
    pub fn ready(&self) -> bool {
        self.entries.len() as u32 == SEAL_N && self.total_rows >= SEAL_ROWS
    }

    /// 出印：置冻结；未满足条件拒印。
    pub fn seal(&mut self) -> Result<u32, u16> {
        if self.frozen {
            return Err(E_WRITABLE);
        }
        if self.entries.is_empty() {
            return Err(E_EMPTY);
        }
        if (self.entries.len() as u32) < SEAL_N {
            return Err(E_EARLY);
        }
        if self.total_rows < SEAL_ROWS {
            return Err(E_ROWS);
        }
        self.frozen = true;
        Ok(self.total_rows)
    }

    /// 行数账守恒：total_rows == 各条目行数和。
    pub fn conserved(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            sum += self.entries[i].rows;
            i += 1;
        }
        sum == self.total_rows
    }
}

/// 收官指纹：条目与累计行数串联 FNV。
pub fn seal_fp(lg: &SealLedger) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.push(lg.frozen as u8);
    buf.extend_from_slice(&lg.total_rows.to_le_bytes());
    let mut i = 0usize;
    while i < lg.entries.len() {
        buf.extend_from_slice(&lg.entries[i].code.to_le_bytes());
        buf.extend_from_slice(&lg.entries[i].rows.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 条目闭集 20 条：满额 20 条入账成功。
    let mut lg = SealLedger::new();
    let mut t = 0u32;
    while t < SEAL_N {
        lg.add(SealEntry {
            code: 48480 + t,
            rows: 7_500,
        })?;
        t += 1;
    }
    if lg.entries.len() as u32 != SEAL_N {
        return Err(E_SEAL);
    }
    hits += 1;

    // R2 累计行数恰 150,000：20 × 7,500 == 150,000，出印成功。
    if lg.total_rows != SEAL_ROWS {
        return Err(E_ROWS);
    }
    let total = lg.seal()?;
    if total != SEAL_ROWS || !lg.frozen {
        return Err(E_ROWS);
    }
    hits += 1;

    // R3 冻结后补录拒收：再 add 一条被拒，账目不变，码恰为 E_WRITABLE。
    let n_before = lg.entries.len();
    match lg.add(SealEntry { code: 99999, rows: 100 }) {
        Err(E_WRITABLE) => hits += 1,
        _ => return Err(E_WRITABLE),
    }
    if lg.entries.len() != n_before {
        return Err(E_WRITABLE);
    }

    // R4 未满额拒印：19 条出印被拒，码恰为 E_EARLY。
    let mut lg2 = SealLedger::new();
    let mut t = 0u32;
    while t < SEAL_N - 1 {
        lg2.add(SealEntry {
            code: 48480 + t,
            rows: 8_000,
        })?;
        t += 1;
    }
    match lg2.seal() {
        Err(E_EARLY) => hits += 1,
        _ => return Err(E_EARLY),
    }

    // R5 行数账守恒：满额账 total_rows == 条目和。
    let mut lg3 = SealLedger::new();
    let mut t = 0u32;
    while t < SEAL_N {
        lg3.add(SealEntry {
            code: 48480 + t,
            rows: 7_500 + t,
        })?;
        t += 1;
    }
    if !lg3.conserved() {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 指纹区分扰动：任一条目行数翻转 → 指纹必变。
    let f1 = seal_fp(&lg3);
    let mut disturbed = lg3.clone_entries();
    disturbed[0].rows = disturbed[0].rows ^ 1u32;
    let mut buf: Vec<u8> = vec![];
    buf.push(0u8);
    buf.extend_from_slice(&lg3.total_rows.to_le_bytes());
    let mut i = 0usize;
    while i < disturbed.len() {
        buf.extend_from_slice(&disturbed[i].code.to_le_bytes());
        buf.extend_from_slice(&disturbed[i].rows.to_le_bytes());
        i += 1;
    }
    if fnv1a64(&buf) == f1 {
        return Err(E_DISTURB);
    }
    hits += 1;

    // R7 零条目拒印：空账出印被拒，码恰为 E_EMPTY。
    let mut lg4 = SealLedger::new();
    match lg4.seal() {
        Err(E_EMPTY) => hits += 1,
        _ => return Err(E_EMPTY),
    }

    // R8 fuzz 入账流零错态：LCG 生成 20 条随机行数条目（每条 ≥ 7,500），
    // 守恒成立、出印成功、frozen 置位。
    let mut lg5 = SealLedger::new();
    let mut lcg = Lcg::new(0x48500);
    let mut t = 0u32;
    while t < SEAL_N {
        let rows = 7_500 + lcg.below(500);
        lg5.add(SealEntry { code: 48480 + t, rows })?;
        t += 1;
    }
    if !lg5.conserved() {
        return Err(E_FUZZ);
    }
    lg5.seal()?;
    if !lg5.frozen || lg5.total_rows < SEAL_ROWS {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let g1 = seal_fp(&lg5);
    let g2 = seal_fp(&lg5);
    if g1 != g2 || g1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：n=/rows=/frozen= 三段前缀在位。
    let line = seal_line(&lg5);
    if !has_kw(line.as_bytes(), b"n=")
        || !has_kw(line.as_bytes(), b"rows=")
        || !has_kw(line.as_bytes(), b"frozen=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

impl SealLedger {
    /// 条目快照（供扰动复核，避免移动原账）。
    pub fn clone_entries(&self) -> Vec<SealEntry> {
        let mut out: Vec<SealEntry> = vec![];
        let mut i = 0usize;
        while i < self.entries.len() {
            out.push(self.entries[i]);
            i += 1;
        }
        out
    }
}

/// 呈现行：n=<条目数> rows=<累计行数> frozen=<0/1>。
pub fn seal_line(lg: &SealLedger) -> String {
    let mut s = String::new();
    s.push_str("n=");
    s.push_str(&(lg.entries.len() as u32).to_string());
    s.push_str(" rows=");
    s.push_str(&lg.total_rows.to_string());
    s.push_str(" frozen=");
    s.push_str(&(lg.frozen as u32).to_string());
    s
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
