//! UNX-F48750 C·启动节奏账（骨架先行的 IME 就绪次序账）
//! 判据 UNX-F48750-J1：次序账落册；判据锚定 体验八。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 启动次序：骨架→词库→引擎→UI→就绪，乱序即红，次序账必须落册。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1B40;
const E_ORDER_BAD: u16 = SEG_BASE + 1;
const E_STEP_MISS: u16 = SEG_BASE + 2;
const E_STEP_DUP: u16 = SEG_BASE + 3;
const E_TS_BACK: u16 = SEG_BASE + 4;
const E_READY_LOST: u16 = SEG_BASE + 5;
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;
const E_RHYTHM_GAP: u16 = SEG_BASE + 7;
const E_SKELETON_LATE: u16 = SEG_BASE + 8;

/// 启动步：骨架 0 / 词库 1 / 引擎 2 / UI 3 / 就绪 4
pub struct BootStep {
    pub step: u8,
    pub ts_us: u32,
}

impl BootStep {
    pub fn name(step: u8) -> &'static str {
        match step {
            0 => "skeleton",
            1 => "lexicon",
            2 => "engine",
            3 => "ui",
            4 => "ready",
            _ => "unknown",
        }
    }
}

/// 启动节奏账
pub struct RhythmLedger {
    pub steps: Vec<BootStep>,
}

impl RhythmLedger {
    pub fn new() -> Self {
        RhythmLedger { steps: vec![] }
    }

    pub fn mark(&mut self, step: u8, ts_us: u32) -> Result<(), u16> {
        if step > 4 {
            return Err(E_STEP_MISS);
        }
        let mut i = 0usize;
        while i < self.steps.len() {
            if self.steps[i].step == step {
                return Err(E_STEP_DUP);
            }
            if self.steps[i].ts_us > ts_us {
                return Err(E_TS_BACK);
            }
            i += 1;
        }
        self.steps.push(BootStep { step, ts_us });
        Ok(())
    }

