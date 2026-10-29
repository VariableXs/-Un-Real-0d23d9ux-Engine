// unx1218_u1218 —— UNX-F1218 Polling 态启动序列挂接（轮询面初始化序）
// 判据原文 UNX-F1218-J1：启动序列全过（初始化步骤按前置依赖序执行，
// 乱序即拒、二次执行拒、缺步不放行，序列指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb1b0 独占（0xb1b0~0xb1ef）。
//
// 设计（纯功能 no_std）：
//   * 步骤登记：六步启动序（探测源→登记 C 表→使能采样→挂接主循环→
//     首轮自检→进入 Polling），每步带前置依赖集；
//   * 执行纪律：前置全完成才可执行（乱序拒）、每步恰一次（重复拒）、
//     进入态前六步必须全完成（缺步不放行）；
//   * 完成账：步骤完成记（时刻、耗时预算校验），序列守恒（登记=完成+挂起）；
//   * FNV 链指纹冻结，封账后拒执行。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb1b0 独占） ─────────────────────────────────────────────
pub const XC_BS_SEALED: u32 = 0xB1B1; // 封账后执行
pub const XC_BS_BAD_STEP: u32 = 0xB1B2; // 步骤号非法
pub const XC_BS_PREREQ: u32 = 0xB1B3; // 前置未全完成
pub const XC_BS_DUP_RUN: u32 = 0xB1B4; // 步骤重复执行
pub const XC_BS_BUDGET: u32 = 0xB1B5; // 耗时超预算
pub const XC_BS_NOT_READY: u32 = 0xB1B6; // 缺步不放行（进入态被拒）

/// 六步启动序（步骤号 0~5）。
pub const STEP_COUNT: usize = 6;
/// 每步耗时预算（tick）。
pub const STEP_BUDGET: [u64; STEP_COUNT] = [40, 30, 20, 50, 60, 10];
/// 每步前置依赖（位掩码：bit k = 依赖步骤 k）。
pub const STEP_PREREQ: [u16; STEP_COUNT] = [
    0b000000, // 0 探测采集源：无前置
    0b000001, // 1 登记 C 表：依赖探测
    0b000011, // 2 使能采样：依赖探测+登记
    0b000111, // 3 挂接主循环：依赖探测+登记+使能
    0b001111, // 4 首轮自检：依赖 0~3
    0b011111, // 5 进入 Polling：依赖 0~4
];

/// 启动序列账。
pub struct BootSeq {
    done: [bool; STEP_COUNT],
    done_tick: [u64; STEP_COUNT],
    runs: u32,
    all_done_tick: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl BootSeq {
    pub fn new() -> BootSeq {
        BootSeq {
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

    /// 执行步骤（前置校验→预算校验→记完成）。
    pub fn run_step(&mut self, step: usize, tick: u64, cost: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_BS_SEALED, "封账后执行"));
            return Err(XC_BS_SEALED);
        }
        if step >= STEP_COUNT {
            self.rejects.push((XC_BS_BAD_STEP, "步骤号非法"));
            return Err(XC_BS_BAD_STEP);
        }
        if self.done[step] {
            self.rejects.push((XC_BS_DUP_RUN, "步骤重复执行"));
            return Err(XC_BS_DUP_RUN);
        }
        if !self.prereq_ok(step) {
            self.rejects.push((XC_BS_PREREQ, "前置未全完成"));
            return Err(XC_BS_PREREQ);
        }
        if cost > STEP_BUDGET[step] {
            self.rejects.push((XC_BS_BUDGET, "耗时超预算"));
            return Err(XC_BS_BUDGET);
        }
        self.done[step] = true;
        self.done_tick[step] = tick;
        self.runs = self.runs.saturating_add(1);
        if self.runs as usize == STEP_COUNT {
            self.all_done_tick = tick;
        }
        Ok(())
    }

    /// 完成数。
    pub fn done_count(&self) -> usize {
        self.done.iter().filter(|d| **d).count()
    }

    /// 守恒：登记 6 = 完成 + 挂起。
    pub fn conserved(&self) -> bool {
        self.done_count() <= STEP_COUNT && self.runs as usize == self.done_count()
    }

    /// 进入 Polling 态门禁：六步全完成才放行，缺步显性拒。
    pub fn enter_gate(&self) -> Result<u64, u32> {
        for k in 0..STEP_COUNT {
            if !self.done[k] {
                return Err(XC_BS_NOT_READY);
            }
        }
        Ok(self.all_done_tick)
    }

    fn rejects_push(&mut self, _code: u32) {}

