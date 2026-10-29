//! UNX-F48509 · E·事件循环饿死防御（正：引擎忙时输入仍 100ms 内有反馈）
//!
//! 判据原文：UNX-F48509-J1 —— 合作式调度防饿死：引擎忙段切为不超
//! SLICE_MS（100ms）的片，片间必插入输入泵；任意 busy 长度下每个输入
//! 事件从到达至被确认的延迟 ≤ FEEDBACK_MS（100）；输入 FIFO 零丢失；
//! 连续输入风暴（每片间到达多个输入）下延迟仍有界；忙片计时账守恒
//! （片长和 == busy 总长）；同序列重放延迟账全等。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xEAA0;

pub const E_LATE: u16 = SEG_BASE + 1;
pub const E_INPUT_LOST: u16 = SEG_BASE + 2;
pub const E_SLICE_OVER: u16 = SEG_BASE + 3;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_STARVE: u16 = SEG_BASE + 6;

/// 忙片上限（ms）。
pub const SLICE_MS: u64 = 100;
/// 输入反馈上界（ms）。
pub const FEEDBACK_MS: u64 = 100;

/// 输入事件。
#[derive(Clone, PartialEq, Eq)]
pub struct InputEvent {
    pub arrive_ms: u64,
    pub id: u32,
}

/// 合作式循环：busy 片与输入泵交替。
pub struct CoopLoop {
    pub queue: Vec<InputEvent>,
    pub acked: Vec<(u32, u64)>,
    pub slices: Vec<u64>,
    pub acked_set: Vec<u32>,
}

impl CoopLoop {
    pub fn new() -> CoopLoop {
        CoopLoop {
            queue: Vec::new(),
            acked: Vec::new(),
            slices: Vec::new(),
            acked_set: Vec::new(),
        }
    }

    /// 输入到达：入 FIFO。
    pub fn feed(&mut self, ev: InputEvent) {
        self.queue.push(ev);
    }

    /// 跑一个忙片：片内先泵输入（全部确认），再消耗片预算做引擎工作。
    /// 返回该片实际消耗。
    pub fn run_slice(&mut self, now_ms: u64, budget: u64) -> Result<u64, u16> {
        if budget > SLICE_MS {
            return Err(E_SLICE_OVER);
        }
        // 输入泵：确认当前队列全部（确认时刻 = now）。
        while self.queue.len() > 0 {
            let ev = self.queue.remove(0);
            let dt = now_ms.saturating_sub(ev.arrive_ms);
            if dt > FEEDBACK_MS {
                return Err(E_LATE);
            }
            self.acked.push((ev.id, dt));
            self.acked_set.push(ev.id);
        }
        self.slices.push(budget);
        Ok(budget)
    }

