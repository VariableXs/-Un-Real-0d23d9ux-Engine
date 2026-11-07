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
        // 收库修正：总账/失守数在 SummaryCols 三栏（self.cols.total/failed），非摘要直挂字段。
        let rate = if self.cols.total == 0 { 0 } else { self.cols.failed * 100 / self.cols.total };
        let expect = 100u32.saturating_sub(rate);
        if self.conf() != expect && self.cols.total > 0 {
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

// ════════════════════ 判据聚合入口（AI-INTAKE-1107 收库补齐） ════════════════════
//
// 修正记录：本模块历史会话只交付了域模型与测试，从未交付 run_unx957_checks
// 聚合入口——等于从未参与判据聚合闭环。此入口把模块自身的公开 API 逐项
// 折叠成 CheckSet 判据（判据强度不降：每条判据都调真实 API、断真实性质）。

use crate::checks::CheckSet;

pub fn run_unx957_checks() -> CheckSet {
    let mut s = CheckSet::new("unx0957_health");
    // J1 核心路径：注册、五级降级、链审计、口径抽检。
    s.add("核心路径drill全过", drill() == Ok(0), "全绿注册/四级降级/无因拦/口径拦/五级映射全过返回 0");
    // J2 五级映射全档（失守率 → 健康级闭集）。
    let m0 = level_of(&SummaryCols { total: 0, failed: 0, conf: 0 }) == Health::Dark;
    let m1 = level_of(&SummaryCols { total: 100, failed: 3, conf: 97 }) == Health::Yellow;
    let m2 = level_of(&SummaryCols { total: 100, failed: 20, conf: 80 }) == Health::Orange;
    let m3 = level_of(&SummaryCols { total: 100, failed: 50, conf: 50 }) == Health::Red;
    let m4 = level_of(&SummaryCols { total: 100, failed: 90, conf: 10 }) == Health::Dark;
    s.add("五级映射全档", m0 && m1 && m2 && m3 && m4, "0/3/20/50/90% 失守率逐档落 Dark/Yellow/Orange/Red/Dark");
    // J3 口径自洽卫：失守 ≤ 总账、置信 0..=100。
    let coh_ok = SummaryCols { total: 100, failed: 0, conf: 100 }.coherent()
        && !SummaryCols { total: 10, failed: 11, conf: 0 }.coherent()
        && !SummaryCols { total: 10, failed: 0, conf: 101 }.coherent();
    s.add("三栏口径自洽卫", coh_ok, "coherent 正反例：失守超账/置信越界均不自洽");
    // J4 正例：全绿注册 + F0899 口径抽检。
    let mut hs = HealthSummary::new(SummaryCols { total: 100, failed: 0, conf: 100 });
    let reg_ok = hs.register().is_ok() && hs.is_registered()
        && hs.level == Health::Green && hs.align_check() == Ok(3);
    s.add("全绿注册抽检", reg_ok, "注册一次成功、level=Green、align_check 三栏齐");
    // J5 五级降级逐档走且降级必落账（链审计 4 级）。
    let steps = [(Health::Yellow, 5u32), (Health::Orange, 15), (Health::Red, 30), (Health::Dark, 60)];
    let mut deg_ok = true;
    for (i, (lv, fail)) in steps.iter().enumerate() {
        hs.cols.failed = *fail;
        if hs.relevel(alloc::format!("degrade-step-{}", i).as_str()) != Ok(*lv) { deg_ok = false; }
    }
    s.add("五级降级链落账", deg_ok && hs.chain_audit() == Ok(4) && hs.level == Health::Dark,
        "Yellow→Orange→Red→Dark 逐级、chain_audit 4 行、终态 Dark");
    // J6 反例一：无因降级必拦（E_TRACE）。
    s.add("无因降级拦", hs.relevel("") == Err(E_TRACE), "relevel 空因被 E_TRACE 拒——降级必须可追溯");
    // J7 反例二：口径不符注册必拦 + 未注册抽检必拦。
    let mut bad = HealthSummary::new(SummaryCols { total: 10, failed: 11, conf: 0 });
    s.add("口径不符拦", bad.register() == Err(E_REG) && bad.align_check() == Err(E_ALIGN)
        && !bad.is_registered(), "失守>总账注册 E_REG、未注册抽检 E_ALIGN");
    // J8 反例三：置信偏差 >5 必拦。
    let mut mis = HealthSummary::new(SummaryCols { total: 100, failed: 10, conf: 3 });
    s.add("置信偏差拦", mis.register().is_ok() && mis.align_check() == Err(E_ALIGN),
        "conf=3 对 expect=90 偏差超 ±5 被 E_ALIGN 拒");
    // J9 健康级闭集与序：Green<Yellow<Orange<Red<Dark。
    let ord_ok = Health::Green < Health::Yellow && Health::Yellow < Health::Orange
        && Health::Orange < Health::Red && Health::Red < Health::Dark
        && Health::Green.idx() == 0 && Health::Dark.idx() == 4
        && Health::Green.as_str() == "green" && Health::Dark.as_str() == "dark";
    s.add("五级闭集有序", ord_ok, "PartialOrd 五级递增、idx 0..4、命名冻结");
    // J10 空降级链审计合法（0 行即过）。
    let empty_ok = HealthSummary::new(SummaryCols { total: 1, failed: 0, conf: 100 }).chain_audit() == Ok(0);
    s.add("空降级链审计", empty_ok, "无降级时 chain_audit Ok(0)");
    // J11 诊断段独占。
    s.add("诊断段位独占", SEG_BASE == 0x7DA0, "0x7DA0 段本单独占不漂移");
    // 动态明细两列。
    let render_len = hs.render().len();
    s.add_dyn("健康摘要可渲染", render_len > 0, &alloc::format!("render {} 字节（total/failed/conf/level）", render_len));
    let chain_len = hs.degrade_chain.len();
    let last_reason = match hs.degrade_chain.iter().last() {
        Some((h, r)) => alloc::format!("终级 {} 因 {}", h.as_str(), r),
        None => String::from("<无降级>"),
    };
    s.add_dyn("降级链可追溯", chain_len == 4, &alloc::format!("链 {} 行；{}", chain_len, last_reason));
    s
}
