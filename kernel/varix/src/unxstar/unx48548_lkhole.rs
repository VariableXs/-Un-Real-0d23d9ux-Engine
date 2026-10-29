//! UNX-F48548 —— E·账本空洞注入（反：伪造断档 ×10 检出告警）
//! 判据原文：UNX-F48548-J1 反向判据：以 10 组伪造断档（跳号/重号/
//! 回退序号）注入丢键账本，完整性核收面必须逐组检出并告警：
//! 每组断档位置可定位、告警计数 == 注入组数；合法对照组全数
//! 通过零误报；检出后不得续记落账；账目守恒、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 序号完整性单点；R2 跳号检出；
//! R3 重号检出；R4 回退检出；R5 十组全检出告警；
//! R6 合法对照组零误报；R7 检出后拦截续记；R8 fuzz 注入流零错态；
//! R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF340;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_SEQ: u16 = SEG_BASE + 1; // 序号完整性异常
const E_GAP: u16 = SEG_BASE + 2; // 跳号未检出
const E_DUP: u16 = SEG_BASE + 3; // 重号未检出
const E_BACK: u16 = SEG_BASE + 4; // 回退未检出
const E_ALARM: u16 = SEG_BASE + 5; // 告警计数不守恒
const E_LEGAL: u16 = SEG_BASE + 6; // 合法账被误报
const E_APPEND: u16 = SEG_BASE + 7; // 检出后仍续记
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 注入流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 伪造断档注入组数。
pub const HOLE_N: u32 = 10;

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

/// 一条账目：序号 + 时戳。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerEntry {
    pub seq: u32,
    pub at: u32,
}

/// 完整性缺陷类别：1 跳号 / 2 重号 / 3 回退。
pub const D_GAP: u8 = 1;
pub const D_DUP: u8 = 2;
pub const D_BACK: u8 = 3;

/// 完整性核收器：账面 + 告警账。
pub struct HoleAudit {
    pub entries: Vec<LedgerEntry>,
    pub alarms: u32,
    pub sealed: bool,
}

impl HoleAudit {
    pub fn new() -> HoleAudit {
        HoleAudit {
            entries: vec![],
            alarms: 0,
            sealed: false,
        }
    }

    /// 完整性单点：账面序号严格 +1 则健全，否则返回首个缺陷位置与类别。
    pub fn defect_at(&self) -> Option<(usize, u8)> {
        let mut i = 1usize;
        while i < self.entries.len() {
            let prev = self.entries[i - 1].seq;
            let cur = self.entries[i].seq;
            if cur == prev {
                return Some((i, D_DUP));
            }
            if cur < prev {
                return Some((i, D_BACK));
            }
            if cur > prev + 1 {
                return Some((i, D_GAP));
            }
            i += 1;
        }
        None
    }

    /// 注入一条：先按期望序收账，完整性缺陷检出即告警并封账。
    /// 返回 Ok(true) 表示合法入账，Ok(false) 表示检出缺陷。
    pub fn inject(&mut self, e: LedgerEntry) -> Result<bool, u16> {
        if self.sealed {
            return Err(E_APPEND);
        }
        self.entries.push(e);
        match self.defect_at() {
            Some((_, _)) => {
                self.alarms += 1;
                self.sealed = true;
                Ok(false)
            }
            None => Ok(true),
        }
    }

