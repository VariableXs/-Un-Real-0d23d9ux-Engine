//! F0168 子模块二 · 一致性校验与修复面（原交付结构六、七、十一、十二、十三节）。
//!
//! 六、约束校验器（判据三：同资源读写队列一致性；跨卡且无拷贝类任务视为直引违例）；
//! 七、映射开销账（判据四；口径复用 F2408：步=0.1% 帧预算，Σ=10 步红线，实测全上游注入——零墙钟）；
//! 十一、跨卡搬运计划器；十二、队列实例路由（slot → 实例号）；十三、负载分布统计（全部无浮点）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::mapping::*;

// 六、约束校验器（判据三：映射结论 × 节点读写集 → 违例清单；三级评级按强度序排列，修复面见 plan_transfer）。

/// 一致性等级闭集（三级强度序由 rank 钉死——违例分档与修复面共用此序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoherenceLevel {
    /// 完全一致：同资源全部任务同队列同卡（最强——零跨队列同步开销）。
    Uniform,
    /// 同卡异队列：同卡上跨队列（需要卡内队列同步原语——合法但计一次同步）。
    SameCardCrossQueue,
    /// 跨卡：同资源被多卡触碰（必须经拷贝队列显式搬运——直引显性拒）。
    CrossCard,
}

impl CoherenceLevel {
    /// 强度序（Uniform 最强；数值越小越强——校验器评级与违例分档均引用，改序即改判据）。
    pub const fn rank(self) -> u8 {
        match self {
            CoherenceLevel::Uniform => 0,
            CoherenceLevel::SameCardCrossQueue => 1,
            CoherenceLevel::CrossCard => 2,
        }
    }
}

/// 一致性违例记录（advice 与 level 配套产出：跨卡无拷贝才落账——每行违例可直接行动）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoherenceViolation {
    /// 违例资源。
    pub resource: u32,
    /// 触碰该资源的任务 id（升序）。
    pub tasks: Vec<u32>,
    /// 当前组合等级。
    pub level: CoherenceLevel,
    /// 人读修复建议。
    pub advice: String,
}

/// 去重追加原语（触碰任务/卡/队列清单共用；保序：首触序即列序——聚合确定性由此保证）。
fn push_unique<T: Copy + PartialEq>(v: &mut Vec<T>, x: T) {
    if !v.contains(&x) {
        v.push(x);
    }
}

/// 拷贝类任务判定（Upload/Download/DeviceCopy——跨卡触碰的合法性依据）。
fn is_copy_kind(k: TaskKind) -> bool {
    k == TaskKind::Upload || k == TaskKind::Download || k == TaskKind::DeviceCopy
}

/// 触碰行落账原语：已有行去重追加任务 id；无行新开——资源首触序即账目序。
fn push_touch(touched_map: &mut Vec<(u32, Vec<u32>)>, rid: u32, task_id: u32) {
    if let Some((_, tasks)) = touched_map.iter_mut().find(|(r, _)| *r == rid) {
        push_unique(tasks, task_id);
        return;
    }
    let mut tasks: Vec<u32> = Vec::new();
    tasks.push(task_id);
    touched_map.push((rid, tasks));
}

/// 单节点触碰展开（collect_resource_touches 内层拆分——压平嵌套，语义逐句等价）。
fn absorb_node_touches(touched_map: &mut Vec<(u32, Vec<u32>)>, node: &MapNode) {
    for rid in node.touched() {
        push_touch(touched_map, rid, node.task_id);
    }
}

/// 资源 → 触碰任务清单聚合：逐节点展开触碰资源，同资源触碰任务去重追加。
/// 输出序确定性：节点序 × 触碰序——同一输入聚合结果恒一致。
fn collect_resource_touches(nodes: &[MapNode]) -> Vec<(u32, Vec<u32>)> {
    let mut touched_map: Vec<(u32, Vec<u32>)> = Vec::new();
    for node in nodes {
        absorb_node_touches(&mut touched_map, node);
    }
    touched_map
}

/// 单任务放置贡献（collect_placement 内层拆分——压平嵌套）：落卡/队列去重追加，
/// 返回该任务是否为拷贝类（拷贝位口径与原内联体逐句等价）。
fn task_placement(
    tid: u32,
    nodes: &[MapNode],
    mappings: &[QueueMapping],
    cards: &mut Vec<u32>,
    queues: &mut Vec<u8>,
) -> bool {
    let kind = nodes.iter().find(|n| n.task_id == tid).map(|n| n.kind);
    if let Some(mp) = mappings.iter().find(|mp| mp.task_id == tid) {
        push_unique(cards, mp.card);
        push_unique(queues, mp.queue.ordinal() as u8);
    }
    kind.map_or(false, is_copy_kind)
}

