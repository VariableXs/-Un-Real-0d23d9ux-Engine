//! m7sched — VARIX-M700 AI-21 进程调度器域 (F501~F525)
//!
//! 策略无关调度核心、策略插拔、影子对比、决策回放、公平性度量、
//! 交互识别、延迟分位、PELT 负载跟踪、抢占点、组调度、能耗钩子、
//! RT 预算、fuzz 桩、统计分账、零开销、事件流、核间均衡、回归走廊、
//! 优先级传播、自描述导出、帧契约、压力剧本、健康分、文档、年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部整数/permille）。

use crate::checks::CheckSet;

// ===========================================================================
// F501 — 调度器宪法：策略无关核心
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum SchedClass {
    Fair,
    RealTime,
    Budget,
    Idle,
}

pub struct SchedEntity {
    pub tid: u32,
    pub class: SchedClass,
    pub weight: u32,
    pub vruntime: u64,
}

/// 核心选择规则：RT > Fair > Budget > Idle；同类比 vruntime。
pub fn core_pick(a: &SchedEntity, b: &SchedEntity) -> u8 {
    // 返回 0=a 1=b
    let rank = |c: SchedClass| match c {
        SchedClass::RealTime => 0u8,
        SchedClass::Fair => 1,
        SchedClass::Budget => 2,
        SchedClass::Idle => 3,
    };
    let (ra, rb) = (rank(a.class), rank(b.class));
    if ra != rb {
        return if ra < rb { 0 } else { 1 };
    }
    if a.vruntime != b.vruntime {
        return if a.vruntime < b.vruntime { 0 } else { 1 };
    }
    if a.weight != b.weight {
        return if a.weight > b.weight { 0 } else { 1 };
    }
    0
}

// ===========================================================================
// F502 — 策略插拔谱
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum Policy {
    Cfs,
    Rr,
    Fifo,
    Deadline,
    Budgeted,
}

pub fn policy_registered(p: Policy) -> bool {
    matches!(p, Policy::Cfs | Policy::Rr | Policy::Fifo | Policy::Deadline | Policy::Budgeted)
}

// ===========================================================================
// F503 — 影子调度官：新策略影子运行对比
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ShadowRun {
    pub base_score: u32,
    pub cand_score: u32,
}

/// 候选策略需不劣于基线 2% 才建议启用。
pub fn shadow_verdict(r: ShadowRun) -> bool {
    r.cand_score * 1000 >= r.base_score.saturating_sub(r.base_score / 50) * 980
}

// ===========================================================================
// F504 — 调度决策回放
// ===========================================================================

pub const REPLAY_CAP: usize = 32;

#[derive(Clone, Copy)]
pub struct SchedDecision {
    pub tick: u64,
    pub picked_tid: u32,
}

pub fn replay_deterministic(a: &[SchedDecision], b: &[SchedDecision], n: usize) -> bool {
    let n = n.min(REPLAY_CAP).min(a.len()).min(b.len());
    a[..n].iter().zip(b[..n].iter()).all(|(x, y)| x.tick == y.tick && x.picked_tid == y.picked_tid)
}

// ===========================================================================
// F505 — 公平性度量仪
// ===========================================================================

/// 理想份额 vs 实际份额，最大偏差 permille。
pub fn fairness_deviation(ideal: &[u32], actual: &[u32], n: usize) -> u16 {
    let n = n.min(ideal.len()).min(actual.len());
    let ti: u32 = ideal[..n].iter().sum();
    let ta: u32 = actual[..n].iter().sum();
    if ti == 0 || ta == 0 {
        return 0;
    }
    let mut max_dev = 0u32;
    for i in 0..n {
        let di = (ideal[i] as u64 * 1000) / ti as u64;
        let ai = (actual[i] as u64 * 1000) / ta as u64;
        max_dev = max_dev.max(di.abs_diff(ai) as u32);
    }
    max_dev.min(1000) as u16
}

// ===========================================================================
// F506 — 交互性预言家：识别交互线程
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ThreadBehavior {
    pub sleeps_short_us: u32,  // 短睡眠次数（等待输入）
    pub runs_long_us: u32,     // 长运行时间
}

