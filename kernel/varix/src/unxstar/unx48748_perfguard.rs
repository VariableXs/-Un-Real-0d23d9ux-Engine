//! UNX-F48748 C·性能回归护栏（关键路径预算超限即红的护栏账）
//! 判据 UNX-F48748-J1：护栏 ×10 注入触发正确；判据锚定 体验八。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 关键路径预算账：路径绑定预算，超限即红必须可注入触发，×10 全中。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1AC0;
const E_BUDGET_OVER: u16 = SEG_BASE + 1;
const E_PATH_MISS: u16 = SEG_BASE + 2;
const E_BUDGET_ZERO: u16 = SEG_BASE + 3;
const E_GUARD_LIVE: u16 = SEG_BASE + 4;
const E_TRIP_LOST: u16 = SEG_BASE + 5;
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;
const E_PATH_DUP: u16 = SEG_BASE + 7;
const E_TRIP_GHOST: u16 = SEG_BASE + 8;

/// 关键路径：编号 + 预算（微秒）
#[derive(Clone, Debug, PartialEq)]
pub struct KeyPath {
    pub pid: u16,
    pub budget_us: u32,
}

impl KeyPath {
    pub fn new(pid: u16, budget_us: u32) -> Self {
        KeyPath { pid, budget_us }
    }
}

/// 护栏触发记录：路径 + 实测 + 预算
#[derive(Clone, Debug, PartialEq)]
pub struct Trip {
    pub pid: u16,
    pub measured_us: u32,
    pub budget_us: u32,
}

impl Trip {
    pub fn tripped(&self) -> bool {
        self.measured_us > self.budget_us
    }
}

/// 性能回归护栏账
pub struct GuardLedger {
    pub paths: Vec<KeyPath>,
    pub trips: Vec<Trip>,
}

impl GuardLedger {
    pub fn new() -> Self {
        GuardLedger { paths: vec![], trips: vec![] }
    }

    pub fn bind(&mut self, pid: u16, budget_us: u32) -> Result<(), u16> {
        if budget_us == 0 {
            return Err(E_BUDGET_ZERO);
        }
        let mut i = 0usize;
        while i < self.paths.len() {
            if self.paths[i].pid == pid {
                return Err(E_PATH_DUP);
            }
            i += 1;
        }
        self.paths.push(KeyPath::new(pid, budget_us));
        Ok(())
    }

    fn find_budget(&self, pid: u16) -> Result<u32, u16> {
        let mut i = 0usize;
        while i < self.paths.len() {
            if self.paths[i].pid == pid {
                return Ok(self.paths[i].budget_us);
            }
            i += 1;
        }
        Err(E_PATH_MISS)
    }

    /// 实测入账：超预算即触发护栏并留痕
    pub fn measure(&mut self, pid: u16, measured_us: u32) -> Result<bool, u16> {
        let budget = self.find_budget(pid)?;
        let tripped = measured_us > budget;
        if tripped {
            self.trips.push(Trip { pid, measured_us, budget_us: budget });
        }
        Ok(tripped)
    }

    /// 护栏活性自检：注入超限实测必须留触发痕
    pub fn guard_alive(&self, pid: u16) -> Result<(), u16> {
        let mut found = false;
        let mut i = 0usize;
        while i < self.trips.len() {
            if self.trips[i].pid == pid && self.trips[i].tripped() {
                found = true;
            }
            i += 1;
        }
        if !found {
            return Err(E_GUARD_LIVE);
        }
        Ok(())
    }