/// 触碰任务群的放置聚合：去重聚合 (卡清单, 队列清单, 是否含拷贝任务)。
/// 三元组即评级原料——卡数判跨卡、队列数判跨队列、拷贝位判搬运合法性。
fn collect_placement(
    tasks: &[u32],
    nodes: &[MapNode],
    mappings: &[QueueMapping],
) -> (Vec<u32>, Vec<u8>, bool) {
    let mut cards: Vec<u32> = Vec::new();
    let mut queues: Vec<u8> = Vec::new();
    let mut copy_present = false;
    for tid in tasks.iter() {
        let task_copy = task_placement(*tid, nodes, mappings, &mut cards, &mut queues);
        copy_present = copy_present || task_copy;
    }
    (cards, queues, copy_present)
}

/// 一致性评级（卡数/队列数 → 等级：跨卡 > 同卡异队列 > 完全一致，强度递减）。
/// 纯函数：只依赖两个计数，不触账——评级规则可独立复核。
fn grade_coherence(cards: usize, queues: usize) -> CoherenceLevel {
    if cards > 1 {
        CoherenceLevel::CrossCard
    } else if queues > 1 {
        CoherenceLevel::SameCardCrossQueue
    } else {
        CoherenceLevel::Uniform
    }
}

/// 跨卡无拷贝违例行落账（check_coherence 内层拆分——压平嵌套；advice 文案与字段装配逐字保留）。
fn push_violation_if_illegal(
    out: &mut Vec<CoherenceViolation>,
    rid: u32,
    tasks: &[u32],
    nodes: &[MapNode],
    mappings: &[QueueMapping],
) {
    let (cards, queues, copy_present) = collect_placement(tasks, nodes, mappings);
    let level = grade_coherence(cards.len(), queues.len());
    if level == CoherenceLevel::CrossCard && !copy_present {
        let advice = "同资源被多卡触碰且无拷贝任务：建议插入 DeviceCopy（拷贝队列）显式搬运，或把触碰任务归并到资源归属卡";
        out.push(CoherenceViolation {
            resource: rid,
            tasks: tasks.to_vec(),
            level,
            advice: advice.to_string(),
        });
    }
}

/// 约束校验器（判据三：映射结论 × 节点读写集 → 违例清单）。
/// 规则：同资源全部触碰任务同卡同队列 → Uniform 合法；同卡异队列 →
/// SameCardCrossQueue 合法但记录；跨卡 → CrossCard，若含拷贝类任务则视为
/// 显式搬运合法，否则违例（建议插入 DeviceCopy）。
/// 结构：触碰聚合 → 放置聚合 → 评级 → 落账，四段各自可独立复核。
pub fn check_coherence(
    nodes: &[MapNode],
    mappings: &[QueueMapping],
) -> Vec<CoherenceViolation> {
    let mut violations: Vec<CoherenceViolation> = Vec::new();
    let touched_map = collect_resource_touches(nodes);
    for (rid, tasks) in touched_map.iter() {
        push_violation_if_illegal(&mut violations, *rid, tasks, nodes, mappings);
    }
    violations
}

/// 校验通过判（违例空表即通过——映射可提交执行面）。
/// 只回答「能不能提交」；违例明细走 [`check_coherence`]（两者对同一输入结论恒一致）。
pub fn coherence_ok(nodes: &[MapNode], mappings: &[QueueMapping]) -> bool {
    check_coherence(nodes, mappings).is_empty()
}

// 七、映射开销账（判据四；口径复用 F2408：步=0.1% 帧预算，Σ=10 步红线；表对拍见 map_budget_table_ok）。

/// 映射面开销段闭集（段名即账面键——与 MAP_BUDGET_TABLE、实测数组下标对齐）。
pub const MAP_SEGMENTS: [&str; 4] = ["规则查行", "路由推导", "一致性校验", "账本落账"];

/// 每段预算步（Σ=10 步 = 1.0% 帧预算——映射是帧的固定税；下标与 MAP_SEGMENTS 对应）。
pub const MAP_BUDGET_STEPS: [u32; 4] = [2, 3, 3, 2];

/// 帧预算总步（与 F2408 同源口径 10000 步）。
pub const MAP_FRAME_BUDGET_STEPS: u32 = 10_000;

/// 开销预算行（一段一账；measured 上游注入——零墙钟；行序 = 段序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapBudgetLine {
    /// 段名。
    pub segment: &'static str,
    /// 预算步。
    pub budget: u32,
    /// 实测步（上游注入）。
    pub measured: u32,
}

