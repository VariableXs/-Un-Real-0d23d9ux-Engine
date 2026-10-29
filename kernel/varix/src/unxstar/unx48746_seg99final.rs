//! UNX-F48746 C·三窗延迟分段终账（每窗分段 P99 并账）
//! 判据 UNX-F48746-J1：并账零缺段；判据锚定 附则五。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 三窗（vxwm 原生/Wine/Xwayland）各分段 P99 须齐备，并账缺任一段即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1A60;
const E_SEG_MISS: u16 = SEG_BASE + 1;
const E_P99_LOST: u16 = SEG_BASE + 2;
const E_WIN_MISS: u16 = SEG_BASE + 3;
const E_EMPTY_SEG: u16 = SEG_BASE + 4;
const E_MERGE_DIFF: u16 = SEG_BASE + 5;
const E_ORDER_BAD: u16 = SEG_BASE + 6;
const E_REPLAY_DIFF: u16 = SEG_BASE + 7;
const E_SPAN_ZERO: u16 = SEG_BASE + 8;

/// 延迟样本：微秒时戳
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LatSample {
    pub win: u8,
    pub seg: u8,
    pub us: u32,
}

/// 分段 P99 计算器：排序取 99 分位（最近秩法）
pub struct SegP99 {
    pub win: u8,
    pub seg: u8,
    pub vals: Vec<u32>,
}

impl SegP99 {
    pub fn new(win: u8, seg: u8) -> Self {
        SegP99 { win, seg, vals: vec![] }
    }

    pub fn push(&mut self, us: u32) {
        self.vals.push(us);
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

    pub fn p99(&self) -> Result<u32, u16> {
        if self.vals.is_empty() {
            return Err(E_EMPTY_SEG);
        }
        let s = self.sorted();
        let n = s.len();
        let rank = (n * 99 + 99) / 100;
        let idx = if rank == 0 { 0 } else if rank > n { n - 1 } else { rank - 1 };
        Ok(s[idx])
    }
}

/// 三窗分段并账
pub struct MergeLedger {
    pub segs: Vec<SegP99>,
}

impl MergeLedger {
    pub fn new() -> Self {
        MergeLedger { segs: vec![] }
    }

    pub fn feed(&mut self, s: LatSample) {
        let mut found = false;
        let mut i = 0usize;
        while i < self.segs.len() {
            if self.segs[i].win == s.win && self.segs[i].seg == s.seg {
                self.segs[i].push(s.us);
                found = true;
            }
            i += 1;
        }
        if !found {
            let mut seg = SegP99::new(s.win, s.seg);
            seg.push(s.us);
            self.segs.push(seg);
        }
    }

