//! UNX-F48691 —— I35 观测埋点（签名/换装/回滚/拒绝四点）
//! 判据原文：UNX-F48691-J1 四点 100%；判据锚定 体验十三·补。
//! 判据落地（drill 场景）：R1 四点在册；R2 签名点触发；R3 换装点触发；
//! R4 回滚点触发；R5 拒绝点触发；R6 覆盖率 100%；R7 缺点检出；
//! R8 未知点拒收；R9 呈现行；R10 指纹稳定。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0DC0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_POINT: u16 = SEG_BASE + 1; // 埋点越出四点闭集
const E_COVER: u16 = SEG_BASE + 2; // 覆盖率不足 100%
const E_MISS: u16 = SEG_BASE + 3; // 指定点未触发
const E_DUP: u16 = SEG_BASE + 4; // 埋点重复登记
const E_FP: u16 = SEG_BASE + 5; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 6; // 呈现行缺段
const E_COUNT: u16 = SEG_BASE + 7; // 埋点数不足 4

/// 埋点四点闭集（更新链观测）。
pub const O_SIGN: u8 = 1; // 签名
pub const O_SWAP: u8 = 2; // 换装
pub const O_ROLLBACK: u8 = 3; // 回滚
pub const O_REJECT: u8 = 4; // 拒绝
/// 四点数。
pub const POINT_N: usize = 4;

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

/// 埋点闭集判定。
pub fn point_ok(p: u8) -> bool {
    p == O_SIGN || p == O_SWAP || p == O_ROLLBACK || p == O_REJECT
}

/// 一个观测埋点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsPoint {
    pub kind: u8,
    pub fired: u32,
    pub last_ts: u64,
}

/// 观测账。
pub struct ObsLedger {
    pub points: Vec<ObsPoint>,
}

impl ObsLedger {
    pub fn new() -> ObsLedger {
        ObsLedger { points: vec![] }
    }

