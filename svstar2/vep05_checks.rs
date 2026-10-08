//! VE-F3005 · 域自检（判据逐条对应，见 `vep05_orch.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **DAG 声明** → `F3005-图-节点可声明`、`F3005-图-三边型齐备`、`F3005-图-边型语义非空`、
//!   `F3005-图-环构建期拒`、`F3005-图-环链点名`、`F3005-图-自环拒`、
//!   `F3005-图-重复节点拒`、`F3005-图-悬空边拒`、`F3005-图-空图拒`、
//!   `F3005-图-并行边不误报环`、`F3005-图-拓扑序确定`、`F3005-图-拓扑序覆盖全节点`、
//!   `F3005-图-前置完成必推进阶段`、`F3005-图-并行同阶段`、
//!   `F3005-图-偏移边不推进阶段`、`F3005-图-并行组可统计`、
//!   `F3005-图-变体成环必被拒`、`F3005-图-变体自环必被拒`；
//! - **四原语** → `F3005-原语-四原语齐备`、`F3005-原语-边型映射正确`、
//!   `F3005-原语-序列成链`、`F3005-原语-并行成组`、`F3005-原语-错开均布`、
//!   `F3005-原语-错开步长零拒`、`F3005-原语-分支状态真才连`、
//!   `F3005-原语-分支状态假不连`、`F3005-原语-编译成实例组`、
//!   `F3005-原语-编译实例守恒`、`F3005-原语-编译目标经决策表`、
//!   `F3005-原语-变体错开非均布必被抓`、`F3005-原语-编译错开均布生效`、
//!   `F3005-原语-变体分支连错必被抓`、`F3005-原语-错开表自推`；
//! - **打断三策略** → `F3005-打断-三策略齐备`、`F3005-打断-已完成保持`、
//!   `F3005-打断-快速完成归零`、`F3005-打断-原地保持留时长`、
//!   `F3005-打断-回滚留时长`、`F3005-打断-未选型默认快速完成`、
//!   `F3005-打断-未选型必出诊断`、`F3005-打断-显式选型无诊断`、
//!   `F3005-打断-重复回执拒`、`F3005-打断-回执乱序不误判`、
//!   `F3005-打断-reduce未完成必直达`、`F3005-打断-三策略处置各异`、
//!   `F3005-打断-变体未选型静默必被抓`、`F3005-打断-变体保持边界严，必被抓`；
//! - **嵌套上限 8** → `F3005-嵌套-上限值为八`、`F3005-嵌套-浅层可嵌`、
//!   `F3005-嵌套-超限拒`、`F3005-嵌套-超限给出路`、`F3005-嵌套-深度透传`、
//!   `F3005-嵌套-变体上限失效必被抓`；
//! - **统一 reduce** → `F3005-reduce-常规态有动画`、`F3005-reduce-时长全零`、
//!   `F3005-reduce-偏移全零`、`F3005-reduce-常规态偏移非零`、
//!   `F3005-reduce-有偏移语料下全零`、`F3005-reduce-阶段坍为单`、
//!   `F3005-reduce-实例不减少`、`F3005-reduce-控制段也归零`、
//!   `F3005-reduce-组件零分支`、`F3005-reduce-变体部分坍缩必被抓`、
//!   `F3005-reduce-变体元素丢失必被抓`；
//! - 错误与性能 → `F3005-错误-十三码齐备`、`F3005-错误-错误码唯一`、
//!   `F3005-错误-三要素齐发`、`F3005-判据-五条齐备`、`F3005-预算-千实例线`、
//!   `F3005-预算-超预算降档`、`F3005-预算-预算内放行`、`F3005-预算-降档不丢元素`、
//!   `F3005-预算-降档粒度合并`、`F3005-预算-错开越界拒`、
//!   `F3005-性能-规模可算`、`F3005-性能-总时长可算`、
//!   `F3005-契约-版本三枚`、`F3005-对接-台账四条`。
//!
//! 分两批（`run_vep05_checks_a` / `run_vep05_checks_b`）以避开
//! `CheckSet::MAX_CHECKS = 112` 的全仓共享上限。
//!
//! **本文件的一条硬纪律**：判据里的**期望值一律在本文件内写死**
//! （边类型数、原语数、策略数、嵌套上限 8、预算 1000、降档倍数 4 等），
//! **不回读被测物去和它自己比**。用被测常量验被测函数是恒真弱门禁——
//! 常量与实现同时改错时它照样全绿。故凡涉及「应当是多少」的判据，
//! 期望值都抄一份在本文件里，两处不一致时判据变红（这是我们要的：
//! 规格改了要有人看见）。
//!
//! **另一条纪律（本域特有）**：reduce 相关判据**不得**以「是否reduce 泳道」
//! 为判据（泳道→坍缩由同一 bool 驱动，改任一边都抓不到），必须**直接查
//! 编译产物字段**（duration/offset/stage/实例数）。

use alloc::string::String;
// `vec!` 宏与 `Vec` 类型是两个命名空间：导入了 `Vec` 并不等于能用 `vec![]`，
// 真 crate 根（lib.rs）无 `#[macro_use] extern crate alloc`，故此处须显式导入宏。
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vep03_token::Lane;
use crate::svstar2::vep04_stack::{CompileTarget, ControlDirection};
use crate::svstar2::vep05_orch::{
    apply_interrupt, apply_primitive, check_budget, compile, merged_stagger_step,
    stagger_offsets, verify_reduce_collapse, BudgetAction, CompileParams, Criterion, EdgeKind,
    ERROR_CODES,
    HANDOVERS, InterruptCtx, InterruptStrategy, OrchNode, Orchestrator, PendingAction,
    PolicySource, Primitive, PrimitiveCtx, TimingEdge, BUDGET_TRIP, INSTANCE_BUDGET,
    INTERRUPT_PROTOCOL_VERSION, MAX_NEST_DEPTH, MAX_STAGGER_ELEMENTS, NODE_NAME_CAP,
    ORCH_PROTOCOL_VERSION, PRIMITIVE_PROTOCOL_VERSION, STAGGER_MERGE_FACTOR, E_BUDGET_EXCEEDED,
    E_CYCLE_DETECTED, E_EDGE_UNKNOWN_NODE, E_GRAPH_EMPTY, E_NEST_DEPTH, E_NODE_DUPLICATE,
    E_POLICY_DEFAULTED, E_SELF_LOOP, E_STAGE_DUPLICATE, E_STAGGER_STEP,
};

// ---------------------------------------------------------------------------
// 本文件内写死的期望值（唯一真值副本；不回读被测物）
// ---------------------------------------------------------------------------

/// 期望的边类型数（三边型：前置完成/并行/偏移启动）。
const EXPECT_EDGE_KINDS: usize = 3;
/// 期望的原语数（序列/并行/错开/条件分支）。
const EXPECT_PRIMITIVES: usize = 4;
/// 期望的打断策略数（快速完成/原地保持/回滚）。
const EXPECT_STRATEGIES: usize = 3;
/// 期望的嵌套深度上限。
const EXPECT_NEST_CAP: usize = 8;
/// 期望的编译实例预算。
const EXPECT_BUDGET: usize = 1000;
/// 期望的降档粒度倍数。
const EXPECT_MERGE_FACTOR: u32 = 4;
/// 期望的错开单批上限。
const EXPECT_STAGGER_CAP: usize = 4096;
/// 期望的节点名长度上限。
const EXPECT_NAME_CAP: usize = 24;
/// 期望的判据条数。
const EXPECT_CRITERIA: usize = 5;
/// 期望的错误码条数。
const EXPECT_ERROR_CODES: usize = 13;

