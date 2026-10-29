//! UNX-F48547 —— E·丢键账本断电续记（正：账本自身持久化续记零断档）
//! 判据原文：UNX-F48547-J1 丢键账本以持久化检查点续记：
//! 断电前账本已落盘的检查点（序号 + 指纹）在重启后恢复；
//! 续记从检查点序号 +1 严格连续（零断档）；账面序号严格递增、
//! 检查点指纹与账面一致、守恒（恢复条数 + 续记条数 == 总条数）、
//! 指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 序号闭集；R2 检查点落盘；
//! R3 断电重启恢复；R4 续记零断档；R5 指纹一致；
//! R6 账守恒；R7 未落盘不得恢复；R8 fuzz 续记流零错态；
//! R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF320;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_SEQ: u16 = SEG_BASE + 1; // 序号越出闭集
const E_CP: u16 = SEG_BASE + 2; // 检查点落盘异常
const E_RECOVER: u16 = SEG_BASE + 3; // 断电恢复异常
const E_GAP: u16 = SEG_BASE + 4; // 续记断档未检出
const E_FP: u16 = SEG_BASE + 5; // 检查点指纹不一致
const E_CONSERV: u16 = SEG_BASE + 6; // 账不守恒
const E_NOCHECK: u16 = SEG_BASE + 7; // 未落盘被恢复
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 续记流错态
const E_SIG: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

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

/// 一条丢键账目：序号 + 时戳。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerEntry {
    pub seq: u32,
    pub at: u32,
}

/// 检查点：已落盘序号 + 账面指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Checkpoint {
    pub seq: u32,
    pub fp: u64,
}

/// 丢键账本：账面 + 检查点。
pub struct KeyLedger {
    pub entries: Vec<LedgerEntry>,
    pub cp: Option<Checkpoint>,
}

impl KeyLedger {
    pub fn new() -> KeyLedger {
        KeyLedger {
            entries: vec![],
            cp: None,
        }
    }

