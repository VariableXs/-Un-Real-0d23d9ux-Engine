//! unx0957_health — UNX-F0957 分发面健康摘要（对齐 F0899 口径注册）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0957 分发面健康摘要（对齐 F0899 口径注册）
//! 判据原文摘录（卡面验收行）：分发面健康摘要按 F0899 口径注册
//! （总账/失守/置信三栏），口径抽检 10/10 次一致，健康度降级路径
//! 全档可走且降级必落账。
//! 诊断段 0x7DA0（本单独占，XcCode 闭集见下）。
//! 设计纪律：健康度是账不是感觉——三栏口径（总账/失守/置信）唯
//! 一来源是分发面账面；健康分五级降级闭集；降级必须可追溯（每级
//! 一条降级因）；全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7DA0;

// ── 诊断段独占 · 健康摘要错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_REG: u16 = SEG_ERR + 1;   // 注册口径不符
const E_HEALTH: u16 = SEG_ERR + 2; // 健康级非法
const E_TRACE: u16 = SEG_ERR + 3;  // 降级无因
const E_STATE: u16 = SEG_ERR + 4;  // 摘要状态非法
const E_ALIGN: u16 = SEG_ERR + 5;  // 与 F0899 口径不齐

/// 健康五级（降级闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Health {
    Green = 0,  // 全绿
    Yellow = 1, // 轻微失守
    Orange = 2, // 明显失守
    Red = 3,    // 严重失守
    Dark = 4,   // 账面崩坏
}

impl Health {
    pub fn idx(self) -> usize { self as usize }
    pub fn as_str(self) -> &'static str {
        match self { Health::Green => "green", Health::Yellow => "yellow", Health::Orange => "orange", Health::Red => "red", Health::Dark => "dark" }
    }
}

/// 三栏口径（对齐 F0899）。
#[derive(Clone, Copy, Debug)]
pub struct SummaryCols {
    pub total: u32,   // 总账（断言/检查总数）
    pub failed: u32,  // 失守数
    pub conf: u32,    // 置信（0-100，账面自洽度）
}

impl SummaryCols {
    /// 口径自洽：失守 ≤ 总账，置信 0..=100。
    pub fn coherent(&self) -> bool {
        self.failed <= self.total && self.conf <= 100
    }
}

/// 健康摘要：三栏 + 健康级 + 降级链。
pub struct HealthSummary {
    pub cols: SummaryCols,
    pub level: Health,
    pub degrade_chain: Vec<(Health, String)>, // 每次降级：到哪级 + 降级因
    registered: bool,
}

impl HealthSummary {
    pub fn new(cols: SummaryCols) -> Self {
        let level = level_of(&cols);
        HealthSummary { cols, level, degrade_chain: Vec::new(), registered: false }
    }

    /// F0899 口径注册：三栏齐 + 自洽才许注册。
    pub fn register(&mut self) -> Result<(), u16> {
        if !self.cols.coherent() { return Err(E_REG); }
        self.registered = true;
        Ok(())
    }

    pub fn is_registered(&self) -> bool { self.registered }

    /// 健康级重算：失守率映射五级。
    pub fn relevel(&mut self, reason: &str) -> Result<Health, u16> {
        if reason.is_empty() { return Err(E_TRACE); }
        let new = level_of(&self.cols);
        if new > self.level {
            self.degrade_chain.push((new, reason.into()));
        }
        self.level = new;
        Ok(new)
    }

    /// 降级链完整性：链上每级必须严格递增且终态 == 当前级。
    pub fn chain_audit(&self) -> Result<u32, u16> {
        let mut prev = self.start_level_hint();
        for (h, r) in &self.degrade_chain {
            if *h <= prev { return Err(E_HEALTH); }
            if r.is_empty() { return Err(E_TRACE); }
            prev = *h;
        }
        if let Some(last) = self.degrade_chain.last() {
            if last.0 != self.level { return Err(E_STATE); }
        }
        Ok(self.degrade_chain.len() as u32)
    }

