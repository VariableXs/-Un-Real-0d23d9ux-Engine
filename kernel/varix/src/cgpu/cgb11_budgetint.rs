//! cgb11_budgetint — CGPU-F0171 帧图预算集成（CGPU 册 cgb 号段深化）。
//!
//! 锚点（施工书 F0171）：任务图与帧预算仲裁（C 域 F0321+）的集成：图构建时
//! 逐任务预算标签校验（每任务的预估耗时来自 F0004 成本模型）、图级预算汇总
//! （全图预估 vs 帧预算的余量检查）、超预算的图级降级建议（建议跳过哪些
//! 非关键节点——L 档降质映射到图节点子集）。判据：标签校验、余量检查、
//! 降级建议映射、仲裁对接、集成测试。
//!
//! 本文件六段深化：
//! 1. 成本预估（CostModelLite，F0004 对接口径）——按节点类别基价 +
//!    读写集规模线性预估 est_us：基价表写死可对拍（Acquire 200 / Render
//!    800 / Present 300 µs，每读资源 +40，每出边 +10），同输入同输出确定性。
//! 2. 预算标签（BudgetTag，F0005 申请制三要素对接）——est_us + prio
//!    （四档 PriorityLite 对齐 F0005 rank）+ degrade_ladder（逐档递减耗时
//!    承诺）；`ladder_valid` 四律（空=不可降级合法 / 逐档严格递减 /
//!    全部 < est / 末档 >0——0 耗时=静默吞帧等价物禁绝）；TagValidator
//!    逐任务校验出**显性非法清单**（拒绝有清单，不静默）。
//! 3. 图级汇总（BudgetSummary）——Σ节点预估 vs 帧预算余量检查：余量 =
//!    帧预算 − Σest；三态分级 Ok / Tight（余量 < 帧预算 10% 储备线）/
//!    Over（超支）；空图零耗、预算可自定义传入。
//! 4. 图级降级建议（DegradeAdvisor，F0008 降质链消费对接）——超支时
//!    **建议单**（不执行）：排序纪律 Low 先降、同级 est 大者先降；逐节点
//!    沿 ladder 降一档回收预算，余量转正即停；ladder 到底仍超支 → 进
//!    「建议跳过」候选集（关键节点 Critical 豁免——保底不可侵占思想在
//!    图侧的映射）；每步记 LadderStep{node, from_us, to_us} 回收账。
//! 5. 仲裁对接（ArbiterGate，F0005 四态裁决映射）——图级申请单 →
//!    Granted（余量足全额批）/ Degraded（套降级计划批准）/ Queued
//!    （预算无法容纳关键下限）/ Rejected（标签非法，带清单归因）；
//!    判定步数账步数化（遍历+档位计数，红线 200 步——50µs 红线的
//!    图侧换算），reason 恒非空（不允许无因裁）。
//! 6. 集成门面（BudgetIntegrator）——图+预算 → 校验 → 汇总 →（超支 →
//!    降级建议）→ 仲裁单 → IntegrationReport 全链一次跑通。
//!
//! 判据总入口 [`run_f0171_checks`]，五族三十条，全部零 panic 面，
//! 预期常量独立给定不从被测反推。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §1 成本预估 — F0004 对接口径（基价表写死可对拍）
// ---------------------------------------------------------------------------

/// 帧图任务类别（与 cgb10 NodeKind 语义对齐的精简闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    Acquire,
    Render,
    Present,
}

/// 类别基价（µs，写死可对拍）：呈现最低、采集次之、渲染最贵。
pub const BASE_US: [u64; 3] = [200, 800, 300];

/// 每读资源加价（µs）。
pub const PER_READ_US: u64 = 40;
/// 每出边加价（µs）。
pub const PER_EDGE_US: u64 = 10;

/// 类别基价取值。
pub const fn base_us(kind: TaskKind) -> u64 {
    match kind {
        TaskKind::Acquire => BASE_US[0],
        TaskKind::Render => BASE_US[1],
        TaskKind::Present => BASE_US[2],
    }
}

/// 单任务成本预估（F0004 CostModel 对接口径：同输入同输出确定性）。
/// est = 基价 + 读数×PER_READ + 出边数×PER_EDGE。
pub fn estimate_us(kind: TaskKind, reads: usize, out_edges: usize) -> u64 {
    base_us(kind) + reads as u64 * PER_READ_US + out_edges as u64 * PER_EDGE_US
}

// ---------------------------------------------------------------------------
// §2 预算标签 — F0005 申请制三要素对接 + 逐任务校验
// ---------------------------------------------------------------------------