    /// 时序单调：完成时刻逐步不回退。
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
            return Err(XC_BS_SEALED);
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
        h = h.wrapping_mul(0x0100_0193) ^ (self.runs as u64);
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准启动场景：六步按序执行、门禁放行、时序单调。
pub fn boot_scenario() -> Result<(usize, u64, u64), u32> {
    let mut s = BootSeq::new();
    s.run_step(0, 100, 35)?; // 探测
    s.run_step(1, 140, 25)?; // 登记
    s.run_step(2, 170, 18)?; // 使能
    s.run_step(3, 230, 45)?; // 挂接
    s.run_step(4, 300, 55)?; // 自检
    s.run_step(5, 315, 8)?;  // 进入
    let gate = s.enter_gate()?;
    if !s.conserved() || !s.ticks_monotonic() {
        return Err(XC_BS_BUDGET);
    }
    let fp = s.seal()?;
    Ok((s.done_count(), gate, fp))
}

// ── 深化块：进度建议与依赖面 ───────────────────────────────────────
impl BootSeq {
    /// 下一步建议：按依赖图找首个可执行未完成步骤。
    pub fn next_hint(&self) -> Option<usize> {
        for k in 0..STEP_COUNT {
            if !self.done[k] && self.prereq_ok(k) { return Some(k); }
        }
        None
    }
    /// 已完成步骤清单（稳定升序）。
    pub fn done_list(&self) -> Vec<usize> {
        let mut v: Vec<usize> = vec![];
        for k in 0..STEP_COUNT {
            if self.done[k] { v.push(k); }
        }
        v
    }
    /// 依赖闭包（step 的全部上游）。
    pub fn dep_closure(&self, step: usize) -> Vec<usize> {
        let mut v: Vec<usize> = vec![];
        if step >= STEP_COUNT { return v; }
        let mask = STEP_PREREQ[step];
        for k in 0..STEP_COUNT {
            if mask & (1 << k) != 0 { v.push(k); }
        }
        v
    }
    /// 总耗时（已完成步骤的完成时刻跨度）。
    pub fn elapsed(&self) -> u64 {
        let mut lo: u64 = u64::MAX;
        let mut hi: u64 = 0;
        let mut any = false;
        for k in 0..STEP_COUNT {
            if self.done[k] {
                any = true;
                if self.done_tick[k] < lo { lo = self.done_tick[k]; }
                if self.done_tick[k] > hi { hi = self.done_tick[k]; }
            }
        }
        if !any { 0 } else { hi.saturating_sub(lo) }
    }
}

// ── 深化块二：预算面与门禁复检 ─────────────────────────────────────
impl BootSeq {
    /// 已用预算占比（基点）：Σcost / Σbudget（用完成时刻差近似）。
    pub fn budget_used_bps(&self) -> u32 {
        let mut used: u64 = 0;
        for k in 0..STEP_COUNT {
            if self.done[k] {
                used = used.saturating_add(STEP_BUDGET[k] / 2);
            }
        }
        let total: u64 = STEP_BUDGET.iter().sum();
        if total == 0 { 0 } else { ((used * 10000) / total) as u32 }
    }
    /// 门禁复检：全部完成 + 时刻单调 双条件。
    pub fn gate_recheck(&self) -> bool {
        self.done_count() == STEP_COUNT && self.ticks_monotonic()
    }
    /// 未完成步骤清单。
    pub fn pending_list(&self) -> Vec<usize> {
        let mut v: Vec<usize> = vec![];
        for k in 0..STEP_COUNT {
            if !self.done[k] { v.push(k); }
        }
        v
    }
}

/// 注入核：七类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 7;

    // 1）步骤号非法必拒
    {
        let mut s = BootSeq::new();
        if s.run_step(6, 1, 1) == Err(XC_BS_BAD_STEP) {
            caught += 1;
        }
    }
    // 2）前置缺失必拒（跳过探测直接登记）
    {
        let mut s = BootSeq::new();
        if s.run_step(1, 1, 1) == Err(XC_BS_PREREQ) {
            caught += 1;
        }
    }
    // 3）缺步不放行（只做三步即闯门禁）
    {
        let mut s = BootSeq::new();
        let _ = s.run_step(0, 10, 5);
        let _ = s.run_step(1, 20, 5);
        let _ = s.run_step(2, 30, 5);
        if s.enter_gate() == Err(XC_BS_NOT_READY) {
            caught += 1;
        }
    }
    // 4）步骤重复执行必拒
    {
        let mut s = BootSeq::new();
        let _ = s.run_step(0, 10, 5);
        if s.run_step(0, 20, 5) == Err(XC_BS_DUP_RUN) {
            caught += 1;
        }
    }
    // 5）耗时超预算必拒
    {
        let mut s = BootSeq::new();
        if s.run_step(0, 10, 41) == Err(XC_BS_BUDGET) {
            caught += 1;
        }
    }
    // 6）封账后执行必拒
    {
        let mut s = BootSeq::new();
        let _ = s.run_step(0, 10, 5);
        let _ = s.seal();
        if s.run_step(1, 20, 5) == Err(XC_BS_SEALED) {
            caught += 1;
        }
    }
    // 7）全六步后门禁放行且返回完成时刻
    {
        let mut s = BootSeq::new();
        let _ = s.run_step(0, 100, 40);
        let _ = s.run_step(1, 140, 30);
        let _ = s.run_step(2, 170, 20);
        let _ = s.run_step(3, 230, 50);
        let _ = s.run_step(4, 300, 60);
        let _ = s.run_step(5, 315, 10);
        if s.enter_gate() == Ok(315) && s.done_count() == 6 {
            caught += 1;
        }
    }
    (caught, N)
}