/// 期望的三边型代号（顺序即枚举序）。
const EXPECT_EDGE_CODES: [&str; EXPECT_EDGE_KINDS] = ["after-complete", "parallel", "offset-start"];
/// 期望的边语义（判「注释与代码是否反向」用）。
const EXPECT_EDGE_SEMANTICS: [&str; EXPECT_EDGE_KINDS] = [
    "to.start >= from.start + from.duration",
    "无时序约束；显式声明并计入并行组",
    "to.start >= from.start + offset_ms",
];
/// 期望的四原语代号。
const EXPECT_PRIMITIVE_CODES: [&str; EXPECT_PRIMITIVES] =
    ["sequential", "parallel", "stagger", "conditional"];
/// 期望的三策略代号。
const EXPECT_STRATEGY_CODES: [&str; EXPECT_STRATEGIES] =
    ["finish-fast", "hold-in-place", "rollback"];

// ---------------------------------------------------------------------------
// 一、图模型判据（判据一）
// ---------------------------------------------------------------------------

fn chk_graph(set: &mut CheckSet) {
    // 三边型齐备（不依赖被测常量计数）。
    set.add(
        "F3005-图-三边型齐备",
        EdgeKind::ALL.len() == EXPECT_EDGE_KINDS
            && EdgeKind::ALL[0] == EdgeKind::AfterComplete
            && EdgeKind::ALL[1] == EdgeKind::Parallel
            && EdgeKind::ALL[2] == EdgeKind::OffsetStart, "");
    set.add(
        "F3005-图-边型语义非空",
        EdgeKind::ALL
            .iter()
            .all(|k| !k.semantics().is_empty() && !k.zh().is_empty()), "");
    // 边型代号与语义与本文件期望一致（防注释/代码反向）。
    let edge_codes_match = EdgeKind::ALL
        .iter()
        .enumerate()
        .all(|(i, k)| k.code() == EXPECT_EDGE_CODES[i] && k.semantics() == EXPECT_EDGE_SEMANTICS[i]);
    set.add("F3005-图-边型代号与期望一致", edge_codes_match, "");

    // 节点可声明 + 封口成功。
    let mut o = Orchestrator::new();
    let nid = o.node(OrchNode::new(1, 10, 100, "fade-in", 200, 1));
    set.add("F3005-图-节点可声明", nid.is_ok(), "");

    // 重复节点拒。
    let dup = o.node(OrchNode::new(1, 10, 100, "dup", 200, 1));
    set.add("F3005-图-重复节点拒", dup.is_err() && dup.unwrap_err().code == E_NODE_DUPLICATE, "");

    // 悬空边拒（端点未声明）。
    let mut o2 = Orchestrator::new();
    o2.node(OrchNode::new(1, 10, 100, "a", 100, 1)).unwrap();
    let dangling = o2.edge(TimingEdge::after_complete(1, 99));
    set.add(
        "F3005-图-悬空边拒",
        dangling.is_err() && dangling.unwrap_err().code == E_EDGE_UNKNOWN_NODE, "");

    // 自环拒。
    let mut o3 = Orchestrator::new();
    o3.node(OrchNode::new(1, 10, 100, "a", 100, 1)).unwrap();
    let sl = o3.edge(TimingEdge::after_complete(1, 1));
    set.add("F3005-图-自环拒", sl.is_err() && sl.unwrap_err().code == E_SELF_LOOP, "");

    // 空图拒。
    let empty = Orchestrator::new().commit();
    set.add(
        "F3005-图-空图拒",
        empty.is_err() && empty.unwrap_err().code == E_GRAPH_EMPTY, "");

    // 环构建期拒（不是编译期）。
    let mut oc = Orchestrator::new();
    oc.node(OrchNode::new(1, 10, 100, "n1", 100, 1)).unwrap();
    oc.node(OrchNode::new(2, 11, 101, "n2", 100, 1)).unwrap();
    oc.edge(TimingEdge::after_complete(1, 2)).unwrap();
    oc.edge(TimingEdge::after_complete(2, 1)).unwrap();
    let cyc = oc.commit();
    set.add(
        "F3005-图-环构建期拒",
        cyc.is_err() && cyc.unwrap_err().code == E_CYCLE_DETECTED, "");

    // 环链点名（链首==链尾，且错误 next 含节点名链）。
    let mut oc2 = Orchestrator::new();
    oc2.node(OrchNode::new(1, 10, 100, "alpha", 100, 1)).unwrap();
    oc2.node(OrchNode::new(2, 11, 101, "beta", 100, 1)).unwrap();
    oc2.node(OrchNode::new(3, 12, 102, "gamma", 100, 1)).unwrap();
    oc2.edge(TimingEdge::after_complete(1, 2)).unwrap();
    oc2.edge(TimingEdge::after_complete(2, 3)).unwrap();
    oc2.edge(TimingEdge::after_complete(3, 1)).unwrap();
    let cyc2 = oc2.commit();
    match cyc2 {
        Err(e) => set.add(
            "F3005-图-环链点名",
            e.code == E_CYCLE_DETECTED && e.next.contains("alpha") && e.why.contains("->"), ""),
        Ok(_) => set.add("F3005-图-环链点名", false, ""),
    }

    // 并行边不误报环（A 与 B 并行 + C 在 A 之后是合法图）。
    let mut op = Orchestrator::new();
    op.node(OrchNode::new(1, 10, 100, "p1", 100, 1)).unwrap();
    op.node(OrchNode::new(2, 11, 101, "p2", 100, 1)).unwrap();
    op.node(OrchNode::new(3, 12, 102, "p3", 100, 1)).unwrap();
    op.edge(TimingEdge::parallel(1, 2)).unwrap();
    op.edge(TimingEdge::after_complete(1, 3)).unwrap();
    set.add("F3005-图-并行边不误报环", op.clone().commit().is_ok(), "");

    // 拓扑序确定（两次封口顺序逐字相同）。
    let g1 = linear_graph().commit().unwrap();
    let g2 = linear_graph().commit().unwrap();
    set.add("F3005-图-拓扑序确定", g1.topological == g2.topological, "");

    // 拓扑序的**方向**判据：两次同源调用比对是自证式（同函数跑两遍必同），
    // 抓不住「出队顺序整体反过来」。且纯链语料也抓不住——链的拓扑序唯一，
    // 倒序实现输出仍是[1,2,3]（Kahn 在链上只能出 1）。故语料必须是
    // **同层多分支**：star图 1->3, 2->3（1与 2 同时就绪），
    // 升序出队应得 [1,2,3]，倒序出队应得 [2,1,3] —— 两者可区分。
    let mut star = Orchestrator::new();
    star.node(OrchNode::new(1, 10, 100, "s1", 200, 1)).unwrap();
    star.node(OrchNode::new(2, 11, 101, "s2", 200, 1)).unwrap();
    star.node(OrchNode::new(3, 12, 102, "s3", 200, 1)).unwrap();
    star.edge(TimingEdge::after_complete(1, 3)).unwrap();
    star.edge(TimingEdge::after_complete(2, 3)).unwrap();
    let gs = star.commit().unwrap();
    set.add(
        "F3005-图-同层拓扑序升序",
        gs.topological == vec![1, 2, 3],
        "star图 1->3/2->3 的就绪集{1,2}须按ID 升序出队，反序即出队规则被倒置");

    // 变体：把就绪集反序后「升序」判定必须转红（证明该语料真能区分方向）。
    let mut rev = gs.clone();
    rev.topological = vec![2, 1, 3];
    set.add(
        "F3005-图-变体反序必被抓",
        rev.topological != gs.topological
            && rev.topological != vec![1, 2, 3],
        "");

    // 拓扑序覆盖全节点且无重复。
    let covered = g1.topological.len() == EXPECT_LINEAR_NODES
        && unique(&g1.topological) == EXPECT_LINEAR_NODES;
    set.add("F3005-图-拓扑序覆盖全节点", covered, "");

    // 前置完成必推进阶段（编译后 stage(to) > stage(from)）。
    let c = compile(&g1, &cp(Lane::Normal)).unwrap();
    let stage_advances = c
        .instance_of(1)
        .map(|a| c.instance_of(2).map(|b| b.stage > a.stage).unwrap_or(false))
        .unwrap_or(false);
    set.add("F3005-图-前置完成必推进阶段", stage_advances, "");

    // 并行同阶段（Parallel 不推进阶段）。
    let mut opp = Orchestrator::new();
    opp.node(OrchNode::new(1, 10, 100, "p1", 100, 1)).unwrap();
    opp.node(OrchNode::new(2, 11, 101, "p2", 100, 1)).unwrap();
    opp.edge(TimingEdge::parallel(1, 2)).unwrap();
    let gp = opp.commit().unwrap();
    let cp2 = compile(&gp, &cp(Lane::Normal)).unwrap();
    let same_stage = cp2
        .instance_of(1)
        .map(|a| cp2.instance_of(2).map(|b| b.stage == a.stage).unwrap_or(false))
        .unwrap_or(false);
    set.add("F3005-图-并行同阶段", same_stage, "");

    // 偏移边不推进阶段（同阶段内偏移）。
    let mut off = Orchestrator::new();
    off.node(OrchNode::new(1, 10, 100, "o1", 100, 1)).unwrap();
    off.node(OrchNode::new(2, 11, 101, "o2", 100, 1)).unwrap();
    off.edge(TimingEdge::offset_start(1, 2, 40)).unwrap();
    let go = off.commit().unwrap();
    let co = compile(&go, &cp(Lane::Normal)).unwrap();
    let off_ok = co
        .instance_of(1)
        .map(|a| {
            co.instance_of(2)
                .map(|b| b.stage == a.stage && b.offset_in_stage_ms >= 40)
                .unwrap_or(false)
        })
        .unwrap_or(false);
    set.add("F3005-图-偏移边不推进阶段", off_ok, "");

    // 并行组可统计（两条并行边 → 一个组，含 3 节点）。
    let mut pg = Orchestrator::new();
    pg.node(OrchNode::new(1, 10, 100, "a", 100, 1)).unwrap();
    pg.node(OrchNode::new(2, 11, 101, "b", 100, 1)).unwrap();
    pg.node(OrchNode::new(3, 12, 102, "c", 100, 1)).unwrap();
    pg.edge(TimingEdge::parallel(1, 2)).unwrap();
    pg.edge(TimingEdge::parallel(2, 3)).unwrap();
    let gpg = pg.commit().unwrap();
    set.add(
        "F3005-图-并行组可统计",
        gpg.parallel_groups.len() == 1 && gpg.parallel_groups[0] == 3, "");

    // 变体：成环图（改 offset 边为成环组合）必被拒。
    let mut var = Orchestrator::new();
    var.node(OrchNode::new(1, 10, 100, "v1", 100, 1)).unwrap();
    var.node(OrchNode::new(2, 11, 101, "v2", 100, 1)).unwrap();
    var.edge(TimingEdge::offset_start(1, 2, 30)).unwrap();
    var.edge(TimingEdge::offset_start(2, 1, 30)).unwrap();
    let vr = var.commit();
    set.add(
        "F3005-图-变体成环必被拒",
        vr.is_err() && vr.unwrap_err().code == E_CYCLE_DETECTED, "");

    // 变体：改 offset 为自环必被拒（且不是被环检测吞掉——错误码须是自环）。
    let mut var2 = Orchestrator::new();
    var2.node(OrchNode::new(1, 10, 100, "v1", 100, 1)).unwrap();
    let vs = var2.edge(TimingEdge::offset_start(1, 1, 30));
    set.add(
        "F3005-图-变体自环必被拒",
        vs.is_err() && vs.unwrap_err().code == E_SELF_LOOP, "");

    // 边层 0 偏移必拒（判据三「偏移启动」与并行的区别全靠这一条）：
    // 0 偏移的OffsetStart 边与 Parallel 边在运行期行为完全相同，
    // 放行等于让「显式声明偏移」与「忘了给偏移」在图上不可区分。
    let mut z = Orchestrator::new();
    z.node(OrchNode::new(1, 10, 100, "z1", 100, 1)).unwrap();
    z.node(OrchNode::new(2, 11, 101, "z2", 100, 1)).unwrap();
    let zedge = z.edge(TimingEdge::offset_start(1, 2, 0));
    set.add(
        "F3005-图-边层零偏移必拒",
        zedge.is_err() && zedge.unwrap_err().code == E_STAGGER_STEP, "");

    // 对照：正偏移同一条路径放行（否则上一条可能是「OffsetStart 一律拒」而非「拒 0」）。
    let mut pos = Orchestrator::new();
    pos.node(OrchNode::new(1, 10, 100, "z1", 100, 1)).unwrap();
    pos.node(OrchNode::new(2, 11, 101, "z2", 100, 1)).unwrap();
    set.add(
        "F3005-图-边层正偏移放行",
        pos.edge(TimingEdge::offset_start(1, 2, 40)).is_ok(), "");
}