/// 优先级四档（对齐 F0005 Priority rank 语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PriorityLite {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl PriorityLite {
    pub const fn rank(self) -> u8 {
        self as u8
    }
}

/// 预算标签（F0005 Request 对接：预估 + 优先级 + 降级梯）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetTag {
    /// 预估耗时（µs，全档位后仍必须 >0）。
    pub est_us: u64,
    pub prio: PriorityLite,
    /// 可降级档位：逐档递减耗时承诺；空 = 不可降级。
    pub degrade_ladder: Vec<u64>,
}

impl BudgetTag {
    /// 梯子四律（对齐 F0005 ladder_valid）：空合法 / 逐档严格递减 /
    /// 全部 < est / 末档 >0。
    pub fn ladder_valid(&self) -> bool {
        let l = &self.degrade_ladder;
        if l.is_empty() {
            return true;
        }
        let mut prev = self.est_us;
        let mut i = 0;
        while i < l.len() {
            let v = l[i];
            if v == 0 || v >= prev {
                return false;
            }
            prev = v;
            i += 1;
        }
        true
    }
}

/// 类别耗时界（类型界校验用：预估必须落在 [下限, 上界] 内——上界防
/// 标签虚报吃穿预算，下限防 0 耗时静默吞帧）。
pub const KIND_EST_CAP: [u64; 3] = [2_000, 8_000, 3_000];

pub const fn kind_est_cap(kind: TaskKind) -> u64 {
    match kind {
        TaskKind::Acquire => KIND_EST_CAP[0],
        TaskKind::Render => KIND_EST_CAP[1],
        TaskKind::Present => KIND_EST_CAP[2],
    }
}

/// 标签非法归因（显性清单条目——拒绝必须带因）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagFault {
    /// 预估为 0（静默吞帧等价物）。
    ZeroEst,
    /// 预估超类别上界（虚报）。
    EstOverCap,
    /// 梯子非递减 / 档值 ≥ est。
    LadderNotDescending,
    /// 梯子含 0 档。
    LadderZeroRung,
    /// 末档为 0。
    LadderLastZero,
}

impl TagFault {
    pub const fn zh(self) -> &'static str {
        match self {
            TagFault::ZeroEst => "预估为零",
            TagFault::EstOverCap => "预估超类别上界",
            TagFault::LadderNotDescending => "梯子非严格递减或档值不小于预估",
            TagFault::LadderZeroRung => "梯子含零档",
            TagFault::LadderLastZero => "末档为零",
        }
    }
}

/// 校验结论：合法 或 归因清单。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagVerdict {
    Valid,
    Invalid(Vec<TagFault>),
}

/// 逐任务标签校验（图构建期门禁）：全部 fault 收集，不短路——
/// 一次看清所有非法，修单一趟。
pub fn validate_tag(kind: TaskKind, tag: &BudgetTag) -> TagVerdict {
    let mut faults: Vec<TagFault> = Vec::new();
    if tag.est_us == 0 {
        faults.push(TagFault::ZeroEst);
    } else if tag.est_us > kind_est_cap(kind) {
        faults.push(TagFault::EstOverCap);
    }
    // 梯子逐条检查（est=0 时梯子律不可判，跳过避免双重归因）。
    if tag.est_us > 0 && !tag.ladder_valid() {
        // 细分归因：0 档 vs 非递减。
        let l = &tag.degrade_ladder;
        let mut i = 0;
        while i < l.len() {
            if l[i] == 0 {
                faults.push(TagFault::LadderZeroRung);
                break;
            }
            i += 1;
        }
        // 末档为 0 单列（与 ZeroRung 可能并存，但这里 0 档已捕获首个）。
        if let Some(&last) = l.last() {
            if last == 0 && faults.last() != Some(&TagFault::LadderZeroRung) {
                faults.push(TagFault::LadderLastZero);
            }
        }
        // 非递减（不含 0 情形）。
        if faults.last() != Some(&TagFault::LadderZeroRung) {
            let mut prev = tag.est_us;
            let mut i = 0;
            let mut descending = true;
            while i < l.len() {
                if l[i] >= prev {
                    descending = false;
                    break;
                }
                prev = l[i];
                i += 1;
            }
            if !descending {
                faults.push(TagFault::LadderNotDescending);
            }
        }
    }
    if faults.is_empty() {
        TagVerdict::Valid
    } else {
        TagVerdict::Invalid(faults)
    }
}

/// 节点（图构建输入）：id、类别、读资源数、出边数、预算标签。
#[derive(Clone, Debug)]
pub struct BudgetNode {
    pub id: u32,
    pub kind: TaskKind,
    pub reads: usize,
    pub out_edges: usize,
    pub tag: BudgetTag,
}

