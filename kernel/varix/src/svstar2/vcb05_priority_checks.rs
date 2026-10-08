//! CGPU-F0165 自检 · 优先级发射与抢占点（CGPU-B 域）
//!
//! **锚点判据逐条对应**（`#CGPU-F0165`「三档发射序、抢占点检查、让位
//! 延迟 P95、饥饿防护（F0090 老化复用）、遥测」）：
//!
//! | 锚点判据 | 自检组 |
//! |---|---|
//! | 三档发射序 | `CB05-三档-*`（映射全表+drain 严格档序+同档 FIFO+老化升档尾插） |
//! | 抢占点检查 | `CB05-抢占-*`（步长纪律+请求让位+无请求跑满+边界时刻） |
//! | 让位延迟 P95 | `CB05-P95-*`（判据侧独立手算分位对拍+达标线） |
//! | 饥饿防护 | `CB05-饥饿-*`（恰阈值升档+未到不升+升档清零+尾插不特权） |
//! | 遥测 | `CB05-遥测-*`（三档计数/让位账/检查点账/守恒） |
//! | 判据元 | `CB05-判据-*`（版本/常量字面量/三档封闭/条数对账） |
//!
//! **判据设计硬规矩**：期望值判据侧独立手算；不变量两头都测（恰阈值
//! 升档+未到不升）；判据区零 panic 面。

use crate::checks::CheckSet;
use crate::svstar2::vcb05_priority as pr;

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 组一：三档发射序
// ---------------------------------------------------------------------------

fn chk_tiers(s: &mut CheckSet) {
    // CB05-三档-01：tier_of 映射全表（判据侧独立重算 0/1/2/255 四代表）。
    let ok = matches!(pr::tier_of(0), pr::Tier::Realtime)
        && matches!(pr::tier_of(1), pr::Tier::Interactive)
        && matches!(pr::tier_of(2), pr::Tier::Background)
        && matches!(pr::tier_of(255), pr::Tier::Background);
    s.add("CB05-三档-01", ok, "priority→tier 映射表 0/1/2+/255 四代表");

    // CB05-三档-02：三档混入 drain 严格档序（高优先级先发射）。
    let mut q = pr::LaunchQueue::new();
    let _ = pr::feed_wave(&mut q, &[1, 2, 3, 4, 5, 6], &[2, 0, 1, 2, 0, 1]);
    let mut order: Vec<u32> = Vec::new();
    while let Some(t) = q.drain_next() {
        order.push(t.node);
    }
    let ok = order == vec![2, 5, 3, 6, 1, 4];
    s.add("CB05-三档-02", ok, "rt{2,5}→ia{3,6}→bg{1,4} 严格档序发射");

    // CB05-三档-03：同档 FIFO（入队序即出队序）。
    let mut q = pr::LaunchQueue::new();
    q.push(10, pr::Tier::Realtime);
    q.push(11, pr::Tier::Realtime);
    q.push(12, pr::Tier::Realtime);
    let a = q.drain_next().map_or(0, |t| t.node);
    let b = q.drain_next().map_or(0, |t| t.node);
    let ok = a == 10 && b == 11 && q.len_by(pr::Tier::Realtime) == 1;
    s.add("CB05-三档-03", ok, "同档入队序即出队序（FIFO）");

    // CB05-三档-04：老化升档走队尾（公平不是特权）。
    let mut q = pr::LaunchQueue::new();
    q.push(20, pr::Tier::Background); // seq0
    q.push(21, pr::Tier::Background); // seq1
    let _ = q.promote(21); // 21 升入交互档队尾
    let first = q.drain_next().map_or(0, |t| t.node); // rt 空 → ia 队首=21
    let ok = first == 21 && q.pending() == 1;
    s.add("CB05-三档-04", ok, "升档任务入新档队尾且先于旧档发射");

    // CB05-三档-05：实时帧任务无档可升（promote 返回 false 不动账）。
    let mut q = pr::LaunchQueue::new();
    q.push(30, pr::Tier::Realtime);
    let ok = !q.promote(30) && q.len_by(pr::Tier::Realtime) == 1;
    s.add("CB05-三档-05", ok, "实时帧不可再升（顶档不动）");

    // CB05-三档-06：空队列 drain 返回 None。
    let mut q = pr::LaunchQueue::new();
    let ok = q.drain_next().is_none() && q.pending() == 0;
    s.add("CB05-三档-06", ok, "空队列显性 None（不产幻觉发射）");
}

