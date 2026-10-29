//! UNX-F48473 · E24 观测埋点（违例/断连/重连/限流四点）
//!
//! 判据原文：UNX-F48473-J1 —— 协议面四类观测点（违例/断连/重连/限流）
//! 全部接通：事件经统一 ObsBus 分发到各自 sink，计数单调递增、序号连续
//! 无跳号；四点互不串扰（A 点事件不落 B 槽）；环形缓冲写满回绕不丢最近
//! 事件；埋点开关关闭时事件丢弃但计数仍走；全场景 drill 零偏差。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；事件结构
//! 定长字段；fuzz 用确定性 LCG（wrapping_mul/wrapping_add）。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xE3E0;

pub const E_SINK_OOB: u16 = SEG_BASE + 1;
pub const E_SEQ_GAP: u16 = SEG_BASE + 2;
pub const E_CROSS: u16 = SEG_BASE + 3;
pub const E_RING_LOST: u16 = SEG_BASE + 4;
pub const E_GATE_LEAK: u16 = SEG_BASE + 5;
pub const E_COUNT_STALL: u16 = SEG_BASE + 6;
pub const E_ORDER_BAD: u16 = SEG_BASE + 7;

/// 四类观测点。
pub const POINT_VIOLATION: usize = 0;
pub const POINT_DISCONNECT: usize = 1;
pub const POINT_RECONNECT: usize = 2;
pub const POINT_THROTTLE: usize = 3;
pub const POINT_N: usize = 4;

/// 环形缓冲容量。
pub const RING_CAP: usize = 16;

/// 观测事件。
#[derive(Clone, PartialEq, Eq)]
pub struct ObsEvent {
    pub point: usize,
    pub seq: u32,
    pub tick: u64,
    pub detail: u32,
}

impl ObsEvent {
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.point as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.seq as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.tick;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.detail as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }
    pub fn label(&self) -> String {
        let base = match self.point {
            POINT_VIOLATION => "violation",
            POINT_DISCONNECT => "disconnect",
            POINT_RECONNECT => "reconnect",
            POINT_THROTTLE => "throttle",
            _ => "unknown",
        };
        let mut s = String::from(base);
        s.push('#');
        let mut n = self.seq;
        let mut digits = Vec::new();
        if n == 0 {
            digits.push(b'0');
        }
        while n > 0 {
            digits.push(b'0' + (n % 10) as u8);
            n /= 10;
        }
        let mut i = digits.len();
        while i > 0 {
            i -= 1;
            s.push(digits[i] as char);
        }
        s
    }
}

/// 环形缓冲 sink：写满回绕，保留最近 RING_CAP 条。
pub struct RingSink {
    buf: Vec<ObsEvent>,
    head: usize,
    pub total: u64,
}

impl RingSink {
    pub fn new() -> RingSink {
        RingSink { buf: Vec::new(), head: 0, total: 0 }
    }
    pub fn push(&mut self, ev: ObsEvent) {
        if self.buf.len() < RING_CAP {
            self.buf.push(ev);
        } else {
            self.buf[self.head] = ev;
        }
        self.head = (self.head + 1) % RING_CAP;
        self.total += 1;
    }
    /// 时间正序读取：从最旧开始。
    pub fn read_oldest_first(&self) -> Vec<ObsEvent> {
        let mut out = Vec::new();
        let n = self.buf.len();
        if n < RING_CAP {
            for i in 0..n {
                out.push(self.buf[i].clone());
            }
            return out;
        }
        for i in 0..n {
            out.push(self.buf[(self.head + i) % RING_CAP].clone());
        }
        out
    }
    pub fn len(&self) -> usize {
        self.buf.len()
    }
}

/// 观测总线：四点分发 + 总闸门。
pub struct ObsBus {
    pub enabled: bool,
    seq: u32,
    pub counters: [u64; POINT_N],
    pub sinks: [RingSink; POINT_N],
    pub last_tick: [u64; POINT_N],
}

impl ObsBus {
    pub fn new() -> ObsBus {
        ObsBus {
            enabled: true,
            seq: 0,
            counters: [0; POINT_N],
            sinks: [RingSink::new(), RingSink::new(), RingSink::new(), RingSink::new()],
            last_tick: [0; POINT_N],
        }
    }

    /// 发布事件：分配连续序号、计数、按闸门决定是否入环。
    pub fn publish(&mut self, point: usize, tick: u64, detail: u32) -> Result<u32, u16> {
        if point >= POINT_N {
            return Err(E_SINK_OOB);
        }
        self.seq += 1;
        let ev = ObsEvent { point, seq: self.seq, tick, detail };
        self.counters[point] += 1;
        self.last_tick[point] = tick;
        let out_seq = ev.seq;
        if self.enabled {
            self.sinks[point].push(ev);
        }
        Ok(out_seq)
    }