    fn start_level_hint(&self) -> Health { Health::Green }

    /// F0899 口径抽检：三栏渲染与注册一致。
    pub fn align_check(&self) -> Result<u32, u16> {
        if !self.registered { return Err(E_ALIGN); }
        if !self.cols.coherent() { return Err(E_REG); }
        // 口径一致性：置信 = 100 - 失守率（域内简化口径）。
        let rate = if self.total == 0 { 0 } else { self.failed * 100 / self.total };
        let expect = 100u32.saturating_sub(rate);
        if self.conf() != expect && self.total > 0 {
            // 允许 ±5 偏差（口径平滑）。
            let diff = if self.conf() > expect { self.conf() - expect } else { expect - self.conf() };
            if diff > 5 { return Err(E_ALIGN); }
        }
        Ok(3) // 三栏
    }

    fn conf(&self) -> u32 { self.cols.conf }

    /// 人话化摘要。
    pub fn render(&self) -> String {
        alloc::format!("health total={} failed={} conf={} level={}",
            self.cols.total, self.cols.failed, self.cols.conf, self.level.as_str())
    }
}

/// 失守率 → 健康级映射（闭集）。
pub fn level_of(c: &SummaryCols) -> Health {
    if c.total == 0 { return Health::Dark; }
    let rate = c.failed * 100 / c.total;
    if rate == 0 { Health::Green }
    else if rate <= 5 { Health::Yellow }
    else if rate <= 20 { Health::Orange }
    else if rate <= 50 { Health::Red }
    else { Health::Dark }
}

/// 自例程：注册、五级降级、链审计、口径抽检全过返回 0。
pub fn drill() -> Result<u32, u16> {
    // 场景一：全绿注册。
    let mut hs = HealthSummary::new(SummaryCols { total: 100, failed: 0, conf: 100 });
    hs.register()?;
    if hs.level != Health::Green { return Err(E_HEALTH); }
    if hs.align_check()? != 3 { return Err(E_ALIGN); }
    // 场景二：五级降级逐档走（失守率抬升）。
    let steps = [(Health::Yellow, 5u32), (Health::Orange, 15), (Health::Red, 30), (Health::Dark, 60)];
    for (i, (lv, fail)) in steps.iter().enumerate() {
        hs.cols.failed = *fail;
        let got = hs.relevel(alloc::format!("degrade-step-{}", i).as_str())?;
        if got != *lv { return Err(E_HEALTH); }
    }
    if hs.chain_audit()? != 4 { return Err(E_TRACE); }
    // 场景三：无因降级必拦。
    if hs.relevel("") != Err(E_TRACE) { return Err(E_TRACE); }
    // 场景四：口径不符注册必拦（失守 > 总账）。
    let mut bad = HealthSummary::new(SummaryCols { total: 10, failed: 11, conf: 0 });
    if bad.register() != Err(E_REG) { return Err(E_REG); }
    // 场景五：未注册抽检必拦。
    if bad.align_check() != Err(E_ALIGN) { return Err(E_ALIGN); }
    // 场景六：置信偏差 >5 必拦。
    let mut mis = HealthSummary::new(SummaryCols { total: 100, failed: 10, conf: 3 });
    mis.register()?;
    if mis.align_check() != Err(E_ALIGN) { return Err(E_ALIGN); }
    // 场景七：五级映射全档。
    let table = [
        (SummaryCols { total: 0, failed: 0, conf: 0 }, Health::Dark),
        (SummaryCols { total: 100, failed: 3, conf: 97 }, Health::Yellow),
        (SummaryCols { total: 100, failed: 20, conf: 80 }, Health::Orange),
        (SummaryCols { total: 100, failed: 50, conf: 50 }, Health::Red),
        (SummaryCols { total: 100, failed: 90, conf: 10 }, Health::Dark),
    ];
    for (c, lv) in table.iter() {
        if level_of(c) != *lv { return Err(E_HEALTH); }
    }
    if hs.render().is_empty() { return Err(E_STATE); }
    Ok(0)
}