/// 预算表（与 MAP_SEGMENTS/MAP_BUDGET_STEPS 对拍——判据侧独立重算）。
pub const MAP_BUDGET_TABLE: [MapBudgetLine; 4] = [
    MapBudgetLine { segment: MAP_SEGMENTS[0], budget: MAP_BUDGET_STEPS[0], measured: 0 },
    MapBudgetLine { segment: MAP_SEGMENTS[1], budget: MAP_BUDGET_STEPS[1], measured: 0 },
    MapBudgetLine { segment: MAP_SEGMENTS[2], budget: MAP_BUDGET_STEPS[2], measured: 0 },
    MapBudgetLine { segment: MAP_SEGMENTS[3], budget: MAP_BUDGET_STEPS[3], measured: 0 },
];

/// 预算行对账（表错先红：逐项 zip 对拍——行序对段序、行值对预算值，任一失配即表错）。
pub fn map_budget_table_ok() -> bool {
    if MAP_BUDGET_TABLE.len() != 4 {
        return false;
    }
    MAP_BUDGET_TABLE.iter().enumerate().all(|(i, row)| {
        MAP_SEGMENTS.get(i) == Some(&row.segment) && MAP_BUDGET_STEPS.get(i) == Some(&row.budget)
    })
}

/// 映射开销审计（判据四：Σmeasured 占帧预算万分比；恰红线为达标「≤」）。
/// ppm = Σ × 10_000 / 10_000 步；饱和加法防溢出（异常大输入只封顶不回绕）。
pub const fn map_overhead_ppm(measured_steps: &[u32; 4]) -> u32 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < 4 {
        let m = measured_steps[i];
        sum = sum.saturating_add(m);
        i += 1;
    }
    sum.saturating_mul(10_000) / MAP_FRAME_BUDGET_STEPS
}

/// 预算占比上界（Σbudget=10 步 = 万分之十 = 0.1% → 10 ppm）——红线数值，实测 ≤ 本值即达标。
pub const MAP_BUDGET_PPM: u32 = 10;

// 十一、跨卡搬运计划器（CrossCard 违例 → 可执行搬运建议：目标卡=非源卡的最小触碰卡，插入位置写依赖先行）。

/// 一条搬运建议（kind_hint 恒 DeviceCopy——拷贝队列显式搬运是本域唯一合法手段）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferPlan {
    /// 被搬运资源。
    pub resource: u32,
    /// 源卡（资源归属卡）。
    pub from_card: u32,
    /// 目标卡（缺拷贝任务的那个触碰卡）。
    pub to_card: u32,
    /// 建议插入位置：在此任务之前执行搬运。
    pub before_task: u32,
    /// 建议搬运类别（恒 DeviceCopy——拷贝队列显式搬运）。
    pub kind_hint: TaskKind,
    /// 人读依据。
    pub basis: String,
}

/// 候选卡更优判（平局规则的唯一裁决拆分：无目标即优、否则取更小——可独立复核）。
fn beats_target(cand: u32, target: Option<u32>) -> bool {
    match target {
        Some(t) => cand < t,
        None => true,
    }
}

/// 单任务异卡候选（min_foreign_card 内层拆分：首个「本任务映射且非源卡」的卡 id）。
fn foreign_card_of(tid: u32, mappings: &[QueueMapping], from_card: u32) -> Option<u32> {
    let found = mappings
        .iter()
        .find(|mp| mp.task_id == tid && mp.card != from_card);
    found.map(|mp| mp.card)
}

/// 候选吸收（目标卡收敛原语：候选更优即改写目标——压平嵌套拆分，语义逐句等价）。
fn absorb_smaller(target: &mut Option<u32>, cand: Option<u32>) {
    let better = match cand {
        Some(c) => beats_target(c, *target),
        None => false,
    };
    if better {
        *target = cand;
    }
}

/// 目标卡推导（触碰任务中「非资源归属卡」的最小卡 id；无跨卡触碰 → None）。
/// 「最小」是平局规则的唯一裁决——多卡触碰时计划落最小卡可复现。
fn min_foreign_card(
    violation: &CoherenceViolation,
    mappings: &[QueueMapping],
    from_card: u32,
) -> Option<u32> {
    let mut target: Option<u32> = None;
    for tid in violation.tasks.iter() {
        let cand = foreign_card_of(*tid, mappings, from_card);
        absorb_smaller(&mut target, cand);
    }
    target
}