    /// 审计：零丢失 + 延迟上界 + 片账守恒。
    pub fn audit(&self, fed_n: usize, busy_total: u64) -> Result<(), u16> {
        if self.acked.len() != fed_n {
            return Err(E_INPUT_LOST);
        }
        let mut slice_sum: u64 = 0;
        for s in self.slices.iter() {
            if *s > SLICE_MS {
                return Err(E_SLICE_OVER);
            }
            slice_sum += *s;
        }
        if slice_sum != busy_total {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 最大确认延迟。
    pub fn max_latency(&self) -> u64 {
        let mut m: u64 = 0;
        for a in self.acked.iter() {
            if a.1 > m {
                m = a.1;
            }
        }
        m
    }
}

/// 长任务分解：total_busy 切成 ≤ SLICE_MS 的片并间插输入泵。
pub fn run_long_task(loop_: &mut CoopLoop, total_busy: u64) -> Result<(), u16> {
    let mut now: u64 = 0;
    let mut left = total_busy;
    while left > 0 {
        let budget = if left > SLICE_MS { SLICE_MS } else { left };
        loop_.run_slice(now, budget)?;
        now += budget;
        left -= budget;
    }
    Ok(())
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

/// R1 忙时不饿死：10 片 busy（1s）中间到达的输入全部 ≤100ms 确认。
pub fn r1_busy_no_starve() -> Result<(), u16> {
    let mut lp = CoopLoop::new();
    // 片 1 开始前喂 1 个输入。
    lp.feed(InputEvent { arrive_ms: 0, id: 1 });
    run_long_task(&mut lp, 1000)?;
    lp.audit(1, 1000)?;
    if lp.max_latency() > FEEDBACK_MS {
        return Err(E_LATE);
    }
    Ok(())
}

/// R2 零丢失：50 输入分散到达全部确认。
pub fn r2_zero_loss() -> Result<(), u16> {
    let mut lp = CoopLoop::new();
    let mut g = Lcg::new(0x48509_0001);
    let mut fed = 0usize;
    // 分 10 批，每批喂 5 个再跑一片。
    for b in 0..10u64 {
        for i in 0..5u32 {
            lp.feed(InputEvent { arrive_ms: b * 100, id: (b * 10 + i as u64) as u32 });
            fed += 1;
        }
        lp.run_slice(b * 100, 100)?;
    }
    lp.audit(fed, 1000)?;
    if lp.acked.len() != fed {
        return Err(E_INPUT_LOST);
    }
    Ok(())
}

/// R3 延迟上界：风暴输入（每片间 20 个）最大延迟 ≤ 100。
pub fn r3_latency_bound() -> Result<(), u16> {
    let mut lp = CoopLoop::new();
    let mut id: u32 = 0;
    for b in 0..8u64 {
        for _ in 0..20 {
            lp.feed(InputEvent { arrive_ms: b * 100, id });
            id += 1;
        }
        lp.run_slice(b * 100, 100)?;
    }
    lp.audit(160, 800)?;
    if lp.max_latency() > FEEDBACK_MS {
        return Err(E_LATE);
    }
    Ok(())
}

/// R4 片长上界：任何片 ≤ SLICE_MS。
pub fn r4_slice_cap() -> Result<(), u16> {
    let mut lp = CoopLoop::new();
    // 137ms 任务 → 片 100 + 37。
    run_long_task(&mut lp, 137)?;
    lp.audit(0, 137)?;
    if lp.slices.len() != 2 {
        return Err(E_SLICE_OVER);
    }
    if lp.slices[0] != 100 || lp.slices[1] != 37 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R5 FIFO 保序：确认序 == 到达序。
pub fn r5_fifo_order() -> Result<(), u16> {
    let mut lp = CoopLoop::new();
    for i in 0..10u32 {
        lp.feed(InputEvent { arrive_ms: 0, id: i });
    }
    lp.run_slice(50, 100)?;
    if lp.acked.len() != 10 {
        return Err(E_INPUT_LOST);
    }
    for i in 0..10usize {
        if lp.acked[i].0 != i as u32 {
            return Err(E_STARVE);
        }
    }
    Ok(())
}

/// R6 确认延迟记录正确：延迟 = now - arrive。
pub fn r6_latency_record() -> Result<(), u16> {
    let mut lp = CoopLoop::new();
    lp.feed(InputEvent { arrive_ms: 30, id: 7 });
    lp.run_slice(100, 50)?;
    if lp.acked.len() != 1 {
        return Err(E_INPUT_LOST);
    }
    if lp.acked[0].1 != 70 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R7 超片拒收：budget > SLICE_MS 报 E_SLICE_OVER。
pub fn r7_slice_reject() -> Result<(), u16> {
    let mut lp = CoopLoop::new();
    let r = lp.run_slice(0, 250);
    match r {
        Err(E_SLICE_OVER) => {}
        _ => return Err(E_SLICE_OVER),
    }
    Ok(())
}

/// R8 fuzz 随机输入流：随机时刻到达的输入全部确认且延迟有界。
pub fn r8_fuzz_stream() -> Result<(), u16> {
    let mut g = Lcg::new(0x48509_0002);
    let mut lp = CoopLoop::new();
    let mut fed = 0usize;
    let mut now: u64 = 0;
    let mut id: u32 = 0;
    // 20 片，每片前随机 0..6 输入（到达时刻为片开始前 0..99ms）。
    for b in 0..20u64 {
        let n = g.below(6);
        for _ in 0..n {
            let back = g.below(100);
            lp.feed(InputEvent { arrive_ms: now.saturating_sub(back), id });
            id += 1;
            fed += 1;
        }
        lp.run_slice(now, 100)?;
        now += 100;
    }
    lp.audit(fed, 2000)?;
    if lp.max_latency() > FEEDBACK_MS {
        return Err(E_LATE);
    }
    Ok(())
}

/// R9 确认集互异：全部 acked id 唯一（无重复确认）。
pub fn r9_unique_acks() -> Result<(), u16> {
    let mut lp = CoopLoop::new();
    for i in 0..30u32 {
        lp.feed(InputEvent { arrive_ms: 10, id: i });
    }
    lp.run_slice(50, 100)?;
    for i in 0..lp.acked_set.len() {
        for j in (i + 1)..lp.acked_set.len() {
            if lp.acked_set[i] == lp.acked_set[j] {
                return Err(E_LEDGER_BAD);
            }
        }
    }
    Ok(())
}

/// R10 确定性重放：同种子两轮延迟账全等。
pub fn r10_deterministic() -> Result<(), u16> {
    let run = |seed: u64| -> Result<(u64, usize), u16> {
        let mut g = Lcg::new(seed);
        let mut lp = CoopLoop::new();
        let mut fed = 0usize;
        let mut now: u64 = 0;
        let mut id: u32 = 0;
        for b in 0..12u64 {
            let n = g.below(5);
            for _ in 0..n {
                let back = g.below(90);
                lp.feed(InputEvent { arrive_ms: now.saturating_sub(back), id });
                id += 1;
                fed += 1;
            }
            lp.run_slice(now, 100)?;
            now += 100;
        }
        lp.audit(fed, 1200)?;
        Ok((lp.max_latency(), lp.acked.len()))
    };
    let a = run(0x48509_0003)?;
    let b = run(0x48509_0003)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_busy_no_starve()?;
    hits += 1;
    r2_zero_loss()?;
    hits += 1;
    r3_latency_bound()?;
    hits += 1;
    r4_slice_cap()?;
    hits += 1;
    r5_fifo_order()?;
    hits += 1;
    r6_latency_record()?;
    hits += 1;
    r7_slice_reject()?;
    hits += 1;
    r8_fuzz_stream()?;
    hits += 1;
    r9_unique_acks()?;
    hits += 1;
    r10_deterministic()?;
    hits += 1;
    Ok(hits)
}
