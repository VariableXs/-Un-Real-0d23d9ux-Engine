//! UNX-F48502 —— E·回注风暴注入（反：10^4 键/秒注入 ×10 限流+告警）
//! 判据原文：UNX-F48502-J1 反向判据：以 10^4 键/秒向回注通道注入风暴，
//! 限流器必须按 ×10 削峰（每秒最多放行 1000 枚），超放部分丢弃记账，
//! 且风暴秒必须触发告警；常态秒不得误告警；允许+丢弃 == 注入总量
//! （守恒）、告警账可复核、指纹与呈现行齐全。
//! 判据落地（drill 场景）：R1 限流常量闭集；R2 风暴削峰恰 ×10；
//! R3 风暴告警必触发；R4 常态秒不误告警；R5 守恒（允许+丢弃=总量）；
//! R6 告警账守恒；R7 阈值边界（恰 1000 不告警 / 1001 告警）；
//! R8 fuzz 风暴流零错态；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE960;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_CAP: u16 = SEG_BASE + 1; // 限流常量异常
const E_LEAK: u16 = SEG_BASE + 2; // 削峰不力（放行超限）
const E_ALARM: u16 = SEG_BASE + 3; // 风暴未告警
const E_FALSE: u16 = SEG_BASE + 4; // 常态误告警
const E_CONSERV: u16 = SEG_BASE + 5; // 守恒失败
const E_ALARMACCT: u16 = SEG_BASE + 6; // 告警账不守恒
const E_BOUND: u16 = SEG_BASE + 7; // 阈值边界判定错误
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 风暴流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 风暴注入强度（键/秒）。
pub const STORM_RATE: u32 = 10_000;
/// 削峰倍数：放行 = 注入 / 10。
pub const THROTTLE_DIV: u32 = 10;
/// 每秒放行上限（枚）。
pub const ADMIT_CAP: u32 = STORM_RATE / THROTTLE_DIV;
/// 告警阈值（枚/秒）：注入超过此值即告警。
pub const ALARM_AT: u32 = ADMIT_CAP;

/// 一秒的回注流量账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SecBill {
    pub sec: u32,
    pub injected: u32,
    pub admitted: u32,
    pub dropped: u32,
    pub alarmed: bool,
}

/// 风暴限流闸：逐秒削峰 + 告警。
pub struct StormGuard {
    pub bills: Vec<SecBill>,
    pub alarm_count: u32,
    pub sec: u32,
}

impl StormGuard {
    pub fn new() -> StormGuard {
        StormGuard {
            bills: vec![],
            alarm_count: 0,
            sec: 0,
        }
    }

    /// 结一秒账：注入 n 枚，放行 min(n, ADMIT_CAP)，其余丢弃；
    /// 注入 > ALARM_AT 触发告警一次。
    pub fn feed(&mut self, sec: u32, injected: u32) -> Result<SecBill, u16> {
        if sec <= self.sec && !self.bills.is_empty() {
            return Err(E_CONSERV);
        }
        self.sec = sec;
        let admitted = if injected <= ADMIT_CAP {
            injected
        } else {
            ADMIT_CAP
        };
        let dropped = injected - admitted;
        let alarmed = injected > ALARM_AT;
        if alarmed {
            self.alarm_count += 1;
        }
        let bill = SecBill {
            sec,
            injected,
            admitted,
            dropped,
            alarmed,
        };
        self.bills.push(bill);
        Ok(bill)
    }