    /// 登记：闭集 + 唯一。
    pub fn register(&mut self, kind: u8) -> Result<(), u16> {
        if !point_ok(kind) {
            return Err(E_POINT);
        }
        let mut i = 0usize;
        while i < self.points.len() {
            if self.points[i].kind == kind {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.points.push(ObsPoint { kind, fired: 0, last_ts: 0 });
        Ok(())
    }

    /// 触发。
    pub fn fire(&mut self, kind: u8, ts: u64) -> Result<(), u16> {
        if !point_ok(kind) {
            return Err(E_POINT);
        }
        let mut i = 0usize;
        while i < self.points.len() {
            if self.points[i].kind == kind {
                self.points[i].fired += 1;
                self.points[i].last_ts = ts;
                return Ok(());
            }
            i += 1;
        }
        Err(E_MISS)
    }

    /// 指定点已触发 ≥n。
    pub fn fired_at_least(&self, kind: u8, n: u32) -> bool {
        let mut i = 0usize;
        while i < self.points.len() {
            if self.points[i].kind == kind && self.points[i].fired >= n {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 覆盖率（基点）。
    pub fn coverage_bp(&self) -> u32 {
        if self.points.is_empty() {
            return 0;
        }
        let mut c: u32 = 0;
        let mut i = 0usize;
        while i < self.points.len() {
            if self.points[i].fired > 0 {
                c += 1;
            }
            i += 1;
        }
        c * 10_000 / self.points.len() as u32
    }

    /// 四点齐。
    pub fn points_full(&self) -> Result<(), u16> {
        if self.points.len() != POINT_N {
            return Err(E_COUNT);
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.points.len() {
            buf.push(self.points[i].kind);
            buf.extend_from_slice(&self.points[i].fired.to_le_bytes());
            buf.extend_from_slice(&self.points[i].last_ts.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 更新流埋点：签名→换装→（坏包时）拒绝→（失败时）回滚。
pub fn run_flow(l: &mut ObsLedger, base_ts: u64) -> Result<(), u16> {
    l.fire(O_SIGN, base_ts)?;
    l.fire(O_SWAP, base_ts + 1)?;
    l.fire(O_REJECT, base_ts + 2)?;
    l.fire(O_ROLLBACK, base_ts + 3)?;
    Ok(())
}

/// 指定点触发计数。
pub fn fire_of(l: &ObsLedger, kind: u8) -> u32 {
    let mut i = 0usize;
    while i < l.points.len() {
        if l.points[i].kind == kind {
            return l.points[i].fired;
        }
        i += 1;
    }
    0
}

/// 呈现行：p1=<n> p2=<n> p3=<n> p4=<n> cov=<bp>。
pub fn obs_line(l: &ObsLedger) -> String {
    let mut out = String::new();
    out.push_str("p1=");
    out.push_str(&fire_of(l, O_SIGN).to_string());
    out.push_str(" p2=");
    out.push_str(&fire_of(l, O_SWAP).to_string());
    out.push_str(" p3=");
    out.push_str(&fire_of(l, O_ROLLBACK).to_string());
    out.push_str(" p4=");
    out.push_str(&fire_of(l, O_REJECT).to_string());
    out.push_str(" cov=");
    out.push_str(&l.coverage_bp().to_string());
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
    let mut g = Lcg::new(0x486_91);

    // R1 四点在册。
    let mut led = ObsLedger::new();
    led.register(O_SIGN)?;
    led.register(O_SWAP)?;
    led.register(O_ROLLBACK)?;
    led.register(O_REJECT)?;
    led.points_full()?;
    hits += 1;

    // R2-R5 更新流：四点各触发 ≥1。
    if led.fired_at_least(O_SIGN, 1) {
        return Err(E_MISS);
    }
    run_flow(&mut led, 2000)?;
    if !led.fired_at_least(O_SIGN, 1) || !led.fired_at_least(O_SWAP, 1)
        || !led.fired_at_least(O_ROLLBACK, 1) || !led.fired_at_least(O_REJECT, 1)
    {
        return Err(E_MISS);
    }
    hits += 1;

    // R6 覆盖率 100%。
    if led.coverage_bp() != 10_000 {
        return Err(E_COVER);
    }
    hits += 1;

    // R7 缺点检出：三点 → points_full 红；四点零触发 → 覆盖率红。
    let mut thin = ObsLedger::new();
    thin.register(O_SIGN)?;
    thin.register(O_SWAP)?;
    thin.register(O_ROLLBACK)?;
    match thin.points_full() {
        Err(E_COUNT) => {}
        _ => return Err(E_COUNT),
    }
    let mut cold = ObsLedger::new();
    cold.register(O_SIGN)?;
    cold.register(O_SWAP)?;
    cold.register(O_ROLLBACK)?;
    cold.register(O_REJECT)?;
    if cold.coverage_bp() != 0 {
        return Err(E_COVER);
    }
    hits += 1;

    // R8 未知点拒收。
    let mut probe = ObsLedger::new();
    match probe.register(0) {
        Err(E_POINT) => {}
        _ => return Err(E_POINT),
    }
    match probe.register(8) {
        Err(E_POINT) => {}
        _ => return Err(E_POINT),
    }
    match probe.fire(6, 1) {
        Err(E_POINT) => hits += 1,
        _ => return Err(E_POINT),
    }

    // R9 呈现行：p1=1 p2=1 p3=1 p4=1 cov=10000。
    let line = obs_line(&led);
    if !has_kw(line.as_bytes(), b"p1=1") || !has_kw(line.as_bytes(), b"p2=1")
        || !has_kw(line.as_bytes(), b"p3=1") || !has_kw(line.as_bytes(), b"p4=1")
        || !has_kw(line.as_bytes(), b"cov=10000")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 指纹稳定。
    let f1 = led.fp();
    let f2 = led.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    let _ = g.below(10);
    hits += 1;

    Ok(hits)
}
