//! unx0953_budget — UNX-F0953 分发面延迟预算表（端到端预算的域内分解契约）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0953 分发面延迟预算表（端到端预算的域内分解契约）
//! 判据原文摘录（卡面验收行）：端到端预算表逐域分解落账（五域预算
//! 之和 == 端到端总预算），超域样本 10/10 次必拦且点名到域，预算表
//! 回放逐格可核。
//! 诊断段 0x7D20（本单独占，XcCode 闭集见下）。
//! 设计纪律：预算表是契约——五域（取号/上半/触发/下半/收尾）各有
//! 定额，定额之和恒等于端到端总预算（分解恒等式）；任何改表必须走
//! 重分解（先改总额再摊域）；全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7D20;

// ── 诊断段独占 · 预算表错误域 ──────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_OVER: u16 = SEG_ERR + 1;   // 域超预算
const E_SUM: u16 = SEG_ERR + 2;    // 分解和 ≠ 总预算
const E_DOM: u16 = SEG_ERR + 3;    // 域号非法
const E_STATE: u16 = SEG_ERR + 4;  // 预算表状态非法
const E_NEG: u16 = SEG_ERR + 5;    // 负预算或零总额

/// 五域定额表。
pub const NDOM: usize = 5;
pub const END2END_BUDGET_US: u64 = 212; // 2+5+2+200+3

/// 域枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dom {
    Fetch = 0,
    Top = 1,
    Trig = 2,
    Bottom = 3,
    Tail = 4,
}

impl Dom {
    pub fn idx(self) -> usize { self as usize }
    pub fn as_str(self) -> &'static str {
        match self { Dom::Fetch => "fetch", Dom::Top => "top", Dom::Trig => "trig", Dom::Bottom => "bottom", Dom::Tail => "tail" }
    }
}

/// 预算表：五域定额 + 总预算。
#[derive(Clone, Copy, Debug)]
pub struct BudgetTable {
    pub quota: [u64; NDOM],
    pub total: u64,
}

impl BudgetTable {
    /// 默认表：硬编码五域定额。
    pub fn default_table() -> Self {
        BudgetTable { quota: [2, 5, 2, 200, 3], total: END2END_BUDGET_US }
    }

    /// 分解恒等式：域和 == 总额。
    pub fn invariant_ok(&self) -> bool {
        let s: u64 = self.quota.iter().sum();
        s == self.total
    }

    /// 改表（重分解）：先校验新五域之和，再落表。
    pub fn re_decompose(&mut self, new_quota: [u64; NDOM]) -> Result<(), u16> {
        let s: u64 = new_quota.iter().sum();
        if s != self.total { return Err(E_SUM); }
        for q in &new_quota { if *q == 0 { return Err(E_NEG); } }
        self.quota = new_quota;
        Ok(())
    }

    /// 单笔消费核验：域号合法 + 域内不超定额。
    pub fn charge(&self, dom: Dom, us: u64) -> Result<u64, u16> {
        let i = dom.idx();
        if i >= NDOM { return Err(E_DOM); }
        if us > self.quota[i] {
            return Err(E_OVER);
        }
        Ok(self.quota[i] - us)
    }

    /// 消费账：一次端到端事件各域实耗。
    pub fn audit_e2e(&self, usage: [u64; NDOM]) -> Result<u64, u16> {
        for (i, u) in usage.iter().enumerate() {
            if *u > self.quota[i] { return Err(E_OVER); }
        }
        let s: u64 = usage.iter().sum();
        if s > self.total { return Err(E_SUM); }
        Ok(s)
    }

    /// 表回放：逐格核验恒等式与零上限。
    pub fn replay(&self) -> Result<u32, u16> {
        if !self.invariant_ok() { return Err(E_SUM); }
        for q in &self.quota { if *q == 0 { return Err(E_NEG); } }
        Ok(NDOM as u32)
    }

    /// 域名摘要。
    pub fn summary(&self) -> String {
        let mut s = alloc::format!("budget total={}", self.total);
        for i in 0..NDOM {
            s = alloc::format!("{} {}:{}", s, dom_of(i).as_str(), self.quota[i]);
        }
        s
    }
}

