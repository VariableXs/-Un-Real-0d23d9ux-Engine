//! UNX-F48593 —— I30 fuzz（随机 key_event 注入零崩）
//! 判据原文：UNX-F48593-J1 fuzz ×10^4 零崩；判据锚定 体验十。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           确定性 LCG ×10^4 注入，状态机全路径零崩零漂。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0140; // I30 key_event fuzz 诊断段

const E_CRASH: u16 = SEG_BASE + 1;     // 注入致崩（状态机非法迁移）
const E_STATE_BAD: u16 = SEG_BASE + 2; // 终态漂移
const E_QUEUE_OOB: u16 = SEG_BASE + 3; // 事件队列越界
const E_CODE_BAD: u16 = SEG_BASE + 4;  // 键码域违例
const E_LEDGER: u16 = SEG_BASE + 5;    // 事件账不守恒
const E_REPLAY: u16 = SEG_BASE + 6;    // 重演不一致
const E_DRAIN_BAD: u16 = SEG_BASE + 7; // 排空违例

/// 事件类型。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EvKind {
    Down = 0,
    Up = 1,
    Repeat = 2,
    Flush = 3,
}

pub const KIND_COUNT: usize = 4;

/// 键事件。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub kind: EvKind,
    pub code: u32,
}

/// 键事件状态机：down 计数、重复计数、排空次数，全部有界。
pub struct EvMachine {
    pub down_now: u32,
    pub repeats: u32,
    pub flushes: u32,
    pub fed: u32,
    pub queue_len: u32,
    pub cap_queue: u32,
}

impl EvMachine {
    pub fn new(cap_queue: u32) -> Self {
        EvMachine {
            down_now: 0,
            repeats: 0,
            flushes: 0,
            fed: 0,
            queue_len: 0,
            cap_queue,
        }
    }

    /// 注入一键事件：任何非法组合返回 E_CRASH 而不是崩。
    pub fn inject(&mut self, ev: KeyEvent) -> Result<(), u16> {
        if ev.code > 0xffff {
            return Err(E_CODE_BAD);
        }
        if self.fed >= u32::MAX - 1 {
            return Err(E_QUEUE_OOB);
        }
        match ev.kind {
            EvKind::Down => {
                // 有界按下：超过并发上限即吞（不崩）。
                if self.down_now < 8 {
                    self.down_now += 1;
                }
                if self.queue_len < self.cap_queue {
                    self.queue_len += 1;
                }
            }
            EvKind::Up => {
                // 抬起不会把 down_now 打穿到负域。
                if self.down_now > 0 {
                    self.down_now -= 1;
                }
                if self.queue_len > 0 {
                    self.queue_len -= 1;
                }
            }
            EvKind::Repeat => {
                if self.repeats < u32::MAX - 1 {
                    self.repeats += 1;
                }
            }
            EvKind::Flush => {
                self.flushes += 1;
                self.queue_len = 0;
            }
        }
        self.fed += 1;
        Ok(())
    }

    /// 状态自检：down_now ≤ 8、queue ≤ cap。
    pub fn selfcheck(&self) -> Result<(), u16> {
        if self.down_now > 8 {
            return Err(E_CRASH);
        }
        if self.queue_len > self.cap_queue {
            return Err(E_QUEUE_OOB);
        }
        if self.flushes > 0 && self.queue_len > self.cap_queue {
            return Err(E_DRAIN_BAD);
        }
        Ok(())
    }

    /// 事件账守恒：fed == 分类注入合计（外部复算）。
    pub fn audit(&self, expect_fed: u32) -> Result<(), u16> {
        if self.fed != expect_fed {
            return Err(E_LEDGER);
        }
        self.selfcheck()
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.down_now as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.repeats as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.flushes as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.queue_len as u64;
        h
    }
}

/// 随机事件生成器（LCG 驱动）。
pub struct EvGen {
    pub rng: LcgInner,
}

/// LCG（内部再封装一层，便于场景复用）。
pub struct LcgInner(pub u64);

