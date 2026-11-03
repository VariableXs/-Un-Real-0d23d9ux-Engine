//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1506 · 回归执行器（顺序执行与超时治理） · 0x721C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B36）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F1506-J1 回归执行器建面 100%（按 F1488 拓扑序执行+
//!     单断言超时治理：超时即记账续跑），执行器演练 10/10 次完成，卡死
//!     注入 10/10 必治（看门狗拦截），执行器开销占比 ≤2% 断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · F1488 计划图：12 步回归链（探测/装载/扫描/断言/归并/裁决/写回），
//!     依赖边冻结；Kahn 拓扑排序，最小 id 先行（序稳定可复算）；
//!   · 环依赖 / 出界依赖 / 重复依赖 / 重复步骤 一律显性拒（拓扑无解即拒）；
//!   · 看门狗：单断言预算 WDT=200 步，超额断言在预算+1 步被掐断，记超时
//!     账后继续跑后续步（超时即记账续跑，执行不许中断）；
//!   · 卡死注入：指定步成本抬到 HANG_COST，看门狗必须拦下且全链跑完；
//!   · 开销口径：执行器簿记（派发+记账）vs 断言求值步数，占比 ≤2%。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x721C0，VX3-1103 回归执行窗口独占）。
pub const SEG_BASE: u32 = 0x721C0;
/// 看门狗单断言预算（步，冻结 200）。
pub const WDT_BUDGET: u32 = 200;
/// 卡死注入成本（步，远超预算，冻结）。
pub const HANG_COST: u32 = 1_000_000;
/// F1488 计划步数（冻结 12）。
pub const PLAN_STEPS: usize = 12;
/// 执行器开销占比上限（≤2% = 200 bps，冻结）。
pub const OVERHEAD_CAP_BPS: u64 = 200;
/// 演练/注入轮数（判据原文 10/10，冻结）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunCode {
    /// 拓扑违约：依赖边被倒置执行（执行序不是合法拓扑序）
    TopoBreak,
    /// 看门狗拦截：断言超预算被掐断（记账后续跑）
    WatchdogHit,
    /// 执行中断：超时后未续跑（记账续跑被破坏）
    RunAbort,
    /// 重复步骤：同一节点/依赖入账两次
    DupStep,
    /// 开销超限：执行器簿记占比 >2%
    OverheadHigh,
    /// 环依赖：F1488 图存在环，拓扑无解
    CycDep,
    /// 双跑不一致：run_plan 两次应同账
    NotReproducible,
    /// 拓扑序不稳：两轮全跑序不同
    OrderUnstable,
}