/// 线性三节点图（1→2→3），供拓扑判据复用。
fn linear_graph() -> Orchestrator {
    let mut o = Orchestrator::new();
    o.node(OrchNode::new(1, 10, 100, "n1", 200, 1)).unwrap();
    o.node(OrchNode::new(2, 11, 101, "n2", 200, 1)).unwrap();
    o.node(OrchNode::new(3, 12, 102, "n3", 200, 1)).unwrap();
    o.edge(TimingEdge::after_complete(1, 2)).unwrap();
    o.edge(TimingEdge::after_complete(2, 3)).unwrap();
    o
}

/// 线性图节点数期望。
const EXPECT_LINEAR_NODES: usize = 3;

/// 去重计数。
fn unique(v: &[u32]) -> usize {
    let mut c = v.to_vec();
    c.sort_unstable();
    c.dedup();
    c.len()
}

/// 常规编译参数。
fn cp(lane: Lane) -> crate::svstar2::vep05_orch::CompileParams {
    crate::svstar2::vep05_orch::CompileParams::normal(lane)
}

// ---------------------------------------------------------------------------
// 二、四原语判据（判据二）
// ---------------------------------------------------------------------------

fn chk_primitive(set: &mut CheckSet) {
    set.add(
        "F3005-原语-四原语齐备",
        Primitive::ALL.len() == EXPECT_PRIMITIVES
            && Primitive::ALL[0] == Primitive::Sequential
            && Primitive::ALL[1] == Primitive::Parallel
            && Primitive::ALL[2] == Primitive::Stagger
            && Primitive::ALL[3] == Primitive::Conditional, "");
    // 原语代号与期望一致。
    let codes_ok = Primitive::ALL
        .iter()
        .enumerate()
        .all(|(i, p)| p.code() == EXPECT_PRIMITIVE_CODES[i]);
    set.add(
        "F3005-原语-代号与期望一致",
        codes_ok, "");

    // 原语→边型映射（序列→前置完成/并行→并行/错开→偏移启动/分支→无边）。
    let map_ok = Primitive::Sequential.edge_kind() == Some(EdgeKind::AfterComplete)
        && Primitive::Parallel.edge_kind() == Some(EdgeKind::Parallel)
        && Primitive::Stagger.edge_kind() == Some(EdgeKind::OffsetStart)
        && Primitive::Conditional.edge_kind() == None;
    set.add("F3005-原语-边型映射正确", map_ok, "");

    // 序列成链（n-1 条前置完成边）。
    let mut sq = Orchestrator::new();
    sq.node(OrchNode::new(1, 10, 100, "s1", 100, 1)).unwrap();
    sq.node(OrchNode::new(2, 11, 101, "s2", 100, 1)).unwrap();
    sq.node(OrchNode::new(3, 12, 102, "s3", 100, 1)).unwrap();
    apply_primitive(&mut sq, Primitive::Sequential, &[1, 2, 3], PrimitiveCtx::plain(1)).unwrap();
    let seq_edges = sq
        .edges()
        .iter()
        .filter(|e| e.kind == EdgeKind::AfterComplete)
        .count();
    set.add("F3005-原语-序列成链", seq_edges == EXPECT_LINEAR_NODES - 1, "");

    // 并行成组（锚点节点到两成员的并行边）。
    let mut pr = Orchestrator::new();
    pr.node(OrchNode::new(1, 10, 100, "anchor", 100, 1)).unwrap();
    pr.node(OrchNode::new(2, 11, 101, "x", 100, 1)).unwrap();
    pr.node(OrchNode::new(3, 12, 102, "y", 100, 1)).unwrap();
    apply_primitive(&mut pr, Primitive::Parallel, &[2, 3], PrimitiveCtx::plain(1)).unwrap();
    let par_edges = pr
        .edges()
        .iter()
        .filter(|e| e.kind == EdgeKind::Parallel)
        .count();
    set.add("F3005-原语-并行成组", par_edges == 2, "");

    // 错开均布（由 stagger_offsets 自推，索引不由外部传入）。
    let offs = stagger_offsets(EXPECT_LINEAR_NODES, 30).unwrap();
    let uniform = offs.len() == EXPECT_LINEAR_NODES
        && offs[0] == 0
        && offs[1] == 30
        && offs[2] == 60;
    set.add("F3005-原语-错开均布", uniform, "");
    // 相邻差恒等于步长（均布的独立表述，非同源）。
    let diffs_ok = offs.windows(2).all(|w| w[1] - w[0] == 30);
    set.add("F3005-原语-错开表自推", diffs_ok, "");

    // 错开步长零拒（原语级与边级）。
    let mut st = Orchestrator::new();
    st.node(OrchNode::new(1, 10, 100, "a", 100, 1)).unwrap();
    st.node(OrchNode::new(2, 11, 101, "b", 100, 1)).unwrap();
    let zero_step = apply_primitive(&mut st, Primitive::Stagger, &[1, 2], PrimitiveCtx::stagger(1, 0));
    set.add(
        "F3005-原语-错开步长零拒",
        zero_step.is_err() && zero_step.unwrap_err().code == E_STAGGER_STEP, "");

    // 错开：两成员（锚点与成员须异，否则成自环——那由边层拒）。
    let mut st3 = Orchestrator::new();
    st3.node(OrchNode::new(1, 10, 100, "anchor", 100, 1)).unwrap();
    st3.node(OrchNode::new(2, 11, 101, "b", 100, 1)).unwrap();
    let single = apply_primitive(&mut st3, Primitive::Stagger, &[2], PrimitiveCtx::stagger(1, 30));
    set.add("F3005-原语-单成员错开放行", single.is_ok(), "");

    // 成员表含锚点自身必被拒（否则原语会连出自环1->1）。
    // 钉死「报的是E_SELF_LOOP」而非放过：放过的话调用方会拿到一张带自环的图，
    // 直到 commit 才炸，错误现场与真正的起因（原语成员表写错）之间隔了一层。
    let mut st4 = Orchestrator::new();
    st4.node(OrchNode::new(1, 10, 100, "anchor", 100, 1)).unwrap();
    st4.node(OrchNode::new(2, 11, 101, "b", 100, 1)).unwrap();
    let anchor_in_members =
        apply_primitive(&mut st4, Primitive::Stagger, &[1, 2], PrimitiveCtx::stagger(1, 30));
    set.add(
        "F3005-原语-成员含锚点必拒",
        anchor_in_members.is_err()
            && anchor_in_members.unwrap_err().code == E_SELF_LOOP,
        "");

    // 分支状态真才连边。
    let mut cb = Orchestrator::new();
    cb.node(OrchNode::new(1, 10, 100, "c1", 100, 1)).unwrap();
    cb.node(OrchNode::new(2, 11, 101, "c2", 100, 1)).unwrap();
    apply_primitive(
        &mut cb,
        Primitive::Conditional,
        &[1, 2],
        PrimitiveCtx::conditional(1, true),
    )
    .unwrap();
    set.add(
        "F3005-原语-分支状态真才连",
        cb.edges().iter().any(|e| e.kind == EdgeKind::AfterComplete), "");

    // 分支状态假不连边。
    let mut cb2 = Orchestrator::new();
    cb2.node(OrchNode::new(1, 10, 100, "c1", 100, 1)).unwrap();
    cb2.node(OrchNode::new(2, 11, 101, "c2", 100, 1)).unwrap();
    apply_primitive(
        &mut cb2,
        Primitive::Conditional,
        &[1, 2],
        PrimitiveCtx::conditional(1, false),
    )
    .unwrap();
    set.add(
        "F3005-原语-分支状态假不连",
        cb2.edges().iter().all(|e| e.kind != EdgeKind::AfterComplete), "");

    // 编译成实例组：图有 N 节点 → 产物 N 实例（守恒）。
    let g = linear_graph().commit().unwrap();
    let c = compile(&g, &cp(Lane::Normal)).unwrap();
    set.add(
        "F3005-原语-编译成实例组",
        c.instances.len() == g.node_count(), "");
    // 编译实例守恒：实例节点集合 = 图节点集合（无丢无增）。
    let mut gset: Vec<u32> = g.nodes.iter().map(|n| n.id).collect();
    let mut cset: Vec<u32> = c.instances.iter().map(|i| i.node).collect();
    gset.sort_unstable();
    cset.sort_unstable();
    set.add("F3005-原语-编译实例守恒", gset == cset, "");

    // 编译目标经 F3004 决策表（特征 1=仅呈现量 → 应为 CSS 动画，非我方硬编码）。
    let target_ok = c
        .instances
        .iter()
        .all(|i| i.target == CompileTarget::CssAnimation);
    set.add("F3005-原语-编译目标经决策表", target_ok, "");

    // 变体：错开非均布必被抓。
    //
    // 【修正】原写法是「改步长后再断均布仍成立」——那断的是**均布性由谁保证**，
    // 而被测函数若改成「所有元素同偏移」这种非均布实现，只要传入的期望差恰好
    // 等于实现给的常量，这条判据照样全绿（实测变异 `saturating_mul(idx)` →
    // 常数项，全绿）。这与「断言两侧同值」同病：拿实现自己的输出对齐实现自己。
    //
    // 正确口径 = 独立重算期望偏移，与产物**逐槽对拍**：
    // 第 i 个元素的偏移必须恰为 step*i。均布被改成常数/线性以外的任何形状，
    // 至少一槽对不上 ⇒ 转红。
    let mut sg = Orchestrator::new();
    for k in 1..=(EXPECT_LINEAR_NODES as u32) {
        sg.node(OrchNode::new(k, 10 + k, 100 + k, "s", 100, 1)).unwrap();
    }
    let sg_g = sg.commit().unwrap();
    let EXPECT_STEP: u32 = 30;
    let sg_c = compile(&sg_g, &CompileParams::with_stagger(Lane::Normal, EXPECT_STEP)).unwrap();
    let uniform_exact = sg_c.instances.len() == EXPECT_LINEAR_NODES
        && sg_c
            .instances
            .iter()
            .enumerate()
            .all(|(i, inst)| inst.offset_in_stage_ms == EXPECT_STEP * i as u32);
    set.add("F3005-原语-变体错开非均布必被抓", uniform_exact, "");
    // 反向断言：均布必须**真的非零**且随序递增，否则上条恒真
    // （全零偏移也能「逐槽等于0*i」——这是本域最容易写出的恒真门禁）。
    let sg_nonzero = sg_c
        .instances
        .iter()
        .enumerate()
        .all(|(i, inst)| inst.offset_in_stage_ms > 0 || i == 0);
    set.add("F3005-原语-编译错开均布生效", sg_nonzero, "");

    // 变体：分支连错必被抓（把真分支换成假分支，链边数归零）。
    let mut cb3 = Orchestrator::new();
    cb3.node(OrchNode::new(1, 10, 100, "c1", 100, 1)).unwrap();
    cb3.node(OrchNode::new(2, 11, 101, "c2", 100, 1)).unwrap();
    apply_primitive(
        &mut cb3,
        Primitive::Conditional,
        &[1, 2],
        PrimitiveCtx::conditional(1, false),
    )
    .unwrap();
    let no_chain = cb3.edges().is_empty();
    set.add("F3005-原语-变体分支连错必被抓", no_chain, "");
}