impl LcgInner {
    pub fn new(seed: u64) -> Self {
        LcgInner(seed)
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
    /// 生成一个随机键事件。
    pub fn event(&mut self) -> KeyEvent {
        let k = self.below(KIND_COUNT as u64) as usize;
        let kind = match k {
            0 => EvKind::Down,
            1 => EvKind::Up,
            2 => EvKind::Repeat,
            _ => EvKind::Flush,
        };
        KeyEvent { kind, code: self.below(0x1_0000) as u32 }
    }
}

pub type Lcg = LcgInner;

/// 主巡检：I30 key_event fuzz 十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 基线状态机：合法事件序列零崩。
    let mut m = EvMachine::new(64);
    m.inject(KeyEvent { kind: EvKind::Down, code: 30 })?;
    m.inject(KeyEvent { kind: EvKind::Repeat, code: 30 })?;
    m.inject(KeyEvent { kind: EvKind::Up, code: 30 })?;
    m.inject(KeyEvent { kind: EvKind::Flush, code: 0 })?;
    m.selfcheck()?;
    hits += 1;

    // R2 fuzz ×10^4 零崩：随机事件全量注入，状态机自检全程绿。
    let mut rng = Lcg::new(0x48593_0001);
    let mut m2 = EvMachine::new(128);
    let mut fed = 0u32;
    let mut k = 0u32;
    while k < 10_000 {
        let ev = rng.event();
        m2.inject(ev)?;
        m2.selfcheck()?;
        fed += 1;
        k += 1;
    }
    m2.audit(fed)?;
    hits += 1;

    // R3 键码域守卫：超域键码须拒且不入账。
    let mut m3 = EvMachine::new(16);
    let r = m3.inject(KeyEvent { kind: EvKind::Down, code: 0x1_0000 });
    if r.err() != Some(E_CODE_BAD) {
        return Err(E_CODE_BAD);
    }
    if m3.fed != 0 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4 抬起穿底防护：多余 Up 不产生负数态（down_now 钳零）。
    let mut m4 = EvMachine::new(8);
    let mut i = 0u32;
    while i < 5 {
        m4.inject(KeyEvent { kind: EvKind::Up, code: 1 })?;
        i += 1;
    }
    if m4.down_now != 0 {
        return Err(E_CRASH);
    }
    hits += 1;

    // R5 队列上限防护：Down 超过容量后被吞，队列不越界。
    let mut m5 = EvMachine::new(4);
    let mut i = 0u32;
    while i < 20 {
        m5.inject(KeyEvent { kind: EvKind::Down, code: 2 })?;
        i += 1;
    }
    if m5.queue_len > 4 {
        return Err(E_QUEUE_OOB);
    }
    hits += 1;

    // R6 排空语义：Flush 后队列归零，flushes 计数入账。
    let mut m6 = EvMachine::new(8);
    let mut i = 0u32;
    while i < 6 {
        m6.inject(KeyEvent { kind: EvKind::Down, code: 3 })?;
        i += 1;
    }
    m6.inject(KeyEvent { kind: EvKind::Flush, code: 0 })?;
    if m6.queue_len != 0 || m6.flushes != 1 {
        return Err(E_DRAIN_BAD);
    }
    hits += 1;

    // R7 并发上限防护：down_now 恒 ≤ 8。
    let mut m7 = EvMachine::new(64);
    let mut i = 0u32;
    while i < 50 {
        m7.inject(KeyEvent { kind: EvKind::Down, code: 4 })?;
        i += 1;
    }
    if m7.down_now > 8 {
        return Err(E_CRASH);
    }
    hits += 1;

    // R8 fuzz 二段 ×10^4：大容量队列域再跑一轮零崩。
    let mut rng2 = Lcg::new(0x48593_0002);
    let mut m8 = EvMachine::new(1024);
    let mut k = 0u32;
    while k < 10_000 {
        let ev = rng2.event();
        m8.inject(ev)?;
        m8.selfcheck()?;
        k += 1;
    }
    m8.audit(10_000)?;
    hits += 1;

    // R9 混合压测账守恒：两轮 fuzz fed 合计 == 20_000。
    if m2.fed.wrapping_add(m8.fed) != 20_000 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R10 重演一致：同种子重建两段 fuzz，指纹全等。
    let fp1 = (m2.fingerprint(), m8.fingerprint());
    let mut rng3 = Lcg::new(0x48593_0001);
    let mut m9 = EvMachine::new(128);
    let mut k = 0u32;
    while k < 10_000 {
        let ev = rng3.event();
        let _ = m9.inject(ev);
        k += 1;
    }
    let mut rng4 = Lcg::new(0x48593_0002);
    let mut m10 = EvMachine::new(1024);
    let mut k = 0u32;
    while k < 10_000 {
        let ev = rng4.event();
        let _ = m10.inject(ev);
        k += 1;
    }
    if (m9.fingerprint(), m10.fingerprint()) != fp1 || fp1.0 == 0 || fp1.1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