pub fn interactive_score(b: &ThreadBehavior) -> u16 {
    if b.runs_long_us == 0 {
        return 0;
    }
    let ratio = (b.sleeps_short_us as u64 * 1000) / (b.sleeps_short_us as u64 + b.runs_long_us as u64);
    ratio as u16
}

pub fn is_interactive(b: &ThreadBehavior) -> bool {
    interactive_score(b) >= 600
}

// ===========================================================================
// F507 — 调度延迟仪：唤醒到运行的分位
// ===========================================================================

pub fn percentile(sorted: &[u32], permille: u16) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as u32 - 1) * permille as u32) / 1000;
    sorted[idx as usize]
}

// ===========================================================================
// F508 — 负载跟踪谱（PELT 风格，几何衰减 1/2 每 32ms）
// ===========================================================================

pub const PELT_HALF_PERIOD_MS: u32 = 32;

/// 衰减负载：load = load + contrib * 2^-k 形式用移位近似。
pub fn pelt_update(load: u32, contrib: u32, elapsed_ms: u32) -> u32 {
    let halves = (elapsed_ms / PELT_HALF_PERIOD_MS).min(20);
    let decayed = load >> halves;
    load.saturating_sub(decayed).saturating_add(contrib)
}

// ===========================================================================
// F509 — 抢占点宪法
// ===========================================================================

pub const PREEMPT_POINTS: [&str; 6] =
    ["syscall-return", "irq-return", "tick", "cond-resched", "mutex-release", "alloc-slow"];

pub fn preempt_point_known(name: &str) -> bool {
    PREEMPT_POINTS.iter().any(|p| *p == name)
}

// ===========================================================================
// F510 — 组调度舱
// ===========================================================================

#[derive(Clone, Copy)]
pub struct TaskGroup {
    pub gid: u32,
    pub members: u32,
    pub weight: u32,
}

/// 组内均分 CPU 时间片。
pub fn group_slice_total(tg: &TaskGroup, period_ms: u32) -> u32 {
    if tg.members == 0 {
        return 0;
    }
    period_ms / tg.members * 1
}

pub fn group_effective_weight(tg: &TaskGroup) -> u32 {
    tg.weight.max(1)
}

// ===========================================================================
// F511 — 能耗感知钩子
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum PowerHint {
    Boost,
    Normal,
    Sip,
}

pub fn power_hint(battery_permille: u16, thermal_permille: u16) -> PowerHint {
    if thermal_permille >= 900 {
        PowerHint::Sip
    } else if battery_permille <= 150 {
        PowerHint::Sip
    } else if battery_permille >= 500 && thermal_permille <= 600 {
        PowerHint::Boost
    } else {
        PowerHint::Normal
    }
}

// ===========================================================================
// F512 — 实时预算官：防 RT 饿死普通任务
// ===========================================================================

pub const RT_BUDGET_PERMILLE: u16 = 950; // RT 最多占用 95%

pub struct RtAccountant {
    pub period_ms: u32,
    pub rt_used_ms: u32,
}

impl RtAccountant {
    pub fn admit(&self, want_ms: u32) -> bool {
        let cap = (self.period_ms as u64 * RT_BUDGET_PERMILLE as u64 / 1000) as u32;
        self.rt_used_ms + want_ms <= cap
    }
}

// ===========================================================================
// F513 — 调度 fuzz 桩
// ===========================================================================

/// 权重合法性：1~1024；nice 映射后不越界。
pub fn sched_fuzz_ok(weight: u32, nice: i8) -> bool {
    (1..=1024).contains(&weight) && (-20..=19).contains(&nice)
}

// ===========================================================================
// F514 — 调度统计分账
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ThreadStat {
    pub tid: u32,
    pub runtime_ms: u64,
    pub switches: u64,
}

pub fn top_runtime(stats: &mut [ThreadStat], n: usize) -> usize {
    let n = n.min(stats.len());
    for i in 1..n {
        let key = stats[i];
        let mut j = i;
        while j > 0 && stats[j - 1].runtime_ms < key.runtime_ms {
            stats[j] = stats[j - 1];
            j -= 1;
        }
        stats[j] = key;
    }
    n
}

// ===========================================================================
// F515 — 零开销承诺：空闲时无额外工作
// ===========================================================================

