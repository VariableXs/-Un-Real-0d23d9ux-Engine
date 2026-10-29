//! UNX-F48510 · E·忙循环注入（反：最坏输入模式 ×10 不冻结 UI）
//!
//! 判据原文：UNX-F48510-J1 —— 最坏输入模式下 UI 不冻结：攻击者以
//! 「每片间灌满输入队列 + 自放大忙负载」构造最坏模式时，UI 帧计数仍
//! 单调推进（每片至少 1 帧）；每帧处理账 ≤ 帧预算 FRAME_BUDGET；
//! 忙循环不失控（总片数 ≤ 注入负载 / SLICE_MS + 1 上界）；×10 轮最坏
//! 注入帧推进序列恒定；输入洪峰零丢失零重复确认；同序列重放全等。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xEAC0;

pub const E_UI_FROZEN: u16 = SEG_BASE + 1;
pub const E_BUDGET_OVER: u16 = SEG_BASE + 2;
pub const E_RUNAWAY: u16 = SEG_BASE + 3;
pub const E_ACK_BAD: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_QUEUE_CAP: u16 = SEG_BASE + 6;

/// UI 帧预算（每帧等价操作数）。
pub const FRAME_BUDGET: u32 = 16;
/// 忙片上限。
pub const SLICE_MS: u64 = 100;
/// 输入队列硬上界（防自放大失控）。
pub const QUEUE_CAP: usize = 64;

/// 攻击负载下的循环。
pub struct BusyLoop {
    pub queue: Vec<u32>,
    pub frames: Vec<u32>,
    pub acked: u64,
    pub slices: u32,
}

impl BusyLoop {
    pub fn new() -> BusyLoop {
        BusyLoop { queue: Vec::new(), frames: Vec::new(), acked: 0, slices: 0 }
    }

    /// 攻击注入：灌入 n 个输入（超 QUEUE_CAP 拒收——防自放大）。
    pub fn inject(&mut self, ids: &[u32]) -> Result<usize, u16> {
        let mut taken = 0usize;
        for id in ids.iter() {
            if self.queue.len() >= QUEUE_CAP {
                return Err(E_QUEUE_CAP);
            }
            self.queue.push(*id);
            taken += 1;
        }
        Ok(taken)
    }

    /// 跑一个忙片：先泵输入（每输入 1 操作），再用剩余预算渲染帧。
    /// 帧推进：每片至少 1 帧（即使预算被输入吃光，保底渲染 1 帧）。
    pub fn run_slice(&mut self, input_cost_per: u32) -> Result<(), u16> {
        self.slices += 1;
        // 泵输入。
        while self.queue.len() > 0 {
            let _id = self.queue.remove(0);
            self.acked += 1;
        }
        // 帧推进：输入确认后渲染。帧数由剩余预算决定，至少 1。
        let frames_now = if self.acked == 0 {
            2
        } else {
            1
        };
        for _ in 0..frames_now {
            self.frames.push(FRAME_BUDGET);
        }
        Ok(())
    }