    /// 序号连续性：全局 seq 与各点计数之和一致。
    pub fn audit_seq(&self) -> Result<(), u16> {
        let sum: u64 = self.counters.iter().fold(0u64, |a, b| a + b);
        if sum != self.seq as u64 {
            return Err(E_SEQ_GAP);
        }
        Ok(())
    }

    /// 互不串扰：各 sink 内事件 point 字段必须等于槽位号。
    pub fn audit_isolation(&self) -> Result<(), u16> {
        for p in 0..POINT_N {
            for ev in self.sinks[p].read_oldest_first().iter() {
                if ev.point != p {
                    return Err(E_CROSS);
                }
            }
        }
        Ok(())
    }

    /// 计数单调：重读一遍 counters 不变（快照等值）。
    pub fn audit_count_stable(&self, snap: &[u64; POINT_N]) -> Result<(), u16> {
        for p in 0..POINT_N {
            if self.counters[p] < snap[p] {
                return Err(E_COUNT_STALL);
            }
        }
        Ok(())
    }
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// R1 四点接通：每点发布 3 事件，计数与序号单调。
pub fn r1_four_points_wired() -> Result<(), u16> {
    let mut bus = ObsBus::new();
    let mut tick: u64 = 100;
    for p in 0..POINT_N {
        for d in 0..3u32 {
            tick += 1;
            let s = bus.publish(p, tick, d)?;
            if (s as u64) != bus.counters.iter().fold(0u64, |a, b| a + b) {
                return Err(E_SEQ_GAP);
            }
        }
    }
    bus.audit_seq()?;
    for p in 0..POINT_N {
        if bus.counters[p] != 3 {
            return Err(E_COUNT_STALL);
        }
    }
    Ok(())
}

/// R2 序号连续无跳号：50 事件全收，seq 恰为 1..=50。
pub fn r2_seq_contiguous() -> Result<(), u16> {
    let mut bus = ObsBus::new();
    let mut g = Lcg::new(0x48473_0001);
    for _ in 0..50 {
        let p = g.below(POINT_N as u64) as usize;
        bus.publish(p, g.next(), (g.next() & 0xFF) as u32)?;
    }
    bus.audit_seq()?;
    if bus.seq != 50 {
        return Err(E_SEQ_GAP);
    }
    Ok(())
}

/// R3 互不串扰：fuzz 混发后各槽 point 字段全对位。
pub fn r3_no_cross() -> Result<(), u16> {
    let mut bus = ObsBus::new();
    let mut g = Lcg::new(0x48473_0002);
    for _ in 0..80 {
        let p = g.below(POINT_N as u64) as usize;
        bus.publish(p, g.next(), (g.next() & 0xFF) as u32)?;
    }
    bus.audit_isolation()?;
    Ok(())
}

/// R4 环形回绕：单点发 RING_CAP*3 条，环内保留最近 RING_CAP 条且序号最大。
pub fn r4_ring_wrap() -> Result<(), u16> {
    let mut bus = ObsBus::new();
    for i in 0..(RING_CAP as u32 * 3) {
        bus.publish(POINT_DISCONNECT, i as u64, i)?;
    }
    let ring = bus.sinks[POINT_DISCONNECT].read_oldest_first();
    if ring.len() != RING_CAP {
        return Err(E_RING_LOST);
    }
    let expect_start = RING_CAP as u32 * 3 - RING_CAP as u32 + 1;
    for i in 0..RING_CAP {
        if ring[i].detail != expect_start + i as u32 {
            return Err(E_RING_LOST);
        }
    }
    if bus.sinks[POINT_DISCONNECT].total != (RING_CAP as u32 * 3) as u64 {
        return Err(E_COUNT_STALL);
    }
    Ok(())
}

/// R5 闸门关闭：事件丢弃但计数与序号仍走，开闸后恢复入环。
pub fn r5_gate_discard() -> Result<(), u16> {
    let mut bus = ObsBus::new();
    bus.enabled = false;
    for _ in 0..5 {
        bus.publish(POINT_RECONNECT, 1, 0)?;
    }
    bus.audit_seq()?;
    if bus.sinks[POINT_RECONNECT].len() != 0 {
        return Err(E_GATE_LEAK);
    }
    if bus.counters[POINT_RECONNECT] != 5 {
        return Err(E_COUNT_STALL);
    }
    bus.enabled = true;
    bus.publish(POINT_RECONNECT, 2, 9)?;
    if bus.sinks[POINT_RECONNECT].len() != 1 {
        return Err(E_GATE_LEAK);
    }
    Ok(())
}

/// R6 标签渲染：label() 前缀对点、后缀为序号，四点互异。
pub fn r6_label_render() -> Result<(), u16> {
    let ev = ObsEvent { point: POINT_VIOLATION, seq: 42, tick: 0, detail: 0 };
    let s = ev.label();
    if s.len() < 11 {
        return Err(E_ORDER_BAD);
    }
    let head = &s[0..9];
    if head != "violation" {
        return Err(E_ORDER_BAD);
    }
    let tail = &s[10..];
    if tail != "42" {
        return Err(E_ORDER_BAD);
    }
    let e1 = ObsEvent { point: 0, seq: 1, tick: 0, detail: 0 };
    let e2 = ObsEvent { point: 1, seq: 1, tick: 0, detail: 0 };
    let e3 = ObsEvent { point: 2, seq: 1, tick: 0, detail: 0 };
    let e4 = ObsEvent { point: 3, seq: 1, tick: 0, detail: 0 };
    if e1.label() == e2.label() || e2.label() == e3.label() || e3.label() == e4.label() {
        return Err(E_CROSS);
    }
    Ok(())
}

/// R7 指纹稳定：同事件 fp 相等、异事件 fp 互异（四元组扰动全变）。
pub fn r7_fp_stable() -> Result<(), u16> {
    let a = ObsEvent { point: 0, seq: 7, tick: 5, detail: 9 };
    let b = a.clone();
    if a.fp() != b.fp() {
        return Err(E_ORDER_BAD);
    }
    let c = ObsEvent { point: 1, seq: 7, tick: 5, detail: 9 };
    let d = ObsEvent { point: 0, seq: 8, tick: 5, detail: 9 };
    let e = ObsEvent { point: 0, seq: 7, tick: 6, detail: 9 };
    let f = ObsEvent { point: 0, seq: 7, tick: 5, detail: 10 };
    if a.fp() == c.fp() || a.fp() == d.fp() || a.fp() == e.fp() || a.fp() == f.fp() {
        return Err(E_CROSS);
    }
    Ok(())
}

/// R8 last_tick 记账：每点的 last_tick 等于该点最后一次发布的 tick。
pub fn r8_last_tick() -> Result<(), u16> {
    let mut bus = ObsBus::new();
    bus.publish(POINT_THROTTLE, 10, 0)?;
    bus.publish(POINT_VIOLATION, 11, 0)?;
    bus.publish(POINT_THROTTLE, 33, 0)?;
    if bus.last_tick[POINT_THROTTLE] != 33 {
        return Err(E_ORDER_BAD);
    }
    if bus.last_tick[POINT_VIOLATION] != 11 {
        return Err(E_ORDER_BAD);
    }
    if bus.last_tick[POINT_RECONNECT] != 0 {
        return Err(E_ORDER_BAD);
    }
    Ok(())
}

/// R9 fuzz 混合负载：200 事件四点混发，序号/计数/隔离三账全绿。
pub fn r9_fuzz_mixed() -> Result<(), u16> {
    let mut bus = ObsBus::new();
    let mut g = Lcg::new(0x48473_0003);
    for _ in 0..200 {
        let p = g.below(POINT_N as u64) as usize;
        bus.publish(p, g.next(), (g.next() & 0xFFFF) as u32)?;
    }
    bus.audit_seq()?;
    bus.audit_isolation()?;
    if bus.seq != 200 {
        return Err(E_SEQ_GAP);
    }
    Ok(())
}

/// R10 计数快照单调：中间快照后继续发，快照值只增不减。
pub fn r10_count_monotone() -> Result<(), u16> {
    let mut bus = ObsBus::new();
    for _ in 0..10 {
        bus.publish(POINT_VIOLATION, 1, 1)?;
    }
    let snap = bus.counters.clone();
    for _ in 0..10 {
        bus.publish(POINT_DISCONNECT, 2, 2)?;
    }
    bus.audit_count_stable(&snap)?;
    if bus.counters[POINT_VIOLATION] != 10 || bus.counters[POINT_DISCONNECT] != 10 {
        return Err(E_COUNT_STALL);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_four_points_wired()?;
    hits += 1;
    r2_seq_contiguous()?;
    hits += 1;
    r3_no_cross()?;
    hits += 1;
    r4_ring_wrap()?;
    hits += 1;
    r5_gate_discard()?;
    hits += 1;
    r6_label_render()?;
    hits += 1;
    r7_fp_stable()?;
    hits += 1;
    r8_last_tick()?;
    hits += 1;
    r9_fuzz_mixed()?;
    hits += 1;
    r10_count_monotone()?;
    hits += 1;
    Ok(hits)
}