// ---------------------------------------------------------------------------
// §3 图级汇总 — Σ预估 vs 帧预算余量三态
// ---------------------------------------------------------------------------

/// 帧预算（µs，F0005 同款 12.5ms）。
pub const FRAME_BUDGET_US: u64 = 12_500;
/// 储备线比例（千分比）：余量低于帧预算 10% 记 Tight。
pub const TIGHT_PERMILLE: u64 = 100;

/// 余量三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarginGrade {
    /// 余量充足（≥ 储备线）。
    Ok,
    /// 余量紧张（0 ≤ 余量 < 储备线）。
    Tight,
    /// 超支（Σest > 帧预算）。
    Over,
}

/// 图级预算汇总。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetSummary {
    /// 图级申请单总预估（Σ节点 est）。
    pub total_est_us: u64,
    /// 本帧预算（µs）。
    pub frame_budget_us: u64,
    /// 余量 = 预算 − Σ（可负）。
    pub margin_us: i64,
    pub grade: MarginGrade,
}

impl BudgetSummary {
    /// 汇总：Σest + 余量 + 分级。空图 Σ=0、余量=全额预算、Ok。
    pub fn summarize(nodes: &[BudgetNode], frame_budget_us: u64) -> BudgetSummary {
        let mut total: u64 = 0;
        let mut i = 0;
        while i < nodes.len() {
            total = total.saturating_add(nodes[i].tag.est_us);
            i += 1;
        }
        let margin = frame_budget_us as i64 - total as i64;
        let grade = if margin < 0 {
            MarginGrade::Over
        } else {
            let reserve = frame_budget_us.saturating_mul(TIGHT_PERMILLE) / 1000;
            if (margin as u64) < reserve {
                MarginGrade::Tight
            } else {
                MarginGrade::Ok
            }
        };
        BudgetSummary {
            total_est_us: total,
            frame_budget_us,
            margin_us: margin,
            grade,
        }
    }
}

// ---------------------------------------------------------------------------
// §4 图级降级建议 — L 档降质映射到节点子集（建议单，不执行）
// ---------------------------------------------------------------------------

/// 一步降档账：某节点从 from_us 降到 to_us。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LadderStep {
    pub node_id: u32,
    pub from_us: u64,
    pub to_us: u64,
}

/// 降级建议单（F0008 降质链消费对接——建议不执行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradePlan {
    /// 降档步骤（按应用序）。
    pub steps: Vec<LadderStep>,
    /// 建议跳过候选（Critical 豁免；ladder 到底仍超支才进）。
    pub skip_candidates: Vec<u32>,
    /// 降档回收合计（µs）。
    pub reclaimed_us: u64,
    /// 执行全部 steps 后的余量（可仍为负——负则需 skip/拒绝）。
    pub projected_margin_us: i64,
}

/// 降级建议器：超支（或应请求）时产出建议单。
/// 排序纪律：Low 先降、同级 est 大者先降（大块头回收快）；Critical
/// 豁免（保底不可侵占的图侧映射）。逐节点沿 ladder 降一档，余量转正
/// 即停；ladder 到底仍超支 → 节点进 skip_candidates（仅非 Critical）。
pub fn advise_degrade(nodes: &[BudgetNode], summary: &BudgetSummary) -> DegradePlan {
    let mut plan = DegradePlan {
        steps: Vec::new(),
        skip_candidates: Vec::new(),
        reclaimed_us: 0,
        projected_margin_us: summary.margin_us,
    };
    if summary.margin_us >= 0 {
        return plan; // 不超支不出单。
    }
    // 可降节点排序视图（下标）：先按 prio 升序，再按 est 降序。
    let mut order: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < nodes.len() {
        if nodes[i].tag.prio != PriorityLite::Critical && !nodes[i].tag.degrade_ladder.is_empty() {
            order.push(i);
        }
        i += 1;
    }
    // 插入排序：prio 升序为主键，est 降序为次键。
    let mut a = 1;
    while a < order.len() {
        let key = order[a];
        let mut b = a;
        while b > 0 {
            let pb = nodes[order[b - 1]].tag.prio;
            let pk = nodes[key].tag.prio;
            let eb = nodes[order[b - 1]].tag.est_us;
            let ek = nodes[key].tag.est_us;
            if pk < pb || (pk == pb && ek > eb) {
                order[b] = order[b - 1];
                b -= 1;
            } else {
                break;
            }
        }
        order[b] = key;
        a += 1;
    }
    // 逐节点逐档回收，余量转正即停。
    let mut margin = summary.margin_us;
    let mut oi = 0;
    let mut stop = false;
    while oi < order.len() && !stop {
        let ni = order[oi];
        let node = &nodes[ni];
        let ladder = &node.tag.degrade_ladder;
        // 从低到高找第一个 < 当前 est 的档（当前 est 可能已在某档）。
        let mut cur = node.tag.est_us;
        let mut li = 0;
        while li < ladder.len() {
            let to = ladder[li];
            if to < cur {
                // 降这一档。
                let step = LadderStep {
                    node_id: node.id,
                    from_us: cur,
                    to_us: to,
                };
                let gain = cur - to;
                plan.reclaimed_us += gain;
                margin += gain as i64;
                cur = to;
                plan.steps.push(step);
                if margin >= 0 {
                    stop = true;
                    break;
                }
            }
            li += 1;
        }
        // ladder 到底仍超支 → skip 候选。
        if margin < 0 && li >= ladder.len() {
            plan.skip_candidates.push(node.id);
        }
        oi += 1;
    }
    plan.projected_margin_us = margin;
    plan
}

