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
const E_STATE: u16 = SEG_ERR + 7;   // 归因错类/底座状态非法

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

// ════════════════════ 判据聚合入口（AI-INTAKE-1107 收库补齐） ════════════════════
//
// 修正记录：本模块历史会话只交付了域模型与测试，从未交付 run_unx950_checks
// 聚合入口——等于从未参与判据聚合闭环。此入口把模块自身的公开 API 逐项
// 折叠成 CheckSet 判据（判据强度不降：每条判据都调真实 API、断真实性质）。

use crate::checks::CheckSet;

pub fn run_unx950_checks() -> CheckSet {
    let mut s = CheckSet::new("unx0950_stall");
    // J1 核心路径：四因注入各 10/10 必抓、健康 20 样本零误报。
    s.add("核心路径drill全过", drill() == Ok(0), "健康20+四因各10的因果账全过返回 0");
    // J2 健康路径零误报：20 样本全判 None。
    let mut doc = StallDoctor::new();
    let mut healthy_all = true;
    for i in 0..20u32 {
        let mut o = Obs { pending: false, votes: 0, busy_us: 0, waiter_prio: 5, runner_prio: 5, irq_enabled: true };
        if i % 2 == 0 { o.pending = true; o.votes = 1; o.busy_us = 10; }
        if doc.diagnose(o, None) != Ok(None) { healthy_all = false; }
    }
    let healthy_ok = healthy_all && doc.false_hits == 0 && doc.healthy_ok().is_ok();
    s.add("健康路径零误报", healthy_ok, "20 健康样本（含 pending 已清票）全判无因、false_hits==0");
    // J3 因一：中断关死 10/10。
    let mut c1 = 0u32;
    for _ in 0..10 {
        let mut o = Obs { pending: true, votes: 1, busy_us: 10, waiter_prio: 5, runner_prio: 5, irq_enabled: false };
        if doc.diagnose(o, Some(StallCause::IrqOff)) == Ok(Some(StallCause::IrqOff)) { c1 += 1; }
    }
    s.add("因一中断关死10/10", c1 == 10, "irq_enabled=false 全部归因 IrqOff");
    // J4 因二：票被吞 10/10（有位无票/有票无位混排）。
    let mut c2 = 0u32;
    for i in 0..10 {
        let mut o = Obs { pending: false, votes: 0, busy_us: 10, waiter_prio: 5, runner_prio: 5, irq_enabled: true };
        if i % 2 == 0 { o.pending = true; o.votes = 0; } else { o.pending = false; o.votes = 3; }
        if doc.diagnose(o, Some(StallCause::Swallow)) == Ok(Some(StallCause::Swallow)) { c2 += 1; }
    }
    s.add("因二票被吞10/10", c2 == 10, "pending 与票仓不一致 10 样本全归因 Swallow");
    // J5 因三：线程饿死 10/10。
    let mut c3 = 0u32;
    for _ in 0..10 {
        let o = Obs { pending: true, votes: 1, busy_us: 2000, waiter_prio: 5, runner_prio: 5, irq_enabled: true };
        if doc.diagnose(o, Some(StallCause::Starve)) == Ok(Some(StallCause::Starve)) { c3 += 1; }
    }
    s.add("因三线程饿死10/10", c3 == 10, "busy_us>1000 且 pending 全归因 Starve");
    // J6 因四：优先级倒挂 10/10。
    let mut c4 = 0u32;
    for _ in 0..10 {
        let o = Obs { pending: true, votes: 1, busy_us: 100, waiter_prio: 2, runner_prio: 9, irq_enabled: true };
        if doc.diagnose(o, Some(StallCause::Invert)) == Ok(Some(StallCause::Invert)) { c4 += 1; }
    }
    s.add("因四优先级倒挂10/10", c4 == 10, "runner_prio>waiter_prio 全归因 Invert");
    // J7 覆盖与误漏双零：四因各 ≥10 命中、误报漏报均为 0。
    let cover_ok = doc.coverage(10).is_ok() && doc.false_hits == 0 && doc.miss_hits == 0 && doc.recs.len() == 60;
    s.add("覆盖与误漏双零", cover_ok, "四因各 10 命中、60 条因果账、false/miss 双 0");
    // J8 归因优先序：多因并存时按排查序取首因（中断关死压过票被吞）。
    let mut doc2 = StallDoctor::new();
    let prio_ok = doc2.diagnose(
        Obs { pending: true, votes: 0, busy_us: 2000, waiter_prio: 2, runner_prio: 9, irq_enabled: false },
        Some(StallCause::IrqOff),
    ) == Ok(Some(StallCause::IrqOff));
    s.add("归因优先序", prio_ok, "irq-off 先于 swallow/starve/invert 排查");
    // J9 四因闭集命名。
    s.add("四因闭集命名",
        StallCause::IrqOff.as_str() == "irq-off" && StallCause::Swallow.as_str() == "swallow"
            && StallCause::Starve.as_str() == "starve" && StallCause::Invert.as_str() == "invert",
        "四因 as_str 与冻结口径逐字一致");
    // J10 诊断段独占。
    s.add("诊断段位独占", SEG_BASE == 0x7CC0, "0x7CC0 段本单独占不漂移");
    // 动态明细两列。
    let sum_len = doc.summary().len();
    s.add_dyn("因果摘要非空", sum_len > 0, &alloc::format!("summary {} 字节（recs/false/miss + 四因分账）", sum_len));
    let n1 = doc.recs.iter().filter(|r| r.cause == Some(StallCause::IrqOff)).count();
    let n2 = doc.recs.iter().filter(|r| r.cause == Some(StallCause::Swallow)).count();
    let n3 = doc.recs.iter().filter(|r| r.cause == Some(StallCause::Starve)).count();
    let n4 = doc.recs.iter().filter(|r| r.cause == Some(StallCause::Invert)).count();
    s.add_dyn("四因分账均衡", n1 == 10 && n2 == 10 && n3 == 10 && n4 == 10,
        &alloc::format!("四因各 {} 条（10/10/10/10）", n1));
    s
}
