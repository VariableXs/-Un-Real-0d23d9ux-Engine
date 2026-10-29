//! UNX-F48611 —— I31 观测埋点（录制/对照/差分/复签四点）
//! 判据原文：UNX-F48611-J1 四点 100%；判据锚定 体验十三·补。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           四埋点会话覆盖机检 100%。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0380; // I31 观测埋点诊断段

const E_POINT_MISS: u16 = SEG_BASE + 1;
const E_COVER_LOW: u16 = SEG_BASE + 2;
const E_DUP: u16 = SEG_BASE + 3;
const E_NAME_BAD: u16 = SEG_BASE + 4;
const E_ORDER_BAD: u16 = SEG_BASE + 5;
const E_LEDGER: u16 = SEG_BASE + 6;
const E_REPLAY: u16 = SEG_BASE + 7;

/// 埋点四点位（I31 面）。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Point {
    Record = 0,    // 录制点
    Compare = 1,   // 对照点
    Diff = 2,      // 差分点
    Resign = 3,    // 复签点
}

pub const POINT_COUNT: usize = 4;

impl Point {
    pub fn from_index(i: usize) -> Result<Point, u16> {
        match i {
            0 => Ok(Point::Record),
            1 => Ok(Point::Compare),
            2 => Ok(Point::Diff),
            3 => Ok(Point::Resign),
            _ => Err(E_NAME_BAD),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Point::Record => "record",
            Point::Compare => "compare",
            Point::Diff => "diff",
            Point::Resign => "resign",
        }
    }
}

/// 单埋点计数器。
pub struct PointCounter {
    pub point: Point,
    pub fired: u32,
}

impl PointCounter {
    pub fn new(point: Point) -> Self {
        PointCounter { point, fired: 0 }
    }
    pub fn fire(&mut self) {
        self.fired += 1;
    }
}

/// 观测会话。
pub struct TraceSession {
    pub label: String,
    pub counters: [PointCounter; POINT_COUNT],
    pub timeline: Vec<u8>,
}

impl TraceSession {
    pub fn new(label: &str) -> Self {
        TraceSession {
            label: label.to_string(),
            counters: [
                PointCounter::new(Point::Record),
                PointCounter::new(Point::Compare),
                PointCounter::new(Point::Diff),
                PointCounter::new(Point::Resign),
            ],
            timeline: Vec::new(),
        }
    }

    pub fn fire(&mut self, p: Point) {
        let idx = p as usize;
        self.counters[idx].fire();
        self.timeline.push(idx as u8);
    }

    pub fn coverage_ok(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < POINT_COUNT {
            if self.counters[i].fired == 0 {
                return Err(E_POINT_MISS);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn audit(&self) -> Result<(), u16> {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < POINT_COUNT {
            sum += self.counters[i].fired;
            i += 1;
        }
        if sum != self.timeline.len() as u32 {
            return Err(E_LEDGER);
        }
        Ok(())
    }

    pub fn timeline_valid(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.timeline.len() {
            if self.timeline[i] as usize >= POINT_COUNT {
                return Err(E_ORDER_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < POINT_COUNT {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.counters[i].fired as u64;
            i += 1;
        }
        i = 0;
        while i < self.timeline.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.timeline[i] as u64;
            i += 1;
        }
        h
    }
}

/// 标准会话：录制 → 对照 → 差分 → 复签。
pub fn run_standard_session(label: &str) -> Result<TraceSession, u16> {
    let mut s = TraceSession::new(label);
    s.fire(Point::Record);
    s.fire(Point::Compare);
    s.fire(Point::Diff);
    s.fire(Point::Resign);
    s.coverage_ok()?;
    s.audit()?;
    s.timeline_valid()?;
    Ok(s)
}

/// 确定性 LCG。
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

/// 主巡检：I31 观测埋点十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 标准会话：四点全触发覆盖率 100%。
    let s1 = run_standard_session("std-1")?;
    let mut i = 0usize;
    while i < POINT_COUNT {
        if s1.counters[i].fired != 1 {
            return Err(E_COVER_LOW);
        }
        i += 1;
    }
    hits += 1;

    // R2 埋点账守恒。
    s1.audit()?;
    if s1.timeline.len() != 4 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3 缺点反例：缺差分点覆盖率机检须红。
    let mut s2 = TraceSession::new("miss-diff");
    s2.fire(Point::Record);
    s2.fire(Point::Compare);
    s2.fire(Point::Resign);
    let r = s2.coverage_ok();
    if r.err() != Some(E_POINT_MISS) {
        return Err(E_POINT_MISS);
    }
    hits += 1;

    // R4 点位索引封闭：0..3 可取，4 越界须拒。
    let mut i = 0usize;
    while i < POINT_COUNT {
        Point::from_index(i)?;
        i += 1;
    }
    let r = Point::from_index(4);
    if r.err() != Some(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    hits += 1;

    // R5 会话 ×10：十轮标准会话全 100%。
    let mut r = 0u32;
    while r < 10 {
        let s = run_standard_session("std-r")?;
        if s.timeline.len() != 4 {
            return Err(E_LEDGER);
        }
        r += 1;
    }
    hits += 1;

    // R6 fuzz ×256 随机触发：账守恒 + 覆盖口径一致。
    let mut rng = Lcg::new(0x48611_0001);
    let mut s3 = TraceSession::new("fuzz");
    let mut k = 0u32;
    while k < 256 {
        let p = Point::from_index(rng.below(POINT_COUNT as u64) as usize)?;
        s3.fire(p);
        k += 1;
    }
    s3.audit()?;
    s3.timeline_valid()?;
    let mut covered = 0u32;
    let mut i = 0usize;
    while i < POINT_COUNT {
        if s3.counters[i].fired > 0 {
            covered += 1;
        }
        i += 1;
    }
    if s3.coverage_ok().is_ok() != (covered == POINT_COUNT as u32) {
        return Err(E_COVER_LOW);
    }
    hits += 1;

    // R7 补触发达标：未覆盖会话补触发后转 100%。
    let mut s4 = TraceSession::new("patch");
    s4.fire(Point::Compare);
    let mut i = 0usize;
    while i < POINT_COUNT {
        let p = Point::from_index(i)?;
        if s4.counters[i].fired == 0 {
            s4.fire(p);
        }
        i += 1;
    }
    s4.coverage_ok()?;
    hits += 1;

    // R8 会话级守恒：16 轮标准会话总触发 64。
    let mut total = 0u32;
    let mut r = 0u32;
    while r < 16 {
        let s = run_standard_session("std-16")?;
        total += s.timeline.len() as u32;
        r += 1;
    }
    if total != 64 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R9 时序序封闭：标准会话时序 0,1,2,3。
    if s1.timeline[0] != 0
        || s1.timeline[1] != 1
        || s1.timeline[2] != 2
        || s1.timeline[3] != 3
    {
        return Err(E_ORDER_BAD);
    }
    hits += 1;

    // R10 重演一致：同种子重建 fuzz 会话指纹相等。
    let fp1 = s3.fingerprint();
    let mut rng2 = Lcg::new(0x48611_0001);
    let mut s5 = TraceSession::new("fuzz");
    let mut k = 0u32;
    while k < 256 {
        let p = Point::from_index(rng2.below(POINT_COUNT as u64) as usize)?;
        s5.fire(p);
        k += 1;
    }
    if s5.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
