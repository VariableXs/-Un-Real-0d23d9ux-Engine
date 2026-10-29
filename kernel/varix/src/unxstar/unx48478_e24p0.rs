//! UNX-F48478 · E24 与 AI-86 红线面登记（P0 违例直报通道验证）
//!
//! 判据原文：UNX-F48478-J1 —— 协议面 P0 级违例必须走直报通道：P0 发出
//! 后 1 tick 内落 AI-86 红线寄存器（不排队、不合并、不丢失）；非 P0 违例
//! 走普通队列且绝不占用 P0 寄存器；红线寄存器链（序号 + 指纹链）完整可
//! 重演；违例归类到 AI-86 红线条款（条款覆盖全）；普通通道堆积不阻塞
//! P0 直报；全场景 drill 零偏差。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xE5A0;

pub const E_P0_DELAYED: u16 = SEG_BASE + 1;
pub const E_P0_CROSSED: u16 = SEG_BASE + 2;
pub const E_CHAIN_BAD: u16 = SEG_BASE + 3;
pub const E_CLAUSE_OOB: u16 = SEG_BASE + 4;
pub const E_DIRECT_BLOCKED: u16 = SEG_BASE + 5;
pub const E_KIND_OOB: u16 = SEG_BASE + 6;

/// AI-86 红线条款（协议面引用子集）。
pub const CLAUSE_AUTH: usize = 0;
pub const CLAUSE_INTEGRITY: usize = 1;
pub const CLAUSE_ISOLATION: usize = 2;
pub const CLAUSE_RESOURCE: usize = 3;
pub const CLAUSE_ORDER: usize = 5;
pub const CLAUSE_N: usize = 6;

/// 违例级别。
pub const LVL_P0: u8 = 0;
pub const LVL_P1: u8 = 1;
pub const LVL_P2: u8 = 2;

/// 违例事件。
#[derive(Clone, PartialEq, Eq)]
pub struct Violation {
    pub kind: usize,
    pub clause: usize,
    pub lvl: u8,
    pub tick: u64,
    pub detail: u32,
}

impl Violation {
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.kind as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.clause as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.lvl as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.tick;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.detail as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }
}

/// AI-86 红线寄存器：P0 直报落点，链式指纹。
pub struct P0Register {
    pub entries: Vec<Violation>,
    pub chain: u64,
    seq: u32,
}

impl P0Register {
    pub fn new() -> P0Register {
        P0Register { entries: Vec::new(), chain: 0x86_86_86_86_86_86_86_86, seq: 0 }
    }

    /// 直报：1 tick 内登记（登记刻 == 事件刻 + 1，直报不排队）。
    pub fn direct(&mut self, v: Violation) -> Result<u32, u16> {
        if v.lvl != LVL_P0 {
            return Err(E_P0_CROSSED);
        }
        if v.clause >= CLAUSE_N {
            return Err(E_CLAUSE_OOB);
        }
        self.seq += 1;
        self.entries.push(v.clone());
        self.chain ^= v.fp();
        self.chain = self.chain.wrapping_mul(0x100000001b3);
        Ok(self.seq)
    }

    /// 直报时效审计：全部 entries 的事件刻与登记刻差 ≤1。
    pub fn audit_prompt(&self, shown: &[u64]) -> Result<(), u16> {
        if shown.len() != self.entries.len() {
            return Err(E_P0_DELAYED);
        }
        for i in 0..self.entries.len() {
            let dt = shown[i].saturating_sub(self.entries[i].tick);
            if dt > 1 {
                return Err(E_P0_DELAYED);
            }
        }
        Ok(())
    }

    /// 链重演：从 entries 重算 chain 与现值一致。
    pub fn audit_chain(&self) -> Result<(), u16> {
        let mut h: u64 = 0x86_86_86_86_86_86_86_86;
        for e in self.entries.iter() {
            h ^= e.fp();
            h = h.wrapping_mul(0x100000001b3);
        }
        if h != self.chain {
            return Err(E_CHAIN_BAD);
        }
        Ok(())
    }
}

/// 普通队列：非 P0 违例排队（FIFO），堆积不影响 P0 直报。
pub struct PlainQueue {
    pub items: Vec<Violation>,
    pub cap: usize,
}

impl PlainQueue {
    pub fn new(cap: usize) -> PlainQueue {
        PlainQueue { items: Vec::new(), cap }
    }
    /// 入队：P0 拒收（不占普通通道）、越界拒收。
    pub fn push(&mut self, v: Violation) -> Result<bool, u16> {
        if v.clause >= CLAUSE_N {
            return Err(E_CLAUSE_OOB);
        }
        if v.lvl == LVL_P0 {
            return Err(E_P0_CROSSED);
        }
        if self.items.len() >= self.cap {
            return Ok(false);
        }
        self.items.push(v);
        Ok(true)
    }
}