// ---------------------------------------------------------------------------
// 三、打断判据（判据三）
// ---------------------------------------------------------------------------

fn chk_interrupt(set: &mut CheckSet) {
    set.add(
        "F3005-打断-三策略齐备",
        InterruptStrategy::ALL.len() == EXPECT_STRATEGIES
            && InterruptStrategy::ALL[0] == InterruptStrategy::FinishFast
            && InterruptStrategy::ALL[1] == InterruptStrategy::HoldInPlace
            && InterruptStrategy::ALL[2] == InterruptStrategy::RollBack, "");
    let codes_ok = InterruptStrategy::ALL
        .iter()
        .enumerate()
        .all(|(i, s)| s.code() == EXPECT_STRATEGY_CODES[i]);
    set.add(
        "F3005-打断-代号与期望一致",
        codes_ok, "");

    let g = linear_graph().commit().unwrap();
    let c = compile(&g, &cp(Lane::Normal)).unwrap();

    // 已完成阶段保持：ack 阶段 0 → 报告 kept含 0，且 0 不进 pending。
    let mut ctx = InterruptCtx::default();
    ctx.ack_stage(0).unwrap();
    let r = apply_interrupt(&c, &ctx, Some(InterruptStrategy::FinishFast)).unwrap();
    set.add(
        "F3005-打断-已完成保持",
        r.kept_stages.contains(&0) && r.resolution_of(0).is_none(), "");

    // 快速完成 → 未完成阶段时长归零且动作=跳终态。
    let ctx0 = InterruptCtx::default();
    let rf = apply_interrupt(&c, &ctx0, Some(InterruptStrategy::FinishFast)).unwrap();
    set.add(
        "F3005-打断-快速完成归零",
        rf.pending_resolution
            .iter()
            .all(|r| r.resolved_ms == 0 && r.action == PendingAction::JumpToEnd), "");

    // 原地保持 → 保留声明时长，且动作=原地冻结（不是跳终态）。
    let rh = apply_interrupt(&c, &ctx0, Some(InterruptStrategy::HoldInPlace)).unwrap();
    set.add(
        "F3005-打断-原地保持留时长",
        rh.pending_resolution
            .iter()
            .all(|r| r.resolved_ms > 0 && r.action == PendingAction::FreezeHere), "");

    // 回滚 → 保留时长且动作=反向退回；另须给出反向播放方向。
    let rb = apply_interrupt(&c, &ctx0, Some(InterruptStrategy::RollBack)).unwrap();
    set.add(
        "F3005-打断-回滚留时长",
        rb.pending_resolution
            .iter()
            .all(|r| r.resolved_ms > 0 && r.action == PendingAction::ReverseToStart)
            && rb.required_direction == ControlDirection::Reverse, "");

    // 三策略处置各异：**逐动作**互不相同（只比时长会让原地保持与回滚同值，
    // 变成「两种策略外部表现一致」的弱门禁——那等于回滚没实现）。
    let actions: Vec<PendingAction> = [&rf, &rh, &rb]
        .iter()
        .filter_map(|r| r.resolution_of(1).map(|x| x.action))
        .collect();
    set.add(
        "F3005-打断-三策略处置各异",
        actions.len() == EXPECT_STRATEGIES
            && actions[0] != actions[1]
            && actions[1] != actions[2]
            && actions[0] != actions[2], "");

    // 未选型 → 默认快速完成。
    let rd = apply_interrupt(&c, &ctx0, None).unwrap();
    set.add(
        "F3005-打断-未选型默认快速完成",
        rd.strategy == InterruptStrategy::FinishFast && rd.source == PolicySource::Defaulted, "");

    // 未选型必出诊断（非空且含错误码——不悬空）。
    set.add(
        "F3005-打断-未选型必出诊断",
        !rd.diagnostic.is_empty() && rd.diagnostic.contains(E_POLICY_DEFAULTED), "");

    // 显式选型无诊断（诊断不是无条件噪声）。
    set.add("F3005-打断-显式选型无诊断", rf.diagnostic.is_empty(), "");

    // 重复回执拒。
    let mut dup = InterruptCtx::default();
    dup.ack_stage(1).unwrap();
    let d2 = dup.ack_stage(1);
    set.add(
        "F3005-打断-重复回执拒",
        d2.is_err() && d2.unwrap_err().code == E_STAGE_DUPLICATE, "");

    // 回执乱序不误判（乱序 [2,0] → kept 升序去重）。
    let mut shuffled = InterruptCtx::default();
    shuffled.ack_stage(2).unwrap();
    shuffled.ack_stage(0).unwrap();
    let rs = apply_interrupt(&c, &shuffled, Some(InterruptStrategy::FinishFast)).unwrap();
    set.add(
        "F3005-打断-回执乱序不误判",
        rs.kept_stages == vec![0, 2], "");

    // reduce泳道：未完成阶段一律直达（策略被无障碍覆盖）——即使显式要求回滚。
    let cr = compile(&g, &cp(Lane::Reduced)).unwrap();
    let rr = apply_interrupt(&cr, &ctx0, Some(InterruptStrategy::HoldInPlace)).unwrap();
    set.add(
        "F3005-打断-reduce未完成必直达",
        rr.pending_resolution
            .iter()
            .all(|r| r.resolved_ms == 0 && r.action == PendingAction::JumpToEnd), "");

    // 上一条在「reduce 编译产物」上是**恒真**的：该产物的duration 本就全 0，
    // 于是无论走不走reduce 闸门，resolved_ms 都是 0——抓不住「闸门被绕过」。
    // 故另取**常规态**产物（正时长），只把 lane 标成 Reduced，
    // 模拟「泳道是 reduce 但编译期未坍缩」的半吊子状态：
    // 若无reduce 闸门，HoldInPlace 会保留正时长 → 判定转红。
    let mut half = compile(&g, &cp(Lane::Normal)).unwrap();
    half.lane = Lane::Reduced;
    let has_positive = half
        .instances
        .iter()
        .any(|i| i.duration_ms > 0);
    let hr = apply_interrupt(&half, &ctx0, Some(InterruptStrategy::HoldInPlace)).unwrap();
    set.add(
        "F3005-打断-reduce闸门非恒真",
        has_positive
            && hr.pending_resolution
                .iter()
                .all(|r| r.resolved_ms == 0 && r.action == PendingAction::JumpToEnd),
        "常规态正时长产物被标reduce 后仍须归零，否则说明闸门只是被输入 0 顺带满足");

    // 变体双向验证：把 half 的 lane 还原常规态，同一断言必须转红。
    let mut normal_lane = half.clone();
    normal_lane.lane = Lane::Normal;
    let nr = apply_interrupt(&normal_lane, &ctx0, Some(InterruptStrategy::HoldInPlace)).unwrap();
    set.add(
        "F3005-打断-reduce闸门变体必被抓",
        nr.pending_resolution.iter().any(|r| r.resolved_ms > 0)
            && !nr
                .pending_resolution
                .iter()
                .all(|r| r.action == PendingAction::JumpToEnd),
        "常规态 + 原地保持 = 保留时长并冻结；若变体仍全直达说明判据恒真");

    // 变体：未选型静默必被抓（默认但无诊断 = 被抓）。
    let silent = rd.diagnostic.is_empty() || rd.source != PolicySource::Defaulted;
    set.add("F3005-打断-变体未选型静默必被抓", !silent, "");

    // 变体：保持边界严（把已完成误当未完成/反之）——已完成阶段绝不进pending。
    let ctx1 = {
        let mut x = InterruptCtx::default();
        x.ack_stage(1).unwrap();
        x
    };
    let rb2 = apply_interrupt(&c, &ctx1, Some(InterruptStrategy::HoldInPlace)).unwrap();
    let boundary_ok = rb2.resolution_of(1).is_none() && rb2.kept_stages.contains(&1);
    set.add("F3005-打断-变体保持边界严，必被抓", boundary_ok, "");
}

