//! unx0950_stall — UNX-F0950 分发停滞分析（bottom half 不跑的因果账）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0950 分发停滞分析（bottom half 不跑的因果账）
//! 判据原文摘录（卡面验收行）：bottom half 不跑的因果账逐因可查
//! （中断关死/票被吞/线程饿死/优先级倒挂四类），每种注入 10/10 次
//! 必抓并归因，健康路径零误报。
//! 诊断段 0x7CC0（本单独占，XcCode 闭集见下）。
//! 设计纪律：停滞必有因——因果账四因枚举闭集，观测面（pending 位
//! +票仓+运行位）三面差分定位；误报与漏报双计零容忍；全部错误走
//! Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7CC0;

// ── 诊断段独占 · 分发停滞错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_IRQOFF: u16 = SEG_ERR + 1; // 因一：中断关死
const E_SWALLOW: u16 = SEG_ERR + 2; // 因二：票被吞（有位无票/有票无位）
const E_STARVE: u16 = SEG_ERR + 3;  // 因三：线程饿死
const E_INVERT: u16 = SEG_ERR + 4;  // 因四：优先级倒挂
const E_FALSE: u16 = SEG_ERR + 5;   // 误报（健康路径报病）
const E_MISS: u16 = SEG_ERR + 6;    // 漏报（病态路径放行）

/// 四类停滞因（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StallCause {
    IrqOff,    // 中断关死：触发面失联
    Swallow,   // 票被吞：pending 与票仓不一致
    Starve,    // 线程饿死：消费者长忙不收
    Invert,    // 优先级倒挂：低活占道高活久候
}

impl StallCause {
    pub fn as_str(self) -> &'static str {
        match self {
            StallCause::IrqOff => "irq-off", StallCause::Swallow => "swallow",
            StallCause::Starve => "starve", StallCause::Invert => "invert",
        }
    }
}

/// 观测面三要素快照。
#[derive(Clone, Copy, Debug)]
pub struct Obs {
    pub pending: bool,   // pending 位
    pub votes: u32,      // 票仓
    pub busy_us: u64,    // 消费者连续忙时长
    pub waiter_prio: u8, // 候者优先级（小者高）
    pub runner_prio: u8, // 在跑者优先级
    pub irq_enabled: bool,
}

/// 因果账：观测快照 → 归因判词。
#[derive(Clone, Copy, Debug)]
pub struct CauseRec {
    pub obs: Obs,
    pub cause: Option<StallCause>,
}

/// 停滞诊断器：三面差分定位。
pub struct StallDoctor {
    pub recs: Vec<CauseRec>,
    pub false_hits: u32,
    pub miss_hits: u32,
}

impl StallDoctor {
    pub fn new() -> Self {
        StallDoctor { recs: Vec::new(), false_hits: 0, miss_hits: 0 }
    }

    /// 单次诊断：按四因优先级顺序排查，返回归因。
    /// 排查序：中断关死 → 票被吞 → 线程饿死 → 优先级倒挂。
    pub fn diagnose(&mut self, obs: Obs, expect: Option<StallCause>) -> Result<Option<StallCause>, u16> {
        let cause = if !obs.irq_enabled {
            Some(StallCause::IrqOff)
        } else if obs.pending != (obs.votes > 0) {
            Some(StallCause::Swallow)
        } else if obs.pending && obs.busy_us > 1000 {
            Some(StallCause::Starve)
        } else if obs.pending && obs.runner_prio > obs.waiter_prio {
            Some(StallCause::Invert)
        } else {
            None
        };
        // 误报/漏报双计。
        if cause != expect {
            if cause.is_none() && expect.is_some() { self.miss_hits += 1; return Err(E_MISS); }
            if cause.is_some() && expect.is_none() { self.false_hits += 1; return Err(E_FALSE); }
            return Err(E_STATE); // 归因错类
        }
        self.recs.push(CauseRec { obs, cause });
        Ok(cause)
    }

    /// 健康路径零误报核验。
    pub fn healthy_ok(&self) -> Result<(), u16> {
        if self.false_hits != 0 { return Err(E_FALSE); }
        Ok(())
    }

    /// 四因注入覆盖核验：每因至少 10 次命中。
    pub fn coverage(&self, per_cause: u32) -> Result<(), u16> {
        for c in [StallCause::IrqOff, StallCause::Swallow, StallCause::Starve, StallCause::Invert] {
            let n = self.recs.iter().filter(|r| r.cause == Some(c)).count() as u32;
            if n < per_cause { return Err(E_MISS); }
        }
        Ok(())
    }

    /// 因果摘要。
    pub fn summary(&self) -> String {
        let mut s = alloc::format!("stall recs={} false={} miss={}", self.recs.len(), self.false_hits, self.miss_hits);
        for c in [StallCause::IrqOff, StallCause::Swallow, StallCause::Starve, StallCause::Invert] {
            let n = self.recs.iter().filter(|r| r.cause == Some(c)).count();
            s = alloc::format!("{} {}:{}", s, c.as_str(), n);
        }
        s
    }
}

fn obs_healthy() -> Obs {
    Obs { pending: false, votes: 0, busy_us: 0, waiter_prio: 5, runner_prio: 5, irq_enabled: true }
}

/// 自例程：四因注入各 10/10 必抓，健康 20 样本零误报返回 0。
pub fn drill() -> Result<u32, u16> {
    let mut doc = StallDoctor::new();
    // 场景一：健康样本 20 条零误报。
    for i in 0..20u32 {
        let mut o = obs_healthy();
        if i % 2 == 0 { o.pending = true; o.votes = 1; o.busy_us = 10; }
        if doc.diagnose(o, None)? != None { return Err(E_FALSE); }
    }
    doc.healthy_ok()?;
    // 场景二：因一中断关死 10/10。
    for _ in 0..10 {
        let mut o = obs_healthy();
        o.irq_enabled = false; o.pending = true; o.votes = 1;
        if doc.diagnose(o, Some(StallCause::IrqOff))? != Some(StallCause::IrqOff) {
            return Err(E_MISS);
        }
    }
    // 场景三：因二票被吞 10/10（位有票无 / 票有位无）。
    for i in 0..10 {
        let mut o = obs_healthy();
        if i % 2 == 0 { o.pending = true; o.votes = 0; } else { o.pending = false; o.votes = 3; }
        if doc.diagnose(o, Some(StallCause::Swallow))? != Some(StallCause::Swallow) {
            return Err(E_MISS);
        }
    }
    // 场景四：因三线程饿死 10/10。
    for _ in 0..10 {
        let mut o = obs_healthy();
        o.pending = true; o.votes = 1; o.busy_us = 2000;
        if doc.diagnose(o, Some(StallCause::Starve))? != Some(StallCause::Starve) {
            return Err(E_MISS);
        }
    }
    // 场景五：因四优先级倒挂 10/10。
    for _ in 0..10 {
        let mut o = obs_healthy();
        o.pending = true; o.votes = 1; o.busy_us = 100;
        o.waiter_prio = 2; o.runner_prio = 9; // 在跑者优先级数值大 = 低活占道
        if doc.diagnose(o, Some(StallCause::Invert))? != Some(StallCause::Invert) {
            return Err(E_MISS);
        }
    }
    doc.coverage(10)?;
    if doc.false_hits != 0 || doc.miss_hits != 0 { return Err(E_FALSE); }
    if doc.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}
