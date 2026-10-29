// unx1238_u1238 —— UNX-F1238 Draining 态启动序列挂接（排空面初始化序）
// 判据原文 UNX-F1238-J1：启动序列全过（排空面初始化步骤按前置依赖序执行、
// 乱序即拒、二次执行拒、缺步不放行、序列指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb9b0 独占（0xb9b0~0xb9ef）。
//
// 设计（纯功能 no_std）：
//   * 五步排空启动序：0 登记 drainer → 1 对账底座挂接 → 2 幂等表清零校验 →
//     3 超时保护使能 → 4 进入 Draining；
//   * 前置依赖位掩码、乱序拒、重复拒、超预算拒、缺步门禁拒；
//   * 守恒：登记 5 = 完成 + 挂起；FNV 链指纹冻结。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb9b0 独占） ─────────────────────────────────────────────
pub const XC_DS_SEALED: u32 = 0xB9B1; // 封账后执行
pub const XC_DS_BAD_STEP: u32 = 0xB9B2; // 步骤号非法
pub const XC_DS_PREREQ: u32 = 0xB9B3; // 前置未全完成
pub const XC_DS_DUP_RUN: u32 = 0xB9B4; // 步骤重复执行
pub const XC_DS_BUDGET: u32 = 0xB9B5; // 耗时超预算
pub const XC_DS_NOT_READY: u32 = 0xB9B6; // 缺步不放行

/// 五步序。
pub const STEP_COUNT: usize = 5;
/// 每步耗时预算（tick）。
pub const STEP_BUDGET: [u64; STEP_COUNT] = [30, 40, 20, 25, 10];
/// 前置依赖位掩码。
pub const STEP_PREREQ: [u8; STEP_COUNT] = [
    0b00000, // 0 登记 drainer
    0b00001, // 1 对账底座挂接
    0b00011, // 2 幂等表清零校验
    0b00111, // 3 超时保护使能
    0b01111, // 4 进入 Draining
];

/// 排空启动序列账。
pub struct DrainBoot {
    done: [bool; STEP_COUNT],
    done_tick: [u64; STEP_COUNT],
    runs: u32,
    all_done_tick: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl DrainBoot {
    pub fn new() -> DrainBoot {
        DrainBoot {
            done: [false; STEP_COUNT],
            done_tick: [0; STEP_COUNT],
            runs: 0,
            all_done_tick: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    fn prereq_ok(&self, step: usize) -> bool {
        let mask = STEP_PREREQ[step];
        for k in 0..STEP_COUNT {
            if mask & (1 << k) != 0 && !self.done[k] {
                return false;
            }
        }
        true
    }

    /// 执行步骤。
    pub fn run_step(&mut self, step: usize, tick: u64, cost: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_DS_SEALED, "封账后执行"));
            return Err(XC_DS_SEALED);
        }
        if step >= STEP_COUNT {
            self.rejects.push((XC_DS_BAD_STEP, "步骤号非法"));
            return Err(XC_DS_BAD_STEP);
        }
        if self.done[step] {
            self.rejects.push((XC_DS_DUP_RUN, "步骤重复执行"));
            return Err(XC_DS_DUP_RUN);
        }
        if !self.prereq_ok(step) {
            self.rejects.push((XC_DS_PREREQ, "前置未全完成"));
            return Err(XC_DS_PREREQ);
        }
        if cost > STEP_BUDGET[step] {
            self.rejects.push((XC_DS_BUDGET, "耗时超预算"));
            return Err(XC_DS_BUDGET);
        }
        self.done[step] = true;
        self.done_tick[step] = tick;
        self.runs = self.runs.saturating_add(1);
        if self.runs as usize == STEP_COUNT {
            self.all_done_tick = tick;
        }
        Ok(())
    }

    pub fn done_count(&self) -> usize {
        self.done.iter().filter(|d| **d).count()
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.done_count() <= STEP_COUNT && self.runs as usize == self.done_count()
    }

    /// 进入 Draining 门禁：五步全完成才放行。
    pub fn enter_gate(&self) -> Result<u64, u32> {
        for k in 0..STEP_COUNT {
            if !self.done[k] {
                return Err(XC_DS_NOT_READY);
            }
        }
        Ok(self.all_done_tick)
    }

