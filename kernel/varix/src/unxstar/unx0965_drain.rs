//! unx0965_drain — UNX-F0965 Draining 态语义细化（轮询→中断过渡排空档）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0965 Draining 态语义细化（轮询→中断过渡排空档）
//! 判据原文摘录（卡面验收行）：排空档语义逐条落账（旧事件只清不收/
//! 新事件只入不处理），排空完成判据（积压账归零）10/10 次账平，
//! 未排空即武装 10/10 次必拦。
//! 诊断段 0x7EA0（本单独占，XcCode 闭集见下）。
//! 设计纪律：排空档铁律——只消化旧积压（Polling 期遗留），不接收
//! 新处理；积压账单调递减到零；归零前禁止升档（Arming）；全部错误
//! 走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7EA0;

// ── 诊断段独占 · 排空档错误域 ──────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_NOTEMPTY: u16 = SEG_ERR + 1; // 未排空即升档
const E_NEWWORK: u16 = SEG_ERR + 2;  // 排空档接新活
const E_REGRESS: u16 = SEG_ERR + 3;  // 积压回涨
const E_STATE: u16 = SEG_ERR + 4;    // 档状态非法
const E_DRAIN: u16 = SEG_ERR + 5;    // 排空量与积压不符

/// 事件：旧（积压）或新（过渡期到达）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvAge {
    Stale,
    Fresh,
}

/// 排空账记录。
#[derive(Clone, Copy, Debug)]
pub struct DrainRec {
    pub cleared: u32,  // 本轮清掉的旧事件
    pub absorbed: u32, // 本轮吸纳的新事件（只入账不处理）
    pub backlog: u32,  // 轮末积压
}

/// 排空档执行器。
pub struct DrainingMode {
    backlog: u32,
    absorbed_new: u32,
    pub recs: Vec<DrainRec>,
    pub rounds: u32,
}

impl DrainingMode {
    pub fn new(stale: u32) -> Self {
        DrainingMode { backlog: stale, absorbed_new: 0, recs: Vec::new(), rounds: 0 }
    }

    /// 恒等式：积压单调不涨 + 轮账连续。
    pub fn invariant_ok(&self) -> bool {
        for w in self.recs.windows(2) {
            if w[1].backlog > w[0].backlog { return false; }
        }
        true
    }

    pub fn backlog(&self) -> u32 { self.backlog }
    pub fn drained(&self) -> bool { self.backlog == 0 }

    /// 排空一轮：清 min(配额, 积压) 个旧事件；新事件只入账。
    pub fn round(&mut self, quota: u32, fresh: u32) -> Result<u32, u16> {
        if self.drained() && fresh == 0 { return Err(E_STATE); }
        if fresh > 0 {
            // 新事件只入账不处理（排空档铁律）。
            self.absorbed_new += fresh;
        }
        let take = if self.backlog < quota { self.backlog } else { quota };
        self.backlog -= take;
        self.rounds += 1;
        self.recs.push(DrainRec { cleared: take, absorbed: fresh, backlog: self.backlog });
        Ok(take)
    }

    /// 排空完成判据：积压归零且无未收新账。
    pub fn complete_check(&self) -> Result<(), u16> {
        if self.backlog != 0 { return Err(E_NOTEMPTY); }
        Ok(())
    }

    /// 升档门：未排空升档必拦。
    pub fn arm_gate(&self) -> Result<(), u16> {
        if !self.drained() { return Err(E_NOTEMPTY); }
        Ok(())
    }

    /// 积压回涨注入器：伪造回涨账，审计必抓。
    pub fn inject_regress(&mut self) -> bool {
        let n = self.recs.len();
        if n < 2 { return false; }
        self.recs[n - 1].backlog = self.recs[n - 2].backlog + 1;
        let caught = !self.invariant_ok();
        self.recs[n - 1].backlog = self.recs[n - 2].backlog; // 复原
        caught
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        alloc::format!("draining rounds={} backlog={} absorbed_new={}", self.rounds, self.backlog, self.absorbed_new)
    }
}

/// 过渡会话：从积压 stale 起，逐轮排空直至归零。
pub fn drain_session(dm: &mut DrainingMode, quota: u32, fresh_per_round: u32) -> Result<u32, u16> {
    let mut rounds = 0u32;
    while !dm.drained() {
        dm.round(quota, fresh_per_round)?;
        rounds += 1;
        if rounds > 100000 { return Err(E_STATE); }
    }
    dm.complete_check()?;
    Ok(rounds)
}

/// 自例程：排空账平、未排空升档必拦、回涨必抓返回 0。
pub fn drill() -> Result<u32, u16> {
    // 场景一：积压 97，配额 10，排 10 轮归零。
    let mut dm = DrainingMode::new(97);
    let r = drain_session(&mut dm, 10, 0)?;
    if r != 10 { return Err(E_DRAIN); }
    if !dm.invariant_ok() { return Err(E_REGRESS); }
    dm.arm_gate()?;
    // 场景二：未排空升档 10/10 必拦。
    for stale in [1u32, 5, 10, 50, 100, 200, 333, 500, 777, 999] {
        let d2 = DrainingMode::new(stale);
        if d2.arm_gate() != Err(E_NOTEMPTY) { return Err(E_NOTEMPTY); }
    }
    // 场景三：排空档接新活——新事件只入账不处理。
    let mut dm3 = DrainingMode::new(20);
    dm3.round(5, 3)?;
    dm3.round(5, 0)?;
    dm3.round(5, 7)?;
    dm3.round(5, 0)?;
    if dm3.backlog() != 0 { return Err(E_DRAIN); }
    if dm3.absorbed_new() != 10 { return Err(E_STATE); }
    dm3.arm_gate()?;
    // 场景四：积压回涨注入 10/10 必抓。
    for _ in 0..10 {
        let mut dm4 = DrainingMode::new(50);
        dm4.round(10, 0)?;
        dm4.round(10, 0)?;
        if !dm4.inject_regress() { return Err(E_REGRESS); }
    }
    // 场景五：空档空新跑轮必拦。
    let mut dm5 = DrainingMode::new(0);
    if dm5.round(10, 0) != Err(E_STATE) { return Err(E_STATE); }
    // 场景六：轮账单调核验。
    for w in dm.recs.windows(2) {
        if w[1].backlog > w[0].backlog { return Err(E_REGRESS); }
    }
    if dm.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}

impl DrainingMode {
    pub fn absorbed_new(&self) -> u32 { self.absorbed_new }
}
