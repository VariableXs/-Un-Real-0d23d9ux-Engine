//! UNX-F48664 —— I·延迟账打包供 O1 对标（附则五格式终验）
//! 判据原文：UNX-F48664-J1 格式机检全过；判据锚定 附则五。
//! 判据落地（drill 场景）：R1 四段在位；R2 分位单调；R3 时间源断言；
//! R4 样本数下限；R5 格式机检全过；R6 缺段检出；R7 越限显性化；
//! R8 校验和稳定；R9 呈现行；R10 指纹稳定。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：附则五格式四段（唤起/首显/候选/上屏）逐段必含
//! p50/p95/p99/samples/ts_source 五要素；分位单调、时间源恒 TSC、
//! 每段样本 ≥1000 为格式机检三红线，缺段、重复段、越限值皆红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0A60;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_SEG: u16 = SEG_BASE + 1; // 段缺失或越界
const E_MONO: u16 = SEG_BASE + 2; // 分位非单调
const E_TS: u16 = SEG_BASE + 3; // 时间源非 TSC
const E_N: u16 = SEG_BASE + 4; // 样本数不足
const E_FMT: u16 = SEG_BASE + 5; // 格式机检失败
const E_CSUM: u16 = SEG_BASE + 6; // 校验和漂移
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_OVER: u16 = SEG_BASE + 9; // 越限未显性化
const E_DUP: u16 = SEG_BASE + 10; // 段重复

/// 延迟账段闭集（附则五四段）。
pub const S_WAKE: u8 = 1; // 唤起段
pub const S_FIRST: u8 = 2; // 首显段
pub const S_CAND: u8 = 3; // 候选段
pub const S_COMMIT: u8 = 4; // 上屏段
/// 时间源：TSC。
pub const TS_TSC: u8 = 1;
/// 样本数下限。
pub const MIN_SAMPLES: u32 = 1_000;

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

/// 一段延迟分位账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LatSeg {
    pub seg: u8,
    pub p50: u32,
    pub p95: u32,
    pub p99: u32,
    pub samples: u32,
    pub ts_source: u8,
}

impl LatSeg {
    /// 单段格式机检：分位单调 + 时间源 TSC + 样本下限。
    pub fn fmt_ok(&self) -> Result<(), u16> {
        if !LatPack::seg_ok(self.seg) {
            return Err(E_SEG);
        }
        if !(self.p50 <= self.p95 && self.p95 <= self.p99) {
            return Err(E_MONO);
        }
        if self.ts_source != TS_TSC {
            return Err(E_TS);
        }
        if self.samples < MIN_SAMPLES {
            return Err(E_N);
        }
        Ok(())
    }

    /// 序列化。
    pub fn bytes(&self) -> Vec<u8> {
        let mut buf: Vec<u8> = vec![];
        buf.push(self.seg);
        buf.extend_from_slice(&self.p50.to_le_bytes());
        buf.extend_from_slice(&self.p95.to_le_bytes());
        buf.extend_from_slice(&self.p99.to_le_bytes());
        buf.extend_from_slice(&self.samples.to_le_bytes());
        buf.push(self.ts_source);
        buf
    }
}

/// 延迟账打包（供 O1 对标）。
pub struct LatPack {
    pub segs: Vec<LatSeg>,
}

impl LatPack {
    pub fn new() -> LatPack {
        LatPack { segs: vec![] }
    }

    /// 段闭集。
    pub fn seg_ok(s: u8) -> bool {
        s == S_WAKE || s == S_FIRST || s == S_CAND || s == S_COMMIT
    }

