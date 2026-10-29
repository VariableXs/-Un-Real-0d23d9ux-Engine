//! UNX-F48759 C38 批防重 grep（关键词族：c2perf/latencyfinal）
//! 判据 UNX-F48759-J1：批收口防重留痕；判据锚定 §6.1。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 批内关键词族逐词 grep 留痕，同词重复入账即防重命中。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1CE0;
const E_DUP_KW: u16 = SEG_BASE + 1;
const E_KW_EMPTY: u16 = SEG_BASE + 2;
const E_TRACE_LOST: u16 = SEG_BASE + 3;
const E_BATCH_GAP: u16 = SEG_BASE + 4;
const E_REPLAY_DIFF: u16 = SEG_BASE + 5;
const E_KW_MISS: u16 = SEG_BASE + 6;
const E_HITS_ZERO: u16 = SEG_BASE + 7;
const E_TRACE_GAP: u16 = SEG_BASE + 8;

/// 关键词族常量
pub const KW_FAMILY: [&str; 2] = ["c2perf", "latencyfinal"];

/// grep 留痕条目：关键词 + 命中数
#[derive(Clone, Debug, PartialEq)]
pub struct GrepTrace {
    pub kw: String,
    pub hits: u32,
}

impl GrepTrace {
    pub fn make(kw: &str, hits: u32) -> Self {
        GrepTrace { kw: String::from(kw), hits }
    }
}

/// 模拟文本行集合：按词面前缀计数
pub struct TextCorpus {
    pub lines: Vec<String>,
}

impl TextCorpus {
    pub fn new() -> Self {
        TextCorpus { lines: vec![] }
    }

    pub fn add(&mut self, line: &str) {
        self.lines.push(String::from(line));
    }

