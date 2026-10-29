//! UNX-F48495 —— E25 校准冻结账（40ms 窗实测校准后冻结声明）
//! 判据原文：UNX-F48495-J1 E25 抖动校准以 40ms 实测窗为单位：
//! 窗内样本只进不出、窗满后由实测样本推出校准值并出具冻结声明；
//! 冻结后账目只读，任何改写路径必须被拒；样本守恒、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 窗宽常量；R2 窗满触发冻结；
//! R3 冻结后改写拒收；R4 校准值落在实测极值内；
//! R5 样本守恒；R6 未满窗不得冻结；R7 空窗拒冻；
//! R8 fuzz 样本流零错态；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE840;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_WIN: u16 = SEG_BASE + 1; // 窗宽常量异常
const E_FREEZE: u16 = SEG_BASE + 2; // 冻结触发异常
const E_WRITABLE: u16 = SEG_BASE + 3; // 冻结后仍可改写
const E_RANGE: u16 = SEG_BASE + 4; // 校准值越出实测极值
const E_CONSERV: u16 = SEG_BASE + 5; // 样本不守恒
const E_EARLY: u16 = SEG_BASE + 6; // 未满窗即冻结
const E_EMPTY: u16 = SEG_BASE + 7; // 空窗拒冻失败
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 样本流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 实测窗宽（毫秒）。
pub const WINDOW_MS: u32 = 40;

/// 一枚抖动实测样本：相对窗内时戳（0..WINDOW_MS）与抖动宽度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CalibSample {
    pub t_ms: u32,
    pub jitter_us: u32,
}

/// 校准冻结账：窗满即冻结，冻结后只读。
pub struct CalibLedger {
    pub samples: Vec<CalibSample>,
    pub frozen: bool,
    pub calib_us: u32,
    pub seq: u32,
}

impl CalibLedger {
    pub fn new() -> CalibLedger {
        CalibLedger {
            samples: vec![],
            frozen: false,
            calib_us: 0,
            seq: 0,
        }
    }

    /// 样本合法性：窗内时戳、抖动宽度闭集（0..=5000us）。
    pub fn valid_sample(s: &CalibSample) -> bool {
        s.t_ms < WINDOW_MS && s.jitter_us <= 5000
    }

    /// 入样一枚：冻结后拒收；非法样本拒收。
    pub fn put(&mut self, s: CalibSample) -> Result<(), u16> {
        if self.frozen {
            return Err(E_WRITABLE);
        }
        if !CalibLedger::valid_sample(&s) {
            return Err(E_RANGE);
        }
        self.samples.push(s);
        self.seq += 1;
        Ok(())
    }

    /// 实测极值：返回 (最小, 最大)；空窗返回 None。
    pub fn extremes(&self) -> Option<(u32, u32)> {
        if self.samples.is_empty() {
            return None;
        }
        let mut lo = self.samples[0].jitter_us;
        let mut hi = self.samples[0].jitter_us;
        let mut i = 1usize;
        while i < self.samples.len() {
            if self.samples[i].jitter_us < lo {
                lo = self.samples[i].jitter_us;
            }
            if self.samples[i].jitter_us > hi {
                hi = self.samples[i].jitter_us;
            }
            i += 1;
        }
        Some((lo, hi))
    }

    /// 由实测样本推出校准值：取平均（整除），落在极值内。
    pub fn calib_value(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.samples.len() {
            sum += self.samples[i].jitter_us;
            i += 1;
        }
        sum / self.samples.len() as u32
    }

    /// 冻结声明：空窗拒冻；未满窗（样本数 < WINDOW_MS，每 ms 一枚的最低密度）
    /// 拒冻；成功时写入 calib_us 并置 frozen。
    pub fn freeze(&mut self) -> Result<u32, u16> {
        if self.frozen {
            return Err(E_FREEZE);
        }
        if self.samples.is_empty() {
            return Err(E_EMPTY);
        }
        if (self.samples.len() as u32) < WINDOW_MS {
            return Err(E_EARLY);
        }
        let cv = self.calib_value();
        let ext = self.extremes();
        match ext {
            Some((lo, hi)) => {
                if cv < lo || cv > hi {
                    return Err(E_RANGE);
                }
            }
            None => return Err(E_EMPTY),
        }
        self.calib_us = cv;
        self.frozen = true;
        Ok(cv)
    }

    /// 样本守恒：账面笔数 == 外部记账笔数。
    pub fn conserved(&self, total: u32) -> bool {
        self.samples.len() as u32 == total
    }