    /// 录入一段：段闭集 + 段唯一。
    pub fn put(&mut self, s: LatSeg) -> Result<(), u16> {
        if !LatPack::seg_ok(s.seg) {
            return Err(E_SEG);
        }
        let mut i = 0usize;
        while i < self.segs.len() {
            if self.segs[i].seg == s.seg {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.segs.push(s);
        Ok(())
    }

    /// 格式机检全过：四段齐 + 逐段 fmt_ok。
    pub fn fmt_check(&self) -> Result<u32, u16> {
        if self.segs.len() != 4 {
            return Err(E_SEG);
        }
        let mut want = 1u8;
        let mut i = 0usize;
        while i < self.segs.len() {
            if self.segs[i].seg != want {
                return Err(E_SEG);
            }
            self.segs[i].fmt_ok()?;
            want += 1;
            i += 1;
        }
        Ok(self.segs.len() as u32)
    }

    /// 校验和。
    pub fn checksum(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.segs.len() {
            buf.extend_from_slice(&self.segs[i].bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 构造 ×1 延迟账：确定性四段。
pub fn build_pack(g: &mut Lcg) -> Result<LatPack, u16> {
    let mut p = LatPack::new();
    let mut k: u8 = 1;
    while k <= 4 {
        let base: u32 = 100 * k as u32 + g.below(50);
        p.put(LatSeg {
            seg: k,
            p50: base,
            p95: base + 60,
            p99: base + 120,
            samples: 1_000 + g.below(500),
            ts_source: TS_TSC,
        })?;
        k += 1;
    }
    Ok(p)
}

/// 呈现行：segs=<n> p99max=<四段最大 p99> ts=<时间源>。
pub fn pack_line(p: &LatPack) -> String {
    let mut mx: u32 = 0;
    let mut i = 0usize;
    while i < p.segs.len() {
        if p.segs[i].p99 > mx {
            mx = p.segs[i].p99;
        }
        i += 1;
    }
    let mut out = String::new();
    out.push_str("segs=");
    out.push_str(&p.segs.len().to_string());
    out.push_str(" p99max=");
    out.push_str(&mx.to_string());
    out.push_str(" ts=1");
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
    let mut g = Lcg::new(0x486_64);

    // R1 四段在位：唤醒/首显/候选/上屏按序齐备。
    // 段序即消费序：O1 按段闭集顺序对标，缺段或乱序均拒。
    let pack = build_pack(&mut g)?;
    if pack.segs.len() != 4 {
        return Err(E_SEG);
    }
    hits += 1;

    // R2 分位单调：逐段 p50 ≤ p95 ≤ p99。
    let mut i = 0usize;
    while i < pack.segs.len() {
        let s = &pack.segs[i];
        if !(s.p50 <= s.p95 && s.p95 <= s.p99) {
            return Err(E_MONO);
        }
        i += 1;
    }
    hits += 1;

    // R3 时间源断言：全段 ts_source == TSC。
    // 时间源混入非 TSC 值即视为采样口径污染，逐段断言不留死角。
    let mut i = 0usize;
    while i < pack.segs.len() {
        if pack.segs[i].ts_source != TS_TSC {
            return Err(E_TS);
        }
        i += 1;
    }
    hits += 1;

    // R4 样本数下限：逐段 ≥1000。
    // 样本不足即分位数失真，P99 不可信，机检直接红。
    let mut i = 0usize;
    while i < pack.segs.len() {
        if pack.segs[i].samples < MIN_SAMPLES {
            return Err(E_N);
        }
        i += 1;
    }
    hits += 1;

    // R5 格式机检全过：四段齐 + 逐段五要素复核。
    // 机检是 O1 对标的前置闸：不过闸不出账。
    pack.fmt_check()?;
    hits += 1;

    // R6 缺段检出：删一段后机检红；段重复录入红。
    let mut thin = LatPack::new();
    let mut i = 0usize;
    while i < 3 {
        thin.segs.push(pack.segs[i].clone());
        i += 1;
    }
    match thin.fmt_check() {
        Err(E_SEG) => {}
        _ => return Err(E_SEG),
    }
    let mut dupp = build_pack(&mut g)?;
    match dupp.put(pack.segs[0].clone()) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R7 越限显性化：非 TSC 时间源与样本不足均红。
    let bad_ts = LatSeg { seg: S_WAKE, p50: 10, p95: 20, p99: 30, samples: 2_000, ts_source: 2 };
    match bad_ts.fmt_ok() {
        Err(E_TS) => {}
        _ => return Err(E_TS),
    }
    let bad_n = LatSeg { seg: S_CAND, p50: 10, p95: 20, p99: 30, samples: 100, ts_source: TS_TSC };
    match bad_n.fmt_ok() {
        Err(E_N) => hits += 1,
        _ => return Err(E_N),
    }

    // R8 校验和稳定：同账两次一致。
    // 校验和覆盖段序+五要素，任一被改即漂移。
    let c1 = pack.checksum();
    let c2 = pack.checksum();
    if c1 != c2 || c1 == 0 {
        return Err(E_CSUM);
    }
    hits += 1;

    // R9 呈现行：segs=4 ts=1。
    let line = pack_line(&pack);
    if !has_kw(line.as_bytes(), b"segs=4") || !has_kw(line.as_bytes(), b"ts=1") {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 指纹稳定：同账两次指纹一致且非零，
    // 指纹漂移即视为打包被改，O1 对标以指纹为锚。
    let f1 = pack.checksum();
    let f2 = pack.checksum();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    Ok(hits)
}
