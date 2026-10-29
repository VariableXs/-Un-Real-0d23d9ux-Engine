//! UNX-F48575 —— I29 性能面（定位数据路径 ≤1ms 采样）
//! 判据原文：UNX-F48575-J1 I29 定位数据路径单次处理成本以采样账核收：
//! 每笔采样 ≤1ms（1000us）为达标；越界采样必须被检出并计数，
//! 不得静默放行；采样账守恒、均值/极值可复核、指纹与呈现行齐全。
//! 判据落地（drill 场景）：R1 阈值常量；R2 全量达标账；
//! R3 边界采样（恰 1000us 达标 / 1001us 越界）；R4 越界检出计数；
//! R5 采样账守恒；R6 均值夹逼极值；R7 零成本与饱和成本边界；
//! R8 fuzz 采样流零错态；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF7C0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_THR: u16 = SEG_BASE + 1; // 阈值常量异常
const E_OVER: u16 = SEG_BASE + 2; // 达标账出现越界采样
const E_BOUND: u16 = SEG_BASE + 3; // 边界采样判定错误
const E_MISS: u16 = SEG_BASE + 4; // 越界未被检出
const E_CONSERV: u16 = SEG_BASE + 5; // 采样账不守恒
const E_MEAN: u16 = SEG_BASE + 6; // 均值越出极值夹逼
const E_EDGE: u16 = SEG_BASE + 7; // 零/饱和成本边界错
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 采样流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 定位数据路径单次成本阈值（微秒）：1ms = 1000us。
pub const BUDGET_US: u32 = 1000;
/// 采样成本闭集上界（微秒）：饱和采样。
pub const MAX_US: u32 = 200_000;

/// 一笔性能采样：路径类别 + 实测成本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfSample {
    pub path: u8,
    pub cost_us: u32,
}

/// 路径闭集：1 = 定位主路径，2 = 复签路径，3 = 通知路径。
pub const P_LOCATE: u8 = 1;
pub const P_RESIGN: u8 = 2;
pub const P_NOTIFY: u8 = 3;

/// 采样账：收样、越界计数、统计量。
pub struct PerfBook {
    pub samples: Vec<PerfSample>,
    pub over_count: u32,
}

impl PerfBook {
    pub fn new() -> PerfBook {
        PerfBook {
            samples: vec![],
            over_count: 0,
        }
    }

    /// 路径闭集判定。
    pub fn known_path(p: u8) -> bool {
        p == P_LOCATE || p == P_RESIGN || p == P_NOTIFY
    }

    /// 收样一笔：路径闭集、成本闭集校验；越界只计数不拒收（观测面）。
    pub fn put(&mut self, s: PerfSample) -> Result<(), u16> {
        if !PerfBook::known_path(s.path) {
            return Err(E_THR);
        }
        if s.cost_us > MAX_US {
            return Err(E_EDGE);
        }
        if s.cost_us > BUDGET_US {
            self.over_count += 1;
        }
        self.samples.push(s);
        Ok(())
    }