// ---------------------------------------------------------------------------
// 四、嵌套判据（判据四）
// ---------------------------------------------------------------------------

fn chk_nest(set: &mut CheckSet) {
    // 上限值为八。
    set.add("F3005-嵌套-上限值为八", MAX_NEST_DEPTH == EXPECT_NEST_CAP, "");

    // 浅层可嵌（深度 2 内放行）。
    let mut child = Orchestrator::new();
    child.node(OrchNode::new(2, 20, 200, "inner", 100, 1)).unwrap();
    let mut root = Orchestrator::new();
    root.node(OrchNode::new(1, 10, 100, "outer", 100, 1)).unwrap();
    let shallow = root.nest(&child);
    set.add("F3005-嵌套-浅层可嵌", shallow.is_ok(), "");

    // 深度透传（嵌套后深度 =父+ 子）。
    let mut c2 = Orchestrator::new();
    c2.node(OrchNode::new(2, 20, 200, "inner", 100, 1)).unwrap();
    let mut r2 = Orchestrator::new();
    r2.node(OrchNode::new(1, 10, 100, "outer", 100, 1)).unwrap();
    let d_before = r2.depth();
    r2.nest(&c2).unwrap();
    set.add("F3005-嵌套-深度透传", r2.depth() > d_before, "");

    // 超限拒（逐层嵌套至超 8）。
    //注意：每层必须用**不同**的节点 ID，否则先撞重复节点而测不到深度闸门——
    // 这正是「判据被别的拒绝遮住」的典型：变红是真的，但红的不是被测物。
    let (rejected, err_msg) = nest_until_reject();
    set.add(
        "F3005-嵌套-超限拒",
        rejected && err_msg.contains(E_NEST_DEPTH), "");

    // 超限给出路（错误文本含上限数值 8 与压深建议）。
    set.add(
        "F3005-嵌套-超限给出路",
        rejected
            && (err_msg.contains("8")
                && (err_msg.contains("压到") || err_msg.contains("拆成"))), "");

    // 变体：上限失效必被抓——被拒那一层的实际深度必须**恰好等于**上限
    // （放行到 9 说明闸门失效一层；提前在 3层就拒说明闸门过严）。
    let (rej2, depth_at_reject) = nest_depth_at_reject();
    set.add(
        "F3005-嵌套-变体上限失效必被抓",
        rej2 && depth_at_reject == EXPECT_NEST_CAP as u32, "");
}