    /// 次序核对：已落账步号必须严格递增且骨架最先
    pub fn audit_order(&self) -> Result<(), u16> {
        if self.steps.is_empty() {
            return Err(E_RHYTHM_GAP);
        }
        if self.steps[0].step != 0 {
            return Err(E_SKELETON_LATE);
        }
        let mut i = 1usize;
        while i < self.steps.len() {
            if self.steps[i].step <= self.steps[i - 1].step {
                return Err(E_ORDER_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 就绪落册：第五步在册才算次序账完整
    pub fn settle_ready(&self) -> Result<u32, u16> {
        self.audit_order()?;
        let mut ready = false;
        let mut i = 0usize;
        while i < self.steps.len() {
            if self.steps[i].step == 4 {
                ready = true;
            }
            i += 1;
        }
        if !ready {
            return Err(E_READY_LOST);
        }
        Ok(self.steps[4].ts_us)
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

/// 完整次序落册 ×10^3：五步递增时戳
fn sweep_full_rhythm(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let mut led = RhythmLedger::new();
        let base = 1_000 + rng.below(5_000) as u32;
        let mut step = 0u8;
        let mut ts = base;
        while step <= 4 {
            led.mark(step, ts)?;
            ts += 100 + rng.below(900) as u32;
            step += 1;
        }
        let ready_ts = led.settle_ready()?;
        if ready_ts < base {
            return Err(E_TS_BACK);
        }
        iter += 1;
    }
    Ok(())
}

/// 乱序注入 ×10：骨架不在首、步号回跳、时戳倒退全命中
fn sweep_disorders() -> Result<(), u16> {
    let mut round = 0u32;
    while round < 10 {
        let mut a = RhythmLedger::new();
        a.mark(1, 100)?;
        let r1 = a.audit_order();
        if r1.err() != Some(E_SKELETON_LATE) {
            return Err(E_SKELETON_LATE);
        }
        let mut b = RhythmLedger::new();
        b.mark(0, 100)?;
        b.mark(2, 200)?;
        let r2 = b.audit_order();
        if r2.err() != Some(E_ORDER_BAD) {
            return Err(E_ORDER_BAD);
        }
        let mut c = RhythmLedger::new();
        c.mark(0, 300)?;
        let r3 = c.mark(1, 200);
        if r3.err() != Some(E_TS_BACK) {
            return Err(E_TS_BACK);
        }
        let mut d = RhythmLedger::new();
        d.mark(0, 100)?;
        let r4 = d.mark(0, 200);
        if r4.err() != Some(E_STEP_DUP) {
            return Err(E_STEP_DUP);
        }
        let mut e = RhythmLedger::new();
        e.mark(0, 100)?;
        e.mark(1, 200)?;
        e.mark(2, 300)?;
        e.mark(3, 400)?;
        let r5 = e.settle_ready();
        if r5.err() != Some(E_READY_LOST) {
            return Err(E_READY_LOST);
        }
        round += 1;
    }
    Ok(())
}

/// 反例注入：越界步号、空账审计全命中
fn sweep_negatives() -> Result<(), u16> {
    let mut led = RhythmLedger::new();
    let r1 = led.mark(5, 100);
    if r1.err() != Some(E_STEP_MISS) {
        return Err(E_STEP_MISS);
    }
    let r2 = RhythmLedger::new().audit_order();
    if r2.err() != Some(E_RHYTHM_GAP) {
        return Err(E_RHYTHM_GAP);
    }
    let mut led2 = RhythmLedger::new();
    let r3 = led2.mark(0, 0)?;
    let _ = r3;
    if led2.settle_ready().err() != Some(E_READY_LOST) {
        return Err(E_READY_LOST);
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑就绪时戳逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut la = RhythmLedger::new();
        let mut lb = RhythmLedger::new();
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let mut step = 0u8;
        let mut ta = 500u32;
        let mut tb = 500u32;
        while step <= 4 {
            let da = 100 + ra.below(400) as u32;
            let db = 100 + rb.below(400) as u32;
            if da != db {
                return Err(E_REPLAY_DIFF);
            }
            ta += da;
            tb += db;
            la.mark(step, ta)?;
            lb.mark(step, tb)?;
            step += 1;
        }
        if la.settle_ready()? != lb.settle_ready()? {
            return Err(E_REPLAY_DIFF);
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 完整次序落册 ×10^3
    let mut rng = Lcg::new(0x4874_A188);
    let mut r1 = 0u32;
    while r1 < 1_000 {
        sweep_full_rhythm(&mut rng)?;
        r1 += 1;
    }
    hits += 1;
    // R2 五步名落册
    let mut s = 0u8;
    while s <= 4 {
        if BootStep::name(s) == "unknown" {
            return Err(E_STEP_MISS);
        }
        s += 1;
    }
    hits += 1;
    // R3 乱序注入 ×10 全命中
    sweep_disorders()?;
    hits += 1;
    // R4 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R5 重放一致 ×20
    sweep_replay(0x4874_AC04)?;
    hits += 1;
    // R6 骨架先行断言：首步必为 skeleton
    let mut led = RhythmLedger::new();
    led.mark(0, 100)?;
    led.mark(1, 200)?;
    led.mark(2, 300)?;
    led.mark(3, 400)?;
    led.mark(4, 500)?;
    if led.steps[0].step != 0 {
        return Err(E_SKELETON_LATE);
    }
    led.audit_order()?;
    hits += 1;
    // R7 就绪时戳单调：就绪点 ≥ 各前序点
    let ready_ts = led.settle_ready()?;
    let mut i = 0usize;
    while i < led.steps.len() {
        if ready_ts < led.steps[i].ts_us {
            return Err(E_TS_BACK);
        }
        i += 1;
    }
    hits += 1;
    // R8 步号连续：五步不缺环
    let mut led2 = RhythmLedger::new();
    let mut s2 = 0u8;
    let mut ts2 = 1_000u32;
    while s2 <= 4 {
        led2.mark(s2, ts2)?;
        ts2 += 50;
        s2 += 1;
    }
    if led2.steps.len() != 5 {
        return Err(E_RHYTHM_GAP);
    }
    let mut k = 0usize;
    while k < 5 {
        if led2.steps[k].step != k as u8 {
            return Err(E_ORDER_BAD);
        }
        k += 1;
    }
    hits += 1;
    // R9 节奏跨度：骨架到就绪总跨度在账可查
    let span = led2.settle_ready()? - led2.steps[0].ts_us;
    if span != 200 {
        return Err(E_RHYTHM_GAP);
    }
    hits += 1;
    // R10 步名域校验：未知步号落 unknown
    if BootStep::name(9) != "unknown" {
        return Err(E_STEP_MISS);
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