/// 任务是否在违例触碰清单（first_touch_on_card 内层拆分——线性在册判定）。
fn touches_task(violation: &CoherenceViolation, task_id: u32) -> bool {
    violation.tasks.iter().any(|t| *t == task_id)
}

/// 插入位置推导：映射序上最早落到目标卡、且触碰本资源的任务（写依赖先行）。
/// 映射清单序即拓扑执行序——「最早」由此序定义，不另排序。
fn first_touch_on_card(
    violation: &CoherenceViolation,
    mappings: &[QueueMapping],
    to_card: u32,
) -> Option<u32> {
    mappings
        .iter()
        .filter(|mp| mp.card == to_card)
        .find(|mp| touches_task(violation, mp.task_id))
        .map(|mp| mp.task_id)
}

/// 从违例生成搬运计划（判据三的修复面：不静默、给可执行建议）。
/// 规则：目标卡 = 非源卡的最小触碰卡（平局取最小）；插入位置 = 最早映射到
/// 目标卡的触碰任务（写依赖先行）。
/// 返回 None 的三种情形（调用方如实降级）：资源无归属行 / 无跨卡触碰 / 目标卡触碰缺失。
pub fn plan_transfer(
    violation: &CoherenceViolation,
    nodes: &[MapNode],
    mappings: &[QueueMapping],
    homes: &ResourceHome,
) -> Option<TransferPlan> {
    let from_card = homes.home_of(violation.resource)?;
    let to_card = min_foreign_card(violation, mappings, from_card)?;
    let before_task = first_touch_on_card(violation, mappings, to_card)?;
    Some(TransferPlan {
        resource: violation.resource,
        from_card,
        to_card,
        before_task,
        kind_hint: TaskKind::DeviceCopy,
        basis: alloc::format!(
            "资源 {} 由卡 {} 搬运至卡 {}：在任务 {} 之前插入 DeviceCopy（拷贝队列），写依赖先行",
            violation.resource, from_card, to_card, before_task
        ),
    })
}

/// 跨卡违例落计划（plan_transfers 内层拆分——压平嵌套；非跨卡级跳过、None 计划自然
/// 不落，与原内联体语义逐句等价）。
fn push_plan_if_cross_card(
    out: &mut Vec<TransferPlan>,
    v: &CoherenceViolation,
    nodes: &[MapNode],
    mappings: &[QueueMapping],
    homes: &ResourceHome,
) {
    if v.level == CoherenceLevel::CrossCard {
        out.extend(plan_transfer(v, nodes, mappings, homes));
    }
}

/// 批量计划（全部跨卡违例 → 计划清单；输出序 = 违例输入序——计划间无隐藏耦合）。
pub fn plan_transfers(
    violations: &[CoherenceViolation],
    nodes: &[MapNode],
    mappings: &[QueueMapping],
    homes: &ResourceHome,
) -> Vec<TransferPlan> {
    let mut out: Vec<TransferPlan> = Vec::new();
    for v in violations.iter() {
        push_plan_if_cross_card(&mut out, v, nodes, mappings, homes);
    }
    out
}

// 十二、队列实例路由（slot → 队列实例号：Pinned 独占 / RoundRobin 轮转；容量 0 防御为 1）。

/// 队列实例分派策略闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotPolicy {
    /// 独占：全部落实例 0（简单、缓存友好——默认）。
    Pinned,
    /// 轮转：slot 按实例数取模（均衡——多实例并行收益场景）。
    RoundRobin,
}

impl SlotPolicy {
    /// 全量闭集。
    pub const ALL: [SlotPolicy; 2] = [SlotPolicy::Pinned, SlotPolicy::RoundRobin];

    /// 人读名（设置/诊断面文案——与 ALL 同序）。
    pub const fn name(self) -> &'static str {
        match self {
            SlotPolicy::Pinned => "独占",
            SlotPolicy::RoundRobin => "轮转",
        }
    }
}

/// slot → 实例号（确定性；capacity ≤ 0 视为 1 实例——防御）。
/// Pinned 恒 0；RoundRobin 取模轮转，slot=0 提前直落实例 0（与 Pinned 首帧行为对齐；
/// 取模本身 slot=0 亦为 0——提前返回压平嵌套，两路语义逐句等价）。
pub const fn slot_to_instance(slot: u32, capacity: u32, policy: SlotPolicy) -> u32 {
    if slot == 0 {
        return 0;
    }
    let cap = if capacity == 0 { 1 } else { capacity };
    match policy {
        SlotPolicy::Pinned => 0,
        SlotPolicy::RoundRobin => slot % cap,
    }
}