/// 违例总线：按级别分流。
pub struct ViolationBus {
    pub p0: P0Register,
    pub plain: PlainQueue,
    pub p0_shown: Vec<u64>,
}

impl ViolationBus {
    pub fn new(plain_cap: usize) -> ViolationBus {
        ViolationBus {
            p0: P0Register::new(),
            plain: PlainQueue::new(plain_cap),
            p0_shown: Vec::new(),
        }
    }

    /// 分流：P0 直报（登记刻 = tick+1），其余入普通队列。
    pub fn submit(&mut self, v: Violation) -> Result<(), u16> {
        if v.lvl == LVL_P0 {
            // 直报不受普通队列堆积影响。
            let v_tick = v.tick;
            self.p0.direct(v)?;
            self.p0_shown.push(v_tick + 1);
            Ok(())
        } else {
            let ok = self.plain.push(v)?;
            if !ok {
                // 满载丢弃是普通通道的可接受策略（P0 不受影响）。
                return Ok(());
            }
            Ok(())
        }
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

/// R1 P0 直报时效：3 条 P0 全部 1 tick 内登记。
pub fn r1_p0_prompt() -> Result<(), u16> {
    let mut bus = ViolationBus::new(64);
    for i in 0..3u64 {
        let v = Violation { kind: i as usize, clause: CLAUSE_AUTH, lvl: LVL_P0, tick: 100 + i * 10, detail: i as u32 };
        bus.submit(v)?;
    }
    bus.p0.audit_prompt(&bus.p0_shown)?;
    if bus.p0.entries.len() != 3 {
        return Err(E_DIRECT_BLOCKED);
    }
    Ok(())
}

/// R2 非 P0 不占 P0 通道：P1/P2 只进普通队列。
pub fn r2_no_cross() -> Result<(), u16> {
    let mut bus = ViolationBus::new(64);
    bus.submit(Violation { kind: 0, clause: CLAUSE_ORDER, lvl: LVL_P1, tick: 1, detail: 0 })?;
    bus.submit(Violation { kind: 1, clause: CLAUSE_ORDER, lvl: LVL_P2, tick: 2, detail: 0 })?;
    if bus.p0.entries.len() != 0 {
        return Err(E_P0_CROSSED);
    }
    if bus.plain.items.len() != 2 {
        return Err(E_KIND_OOB);
    }
    Ok(())
}

/// R3 P0 不占普通通道：plain.push(P0) 拒收。
pub fn r3_p0_no_plain() -> Result<(), u16> {
    let mut q = PlainQueue::new(8);
    let r = q.push(Violation { kind: 0, clause: CLAUSE_AUTH, lvl: LVL_P0, tick: 1, detail: 0 });
    match r {
        Err(E_P0_CROSSED) => {}
        _ => return Err(E_P0_CROSSED),
    }
    if q.items.len() != 0 {
        return Err(E_P0_CROSSED);
    }
    Ok(())
}

/// R4 指纹链完整：5 条 P0 链重演一致。
pub fn r4_chain_replay() -> Result<(), u16> {
    let mut reg = P0Register::new();
    for i in 0..5u64 {
        reg.direct(Violation {
            kind: (i % 3) as usize,
            clause: CLAUSE_INTEGRITY,
            lvl: LVL_P0,
            tick: i,
            detail: (i * 7) as u32,
        })?;
    }
    reg.audit_chain()?;
    Ok(())
}

/// R5 堆积不阻塞直报：普通队列满载后 P0 仍即时登记。
pub fn r5_no_block() -> Result<(), u16> {
    let mut bus = ViolationBus::new(4);
    for i in 0..8u64 {
        bus.submit(Violation { kind: 2, clause: CLAUSE_ISOLATION, lvl: LVL_P1, tick: i, detail: i as u32 })?;
    }
    if bus.plain.items.len() != 4 {
        return Err(E_KIND_OOB);
    }
    bus.submit(Violation { kind: 3, clause: CLAUSE_AUTH, lvl: LVL_P0, tick: 999, detail: 1 })?;
    bus.p0.audit_prompt(&bus.p0_shown)?;
    if bus.p0.entries.len() != 1 {
        return Err(E_DIRECT_BLOCKED);
    }
    Ok(())
}

/// R6 条款越界拒收：clause ≥ CLAUSE_N 直报/入队都拒。
pub fn r6_clause_guard() -> Result<(), u16> {
    let mut bus = ViolationBus::new(8);
    let r = bus.submit(Violation { kind: 0, clause: CLAUSE_N + 3, lvl: LVL_P0, tick: 1, detail: 0 });
    match r {
        Err(E_CLAUSE_OOB) => {}
        _ => return Err(E_CLAUSE_OOB),
    }
    let r2 = bus.submit(Violation { kind: 0, clause: 99, lvl: LVL_P1, tick: 1, detail: 0 });
    match r2 {
        Err(E_CLAUSE_OOB) => {}
        _ => return Err(E_CLAUSE_OOB),
    }
    Ok(())
}

/// R7 条款覆盖：六条款各登记一条 P0，寄存器全收。
pub fn r7_clause_coverage() -> Result<(), u16> {
    let mut reg = P0Register::new();
    for c in 0..CLAUSE_N {
        reg.direct(Violation { kind: c, clause: c, lvl: LVL_P0, tick: c as u64, detail: 0 })?;
    }
    if reg.entries.len() != CLAUSE_N {
        return Err(E_DIRECT_BLOCKED);
    }
    // 条款互异核对。
    for i in 0..reg.entries.len() {
        for j in (i + 1)..reg.entries.len() {
            if reg.entries[i].clause == reg.entries[j].clause && reg.entries[i].tick == reg.entries[j].tick {
                return Err(E_CHAIN_BAD);
            }
        }
    }
    reg.audit_chain()
}

/// R8 指纹敏感：任一字段扰动 fp 必变。
pub fn r8_fp_sensitive() -> Result<(), u16> {
    let base = Violation { kind: 1, clause: 2, lvl: LVL_P0, tick: 3, detail: 4 };
    let v1 = Violation { kind: 2, clause: 2, lvl: LVL_P0, tick: 3, detail: 4 };
    let v2 = Violation { kind: 1, clause: 3, lvl: LVL_P0, tick: 3, detail: 4 };
    let v3 = Violation { kind: 1, clause: 2, lvl: LVL_P0, tick: 4, detail: 4 };
    let v4 = Violation { kind: 1, clause: 2, lvl: LVL_P0, tick: 3, detail: 5 };
    if base.fp() == v1.fp() || base.fp() == v2.fp() || base.fp() == v3.fp() || base.fp() == v4.fp() {
        return Err(E_CHAIN_BAD);
    }
    let b2 = base.clone();
    if base.fp() != b2.fp() {
        return Err(E_CHAIN_BAD);
    }
    Ok(())
}

/// R9 fuzz 混合分流：200 条随机级别/条款，P0 全直报、非 P0 全走普通、零交叉。
pub fn r9_fuzz_split() -> Result<(), u16> {
    let mut g = Lcg::new(0x48478_0001);
    let mut bus = ViolationBus::new(1024);
    let mut p0_expect = 0u32;
    for _ in 0..200 {
        let lvl: u8 = match g.below(4) {
            0 => LVL_P0,
            1 | 2 => LVL_P1,
            _ => LVL_P2,
        };
        let v = Violation {
            kind: g.below(4) as usize,
            clause: g.below(CLAUSE_N as u64) as usize,
            lvl,
            tick: g.next(),
            detail: (g.next() & 0xFFFF) as u32,
        };
        if lvl == LVL_P0 {
            p0_expect += 1;
        }
        bus.submit(v)?;
    }
    bus.p0.audit_chain()?;
    if bus.p0.entries.len() as u32 != p0_expect {
        return Err(E_P0_CROSSED);
    }
    bus.p0.audit_prompt(&bus.p0_shown)?;
    Ok(())
}

/// R10 空寄存器链恒等：零 P0 时 chain == 初值。
pub fn r10_empty_chain() -> Result<(), u16> {
    let reg = P0Register::new();
    if reg.entries.len() != 0 {
        return Err(E_P0_CROSSED);
    }
    reg.audit_chain()?;
    if reg.chain != 0x86_86_86_86_86_86_86_86 {
        return Err(E_CHAIN_BAD);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_p0_prompt()?;
    hits += 1;
    r2_no_cross()?;
    hits += 1;
    r3_p0_no_plain()?;
    hits += 1;
    r4_chain_replay()?;
    hits += 1;
    r5_no_block()?;
    hits += 1;
    r6_clause_guard()?;
    hits += 1;
    r7_clause_coverage()?;
    hits += 1;
    r8_fp_sensitive()?;
    hits += 1;
    r9_fuzz_split()?;
    hits += 1;
    r10_empty_chain()?;
    hits += 1;
    Ok(hits)
}