// ---------------------------------------------------------------------------
// §5 仲裁对接 — F0005 四态裁决映射 + 步数账
// ---------------------------------------------------------------------------

/// 图级裁决四态（对齐 F0005 Verdict 语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphVerdict {
    /// 全额批准（余量足或恰平）。
    Granted,
    /// 降级批准（套降级建议单后余量转正）。
    Degraded,
    /// 排队（降到底+skip 仍超支——本帧容纳不下关键下限）。
    Queued,
    /// 显性拒绝（标签非法，带清单归因——不允许无因拒）。
    Rejected,
}

impl GraphVerdict {
    pub const fn zh(self) -> &'static str {
        match self {
            GraphVerdict::Granted => "批准",
            GraphVerdict::Degraded => "降级批准",
            GraphVerdict::Queued => "排队",
            GraphVerdict::Rejected => "拒绝",
        }
    }
}

/// 判定步数账（F0005 50µs 红线步数化的图侧换算）：校验步 + 汇总步 +
/// 降级步（节点×档位）+ 裁决步。红线 200 步。
pub const VERDICT_STEP_CAP: usize = 200;

/// 仲裁回执。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArbiterReply {
    pub verdict: GraphVerdict,
    /// 实际批准耗时（Granted=Σest / Degraded=降后Σ / Queued·Rejected=0）。
    pub grant_us: u64,
    /// 归因（恒非空）。
    pub reason: &'static str,
    /// 判定步数账。
    pub steps: usize,
    /// Rejected 时的非法任务 id 清单。
    pub rejected_nodes: Vec<u32>,
}

