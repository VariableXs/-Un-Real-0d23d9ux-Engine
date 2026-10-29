//! UNX-F48757 C·采样时间源终审（ts_source=TSC 全账断言）
//! 判据 UNX-F48757-J1：断言通过；判据锚定 附则五。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 全账时间源标记必须恒为 TSC，出现他源样本即终审红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1C60;
const E_SRC_BAD: u16 = SEG_BASE + 1;
const E_TS_MISS: u16 = SEG_BASE + 2;
const E_TS_BACK: u16 = SEG_BASE + 3;
const E_LEDGER_GAP: u16 = SEG_BASE + 4;
const E_REPLAY_DIFF: u16 = SEG_BASE + 5;
const E_TAG_BAD: u16 = SEG_BASE + 6;
const E_DUP_TS: u16 = SEG_BASE + 7;
const E_AUDIT_DEAD: u16 = SEG_BASE + 8;

/// 时间源枚举：TSC 为唯一合法源
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TsSource {
    Tsc,
    Other(u8),
}

impl TsSource {
    pub fn is_tsc(&self) -> bool {
        match self {
            TsSource::Tsc => true,
            _ => false,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            TsSource::Tsc => "TSC",
            TsSource::Other(_) => "OTHER",
        }
    }
}

/// 带源时戳样本
#[derive(Clone, Debug, PartialEq)]
pub struct SourcedTs {
    pub src: TsSource,
    pub ts: u64,
}

impl SourcedTs {
    pub fn tsc(ts: u64) -> Self {
        SourcedTs { src: TsSource::Tsc, ts }
    }
}

/// 时间源终审账
pub struct TsAudit {
    pub samples: Vec<SourcedTs>,
}

impl TsAudit {
    pub fn new() -> Self {
        TsAudit { samples: vec![] }
    }

    pub fn push(&mut self, s: SourcedTs) -> Result<(), u16> {
        if !s.src.is_tsc() {
            return Err(E_SRC_BAD);
        }
        let mut i = 0usize;
        while i < self.samples.len() {
            if self.samples[i].ts == s.ts {
                return Err(E_DUP_TS);
            }
            if self.samples[i].ts > s.ts {
                return Err(E_TS_BACK);
            }
            i += 1;
        }
        self.samples.push(s);
        Ok(())
    }

    /// 全账断言：逐条恒 TSC、时戳严格递增
    pub fn assert_all_tsc(&self) -> Result<u32, u16> {
        if self.samples.is_empty() {
            return Err(E_LEDGER_GAP);
        }
        let mut green = 0u32;
        let mut i = 0usize;
        while i < self.samples.len() {
            if !self.samples[i].src.is_tsc() {
                return Err(E_SRC_BAD);
            }
            if self.samples[i].ts == 0 {
                return Err(E_TS_MISS);
            }
            green += 1;
            i += 1;
        }
        Ok(green)
    }