    /// 达标账：主路径全部采样 ≤ BUDGET_US。
    pub fn locate_all_within(&self) -> bool {
        let mut i = 0usize;
        while i < self.samples.len() {
            if self.samples[i].path == P_LOCATE && self.samples[i].cost_us > BUDGET_US {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 极值：(min, max)；空账 None。
    pub fn extremes(&self) -> Option<(u32, u32)> {
        if self.samples.is_empty() {
            return None;
        }
        let mut lo = self.samples[0].cost_us;
        let mut hi = self.samples[0].cost_us;
        let mut i = 1usize;
        while i < self.samples.len() {
            if self.samples[i].cost_us < lo {
                lo = self.samples[i].cost_us;
            }
            if self.samples[i].cost_us > hi {
                hi = self.samples[i].cost_us;
            }
            i += 1;
        }
        Some((lo, hi))
    }

    /// 均值（整除）：空账返回 0。
    pub fn mean(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.samples.len() {
            sum += self.samples[i].cost_us as u64;
            i += 1;
        }
        (sum / self.samples.len() as u64) as u32
    }

    /// 守恒：账面笔数 == 外部记账。
    pub fn conserved(&self, total: u32) -> bool {
        self.samples.len() as u32 == total
    }
}

/// 越界判定单点：cost > BUDGET_US 即越界。
pub fn is_over(cost_us: u32) -> bool {
    cost_us > BUDGET_US
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 阈值常量：BUDGET_US == 1000，is_over 单点判定自洽。
    if BUDGET_US != 1000 {
        return Err(E_THR);
    }
    if is_over(BUDGET_US) || !is_over(BUDGET_US + 1) || is_over(0) {
        return Err(E_THR);
    }
    hits += 1;

    // R2 全量达标账：主路径 64 笔全部 ≤1000us，over_count == 0。
    let mut bk = PerfBook::new();
    let mut lcg = Lcg::new(0x48575);
    let mut t = 0u32;
    while t < 64 {
        bk.put(PerfSample {
            path: P_LOCATE,
            cost_us: lcg.below(BUDGET_US + 1),
        })?;
        t += 1;
    }
    if bk.over_count != 0 || !bk.locate_all_within() {
        return Err(E_OVER);
    }
    hits += 1;

    // R3 边界采样：恰 1000us 达标、1001us 越界、0 达标。
    let mut bk2 = PerfBook::new();
    bk2.put(PerfSample { path: P_LOCATE, cost_us: BUDGET_US })?;
    if is_over(BUDGET_US) {
        return Err(E_BOUND);
    }
    bk2.put(PerfSample { path: P_LOCATE, cost_us: BUDGET_US + 1 })?;
    if !is_over(BUDGET_US + 1) || bk2.over_count != 1 {
        return Err(E_BOUND);
    }
    bk2.put(PerfSample { path: P_LOCATE, cost_us: 0 })?;
    if is_over(0) {
        return Err(E_BOUND);
    }
    hits += 1;

    // R4 越界检出计数：混合流 8 笔含 3 笔越界，over_count 恰 3。
    let mut bk3 = PerfBook::new();
    let costs = [10u32, 3000u32, 999u32, 20000u32, 1001u32, 100u32, 50000u32, 45u32];
    let mut i = 0usize;
    while i < costs.len() {
        bk3.put(PerfSample { path: P_LOCATE, cost_us: costs[i] })?;
        i += 1;
    }
    if bk3.over_count != 3 {
        return Err(E_MISS);
    }
    hits += 1;

    // R5 采样账守恒：三路径混合 24 笔，账面笔数一致。
    let mut bk4 = PerfBook::new();
    let mut lcg2 = Lcg::new(0x2910_10);
    let mut t = 0u32;
    while t < 24 {
        let p = (1 + lcg2.below(3)) as u8;
        bk4.put(PerfSample {
            path: p,
            cost_us: lcg2.below(MAX_US + 1),
        })?;
        t += 1;
    }
    if !bk4.conserved(24) {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 均值夹逼极值：min ≤ mean ≤ max。
    match bk4.extremes() {
        Some((lo, hi)) => {
            let m = bk4.mean();
            if m < lo || m > hi {
                return Err(E_MEAN);
            }
        }
        None => return Err(E_MEAN),
    }
    hits += 1;

    // R7 零成本与饱和成本边界：0 与 MAX_US 均可入账，饱和账极值恰为 MAX_US。
    let mut bk5 = PerfBook::new();
    bk5.put(PerfSample { path: P_RESIGN, cost_us: 0 })?;
    bk5.put(PerfSample {
        path: P_NOTIFY,
        cost_us: MAX_US,
    })?;
    match bk5.extremes() {
        Some((lo, hi)) => {
            if lo != 0 || hi != MAX_US {
                return Err(E_EDGE);
            }
        }
        None => return Err(E_EDGE),
    }
    hits += 1;

    // R8 fuzz 采样流零错态：LCG 注入 256 笔合法采样，守恒 + 越界计数与扫描复核一致。
    let mut bk6 = PerfBook::new();
    let mut lcg3 = Lcg::new(0x2910_11);
    let mut t = 0u32;
    while t < 256 {
        let s = PerfSample {
            path: (1 + lcg3.below(3)) as u8,
            cost_us: lcg3.below(MAX_US + 1),
        };
        bk6.put(s)?;
        t += 1;
    }
    if !bk6.conserved(256) {
        return Err(E_FUZZ);
    }
    // 独立扫描复核越界数。
    let mut expect_over = 0u32;
    let mut i = 0usize;
    while i < bk6.samples.len() {
        if is_over(bk6.samples[i].cost_us) {
            expect_over += 1;
        }
        i += 1;
    }
    if expect_over != bk6.over_count {
        return Err(E_MISS);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = book_fp(&bk6);
    let f2 = book_fp(&bk6);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：n=/over=/mean= 三段前缀在位。
    let line = book_line(&bk6);
    if !has_kw(line.as_bytes(), b"n=")
        || !has_kw(line.as_bytes(), b"over=")
        || !has_kw(line.as_bytes(), b"mean=")
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

/// 账面指纹：over_count + 全采样串联。
pub fn book_fp(bk: &PerfBook) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&bk.over_count.to_le_bytes());
    let mut i = 0usize;
    while i < bk.samples.len() {
        buf.push(bk.samples[i].path);
        buf.extend_from_slice(&bk.samples[i].cost_us.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：n=<笔数> over=<越界> mean=<均值>。
pub fn book_line(bk: &PerfBook) -> String {
    let mut s = String::new();
    s.push_str("n=");
    s.push_str(&(bk.samples.len() as u32).to_string());
    s.push_str(" over=");
    s.push_str(&bk.over_count.to_string());
    s.push_str(" mean=");
    s.push_str(&bk.mean().to_string());
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