    /// 记一条：序号必须严格 +1（首条序号 1）。
    pub fn append(&mut self, e: LedgerEntry) -> Result<(), u16> {
        let expect = self.entries.len() as u32 + 1;
        if e.seq != expect {
            return Err(E_SEQ);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 落盘检查点：以当前账面末序号 + 指纹固化。
    pub fn flush(&mut self) -> Result<u32, u16> {
        if self.entries.is_empty() {
            return Err(E_CP);
        }
        let seq = self.entries.len() as u32;
        let fp = ledger_fp(self);
        self.cp = Some(Checkpoint { seq, fp });
        Ok(seq)
    }

    /// 断电重启恢复：账本重建至检查点序号，检查点指纹必须与重建账面一致；
    /// 未落盘账本不得恢复。
    pub fn recover(&mut self, disk: &[LedgerEntry]) -> Result<u32, u16> {
        let cp = match self.cp {
            Some(c) => c,
            None => return Err(E_NOCHECK),
        };
        if disk.len() as u32 != cp.seq {
            return Err(E_RECOVER);
        }
        let mut rebuilt = KeyLedger::new();
        let mut i = 0usize;
        while i < disk.len() {
            rebuilt.append(disk[i])?;
            i += 1;
        }
        if ledger_fp(&rebuilt) != cp.fp {
            return Err(E_FP);
        }
        self.entries = rebuilt.entries;
        Ok(cp.seq)
    }

    /// 续记零断档：恢复后下一序号 == 检查点序号 + 1。
    pub fn next_seq(&self) -> u32 {
        self.entries.len() as u32 + 1
    }

    /// 断档检出：账面序号必须严格 +1，任何跳号返回断档位置。
    pub fn gap_at(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].seq != i as u32 + 1 {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 账守恒：恢复条数 + 续记条数 == 总条数。
    pub fn conserved(&self, recovered: u32, continued: u32) -> bool {
        self.entries.len() as u32 == recovered + continued
    }
}

/// 账面指纹：全部条目 (seq, at) 串联 FNV。
pub fn ledger_fp(lg: &KeyLedger) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < lg.entries.len() {
        buf.extend_from_slice(&lg.entries[i].seq.to_le_bytes());
        buf.extend_from_slice(&lg.entries[i].at.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 序号闭集：首条必须 seq=1，跳号拒收，码恰为 E_SEQ。
    let mut lg = KeyLedger::new();
    match lg.append(LedgerEntry { seq: 2, at: 0 }) {
        Err(E_SEQ) => hits += 1,
        _ => return Err(E_SEQ),
    }

    // R2 检查点落盘：记 16 条后 flush，检查点 seq == 16。
    let mut t = 0u32;
    while t < 16 {
        lg.append(LedgerEntry { seq: t + 1, at: t * 10 })?;
        t += 1;
    }
    let cp_seq = lg.flush()?;
    if cp_seq != 16 || lg.cp.is_none() {
        return Err(E_CP);
    }
    hits += 1;

    // R3 断电重启恢复：以落盘前 16 条磁盘账恢复，条数一致、指纹一致。
    let disk = lg.entries.clone();
    let mut lg2 = KeyLedger::new();
    // 模拟重启：新账本携带旧检查点。
    lg2.cp = lg.cp;
    let rec = lg2.recover(&disk)?;
    if rec != 16 || lg2.entries.len() != 16 {
        return Err(E_RECOVER);
    }
    hits += 1;

    // R4 续记零断档：恢复后继续记 8 条，全程 gap_at 为 None。
    let mut t = 0u32;
    while t < 8 {
        lg2.append(LedgerEntry {
            seq: lg2.next_seq(),
            at: 160 + t * 10,
        })?;
        t += 1;
    }
    if lg2.gap_at().is_some() || lg2.next_seq() != 25 {
        return Err(E_GAP);
    }
    hits += 1;

    // R5 指纹一致：恢复账指纹 == 检查点指纹。
    let cp = lg2.cp;
    match cp {
        Some(c) => {
            let mut head = KeyLedger::new();
            let mut i = 0usize;
            while i < c.seq as usize {
                head.append(disk[i])?;
                i += 1;
            }
            if ledger_fp(&head) != c.fp {
                return Err(E_FP);
            }
        }
        None => return Err(E_FP),
    }
    hits += 1;

    // R6 账守恒：16（恢复）+ 8（续记）== 24。
    if !lg2.conserved(16, 8) {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R7 未落盘不得恢复：无检查点账本恢复被拒，码恰为 E_NOCHECK。
    let mut lg3 = KeyLedger::new();
    lg3.append(LedgerEntry { seq: 1, at: 0 })?;
    let disk3 = lg3.entries.clone();
    match lg3.recover(&disk3) {
        Err(E_NOCHECK) => hits += 1,
        _ => return Err(E_NOCHECK),
    }

    // R8 fuzz 续记流零错态：LCG 驱动 5 轮（落盘 n 条 → 恢复 → 续记 m 条），
    // 每轮零断档、守恒、指纹一致。
    let mut lcg = Lcg::new(0x48547);
    let mut round = 0u32;
    while round < 5 {
        let n = 4 + lcg.below(16);
        let mut src = KeyLedger::new();
        let mut t = 0u32;
        while t < n {
            src.append(LedgerEntry { seq: src.next_seq(), at: t })?;
            t += 1;
        }
        src.flush()?;
        let disk = src.entries.clone();
        let mut rec = KeyLedger::new();
        rec.cp = src.cp;
        rec.recover(&disk)?;
        let m = 1 + lcg.below(8);
        let mut t = 0u32;
        while t < m {
            rec.append(LedgerEntry {
                seq: rec.next_seq(),
                at: 1000 + t,
            })?;
            t += 1;
        }
        if rec.gap_at().is_some() || !rec.conserved(n, m) {
            return Err(E_FUZZ);
        }
        round += 1;
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = ledger_fp(&lg2);
    let f2 = ledger_fp(&lg2);
    if f1 != f2 || f1 == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    // R10 呈现行齐全：n=/cp=/next= 三段前缀在位。
    let line = ledger_line(&lg2);
    if !has_kw(line.as_bytes(), b"n=")
        || !has_kw(line.as_bytes(), b"cp=")
        || !has_kw(line.as_bytes(), b"next=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 呈现行：n=<条数> cp=<检查点序号> next=<下一序号>。
pub fn ledger_line(lg: &KeyLedger) -> String {
    let cp = match lg.cp {
        Some(c) => c.seq,
        None => 0,
    };
    let mut s = String::new();
    s.push_str("n=");
    s.push_str(&(lg.entries.len() as u32).to_string());
    s.push_str(" cp=");
    s.push_str(&cp.to_string());
    s.push_str(" next=");
    s.push_str(&lg.next_seq().to_string());
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
