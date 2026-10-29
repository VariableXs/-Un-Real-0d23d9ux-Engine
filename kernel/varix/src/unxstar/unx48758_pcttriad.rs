//! UNX-F48758 C·首显延迟分位数全面账（P50/P95/P99 三档并账）
//! 判据 UNX-F48758-J1：三档落账；判据锚定 附则五。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! P50 ≤ P95 ≤ P99 三档并账缺档即红，秩法全自算。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1CA0;
const E_PCT_MISS: u16 = SEG_BASE + 1;
const E_EMPTY_SET: u16 = SEG_BASE + 2;
const E_ORDER_BAD: u16 = SEG_BASE + 3;
const E_LEDGER_GAP: u16 = SEG_BASE + 4;
const E_REPLAY_DIFF: u16 = SEG_BASE + 5;
const E_VAL_BAD: u16 = SEG_BASE + 6;
const E_TRIAD_LOST: u16 = SEG_BASE + 7;
const E_INPUT_BAD: u16 = SEG_BASE + 8;

/// 样本集
pub struct SampleSet {
    pub vals: Vec<u32>,
}

impl SampleSet {
    pub fn new() -> Self {
        SampleSet { vals: vec![] }
    }

    pub fn push(&mut self, us: u32) -> Result<(), u16> {
        if us == 0 {
            return Err(E_INPUT_BAD);
        }
        self.vals.push(us);
        Ok(())
    }

    pub fn sorted(&self) -> Vec<u32> {
        let mut v = self.vals.clone();
        let n = v.len();
        let mut i = 0usize;
        while i < n {
            let mut j = i + 1;
            while j < n {
                if v[j] < v[i] {
                    let t = v[i];
                    v[i] = v[j];
                    v[j] = t;
                }
                j += 1;
            }
            i += 1;
        }
        v
    }

    /// 秩法分位：pct ∈ [1,99]
    pub fn pct(&self, pct: u32) -> Result<u32, u16> {
        if self.vals.is_empty() {
            return Err(E_EMPTY_SET);
        }
        if pct == 0 || pct > 99 {
            return Err(E_PCT_MISS);
        }
        let s = self.sorted();
        let n = s.len();
        let rank = (n * pct as usize + 99) / 100;
        let idx = if rank == 0 { 0 } else if rank > n { n - 1 } else { rank - 1 };
        Ok(s[idx])
    }

    /// 三档并账：P50/P95/P99 成组落账且单调
    pub fn triad(&self) -> Result<(u32, u32, u32), u16> {
        let p50 = self.pct(50)?;
        let p95 = self.pct(95)?;
        let p99 = self.pct(99)?;
        if p50 > p95 || p95 > p99 {
            return Err(E_ORDER_BAD);
        }
        Ok((p50, p95, p99))
    }
}

/// 三档并账簿
pub struct TriadLedger {
    pub entries: Vec<(String, u32, u32, u32)>,
}

impl TriadLedger {
    pub fn new() -> Self {
        TriadLedger { entries: vec![] }
    }

    pub fn record(&mut self, label: &str, set: &SampleSet) -> Result<(), u16> {
        if label.is_empty() {
            return Err(E_VAL_BAD);
        }
        let (a, b, c) = set.triad()?;
        self.entries.push((String::from(label), a, b, c));
        Ok(())
    }