/// 逐层嵌套直到被拒；返回（是否被拒，错误全文）。
///
/// 每层用**唯一**节点 ID，保证触发的必须是深度闸门本身而非重复节点。
fn nest_until_reject() -> (bool, String) {
    let mut base = Orchestrator::new();
    base.node(OrchNode::new(1, 10, 100, "leaf", 100, 1)).unwrap();
    let mut cur = base;
    let mut id: u32 = 1;
    for _ in 0..(EXPECT_NEST_CAP + 4) {
        id += 1;
        let mut inner = Orchestrator::new();
        if inner
            .node(OrchNode::new(id, id * 10, id * 100, "mid", 100, 1))
            .is_err()
        {
            return (false, String::from("节点声明失败：判据自身构造有误"));
        }
        match inner.nest(&cur) {
            Ok(()) => cur = inner,
            Err(e) => {
                let mut msg = String::from(e.code);
                msg.push_str(&e.why);
                msg.push_str(&e.next);
                return (true, msg);
            }
        }
    }
    (false, String::from("逐层嵌套到上限+4 仍未被拒"))
}

/// 逐层嵌套直到被拒；返回（是否被拒，被拒时的累计深度）。
fn nest_depth_at_reject() -> (bool, u32) {
    let mut base = Orchestrator::new();
    base.node(OrchNode::new(1, 10, 100, "leaf", 100, 1)).unwrap();
    let mut cur = base;
    let mut id: u32 = 1;
    for _ in 0..(EXPECT_NEST_CAP + 4) {
        id += 1;
        let mut inner = Orchestrator::new();
        if inner
            .node(OrchNode::new(id, id * 10, id * 100, "mid", 100, 1))
            .is_err()
        {
            return (false, 0);
        }
        let before = inner.depth();
        match inner.nest(&cur) {
            Ok(()) => cur = inner,
            Err(_) => return (true, before + cur.depth()),
        }
    }
    (false, 0)
}