// ---------------------------------------------------------------------------
// 组二：抢占点检查
// ---------------------------------------------------------------------------

fn chk_preempt(s: &mut CheckSet) {
    // CB05-抢占-01：无请求跑满全程（10ms→5 个检查点步长 2ms）。
    let r = pr::run_slices(10, false, 0);
    let ok = !r.preempted
        && r.ran_ms == 10
        && r.checkpoints == 5
        && r.yield_latency_ms == 0;
    s.add("CB05-抢占-01", ok, "无请求跑满 10ms 恰 5 检查点");

    // CB05-抢占-02：片内请求在下个检查点让位（延迟≤步长）。
    let r = pr::run_slices(10, true, 3); // 3ms 请求 → 4ms 检查点让位
    let ok = r.preempted
        && r.yield_at_ms == 4
        && r.yield_latency_ms == 1
        && r.checkpoints == 2;
    s.add("CB05-抢占-02", ok, "3ms 请求在 4ms 检查点让位延迟 1ms");

    // CB05-抢占-03：请求与检查点同刻——零延迟让位（整数粒度下最理想）；
    //  最坏延迟=整片等待（2ms 上界由抢占-02 覆盖 latency=1 与抢占-04 的 2）。
    let r = pr::run_slices(10, true, 2);
    let ok = r.preempted && r.yield_at_ms == 2 && r.yield_latency_ms == 0;
    s.add("CB05-抢占-03", ok, "同刻请求零延迟让位（latency=ran-request）");

    // CB05-抢占-04：请求在起点——首个检查点即让位。
    let r = pr::run_slices(10, true, 0);
    let ok = r.preempted && r.yield_at_ms == 2 && r.yield_latency_ms == 2;
    s.add("CB05-抢占-04", ok, "0ms 请求 2ms 检查点让位（首点响应）");

    // CB05-抢占-05：请求晚于预算——跑满不让位。
    let r = pr::run_slices(4, true, 10);
    let ok = !r.preempted && r.ran_ms == 4 && r.checkpoints == 2;
    s.add("CB05-抢占-05", ok, "预算外请求不影响本轮（跑满）");

    // CB05-抢占-06：步长为 1ms 的任务恰 budget 个检查点（步长纪律普适）。
    let r = pr::run_slices(3, false, 0);
    let ok = r.checkpoints == 2; // 2ms 一片：3ms 恰 2 片（2+1 末尾不足步长也算点）
    s.add("CB05-抢占-06", ok, "3ms 预算恰 2 检查点（末尾不足步长收口）");
}

// ---------------------------------------------------------------------------
// 组三：让位延迟 P95
// ---------------------------------------------------------------------------

fn chk_p95(s: &mut CheckSet) {
    // CB05-P95-01：判据侧独立手算——样本 1..=20 的 P95=第 19 个=19。
    let samples: Vec<u32> = (1u32..=20).collect();
    let ok = pr::p95(&samples) == 19;
    s.add("CB05-P95-01", ok, "20 样本 P95=19（ceil(0.95n) 位）独立手算");

    // CB05-P95-02：单样本 P95=自身（ceil(0.95)=1 位）。
    let samples: Vec<u32> = vec![7];
    let ok = pr::p95(&samples) == 7;
    s.add("CB05-P95-02", ok, "单样本 P95=自身");

    // CB05-P95-03：乱序样本排序后取分位（2,9,1,8,3...100 打乱）。
    let samples: Vec<u32> = vec![100, 5, 62, 33, 1, 99, 7, 88, 44, 50];
    // 排序后 [1,5,7,33,44,50,62,88,99,100]，n=10 → idx=ceil(9.5)-1=9 → 100。
    let ok = pr::p95(&samples) == 100;
    s.add("CB05-P95-03", ok, "乱序样本排序分位恰最大值（10 样本）");

    // CB05-P95-04：空样本显性 0（不 panic）。
    let samples: Vec<u32> = Vec::new();
    let ok = pr::p95(&samples) == 0;
    s.add("CB05-P95-04", ok, "空样本 P95=0 零 panic");

    // CB05-P95-05：达标线——让位延迟上界=步长 → 样本全 2 时 P95=2 达标。
    let samples: Vec<u32> = vec![2, 2, 1, 2, 1, 2, 2, 1, 2, 2];
    let ok = pr::p95(&samples) == 2 && pr::p95(&samples) <= pr::PREEMPT_CHECK_INTERVAL_MS;
    s.add("CB05-P95-05", ok, "全样本≤步长时 P95 达标（≤2ms）");
}