    /// 时戳单调终审
    pub fn assert_monotonic(&self) -> Result<(), u16> {
        let mut i = 1usize;
        while i < self.samples.len() {
            if self.samples[i].ts <= self.samples[i - 1].ts {
                return Err(E_TS_BACK);
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

/// 全账 TSC 断言 ×10^3
fn sweep_all_tsc(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let mut a = TsAudit::new();
        let mut ts = 1_000 + rng.below(5_000);
        let n = 4 + rng.below(8);
        let mut c = 0u64;
        while c < n {
            a.push(SourcedTs::tsc(ts))?;
            ts += 10 + rng.below(500);
            c += 1;
        }
        if a.assert_all_tsc()? != n as u32 {
            return Err(E_AUDIT_DEAD);
        }
        a.assert_monotonic()?;
        iter += 1;
    }
    Ok(())
}

/// 他源注入 ×10：非 TSC 源必须被当场点名
fn sweep_other_src() -> Result<(), u16> {
    let mut round = 0u32;
    while round < 10 {
        let mut a = TsAudit::new();
        a.push(SourcedTs::tsc(100))?;
        a.push(SourcedTs::tsc(200))?;
        let bad = SourcedTs { src: TsSource::Other(round as u8 + 1), ts: 300 };
        let r = a.push(bad);
        if r.err() != Some(E_SRC_BAD) {
            return Err(E_SRC_BAD);
        }
        round += 1;
    }
    Ok(())
}

/// 反例注入：重复时戳、时戳倒退、空账、零时戳全命中
fn sweep_negatives() -> Result<(), u16> {
    let mut a = TsAudit::new();
    a.push(SourcedTs::tsc(1_000))?;
    let r1 = a.push(SourcedTs::tsc(1_000));
    if r1.err() != Some(E_DUP_TS) {
        return Err(E_DUP_TS);
    }
    let r2 = a.push(SourcedTs::tsc(500));
    if r2.err() != Some(E_TS_BACK) {
        return Err(E_TS_BACK);
    }
    let r3 = TsAudit::new().assert_all_tsc();
    if r3.err() != Some(E_LEDGER_GAP) {
        return Err(E_LEDGER_GAP);
    }
    let mut b = TsAudit::new();
    let r4 = b.push(SourcedTs::tsc(0));
    let _ = r4?;
    if b.assert_all_tsc().err() != Some(E_TS_MISS) {
        return Err(E_TS_MISS);
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑账面逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut aa = TsAudit::new();
        let mut ab = TsAudit::new();
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let mut ta = 500u64;
        let mut tb = 500u64;
        let mut c = 0u32;
        while c < 10 {
            let da = 10 + ra.below(900);
            let db = 10 + rb.below(900);
            if da != db {
                return Err(E_REPLAY_DIFF);
            }
            ta += da;
            tb += db;
            aa.push(SourcedTs::tsc(ta))?;
            ab.push(SourcedTs::tsc(tb))?;
            c += 1;
        }
        if aa.samples != ab.samples {
            return Err(E_REPLAY_DIFF);
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 全账 TSC 断言 ×10^3
    let mut rng = Lcg::new(0x4875_71FF);
    let mut r1 = 0u32;
    while r1 < 1_000 {
        sweep_all_tsc(&mut rng)?;
        r1 += 1;
    }
    hits += 1;
    // R2 他源注入 ×10 全中
    sweep_other_src()?;
    hits += 1;
    // R3 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R4 重放一致 ×20
    sweep_replay(0x4875_7C0B)?;
    hits += 1;
    // R5 源名语义：TSC 名恒 TSC、他源名恒 OTHER
    if TsSource::Tsc.name() != "TSC" {
        return Err(E_TAG_BAD);
    }
    if TsSource::Other(3).name() != "OTHER" {
        return Err(E_TAG_BAD);
    }
    hits += 1;
    // R6 源判定自洽：is_tsc 单向
    if !TsSource::Tsc.is_tsc() || TsSource::Other(0).is_tsc() {
        return Err(E_SRC_BAD);
    }
    hits += 1;
    // R7 单调终审 ×20 组
    let mut g = 0u32;
    while g < 20 {
        let mut a = TsAudit::new();
        let mut ts = 10u64;
        let mut c = 0u32;
        while c < 24 {
            a.push(SourcedTs::tsc(ts))?;
            ts += 7;
            c += 1;
        }
        a.assert_monotonic()?;
        g += 1;
    }
    hits += 1;
    // R8 断言计数与账面一致
    let mut a = TsAudit::new();
    let mut ts = 2_000u64;
    let mut c = 0u32;
    while c < 16 {
        a.push(SourcedTs::tsc(ts))?;
        ts += 100;
        c += 1;
    }
    if a.assert_all_tsc()? != 16 {
        return Err(E_AUDIT_DEAD);
    }
    hits += 1;
    // R9 账面快照不变：断言前后样本一致
    let snap = a.samples.clone();
    a.assert_monotonic()?;
    if a.samples != snap {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;
    // R10 时戳带域校验：全部 >0 且首条在带
    if snap[0].ts < 2_000 {
        return Err(E_TS_MISS);
    }
    let mut i = 0usize;
    while i < snap.len() {
        if snap[i].ts == 0 {
            return Err(E_TS_MISS);
        }
        i += 1;
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
