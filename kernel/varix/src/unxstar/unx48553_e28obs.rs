//! UNX-F48553 · E28 观测埋点（断电/恢复/清理/复位四点）
//!
//! 判据原文：UNX-F48553-J1 —— 恢复面四类观测点（断电/恢复/清理/复位）
//! 全部接通：事件经统一 RecBus 分发到各自 sink，计数单调、序号连续无
//! 跳号；四点互不串扰（A 点事件不落 B 槽）；环形缓冲写满回绕不丢最近
//! 事件；埋点开关关闭时事件丢弃但计数仍走；断电点事件带恢复线索字段；
//! 全场景 drill 零偏差。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF420;

pub const E_SINK_OOB: u16 = SEG_BASE + 1;
pub const E_SEQ_GAP: u16 = SEG_BASE + 2;
pub const E_CROSS: u16 = SEG_BASE + 3;
pub const E_RING_LOST: u16 = SEG_BASE + 4;
pub const E_GATE_LEAK: u16 = SEG_BASE + 5;
pub const E_COUNT_STALL: u16 = SEG_BASE + 6;
pub const E_CLUE_MISS: u16 = SEG_BASE + 7;

/// 四类观测点。
pub const PT_POWER_LOSS: usize = 0;
pub const PT_RECOVER: usize = 1;
pub const PT_CLEANUP: usize = 2;
pub const PT_RESET: usize = 3;
pub const PT_N: usize = 4;

/// 环形缓冲容量。
pub const RING_CAP: usize = 12;

/// 恢复面观测事件（含恢复线索字段）。
#[derive(Clone, PartialEq, Eq)]
pub struct RecEvent {
    pub point: usize,
    pub seq: u32,
    pub tick: u64,
    pub clue: u32,
}

impl RecEvent {
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.point as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.seq as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.tick;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.clue as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }
    /// 恢复线索标签：断电点必须带非零线索。
    pub fn clue_label(&self) -> String {
        let base = match self.point {
            PT_POWER_LOSS => "powerloss",
            PT_RECOVER => "recover",
            PT_CLEANUP => "cleanup",
            PT_RESET => "reset",
            _ => "unknown",
        };
        let mut s = String::from(base);
        s.push('|');
        let mut v = self.clue;
        let mut digits = Vec::new();
        if v == 0 {
            digits.push(b'0');
        }
        while v > 0 {
            digits.push(b'0' + (v % 10) as u8);
            v /= 10;
        }
        let mut i = digits.len();
        while i > 0 {
            i -= 1;
            s.push(digits[i] as char);
        }
        s
    }
}

/// 环形 sink。
pub struct RecRing {
    buf: Vec<RecEvent>,
    head: usize,
    pub total: u64,
}