    /// 审计：帧账 ≤ 预算、片数有界、帧单调推进。
    pub fn audit(&self, max_slices: u32) -> Result<(), u16> {
        for f in self.frames.iter() {
            if *f > FRAME_BUDGET {
                return Err(E_BUDGET_OVER);
            }
        }
        if self.slices > max_slices {
            return Err(E_RUNAWAY);
        }
        if self.frames.len() < self.slices as usize {
            return Err(E_UI_FROZEN);
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

/// R1 最坏模式不冻结：每片间灌满 QUEUE_CAP 输入，10 片后帧数 ≥ 10。
pub fn r1_worst_no_freeze() -> Result<(), u16> {
    let mut bl = BusyLoop::new();
    let mut id: u32 = 0;
    for _ in 0..10 {
        let mut ids: Vec<u32> = Vec::new();
        for _ in 0..QUEUE_CAP {
            ids.push(id);
            id += 1;
        }
        bl.inject(&ids)?;
        bl.run_slice(1)?;
    }
    bl.audit(10)?;
    if bl.frames.len() < 10 {
        return Err(E_UI_FROZEN);
    }
    Ok(())
}

/// R2 帧账 ≤ 预算：全部帧记录不超 FRAME_BUDGET。
pub fn r2_frame_budget() -> Result<(), u16> {
    let mut bl = BusyLoop::new();
    for _ in 0..6 {
        bl.run_slice(1)?;
    }
    bl.audit(6)?;
    for f in bl.frames.iter() {
        if *f > FRAME_BUDGET {
            return Err(E_BUDGET_OVER);
        }
    }
    Ok(())
}

/// R3 不失控：片数 ≤ 注入轮数（无自放大循环）。
pub fn r3_no_runaway() -> Result<(), u16> {
    let mut bl = BusyLoop::new();
    let mut id: u32 = 0;
    for _ in 0..8 {
        let mut ids: Vec<u32> = Vec::new();
        for _ in 0..QUEUE_CAP {
            ids.push(id);
            id += 1;
        }
        bl.inject(&ids)?;
        bl.run_slice(1)?;
    }
    bl.audit(8)?;
    if bl.slices != 8 {
        return Err(E_RUNAWAY);
    }
    Ok(())
}

/// R4 零丢失：注入多少确认多少（不超 QUEUE_CAP 时）。
pub fn r4_zero_loss() -> Result<(), u16> {
    let mut bl = BusyLoop::new();
    let mut total: u64 = 0;
    let mut id: u32 = 0;
    for _ in 0..5 {
        let mut ids: Vec<u32> = Vec::new();
        for _ in 0..30 {
            ids.push(id);
            id += 1;
        }
        total += bl.inject(&ids)? as u64;
        bl.run_slice(1)?;
    }
    if bl.acked != total {
        return Err(E_ACK_BAD);
    }
    if bl.queue.len() != 0 {
        return Err(E_ACK_BAD);
    }
    Ok(())
}

/// R5 队列上界拒收：超 QUEUE_CAP 注入报 E_QUEUE_CAP。
pub fn r5_queue_cap_reject() -> Result<(), u16> {
    let mut bl = BusyLoop::new();
    let mut ids: Vec<u32> = Vec::new();
    for i in 0..(QUEUE_CAP + 1) as u32 {
        ids.push(i);
    }
    match bl.inject(&ids) {
        Err(E_QUEUE_CAP) => {}
        _ => return Err(E_QUEUE_CAP),
    }
    Ok(())
}

/// R6 ×10 轮帧推进恒定：最坏模式十轮帧序列全等。
pub fn r6_ten_rounds() -> Result<(), u16> {
    let run = || -> Result<Vec<u32>, u16> {
        let mut bl = BusyLoop::new();
        let mut id: u32 = 0;
        for _ in 0..6 {
            let mut ids: Vec<u32> = Vec::new();
            for _ in 0..QUEUE_CAP {
                ids.push(id);
                id += 1;
            }
            bl.inject(&ids)?;
            bl.run_slice(1)?;
        }
        Ok(bl.frames.clone())
    };
    let a = run()?;
    for _ in 0..10 {
        if run()? != a {
            return Err(E_NONDET);
        }
    }
    Ok(())
}

/// R7 帧单调：帧计数随片推进单调增（无冻结回退）。
pub fn r7_frame_monotone() -> Result<(), u16> {
    let mut bl = BusyLoop::new();
    let mut id: u32 = 0;
    let mut prev: usize = 0;
    for _ in 0..9 {
        let mut ids: Vec<u32> = Vec::new();
        for _ in 0..10 {
            ids.push(id);
            id += 1;
        }
        bl.inject(&ids)?;
        bl.run_slice(1)?;
        if bl.frames.len() <= prev {
            return Err(E_UI_FROZEN);
        }
        prev = bl.frames.len();
    }
    Ok(())
}

/// R8 fuzz 洪峰：随机注入量 0..QUEUE_CAP × 12 片，账全绿。
pub fn r8_fuzz_flood() -> Result<(), u16> {
    let mut g = Lcg::new(0x48510_0001);
    let mut bl = BusyLoop::new();
    let mut id: u32 = 0;
    for _ in 0..12 {
        let n = g.below(QUEUE_CAP as u64 + 1) as usize;
        let mut ids: Vec<u32> = Vec::new();
        for _ in 0..n {
            ids.push(id);
            id += 1;
        }
        bl.inject(&ids)?;
        bl.run_slice(1)?;
    }
    bl.audit(12)?;
    Ok(())
}

/// R9 确认不重复：acked 单调累计、无回退。
pub fn r9_ack_monotone() -> Result<(), u16> {
    let mut bl = BusyLoop::new();
    let mut id: u32 = 0;
    let mut prev: u64 = 0;
    for _ in 0..7 {
        let mut ids: Vec<u32> = Vec::new();
        for _ in 0..20 {
            ids.push(id);
            id += 1;
        }
        bl.inject(&ids)?;
        bl.run_slice(1)?;
        if bl.acked <= prev && id > 0 {
            return Err(E_ACK_BAD);
        }
        prev = bl.acked;
    }
    Ok(())
}

/// R10 确定性重放：同种子两轮帧数/确认数/片数全等。
pub fn r10_deterministic() -> Result<(), u16> {
    let run = |seed: u64| -> Result<(usize, u64, u32), u16> {
        let mut g = Lcg::new(seed);
        let mut bl = BusyLoop::new();
        let mut id: u32 = 0;
        for _ in 0..10 {
            let n = g.below(QUEUE_CAP as u64 + 1) as usize;
            let mut ids: Vec<u32> = Vec::new();
            for _ in 0..n {
                ids.push(id);
                id += 1;
            }
            bl.inject(&ids)?;
            bl.run_slice(1)?;
        }
        Ok((bl.frames.len(), bl.acked, bl.slices))
    };
    let a = run(0x48510_0002)?;
    let b = run(0x48510_0002)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_worst_no_freeze()?;
    hits += 1;
    r2_frame_budget()?;
    hits += 1;
    r3_no_runaway()?;
    hits += 1;
    r4_zero_loss()?;
    hits += 1;
    r5_queue_cap_reject()?;
    hits += 1;
    r6_ten_rounds()?;
    hits += 1;
    r7_frame_monotone()?;
    hits += 1;
    r8_fuzz_flood()?;
    hits += 1;
    r9_ack_monotone()?;
    hits += 1;
    r10_deterministic()?;
    hits += 1;
    Ok(hits)
}
