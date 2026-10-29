//! UNX-F48517 —— E26 性能面（防御路径开销 ≤5% 采样）
//! 判据原文：UNX-F48517-J1 E26 风暴防御路径开销以成对采样账核收：
//! 每笔采样绑定基准路径与防御路径的实测成本，防御开销
//! (defended - base)/base ≤ 5% 为达标；越界采样必须被检出并计数，
//! 不得静默放行；采样账守恒、均值/极值可复核、指纹与呈现行齐全。
//! 判据落地（drill 场景）：R1 阈值常量；R2 全量达标账；
//! R3 开销边界（恰 5% 达标 / 6% 越界）；R4 越界检出计数；
//! R5 采样账守恒；R6 均值夹逼极值；R7 零开销与饱和边界；
//! R8 fuzz 采样流零错态；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xEBE0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_THR: u16 = SEG_BASE + 1; // 阈值常量异常
const E_OVER: u16 = SEG_BASE + 2; // 达标账出现越界采样
const E_BOUND: u16 = SEG_BASE + 3; // 开销边界判定错误
const E_MISS: u16 = SEG_BASE + 4; // 越界未被检出
const E_CONSERV: u16 = SEG_BASE + 5; // 采样账不守恒
const E_MEAN: u16 = SEG_BASE + 6; // 均值越出极值夹逼
const E_EDGE: u16 = SEG_BASE + 7; // 零/饱和成本边界错
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 采样流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 防御开销上限（百分比 ×100 整数口径）：5%。
pub const OVERHEAD_PCT100: u32 = 500;
/// 采样成本闭集上界（微秒）：饱和采样。
pub const MAX_US: u32 = 1_000_000;