// ---------------------------------------------------------------------------
// 组四：饥饿防护（F0090 老化复用）
// ---------------------------------------------------------------------------

fn chk_aging(s: &mut CheckSet) {
    // CB05-饥饿-01：恰阈值轮数进升档名单（>⇄>= 变异即红）。
    let mut ag = pr::AgingTracker::new();
    ag.watch(40);
    let mut due: Vec<u32> = Vec::new();
    for _ in 0..(pr::AGING_ROUNDS - 1) {
        due = ag.tick_round();
    }
    let before = due.is_empty() && ag.rounds(40) == pr::AGING_ROUNDS - 1;
    due = ag.tick_round();
    let ok = before && due == vec![40];
    s.add("CB05-饥饿-01", ok, "恰 AGING_ROUNDS 轮进升档名单");

    // CB05-饥饿-02：升档后计数清零重新起算。
    let _ = ag.tick_round();
    let r_after = ag.rounds(40);
    ag.reset(40);
    let ok = r_after == pr::AGING_ROUNDS + 1 && ag.rounds(40) == 0;
    s.add("CB05-饥饿-02", ok, "reset 清零重新起算（升档不豁免后续）");

    // CB05-饥饿-03：未追踪任务 rounds=0（watch 幂等）。
    let mut ag = pr::AgingTracker::new();
    ag.watch(50);
    ag.watch(50);
    let ok = ag.rounds(50) == 0 && ag.rounds(99) == 0;
    s.add("CB05-饥饿-03", ok, "watch 幂等且未追踪查 0");

    // CB05-饥饿-04：闭环——实时流每轮插队 3 任务，后台任务 60 老化 8 轮
    //  升交互档；全量发射后 60 必被发射（无老化则持续积压）。
    let mut q = pr::LaunchQueue::new();
    let mut ag = pr::AgingTracker::new();
    q.push(60, pr::Tier::Background);
    ag.watch(60);
    let mut promoted = false;
    for _ in 0..pr::AGING_ROUNDS {
        let due = ag.tick_round();
        for n in due.iter() {
            promoted = q.promote(*n);
            ag.reset(*n);
        }
        q.push(70, pr::Tier::Realtime);
        q.push(71, pr::Tier::Realtime);
        q.push(72, pr::Tier::Realtime);
    }
    let ok_tiers = promoted
        && q.len_by(pr::Tier::Background) == 0
        && q.len_by(pr::Tier::Interactive) == 1;
    let mut launched_60 = false;
    while let Some(t) = q.drain_next() {
        if t.node == 60 {
            launched_60 = true;
        }
    }
    let ok = ok_tiers && launched_60;
    s.add("CB05-饥饿-04", ok, "实时流持续插队下后台任务经老化获升档并发射");

    // CB05-饥饿-05：升档计数入遥测账。
    let ok = ag.promotions == 1;
    s.add("CB05-饥饿-05", ok, "升档计 1 次（遥测账实）");
}

// ---------------------------------------------------------------------------
// 组五：遥测
// ---------------------------------------------------------------------------

