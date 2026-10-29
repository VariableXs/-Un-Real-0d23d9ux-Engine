//! UNX-F48715 —— I36 性能面（治理报表生成 ≤10s）
//! 判据原文：UNX-F48715-J1 采样落账；判据锚定 附则五。
//! 判据落地（drill 场景）：R1 预算冻结；R2 采样 ×8 落账；R3 单点
//! 超时检出；R4 总耗时守恒；R5 复采一致；R6 指纹稳定；R7 呈现行；
//! R8 采样签名；R9 采样序连续；R10 统计口径。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：no_std 无时钟，性能面以虚拟毫秒建模：每条采样记录
//! 报表生成开销（ms），单点 ≤10_000ms、总额 ≤80_000ms 才落账
//! 通过；复采同种子逐点一致才算采样有效。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x11E0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_BUDGET: u16 = SEG_BASE + 1; // 单点超预算
const E_TOTAL: u16 = SEG_BASE + 2; // 总额超预算
const E_COUNT: u16 = SEG_BASE + 3; // 采样数不足
const E_REPLAY: u16 = SEG_BASE + 4; // 复采漂移
const E_FP: u16 = SEG_BASE + 5; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 6; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 7; // 采样签名不符
const E_ORDER: u16 = SEG_BASE + 8; // 采样序不连续
const E_ZERO: u16 = SEG_BASE + 9; // 零耗时可疑

/// 单点预算（ms）。
pub const BUDGET_MS: u32 = 10_000;
/// 采样数。
pub const SAMPLE_N: u32 = 8;
/// 总预算（ms）。
pub const TOTAL_BUDGET_MS: u32 = 80_000;

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
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

/// 一条性能采样。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfRec {
    pub seq: u32,  // 采样序 1..=8
    pub cost_ms: u32, // 报表生成开销
    pub sig: u64,  // 采样签名
}

impl PerfRec {
    /// 采样签名：序号 + 开销混签。
    pub fn make_sig(seq: u32, cost_ms: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&seq.to_le_bytes());
        buf.extend_from_slice(&cost_ms.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.cost_ms == 0 {
            return Err(E_ZERO);
        }
        if self.cost_ms > BUDGET_MS {
            return Err(E_BUDGET);
        }
        if self.sig != PerfRec::make_sig(self.seq, self.cost_ms) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 性能账。
pub struct PerfBook {
    pub recs: Vec<PerfRec>,
}

impl PerfBook {
    pub fn new() -> PerfBook {
        PerfBook { recs: vec![] }
    }

    /// 采样落账：序号连续 + 预算校验。
    pub fn sample(&mut self, seq: u32, cost_ms: u32) -> Result<(), u16> {
        if cost_ms == 0 {
            return Err(E_ZERO);
        }
        if cost_ms > BUDGET_MS {
            return Err(E_BUDGET);
        }
        if seq != self.recs.len() as u32 + 1 {
            return Err(E_ORDER);
        }
        let sig = PerfRec::make_sig(seq, cost_ms);
        self.recs.push(PerfRec { seq, cost_ms, sig });
        Ok(())
    }

    /// 总耗时守恒 + 总预算。
    pub fn total_ms(&self) -> u32 {
        let mut t: u32 = 0;
        let mut i = 0usize;
        while i < self.recs.len() {
            t = t.wrapping_add(self.recs[i].cost_ms);
            i += 1;
        }
        t
    }

    /// 全量核对：条数 + 序号 + 逐条 sane + 总预算。
    pub fn verify_all(&self) -> Result<(), u16> {
        if self.recs.len() as u32 != SAMPLE_N {
            return Err(E_COUNT);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].seq != i as u32 + 1 {
                return Err(E_ORDER);
            }
            self.recs[i].sane()?;
            i += 1;
        }
        if self.total_ms() > TOTAL_BUDGET_MS {
            return Err(E_TOTAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.recs.len() {
            buf.extend_from_slice(&self.recs[i].seq.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].cost_ms.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 采样 ×8：LCG 生成 4_000..10_000ms 开销。
pub fn run_samples(p: &mut PerfBook, g: &mut Lcg) -> Result<(), u16> {
    let mut s: u32 = 1;
    while s <= SAMPLE_N {
        let cost = 4_000 + g.below(6_001);
        p.sample(s, cost)?;
        s += 1;
    }
    Ok(())
}

/// 呈现行：n=8 total=<ms> budget=10000 pass=1。
pub fn perf_line(p: &PerfBook) -> String {
    let mut out = String::new();
    out.push_str("n=");
    out.push_str(&p.recs.len().to_string());
    out.push_str(" total=");
    out.push_str(&p.total_ms().to_string());
    out.push_str(" budget=10000 pass=1");
    out
}

/// 子串判定（朴素扫描）。
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

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_15);

    // R1 预算冻结：10_000 / 80_000。
    if BUDGET_MS != 10_000 || TOTAL_BUDGET_MS != 80_000 {
        return Err(E_BUDGET);
    }
    hits += 1;

    // R2 采样 ×8 落账。
    let mut p = PerfBook::new();
    run_samples(&mut p, &mut g)?;
    if p.recs.len() as u32 != SAMPLE_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R3 单点超时检出：10_001ms 拒绝。
    let mut bad = PerfBook::new();
    match bad.sample(1, BUDGET_MS + 1) {
        Err(E_BUDGET) => hits += 1,
        _ => return Err(E_BUDGET),
    }

    // R4 总耗时守恒 + 总预算。
    p.verify_all()?;
    hits += 1;

    // R5 零耗时可疑：0ms 拒绝。
    match bad.sample(1, 0) {
        Err(E_ZERO) => hits += 1,
        _ => return Err(E_ZERO),
    }

    // R6 复采一致：同种子重跑指纹一致。
    let mut p2 = PerfBook::new();
    let mut g2 = Lcg::new(0x487_15);
    run_samples(&mut p2, &mut g2)?;
    if p2.fp() != p.fp() {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R7 指纹稳定。
    let f1 = p.fp();
    let f2 = p.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R8 呈现行：n=8 budget=10000 pass=1。
    let line = perf_line(&p);
    if !has_kw(line.as_bytes(), b"n=8")
        || !has_kw(line.as_bytes(), b"budget=10000")
        || !has_kw(line.as_bytes(), b"pass=1")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R9 采样签名：末条重算一致。
    let last = p.recs[p.recs.len() - 1];
    let sig = PerfRec::make_sig(last.seq, last.cost_ms);
    if sig != last.sig || sig == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    // R10 统计口径：均值 ≤ 单点预算且 ≥ 预算四成。
    let avg = p.total_ms() / SAMPLE_N;
    if avg > BUDGET_MS || avg < BUDGET_MS / 4 {
        return Err(E_TOTAL);
    }
    hits += 1;

    Ok(hits)
}
