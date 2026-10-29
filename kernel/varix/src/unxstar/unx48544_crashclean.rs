//! UNX-F48544 —— E·崩溃残留清理注入（反：陈旧锁/临时档 ×10 自清零残留）
//! 判据原文：UNX-F48544-J1 反向判据：以 10 批陈旧锁与临时档注入
//! 崩溃残留面，清理器必须逐批自清零残留：锁位全部释放、
//! 临时档全部回收、清零账与注入账逐批对齐（守恒）；
//! 清理后指纹必须与干净态一致；账目可复核、呈现行齐全。
//! 判据落地（drill 场景）：R1 残留类别闭集；R2 注入入账；
//! R3 逐批清零；R4 十批全清验收；R5 清理账守恒；
//! R6 二次清理幂等；R7 未知类别拒收；R8 fuzz 残留流零错态；
//! R9 指纹回到干净态；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF240;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_KIND: u16 = SEG_BASE + 1; // 残留类别越出闭集
const E_ADD: u16 = SEG_BASE + 2; // 注入入账失败
const E_LEFT: u16 = SEG_BASE + 3; // 清理后仍有残留
const E_BATCH: u16 = SEG_BASE + 4; // 批次推进异常
const E_CONSERV: u16 = SEG_BASE + 5; // 清理账不守恒
const E_IDEM: u16 = SEG_BASE + 6; // 二次清理不幂等
const E_UNKNOWN: u16 = SEG_BASE + 7; // 未知类别拒收失败
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 残留流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹未回干净态
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 残留类别闭集：1 陈旧锁 / 2 临时档。
pub const R_STALE_LOCK: u8 = 1;
pub const R_TEMP_FILE: u8 = 2;
pub const KIND_NAMES: [&str; 2] = ["stale_lock", "temp_file"];

/// 注入批次数。
pub const BATCH_N: u32 = 10;

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

/// 残留类别闭集判定。
pub fn known_kind(k: u8) -> bool {
    k == R_STALE_LOCK || k == R_TEMP_FILE
}