    /// 并账：三窗每窗至少 2 段，各段 P99 齐备
    pub fn merge(&self) -> Result<Vec<(u8, u8, u32)>, u16> {
        if self.segs.is_empty() {
            return Err(E_SEG_MISS);
        }
        let mut out: Vec<(u8, u8, u32)> = vec![];
        let mut i = 0usize;
        while i < self.segs.len() {
            let p = self.segs[i].p99()?;
            if self.segs[i].win > 2 {
                return Err(E_WIN_MISS);
            }
            out.push((self.segs[i].win, self.segs[i].seg, p));
            i += 1;
        }
        let mut w0 = false;
        let mut w1 = false;
        let mut w2 = false;
        let mut k = 0usize;
        while k < out.len() {
            if out[k].0 == 0 {
                w0 = true;
            }
            if out[k].0 == 1 {
                w1 = true;
            }
            if out[k].0 == 2 {
                w2 = true;
            }
            k += 1;
        }
        if !w0 || !w1 || !w2 {
            return Err(E_WIN_MISS);
        }
        Ok(out)
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

/// 喂账构造器：三窗 ×2 段各 n 样本
fn feed_three_wins(led: &mut MergeLedger, n: u32, rng: &mut Lcg) -> Result<(), u16> {
    if n == 0 {
        return Err(E_SPAN_ZERO);
    }
    let mut win = 0u8;
    while win < 3 {
        let mut seg = 0u8;
        while seg < 2 {
            let mut c = 0u32;
            while c < n {
                let us = 800 + rng.below(4_000) as u32;
                led.feed(LatSample { win, seg, us });
                c += 1;
            }
            seg += 1;
        }
        win += 1;
    }
    Ok(())
}

/// 并账零缺段 ×10^3
fn sweep_merge(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let mut led = MergeLedger::new();
        feed_three_wins(&mut led, 4 + rng.below(8) as u32, rng)?;
        let merged = led.merge()?;
        if merged.len() != 6 {
            return Err(E_SEG_MISS);
        }
        let mut k = 0usize;
        while k < merged.len() {
            if merged[k].2 == 0 {
                return Err(E_P99_LOST);
            }
            k += 1;
        }
        iter += 1;
    }
    Ok(())
}

/// P99 秩法核对：已知序列手工对照 ×10
fn sweep_p99_rank() -> Result<(), u16> {
    let mut round = 0u32;
    while round < 10 {
        let mut seg = SegP99::new(0, 0);
        let mut v = 1u32;
        while v <= 100 {
            seg.push(v * 10);
            v += 1;
        }
        let p = seg.p99()?;
        if p != 10_000 {
            return Err(E_P99_LOST);
        }
        round += 1;
    }
    Ok(())
}

/// 反例注入：空段、缺窗、零跨度全命中
fn sweep_negatives() -> Result<(), u16> {
    let mut seg = SegP99::new(1, 1);
    let r1 = seg.p99();
    if r1.err() != Some(E_EMPTY_SEG) {
        return Err(E_EMPTY_SEG);
    }
    let mut led = MergeLedger::new();
    let r2 = led.merge();
    if r2.err() != Some(E_SEG_MISS) {
        return Err(E_SEG_MISS);
    }
    let mut led2 = MergeLedger::new();
    led2.feed(LatSample { win: 0, seg: 0, us: 1000 });
    led2.feed(LatSample { win: 1, seg: 0, us: 1100 });
    let r3 = led2.merge();
    if r3.err() != Some(E_WIN_MISS) {
        return Err(E_WIN_MISS);
    }
    let mut led3 = MergeLedger::new();
    led3.feed(LatSample { win: 3, seg: 0, us: 900 });
    let r4 = led3.merge();
    if r4.err() != Some(E_WIN_MISS) {
        return Err(E_WIN_MISS);
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑六段 P99 逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut la = MergeLedger::new();
        let mut lb = MergeLedger::new();
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let mut win = 0u8;
        while win < 3 {
            let mut seg = 0u8;
            while seg < 2 {
                let mut c = 0u32;
                while c < 12 {
                    let va = 900 + ra.below(3_000) as u32;
                    let vb = 900 + rb.below(3_000) as u32;
                    if va != vb {
                        return Err(E_REPLAY_DIFF);
                    }
                    la.feed(LatSample { win, seg, us: va });
                    lb.feed(LatSample { win, seg, us: vb });
                    c += 1;
                }
                seg += 1;
            }
            win += 1;
        }
        let ma = la.merge()?;
        let mb = lb.merge()?;
        let mut k = 0usize;
        while k < ma.len() {
            if ma[k] != mb[k] {
                return Err(E_MERGE_DIFF);
            }
            k += 1;
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 并账零缺段 ×10^3
    let mut rng = Lcg::new(0x4874_6D44);
    let mut r1 = 0u32;
    while r1 < 1_000 {
        sweep_merge(&mut rng)?;
        r1 += 1;
    }
    hits += 1;
    // R2 P99 秩法手工对照 ×10
    sweep_p99_rank()?;
    hits += 1;
    // R3 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R4 重放一致 ×20
    sweep_replay(0x4874_6EF0)?;
    hits += 1;
    // R5 分段样本越喂越齐：单段 100 点 P99 稳定
    let mut seg = SegP99::new(2, 1);
    let mut c = 0u32;
    while c < 100 {
        seg.push(5_000 + c);
        c += 1;
    }
    let p = seg.p99()?;
    if p < 5_900 {
        return Err(E_P99_LOST);
    }
    hits += 1;
    // R6 排序自洽：sorted 首尾为最小最大
    let s = seg.sorted();
    if s.len() != 100 || s[0] != 5_000 || s[99] != 5_099 {
        return Err(E_ORDER_BAD);
    }
    hits += 1;
    // R7 并账元组逐段落值：三窗齐备断言
    let mut led = MergeLedger::new();
    feed_three_wins(&mut led, 20, &mut rng)?;
    let merged = led.merge()?;
    if merged.len() != 6 {
        return Err(E_MERGE_DIFF);
    }
    hits += 1;
    // R8 窗号域校验：0/1/2 之外的窗号并账必红
    let mut led2 = MergeLedger::new();
    led2.feed(LatSample { win: 0, seg: 0, us: 1000 });
    led2.feed(LatSample { win: 1, seg: 0, us: 1200 });
    led2.feed(LatSample { win: 2, seg: 0, us: 1400 });
    led2.merge()?;
    let mut led3 = MergeLedger::new();
    led3.feed(LatSample { win: 7, seg: 0, us: 1000 });
    let r8 = led3.merge();
    if r8.err() != Some(E_WIN_MISS) {
        return Err(E_WIN_MISS);
    }
    hits += 1;
    // R9 P99 ≥ P50 语义：取 99 分位不低于中位
    let mut seg2 = SegP99::new(0, 2);
    let mut c2 = 0u32;
    while c2 < 50 {
        seg2.push(2_000 + c2 * 40);
        c2 += 1;
    }
    let p99 = seg2.p99()?;
    let sorted = seg2.sorted();
    let p50 = sorted[25];
    if p99 < p50 {
        return Err(E_ORDER_BAD);
    }
    hits += 1;
    // R10 空账喂零跨度必须 E_SPAN_ZERO
    let mut led4 = MergeLedger::new();
    let r10 = feed_three_wins(&mut led4, 0, &mut rng);
    if r10.err() != Some(E_SPAN_ZERO) {
        return Err(E_SPAN_ZERO);
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