impl RunCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RunCode::TopoBreak => "拓扑违约：执行序不满足依赖偏序，按 Kahn 序重排",
            RunCode::WatchdogHit => "看门狗拦截：断言超预算被掐断，已记超时账",
            RunCode::RunAbort => "执行中断：超时后必须记账续跑，不得整链中止",
            RunCode::DupStep => "重复步骤：节点/依赖只许入账一次，查计划去重",
            RunCode::OverheadHigh => "开销超限：簿记占比 >2%，压派发/记账常数",
            RunCode::CycDep => "环依赖：计划图有环拓扑无解，断环后重排",
            RunCode::NotReproducible => "双跑不一致：复现红线，run_plan 两次应同账",
            RunCode::OrderUnstable => "拓扑序不稳：最小 id 先行被破坏，查排序并列处理",
        }
    }
    pub const ALL: [RunCode; 8] = [
        RunCode::TopoBreak,
        RunCode::WatchdogHit,
        RunCode::RunAbort,
        RunCode::DupStep,
        RunCode::OverheadHigh,
        RunCode::CycDep,
        RunCode::NotReproducible,
        RunCode::OrderUnstable,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RunCode::ALL.len() {
        if RunCode::ALL[i].val() == code {
            return RunCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x721C0 段闭集内"
}

// ── F1488 计划图（冻结 12 步回归链） ────────────────────────────
/// 回归步骤：id 即计划位序，deps 为依赖步骤 id 表，cost 为断言求值步数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub id: u32,
    pub name: &'static str,
    pub deps: Vec<u32>,
    pub cost: u32,
}

/// 建步助手。
fn step(id: u32, name: &'static str, deps: &[u32], cost: u32) -> Step {
    let mut d: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < deps.len() {
        if let Some(x) = deps.get(i) {
            d.push(*x);
        }
        i += 1;
    }
    Step { id, name, deps: d, cost }
}

/// F1488 冻结计划：探测→装载/锁环境→扫描→四路断言→两路归并→裁决→写回。
pub fn f1488_plan() -> Vec<Step> {
    let mut v: Vec<Step> = Vec::new();
    v.push(step(0, "boot_probe", &[], 150));
    v.push(step(1, "cfg_load", &[0], 120));
    v.push(step(2, "env_lock", &[0], 140));
    v.push(step(3, "case_scan", &[1, 2], 180));
    v.push(step(4, "assert_core", &[3], 130));
    v.push(step(5, "assert_mem", &[3], 160));
    v.push(step(6, "assert_sched", &[4], 170));
    v.push(step(7, "assert_io", &[5], 120));
    v.push(step(8, "rollup_core", &[6, 7], 190));
    v.push(step(9, "rollup_full", &[6, 8], 150));
    v.push(step(10, "verdict", &[9], 140));
    v.push(step(11, "writeback", &[10], 100));
    v
}

/// 环依赖计划（探针用）：writeback 反指 cfg_load 成环。
pub fn f1488_plan_cycled() -> Vec<Step> {
    let mut v: Vec<Step> = Vec::new();
    v.push(step(0, "boot_probe", &[11], 150));
    v.push(step(1, "cfg_load", &[0], 120));
    v.push(step(2, "env_lock", &[0], 140));
    v.push(step(3, "case_scan", &[1, 2], 180));
    v.push(step(4, "assert_core", &[3], 130));
    v.push(step(5, "assert_mem", &[3], 160));
    v.push(step(6, "assert_sched", &[4], 170));
    v.push(step(7, "assert_io", &[5], 120));
    v.push(step(8, "rollup_core", &[6, 7], 190));
    v.push(step(9, "rollup_full", &[6, 8], 150));
    v.push(step(10, "verdict", &[9], 140));
    v.push(step(11, "writeback", &[10], 100));
    v
}

/// 重复依赖计划（探针用）：case_scan 依赖表里 cfg_load 出现两次。
pub fn f1488_plan_dupdep() -> Vec<Step> {
    let mut v: Vec<Step> = Vec::new();
    v.push(step(0, "boot_probe", &[], 150));
    v.push(step(1, "cfg_load", &[0], 120));
    v.push(step(2, "env_lock", &[0], 140));
    v.push(step(3, "case_scan", &[1, 1, 2], 180));
    v.push(step(4, "assert_core", &[3], 130));
    v.push(step(5, "assert_mem", &[3], 160));
    v.push(step(6, "assert_sched", &[4], 170));
    v.push(step(7, "assert_io", &[5], 120));
    v.push(step(8, "rollup_core", &[6, 7], 190));
    v.push(step(9, "rollup_full", &[6, 8], 150));
    v.push(step(10, "verdict", &[9], 140));
    v.push(step(11, "writeback", &[10], 100));
    v
}

// ── 计划校验 ────────────────────────────────────────────────────
/// 计划校验：id 连续、依赖出界、依赖重复 → 拓扑前置闸。
pub fn validate_plan(plan: &[Step]) -> Result<(), u32> {
    if plan.len() != PLAN_STEPS {
        return Err(RunCode::CycDep.val());
    }
    let mut i = 0usize;
    while i < plan.len() {
        let s = match plan.get(i) {
            Some(s) => s,
            None => return Err(RunCode::CycDep.val()),
        };
        if s.id != i as u32 {
            return Err(RunCode::DupStep.val());
        }
        let mut j = 0usize;
        while j < s.deps.len() {
            let d = match s.deps.get(j) {
                Some(d) => *d,
                None => return Err(RunCode::CycDep.val()),
            };
            if d as usize >= plan.len() {
                return Err(RunCode::CycDep.val());
            }
            // 依赖重复闸。
            let mut k = 0usize;
            while k < j {
                if let Some(e) = s.deps.get(k) {
                    if *e == d {
                        return Err(RunCode::DupStep.val());
                    }
                }
                k += 1;
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

// ── Kahn 拓扑排序（最小 id 先行，序冻结） ───────────────────────
/// 拓扑排序：入度表 + 逐轮取最小 id 零入度节点；有环即 CycDep。
pub fn topo_sort(plan: &[Step]) -> Result<Vec<u32>, u32> {
    validate_plan(plan)?;
    let n = plan.len();
    // 入度表。
    let mut indeg: Vec<u32> = Vec::new();
    let mut z = 0usize;
    while z < n {
        indeg.push(0);
        z += 1;
    }
    let mut i = 0usize;
    while i < n {
        // 入度 = 本步依赖数（依赖先行，依赖清零即可发）。
        if let Some(s) = plan.get(i) {
            if let Some(c) = indeg.get_mut(i) {
                *c = s.deps.len() as u32;
            }
        }
        i += 1;
    }
    // 逐轮取最小 id 零入度节点（稳定序：并列取 id 最小）。
    let mut order: Vec<u32> = Vec::new();
    let mut done: Vec<bool> = Vec::new();
    let mut z = 0usize;
    while z < n {
        done.push(false);
        z += 1;
    }
    let mut emitted = 0usize;
    while emitted < n {
        let mut pick: Option<usize> = None;
        let mut i = 0usize;
        while i < n {
            let zero = match indeg.get(i) {
                Some(&c) => c == 0,
                None => false,
            };
            let not_done = match done.get(i) {
                Some(&d) => !d,
                None => false,
            };
            if zero && not_done {
                pick = Some(i);
                break;
            }
            i += 1;
        }
        let p = match pick {
            Some(p) => p,
            None => return Err(RunCode::CycDep.val()),
        };
        // 摘点：所有依赖 p 的节点入度减一。
        let mut k = 0usize;
        while k < n {
            if let Some(s) = plan.get(k) {
                let mut j = 0usize;
                while j < s.deps.len() {
                    if let Some(d) = s.deps.get(j) {
                        if *d == p as u32 {
                            if let Some(c) = indeg.get_mut(k) {
                                *c = c.saturating_sub(1);
                            }
                        }
                    }
                    j += 1;
                }
            }
            k += 1;
        }
        if let Some(d) = done.get_mut(p) {
            *d = true;
        }
        order.push(p as u32);
        emitted += 1;
    }
    Ok(order)
}

/// 拓扑序合法性复核：每个依赖必须先于本步出现。
pub fn topo_valid(plan: &[Step], order: &[u32]) -> bool {
    if order.len() != plan.len() {
        return false;
    }
    let mut i = 0usize;
    while i < order.len() {
        let pos = match order.get(i) {
            Some(p) => *p as usize,
            None => return false,
        };
        let s = match plan.get(pos) {
            Some(s) => s,
            None => return false,
        };
        let mut j = 0usize;
        while j < s.deps.len() {
            let d = match s.deps.get(j) {
                Some(d) => *d as usize,
                None => return false,
            };
            // d 必须在 order[0..i) 中出现过。
            let mut found = false;
            let mut k = 0usize;
            while k < i {
                if let Some(q) = order.get(k) {
                    if *q as usize == d {
                        found = true;
                    }
                }
                k += 1;
            }
            if !found {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 执行账 ──────────────────────────────────────────────────────
/// 执行报告：拓扑序 + 通过/超时账 + 簿记/载荷开销账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunReport {
    /// 实际执行序（拓扑序）。
    pub order: Vec<u32>,
    /// 预算内通过的断言数。
    pub passed: u32,
    /// 超时账 (步 id, 原始成本)。
    pub timeouts: Vec<(u32, u32)>,
    /// 执行器簿记（派发 + 记账）。
    pub admin_ops: u64,
    /// 断言求值步数（被掐断的计预算+1）。
    pub payload_ops: u64,
    /// 走完全链的步数（含超时步，记账续跑）。
    pub completed: u32,
    /// 是否发生过超时续跑。
    pub continued: bool,
}

impl RunReport {
    /// 开销占比（bps）：簿记 / 载荷；无载荷视为超限。
    pub fn overhead_bps(&self) -> u64 {
        if self.payload_ops == 0 {
            return u64::MAX;
        }
        self.admin_ops.saturating_mul(10_000) / self.payload_ops
    }
}

// ── 回归执行器 ──────────────────────────────────────────────────
/// 执行计划：按 F1488 拓扑序逐步执行；hang_at 指定步成本抬成 HANG_COST。
/// 超预算断言被看门狗掐断：记超时账后续跑，执行链不中断。
pub fn run_plan(plan: &[Step], hang_at: Option<usize>) -> Result<RunReport, u32> {
    let order = topo_sort(plan)?;
    let mut rep = RunReport {
        order,
        passed: 0,
        timeouts: Vec::new(),
        admin_ops: 0,
        payload_ops: 0,
        completed: 0,
        continued: false,
    };
    let mut i = 0usize;
    while i < rep.order.len() {
        let pos = match rep.order.get(i) {
            Some(p) => *p as usize,
            None => return Err(RunCode::TopoBreak.val()),
        };
        let s = match plan.get(pos) {
            Some(s) => s,
            None => return Err(RunCode::TopoBreak.val()),
        };
        rep.admin_ops += 2; // 派发 + 记账
        let cost = if hang_at == Some(pos) { HANG_COST } else { s.cost };
        if cost > WDT_BUDGET {
            // 看门狗拦截：预算+1 步掐断，记超时账后续跑。
            rep.timeouts.push((s.id, cost));
            rep.payload_ops += WDT_BUDGET as u64 + 1;
            rep.continued = true;
        } else {
            rep.payload_ops += cost as u64;
            rep.passed += 1;
        }
        rep.completed += 1;
        i += 1;
    }
    Ok(rep)
}

// ── 演练（判据原文：执行器演练 10/10 次完成） ───────────────────
pub struct DrillLedger {
    pub tries: u32,
    pub done: u32,
    /// 未完成的轮号（诊断用）。
    pub miss_rounds: Vec<u32>,
}

impl DrillLedger {
    pub fn new() -> DrillLedger {
        DrillLedger { tries: 0, done: 0, miss_rounds: Vec::new() }
    }
    pub fn record(&mut self, round: u32, hit: bool) {
        self.tries += 1;
        if hit {
            self.done += 1;
        } else {
            self.miss_rounds.push(round);
        }
    }
    pub fn all_done(&self) -> bool {
        self.tries > 0 && self.done == self.tries
    }
}

/// 演练轮：干净计划全链跑通，超时账必须为空，序必须与首轮全等。
pub fn drill_rounds(rounds: u32) -> Result<DrillLedger, u32> {
    let plan = f1488_plan();
    let base = run_plan(&plan, None)?;
    let mut led = DrillLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let rep = run_plan(&plan, None)?;
        let ok = rep.completed == PLAN_STEPS as u32
            && rep.timeouts.is_empty()
            && rep.passed == PLAN_STEPS as u32
            && rep.order == base.order
            && topo_valid(&plan, &rep.order);
        led.record(r, ok);
        r += 1;
    }
    Ok(led)
}

// ── 卡死注入（判据原文：卡死注入 10/10 必治） ───────────────────
pub struct HangLedger {
    pub tries: u32,
    pub cured: u32,
    /// 未治的轮号（诊断用）。
    pub miss_rounds: Vec<u32>,
}

impl HangLedger {
    pub fn new() -> HangLedger {
        HangLedger { tries: 0, cured: 0, miss_rounds: Vec::new() }
    }
    pub fn record(&mut self, round: u32, hit: bool) {
        self.tries += 1;
        if hit {
            self.cured += 1;
        } else {
            self.miss_rounds.push(round);
        }
    }
    pub fn all_cured(&self) -> bool {
        self.tries > 0 && self.cured == self.tries
    }
}

/// 卡死注入轮：每轮把一个步抬成卡死成本，看门狗必须拦下且全链跑完
///（必治 = 恰 1 笔超时账 + 走完全链 + 续跑不乱序）。
pub fn hang_rounds(rounds: u32) -> Result<HangLedger, u32> {
    let plan = f1488_plan();
    let base = run_plan(&plan, None)?;
    let mut led = HangLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let hang_at = (r as usize * 5 + 3) % PLAN_STEPS;
        let rep = run_plan(&plan, Some(hang_at))?;
        let expect_id = match plan.get(hang_at) {
            Some(s) => s.id,
            None => return Err(RunCode::TopoBreak.val()),
        };
        let hit1 = match rep.timeouts.first() {
            Some(t) => t.0 == expect_id && t.1 == HANG_COST,
            None => false,
        };
        let ok = rep.completed == PLAN_STEPS as u32
            && rep.timeouts.len() == 1
            && hit1
            && rep.order == base.order
            && rep.continued
            && rep.passed == (PLAN_STEPS - 1) as u32;
        led.record(r, ok);
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：拓扑合法 → 干净全跑 → 卡死必治 → 四拒探 → 开销闸。
/// 返回 (正旗数, 拒旗数, 载荷账, 拓扑序长)。
pub fn golden_run() -> Result<(u32, u32, u64, usize), u32> {
    let plan = f1488_plan();
    // 拓扑合法 + 序稳定（3 轮全等）。
    let o1 = topo_sort(&plan)?;
    let o2 = topo_sort(&plan)?;
    let o3 = topo_sort(&plan)?;
    if o1 != o2 || o2 != o3 || !topo_valid(&plan, &o1) {
        return Err(RunCode::OrderUnstable.val());
    }
    // 干净全跑。
    let clean = run_plan(&plan, None)?;
    let clean_ok = clean.completed == PLAN_STEPS as u32
        && clean.timeouts.is_empty()
        && clean.passed == PLAN_STEPS as u32;
    // 卡死必治（中段卡死：assert_mem）。
    let hang = run_plan(&plan, Some(5))?;
    let hang_ok = hang.completed == PLAN_STEPS as u32
        && hang.timeouts.len() == 1
        && hang.continued
        && hang.order == clean.order;
    // 开销闸。
    if clean.overhead_bps() > OVERHEAD_CAP_BPS {
        return Err(RunCode::OverheadHigh.val());
    }
    // ── 四拒探 ──
    // 环依赖拒。
    let cyc_rej = topo_sort(&f1488_plan_cycled()).is_err();
    // 重复依赖拒。
    let dup_rej = topo_sort(&f1488_plan_dupdep()).is_err();
    // 看门狗拦截确认：干净跑无拦截，卡死跑必有。
    let wdt_rej = !clean.continued && hang.timeouts.first().map(|t| t.1 == HANG_COST).unwrap_or(false);
    // 环计划若侥幸排出序必非法（双保险）。
    let topo_rej = match topo_sort(&f1488_plan_cycled()) {
        Ok(bad) => !topo_valid(&f1488_plan_cycled(), &bad),
        Err(_) => true,
    };
    let flags = clean_ok as u32 + hang_ok as u32 + (o1.len() == PLAN_STEPS) as u32 + topo_valid(&plan, &o1) as u32;
    let rejects = cyc_rej as u32 + dup_rej as u32 + wdt_rej as u32 + topo_rej as u32;
    Ok((flags, rejects, clean.payload_ops, clean.order.len()))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, _)) => Ok(()),
        _ => Err(RunCode::NotReproducible.val()),
    }
}

/// 序 B：演练 10/10 + 卡死注入 10/10 必治。
pub fn probe_seq_b() -> Result<(), u32> {
    let d = drill_rounds(JUDGE_ROUNDS)?;
    let h = hang_rounds(JUDGE_ROUNDS)?;
    if d.all_done() && h.all_cured() {
        Ok(())
    } else {
        Err(RunCode::RunAbort.val())
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x721C0 && RunCode::OrderUnstable.val() < SEG_BASE + 0x40;
    let ok = WDT_BUDGET == 200 && HANG_COST == 1_000_000 && PLAN_STEPS == 12
        && OVERHEAD_CAP_BPS == 200 && JUDGE_ROUNDS == 10 && in_window;
    if ok {
        Ok(())
    } else {
        Err(RunCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let plan = f1488_plan();
    let a = run_plan(&plan, None)?;
    let b = run_plan(&plan, None)?;
    if a != b {
        return Err(RunCode::NotReproducible.val());
    }
    let ha = run_plan(&plan, Some(4))?;
    let hb = run_plan(&plan, Some(4))?;
    if ha != hb {
        return Err(RunCode::NotReproducible.val());
    }
    let ga = golden_run()?;
    let gb = golden_run()?;
    if ga != gb {
        return Err(RunCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RunCode::ALL.len() {
        let v = RunCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let d = drill_rounds(JUDGE_ROUNDS)?;
    let h = hang_rounds(JUDGE_ROUNDS)?;
    if !d.all_done() || !h.all_cured() {
        return Err(RunCode::RunAbort.val());
    }
    let plan = f1488_plan();
    let rep = run_plan(&plan, None)?;
    Ok(format!(
        "状态=已完成 drills={}/{} cured={}/{} passed={} overhead={}bps",
        d.done, d.tries, h.cured, h.tries, rep.passed, rep.overhead_bps()
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, p, n)) => (*f == 4 && *r == 4, format!("flags={} rejects={} payload={} order_len={}", f, r, p, n)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-拓扑序执行", gp, gd));
    // J1 演练 10/10 次完成。
    let d = drill_rounds(JUDGE_ROUNDS);
    let (dp, dd) = match &d {
        Ok(l) => (l.all_done(), format!("t={} done={} miss={:?}", l.tries, l.done, l.miss_rounds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-演练10/10", dp, dd));
    // J1 卡死注入 10/10 必治（看门狗拦截）。
    let h = hang_rounds(JUDGE_ROUNDS);
    let (hp, hd) = match &h {
        Ok(l) => (l.all_cured(), format!("t={} cured={} miss={:?}", l.tries, l.cured, l.miss_rounds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-卡死注入10/10", hp, hd));
    // J1 超时记账续跑（中断面双探：续跑到底 + 后续步全过）。
    let plan = f1488_plan();
    let rep = run_plan(&plan, Some(5));
    let (cp, cd) = match &rep {
        Ok(r) => {
            // 卡死步之后必须还有步被执行（续跑证据）。
            let mut after = 0u32;
            let mut seen = false;
            let mut i = 0usize;
            while i < r.order.len() {
                if let Some(p) = r.order.get(i) {
                    if *p == 5 {
                        seen = true;
                    } else if seen {
                        after += 1;
                    }
                }
                i += 1;
            }
            (r.completed == PLAN_STEPS as u32 && r.continued && after > 0,
             format!("completed={} continued={} after_hang={}", r.completed, r.continued, after))
        }
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-超时记账续跑", cp, cd));
    // J1 执行器开销 ≤2%。
    let clean = run_plan(&plan, None);
    let (op, od) = match &clean {
        Ok(r) => (r.overhead_bps() <= OVERHEAD_CAP_BPS,
                  format!("admin={} payload={} bps={} cap={}", r.admin_ops, r.payload_ops, r.overhead_bps(), OVERHEAD_CAP_BPS)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-开销占比≤2%", op, od));
    // J1 拓扑序稳定（3 轮全等 + 逐边复核）。
    let o1 = topo_sort(&plan);
    let (tp, td) = match &o1 {
        Ok(o1) => {
            let o2 = topo_sort(&plan);
            let o3 = topo_sort(&plan);
            let same = match (&o2, &o3) {
                (Ok(b), Ok(c)) => o1 == b && b == c,
                _ => false,
            };
            (same && topo_valid(&plan, o1), format!("order={:?} valid={}", o1, topo_valid(&plan, o1)))
        }
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-拓扑序稳定", tp, td));
    // J1 环依赖 / 重复依赖 拒。
    let cyc = topo_sort(&f1488_plan_cycled()).is_err();
    let dup = topo_sort(&f1488_plan_dupdep()).is_err();
    v.push(("J1-环依赖重复依赖拒", cyc && dup, format!("环={} 重复依赖={}", cyc, dup)));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("干净/卡死/全链 三对两跑同账")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x721C0 窗口")));
    v
}

pub fn run_unx1506_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1506 回归执行器（顺序执行与超时治理）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