/// 队列实例容量查（assign_instances 内层拆分：表外卡按 1 实例防御——原语义原样）。
fn instance_capacity(devices: &DeviceTable, card: u32, queue: QueueKind) -> u32 {
    devices.device_of(card).map_or(1, |d| d.queue_capacity(queue))
}

/// 实例路由器（对既有映射清单补实例号——映射后处理段）。
/// 逐条就地改写 slot（映射清单序不变）；表外卡容量按 1 防御。
pub fn assign_instances(
    mappings: &mut [QueueMapping],
    devices: &DeviceTable,
    policy: SlotPolicy,
) {
    for m in mappings.iter_mut() {
        let cap = instance_capacity(devices, m.card, m.queue);
        m.slot = slot_to_instance(m.slot, cap, policy);
    }
}

// 十三、负载分布统计（按卡/按队列聚合——热点与均衡度；触发放置再平衡的输入面，全部纯函数零墙钟）。

/// 单卡计数落桶（load_by_card 内层拆分：命中行 +1、无行新开——语义逐句等价）。
fn bump_card_count(out: &mut Vec<(u32, u32)>, card: u32) {
    match out.iter_mut().find(|entry| entry.0 == card) {
        Some(entry) => entry.1 += 1,
        None => out.push((card, 1)),
    }
}

/// 按卡聚合（卡 → 映射数；输出按卡 id 升序——热度面对拍与再平衡输入都依赖此序）。
pub fn load_by_card(mappings: &[QueueMapping]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for m in mappings.iter() {
        bump_card_count(&mut out, m.card);
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// 队列计数落桶（load_by_queue 内层拆分：三行闭集外丢弃——表外序防御原样）。
fn bump_queue_slot(out: &mut [u32; 3], ordinal: usize) {
    if ordinal < 3 {
        out[ordinal] += 1;
    }
}

/// 按队列聚合（队列 → 映射数；三行闭集——零映射也出零行；下标即 QueueKind 稳定序）。
pub fn load_by_queue(mappings: &[QueueMapping]) -> [u32; 3] {
    let mut out = [0u32; 3];
    for m in mappings.iter() {
        bump_queue_slot(&mut out, m.queue.ordinal());
    }
    out
}

/// 单元素极值收敛（balance_ppm 内层拆分：扩上界、收下界——返回新 (max, min)，语义逐句等价）。
const fn absorb_extremes(max: u32, min: u32, v: u32) -> (u32, u32) {
    let max = if v > max { v } else { max };
    let min = if v < min { v } else { min };
    (max, min)
}

/// 均衡度（万分比：1 - (最小/最大)×10000；全零映射 = 10000 满均衡；确定性、无浮点）。
/// 口径：spread = (max-min)×10_000/max（饱和乘法防溢出）；三队列全零满值直返。
pub const fn balance_ppm(by_queue: &[u32; 3]) -> u32 {
    let mut max = 0u32;
    let mut min = u32::MAX;
    let mut i = 0usize;
    while i < 3 {
        let (m1, m2) = absorb_extremes(max, min, by_queue[i]);
        max = m1;
        min = m2;
        i += 1;
    }
    if max == 0 {
        return 10_000;
    }
    let spread = (max - min).saturating_mul(10_000) / max;
    10_000 - spread
}

/// 热点卡落清单（hot_cards 内层拆分：负载超均值 1.5 倍即落——u64 整数口径原样）。
fn push_card_if_hot(out: &mut Vec<u32>, card: u32, count: u32, total: u32, n: u32) {
    let lhs = (count as u64) * (n as u64) * 2;
    let rhs = (total as u64) * 3;
    if lhs > rhs {
        out.push(card);
    }
}

/// 热点卡检测（负载超均值 1.5 倍的卡清单——触发放置再平衡的观测面；输出升序）。
pub fn hot_cards(mappings: &[QueueMapping]) -> Vec<u32> {
    let by_card = load_by_card(mappings);
    let total: u32 = by_card.iter().map(|(_, c)| *c).sum();
    let n = by_card.len() as u32;
    if n == 0 || total == 0 {
        return Vec::new();
    }
    let mut out: Vec<u32> = Vec::new();
    for (card, c) in by_card.iter() {
        push_card_if_hot(&mut out, *card, *c, total, n);
    }
    out
}

/// 守恒断言（编译期钉死：段名与预算数等长；分派策略二闭集）。
const _: () = {
    assert!(MAP_SEGMENTS.len() == MAP_BUDGET_STEPS.len(), "段名与预算数必须等长");
    assert!(SlotPolicy::ALL.len() == 2, "分派策略必须二闭集");
};