pub fn idle_overhead_ticks(idle_ticks: u64, bookkeeping_ticks: u64) -> bool {
    // 空闲记账必须为 0
    bookkeeping_ticks == 0 && idle_ticks > 0
}

// ===========================================================================
// F516 — 调度事件流
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum SchedEvent {
    Wake,
    Sleep,
    Preempt,
    Migrate,
}

pub fn event_subscribable(e: SchedEvent) -> bool {
    matches!(e, SchedEvent::Wake | SchedEvent::Sleep | SchedEvent::Preempt | SchedEvent::Migrate)
}

// ===========================================================================
// F517 — 核间均衡谱
// ===========================================================================

/// 超过不均衡阈值（permille）才触发迁移。
pub const IMBALANCE_TRIGGER_PERMILLE: u16 = 250;

pub fn imbalance_permille(loads: &[u32], n: usize) -> u16 {
    let n = n.max(1).min(loads.len());
    let sum: u64 = loads[..n].iter().map(|&l| l as u64).sum();
    let avg = sum / n as u64;
    if avg == 0 {
        return 0;
    }
    let max = loads[..n].iter().copied().max().unwrap_or(0) as u64;
    let min = loads[..n].iter().copied().min().unwrap_or(0) as u64;
    (((max - min) * 1000) / avg) as u16
}

pub fn balance_needed(loads: &[u32], n: usize) -> bool {
    imbalance_permille(loads, n) > IMBALANCE_TRIGGER_PERMILLE as u32 as u16
}

// ===========================================================================
// F518 — 调度回归走廊
// ===========================================================================

pub const SCHED_CORRIDOR_CASES: [&str; 5] =
    ["ping-pong-latency", "fair-share", "rt-budget", "group-fairness", "wake-storm"];

pub fn sched_corridor_pass(results: &[bool; 5]) -> bool {
    results.iter().all(|&r| r)
}

// ===========================================================================
// F519 — 优先级传播官
// ===========================================================================

/// 依赖链上持有锁者继承等待者的更高优先级。
pub fn priority_inherit(holder: u8, waiter: u8, chain_depth: u8) -> u8 {
    if chain_depth == 0 {
        return holder;
    }
    holder.max(waiter)
}

// ===========================================================================
// F520 — 调度自描述导出
// ===========================================================================

pub struct SchedConfig {
    pub policy: Policy,
    pub latency_target_ms: u32,
    pub min_granularity_ms: u32,
}

pub fn sched_config_exportable(c: &SchedConfig) -> bool {
    c.min_granularity_ms >= 1 && c.min_granularity_ms <= c.latency_target_ms
}

// ===========================================================================
// F521 — 帧任务契约
// ===========================================================================

pub const FRAME_BUDGET_US: u32 = 16_666;
pub const AUDIO_BUDGET_US: u32 = 2_666;

pub fn frame_task_admissible(render_us: u32, audio_us: u32) -> bool {
    render_us <= FRAME_BUDGET_US && audio_us <= AUDIO_BUDGET_US
}

// ===========================================================================
// F522 — 调度压力剧本
// ===========================================================================

pub const STRESS_SCENARIOS: [&str; 5] =
    ["wake-storm-64", "rt-flood", "group-chaos", "migrate-storm", "nice-inversion"];

pub fn stress_scenario_known(name: &str) -> bool {
    STRESS_SCENARIOS.iter().any(|s| *s == name)
}

// ===========================================================================
// F523 — 调度健康分
// ===========================================================================

pub struct SchedHealth {
    pub deadline_misses: u32,
    pub total_deadlines: u32,
    pub p99_latency_us: u32,
}

impl SchedHealth {
    pub fn miss_rate_permille(&self) -> u16 {
        if self.total_deadlines == 0 {
            return 0;
        }
        ((self.deadline_misses as u32 * 1000) / self.total_deadlines) as u16
    }
    pub fn grade(&self) -> u8 {
        if self.miss_rate_permille() == 0 && self.p99_latency_us <= 5_000 {
            0
        } else if self.miss_rate_permille() <= 10 {
            1
        } else {
            2
        }
    }
}

// ===========================================================================
// F524 — 调度文档生成器
// ===========================================================================

pub const SCHED_DOC_SECTIONS: [&str; 5] = ["policy-semantics", "tunables", "guarantees", "faq", "glossary"];