/// 单类别计数：注入账内该类残留数。
pub fn count_kind(injected: &[Residue], kind: u8) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < injected.len() {
        if injected[i].kind == kind {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 一枚残留：类别 + 批次号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Residue {
    pub kind: u8,
    pub batch: u32,
}

/// 残留清理器：注入账 + 清零账，逐批自清。
pub struct Cleaner {
    pub injected: Vec<Residue>,
    pub cleaned: u32,
    pub batch_done: u32,
}

impl Cleaner {
    pub fn new() -> Cleaner {
        Cleaner {
            injected: vec![],
            cleaned: 0,
            batch_done: 0,
        }
    }

    /// 注入一枚残留：闭集校验、入账。
    pub fn inject(&mut self, r: Residue) -> Result<(), u16> {
        if !known_kind(r.kind) {
            return Err(E_KIND);
        }
        self.injected.push(r);
        Ok(())
    }

    /// 当前未清残留数。
    pub fn pending(&self) -> u32 {
        self.injected.len() as u32 - self.cleaned
    }

    /// 清理一批：清掉该批次号的全部残留，返回本批清理数。
    pub fn clean_batch(&mut self, batch: u32) -> Result<u32, u16> {
        if batch <= self.batch_done {
            return Err(E_BATCH);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.injected.len() {
            if self.injected[i].batch == batch && self.injected[i].batch > self.batch_done {
                self.cleaned += 1;
                n += 1;
            }
            i += 1;
        }
        self.batch_done = batch;
        Ok(n)
    }

    /// 清零验收：pending == 0 且清理数 == 注入数。
    pub fn all_clear(&self) -> bool {
        self.pending() == 0 && self.cleaned == self.injected.len() as u32
    }

    /// 清理账守恒：cleaned ≤ 注入数；批次数一致。
    pub fn conserved(&self, batches: u32) -> bool {
        self.cleaned <= self.injected.len() as u32 && self.batch_done == batches
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 残留类别闭集：1/2 合法、0/3/255 非法；类名非空互异。
    if !known_kind(R_STALE_LOCK) || !known_kind(R_TEMP_FILE) || known_kind(0) || known_kind(3) {
        return Err(E_KIND);
    }
    if known_kind(255) || KIND_NAMES[0] == KIND_NAMES[1] || KIND_NAMES[0].is_empty() {
        return Err(E_KIND);
    }
    hits += 1;

    // R2 注入入账：10 批 × 每批 8 枚混合注入，账面恰 80。
    let mut cl = Cleaner::new();
    let mut lcg = Lcg::new(0x48544);
    let mut batch = 1u32;
    while batch <= BATCH_N {
        let mut t = 0u32;
        while t < 8 {
            let kind = if lcg.below(2) == 0 { R_STALE_LOCK } else { R_TEMP_FILE };
            cl.inject(Residue { kind, batch })?;
            t += 1;
        }
        batch += 1;
    }
    if cl.injected.len() as u32 != 80 {
        return Err(E_ADD);
    }
    hits += 1;

    // R2b 分类入账：锁/临时档两类计数之和恰为注入总数，且各类非零。
    let lock_n = count_kind(&cl.injected, R_STALE_LOCK);
    let temp_n = count_kind(&cl.injected, R_TEMP_FILE);
    if lock_n + temp_n != 80 || lock_n == 0 || temp_n == 0 {
        return Err(E_ADD);
    }
    hits += 1;

    // R3 逐批清零：每批清 8 枚，清理数逐批累计。
    let mut batch = 1u32;
    let mut total_cleaned = 0u32;
    while batch <= BATCH_N {
        let n = cl.clean_batch(batch)?;
        if n != 8 {
            return Err(E_LEFT);
        }
        total_cleaned += n;
        batch += 1;
    }
    if total_cleaned != 80 || cl.cleaned != 80 {
        return Err(E_LEFT);
    }
    hits += 1;

    // R4 十批全清验收：pending == 0、all_clear 为真。
    if !cl.all_clear() || cl.pending() != 0 {
        return Err(E_LEFT);
    }
    hits += 1;

    // R5 清理账守恒：10 批次号对齐。
    if !cl.conserved(BATCH_N) {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 二次清理幂等：对已清批次再清被拒，码恰为 E_BATCH；账不变。
    let cleaned_before = cl.cleaned;
    match cl.clean_batch(1) {
        Err(E_BATCH) => {}
        _ => return Err(E_IDEM),
    }
    if cl.cleaned != cleaned_before {
        return Err(E_IDEM);
    }
    hits += 1;

    // R7 未知类别拒收：kind=9 注入被拒，码恰为 E_KIND，账不增。
    let before = cl.injected.len();
    match cl.inject(Residue { kind: 9, batch: 11 }) {
        Err(E_KIND) => hits += 1,
        _ => return Err(E_UNKNOWN),
    }
    if cl.injected.len() != before {
        return Err(E_UNKNOWN);
    }

    // R8 fuzz 残留流零错态：LCG 驱动 5 批随机数量注入（每批 1..=20 枚）
    // 再逐批清理，全程守恒、终态零残留。
    let mut cl2 = Cleaner::new();
    let mut lcg2 = Lcg::new(0x26E4_00);
    let mut total = 0u32;
    let mut batch = 1u32;
    while batch <= 5 {
        let n = 1 + lcg2.below(20);
        let mut t = 0u32;
        while t < n {
            let kind = (1 + lcg2.below(2)) as u8;
            cl2.inject(Residue { kind, batch })?;
            t += 1;
        }
        total += n;
        batch += 1;
    }
    let mut batch = 1u32;
    while batch <= 5 {
        cl2.clean_batch(batch)?;
        batch += 1;
    }
    if !cl2.all_clear() || cl2.cleaned != total || !cl2.conserved(5) {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R9 指纹回到干净态：清零后指纹与空账指纹一致且非零。
    let empty = Cleaner::new();
    let f_clean = cleaner_fp(&cl);
    let f_empty = cleaner_fp(&empty);
    if f_clean != f_empty || f_clean == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：inj=/cln=/pend= 三段前缀在位。
    let line = cleaner_line(&cl);
    if !has_kw(line.as_bytes(), b"inj=")
        || !has_kw(line.as_bytes(), b"cln=")
        || !has_kw(line.as_bytes(), b"pend=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：注入数 + 清理数 + 批次游标串联。
pub fn cleaner_fp(cl: &Cleaner) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&(cl.injected.len() as u32).to_le_bytes());
    buf.extend_from_slice(&cl.cleaned.to_le_bytes());
    buf.extend_from_slice(&cl.batch_done.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：inj=<注入> cln=<清理> pend=<残留>。
pub fn cleaner_line(cl: &Cleaner) -> String {
    let mut s = String::new();
    s.push_str("inj=");
    s.push_str(&(cl.injected.len() as u32).to_string());
    s.push_str(" cln=");
    s.push_str(&cl.cleaned.to_string());
    s.push_str(" pend=");
    s.push_str(&cl.pending().to_string());
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