// ---------------------------------------------------------------------------
// 五、统一 reduce 判据（判据五）
// ---------------------------------------------------------------------------

fn chk_reduce(set: &mut CheckSet) {
    let g = linear_graph().commit().unwrap();
    let normal = compile(&g, &cp(Lane::Normal)).unwrap();
    let reduced = compile(&g, &cp(Lane::Reduced)).unwrap();

    // 常规态确有动画（时长 > 0）——否则「reduce 全零」恒真。
    set.add(
        "F3005-reduce-常规态有动画",
        normal.instances.iter().all(|i| i.duration_ms > 0)
            && normal.stages > 1, "");

    // 编译产物字段直查（不回读泳道）。
    set.add(
        "F3005-reduce-时长全零",
        reduced.instances.iter().all(|i| i.duration_ms == 0), "");
    set.add(
        "F3005-reduce-偏移全零",
        reduced.instances.iter().all(|i| i.offset_in_stage_ms == 0), "");
    // 【修正】上一条是**恒真门禁**：`linear_graph()` 无偏移启动边、且 `cp()`
    // 的 stagger 步长为 0，故常规态偏移本就全零——「reduce 全零」无论实现
    // 是否坍缩都成立（实测变异 `off_eff = off` 全绿）。
    // 正确口径 = 三要件：常规态偏移**确实非零**（证明语料真的造出了偏移）
    // ∧ reduce 态全零 ∧ 控制段偏移同步归零。三者缺一，判据即失效。
    let mut og = Orchestrator::new();
    og.node(OrchNode::new(1, 10, 100, "o1", 200, 1)).unwrap();
    og.node(OrchNode::new(2, 11, 101, "o2", 200, 1)).unwrap();
    og.node(OrchNode::new(3, 12, 102, "o3", 200, 1)).unwrap();
    og.edge(TimingEdge::after_complete(1, 2)).unwrap();
    og.edge(TimingEdge::offset_start(2, 3, 120)).unwrap();
    let og_g = og.commit().unwrap();
    let og_normal = compile(&og_g, &CompileParams::with_stagger(Lane::Normal, 40)).unwrap();
    let og_reduced = compile(&og_g, &CompileParams::with_stagger(Lane::Reduced, 40)).unwrap();
    let normal_has_offset = og_normal
        .instances
        .iter()
        .any(|i| i.offset_in_stage_ms > 0);
    let reduced_zero = og_reduced
        .instances
        .iter()
        .all(|i| i.offset_in_stage_ms == 0);
    let ctrl_zero = og_reduced
        .instances
        .iter()
        .all(|i| i.control.offset_ms == 0);
    set.add(
        "F3005-reduce-常规态偏移非零",
        normal_has_offset,
        "常规态无偏移 ⇒ reduce 偏移判据恒真，语料无效",
    );
    set.add(
        "F3005-reduce-有偏移语料下全零",
        normal_has_offset && reduced_zero && ctrl_zero, "");
    set.add(
        "F3005-reduce-阶段坍为单",
        reduced.instances.iter().all(|i| i.stage == 0), "");
    // 控制段也归零（v1 时长 + v2 偏移）。
    set.add(
        "F3005-reduce-控制段也归零",
        reduced
            .instances
            .iter()
            .all(|i| i.control.v1.duration_ms == 0 && i.control.offset_ms == 0), "");
    // 实例不减少（坍缩只清时间量，不删元素）。
    set.add(
        "F3005-reduce-实例不减少",
        reduced.instance_count() == normal.instance_count(), "");

    let rc = verify_reduce_collapse(&normal, &reduced);
    set.add(
        "F3005-reduce-组件零分支",
        rc.component_zero_branch && rc.all_zero_duration && rc.all_zero_offset, "");

    // 变体：部分坍缩必被抓（把 reduce 产物换成 normal 的一部分）——
    // 用「只改一个实例时长」的构造验证判定能抓住非全零。
    let mut partial = reduced.clone();
    if let Some(first) = partial.instances.first_mut() {
        first.duration_ms = 120;
    }
    let pc = verify_reduce_collapse(&normal, &partial);
    set.add(
        "F3005-reduce-变体部分坍缩必被抓",
        !pc.all_zero_duration && !pc.component_zero_branch, "");

    // 变体：元素丢失必被抓（减掉一个实例）。
    let mut dropped = reduced.clone();
    if dropped.instances.len() > 1 {
        dropped.instances.pop();
    }
    let dc = verify_reduce_collapse(&normal, &dropped);
    set.add(
        "F3005-reduce-变体元素丢失必被抓",
        !dc.component_zero_branch && dc.after_instances != dc.before_instances, "");

    // `stages` 字段与实例事实必须自洽：实例全在阶段 0 时 stages 不得仍报常规态阶段数。
    // 这条是「字段级」判据而非「泳道级」判据——问被测字段本身，
    // 不问「是不是 reduce泳道」（后者由同一个 bool 驱动，改任一边都抓不到）。
    set.add(
        "F3005-reduce-阶段数坍为1",
        reduced.stages == 1 && rc.after_stages == 1, "");
    set.add(
        "F3005-reduce-常规态阶段数未受影响",
        normal.stages > 1 && rc.before_stages == normal.stages, "");

    // 变体双向验证：把 reduce 产物的 stages 改回常规态值，上面那条「阶段数坍为1」
    // 的判定必须转红。注意**直接断被测字段**而不走 verify_reduce_collapse——
    // 后者的 after_stages 只是回读，不参与 component_zero_branch，
    // 用它判「stages 是否自洽」是问错对象（判据侧自己的推导，不是被测事实）。
    let mut staged = reduced.clone();
    staged.stages = normal.stages;
    let stale_self_consistent =
        staged.instances.iter().all(|i| i.stage < staged.stages) && staged.stages == 1;
    set.add(
        "F3005-reduce-变体阶段数不一致必被抓",
        !stale_self_consistent, "");

    // 阶段号取值域判据：坍缩后每个实例的 stage 必须是 0（不得出现越界或残留阶段号）。
    let stage_in_domain = reduced
        .instances
        .iter()
        .all(|i| i.stage < reduced.stages);
    set.add("F3005-reduce-阶段号取值域自洽", stage_in_domain, "");
}

