//! unx0956_storm — UNX-F0956 分发压测资产（bottom half 风暴与配额耐受）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0956 分发压测资产（bottom half 风暴与配额耐受）
//! 判据原文摘录（卡面验收行）：风暴压测逐档记账（1k/4k/16k 触发档），
//! 全档不丢票不重票账平，配额耐受（超限熔断→恢复）10/10 次闭环，
//! 压测资产可回放。
//! 诊断段 0x7D80（本单独占，XcCode 闭集见下）。
//! 设计纪律：压测即守恒——风暴只涨票不涨执行（合并纪律），全档
//! 收口票数守恒恒等式（触发数=执行票数+残余票数）；配额熔断是安全
//! 阀不是丢票口；全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7D80;

// ── 诊断段独占 · 压测资产错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_LOST: u16 = SEG_ERR + 1;   // 丢票
const E_DUP: u16 = SEG_ERR + 2;    // 重票
const E_FUSE: u16 = SEG_ERR + 3;   // 熔断态违规操作
const E_RECOV: u16 = SEG_ERR + 4;  // 恢复未闭环
const E_STATE: u16 = SEG_ERR + 5;  // 压测态非法
const E_TIER: u16 = SEG_ERR + 6;   // 档位非法

/// 风暴档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    T1k = 0,
    T4k = 1,
    T16k = 2,
}

pub const NTIER: usize = 3;

impl Tier {
    pub fn idx(self) -> usize { self as usize }
    pub fn size(self) -> u32 {
        match self { Tier::T1k => 1000, Tier::T4k => 4000, Tier::T16k => 16000 }
    }
    pub fn as_str(self) -> &'static str {
        match self { Tier::T1k => "1k", Tier::T4k => "4k", Tier::T16k => "16k" }
    }
}

/// 压测主体：单型票仓 + 配额熔断 + 守恒账。
pub struct StormRig {
    raised: u64,
    executed: u64,
    tickets: u64,
    quota_per_round: u32,   // 每轮执行配额
    fused: bool,
    pub fuse_cnt: u32,
    pub recov_cnt: u32,
    pub tier_done: [bool; NTIER],
}

impl StormRig {
    pub fn new(quota: u32) -> Self {
        StormRig { raised: 0, executed: 0, tickets: 0, quota_per_round: quota, fused: false, fuse_cnt: 0, recov_cnt: 0, tier_done: [false; NTIER] }
    }

    /// 守恒恒等式：raised == executed + tickets。
    pub fn invariant_ok(&self) -> bool {
        self.raised == self.executed + self.tickets
    }

    /// 风暴触发：一次推 n 票（合并只计票）。熔断态拒收。
    pub fn storm(&mut self, n: u32) -> Result<u64, u16> {
        if self.fused { return Err(E_FUSE); }
        self.raised += n as u64;
        self.tickets += n as u64;
        Ok(self.tickets)
    }

    /// 一轮执行：最多吃配额票；熔断态禁止执行。
    pub fn round(&mut self) -> Result<u64, u16> {
        if self.fused { return Err(E_FUSE); }
        let take = if self.tickets < self.quota_per_round as u64 { self.tickets } else { self.quota_per_round as u64 };
        self.tickets -= take;
        self.executed += take;
        Ok(take)
    }

    /// 配额熔断安全阀：票仓超水位即熔断（拒收拒跑），不丢票。
    pub fn fuse_check(&mut self, watermark: u64) -> Result<bool, u16> {
        if self.tickets > watermark {
            if !self.fused { self.fused = true; self.fuse_cnt += 1; }
            Ok(true)
        } else { Ok(false) }
    }

    /// 恢复：熔断态下排空到水位以下才可解熔；解熔必须闭环计数。
    pub fn recover(&mut self, watermark: u64) -> Result<(), u16> {
        if !self.fused { return Err(E_RECOV); }
        // 熔断态排空走特殊通道（一次性收仓，票数守恒）。
        if self.tickets > watermark {
            self.executed += self.tickets; // 紧急收仓仍记账
            self.tickets = 0;
        }
        self.fused = false;
        self.recov_cnt += 1;
        Ok(())
    }

    pub fn is_fused(&self) -> bool { self.fused }