impl RecRing {
    pub fn new() -> RecRing {
        RecRing { buf: Vec::new(), head: 0, total: 0 }
    }
    pub fn push(&mut self, ev: RecEvent) {
        if self.buf.len() < RING_CAP {
            self.buf.push(ev);
        } else {
            self.buf[self.head] = ev;
        }
        self.head = (self.head + 1) % RING_CAP;
        self.total += 1;
    }
    pub fn read_oldest_first(&self) -> Vec<RecEvent> {
        let n = self.buf.len();
        let mut out = Vec::new();
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

/// 恢复观测总线。
pub struct RecBus {
    pub enabled: bool,
    seq: u32,
    pub counters: [u64; PT_N],
    pub sinks: [RecRing; PT_N],
}

impl RecBus {
    pub fn new() -> RecBus {
        RecBus {
            enabled: true,
            seq: 0,
            counters: [0; PT_N],
            sinks: [RecRing::new(), RecRing::new(), RecRing::new(), RecRing::new()],
        }
    }

    /// 发布：分配连续序号；断电点必须带非零线索。
    pub fn publish(&mut self, point: usize, tick: u64, clue: u32) -> Result<u32, u16> {
        if point >= PT_N {
            return Err(E_SINK_OOB);
        }
        if point == PT_POWER_LOSS && clue == 0 {
            return Err(E_CLUE_MISS);
        }
        self.seq += 1;
        let ev = RecEvent { point, seq: self.seq, tick, clue };
        self.counters[point] += 1;
        let out_seq = ev.seq;
        if self.enabled {
            self.sinks[point].push(ev);
        }
        Ok(out_seq)
    }

    pub fn audit_seq(&self) -> Result<(), u16> {
        let sum: u64 = self.counters.iter().fold(0u64, |a, b| a + b);
        if sum != self.seq as u64 {
            return Err(E_SEQ_GAP);
        }
        Ok(())
    }

    pub fn audit_isolation(&self) -> Result<(), u16> {
        for p in 0..PT_N {
            for ev in self.sinks[p].read_oldest_first().iter() {
                if ev.point != p {
                    return Err(E_CROSS);
                }
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

/// R1 四点接通：每点 3 事件，计数与序号单调。
pub fn r1_four_points() -> Result<(), u16> {
    let mut bus = RecBus::new();
    let mut tick: u64 = 10;
    for p in 0..PT_N {
        for d in 1..4u32 {
            tick += 1;
            let s = bus.publish(p, tick, d)?;
            if s == 0 {
                return Err(E_SEQ_GAP);
            }
        }
    }
    bus.audit_seq()?;
    for p in 0..PT_N {
        if bus.counters[p] != 3 {
            return Err(E_COUNT_STALL);
        }
    }
    Ok(())
}

/// R2 序号连续：60 事件 seq 恰 1..=60。
pub fn r2_seq_contiguous() -> Result<(), u16> {
    let mut bus = RecBus::new();
    let mut g = Lcg::new(0x48553_0001);
    for _ in 0..60 {
        let p = g.below(PT_N as u64) as usize;
        let clue = if p == PT_POWER_LOSS { 1 + g.below(9) as u32 } else { g.below(10) as u32 };
        bus.publish(p, g.next(), clue)?;
    }
    bus.audit_seq()?;
    if bus.seq != 60 {
        return Err(E_SEQ_GAP);
    }
    Ok(())
}

/// R3 互不串扰：fuzz 混发后各槽对位。
pub fn r3_no_cross() -> Result<(), u16> {
    let mut bus = RecBus::new();
    let mut g = Lcg::new(0x48553_0002);
    for _ in 0..80 {
        let p = g.below(PT_N as u64) as usize;
        let clue = if p == PT_POWER_LOSS { 1 + g.below(9) as u32 } else { 0 };
        bus.publish(p, g.next(), clue)?;
    }
    bus.audit_isolation()
}

/// R4 环形回绕：单点 RING_CAP*3 条保留最近。
pub fn r4_ring_wrap() -> Result<(), u16> {
    let mut bus = RecBus::new();
    for i in 0..(RING_CAP as u32 * 3) {
        bus.publish(PT_RECOVER, i as u64, i + 1)?;
    }
    let ring = bus.sinks[PT_RECOVER].read_oldest_first();
    if ring.len() != RING_CAP {
        return Err(E_RING_LOST);
    }
    let start = RING_CAP as u32 * 3 - RING_CAP as u32 + 1;
    for i in 0..RING_CAP {
        if ring[i].clue != start + i as u32 {
            return Err(E_RING_LOST);
        }
    }
    Ok(())
}

/// R5 断电线索强制：断电点零线索拒收 E_CLUE_MISS。
pub fn r5_clue_required() -> Result<(), u16> {
    let mut bus = RecBus::new();
    match bus.publish(PT_POWER_LOSS, 1, 0) {
        Err(E_CLUE_MISS) => {}
        _ => return Err(E_CLUE_MISS),
    }
    bus.publish(PT_POWER_LOSS, 2, 42)?;
    if bus.sinks[PT_POWER_LOSS].len() != 1 {
        return Err(E_CLUE_MISS);
    }
    Ok(())
}

/// R6 闸门：关闭时丢弃但计数走。
pub fn r6_gate_discard() -> Result<(), u16> {
    let mut bus = RecBus::new();
    bus.enabled = false;
    for _ in 0..4 {
        bus.publish(PT_CLEANUP, 1, 0)?;
    }
    bus.audit_seq()?;
    if bus.sinks[PT_CLEANUP].len() != 0 {
        return Err(E_GATE_LEAK);
    }
    if bus.counters[PT_CLEANUP] != 4 {
        return Err(E_COUNT_STALL);
    }
    bus.enabled = true;
    bus.publish(PT_CLEANUP, 2, 7)?;
    if bus.sinks[PT_CLEANUP].len() != 1 {
        return Err(E_GATE_LEAK);
    }
    Ok(())
}

/// R7 线索标签：四点标签前缀互异且含 clue。
pub fn r7_clue_label() -> Result<(), u16> {
    let e0 = RecEvent { point: 0, seq: 1, tick: 0, clue: 5 };
    let e1 = RecEvent { point: 1, seq: 1, tick: 0, clue: 5 };
    let e2 = RecEvent { point: 2, seq: 1, tick: 0, clue: 5 };
    let e3 = RecEvent { point: 3, seq: 1, tick: 0, clue: 5 };
    let l0 = e0.clue_label();
    let l1 = e1.clue_label();
    let l2 = e2.clue_label();
    let l3 = e3.clue_label();
    if l0 == l1 || l1 == l2 || l2 == l3 || l0 == l2 {
        return Err(E_CROSS);
    }
    // 尾部带 clue。
    let tail = &l0[l0.len() - 1..];
    if tail != "5" {
        return Err(E_CLUE_MISS);
    }
    Ok(())
}

/// R8 指纹敏感：四字段扰动全变。
pub fn r8_fp_sensitive() -> Result<(), u16> {
    let base = RecEvent { point: 0, seq: 7, tick: 5, clue: 9 };
    let v1 = RecEvent { point: 1, seq: 7, tick: 5, clue: 9 };
    let v2 = RecEvent { point: 0, seq: 8, tick: 5, clue: 9 };
    let v3 = RecEvent { point: 0, seq: 7, tick: 6, clue: 9 };
    let v4 = RecEvent { point: 0, seq: 7, tick: 5, clue: 10 };
    if base.fp() == v1.fp() || base.fp() == v2.fp() || base.fp() == v3.fp() || base.fp() == v4.fp() {
        return Err(E_CROSS);
    }
    Ok(())
}

/// R9 fuzz 混合：200 事件全绿。
pub fn r9_fuzz_mixed() -> Result<(), u16> {
    let mut bus = RecBus::new();
    let mut g = Lcg::new(0x48553_0003);
    for _ in 0..200 {
        let p = g.below(PT_N as u64) as usize;
        let clue = if p == PT_POWER_LOSS { 1 + g.below(99) as u32 } else { (g.next() & 0xFF) as u32 };
        bus.publish(p, g.next(), clue)?;
    }
    bus.audit_seq()?;
    bus.audit_isolation()?;
    if bus.seq != 200 {
        return Err(E_SEQ_GAP);
    }
    Ok(())
}

/// R10 越界拒收：point ≥ PT_N 报 E_SINK_OOB。
pub fn r10_oob_reject() -> Result<(), u16> {
    let mut bus = RecBus::new();
    for p in [PT_N, PT_N + 5, 99].iter() {
        match bus.publish(*p, 1, 0) {
            Err(E_SINK_OOB) => {}
            _ => return Err(E_SINK_OOB),
        }
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_four_points()?;
    hits += 1;
    r2_seq_contiguous()?;
    hits += 1;
    r3_no_cross()?;
    hits += 1;
    r4_ring_wrap()?;
    hits += 1;
    r5_clue_required()?;
    hits += 1;
    r6_gate_discard()?;
    hits += 1;
    r7_clue_label()?;
    hits += 1;
    r8_fp_sensitive()?;
    hits += 1;
    r9_fuzz_mixed()?;
    hits += 1;
    r10_oob_reject()?;
    hits += 1;
    Ok(hits)
}