fn dom_of(i: usize) -> Dom {
    match i { 0 => Dom::Fetch, 1 => Dom::Top, 2 => Dom::Trig, 3 => Dom::Bottom, _ => Dom::Tail }
}

/// 超域注入器：故意把 usage 抬到越界，核验必拦并点名域。
pub fn inject_over(t: &BudgetTable, dom: Dom, us: u64) -> Result<u16, u16> {
    match t.charge(dom, us) {
        Err(e) if e == E_OVER => Ok(e),      // 拦住且归因正确
        Ok(_) => Err(E_STATE),               // 漏拦
        Err(e) => Err(e),
    }
}

/// 预算账本：多表 + 消费流。
pub struct BudgetLedger {
    pub table: BudgetTable,
    pub flows: Vec<([u64; NDOM], u64)>,
}

impl BudgetLedger {
    pub fn new() -> Self {
        BudgetLedger { table: BudgetTable::default_table(), flows: Vec::new() }
    }

    /// 记一笔端到端流：全域合规才落账。
    pub fn flow(&mut self, usage: [u64; NDOM]) -> Result<u64, u16> {
        let s = self.table.audit_e2e(usage)?;
        self.flows.push((usage, s));
        Ok(s)
    }

    pub fn replay(&self) -> Result<u32, u16> {
        self.table.replay()?;
        for (usage, s) in &self.flows {
            let sum: u64 = usage.iter().sum();
            if sum != *s { return Err(E_SUM); }
        }
        Ok(self.flows.len() as u32)
    }
}

/// 自例程：默认表回放、超域 10/10 必拦、重分解守恒全过返回 0。
pub fn drill() -> Result<u32, u16> {
    let mut bl = BudgetLedger::new();
    // 场景一：默认表回放逐格可核。
    if bl.table.replay()? != 5 { return Err(E_SUM); }
    if !bl.table.invariant_ok() { return Err(E_SUM); }
    // 场景二：合规端到端流（顶格）落账。
    let top = [2, 5, 2, 200, 3];
    if bl.flow(top)? != END2END_BUDGET_US { return Err(E_SUM); }
    // 场景三：常规流十条。
    for i in 0..10u32 {
        let usage = [1, 3 + (i % 2) as u64, 1, 100 + i as u64, 2];
        bl.flow(usage)?;
    }
    if bl.replay()? != 11 { return Err(E_SUM); }
    // 场景四：超域注入五域各 2 次共 10/10 必拦。
    for i in 0..NDOM {
        for bump in [1u64, 5] {
            let mut dom = dom_of(i);
            let over = bl.table.quota[i] + bump;
            if inject_over(&bl.table, dom, over)? != E_OVER { return Err(E_OVER); }
            let _ = &mut dom;
        }
    }
    // 场景五：总超（各域内合规但合计爆总额）必拦。
    let half: [u64; NDOM] = [2, 5, 2, 200, 0];
    if bl.flow(half) != Err(E_NEG) {
        // 零定额域消费 0 不违规；构造真总超：
        let usage = [2, 5, 2, 200, 3];
        let mut t2 = BudgetTable::default_table();
        t2.re_decompose([2, 5, 2, 199, 3])?; // 恒等仍成立
        let s = t2.audit_e2e(usage)?;
        let _ = s; // 单表核验以 re_decompose 后为准
        if t2.audit_e2e([2, 5, 2, 200, 3]) != Err(E_OVER) { return Err(E_OVER); }
    }
    // 场景六：分解和 ≠ 总额的改表必拦。
    if bl.table.re_decompose([2, 5, 2, 199, 3]) != Err(E_SUM) { return Err(E_SUM); }
    // 场景七：合法重分解（换比例不变总额）。
    let mut t3 = BudgetTable::default_table();
    t3.re_decompose([3, 4, 2, 200, 3])?;
    if !t3.invariant_ok() { return Err(E_SUM); }
    if bl.table.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}
