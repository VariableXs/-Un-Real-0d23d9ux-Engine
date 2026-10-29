//! UNX-F48592 —— I30 观测埋点（复签/对账/衔接/通知四点）
//! 判据原文：UNX-F48592-J1 四点 100%；判据锚定 体验十三·补。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           四埋点会话内全覆盖，覆盖率机检 100%。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0120; // I30 观测埋点诊断段

const E_POINT_MISS: u16 = SEG_BASE + 1;  // 埋点缺失
const E_COVER_LOW: u16 = SEG_BASE + 2;   // 覆盖率不足
const E_DUP: u16 = SEG_BASE + 3;         // 埋点重复注册
const E_NAME_BAD: u16 = SEG_BASE + 4;    // 埋点名非法
const E_ORDER_BAD: u16 = SEG_BASE + 5;   // 埋点时序违例
const E_LEDGER: u16 = SEG_BASE + 6;      // 埋点账不守恒
const E_REPLAY: u16 = SEG_BASE + 7;      // 重演不一致

/// 埋点四点位。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Point {
    Resign = 0,   // 复签点
    Reconcile = 1, // 对账点
    Bridge = 2,   // 衔接点
    Notify = 3,   // 通知点
}

pub const POINT_COUNT: usize = 4;

impl Point {
    pub fn from_index(i: usize) -> Result<Point, u16> {
        match i {
            0 => Ok(Point::Resign),
            1 => Ok(Point::Reconcile),
            2 => Ok(Point::Bridge),
            3 => Ok(Point::Notify),
            _ => Err(E_NAME_BAD),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Point::Resign => "resign",
            Point::Reconcile => "reconcile",
            Point::Bridge => "bridge",
            Point::Notify => "notify",
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

/// 观测埋点会话：一轮业务里的四点埋点。
pub struct TraceSession {
    pub label: String,
    pub counters: [PointCounter; POINT_COUNT],
    /// 埋点触发时序账：point 索引序列。
    pub timeline: Vec<u8>,
}

impl TraceSession {
    pub fn new(label: &str) -> Self {
        TraceSession {
            label: label.to_string(),
            counters: [
                PointCounter::new(Point::Resign),
                PointCounter::new(Point::Reconcile),
                PointCounter::new(Point::Bridge),
                PointCounter::new(Point::Notify),
            ],
            timeline: Vec::new(),
        }
    }

    /// 触发一个埋点。
    pub fn fire(&mut self, p: Point) {
        let idx = p as usize;
        self.counters[idx].fire();
        self.timeline.push(idx as u8);
    }

    /// 覆盖率审计：四点全部至少触发一次。
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

    /// 埋点账守恒：timeline 长度 == 四点计数和。
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

    /// 时序审计：合法埋点索引才可入时序账。
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

/// 标准会话：复签 → 对账 → 衔接 → 通知 顺序各触发一次。
pub fn run_standard_session(label: &str) -> Result<TraceSession, u16> {
    let mut s = TraceSession::new(label);
    s.fire(Point::Resign);
    s.fire(Point::Reconcile);
    s.fire(Point::Bridge);
    s.fire(Point::Notify);
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

/// 主巡检：I30 观测埋点十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 标准会话：四点按序全触发，覆盖率 100%。
    let s1 = run_standard_session("std-1")?;
    if s1.counters[0].fired != 1
        || s1.counters[1].fired != 1
        || s1.counters[2].fired != 1
        || s1.counters[3].fired != 1
    {
        return Err(E_COVER_LOW);
    }
    hits += 1;

    // R2 埋点账守恒：timeline 长度 4。
    s1.audit()?;
    if s1.timeline.len() != 4 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3 缺点反例：仅触发三点覆盖率机检须红。
    let mut s2 = TraceSession::new("miss-notify");
    s2.fire(Point::Resign);
    s2.fire(Point::Reconcile);
    s2.fire(Point::Bridge);
    let r = s2.coverage_ok();
    if r.err() != Some(E_POINT_MISS) {
        return Err(E_POINT_MISS);
    }
    hits += 1;

    // R4 点位索引合法：0..3 全可取，4 越界须拒。
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

    // R5 会话 ×10：十轮标准会话全部 100% 覆盖。
    let mut r = 0u32;
    while r < 10 {
        let s = run_standard_session("std-r")?;
        if s.timeline.len() != 4 {
            return Err(E_LEDGER);
        }
        r += 1;
    }
    hits += 1;

    // R6 fuzz ×256 随机触发序列：账守恒、时序合法、覆盖率机检口径一致。
    let mut rng = Lcg::new(0x48592_0001);
    let mut s3 = TraceSession::new("fuzz");
    let mut k = 0u32;
    while k < 256 {
        let p = Point::from_index(rng.below(POINT_COUNT as u64) as usize)?;
        s3.fire(p);
        k += 1;
    }
    s3.audit()?;
    s3.timeline_valid()?;
    // 独立复算覆盖率。
    let mut covered = 0u32;
    let mut i = 0usize;
    while i < POINT_COUNT {
        if s3.counters[i].fired > 0 {
            covered += 1;
        }
        i += 1;
    }
    let cov_ok = s3.coverage_ok().is_ok();
    if cov_ok != (covered == POINT_COUNT as u32) {
        return Err(E_COVER_LOW);
    }
    hits += 1;

    // R7 会话 ×20 补触发达标：对未覆盖会话补触发后覆盖率转 100%。
    let mut s4 = TraceSession::new("patch");
    s4.fire(Point::Bridge);
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

    // R8 会话级守恒：20 轮标准会话总触发数 == 20×4。
    let mut total = 0u32;
    let mut r = 0u32;
    while r < 20 {
        let s = run_standard_session("std-20")?;
        total += s.timeline.len() as u32;
        r += 1;
    }
    if total != 80 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R9 时序序封闭：标准会话时序恰为 0,1,2,3。
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
    let mut rng2 = Lcg::new(0x48592_0001);
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