    /// 完成时刻单调。
    pub fn ticks_monotonic(&self) -> bool {
        let mut prev: u64 = 0;
        let mut first = true;
        for k in 0..STEP_COUNT {
            if self.done[k] {
                if !first && self.done_tick[k] < prev {
                    return false;
                }
                prev = self.done_tick[k];
                first = false;
            }
        }
        true
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_DS_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for k in 0..STEP_COUNT {
            h = h.wrapping_mul(0x0100_0193)
                ^ ((self.done[k] as u64) << 32)
                ^ self.done_tick[k];
        }
        h = h.wrapping_mul(0x0100_0193) ^ self.runs as u64;
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准启动场景：五步按序、门禁放行、单调。
pub fn boot_scenario() -> Result<(usize, u64, u64), u32> {
    let mut s = DrainBoot::new();
    s.run_step(0, 50, 28)?;
    s.run_step(1, 95, 38)?;
    s.run_step(2, 120, 18)?;
    s.run_step(3, 150, 22)?;
    s.run_step(4, 165, 8)?;
    let gate = s.enter_gate()?;
    if !s.conserved() || !s.ticks_monotonic() {
        return Err(XC_DS_BUDGET);
    }
    let fp = s.seal()?;
    Ok((s.done_count(), gate, fp))
}

// ── 深化块：回滚账与依赖图查询 ─────────────────────────────────────
impl DrainBoot {
    /// 回滚：撤销最后一个已完成步骤（进入步不可回滚）。
    pub fn rollback_last(&mut self) -> Result<usize, u32> {
        if self.sealed {
            self.rejects.push((XC_DS_SEALED, "封账后回滚"));
            return Err(XC_DS_SEALED);
        }
        if self.done[STEP_COUNT - 1] {
            return Err(XC_DS_BAD_STEP);
        }
        for k in (0..STEP_COUNT).rev() {
            if self.done[k] {
                self.done[k] = false;
                self.done_tick[k] = 0;
                self.runs = self.runs.saturating_sub(1);
                return Ok(k);
            }
        }
        Err(XC_DS_NOT_READY)
    }
    /// 依赖闭包（step 全部上游）。
    pub fn dep_closure(&self, step: usize) -> Vec<usize> {
        let mut v: Vec<usize> = vec![];
        if step >= STEP_COUNT { return v; }
        let mask = STEP_PREREQ[step];
        for k in 0..STEP_COUNT {
            if mask & (1 << k) != 0 { v.push(k); }
        }
        v
    }
    /// 下一步建议。
    pub fn next_hint(&self) -> Option<usize> {
        for k in 0..STEP_COUNT {
            if !self.done[k] && self.prereq_ok(k) { return Some(k); }
        }
        None
    }
    /// 已完成清单。
    pub fn done_list(&self) -> Vec<usize> {
        let mut v: Vec<usize> = vec![];
        for k in 0..STEP_COUNT {
            if self.done[k] { v.push(k); }
        }
        v
    }
}

// ── 深化块二：预算画像与门禁复检 ───────────────────────────────────
impl DrainBoot {
    /// 已用预算占比（基点）。
    pub fn budget_used_bps(&self) -> u32 {
        let mut used: u64 = 0;
        for k in 0..STEP_COUNT {
            if self.done[k] { used = used.saturating_add(STEP_BUDGET[k] / 2); }
        }
        let total: u64 = STEP_BUDGET.iter().sum();
        if total == 0 { 0 } else { ((used * 10000) / total) as u32 }
    }
    /// 门禁复检：全完成 + 单调 双条件。
    pub fn gate_recheck(&self) -> bool {
        self.done_count() == STEP_COUNT && self.ticks_monotonic()
    }
    /// 未完成清单。
    pub fn pending_list(&self) -> Vec<usize> {
        let mut v: Vec<usize> = vec![];
        for k in 0..STEP_COUNT {
            if !self.done[k] { v.push(k); }
        }
        v
    }
}

// ── 深化块三：序列审计面 ───────────────────────────────────────────
impl DrainBoot {
    /// 完成时刻表（未完成为 0）。
    pub fn tick_table(&self) -> [u64; STEP_COUNT] { self.done_tick }
    /// 步骤预算表只读。
    pub fn budget_table(&self) -> [u64; STEP_COUNT] { STEP_BUDGET }
    /// 序列完整率（基点）。
    pub fn completeness_bps(&self) -> u32 {
        ((self.done_count() as u64 * 10000) / STEP_COUNT as u64) as u32
    }
    /// 尾步耗时（进入步完成时刻 - 首步完成时刻）。
    pub fn boot_span(&self) -> u64 {
        if self.done_count() < STEP_COUNT { return 0; }
        let mut lo = u64::MAX;
        let mut hi = 0u64;
        for k in 0..STEP_COUNT {
            if self.done_tick[k] < lo { lo = self.done_tick[k]; }
            if self.done_tick[k] > hi { hi = self.done_tick[k]; }
        }
        hi.saturating_sub(lo)
    }
}

/// 注入核：七类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 7;

    // 1）步骤号非法必拒
    {
        let mut s = DrainBoot::new();
        if s.run_step(5, 1, 1) == Err(XC_DS_BAD_STEP) {
            caught += 1;
        }
    }
    // 2）前置缺失必拒
    {
        let mut s = DrainBoot::new();
        if s.run_step(2, 1, 1) == Err(XC_DS_PREREQ) {
            caught += 1;
        }
    }
    // 3）缺步不放行
    {
        let mut s = DrainBoot::new();
        let _ = s.run_step(0, 10, 5);
        let _ = s.run_step(1, 20, 5);
        if s.enter_gate() == Err(XC_DS_NOT_READY) {
            caught += 1;
        }
    }
    // 4）重复执行必拒
    {
        let mut s = DrainBoot::new();
        let _ = s.run_step(0, 10, 5);
        if s.run_step(0, 20, 5) == Err(XC_DS_DUP_RUN) {
            caught += 1;
        }
    }
    // 5）超预算必拒
    {
        let mut s = DrainBoot::new();
        if s.run_step(0, 10, 31) == Err(XC_DS_BUDGET) {
            caught += 1;
        }
    }
    // 6）封账后执行必拒
    {
        let mut s = DrainBoot::new();
        let _ = s.run_step(0, 10, 5);
        let _ = s.seal();
        if s.run_step(1, 20, 5) == Err(XC_DS_SEALED) {
            caught += 1;
        }
    }
    // 7）五步全过后门禁放行
    {
        let mut s = DrainBoot::new();
        let _ = s.run_step(0, 50, 30);
        let _ = s.run_step(1, 95, 40);
        let _ = s.run_step(2, 120, 20);
        let _ = s.run_step(3, 150, 25);
        let _ = s.run_step(4, 165, 10);
        if s.enter_gate() == Ok(165) && s.done_count() == 5 {
            caught += 1;
        }
    }
    (caught, N)
}