/// 一笔成对采样：基准成本 + 防御成本（微秒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfSample {
    pub base_us: u32,
    pub def_us: u32,
}

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

    /// 收样一笔：成本闭集（def ≥ base，def ≤ MAX_US）校验；
    /// 开销越界只计数不拒收（观测面）。
    pub fn put(&mut self, s: PerfSample) -> Result<(), u16> {
        if s.def_us < s.base_us || s.def_us > MAX_US {
            return Err(E_EDGE);
        }
        if s.base_us == 0 {
            return Err(E_EDGE);
        }
        if overhead_pct100(&s) > OVERHEAD_PCT100 {
            self.over_count += 1;
        }
        self.samples.push(s);
        Ok(())
    }

    /// 开销账：全量采样开销 ≤ 上限。
    pub fn all_within(&self) -> bool {
        let mut i = 0usize;
        while i < self.samples.len() {
            if overhead_pct100(&self.samples[i]) > OVERHEAD_PCT100 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 开销极值：(min, max)，以 pct100 口径；空账 None。
    pub fn extremes(&self) -> Option<(u32, u32)> {
        if self.samples.is_empty() {
            return None;
        }
        let mut lo = overhead_pct100(&self.samples[0]);
        let mut hi = lo;
        let mut i = 1usize;
        while i < self.samples.len() {
            let o = overhead_pct100(&self.samples[i]);
            if o < lo {
                lo = o;
            }
            if o > hi {
                hi = o;
            }
            i += 1;
        }
        Some((lo, hi))
    }

    /// 平均开销（pct100 口径，整除）：空账返回 0。
    pub fn mean(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.samples.len() {
            sum += overhead_pct100(&self.samples[i]) as u64;
            i += 1;
        }
        (sum / self.samples.len() as u64) as u32
    }

    /// 守恒：账面笔数 == 外部记账。
    pub fn conserved(&self, total: u32) -> bool {
        self.samples.len() as u32 == total
    }
}

/// 开销计算（×100 整数口径）：(def - base) * 100 / base。
/// 调用方保证 base > 0 且 def ≥ base。
pub fn overhead_pct100(s: &PerfSample) -> u32 {
    let d = (s.def_us - s.base_us) as u64;
    let b = s.base_us as u64;
    ((d * 100) / b) as u32
}

/// 越界判定单点：overhead_pct100 > OVERHEAD_PCT100 即越界。
pub fn is_over(s: &PerfSample) -> bool {
    overhead_pct100(s) > OVERHEAD_PCT100
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 阈值常量：OVERHEAD_PCT100 == 500，is_over 单点判定自洽。
    if OVERHEAD_PCT100 != 500 {
        return Err(E_THR);
    }
    let at = PerfSample { base_us: 1000, def_us: 1050 };
    let over = PerfSample { base_us: 1000, def_us: 1060 };
    if is_over(&at) || !is_over(&over) {
        return Err(E_THR);
    }
    hits += 1;

    // R2 全量达标账：64 笔随机低开销采样（def = base + base/100），
    // over_count == 0 且 all_within。
    let mut bk = PerfBook::new();
    let mut lcg = Lcg::new(0x48517);
    let mut t = 0u32;
    while t < 64 {
        let b = 2000 + lcg.below(10_000u32);
        bk.put(PerfSample {
            base_us: b,
            def_us: b + b / 100,
        })?;
        t += 1;
    }
    if bk.over_count != 0 || !bk.all_within() {
        return Err(E_OVER);
    }
    hits += 1;

    // R3 开销边界：恰 5% 达标、6% 越界、0 开销达标。
    let mut bk2 = PerfBook::new();
    bk2.put(PerfSample { base_us: 1000, def_us: 1050 })?;
    if is_over(&PerfSample { base_us: 1000, def_us: 1050 }) {
        return Err(E_BOUND);
    }
    bk2.put(PerfSample { base_us: 1000, def_us: 1060 })?;
    if !is_over(&PerfSample { base_us: 1000, def_us: 1060 }) || bk2.over_count != 1 {
        return Err(E_BOUND);
    }
    bk2.put(PerfSample { base_us: 4000, def_us: 4000 })?;
    if is_over(&PerfSample { base_us: 4000, def_us: 4000 }) {
        return Err(E_BOUND);
    }
    hits += 1;

    // R4 越界检出计数：固定 6 笔含 2 笔越界，over_count 恰 2。
    let mut bk3 = PerfBook::new();
    let sams = [
        PerfSample { base_us: 1000, def_us: 1000 },   // 0% 达标
        PerfSample { base_us: 1000, def_us: 1100 },   // 10% 越界
        PerfSample { base_us: 2000, def_us: 2050 },   // 2.5% 达标
        PerfSample { base_us: 1000, def_us: 1200 },   // 20% 越界
        PerfSample { base_us: 5000, def_us: 5100 },   // 2% 达标
        PerfSample { base_us: 1000, def_us: 1050 },   // 5% 达标
    ];
    let mut i = 0usize;
    while i < sams.len() {
        bk3.put(sams[i])?;
        i += 1;
    }
    if bk3.over_count != 2 {
        return Err(E_MISS);
    }
    hits += 1;

    // R5 采样账守恒：混合 24 笔，账面笔数一致。
    let mut bk4 = PerfBook::new();
    let mut lcg2 = Lcg::new(0x26E1_00);
    let mut t = 0u32;
    while t < 24 {
        let b = 1000 + lcg2.below(20_000u32);
        let pct = lcg2.below(1001);
        bk4.put(PerfSample {
            base_us: b,
            def_us: b + b * pct / 10_000,
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

    // R7 零开销与饱和边界：0 开销可入账且极值口径一致；def 缺省不可小于 base。
    let mut bk5 = PerfBook::new();
    bk5.put(PerfSample { base_us: 9000, def_us: 9000 })?;
    match bk5.extremes() {
        Some((lo, hi)) => {
            if lo != 0 || hi != 0 {
                return Err(E_EDGE);
            }
        }
        None => return Err(E_EDGE),
    }
    let bad = PerfSample { base_us: 2000, def_us: 1999 };
    let mut bk6 = PerfBook::new();
    match bk6.put(bad) {
        Err(E_EDGE) => hits += 1,
        _ => return Err(E_EDGE),
    }

    // R8 fuzz 采样流零错态：LCG 注入 256 笔合法采样，守恒 + 越界计数与
    // 独立扫描复核一致。
    let mut bk7 = PerfBook::new();
    let mut lcg3 = Lcg::new(0x26E1_01);
    let mut t = 0u32;
    while t < 256 {
        let b = 1000 + lcg3.below(50_000u32);
        let pct = lcg3.below(2001);
        let s = PerfSample {
            base_us: b,
            def_us: b + b * pct / 10_000,
        };
        bk7.put(s)?;
        t += 1;
    }
    if !bk7.conserved(256) {
        return Err(E_FUZZ);
    }
    let mut expect_over = 0u32;
    let mut i = 0usize;
    while i < bk7.samples.len() {
        if is_over(&bk7.samples[i]) {
            expect_over += 1;
        }
        i += 1;
    }
    if expect_over != bk7.over_count {
        return Err(E_MISS);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = book_fp(&bk7);
    let f2 = book_fp(&bk7);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：n=/over=/mean= 三段前缀在位。
    let line = book_line(&bk7);
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
        buf.extend_from_slice(&bk.samples[i].base_us.to_le_bytes());
        buf.extend_from_slice(&bk.samples[i].def_us.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：n=<笔数> over=<越界> mean=<平均开销 pct100>。
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