    /// 全账守恒：每秒 admitted + dropped == injected。
    pub fn conserved(&self) -> bool {
        let mut i = 0usize;
        while i < self.bills.len() {
            if self.bills[i].admitted + self.bills[i].dropped != self.bills[i].injected {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 告警账守恒：告警计数 == 账内 alarmed 秒数。
    pub fn alarm_sane(&self) -> bool {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.bills.len() {
            if self.bills[i].alarmed {
                n += 1;
            }
            i += 1;
        }
        n == self.alarm_count
    }

    /// 放行不越限：所有秒 admitted ≤ ADMIT_CAP。
    pub fn no_leak(&self) -> bool {
        let mut i = 0usize;
        while i < self.bills.len() {
            if self.bills[i].admitted > ADMIT_CAP {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 汇总注入量。
    pub fn total_injected(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.bills.len() {
            s += self.bills[i].injected;
            i += 1;
        }
        s
    }

    /// 汇总放行量。
    pub fn total_admitted(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.bills.len() {
            s += self.bills[i].admitted;
            i += 1;
        }
        s
    }
}

/// 削峰单点：返回放行数（≤ ADMIT_CAP）。
pub fn throttle(injected: u32) -> u32 {
    if injected <= ADMIT_CAP {
        injected
    } else {
        ADMIT_CAP
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 限流常量闭集：STORM_RATE == 10^4、×10 削峰、ADMIT_CAP == 1000。
    if STORM_RATE != 10_000 || THROTTLE_DIV != 10 || ADMIT_CAP != 1000 {
        return Err(E_CAP);
    }
    hits += 1;

    // R2 风暴削峰恰 ×10：注入 10,000 枚 → 放行恰 1,000、丢弃恰 9,000。
    let b = throttle(STORM_RATE);
    if b != ADMIT_CAP {
        return Err(E_LEAK);
    }
    let mut sg = StormGuard::new();
    let bill = sg.feed(1, STORM_RATE)?;
    if bill.admitted != ADMIT_CAP || bill.dropped != STORM_RATE - ADMIT_CAP {
        return Err(E_LEAK);
    }
    hits += 1;

    // R3 风暴告警必触发：注入 10,000 枚的秒 alarmed 且告警计数恰 1。
    if !bill.alarmed || sg.alarm_count != 1 {
        return Err(E_ALARM);
    }
    if !sg.alarm_sane() {
        return Err(E_ALARMACCT);
    }
    hits += 1;

    // R4 常态秒不误告警：注入 500/1000 枚均不告警。
    let mut sg2 = StormGuard::new();
    let b1 = sg2.feed(1, 500)?;
    let b2 = sg2.feed(2, ADMIT_CAP)?;
    if b1.alarmed || b2.alarmed || sg2.alarm_count != 0 {
        return Err(E_FALSE);
    }
    hits += 1;

    // R5 守恒（允许+丢弃=总量）：连续 6 风暴秒守恒且全账守恒。
    let mut sg3 = StormGuard::new();
    let mut sec = 1u32;
    while sec <= 6 {
        sg3.feed(sec, STORM_RATE)?;
        sec += 1;
    }
    if !sg3.conserved() || !sg3.no_leak() {
        return Err(E_CONSERV);
    }
    if sg3.total_admitted() != 6 * ADMIT_CAP {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 告警账守恒：6 风暴秒告警恰 6、账面 alarmed 秒数与计数一致。
    if sg3.alarm_count != 6 || !sg3.alarm_sane() {
        return Err(E_ALARMACCT);
    }
    hits += 1;

    // R7 阈值边界：恰 1000 不告警；1001 告警且丢弃恰 1。
    if throttle(ALARM_AT) != ALARM_AT {
        return Err(E_BOUND);
    }
    let mut sg4 = StormGuard::new();
    let b3 = sg4.feed(1, ALARM_AT)?;
    if b3.alarmed {
        return Err(E_FALSE);
    }
    let b4 = sg4.feed(2, ALARM_AT + 1)?;
    if !b4.alarmed || b4.admitted != ADMIT_CAP || b4.dropped != 1 {
        return Err(E_BOUND);
    }
    hits += 1;

    // R8 fuzz 风暴流零错态：LCG 驱动 64 秒混合强度（0..=12,000），
    // 逐秒守恒、放行不越限、告警账一致、常态秒零误告。
    let mut sg5 = StormGuard::new();
    let mut lcg = Lcg::new(0x48502);
    let mut sec = 1u32;
    let mut t = 0u32;
    while t < 64 {
        let injected = lcg.below(12_001);
        let b = sg5.feed(sec, injected)?;
        if b.admitted + b.dropped != b.injected {
            return Err(E_FUZZ);
        }
        if b.admitted > ADMIT_CAP {
            return Err(E_LEAK);
        }
        if injected <= NORMAL_BOUND && b.alarmed {
            return Err(E_FALSE);
        }
        if injected > NORMAL_BOUND && !b.alarmed {
            return Err(E_ALARM);
        }
        sec += 1;
        t += 1;
    }
    if !sg5.conserved() || !sg5.alarm_sane() || !sg5.no_leak() {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = guard_fp(&sg5);
    let f2 = guard_fp(&sg5);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：cap=/in=/admit=/alarm= 四段前缀在位。
    let line = guard_line(&sg5);
    if !has_kw(line.as_bytes(), b"cap=")
        || !has_kw(line.as_bytes(), b"in=")
        || !has_kw(line.as_bytes(), b"admit=")
        || !has_kw(line.as_bytes(), b"alarm=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 常态判据上界（与告警阈值同值，语义侧写）。
pub const NORMAL_BOUND: u32 = ALARM_AT;

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

/// 账面指纹：告警数 + 全秒账串联。
pub fn guard_fp(sg: &StormGuard) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&sg.alarm_count.to_le_bytes());
    let mut i = 0usize;
    while i < sg.bills.len() {
        buf.extend_from_slice(&sg.bills[i].injected.to_le_bytes());
        buf.extend_from_slice(&sg.bills[i].admitted.to_le_bytes());
        buf.extend_from_slice(&sg.bills[i].dropped.to_le_bytes());
        buf.push(sg.bills[i].alarmed as u8);
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：cap=<放行上限> in=<注入> admit=<放行> alarm=<告警数>。
pub fn guard_line(sg: &StormGuard) -> String {
    let mut s = String::new();
    s.push_str("cap=");
    s.push_str(&ADMIT_CAP.to_string());
    s.push_str(" in=");
    s.push_str(&sg.total_injected().to_string());
    s.push_str(" admit=");
    s.push_str(&sg.total_admitted().to_string());
    s.push_str(" alarm=");
    s.push_str(&sg.alarm_count.to_string());
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