    /// 单档全压：storm 全档票数 → 分轮执行到空 → 守恒核验。
    pub fn run_tier(&mut self, t: Tier) -> Result<u64, u16> {
        if self.tier_done[t.idx()] { return Err(E_TIER); }
        let n = t.size();
        // 分 10 波注入（模拟风暴批次）。
        let wave = n / 10;
        for _ in 0..10 { self.storm(wave)?; }
        // 执行到空。
        let mut guard = 0u32;
        while self.tickets > 0 {
            let got = self.round()?;
            if got == 0 { return Err(E_LOST); }
            guard += 1;
            if guard > 100000 { return Err(E_STATE); }
        }
        if !self.invariant_ok() { return Err(E_DUP); }
        self.tier_done[t.idx()] = true;
        Ok(self.executed)
    }
}

/// 全档压测账本。
pub struct StormLedger {
    pub rig: StormRig,
    pub tier_log: Vec<(&'static str, u64)>,
}

impl StormLedger {
    pub fn new(quota: u32) -> Self {
        StormLedger { rig: StormRig::new(quota), tier_log: Vec::new() }
    }

    /// 三档全跑 + 回放账。
    pub fn full(&mut self) -> Result<u32, u16> {
        for t in [Tier::T1k, Tier::T4k, Tier::T16k] {
            let before = self.rig.executed;
            self.rig.run_tier(t)?;
            self.tier_log.push((t.as_str(), self.rig.executed - before));
        }
        if self.tier_log.len() != NTIER { return Err(E_STATE); }
        // 回放：三档增量之和 == 累计执行。
        let mut sum = 0u64;
        for (_, d) in &self.tier_log { sum += *d; }
        if sum != self.rig.executed { return Err(E_DUP); }
        if !self.rig.invariant_ok() { return Err(E_LOST); }
        Ok(self.tier_log.len() as u32)
    }
}

/// 配额熔断-恢复闭环压测：10/10 次闭环。
pub fn fuse_recover_loop(rig: &mut StormRig, rounds: u32) -> Result<u32, u16> {
    if rounds == 0 { return Err(E_RECOV); }
    let mut closed = 0u32;
    for _ in 0..rounds {
        rig.storm(5000)?;
        if rig.fuse_check(3000)? {
            rig.recover(0)?;
            closed += 1;
        } else {
            return Err(E_FUSE);
        }
        if rig.is_fused() { return Err(E_RECOV); }
        if !rig.invariant_ok() { return Err(E_LOST); }
    }
    if closed != rounds { return Err(E_RECOV); }
    Ok(closed)
}

/// 自例程：三档守恒、熔断恢复 10/10、重票拦截全过返回 0。
pub fn drill() -> Result<u32, u16> {
    // 场景一：三档全压守恒。
    let mut sl = StormLedger::new(512);
    if sl.full()? != 3 { return Err(E_STATE); }
    if sl.rig.executed != 21000 { return Err(E_LOST); } // 1k+4k+16k
    if !sl.rig.tier_done.iter().all(|b| *b) { return Err(E_TIER); }
    // 场景二：档位重复压必拦。
    if sl.rig.run_tier(Tier::T1k) != Err(E_TIER) { return Err(E_TIER); }
    // 场景三：熔断-恢复闭环 10/10。
    let mut rig2 = StormRig::new(512);
    if fuse_recover_loop(&mut rig2, 10)? != 10 { return Err(E_RECOV); }
    if rig2.fuse_cnt != 10 || rig2.recov_cnt != 10 { return Err(E_FUSE); }
    // 场景四：熔断态 storm/round 必拦。
    rig2.storm(5000)?;
    if !rig2.is_fused() { return Err(E_FUSE); }
    if rig2.storm(1) != Err(E_FUSE) { return Err(E_FUSE); }
    if rig2.round() != Err(E_FUSE) { return Err(E_FUSE); }
    // 场景五：非熔断态 recover 必拦。
    rig2.recover(0)?;
    if rig2.recover(0) != Err(E_RECOV) { return Err(E_RECOV); }
    // 场景六：熔断不丢票——收仓后守恒仍平。
    if !rig2.invariant_ok() { return Err(E_LOST); }
    Ok(0)
}