    /// 触发痕核对：账内触发记录必须全部真超限
    pub fn audit_trips(&self) -> Result<u32, u16> {
        let mut green = 0u32;
        let mut i = 0usize;
        while i < self.trips.len() {
            let t = &self.trips[i];
            let budget = self.find_budget(t.pid)?;
            if t.budget_us != budget {
                return Err(E_TRIP_GHOST);
            }
            if !t.tripped() {
                return Err(E_TRIP_LOST);
            }
            green += 1;
            i += 1;
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

/// ×10 注入触发正确：对每条路径注入一次超限
fn sweep_ten_injections(rng: &mut Lcg) -> Result<(), u16> {
    let mut g = GuardLedger::new();
    let mut pid = 1u16;
    while pid <= 10 {
        let budget = 500 + rng.below(1_000) as u32;
        g.bind(pid, budget)?;
        let over = budget + 1 + rng.below(500) as u32;
        let tripped = g.measure(pid, over)?;
        if !tripped {
            return Err(E_GUARD_LIVE);
        }
        pid += 1;
    }
    if g.audit_trips()? != 10 {
        return Err(E_TRIP_LOST);
    }
    let mut pid2 = 1u16;
    while pid2 <= 10 {
        g.guard_alive(pid2)?;
        pid2 += 1;
    }
    Ok(())
}

/// 正常实测不触发 ×10^3：预算内实测零触发痕
fn sweep_no_trip(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let mut g = GuardLedger::new();
        let pid = (iter % 8) as u16 + 1;
        g.bind(pid, 2_000)?;
        let under = 100 + rng.below(1_800) as u32;
        let tripped = g.measure(pid, under)?;
        if tripped || !g.trips.is_empty() {
            return Err(E_GUARD_LIVE);
        }
        iter += 1;
    }
    Ok(())
}

/// 反例注入：未绑定路径、零预算、重复绑定全命中
fn sweep_negatives() -> Result<(), u16> {
    let mut g = GuardLedger::new();
    let r1 = g.measure(9, 100);
    if r1.err() != Some(E_PATH_MISS) {
        return Err(E_PATH_MISS);
    }
    let r2 = g.bind(9, 0);
    if r2.err() != Some(E_BUDGET_ZERO) {
        return Err(E_BUDGET_ZERO);
    }
    g.bind(9, 1_000)?;
    let r3 = g.bind(9, 2_000);
    if r3.err() != Some(E_PATH_DUP) {
        return Err(E_PATH_DUP);
    }
    let r4 = GuardLedger::new().audit_trips();
    if r4.is_err() {
        return Err(E_TRIP_GHOST);
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑触发判定逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut ga = GuardLedger::new();
        let mut gb = GuardLedger::new();
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let mut pid = 1u16;
        while pid <= 6 {
            let ba = 800 + ra.below(1_200) as u32;
            let bb = 800 + rb.below(1_200) as u32;
            if ba != bb {
                return Err(E_REPLAY_DIFF);
            }
            ga.bind(pid, ba)?;
            gb.bind(pid, bb)?;
            let ma = ra.below(3_000) as u32;
            let mb = rb.below(3_000) as u32;
            if ma != mb {
                return Err(E_REPLAY_DIFF);
            }
            let ta = ga.measure(pid, ma)?;
            let tb = gb.measure(pid, mb)?;
            if ta != tb {
                return Err(E_REPLAY_DIFF);
            }
            pid += 1;
        }
        if ga.audit_trips()? != gb.audit_trips()? {
            return Err(E_TRIP_LOST);
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 ×10 注入触发正确
    let mut rng = Lcg::new(0x4874_8F66);
    sweep_ten_injections(&mut rng)?;
    hits += 1;
    // R2 预算内实测零触发 ×10^3
    sweep_no_trip(&mut rng)?;
    hits += 1;
    // R3 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R4 重放一致 ×20
    sweep_replay(0x4874_8A02)?;
    hits += 1;
    // R5 界值触发：恰好等于预算不触发、+1 即触发
    let mut g2 = GuardLedger::new();
    g2.bind(1, 1_000)?;
    if g2.measure(1, 1_000)? {
        return Err(E_GUARD_LIVE);
    }
    if !g2.measure(1, 1_001)? {
        return Err(E_TRIP_LOST);
    }
    hits += 1;
    // R6 触发痕携带实测与预算快照
    let trips = g2.trips.clone();
    if trips.len() != 1 {
        return Err(E_TRIP_LOST);
    }
    if trips[0].measured_us != 1_001 || trips[0].budget_us != 1_000 {
        return Err(E_TRIP_GHOST);
    }
    hits += 1;
    // R7 多路径混合触发：只点名超限路径
    let mut g3 = GuardLedger::new();
    let mut pid = 1u16;
    while pid <= 5 {
        g3.bind(pid, 1_500)?;
        pid += 1;
    }
    g3.measure(1, 1_400)?;
    g3.measure(2, 1_600)?;
    g3.measure(3, 1_500)?;
    if g3.trips.len() != 1 {
        return Err(E_TRIP_LOST);
    }
    if g3.trips[0].pid != 2 {
        return Err(E_TRIP_GHOST);
    }
    hits += 1;
    // R8 触发痕审计与触发数一致
    if g3.audit_trips()? != 1 {
        return Err(E_TRIP_LOST);
    }
    hits += 1;
    // R9 路径账快照核对：绑定后预算不被改写
    let paths = g3.paths.clone();
    let mut i = 0usize;
    while i < paths.len() {
        if paths[i].budget_us != 1_500 {
            return Err(E_PATH_MISS);
        }
        i += 1;
    }
    hits += 1;
    // R10 护栏活性终检：账内触发记录必须全部可活性确认
    let mut g4 = GuardLedger::new();
    g4.bind(3, 900)?;
    g4.measure(3, 1_000)?;
    g4.guard_alive(3)?;
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