    /// 静态核收一组磁盘账：返回是否检出缺陷（不改动本账）。
    pub fn scan_disk(entries: &[LedgerEntry]) -> bool {
        let mut i = 0usize;
        while i < entries.len() {
            if entries[i].seq != i as u32 + 1 {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 告警账守恒：sealed 与 alarms 联动。
    pub fn conserved(&self) -> bool {
        self.sealed == (self.alarms >= 1) && self.alarms <= 1
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 序号完整性单点：合法账 defect_at 为 None；单条账恒健全。
    let mut au = HoleAudit::new();
    let mut t = 0u32;
    while t < 8 {
        au.inject(LedgerEntry { seq: t + 1, at: t })?;
        t += 1;
    }
    if au.defect_at().is_some() || au.alarms != 0 {
        return Err(E_SEQ);
    }
    hits += 1;

    // R2 跳号检出：…3→5 检出且类别恰 D_GAP。
    let mut au2 = HoleAudit::new();
    let mut t = 0u32;
    while t < 3 {
        au2.inject(LedgerEntry { seq: t + 1, at: t })?;
        t += 1;
    }
    let r = au2.inject(LedgerEntry { seq: 5, at: 5 })?;
    if r {
        return Err(E_GAP);
    }
    match au2.defect_at() {
        Some((_, D_GAP)) => hits += 1,
        _ => return Err(E_GAP),
    }

    // R3 重号检出：…3→3 检出且类别恰 D_DUP。
    let mut au3 = HoleAudit::new();
    let mut t = 0u32;
    while t < 3 {
        au3.inject(LedgerEntry { seq: t + 1, at: t })?;
        t += 1;
    }
    let r = au3.inject(LedgerEntry { seq: 3, at: 5 })?;
    if r {
        return Err(E_DUP);
    }
    match au3.defect_at() {
        Some((_, D_DUP)) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R4 回退检出：…5→3 检出且类别恰 D_BACK。
    let mut au4 = HoleAudit::new();
    let mut t = 0u32;
    while t < 5 {
        au4.inject(LedgerEntry { seq: t + 1, at: t })?;
        t += 1;
    }
    let r = au4.inject(LedgerEntry { seq: 3, at: 9 })?;
    if r {
        return Err(E_BACK);
    }
    match au4.defect_at() {
        Some((_, D_BACK)) => hits += 1,
        _ => return Err(E_BACK),
    }

    // R5 十组全检出告警：10 组磁盘级伪造断档逐组 scan_disk 检出。
    let mut lcg = Lcg::new(0x48548);
    let mut detected = 0u32;
    let mut g = 0u32;
    while g < HOLE_N {
        let mut disk: Vec<LedgerEntry> = vec![];
        let mut t = 0u32;
        let n = 4 + lcg.below(6);
        while t < n {
            disk.push(LedgerEntry { seq: t + 1, at: t });
            t += 1;
        }
        // 伪造断档：尾条序号 +2（跳号）。
        let last = disk.len() - 1;
        disk[last].seq = disk[last].seq + 2;
        if HoleAudit::scan_disk(&disk) {
            detected += 1;
        }
        g += 1;
    }
    if detected != HOLE_N {
        return Err(E_ALARM);
    }
    hits += 1;

    // R6 合法对照组零误报：10 组合法磁盘账全数通过。
    let mut g = 0u32;
    while g < HOLE_N {
        let mut disk: Vec<LedgerEntry> = vec![];
        let mut t = 0u32;
        let n = 3 + lcg.below(6);
        while t < n {
            disk.push(LedgerEntry { seq: t + 1, at: t });
            t += 1;
        }
        if HoleAudit::scan_disk(&disk) {
            return Err(E_LEGAL);
        }
        g += 1;
    }
    hits += 1;

    // R7 检出后拦截续记：封账后再注入被拒，码恰为 E_APPEND，账与告警不变。
    let n_before = au2.entries.len();
    let a_before = au2.alarms;
    match au2.inject(LedgerEntry { seq: 6, at: 9 }) {
        Err(E_APPEND) => hits += 1,
        _ => return Err(E_APPEND),
    }
    if au2.entries.len() != n_before || au2.alarms != a_before {
        return Err(E_APPEND);
    }

    // R8 fuzz 注入流零错态：LCG 驱动 64 组随机账（合法/缺陷混合），
    // 缺陷组必检出、合法组必通过、告警与封账联动守恒。
    let mut lcg2 = Lcg::new(0x26E5_00);
    let mut t = 0u32;
    let mut bad_n = 0u32;
    while t < 64 {
        let mut h = HoleAudit::new();
        let len = 3 + lcg2.below(8);
        let corrupt = lcg2.below(2) == 1;
        let cut = 1 + lcg2.below(len - 1);
        let mut i = 0u32;
        while i < len {
            let seq = if corrupt && i == cut { i + 3 } else { i + 1 };
            h.inject(LedgerEntry { seq, at: i })?;
            i += 1;
        }
        if corrupt {
            bad_n += 1;
            if h.alarms != 1 || !h.sealed {
                return Err(E_FUZZ);
            }
        } else if h.alarms != 0 || h.sealed {
            return Err(E_LEGAL);
        }
        if !h.conserved() {
            return Err(E_FUZZ);
        }
        t += 1;
    }
    if bad_n == 0 {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = audit_fp(&au);
    let f2 = audit_fp(&au);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：n=/alarm=/seal= 三段前缀在位。
    let line = audit_line(&au);
    if !has_kw(line.as_bytes(), b"n=")
        || !has_kw(line.as_bytes(), b"alarm=")
        || !has_kw(line.as_bytes(), b"seal=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：告警数 + 封账位 + 全条目串联。
pub fn audit_fp(h: &HoleAudit) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&h.alarms.to_le_bytes());
    buf.push(h.sealed as u8);
    let mut i = 0usize;
    while i < h.entries.len() {
        buf.extend_from_slice(&h.entries[i].seq.to_le_bytes());
        buf.extend_from_slice(&h.entries[i].at.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：n=<条数> alarm=<告警> seal=<0/1>。
pub fn audit_line(h: &HoleAudit) -> String {
    let mut s = String::new();
    s.push_str("n=");
    s.push_str(&(h.entries.len() as u32).to_string());
    s.push_str(" alarm=");
    s.push_str(&h.alarms.to_string());
    s.push_str(" seal=");
    s.push_str(&(h.sealed as u32).to_string());
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