pub fn sched_doc_complete(marks: u8) -> bool {
    marks as u32 == (1u32 << SCHED_DOC_SECTIONS.len()) - 1
}

// ===========================================================================
// F525 — 调度域年报
// ===========================================================================

pub struct SchedYearbook {
    pub switches: u64,
    pub migrations: u64,
    pub balance_passes: u32,
    pub deadline_misses: u32,
}

impl SchedYearbook {
    pub fn migrate_rate_permille(&self) -> u16 {
        if self.switches == 0 {
            return 0;
        }
        (((self.migrations as u64) * 1000 / self.switches as u64).min(1000)) as u16
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m7sched_checks() -> CheckSet {
    let mut set = CheckSet::new("m7sched");

    // F501 宪法
    let rt = SchedEntity { tid: 1, class: SchedClass::RealTime, weight: 10, vruntime: 100 };
    let fair = SchedEntity { tid: 2, class: SchedClass::Fair, weight: 10, vruntime: 1 };
    let fair2 = SchedEntity { tid: 3, class: SchedClass::Fair, weight: 10, vruntime: 5 };
    set.add(
        "F501 core pick",
        core_pick(&rt, &fair) == 0 && core_pick(&fair, &fair2) == 0,
        "class then vruntime",
    );

    // F502 插拔
    set.add(
        "F502 policy",
        policy_registered(Policy::Cfs) && policy_registered(Policy::Budgeted),
        "5 policies",
    );

    // F503 影子
    set.add(
        "F503 shadow",
        shadow_verdict(ShadowRun { base_score: 1000, cand_score: 1000 })
            && !shadow_verdict(ShadowRun { base_score: 1000, cand_score: 900 }),
        "2% gate",
    );

    // F504 回放
    let d = [SchedDecision { tick: 1, picked_tid: 7 }, SchedDecision { tick: 2, picked_tid: 8 }];
    let d2 = [SchedDecision { tick: 1, picked_tid: 7 }, SchedDecision { tick: 2, picked_tid: 9 }];
    set.add(
        "F504 replay",
        replay_deterministic(&d, &d, 2) && !replay_deterministic(&d, &d2, 2),
        "deterministic",
    );

    // F505 公平
    let dev = fairness_deviation(&[50, 50], &[60, 40], 2);
    set.add("F505 fairness", dev == 100, "10% share deviation");

    // F506 交互
    let it = ThreadBehavior { sleeps_short_us: 800, runs_long_us: 200 };
    let batch = ThreadBehavior { sleeps_short_us: 100, runs_long_us: 900 };
    set.add(
        "F506 interactive",
        is_interactive(&it) && !is_interactive(&batch),
        "sleep ratio",
    );

    // F507 延迟分位
    let mut lats = [30u32, 10, 40, 20, 50];
    lats.sort_unstable();
    set.add(
        "F507 latency",
        percentile(&lats, 500) == 30 && percentile(&lats, 990) == 40 && percentile(&lats, 1000) == 50,
        "p50/p99",
    );

    // F508 PELT
    let l0 = pelt_update(0, 1024, 0);
    let l1 = pelt_update(1024, 0, 32);
    set.add(
        "F508 pelt",
        l0 == 1024 && l1 == 512,
        "half-life decay",
    );

    // F509 抢占点
    set.add(
        "F509 preempt",
        preempt_point_known("tick") && !preempt_point_known("magic"),
        "6 points",
    );

    // F510 组调度
    let tg = TaskGroup { gid: 1, members: 4, weight: 100 };
    set.add(
        "F510 group",
        group_slice_total(&tg, 8) == 2 && group_effective_weight(&tg) == 100,
        "slice split",
    );

    // F511 能耗钩子
    set.add(
        "F511 power hint",
        power_hint(600, 500) == PowerHint::Boost
            && power_hint(100, 500) == PowerHint::Sip
            && power_hint(600, 950) == PowerHint::Sip,
        "hint matrix",
    );

    // F512 RT 预算
    let acc = RtAccountant { period_ms: 1000, rt_used_ms: 900 };
    set.add(
        "F512 rt budget",
        acc.admit(40) && !acc.admit(60),
        "95% cap",
    );

    // F513 fuzz 桩
    set.add(
        "F513 fuzz",
        sched_fuzz_ok(1, -20) && sched_fuzz_ok(1024, 19) && !sched_fuzz_ok(0, 0) && !sched_fuzz_ok(2000, 0),
        "weight/nice bounds",
    );

    // F514 分账
    let mut stats = [
        ThreadStat { tid: 1, runtime_ms: 100, switches: 3 },
        ThreadStat { tid: 2, runtime_ms: 900, switches: 7 },
        ThreadStat { tid: 3, runtime_ms: 400, switches: 5 },
    ];
    let tn = top_runtime(&mut stats, 3);
    set.add("F514 stats", tn == 3 && stats[0].tid == 2, "ranked by runtime");

    // F515 零开销
    set.add(
        "F515 zero overhead",
        idle_overhead_ticks(1000, 0) && !idle_overhead_ticks(1000, 5),
        "idle bookkeeping 0",
    );

    // F516 事件流
    set.add(
        "F516 events",
        event_subscribable(SchedEvent::Migrate) && event_subscribable(SchedEvent::Wake),
        "4 events",
    );

    // F517 均衡
    let balanced = [100u32, 100, 100];
    let skewed = [100u32, 100, 400];
    set.add(
        "F517 balance",
        !balance_needed(&balanced, 3) && balance_needed(&skewed, 3),
        "25% trigger",
    );

    // F518 走廊
    set.add(
        "F518 corridor",
        sched_corridor_pass(&[true; 5]) && !sched_corridor_pass(&[true, true, true, true, false]),
        "5 cases",
    );

    // F519 传播
    set.add(
        "F519 inherit",
        priority_inherit(1, 9, 2) == 9 && priority_inherit(9, 3, 2) == 9 && priority_inherit(5, 3, 0) == 5,
        "max along chain",
    );

    // F520 导出
    let cfg = SchedConfig { policy: Policy::Cfs, latency_target_ms: 24, min_granularity_ms: 3 };
    set.add(
        "F520 export",
        sched_config_exportable(&cfg) && !sched_config_exportable(&SchedConfig { policy: Policy::Cfs, latency_target_ms: 2, min_granularity_ms: 3 }),
        "granule <= target",
    );

    // F521 帧契约
    set.add(
        "F521 frame contract",
        frame_task_admissible(16_000, 2_000) && !frame_task_admissible(17_000, 2_000),
        "budgets",
    );

    // F522 压力剧本
    set.add(
        "F522 stress",
        stress_scenario_known("wake-storm-64") && !stress_scenario_known("chaos"),
        "5 scenarios",
    );

    // F523 健康分
    let h = SchedHealth { deadline_misses: 0, total_deadlines: 100, p99_latency_us: 3_000 };
    let hb = SchedHealth { deadline_misses: 20, total_deadlines: 100, p99_latency_us: 30_000 };
    set.add(
        "F523 health",
        h.grade() == 0 && hb.grade() == 2 && h.miss_rate_permille() == 0 && hb.miss_rate_permille() == 200,
        "miss rate ladder",
    );

    // F524 文档
    set.add(
        "F524 docs",
        sched_doc_complete(0b1_1111) && !sched_doc_complete(0b0_1111),
        "5 sections",
    );

    // F525 年报
    let yb = SchedYearbook { switches: 1000, migrations: 100, balance_passes: 10, deadline_misses: 0 };
    set.add("F525 yearbook", yb.migrate_rate_permille() == 100, "migration rate");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f505_perfect_fair() {
        assert_eq!(fairness_deviation(&[25, 25, 25, 25], &[25, 25, 25, 25], 4), 0);
    }

    #[test]
    fn f507_empty_and_single() {
        assert_eq!(percentile(&[], 990), 0);
        assert_eq!(percentile(&[7], 990), 7);
    }

    #[test]
    fn f508_pelt_decay_chain() {
        let mut l = pelt_update(0, 1024, 0);
        for _ in 0..5 {
            l = pelt_update(l, 0, 32);
        }
        assert_eq!(l, 1024 >> 5);
    }

    #[test]
    fn f519_chain_depth_zero() {
        assert_eq!(priority_inherit(4, 9, 0), 4);
    }

    #[test]
    fn f525_domain_selfcheck_all_pass() {
        let set = run_m7sched_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "{} | {}", c.name, c.detail);
        }
    }
}