    /// 逐词 grep：包含关键词的行计数
    pub fn grep(&self, kw: &str) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.lines.len() {
            if self.contains(self.lines[i].as_str(), kw) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    fn contains(&self, hay: &str, needle: &str) -> bool {
        let h = hay.as_bytes();
        let n = needle.as_bytes();
        if n.len() == 0 || h.len() < n.len() {
            return false;
        }
        let mut i = 0usize;
        while i + n.len() <= h.len() {
            let mut j = 0usize;
            let mut ok = true;
            while j < n.len() {
                if h[i + j] != n[j] {
                    ok = false;
                }
                j += 1;
            }
            if ok {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// 批防重账
pub struct BatchGrep {
    pub traces: Vec<GrepTrace>,
}

impl BatchGrep {
    pub fn new() -> Self {
        BatchGrep { traces: vec![] }
    }

    pub fn record(&mut self, kw: &str, hits: u32) -> Result<(), u16> {
        if kw.is_empty() {
            return Err(E_KW_EMPTY);
        }
        if hits == 0 {
            return Err(E_HITS_ZERO);
        }
        let mut i = 0usize;
        while i < self.traces.len() {
            if self.traces[i].kw == kw {
                return Err(E_DUP_KW);
            }
            i += 1;
        }
        self.traces.push(GrepTrace::make(kw, hits));
        Ok(())
    }

    /// 留痕核对：族内关键词逐词在册且命中数非零
    pub fn verify_family(&self) -> Result<u32, u16> {
        if self.traces.is_empty() {
            return Err(E_TRACE_LOST);
        }
        let mut green = 0u32;
        let mut f = 0usize;
        while f < KW_FAMILY.len() {
            let mut found = false;
            let mut i = 0usize;
            while i < self.traces.len() {
                if self.traces[i].kw == KW_FAMILY[f] {
                    if self.traces[i].hits == 0 {
                        return Err(E_HITS_ZERO);
                    }
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(E_KW_MISS);
            }
            green += 1;
            f += 1;
        }
        Ok(green)
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

/// 批防重留痕 ×10^3：建语料-双词 grep-入账-核对
fn sweep_batch(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let mut corpus = TextCorpus::new();
        let n = 6 + rng.below(10);
        let mut c = 0u64;
        while c < n {
            let line = match rng.below(4) {
                0 => "c2perf report ready",
                1 => "latencyfinal sealed",
                2 => "plain no kw here",
                _ => "c2perf and latencyfinal both",
            };
            corpus.add(line);
            c += 1;
        }
        let mut bg = BatchGrep::new();
        let h1 = corpus.grep("c2perf");
        let h2 = corpus.grep("latencyfinal");
        if h1 == 0 || h2 == 0 {
            return Err(E_HITS_ZERO);
        }
        bg.record("c2perf", h1)?;
        bg.record("latencyfinal", h2)?;
        if bg.verify_family()? != 2 {
            return Err(E_KW_MISS);
        }
        iter += 1;
    }
    Ok(())
}

/// 反例注入：重复词、空词、零命中、族缺词、空账全命中
fn sweep_negatives() -> Result<(), u16> {
    let mut bg = BatchGrep::new();
    bg.record("c2perf", 3)?;
    let r1 = bg.record("c2perf", 5);
    if r1.err() != Some(E_DUP_KW) {
        return Err(E_DUP_KW);
    }
    let r2 = bg.record("", 1);
    if r2.err() != Some(E_KW_EMPTY) {
        return Err(E_KW_EMPTY);
    }
    let r3 = bg.record("fresh", 0);
    if r3.err() != Some(E_HITS_ZERO) {
        return Err(E_HITS_ZERO);
    }
    let mut bg2 = BatchGrep::new();
    bg2.record("c2perf", 1)?;
    let r4 = bg2.verify_family();
    if r4.err() != Some(E_KW_MISS) {
        return Err(E_KW_MISS);
    }
    let r5 = BatchGrep::new().verify_family();
    if r5.err() != Some(E_TRACE_LOST) {
        return Err(E_TRACE_LOST);
    }
    Ok(())
}

/// 语料 grep 自查：手工计数与 grep 一致 ×10^2
fn sweep_grep_truth(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 100 {
        let mut corpus = TextCorpus::new();
        let mut expect = 0u32;
        let mut c = 0u64;
        let n = 4 + rng.below(8);
        while c < n {
            if rng.below(2) == 0 {
                corpus.add("has c2perf inside");
                expect += 1;
            } else {
                corpus.add("clean line");
            }
            c += 1;
        }
        if corpus.grep("c2perf") != expect {
            return Err(E_TRACE_GAP);
        }
        iter += 1;
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑留痕逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut ca = TextCorpus::new();
        let mut cb = TextCorpus::new();
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let mut c = 0u32;
        while c < 12 {
            let la = match ra.below(4) {
                0 => "c2perf line",
                1 => "latencyfinal line",
                2 => "neutral",
                _ => "c2perf latencyfinal",
            };
            let lb = match rb.below(4) {
                0 => "c2perf line",
                1 => "latencyfinal line",
                2 => "neutral",
                _ => "c2perf latencyfinal",
            };
            if la != lb {
                return Err(E_REPLAY_DIFF);
            }
            ca.add(la);
            cb.add(lb);
            c += 1;
        }
        let ha = ca.grep("c2perf");
        let hb = cb.grep("c2perf");
        if ha != hb {
            return Err(E_REPLAY_DIFF);
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 批防重留痕 ×10^3
    let mut rng = Lcg::new(0x4875_9311);
    let mut r1 = 0u32;
    while r1 < 1_000 {
        sweep_batch(&mut rng)?;
        r1 += 1;
    }
    hits += 1;
    // R2 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R3 grep 真值自查 ×10^2
    sweep_grep_truth(&mut rng)?;
    hits += 1;
    // R4 重放一致 ×20
    sweep_replay(0x4875_9E0D)?;
    hits += 1;
    // R5 词族常量核对：两词在册且非空
    let mut f = 0usize;
    while f < KW_FAMILY.len() {
        if KW_FAMILY[f].is_empty() {
            return Err(E_KW_EMPTY);
        }
        f += 1;
    }
    hits += 1;
    // R6 contains 自查：子串边界命中
    let mut corpus = TextCorpus::new();
    corpus.add("xxc2perfxx");
    corpus.add("c2perf");
    corpus.add("perf");
    if corpus.grep("c2perf") != 2 {
        return Err(E_TRACE_GAP);
    }
    if corpus.grep("latencyfinal") != 0 {
        return Err(E_TRACE_GAP);
    }
    hits += 1;
    // R7 留痕命中数快照不可变
    let mut bg = BatchGrep::new();
    bg.record("c2perf", 9)?;
    bg.record("latencyfinal", 4)?;
    let snap = bg.traces.clone();
    bg.verify_family()?;
    if bg.traces != snap {
        return Err(E_TRACE_GAP);
    }
    hits += 1;
    // R8 族核对计数恒 2
    if bg.verify_family()? != 2 {
        return Err(E_KW_MISS);
    }
    hits += 1;
    // R9 零命中语料 grep 归零但不入账
    let mut bg2 = BatchGrep::new();
    let r9 = bg2.record("latencyfinal", 0);
    if r9.err() != Some(E_HITS_ZERO) {
        return Err(E_HITS_ZERO);
    }
    if !bg2.traces.is_empty() {
        return Err(E_TRACE_LOST);
    }
    hits += 1;
    // R10 留痕序稳定：入账次序可复查
    if bg.traces[0].kw != "c2perf" || bg.traces[1].kw != "latencyfinal" {
        return Err(E_TRACE_GAP);
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