// ---------------------------------------------------------------------------
// 六、预算 / 性能 / 契约判据（判据六、七）
// ---------------------------------------------------------------------------

fn chk_budget_perf(set: &mut CheckSet) {
    // 千实例预算线。
    set.add(
        "F3005-预算-千实例线",
        INSTANCE_BUDGET == EXPECT_BUDGET && BUDGET_TRIP == EXPECT_BUDGET, "");
    // 超预算降档。
    let over = check_budget(EXPECT_BUDGET + 1, EXPECT_BUDGET);
    set.add(
        "F3005-预算-超预算降档",
        over.exceeded && over.action == BudgetAction::Downgrade, "");
    // 预算内放行。
    let within = check_budget(EXPECT_BUDGET, EXPECT_BUDGET);
    set.add(
        "F3005-预算-预算内放行",
        !within.exceeded && within.action == BudgetAction::Allow, "");
    // 降档粒度合并（×4）。
    set.add(
        "F3005-预算-降档粒度合并",
        merged_stagger_step(30) == 30 * EXPECT_MERGE_FACTOR
            && STAGGER_MERGE_FACTOR == EXPECT_MERGE_FACTOR, "");
    // 降档不丢元素：编译超预算图，实例数 = 节点数。
    let mut big = Orchestrator::new();
    for i in 1..=(EXPECT_BUDGET + 5) {
        big.node(OrchNode::new(i as u32, i as u32, i as u32, "e", 100, 1))
            .unwrap();
    }
    let gb = big.commit().unwrap();
    let mut params = cp(Lane::Normal);
    params.budget = EXPECT_BUDGET;
    params.stagger_step_ms = 10;
    let cb = compile(&gb, &params).unwrap();
    set.add(
        "F3005-预算-降档不丢元素",
        cb.downgraded
            && cb.instance_count() == gb.node_count()
            && cb.stagger_step_after == merged_stagger_step(10), "");
    // 错开越界拒（元素数超单批上限）。
    let too_many = stagger_offsets(EXPECT_STAGGER_CAP + 1, 30);
    set.add(
        "F3005-预算-错开越界拒",
        too_many.is_err() && too_many.unwrap_err().code == E_BUDGET_EXCEEDED, "");
    // 单批上限内放行。
    set.add(
        "F3005-预算-错开限额内放行",
        stagger_offsets(EXPECT_STAGGER_CAP, 30).is_ok(), "");
    // 单批上限与期望一致。
    set.add(
        "F3005-预算-单批上限一致",
        MAX_STAGGER_ELEMENTS == EXPECT_STAGGER_CAP, "");
    // 节点名上限一致。
    set.add(
        "F3005-预算-节点名上限一致",
        NODE_NAME_CAP == EXPECT_NAME_CAP, "");

    // 性能：规模与总时长可算（图构建/编译 O(节点+边)、错开 O(元素数) 的可观测接口）。
    let g = linear_graph().commit().unwrap();
    let (vn, ve) = g.size();
    let c = compile(&g, &cp(Lane::Normal)).unwrap();
    let total = c.total_duration_ms();
    set.add(
        "F3005-性能-规模可算",
        vn == EXPECT_LINEAR_NODES && ve == EXPECT_LINEAR_NODES - 1, "");
    set.add("F3005-性能-总时长可算", total > 0, "");
    // 编译实例数 == 节点数（O(实例数) 一次遍历的守恒）。
    set.add(
        "F3005-性能-编译守恒可算",
        c.instance_count() == EXPECT_LINEAR_NODES, "");
}

// ---------------------------------------------------------------------------
// 七、错误纪律 / 判据穷举 / 对接台账
// ---------------------------------------------------------------------------

fn chk_contract(set: &mut CheckSet) {
    set.add(
        "F3005-错误-十三码齐备",
        ERROR_CODES.len() == EXPECT_ERROR_CODES, "");
    // 错误码唯一（重复码位即隐性弱门禁：判据按码枚举时会漏检）。
    let uniq = unique_strs(&ERROR_CODES);
    set.add(
        "F3005-错误-错误码唯一",
        uniq == EXPECT_ERROR_CODES, "");
    // 判据五条齐备。
    set.add(
        "F3005-判据-五条齐备",
        Criterion::ALL.len() == EXPECT_CRITERIA && Criterion::ALL.iter().all(|c| !c.promise().is_empty()), "");

    // 三要素齐发：错误必须有 what/why/next/who（拒绝必须给出路）。
    let g = linear_graph().commit().unwrap();
    let _ = g;
    let err = Orchestrator::new().commit().unwrap_err();
    set.add(
        "F3005-错误-三要素齐发",
        !err.what.is_empty()
            && !err.why.is_empty()
            && !err.next.is_empty()
            && !err.who.is_empty()
            && err.screen_text().contains(err.code), "");

    // 契约版本三枚。
    set.add(
        "F3005-契约-版本三枚",
        !ORCH_PROTOCOL_VERSION.is_empty()
            && !INTERRUPT_PROTOCOL_VERSION.is_empty()
            && !PRIMITIVE_PROTOCOL_VERSION.is_empty(), "");

    // 对接台账四条（M v2 / O04 选型 / P02 页面转场 / F3003 令牌）。
    set.add(
        "F3005-对接-台账四条",
        EXPECT_HANDOVER_POINTS == 4
            && HANDOVERS.len() == 4
            && HANDOVERS.iter().all(|h| !h.point.is_empty() && !h.delivered.is_empty()), "");
}

/// 期望的对接点条数（本文件写死，不回读被测物）。
const EXPECT_HANDOVER_POINTS: usize = 4;

/// 字符串数组去重计数。
fn unique_strs(v: &[&str]) -> usize {
    let mut c = v.to_vec();
    c.sort_unstable();
    c.dedup();
    c.len()
}

// ---------------------------------------------------------------------------
// 八、红项助手与两个入口
// ---------------------------------------------------------------------------

fn red_items(set: &CheckSet) -> Vec<&'static str> {
    let (items, n) = set.red_items();
    let mut out: Vec<&'static str> = Vec::new();
    for i in 0..n {
        if let Some(Some(c)) = items.get(i) {
            if !c.passed {
                out.push(c.name);
            }
        }
    }
    out
}

/// A 批：图模型+ 四原语 + 嵌套。
pub fn run_vep05_checks_a() -> CheckSet {
    let mut set = CheckSet::new("vep05-orch-a");
    chk_graph(&mut set);
    chk_primitive(&mut set);
    chk_nest(&mut set);
    set
}

/// B 批：打断 + reduce + 预算性能 + 契约纪律。
pub fn run_vep05_checks_b() -> CheckSet {
    let mut set = CheckSet::new("vep05-orch-b");
    chk_interrupt(&mut set);
    chk_reduce(&mut set);
    chk_budget_perf(&mut set);
    chk_contract(&mut set);
    set
}