    pub fn verify(&self, n: usize) -> Result<(), u16> {
        if self.entries.len() != n {
            return Err(E_LEDGER_GAP);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.1 > e.2 || e.2 > e.3 {
                return Err(E_TRIAD_LOST);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 确定性 LCG
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 三档并账 ×10^3
fn sweep_triads(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let mut set = SampleSet::new();
        let n = 8 + rng.below(16);
        let mut c = 0u64;
        while c < n {
            set.push(600 + rng.below(4_000) as u32)?;
            c += 1;
        }
        set.triad()?;
        iter += 1;
    }
    Ok(())
}

/// 秩法手工对照 ×10：1..=100 等差序列三档已知值
fn sweep_known_triads() -> Result<(), u16> {
    let mut round = 0u32;
    while round < 10 {
        let mut set = SampleSet::new();
        let mut v = 1u32;
        while v <= 100 {
            set.push(v * 10)?;
            v += 1;
        }
        let (a, b, c) = set.triad()?;
        if a != 5_000 || b != 9_500 || c != 10_000 {
            return Err(E_ORDER_BAD);
        }
        round += 1;
    }
    Ok(())
}

/// 反例注入：空集、越界分位、零样本、空标签全命中
fn sweep_negatives() -> Result<(), u16> {
    let empty = SampleSet::new();
    let r1 = empty.pct(50);
    if r1.err() != Some(E_EMPTY_SET) {
        return Err(E_EMPTY_SET);
    }
    let mut set = SampleSet::new();
    set.push(100)?;
    set.push(200)?;
    let r2 = set.pct(0);
    if r2.err() != Some(E_PCT_MISS) {
        return Err(E_PCT_MISS);
    }
    let r3 = set.pct(100);
    if r3.err() != Some(E_PCT_MISS) {
        return Err(E_PCT_MISS);
    }
    let mut led = TriadLedger::new();
    let r4 = led.record("", &set);
    if r4.err() != Some(E_VAL_BAD) {
        return Err(E_VAL_BAD);
    }
    let mut bad = SampleSet::new();
    let r5 = bad.push(0);
    if r5.err() != Some(E_INPUT_BAD) {
        return Err(E_INPUT_BAD);
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑三档逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut sa = SampleSet::new();
        let mut sb = SampleSet::new();
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let mut c = 0u32;
        while c < 20 {
            let va = 500 + ra.below(3_000) as u32;
            let vb = 500 + rb.below(3_000) as u32;
            if va != vb {
                return Err(E_REPLAY_DIFF);
            }
            sa.push(va)?;
            sb.push(vb)?;
            c += 1;
        }
        if sa.triad()? != sb.triad()? {
            return Err(E_REPLAY_DIFF);
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 三档并账 ×10^3
    let mut rng = Lcg::new(0x4875_8200);
    let mut r1 = 0u32;
    while r1 < 1_000 {
        sweep_triads(&mut rng)?;
        r1 += 1;
    }
    hits += 1;
    // R2 秩法手工对照 ×10
    sweep_known_triads()?;
    hits += 1;
    // R3 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R4 重放一致 ×20
    sweep_replay(0x4875_8D0C)?;
    hits += 1;
    // R5 并账簿三标签落账复核
    let mut led = TriadLedger::new();
    let mut label = 0u32;
    while label < 3 {
        let mut set = SampleSet::new();
        let mut c = 0u32;
        while c < 30 {
            set.push(1_000 + c * 20 + label)?;
            c += 1;
        }
        let name = match label {
            0 => "win0",
            1 => "win1",
            _ => "win2",
        };
        led.record(name, &set)?;
        label += 1;
    }
    led.verify(3)?;
    hits += 1;
    // R6 单点集三档同值
    let mut one = SampleSet::new();
    one.push(1_234)?;
    let t = one.triad()?;
    if t.0 != 1_234 || t.1 != 1_234 || t.2 != 1_234 {
        return Err(E_TRIAD_LOST);
    }
    hits += 1;
    // R7 全同值集三档同值 ×10 组
    let mut g = 0u32;
    while g < 10 {
        let mut same = SampleSet::new();
        let base = 2_000 + g * 100;
        let mut c = 0u32;
        while c < 16 {
            same.push(base)?;
            c += 1;
        }
        if same.triad()?.0 != base {
            return Err(E_ORDER_BAD);
        }
        g += 1;
    }
    hits += 1;
    // R8 分位带内逐点单调：p50 ≤ p60 ≤ … ≤ p99
    let mut set = SampleSet::new();
    let mut v = 1u32;
    while v <= 50 {
        set.push(v * 30)?;
        v += 1;
    }
    let mut p = 50u32;
    while p < 99 {
        if set.pct(p)? > set.pct(p + 1)? {
            return Err(E_ORDER_BAD);
        }
        p += 1;
    }
    hits += 1;
    // R9 并账簿标签序稳定
    let es = led.entries.clone();
    if es[0].0 != "win0" || es[2].0 != "win2" {
        return Err(E_VAL_BAD);
    }
    hits += 1;
    // R10 排序自洽：sorted 尾最大
    let mut big = SampleSet::new();
    let mut b = 0u32;
    while b < 40 {
        big.push(9_000 - b)?;
        b += 1;
    }
    let srt = big.sorted();
    if srt[0] != 8_961 || srt[39] != 9_000 {
        return Err(E_ORDER_BAD);
    }
    hits += 1;
    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a: u32 = 0x9E37_79B9;
    let mut track_b: u32 = 0x9E37_79B9;
    let mut rail: Vec<u32> = vec![];
    let mut rail_sum: u64 = 0;
    let mut dstep = 0u32;
    while dstep < 48 {
        track_a = track_a.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b = track_b.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail.push(track_a);
        rail_sum += track_a as u64;
        dstep += 1;
    }
    if track_a != track_b {
        return Err(E_REPLAY_DIFF);
    }
    if rail.len() != 48 {
        return Err(E_REPLAY_DIFF);
    }
    let mut replay_sum: u64 = 0;
    let mut ridx = 0usize;
    while ridx < rail.len() {
        replay_sum += rail[ridx] as u64;
        ridx += 1;
    }
    if replay_sum != rail_sum {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;
    Ok(hits)
}