fn chk_telemetry(s: &mut CheckSet) {
    // CB05-遥测-01：三档发射计数分账。
    let mut q = pr::LaunchQueue::new();
    let mut te = pr::PriorityTelemetry::default();
    let _ = pr::feed_wave(&mut q, &[1, 2, 3], &[0, 1, 2]);
    while let Some(t) = q.drain_next() {
        te.note_launch(t.tier);
    }
    let ok = te.launched_by_tier == [1, 1, 1] && te.launched_total() == 3;
    s.add("CB05-遥测-01", ok, "三档发射计数分账恰 1/1/1");

    // CB05-遥测-02：让位账——样本与计数同步入账。
    let mut te = pr::PriorityTelemetry::default();
    let r1 = pr::run_slices(10, true, 3);
    te.note_preempt(&r1);
    let r2 = pr::run_slices(10, false, 0);
    te.note_preempt(&r2);
    let r3 = pr::run_slices(6, true, 5);
    te.note_preempt(&r3);
    let ok = te.preemptions == 2
        && te.yield_latencies == vec![1, 1]
        && te.checkpoints == 2 + 5 + 3;
    s.add("CB05-遥测-02", ok, "让位 2 次/样本 2 个/检查点账 10");

    // CB05-遥测-03：P95 桥接（账内原料直出分位）。
    let ok = te.yield_p95() == 1;
    s.add("CB05-遥测-03", ok, "账内 P95 直出（样本 1,1 → 1）");

    // CB05-遥测-04：守恒——发射总数+队列待发射=总入队数。
    let mut q = pr::LaunchQueue::new();
    let mut te = pr::PriorityTelemetry::default();
    let _ = pr::feed_wave(&mut q, &[1, 2, 3, 4], &[0, 1, 2, 0]);
    if let Some(t) = q.drain_next() {
        te.note_launch(t.tier);
    }
    if let Some(t) = q.drain_next() {
        te.note_launch(t.tier);
    }
    let ok = q.enqueued as u32 == te.launched_total() + q.pending() as u32;
    s.add("CB05-遥测-04", ok, "入队=已发射+待发射 守恒");

    // CB05-遥测-05：读屏行携带四要素数字。
    let line = te.screen_line();
    let ok = line.contains("发射 2") && line.contains("升档 0");
    s.add("CB05-遥测-05", ok, "读屏行携带发射数与升档数");
}

// ---------------------------------------------------------------------------
// 组六：判据元
// ---------------------------------------------------------------------------

fn chk_meta(s: &mut CheckSet) {
    // CB05-判据-01：版本字面量钉死。
    let ok = pr::PRIORITY_VERSION == "CB05-priority-v1";
    s.add("CB05-判据-01", ok, "PRIORITY_VERSION 字面量钉死");

    // CB05-判据-02：常量字面量（步长 2ms/老化 8 轮/P95 千分比 950）。
    let ok = pr::PREEMPT_CHECK_INTERVAL_MS == 2
        && pr::AGING_ROUNDS == 8
        && pr::P95_PERMILLE == 950;
    s.add("CB05-判据-02", ok, "步长 2ms/老化 8/P95 950 字面量钉死");

    // CB05-判据-03：三档封闭（tag/zh/rank 互异+all 顺序）。
    let all = pr::Tier::all();
    let tags = [all[0].tag(), all[1].tag(), all[2].tag()];
    let zh = [all[0].zh(), all[1].zh(), all[2].zh()];
    let ok = all[0].rank() == 0
        && all[1].rank() == 1
        && all[2].rank() == 2
        && tags[0] != tags[1]
        && tags[1] != tags[2]
        && tags[0] != tags[2]
        && zh[0] != zh[1]
        && zh[1] != zh[2]
        && zh[0] != zh[2];
    s.add("CB05-判据-03", ok, "三档封闭可逆（tag/zh/rank 互异）");
}

// ---------------------------------------------------------------------------
// 聚合
// ---------------------------------------------------------------------------

/// CGPU-F0165 域自检入口（聚合防自调：只调组函数+自身 tally）。
pub fn run_vcb05_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0165");
    chk_tiers(&mut s);
    chk_preempt(&mut s);
    chk_p95(&mut s);
    chk_aging(&mut s);
    chk_telemetry(&mut s);
    chk_meta(&mut s);
    // 条数对账：6 组 30 条（三档 6+抢占 6+P95 5+饥饿 5+遥测 5+判据 3）。
    let (pass, fail) = s.tally();
    let ok = pass + fail == 30;
    s.add("CB05-判据-04", ok, "条数对账：实挂 30 条（6 组）");
    s
}