/// 仲裁门：校验→汇总→（降级）→四态映射，一次跑通。
pub fn arbitrate(nodes: &[BudgetNode], frame_budget_us: u64) -> ArbiterReply {
    let mut steps: usize = 0;
    // ① 逐任务校验（步数=节点数）。
    let mut rejected: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < nodes.len() {
        steps += 1;
        if validate_tag(nodes[i].kind, &nodes[i].tag) != TagVerdict::Valid {
            rejected.push(nodes[i].id);
        }
        i += 1;
    }
    if !rejected.is_empty() {
        steps += 1; // 裁决步。
        return ArbiterReply {
            verdict: GraphVerdict::Rejected,
            grant_us: 0,
            reason: "标签非法：图构建期校验未过（见 rejected_nodes 清单）",
            steps,
            rejected_nodes: rejected,
        };
    }
    // ② 图级汇总（步数=节点数）。
    steps += nodes.len();
    let summary = BudgetSummary::summarize(nodes, frame_budget_us);
    match summary.grade {
        MarginGrade::Ok => {
            steps += 1; // 裁决步。
            ArbiterReply {
                verdict: GraphVerdict::Granted,
                grant_us: summary.total_est_us,
                reason: "全图预估在帧预算储备线内，全额批准",
                steps,
                rejected_nodes: Vec::new(),
            }
        }
        MarginGrade::Tight => {
            steps += 1; // 裁决步。
            ArbiterReply {
                verdict: GraphVerdict::Granted,
                grant_us: summary.total_est_us,
                reason: "余量紧张但未超支，全额批准并提示储备线告警",
                steps,
                rejected_nodes: Vec::new(),
            }
        }
        MarginGrade::Over => {
            // ③ 降级建议（步数=节点×档位）。
            let plan = advise_degrade(nodes, &summary);
            steps += plan.steps.len();
            steps += plan.skip_candidates.len();
            steps += 1; // 裁决步。
            if plan.projected_margin_us >= 0 {
                let mut grant: u64 = 0;
                let mut i = 0;
                while i < nodes.len() {
                    grant = grant.saturating_add(nodes[i].tag.est_us);
                    i += 1;
                }
                grant = grant.saturating_sub(plan.reclaimed_us);
                ArbiterReply {
                    verdict: GraphVerdict::Degraded,
                    grant_us: grant,
                    reason: "超支但降级建议单可使余量转正：套梯子降档批准",
                    steps,
                    rejected_nodes: Vec::new(),
                }
            } else {
                ArbiterReply {
                    verdict: GraphVerdict::Queued,
                    grant_us: 0,
                    reason: "降到底加跳过候选仍超支：本帧容纳不下，排队",
                    steps,
                    rejected_nodes: Vec::new(),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// §6 集成门面 — 全链一次跑通出报告
// ---------------------------------------------------------------------------

/// 集成报告（全链产物：校验+汇总+建议+裁决）。
#[derive(Clone, Debug)]
pub struct IntegrationReport {
    /// 非法标签清单（node_id → faults）。
    pub invalid_tags: Vec<(u32, Vec<TagFault>)>,
    pub summary: BudgetSummary,
    pub plan: DegradePlan,
    pub reply: ArbiterReply,
}

/// 集成门面：图+预算 → 报告。
pub struct BudgetIntegrator;

impl BudgetIntegrator {
    pub fn integrate(nodes: &[BudgetNode], frame_budget_us: u64) -> IntegrationReport {
        // 校验清单。
        let mut invalid_tags: Vec<(u32, Vec<TagFault>)> = Vec::new();
        let mut i = 0;
        while i < nodes.len() {
            if let TagVerdict::Invalid(fs) = validate_tag(nodes[i].kind, &nodes[i].tag) {
                invalid_tags.push((nodes[i].id, fs));
            }
            i += 1;
        }
        let summary = BudgetSummary::summarize(nodes, frame_budget_us);
        let plan = advise_degrade(nodes, &summary);
        let reply = arbitrate(nodes, frame_budget_us);
        IntegrationReport {
            invalid_tags,
            summary,
            plan,
            reply,
        }
    }
}

/// 便捷节点构造（标签由预估模型自动生成——集成测试用例建造器）。
pub fn auto_node(id: u32, kind: TaskKind, reads: usize, out_edges: usize, prio: PriorityLite) -> BudgetNode {
    let est = estimate_us(kind, reads, out_edges);
    BudgetNode {
        id,
        kind,
        reads,
        out_edges,
        tag: BudgetTag {
            est_us: est,
            prio,
            degrade_ladder: Vec::new(),
        },
    }
}

/// 便捷节点构造（带梯子）。
pub fn ladder_node(
    id: u32,
    kind: TaskKind,
    reads: usize,
    out_edges: usize,
    prio: PriorityLite,
    ladder: &[u64],
) -> BudgetNode {
    let mut n = auto_node(id, kind, reads, out_edges, prio);
    let mut i = 0;
    while i < ladder.len() {
        n.tag.degrade_ladder.push(ladder[i]);
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// §7 判据 — 五族三十条（零 panic 面，预期独立给定）
// ---------------------------------------------------------------------------

/// F0171 判据总入口。
pub fn run_f0171_checks(checks: &mut CheckSet) {
    // ---- 标签族（标签-01..06）----
    // 标签-01 est>0：零预估非法。
    let t0 = BudgetTag { est_us: 0, prio: PriorityLite::Normal, degrade_ladder: Vec::new() };
    checks.add(
        "标签-01-零预估拒",
        validate_tag(TaskKind::Render, &t0) == TagVerdict::Invalid(vec![TagFault::ZeroEst]),
        "预估为零=静默吞帧等价物应拒",
    );

    // 标签-02 梯子递减：合法梯子 Valid。
    let t1 = BudgetTag {
        est_us: 800,
        prio: PriorityLite::Normal,
        degrade_ladder: vec![600, 400, 200],
    };
    checks.add(
        "标签-02-合法梯子过",
        validate_tag(TaskKind::Render, &t1) == TagVerdict::Valid,
        "递减梯子（600>400>200 全<800 末档>0）应合法",
    );

    // 标签-03 梯子非递减拒。
    let t2 = BudgetTag {
        est_us: 800,
        prio: PriorityLite::Normal,
        degrade_ladder: vec![600, 700],
    };
    checks.add(
        "标签-03-非递减拒",
        validate_tag(TaskKind::Render, &t2) == TagVerdict::Invalid(vec![TagFault::LadderNotDescending]),
        "600 后 700 应判非递减",
    );

    // 标签-04 零档拒。
    let t3 = BudgetTag {
        est_us: 800,
        prio: PriorityLite::Normal,
        degrade_ladder: vec![600, 0],
    };
    checks.add(
        "标签-04-零档拒",
        validate_tag(TaskKind::Render, &t3) == TagVerdict::Invalid(vec![TagFault::LadderZeroRung]),
        "零档=静默吞帧应拒",
    );

    // 标签-05 类型界：Render 上界 8000，9000 虚报拒。
    let t4 = BudgetTag { est_us: 9_000, prio: PriorityLite::High, degrade_ladder: Vec::new() };
    checks.add(
        "标签-05-超上界拒",
        validate_tag(TaskKind::Render, &t4) == TagVerdict::Invalid(vec![TagFault::EstOverCap]),
        "9000 > Render 上界 8000 应拒",
    );

    // 标签-06 空梯子合法（不可降级声明）。
    let t5 = BudgetTag { est_us: 800, prio: PriorityLite::Critical, degrade_ladder: Vec::new() };
    checks.add(
        "标签-06-空梯子合法",
        validate_tag(TaskKind::Render, &t5) == TagVerdict::Valid,
        "空梯=不可降级是合法声明",
    );

    // ---- 余量族（余量-01..05）----
    // 余量-01 Σ 正确：200+800+300=1300。
    let ns1 = vec![
        auto_node(1, TaskKind::Acquire, 0, 1, PriorityLite::Normal),
        auto_node(2, TaskKind::Render, 0, 1, PriorityLite::Normal),
        auto_node(3, TaskKind::Present, 0, 0, PriorityLite::Normal),
    ];
    // 读/边加价：节点1 = 200+1×10=210；节点2 = 800+1×10=810；节点3 = 300。
    let sum1 = BudgetSummary::summarize(&ns1, FRAME_BUDGET_US);
    checks.add(
        "余量-01-求和正确",
        sum1.total_est_us == 210 + 810 + 300,
        "Σ=基价+读×40+边×10 逐项可对拍",
    );

    // 余量-02 余量与 Ok 分级：1300 ≪ 12500。
    checks.add(
        "余量-02-Ok分级",
        sum1.margin_us == 12_500 - 1_320 && sum1.grade == MarginGrade::Ok,
        "余量=12500−1320=11180 应 Ok",
    );

    // 余量-03 Tight 分级：4×Render(20读)=4×1600=6400，预算 7000，
    // 余量 600 < 储备线 700。
    let mut tight_nodes: Vec<BudgetNode> = Vec::new();
    let mut ti = 0;
    while ti < 4 {
        tight_nodes.push(ladder_node(10 + ti, TaskKind::Render, 20, 0, PriorityLite::High, &[1_000]));
        ti += 1;
    }
    let sum2 = BudgetSummary::summarize(&tight_nodes, 7_000);
    checks.add(
        "余量-03-Tight分级",
        sum2.total_est_us == 6_400
            && sum2.margin_us == 600
            && sum2.grade == MarginGrade::Tight,
        "余量 600 < 储备线 700 应 Tight",
    );

    // 余量-04 Over 分级：同图预算 6000 超支。
    let sum3 = BudgetSummary::summarize(&tight_nodes, 6_000);
    checks.add(
        "余量-04-Over分级",
        sum3.total_est_us == 6_400 && sum3.margin_us == -400 && sum3.grade == MarginGrade::Over,
        "Σ=6400 > 6000 应 Over 且余量 −400",
    );

    // 余量-05 空图零耗 Ok。
    let sum4 = BudgetSummary::summarize(&[], FRAME_BUDGET_US);
    checks.add(
        "余量-05-空图零耗",
        sum4.total_est_us == 0 && sum4.margin_us == 12_500 && sum4.grade == MarginGrade::Ok,
        "空图 Σ=0 余量=全额预算",
    );

    // ---- 降级族（降级-01..07）----
    // 场景：三节点超支（Low + Normal 可降，Critical 保底）。各 est=800，
    // Σ=2400，预算 1900 → 超支 500。
    let dn = vec![
        ladder_node(1, TaskKind::Render, 0, 0, PriorityLite::Low, &[600, 400]),
        ladder_node(2, TaskKind::Render, 0, 0, PriorityLite::Normal, &[500, 300]),
        ladder_node(3, TaskKind::Render, 0, 0, PriorityLite::Critical, &[]),
    ];
    let dsum = BudgetSummary::summarize(&dn, 1_900);
    let dplan = advise_degrade(&dn, &dsum);

    // 降级-01 超支才出单：Ok 场景空单。
    let ok_plan = advise_degrade(&ns1, &sum1);
    checks.add(
        "降级-01-不超支空单",
        ok_plan.steps.is_empty() && ok_plan.skip_candidates.is_empty(),
        "余量充足不出建议单",
    );

    // 降级-02 Low 先降：首个 step 是节点 1（Low）。
    let first_low = match dplan.steps.first() {
        Some(s) => s.node_id == 1,
        None => false,
    };
    checks.add(
        "降级-02-Low先降",
        first_low,
        "排序纪律 Low 优先于 Normal（Critical 豁免）",
    );

    // 降级-03 逐档回收：节点 1 先降到 600（第一档），再 400（第二档）。
    let s1_ok = dplan.steps.len() >= 2
        && dplan.steps[0] == LadderStep { node_id: 1, from_us: 800, to_us: 600 }
        && dplan.steps[1] == LadderStep { node_id: 1, from_us: 600, to_us: 400 };
    checks.add(
        "降级-03-逐档回收",
        s1_ok,
        "沿梯子一档一档降：800→600→400",
    );

    // 降级-04 余量转正即停：超支 500，节点1 两档回收 400（余 −100，到底
    // 进 skip），节点2 降一档回收 300（余 +200 ≥0）→ 停；不碰节点3、
    // 不用节点2 第二档。
    let stop_ok = dplan.projected_margin_us == 200
        && dplan.steps.len() == 3
        && dplan.reclaimed_us == 700
        && dplan.skip_candidates == vec![1]
        && !dplan.steps.iter().any(|s| s.node_id == 3);
    checks.add(
        "降级-04-转正即停",
        stop_ok,
        "余量 ≥0 即停、Critical 未触碰、到底者进 skip",
    );

    // 降级-05 到底进 skip：两节点各一档仍超支 → 双双进候选。
    let dn2 = vec![
        ladder_node(1, TaskKind::Render, 0, 0, PriorityLite::Low, &[700]),
        ladder_node(2, TaskKind::Render, 0, 0, PriorityLite::Normal, &[700]),
    ];
    // Σ=1600 预算 1000 超支 600；节点1 降档回收 100（余 −500）到底 → skip；
    // 节点2 降档回收 100（余 −400）到底 → skip。
    let dsum2 = BudgetSummary::summarize(&dn2, 1_000);
    let dplan2 = advise_degrade(&dn2, &dsum2);
    checks.add(
        "降级-05-到底进skip",
        dplan2.skip_candidates.contains(&1)
            && dplan2.skip_candidates.contains(&2)
            && dplan2.projected_margin_us == -400,
        "梯子到底仍超支进跳过候选且余量账精确",
    );

    // 降级-06 Critical 豁免：skip 候选永不包含 Critical。
    let dn3 = vec![
        ladder_node(1, TaskKind::Render, 0, 0, PriorityLite::Critical, &[700]),
        ladder_node(2, TaskKind::Render, 0, 0, PriorityLite::Low, &[600]),
    ];
    // Σ=1600 预算 400 超支 1200；Low 节点2 降档回收 200 仍超支进 skip；
    // Critical 节点1 全程豁免。
    let dsum3 = BudgetSummary::summarize(&dn3, 400);
    let dplan3 = advise_degrade(&dn3, &dsum3);
    checks.add(
        "降级-06-Critical豁免",
        !dplan3.steps.iter().any(|s| s.node_id == 1)
            && !dplan3.skip_candidates.contains(&1)
            && dplan3.skip_candidates.contains(&2),
        "保底不可侵占：Critical 不降不跳",
    );

    // 降级-07 建议不改原图：advise 是纯函数，输入节点 est 不变。
    let before = dn[0].tag.est_us;
    let _ = advise_degrade(&dn, &dsum);
    checks.add(
        "降级-07-建议不执行",
        dn[0].tag.est_us == before,
        "建议单只读：原图标签不被修改（F0008 消费侧执行）",
    );

    // ---- 仲裁族（仲裁-01..06）----
    // 仲裁-01 Granted 映射。
    let r1 = arbitrate(&ns1, FRAME_BUDGET_US);
    checks.add(
        "仲裁-01-Granted映射",
        r1.verdict == GraphVerdict::Granted
            && r1.grant_us == 1_320
            && r1.rejected_nodes.is_empty(),
        "余量充足全额批准",
    );

    // 仲裁-02 Degraded 映射。
    let r2 = arbitrate(&dn, 1_900);
    checks.add(
        "仲裁-02-Degraded映射",
        r2.verdict == GraphVerdict::Degraded && r2.grant_us == 2_400 - 700,
        "套降级计划后批准：grant=Σ−回收=1700",
    );

    // 仲裁-03 Queued 映射。
    let r3 = arbitrate(&dn2, 1_000);
    checks.add(
        "仲裁-03-Queued映射",
        r3.verdict == GraphVerdict::Queued && r3.grant_us == 0,
        "降到底仍超支排队且不批耗时",
    );

    // 仲裁-04 Rejected 映射（非法标签带清单）。
    let bad = vec![
        auto_node(1, TaskKind::Acquire, 0, 0, PriorityLite::Normal),
        BudgetNode {
            id: 2,
            kind: TaskKind::Render,
            reads: 0,
            out_edges: 0,
            tag: BudgetTag { est_us: 0, prio: PriorityLite::Normal, degrade_ladder: Vec::new() },
        },
    ];
    let r4 = arbitrate(&bad, FRAME_BUDGET_US);
    checks.add(
        "仲裁-04-Rejected带清单",
        r4.verdict == GraphVerdict::Rejected
            && r4.rejected_nodes == vec![2]
            && r4.grant_us == 0,
        "标签非法显性拒绝且清单只含非法者",
    );

    // 仲裁-05 步数账有界：50 节点全链步数 ≤ 200。
    let mut big: Vec<BudgetNode> = Vec::new();
    let mut i = 0;
    while i < 50 {
        big.push(auto_node(i as u32, TaskKind::Render, 2, 1, PriorityLite::Normal));
        i += 1;
    }
    let r5 = arbitrate(&big, 100_000);
    checks.add(
        "仲裁-05-步数账",
        r5.steps == 50 + 50 + 1 && r5.steps <= VERDICT_STEP_CAP,
        "步数=校验50+汇总50+裁决1 有账且在红线内",
    );
    // 仲裁-06 reason 恒非空。
    let reason_ok = !r1.reason.is_empty()
        && !r2.reason.is_empty()
        && !r3.reason.is_empty()
        && !r4.reason.is_empty()
        && !r5.reason.is_empty();
    checks.add(
        "仲裁-06-归因恒非空",
        reason_ok,
        "五种裁决的 reason 均非空（不允许无因裁）",
    );

    // ---- 集成族（集成-01..06）----
    // 集成-01 正常路径：报告字段一致。
    let rep1 = BudgetIntegrator::integrate(&ns1, FRAME_BUDGET_US);
    checks.add(
        "集成-01-正常路径",
        rep1.invalid_tags.is_empty()
            && rep1.summary.grade == MarginGrade::Ok
            && rep1.reply.verdict == GraphVerdict::Granted,
        "校验干净+Ok+批准三段一致",
    );

    // 集成-02 超支路径：建议单与裁决联动。
    let rep2 = BudgetIntegrator::integrate(&dn, 1_900);
    checks.add(
        "集成-02-超支路径",
        rep2.summary.grade == MarginGrade::Over
            && rep2.plan.projected_margin_us == 200
            && rep2.reply.verdict == GraphVerdict::Degraded,
        "汇总 Over + 建议 +200 + 降级批准联动",
    );

    // 集成-03 非法路径：清单与拒绝联动。
    let rep3 = BudgetIntegrator::integrate(&bad, FRAME_BUDGET_US);
    checks.add(
        "集成-03-非法路径",
        rep3.invalid_tags.len() == 1
            && rep3.invalid_tags[0].0 == 2
            && rep3.reply.verdict == GraphVerdict::Rejected,
        "校验清单与仲裁拒绝同源同清单",
    );

    // 集成-04 Tight 路径：紧但不降（Tight 全额批）。
    let rep4 = BudgetIntegrator::integrate(&tight_nodes, 7_000);
    checks.add(
        "集成-04-Tight路径",
        rep4.summary.grade == MarginGrade::Tight
            && rep4.plan.steps.is_empty()
            && rep4.reply.verdict == GraphVerdict::Granted,
        "Tight 不触发降级建议（余量非负）",
    );

    // 集成-05 自定义预算：小预算下同一图从 Ok 变 Over。
    let rep5 = BudgetIntegrator::integrate(&ns1, 1_000);
    checks.add(
        "集成-05-自定义预算",
        rep5.summary.frame_budget_us == 1_000
            && rep5.summary.grade == MarginGrade::Over,
        "预算 1000 < Σ1320 应 Over（预算参数生效）",
    );

    // 集成-06 预估模型确定性：同输入两次预估同值（F0004 对接口径）。
    let e1 = estimate_us(TaskKind::Render, 3, 2);
    let e2 = estimate_us(TaskKind::Render, 3, 2);
    checks.add(
        "集成-06-预估确定性",
        e1 == e2 && e1 == 800 + 3 * 40 + 2 * 10,
        "同输入同输出：800+120+20=940 可对拍",
    );
}