    /// 序号连续性：seq == 样本数。
    pub fn seq_sane(&self) -> bool {
        self.seq as usize == self.samples.len()
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 窗宽常量：WINDOW_MS == 40，且窗内时戳闭集判定成立。
    if WINDOW_MS != 40 {
        return Err(E_WIN);
    }
    let s_ok = CalibSample { t_ms: 39, jitter_us: 100 };
    let s_bad_t = CalibSample { t_ms: 40, jitter_us: 100 };
    if !CalibLedger::valid_sample(&s_ok) || CalibLedger::valid_sample(&s_bad_t) {
        return Err(E_WIN);
    }
    hits += 1;

    // R2 窗满触发冻结：投满 40 枚样本（每 ms 一枚），freeze 成功且 frozen 置位。
    let mut lg = CalibLedger::new();
    let mut t = 0u32;
    while t < WINDOW_MS {
        lg.put(CalibSample { t_ms: t, jitter_us: 100 + t })?;
        t += 1;
    }
    let cv = lg.freeze()?;
    if !lg.frozen || cv == 0 {
        return Err(E_FREEZE);
    }
    hits += 1;

    // R3 冻结后改写拒收：再 put 一枚被拒，账目不变，码恰为 E_WRITABLE。
    let before = lg.samples.len();
    match lg.put(CalibSample { t_ms: 0, jitter_us: 1 }) {
        Err(E_WRITABLE) => hits += 1,
        _ => return Err(E_WRITABLE),
    }
    if lg.samples.len() != before {
        return Err(E_WRITABLE);
    }

    // R4 校准值落在实测极值内：极值夹逼校准值。
    match lg.extremes() {
        Some((lo, hi)) => {
            if lg.calib_us < lo || lg.calib_us > hi {
                return Err(E_RANGE);
            }
        }
        None => return Err(E_RANGE),
    }
    hits += 1;

    // R5 样本守恒：窗满账 40 枚，seq 一致。
    if !lg.conserved(40) || !lg.seq_sane() {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 未满窗不得冻结：39 枚样本 freeze 被拒，码恰为 E_EARLY。
    let mut lg2 = CalibLedger::new();
    let mut t = 0u32;
    while t < WINDOW_MS - 1 {
        lg2.put(CalibSample { t_ms: t, jitter_us: 10 })?;
        t += 1;
    }
    match lg2.freeze() {
        Err(E_EARLY) => hits += 1,
        _ => return Err(E_EARLY),
    }

    // R7 空窗拒冻：零样本 freeze 被拒，码恰为 E_EMPTY。
    let mut lg3 = CalibLedger::new();
    match lg3.freeze() {
        Err(E_EMPTY) => hits += 1,
        _ => return Err(E_EMPTY),
    }

    // R8 fuzz 样本流零错态：LCG 注入 200 枚合法样本（自动超额入窗），
    // 过程零错态、守恒、极值夹逼成立。
    let mut lg4 = CalibLedger::new();
    let mut lcg = Lcg::new(0x48495);
    let mut total = 0u32;
    let mut t = 0u32;
    while t < 200 {
        let s = CalibSample {
            t_ms: lcg.below(WINDOW_MS),
            jitter_us: lcg.below(5001),
        };
        lg4.put(s)?;
        total += 1;
        t += 1;
    }
    if !lg4.conserved(total) || !lg4.seq_sane() {
        return Err(E_FUZZ);
    }
    match lg4.extremes() {
        Some((lo, hi)) => {
            let cv = lg4.calib_value();
            if cv < lo || cv > hi {
                return Err(E_FUZZ);
            }
        }
        None => return Err(E_FUZZ),
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = ledger_fp(&lg4);
    let f2 = ledger_fp(&lg4);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：win=/frozen=/calib= 三段前缀在位。
    let line = ledger_line(&lg);
    if !has_kw(line.as_bytes(), b"win=")
        || !has_kw(line.as_bytes(), b"frozen=")
        || !has_kw(line.as_bytes(), b"calib=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
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

/// 账面指纹：frozen + calib + 全样本串联。
pub fn ledger_fp(lg: &CalibLedger) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.push(lg.frozen as u8);
    buf.extend_from_slice(&lg.calib_us.to_le_bytes());
    let mut i = 0usize;
    while i < lg.samples.len() {
        buf.extend_from_slice(&lg.samples[i].t_ms.to_le_bytes());
        buf.extend_from_slice(&lg.samples[i].jitter_us.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：win=<窗宽> frozen=<0/1> calib=<校准值>。
pub fn ledger_line(lg: &CalibLedger) -> String {
    let mut s = String::new();
    s.push_str("win=");
    s.push_str(&WINDOW_MS.to_string());
    s.push_str(" frozen=");
    s.push_str(&(lg.frozen as u32).to_string());
    s.push_str(" calib=");
    s.push_str(&lg.calib_us.to_string());
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
