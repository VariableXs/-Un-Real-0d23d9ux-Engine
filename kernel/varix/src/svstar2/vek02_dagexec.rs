//! VE-F2002 · 后处理 DAG 执行器（Rust 迁移版 · VE-K 域）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2002`
//!
//! **迁移声明（塔主 2026-10-07 TypeScript 全面迁移指令）**：本模块是
//! `src/system/ve/kDomainPost/f2002-post-fx-dag-executor.ts`（1247 行 TS，
//! 板面 VE-F2002 已完成单）的 **Rust 重实现**——按锚点原文逐面移植：
//! DAG 模型（效果节点+生产者→消费者边，数据流显式声明）、拓扑校验（拒绝
//! 成环并列出环路径/输入槽无生产者/RT 槽多生产者/悬空边/重复 id/超存活
//! 上限/原地未登记白名单）、以声明序打破平局保拓扑序确定性、原子重组
//! （后台编译不阻塞当前帧、帧边界单次指针替换、半新半旧禁止）、开关过滤
//! 连带失效、初始化失败旁路（旁路写**下游真正消费**的输出槽）、帧内重编译
//! 风暴去抖（与最近一次已受理请求比对，防去抖计数恒 0）。迁移完成后该 TS
//! 文件被本实现替代（全仓无其他引用，可删——本任务收尾时删除）。
//!
//! **与 VE-F2016 的关系**：本模块是 F2016 三段 fuzz 第二段（DAG 配置 fuzz）
//! 的被测对象——环/缺边/双生产者/自环/悬空边/重复 id 六类非法图形态必须
//! 全部被 [`validate_graph`] 拦截且报错三要素（码/描述/处置建议）。
//!
//! 与 F2001 的关系：TS 版 import F2001 的 `DiagCode`/`RtRef` 与能力常量；
//! Rust 版为避免把 F2001 整体拖进迁移面，**能力上限 16 与 RtRef 结构按
//! f2001 TS 源单源转录**（`MAX_LIVE_EFFECTS`/`RtRef`），并注明转录来源——
//! F2001 自己的 Rust 迁移由其归属任务负责。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// 一、能力常量与 RT 引用（自 f2001 TS 单源转录）
// ===========================================================================

/// 最大同时存活效果数（f2001 TS `F2002_EXECUTOR_CAPABILITIES.maxLiveEffects`
/// 转录；RT 池容量同源上限）。
pub const MAX_LIVE_EFFECTS: usize = 16;

/// RT 引用（f2001 TS `RtRef` 转录：效果输入输出统一按槽名寻址）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RtRef {
    /// RT 槽名。
    pub slot: String,
}

impl RtRef {
    /// 构造。
    pub fn new(slot: &str) -> RtRef {
        RtRef { slot: slot.to_string() }
    }
}

// ===========================================================================
// 二、诊断与结果类型
// ===========================================================================

/// 执行器专属诊断码（独立枚举，不与 F2003 池共用——处置方向不同）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecDiagCode {
    /// 图中存在环——不可执行，必须列出环路径。
    DagCyclic,
    /// 某效果的输入 RT 无任何生产者——缺边。
    InputSlotHasNoProducer,
    /// RT 槽多生产者冲突——同名输出被两个效果写。
    RtSlotMultiProducer,
    /// 环内效果引用了不存在的节点。
    EdgeEndpointUnknown,
    /// 效果初始化失败——已旁路。
    EffectInitFailedBypassed,
    /// 重编译请求在帧内被合并（风暴去抖）。
    RecompileDebounced,
    /// 同时存活效果数超执行器能力上限。
    LiveEffectLimitExceeded,
    /// 原地效果未登记白名单——自环被拒。
    InPlaceNotDeclared,
    /// 执行列表为空——链无任何可执行节点。
    EmptyPlan,
    /// 图结构非法（节点表与边表不一致等）。
    GraphMalformed,
    /// 换绑请求与当前状态不符（未编译完成就 swap 等）。
    SwapStateInvalid,
}

/// 一条诊断（三要素：码/描述/处置建议）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExecDiagnostic {
    /// 诊断码。
    pub code: ExecDiagCode,
    /// 描述。
    pub message: String,
    /// 处置建议。
    pub hint: String,
}

/// 诊断聚合器（三要素袋：空描述/空建议入袋时补占位——报错不许缺要素）。
#[derive(Clone, Default, Debug)]
pub struct ExecDiagBag {
    items: Vec<ExecDiagnostic>,
}

impl ExecDiagBag {
    /// 入一条诊断。
    pub fn push(&mut self, code: ExecDiagCode, message: &str, hint: &str) {
        self.items.push(ExecDiagnostic {
            code,
            message: if message.is_empty() { "（未提供描述）".to_string() } else { message.to_string() },
            hint: if hint.is_empty() { "（未提供处置建议）".to_string() } else { hint.to_string() },
        });
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 全部诊断。
    pub fn all(&self) -> &[ExecDiagnostic] {
        &self.items
    }

    /// 某码的全部诊断。
    pub fn by_code(&self, code: ExecDiagCode) -> Vec<&ExecDiagnostic> {
        self.items.iter().filter(|d| d.code == code).collect()
    }

    /// 是否含某码。
    pub fn has(&self, code: ExecDiagCode) -> bool {
        self.items.iter().any(|d| d.code == code)
    }
}

/// 校验/编译失败（携带完整三要素与累计诊断——调用方不需要再查袋）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExecError {
    /// 阻断码。
    pub code: ExecDiagCode,
    /// 描述。
    pub message: String,
    /// 处置建议。
    pub hint: String,
    /// 累计诊断。
    pub diagnostics: Vec<ExecDiagnostic>,
}

impl ExecError {
    /// 构造。
    pub fn new(code: ExecDiagCode, message: &str, hint: &str, bag: &ExecDiagBag) -> ExecError {
        let mut diagnostics = bag.all().to_vec();
        diagnostics.push(ExecDiagnostic {
            code,
            message: message.to_string(),
            hint: hint.to_string(),
        });
        ExecError { code, message: message.to_string(), hint: hint.to_string(), diagnostics }
    }
}

// ===========================================================================
// 三、图模型（判据一：DAG 模型）
// ===========================================================================

/// 参数块引用：效果参数由外部块持有，执行器只读不写（参数与执行解耦）。
#[derive(Clone, PartialEq, Debug)]
pub struct ParamBlockRef {
    /// 参数块 id。
    pub block_id: String,
    /// 参数键值表（缺失键按效果声明判定参数不完整）。
    pub values: Vec<(String, f64)>,
}

/// 效果实例 = 效果声明（类型与绑定）+ 参数块引用 + 开关状态。
#[derive(Clone, PartialEq, Debug)]
pub struct EffectNode {
    /// 效果实例 id（执行器分发键，必须唯一）。
    pub effect_id: String,
    /// 效果类型标识（注册制分发键）。
    pub kind: String,
    /// 管线阶段（与 F2001 的 POST_STAGE_ORDER 同源）。
    pub stage: String,
    /// 输入 RT 绑定表。
    pub inputs: Vec<RtRef>,
    /// 输出 RT 槽。
    pub output: RtRef,
    /// 参数块引用。
    pub params: ParamBlockRef,
    /// 开关名（与 F2001 的 toggle 同名同源）。
    pub toggle: String,
    /// 是否原地效果（读写同槽，须在 inPlace 白名单登记）。
    pub in_place: bool,
}

/// RT 依赖边：from 产出的槽被 to 消费。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DagEdge {
    /// 生产者效果 id。
    pub from: String,
    /// 消费者效果 id。
    pub to: String,
}

/// 图模型：节点 + 边。
#[derive(Clone, PartialEq, Debug, Default)]
pub struct EffectGraph {
    /// 效果节点。
    pub nodes: Vec<EffectNode>,
    /// 依赖边。
    pub edges: Vec<DagEdge>,
}

/// 图的完整输入：内部图 + 外部可读槽 + 开关状态 + 原地白名单。
#[derive(Clone, PartialEq, Debug, Default)]
pub struct GraphSpec {
    /// 图。
    pub graph: EffectGraph,
    /// 图外可读槽（场景 RT 等）。
    pub external_slots: Vec<String>,
    /// 开关 → 是否开启（未登记视为开启——缺省不改变既有行为）。
    pub toggles: Vec<(String, bool)>,
    /// 原地效果白名单（effectId 集合）。
    pub in_place_allowed: Vec<String>,
}

impl GraphSpec {
    /// 读开关状态（未登记 = 开启）。
    pub fn toggle_on(&self, toggle: &str) -> bool {
        !self
            .toggles
            .iter()
            .any(|(k, v)| k == toggle && !*v)
    }
}

/// RT 规格（供 F2003 池分配；本条只声明需求不持有实体）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RtSpec {
    /// 槽名。
    pub slot: String,
    /// 宽。
    pub width: u32,
    /// 高。
    pub height: u32,
    /// 格式。
    pub format: String,
}

/// RT 分配计划：编译产物之一（与 F2003 池联动）。
#[derive(Clone, PartialEq, Debug)]
pub struct RtAllocationPlan {
    /// 逐节点的 RT 需求。
    pub allocations: Vec<(String, RtSpec)>,
    /// 池容量需求（对齐 [`MAX_LIVE_EFFECTS`]）。
    pub required_slots: usize,
}

/// 执行计划中的一个步骤（线性、可直接顺序执行）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExecStep {
    /// 效果实例 id。
    pub effect_id: String,
    /// 类型标识。
    pub kind: String,
    /// 阶段。
    pub stage: String,
    /// 实际读取的 RT 槽。
    pub reads: Vec<String>,
    /// 写入的 RT 槽。
    pub writes: String,
    /// 是否为旁路（初始化失败直通）。
    pub bypass: bool,
    /// 拓扑序下标。
    pub index: usize,
}

/// 编译产物：拓扑序执行列表 + RT 分配计划（图 → 线性列表，判据一的核心收益）。
#[derive(Clone, PartialEq, Debug)]
pub struct CompiledPlan {
    /// 线性执行列表。
    pub steps: Vec<ExecStep>,
    /// RT 分配计划。
    pub allocations: RtAllocationPlan,
    /// 图版本号（每次重编译递增）。
    pub version: u64,
    /// 被旁路的节点（显性记录，供 F2013 调试面板显示）。
    pub bypassed: Vec<String>,
    /// 编译期诊断（告警级，如旁路）。
    pub diagnostics: Vec<ExecDiagnostic>,
}

/// 一个环的路径信息（三要素的第一项：环上节点序列）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CycleReport {
    /// 环上节点序列（首尾同一节点，可拼成 a→b→c→a）。
    pub path: Vec<String>,
    /// 长度（节点数）。
    pub length: usize,
}

// ===========================================================================
// 四、拓扑排序与校验（判据二：拓扑校验）
// ===========================================================================

/// 拓扑排序结果。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TopoResult {
    /// 执行序（有环时只含无环部分——可观测但不执行）。
    pub order: Vec<String>,
    /// 全部环报告。
    pub cycles: Vec<CycleReport>,
    /// 未登记白名单的原地效果。
    pub in_place_violation: Vec<String>,
}

/**
 * 拓扑排序（Kahn 算法，打破平局用**声明序**而非哈希迭代序——确定性立场）。
 *
 * 原地效果处理：原地效果的自环不是环——自环边建图时剔除，并要求该效果在
 * `in_place_allowed` 白名单中；未登记的自环按环拒绝并给
 * [`ExecDiagCode::InPlaceNotDeclared`]。
 */
pub fn topo_sort(graph: &EffectGraph, in_place_allowed: &[String]) -> TopoResult {
    let mut node_ids: Vec<String> = Vec::new();
    for n in &graph.nodes {
        if !node_ids.iter().any(|id| id == &n.effect_id) {
            node_ids.push(n.effect_id.clone());
        }
    }
    let in_place_violation: Vec<String> = graph
        .nodes
        .iter()
        .filter(|n| n.in_place && !in_place_allowed.iter().any(|a| a == &n.effect_id))
        .map(|n| n.effect_id.clone())
        .collect();
    // 自环剔除集：原地效果（无论是否登记——未登记的另有违例报告）
    let self_loop: Vec<&str> = graph
        .nodes
        .iter()
        .filter(|n| n.in_place)
        .map(|n| n.effect_id.as_str())
        .collect();

    let mut adj: Vec<(String, Vec<String>)> = node_ids.iter().map(|id| (id.clone(), Vec::new())).collect();
    let mut indeg: Vec<(String, usize)> = node_ids.iter().map(|id| (id.clone(), 0)).collect();
    let indeg_of = |t: &[(String, usize)], id: &str| -> usize {
        t.iter().find(|(k, _)| k == id).map(|(_, v)| *v).unwrap_or(0)
    };

    let mut cycles: Vec<CycleReport> = Vec::new();
    let mut seen_edge: Vec<(String, String)> = Vec::new();
    for e in &graph.edges {
        if seen_edge.iter().any(|(f, t)| f == &e.from && t == &e.to) {
            continue;
        }
        seen_edge.push((e.from.clone(), e.to.clone()));
        if !node_ids.iter().any(|id| id == &e.from) || !node_ids.iter().any(|id| id == &e.to) {
            continue; // 端点未知交由缺失依赖校验报
        }
        if e.from == e.to {
            // 自环：原地效果已剔除；非原地效果的自环是真环
            if self_loop.iter().any(|s| *s == e.from) {
                continue;
            }
            cycles.push(CycleReport { path: vec![e.from.clone(), e.from.clone()], length: 1 });
            continue;
        }
        if let Some(list) = adj.iter_mut().find(|(k, _)| k == &e.from) {
            list.1.push(e.to.clone());
        }
        if let Some(d) = indeg.iter_mut().find(|(k, _)| k == &e.to) {
            d.1 += 1;
        }
    }
    let _ = indeg_of;

    // 确定性入队：按声明序扫描，入度为 0 者依序入队
    let mut queue: Vec<String> = node_ids
        .iter()
        .filter(|id| indeg_of(&indeg, id) == 0)
        .cloned()
        .collect();
    let mut order: Vec<String> = Vec::new();
    let mut popped: Vec<String> = Vec::new();
    while !queue.is_empty() {
        let cur = queue.remove(0);
        if popped.iter().any(|p| p == &cur) {
            continue;
        }
        popped.push(cur.clone());
        order.push(cur.clone());
        let next: Vec<String> = adj
            .iter()
            .find(|(k, _)| k == &cur)
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        // 下一批候选按声明序重扫，保证同层入队顺序确定
        for id in &node_ids {
            if popped.iter().any(|p| p == id) {
                continue;
            }
            if !next.iter().any(|n| n == id) {
                continue;
            }
            if let Some(d) = indeg.iter_mut().find(|(k, _)| k == id) {
                d.1 = d.1.saturating_sub(1);
                if d.1 == 0 {
                    queue.push(id.clone());
                }
            }
        }
    }
    // 未被弹出的节点必在环上——用 DFS 提取具体环路径（三要素要求列出环）
    if order.len() < node_ids.len() {
        for c in find_cycles(&adj, &node_ids, &popped) {
            cycles.push(c);
        }
    }
    TopoResult { order, cycles, in_place_violation }
}

/// 在残余子图中提取具体环路径（迭代 DFS 带路径栈；最多报 8 条防诊断爆炸）。
///
/// 迭代而非递归：no_std 下深图递归有栈溢出风险，纪律统一走显式栈。
fn find_cycles(
    adj: &[(String, Vec<String>)],
    node_ids: &[String],
    popped: &[String],
) -> Vec<CycleReport> {
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }
    let mut color: Vec<(String, Color)> = node_ids
        .iter()
        .map(|id| {
            (
                id.clone(),
                if popped.iter().any(|p| p == id) { Color::Black } else { Color::White },
            )
        })
        .collect();
    let color_of = |t: &[(String, Color)], id: &str| {
        t.iter().find(|(k, _)| k == id).map(|(_, c)| *c).unwrap_or(Color::White)
    };
    let set_color = |t: &mut Vec<(String, Color)>, id: &str, c: Color| {
        if let Some(slot) = t.iter_mut().find(|(k, _)| k == id) {
            slot.1 = c;
        }
    };
    let mut found: Vec<CycleReport> = Vec::new();
    for start in node_ids {
        if color_of(&color, start) != Color::White || found.len() >= 8 {
            continue;
        }
        // 显式 DFS：(节点, 下一条待展开的邻边下标)；path 即灰色栈
        let mut work: Vec<(String, usize)> = vec![(start.clone(), 0usize)];
        let mut path: Vec<String> = vec![start.clone()];
        set_color(&mut color, start, Color::Gray);
        while let Some(top) = work.last_mut() {
            let neighbors = adj
                .iter()
                .find(|(k, _)| k == &top.0)
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            let mut descended = false;
            while (top.1 as usize) < neighbors.len() && found.len() < 8 {
                let v = neighbors[top.1 as usize].clone();
                top.1 += 1;
                match color_of(&color, &v) {
                    Color::White => {
                        set_color(&mut color, &v, Color::Gray);
                        path.push(v.clone());
                        work.push((v, 0usize));
                        descended = true;
                        break;
                    }
                    Color::Gray => {
                        // 回边：从灰色栈中截出环段（栈内首现位置到栈顶，再接回 v）
                        let begin = path.iter().position(|s| s == &v).unwrap_or(0);
                        let mut cycle_path: Vec<String> = path[begin..].to_vec();
                        cycle_path.push(v);
                        let length = cycle_path.len().saturating_sub(1);
                        found.push(CycleReport { path: cycle_path, length });
                    }
                    Color::Black => {}
                }
            }
            if descended {
                continue;
            }
            // 邻边展开完：出栈、染黑
            work.pop();
            if let Some(u) = path.pop() {
                set_color(&mut color, &u, Color::Black);
            }
        }
        if found.len() >= 8 {
            break;
        }
    }
    found
}

/// 校验通过的图（执行序 + 节点表）。
#[derive(Clone, PartialEq, Debug)]
pub struct ValidatedGraph {
    /// 拓扑执行序。
    pub order: Vec<String>,
    /// 节点表（按 effectId 寻址）。
    pub nodes: Vec<EffectNode>,
}

impl ValidatedGraph {
    /// 按 id 取节点。
    pub fn node(&self, id: &str) -> Option<&EffectNode> {
        self.nodes.iter().find(|n| n.effect_id == id)
    }
}

/// 校验图结构并返回可用的执行序（阻断项一律拒绝；报错三要素）。
///
/// 拒绝序：重复 id → 悬空边 → RT 槽多生产者 → 输入槽无生产者 → 原地未登记
/// → 环 → 超存活上限。每类给独立诊断码，前一类命中即返回（首错阻断语义
/// 与 TS 版一致）。
pub fn validate_graph(spec: &GraphSpec) -> Result<ValidatedGraph, ExecError> {
    let bag = ExecDiagBag::default();
    let graph = &spec.graph;
    let mut nodes: Vec<EffectNode> = Vec::new();
    for n in &graph.nodes {
        if nodes.iter().any(|m| m.effect_id == n.effect_id) {
            let msg = format!("效果 id 重复：{}。", n.effect_id);
            return Err(ExecError::new(
                ExecDiagCode::GraphMalformed,
                &msg,
                "效果 id 是执行器分发键，必须唯一；改用唯一 effectId。",
                &bag,
            ));
        }
        nodes.push(n.clone());
    }
    // 边端点必须存在
    let mut bag2 = ExecDiagBag::default();
    for e in &graph.edges {
        let from_ok = nodes.iter().any(|n| n.effect_id == e.from);
        let to_ok = nodes.iter().any(|n| n.effect_id == e.to);
        if !from_ok || !to_ok {
            let bad = if from_ok { &e.to } else { &e.from };
            bag2.push(
                ExecDiagCode::EdgeEndpointUnknown,
                &format!("依赖边端点不存在：{}。", bad),
                "边的两端都必须是已声明的效果节点。",
            );
        }
    }
    if bag2.has(ExecDiagCode::EdgeEndpointUnknown) {
        let first = bag2.by_code(ExecDiagCode::EdgeEndpointUnknown)[0].clone();
        return Err(ExecError {
            code: first.code,
            message: first.message,
            hint: "补齐节点声明或删除悬空边。".to_string(),
            diagnostics: bag2.all().to_vec(),
        });
    }
    // RT 槽多生产者冲突
    let mut producer: Vec<(String, String)> = Vec::new();
    for n in &nodes {
        if let Some(prev) = producer.iter().find(|(s, _)| s == &n.output.slot) {
            let mut bag3 = ExecDiagBag::default();
            bag3.push(
                ExecDiagCode::RtSlotMultiProducer,
                &format!("RT 槽 {} 有多个生产者：{} 与 {}。", n.output.slot, prev.1, n.effect_id),
                "同名输出多生产者会让执行序决定谁赢且结果不可预测；改为不同槽或合并为一个效果。",
            );
            return Err(ExecError::new(
                ExecDiagCode::RtSlotMultiProducer,
                &bag3.all()[0].message,
                "编译期拒绝，不用后者覆盖。",
                &bag3,
            ));
        }
        producer.push((n.output.slot.clone(), n.effect_id.clone()));
    }
    // 输入槽必须有生产者（内部或外部）
    let mut bag4 = ExecDiagBag::default();
    for n in &nodes {
        for inp in &n.inputs {
            let known = spec.external_slots.iter().any(|s| s == &inp.slot)
                || producer.iter().any(|(s, _)| s == &inp.slot);
            if !known {
                bag4.push(
                    ExecDiagCode::InputSlotHasNoProducer,
                    &format!("效果 {} 的输入槽 {} 无任何生产者。", n.effect_id, inp.slot),
                    "补上生产该槽的效果，或将其登记为外部可读槽（场景缓冲等）。",
                );
            }
        }
    }
    if bag4.has(ExecDiagCode::InputSlotHasNoProducer) {
        let first = bag4.by_code(ExecDiagCode::InputSlotHasNoProducer)[0].clone();
        return Err(ExecError {
            code: first.code,
            message: first.message,
            hint: "补齐生产者或声明为外部槽。".to_string(),
            diagnostics: bag4.all().to_vec(),
        });
    }
    // 原地效果白名单
    for n in &nodes {
        if n.in_place && !spec.in_place_allowed.iter().any(|a| a == &n.effect_id) {
            let mut bag5 = ExecDiagBag::default();
            bag5.push(
                ExecDiagCode::InPlaceNotDeclared,
                &format!("原地效果 {} 未登记白名单。", n.effect_id),
                "原地效果读写同槽会在图上形成自环；须在 inPlaceAllowed 中显式登记。",
            );
            return Err(ExecError::new(
                ExecDiagCode::InPlaceNotDeclared,
                &bag5.all()[0].message,
                "登记到白名单，或改为非原地。",
                &bag5,
            ));
        }
    }
    // 环检测
    let topo = topo_sort(graph, &spec.in_place_allowed);
    if !topo.cycles.is_empty() {
        let c0 = &topo.cycles[0];
        let path_text = c0.path.join(" → ");
        let mut bag6 = ExecDiagBag::default();
        bag6.push(
            ExecDiagCode::DagCyclic,
            &format!("图中存在环：{}。", path_text),
            "环上每个效果都在等待另一个先执行，无合法执行序；断开环中任一条边。",
        );
        return Err(ExecError::new(
            ExecDiagCode::DagCyclic,
            &format!("图中存在环：{}。", path_text),
            &format!("环路径：{}；断开其中任一条边即可解环。", path_text),
            &bag6,
        ));
    }
    // 同时存活效果数上限
    if graph.nodes.len() > MAX_LIVE_EFFECTS {
        let mut bag7 = ExecDiagBag::default();
        bag7.push(
            ExecDiagCode::LiveEffectLimitExceeded,
            &format!(
                "图含 {} 个效果，超执行器同时存活上限 {}。",
                graph.nodes.len(),
                MAX_LIVE_EFFECTS
            ),
            "拆分链或降档（F2015）；执行器能力上限以 F2001 声明为准。",
        );
        return Err(ExecError::new(
            ExecDiagCode::LiveEffectLimitExceeded,
            &format!("图含 {} 个效果，超上限 {}。", graph.nodes.len(), MAX_LIVE_EFFECTS),
            "拆分链或降档。",
            &bag7,
        ));
    }
    Ok(ValidatedGraph { order: topo.order, nodes })
}

// ===========================================================================
// 五、编译（拓扑序 → 线性执行列表 + RT 分配计划）
// ===========================================================================

/// RT 默认规格（由 F2003 池按实际渲染目标覆写；本条只给兜底声明）。
pub fn default_rt_spec() -> RtSpec {
    RtSpec {
        slot: String::new(),
        width: 1920,
        height: 1080,
        format: "rgba16f".to_string(),
    }
}

/// 效果初始化器：给定节点，`Err(reason)` 即旁路。
pub type EffectInitializer<'a> = dyn Fn(&EffectNode) -> Result<(), String> + 'a;

/// 编译选项。
pub struct CompileOptions<'a> {
    /// 图版本号（每次重编译递增）。
    pub version: u64,
    /// RT 规格覆写（按槽给）。
    pub rt_spec_override: Vec<(String, RtSpec)>,
    /// 初始化器；`None` 视为全部成功。
    pub init: Option<&'a EffectInitializer<'a>>,
}

/**
 * 编译：图 → 线性执行列表。
 *
 * 开关关闭的节点被**剔除**（不进执行列表），其下游若失去输入则连带失效——
 * 这是开关切换必须重编译的原因：图结构变了，不是只打个标记。
 */
pub fn compile_graph(spec: &GraphSpec, options: &CompileOptions) -> Result<CompiledPlan, ExecError> {
    let mut bag = ExecDiagBag::default();
    let validated = validate_graph(spec)?;
    // 开关过滤：关闭的效果不进图
    let enabled: Vec<&EffectNode> = spec
        .graph
        .nodes
        .iter()
        .filter(|n| spec.toggle_on(&n.toggle))
        .collect();
    // 过滤后重新校验（剔除节点可能造成输入槽无主）
    let mut live_slots: Vec<String> = spec.external_slots.clone();
    for n in &enabled {
        live_slots.push(n.output.slot.clone());
    }
    let mut orphan: Vec<String> = Vec::new();
    for n in &enabled {
        for inp in &n.inputs {
            if !live_slots.iter().any(|s| s == &inp.slot) {
                orphan.push(format!("{}←{}", n.effect_id, inp.slot));
            }
        }
    }
    if !orphan.is_empty() {
        let text = orphan.join("、");
        bag.push(
            ExecDiagCode::InputSlotHasNoProducer,
            &format!("开关关闭后以下输入失去生产者：{}。", text),
            "被关闭效果的下游若仍开启，须为其提供替代输入或连带关闭该下游。",
        );
        return Err(ExecError::new(
            ExecDiagCode::InputSlotHasNoProducer,
            &format!("开关关闭后以下输入失去生产者：{}。", text),
            "连带关闭依赖它的下游效果，或改用外部槽。",
            &bag,
        ));
    }
    let filtered_graph = EffectGraph {
        nodes: enabled.iter().map(|n| (*n).clone()).collect(),
        edges: spec
            .graph
            .edges
            .iter()
            .filter(|e| {
                enabled.iter().any(|n| n.effect_id == e.from)
                    && enabled.iter().any(|n| n.effect_id == e.to)
            })
            .cloned()
            .collect(),
    };
    let topo = topo_sort(&filtered_graph, &spec.in_place_allowed);
    if !topo.cycles.is_empty() {
        let path_text = topo.cycles[0].path.join(" → ");
        return Err(ExecError::new(
            ExecDiagCode::DagCyclic,
            &format!("开关过滤后图中存在环：{}。", path_text),
            &format!("环路径：{}；检查开关组合。", path_text),
            &bag,
        ));
    }
    if topo.order.is_empty() {
        bag.push(
            ExecDiagCode::EmptyPlan,
            "编译结果为空执行列表。",
            "至少保留一个开启的效果，或接受本帧无后处理。",
        );
        return Err(ExecError::new(
            ExecDiagCode::EmptyPlan,
            "编译结果为空执行列表。",
            "至少保留一个开启的效果。",
            &bag,
        ));
    }
    // 逐步骤生成
    let mut steps: Vec<ExecStep> = Vec::new();
    let mut allocations: Vec<(String, RtSpec)> = Vec::new();
    let mut bypassed: Vec<String> = Vec::new();
    let mut index = 0usize;
    for id in &topo.order {
        let node = match validated.node(id) {
            Some(n) => n.clone(),
            None => continue,
        };
        // 初始化：失败即旁路（判据四）
        if let Some(init) = options.init {
            if let Err(reason) = init(&node) {
                bypassed.push(node.effect_id.clone());
                bag.push(
                    ExecDiagCode::EffectInitFailedBypassed,
                    &format!("效果 {} 初始化失败（{}），已旁路。", node.effect_id, reason),
                    "该节点直通：输入原样接到输出槽，下游仍可消费；请修效果实现或用开关禁用。",
                );
                // 直通语义：旁路必须写下游**真正消费**的那个槽——
                // 下游读的是本节点的 output.slot，不是 inputs[0].slot。写错槽
                // 会让下游读到未初始化 RT，表现为「效果偶发失效」而非报错。
                let has_downstream = filtered_graph.edges.iter().any(|e| e.from == node.effect_id);
                if has_downstream && node.inputs.is_empty() {
                    bag.push(
                        ExecDiagCode::EffectInitFailedBypassed,
                        &format!("效果 {} 初始化失败且无输入可直通，但存在下游消费者。", node.effect_id),
                        "无法旁路：该效果无输入可继承，输出槽将保持未初始化；请给该效果声明至少一个输入，或关闭其下游。",
                    );
                }
                steps.push(ExecStep {
                    effect_id: node.effect_id.clone(),
                    kind: node.kind.clone(),
                    stage: node.stage.clone(),
                    reads: if node.in_place {
                        vec![node.output.slot.clone()]
                    } else {
                        node.inputs.iter().map(|i| i.slot.clone()).collect()
                    },
                    writes: node.output.slot.clone(),
                    bypass: true,
                    index,
                });
                index += 1;
                continue;
            }
        }
        steps.push(ExecStep {
            effect_id: node.effect_id.clone(),
            kind: node.kind.clone(),
            stage: node.stage.clone(),
            reads: if node.in_place {
                vec![node.output.slot.clone()]
            } else {
                node.inputs.iter().map(|i| i.slot.clone()).collect()
            },
            writes: node.output.slot.clone(),
            bypass: false,
            index,
        });
        let spec_for_node = options
            .rt_spec_override
            .iter()
            .find(|(s, _)| s == &node.output.slot)
            .map(|(_, sp)| sp.clone())
            .unwrap_or_else(|| RtSpec {
                slot: node.output.slot.clone(),
                ..default_rt_spec()
            });
        allocations.push((node.effect_id.clone(), spec_for_node));
        index += 1;
    }
    let required = allocations.len();
    Ok(CompiledPlan {
        steps,
        allocations: RtAllocationPlan { allocations, required_slots: required },
        version: options.version,
        bypassed,
        diagnostics: bag.all().to_vec(),
    })
}

// ===========================================================================
// 六、动态链重组（判据三：帧边界原子换绑 + 风暴去抖）
// ===========================================================================

/**
 * 执行器：持有当前生效计划与待换绑计划。
 *
 * 状态机：compiling（后台编译中，当前计划继续服务帧）→ ready（挂 pending）
 * → running（帧边界原子 swap）。半新半旧禁止：swap 是单次替换，发生在帧边界，
 * 不存在两套绑定同时服务一帧。
 */
pub struct PostFxExecutor {
    current: Option<CompiledPlan>,
    pending: Option<CompiledPlan>,
    compiling: Option<CompiledPlan>,
    diag_bag: ExecDiagBag,
    queued: Option<(GraphSpec, QueuedOpts)>,
    queued_count: u64,
    next_version: u64,
    frame_index: u64,
    last_spec: Option<GraphSpec>,
}

/// 排队请求的编译选项（TS 闭包可存，Rust 侧初始化器不可复制存储——
/// 排队重放统一走「无初始化器编译」，携带初始化器的语料走同步
/// `compile_graph` 验证，不经执行器队列）。
type QueuedOpts = ();

/// 重编译请求结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecompileAck {
    /// 已受理（后台编译中或已入队）。
    Queued,
    /// 与最近一次受理请求等价，被去抖合并。
    Debounced,
}

impl Default for PostFxExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl PostFxExecutor {
    /// 构造。
    pub fn new() -> PostFxExecutor {
        PostFxExecutor {
            current: None,
            pending: None,
            compiling: None,
            diag_bag: ExecDiagBag::default(),
            queued: None,
            queued_count: 0,
            next_version: 1,
            frame_index: 0,
            last_spec: None,
        }
    }

    /// 当前生效计划（未编译过则为 `None`）。
    pub fn active_plan(&self) -> Option<&CompiledPlan> {
        self.current.as_ref()
    }

    /// 待换绑计划（编译完成即算待换绑——返回 compiling/pending 中较新的）。
    pub fn pending_plan(&self) -> Option<&CompiledPlan> {
        self.pending.as_ref().or(self.compiling.as_ref())
    }

    /// 是否有正在后台编译的请求。
    pub fn is_compiling(&self) -> bool {
        self.compiling.is_some()
    }

    /// 重编译请求计数（风暴观测）。
    pub fn queued_request_count(&self) -> u64 {
        self.queued_count
    }

    /// 最近一次请求重编译的图（F2013 调试面板消费）。
    pub fn last_requested_spec(&self) -> Option<&GraphSpec> {
        self.last_spec.as_ref()
    }

    /// 累计诊断。
    pub fn diagnostics(&self) -> &[ExecDiagnostic] {
        self.diag_bag.all()
    }

    /**
     * 请求重编译（开关切换等）。帧内多次请求**合并为一次**（判据五）：
     * 与**最近一次已受理请求**（`last_spec`，而非 queued_spec——编译产物同步
     * 产出后 queued 会被清空，只比 queued 会让去抖计数恒 0，狂拖滑条引发
     * 几十次重复入队，实测踩过）等价即折叠。
     */
    pub fn request_recompile(&mut self, spec: GraphSpec) -> Result<RecompileAck, ExecError> {
        let same_as_last = self
            .last_spec
            .as_ref()
            .map(|l| same_graph_spec(l, &spec))
            .unwrap_or(false);
        let idle = self.compiling.is_none() && self.pending.is_none() && self.queued.is_none();
        if same_as_last && !idle {
            self.queued_count += 1;
            self.diag_bag.push(
                ExecDiagCode::RecompileDebounced,
                "重编译请求与最近一次受理请求相同，已合并，未触发新编译。",
                "开关高频切换已去抖到帧边界；如需每帧生效请勿提交等价请求。",
            );
            return Ok(RecompileAck::Debounced);
        }
        if self.compiling.is_some() {
            // 上一轮还在编译：覆盖待处理 spec，等本轮结束后只编一次
            self.queued = Some((spec.clone(), ()));
            self.last_spec = Some(spec);
            self.queued_count += 1;
            self.diag_bag.push(
                ExecDiagCode::RecompileDebounced,
                "上一轮编译尚未完成，新请求已并入待处理队列（帧边界后只编一次）。",
                "编译在后台进行，不阻塞当前帧渲染。",
            );
            return Ok(RecompileAck::Queued);
        }
        let version = self.next_version;
        self.next_version += 1;
        self.last_spec = Some(spec.clone());
        self.queued_count += 1;
        let opts = CompileOptions { version, rt_spec_override: Vec::new(), init: None };
        match compile_graph(&spec, &opts) {
            Err(e) => {
                // 编译失败：保留当前计划继续服务，不换绑（失败不该打断渲染）
                self.queued = None;
                Err(e)
            }
            Ok(plan) => {
                self.compiling = Some(plan);
                self.queued = None;
                Ok(RecompileAck::Queued)
            }
        }
    }

    /**
     * 帧边界：把编译完成的计划原子换绑（判据三）。返回是否发生了换绑。
     * 编译期间积累的请求在帧边界后启动下一轮（批处理）。
     */
    pub fn begin_frame(&mut self) -> bool {
        self.frame_index += 1;
        let mut swapped = false;
        if let Some(c) = self.compiling.take() {
            self.pending = Some(c);
        }
        if let Some(p) = self.pending.take() {
            // 原子换绑：单次替换，帧内不再变更
            self.current = Some(p);
            swapped = true;
        }
        // 编译期间积累的请求：帧边界后启动下一轮
        if let Some((spec, _opts)) = self.queued.take() {
            if self.compiling.is_none() {
                let version = self.next_version;
                self.next_version += 1;
                let opts = CompileOptions { version, rt_spec_override: Vec::new(), init: None };
                match compile_graph(&spec, &opts) {
                    Ok(plan) => self.compiling = Some(plan),
                    Err(e) => self.diag_bag.push(e.code, &e.message, &e.hint),
                }
            }
        }
        swapped
    }

    /// 帧号（单调递增）。
    pub fn current_frame(&self) -> u64 {
        self.frame_index
    }

    /**
     * 执行当前计划的一帧（零图遍历开销：只顺序走线性列表）。
     * 执行记录供 F2013 调试可视化消费。
     */
    pub fn execute_frame(&self) -> Result<Vec<ExecStep>, ExecError> {
        match &self.current {
            None => Err(ExecError::new(
                ExecDiagCode::SwapStateInvalid,
                "当前无生效计划。",
                "先 requestRecompile 并经过一次 beginFrame 换绑。",
                &self.diag_bag,
            )),
            Some(plan) => Ok(plan.steps.clone()),
        }
    }
}

/// 图 spec 等价性判定（用于去抖：同图重复请求折叠为一次）。
pub fn same_graph_spec(a: &GraphSpec, b: &GraphSpec) -> bool {
    if a.external_slots.len() != b.external_slots.len()
        || a.graph.nodes.len() != b.graph.nodes.len()
        || a.graph.edges.len() != b.graph.edges.len()
    {
        return false;
    }
    let mut ka: Vec<&(String, bool)> = a.toggles.iter().collect();
    let mut kb: Vec<&(String, bool)> = b.toggles.iter().collect();
    ka.sort_by(|x, y| x.0.cmp(&y.0));
    kb.sort_by(|x, y| x.0.cmp(&y.0));
    if ka.len() != kb.len() {
        return false;
    }
    for (x, y) in ka.iter().zip(kb.iter()) {
        if x.0 != y.0 || x.1 != y.1 {
            return false;
        }
    }
    let mut na: Vec<String> = a
        .graph
        .nodes
        .iter()
        .map(|n| format!("{}:{}:{}", n.effect_id, n.toggle, n.output.slot))
        .collect();
    let mut nb: Vec<String> = b
        .graph
        .nodes
        .iter()
        .map(|n| format!("{}:{}:{}", n.effect_id, n.toggle, n.output.slot))
        .collect();
    na.sort();
    nb.sort();
    na == nb
}

/// 帧边界重编译去抖的批处理器：帧内多次请求折叠为一次帧边界调用。
#[derive(Default)]
pub struct RecompileDebouncer {
    pending: Vec<(String, GraphSpec)>,
    dropped: u64,
}

impl RecompileDebouncer {
    /// 登记一次请求（同 key 覆盖，只有一次生效）。
    pub fn enqueue(&mut self, key: &str, spec: GraphSpec) {
        if self.pending.iter().any(|(k, _)| k == key) {
            self.dropped += 1;
            if let Some(slot) = self.pending.iter_mut().find(|(k, _)| k == key) {
                slot.1 = spec;
            }
        } else {
            self.pending.push((key.to_string(), spec));
        }
    }

    /// 帧边界取出全部待编译请求并清空。
    pub fn drain(&mut self) -> Vec<GraphSpec> {
        let out = self.pending.iter().map(|(_, s)| s.clone()).collect();
        self.pending.clear();
        out
    }

    /// 被去抖丢弃的请求数。
    pub fn dropped_count(&self) -> u64 {
        self.dropped
    }

    /// 待处理条数。
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

// ===========================================================================
// 七、域自检（TS 版 runSelfCheck 判据组的 Rust 移植）
// ===========================================================================

/// VE-F2002 迁移件自检（TS 版 selfCheckDag/selfCheckValidation/
/// selfCheckAtomicSwap/selfCheckBypass 判据组的移植；随模块常驻编译）。
pub mod checks {
    use super::*;
    use crate::checks::CheckSet;

    fn mk(effect_id: &str, out: &str, ins: &[&str], in_place: bool) -> EffectNode {
        EffectNode {
            effect_id: effect_id.to_string(),
            kind: format!("kind_{}", effect_id),
            stage: "post".to_string(),
            inputs: ins.iter().map(|s| RtRef::new(s)).collect(),
            output: RtRef::new(out),
            params: ParamBlockRef { block_id: format!("pb_{}", effect_id), values: Vec::new() },
            toggle: format!("t_{}", effect_id),
            in_place,
        }
    }

    fn clean_spec() -> GraphSpec {
        GraphSpec {
            graph: EffectGraph {
                nodes: vec![
                    mk("a", "s1", &["sceneColor"], false),
                    mk("b", "s2", &["s1"], false),
                    mk("c", "s3", &["s2"], false),
                ],
                edges: vec![
                    DagEdge { from: "a".to_string(), to: "b".to_string() },
                    DagEdge { from: "b".to_string(), to: "c".to_string() },
                ],
            },
            external_slots: vec!["sceneColor".to_string()],
            toggles: Vec::new(),
            in_place_allowed: Vec::new(),
        }
    }

    /// 跑全部判据。
    pub fn run_vek02_checks() -> CheckSet {
        let mut s = CheckSet::new("svstar2-vek02");

        // -- P02-DAG-01 链式拓扑序 + 线性序号 + 分配计划（TS selfCheckDag）----
        {
            let plan = compile_graph(&clean_spec(), &CompileOptions { version: 1, rt_spec_override: Vec::new(), init: None });
            let ok = match &plan {
                Ok(p) => {
                    p.steps.len() == 3
                        && p.steps[0].effect_id == "a"
                        && p.steps[1].effect_id == "b"
                        && p.steps[2].effect_id == "c"
                        && p.steps.iter().enumerate().all(|(i, st)| st.index == i)
                        && p.allocations.allocations.len() == 3
                        && p.allocations.required_slots == 3
                }
                Err(_) => false,
            };
            s.add(
                "P02-DAG-01 链式图拓扑序 a→b→c、线性序号连续、RT 分配逐节点",
                ok,
                "TS 版 selfCheckDag 判据一移植；图→线性列表是执行器零图遍历的根基",
            );
        }

        // -- P02-DAG-02 菱形序满足依赖且 5 次编译序确定（确定性立场）----------
        {
            let dia = GraphSpec {
                graph: EffectGraph {
                    nodes: vec![
                        mk("D", "sd", &["sceneColor"], false),
                        mk("B", "sb", &["sd"], false),
                        mk("C", "sc", &["sd"], false),
                        mk("E", "se", &["sb", "sc"], false),
                    ],
                    edges: vec![
                        DagEdge { from: "D".to_string(), to: "B".to_string() },
                        DagEdge { from: "D".to_string(), to: "C".to_string() },
                        DagEdge { from: "B".to_string(), to: "E".to_string() },
                        DagEdge { from: "C".to_string(), to: "E".to_string() },
                    ],
                },
                external_slots: vec!["sceneColor".to_string()],
                toggles: Vec::new(),
                in_place_allowed: Vec::new(),
            };
            let mut runs: Vec<Vec<String>> = Vec::new();
            for _ in 0..5 {
                match compile_graph(&dia, &CompileOptions { version: 1, rt_spec_override: Vec::new(), init: None }) {
                    Ok(p) => runs.push(p.steps.iter().map(|st| st.effect_id.clone()).collect()),
                    Err(_) => runs.push(Vec::new()),
                }
            }
            let deterministic = runs.iter().all(|r| *r == runs[0]);
            let order_ok = match runs.first() {
                Some(o) if o.len() == 4 => {
                    let pos = |id: &str| o.iter().position(|x| x == id).unwrap_or(99);
                    pos("D") < pos("B") && pos("D") < pos("C") && pos("B") < pos("E") && pos("C") < pos("E")
                }
                _ => false,
            };
            s.add(
                "P02-DAG-02 菱形图依赖序满足全部约束且 5 次编译逐位一致",
                order_ok && deterministic,
                "声明序打破平局保拓扑序确定性（立场一）——哈希迭代序会让同图两编译不同序",
            );
        }

        // -- P02-VAL-01 六类非法形态拒绝且诊断码各自正确 ----------------------
        {
            let cyc = GraphSpec {
                graph: EffectGraph {
                    nodes: vec![mk("a", "s1", &["sceneColor"], false), mk("b", "s2", &["s1"], false)],
                    edges: vec![
                        DagEdge { from: "a".to_string(), to: "b".to_string() },
                        DagEdge { from: "b".to_string(), to: "a".to_string() },
                    ],
                },
                external_slots: vec!["sceneColor".to_string()],
                toggles: Vec::new(),
                in_place_allowed: Vec::new(),
            };
            let cyc_code = validate_graph(&cyc).err().map(|e| e.code);
            let miss = GraphSpec {
                graph: EffectGraph { nodes: vec![mk("a", "s1", &["notProduced"], false)], edges: Vec::new() },
                external_slots: vec!["sceneColor".to_string()],
                toggles: Vec::new(),
                in_place_allowed: Vec::new(),
            };
            let miss_code = validate_graph(&miss).err().map(|e| e.code);
            let dual = GraphSpec {
                graph: EffectGraph {
                    nodes: vec![mk("a", "s1", &["sceneColor"], false), mk("b", "s1", &["sceneColor"], false)],
                    edges: Vec::new(),
                },
                external_slots: vec!["sceneColor".to_string()],
                toggles: Vec::new(),
                in_place_allowed: Vec::new(),
            };
            let dual_code = validate_graph(&dual).err().map(|e| e.code);
            let dangling = GraphSpec {
                graph: EffectGraph {
                    nodes: vec![mk("a", "s1", &["sceneColor"], false)],
                    edges: vec![DagEdge { from: "a".to_string(), to: "ghost".to_string() }],
                },
                external_slots: vec!["sceneColor".to_string()],
                toggles: Vec::new(),
                in_place_allowed: Vec::new(),
            };
            let dangling_code = validate_graph(&dangling).err().map(|e| e.code);
            let dup = GraphSpec {
                graph: EffectGraph {
                    nodes: vec![mk("a", "s1", &["sceneColor"], false), mk("a", "s2", &["sceneColor"], false)],
                    edges: Vec::new(),
                },
                external_slots: vec!["sceneColor".to_string()],
                toggles: Vec::new(),
                in_place_allowed: Vec::new(),
            };
            let dup_code = validate_graph(&dup).err().map(|e| e.code);
            let ip = GraphSpec {
                graph: EffectGraph { nodes: vec![mk("a", "s1", &["sceneColor"], true)], edges: Vec::new() },
                external_slots: vec!["sceneColor".to_string()],
                toggles: Vec::new(),
                in_place_allowed: Vec::new(),
            };
            let ip_code = validate_graph(&ip).err().map(|e| e.code);
            let ok_code = |c: Option<ExecDiagCode>, want: ExecDiagCode| matches!(c, Some(x) if x == want);
            s.add(
                "P02-VAL-01 六类非法形态各自拒绝码正确（环/缺生产者/双生产者/悬空边/重复id/原地未登记）",
                ok_code(cyc_code, ExecDiagCode::DagCyclic)
                    && ok_code(miss_code, ExecDiagCode::InputSlotHasNoProducer)
                    && ok_code(dual_code, ExecDiagCode::RtSlotMultiProducer)
                    && ok_code(dangling_code, ExecDiagCode::EdgeEndpointUnknown)
                    && ok_code(dup_code, ExecDiagCode::GraphMalformed)
                    && ok_code(ip_code, ExecDiagCode::InPlaceNotDeclared),
                "TS 版 selfCheckValidation 判据二移植；拒绝码分型是处置路由的依据",
            );
        }

        // -- P02-SW-01 开关过滤连带失效（关上游后下游失去输入被拒）------------
        {
            let mut spec = clean_spec();
            spec.toggles = vec![("t_a".to_string(), false)];
            let r = compile_graph(&spec, &CompileOptions { version: 1, rt_spec_override: Vec::new(), init: None });
            let rejected = matches!(&r, Err(e) if e.code == ExecDiagCode::InputSlotHasNoProducer);
            s.add(
                "P02-SW-01 关闭上游效果后下游失去生产者被拒（开关=改图结构非打标记）",
                rejected,
                "这是开关切换必须重编译的原因；静默跳过会让下游读到未初始化 RT",
            );
        }

        // -- P02-BY-01 初始化失败旁路：写下游真正消费的槽 + 显性记录 ----------
        {
            let opts = CompileOptions {
                version: 1,
                rt_spec_override: Vec::new(),
                init: Some(&|n: &EffectNode| {
                    if n.effect_id == "b" { Err("boom".to_string()) } else { Ok(()) }
                }),
            };
            let r = compile_graph(&clean_spec(), &opts);
            let ok = match &r {
                Ok(p) => {
                    let b = p.steps.iter().find(|st| st.effect_id == "b");
                    let bypassed = p.bypassed.len() == 1 && p.bypassed[0] == "b";
                    let diag = p.diagnostics.iter().any(|d| d.code == ExecDiagCode::EffectInitFailedBypassed);
                    b.map(|st| st.bypass && st.writes == "s2" && st.reads == vec!["s1".to_string()])
                        .unwrap_or(false)
                        && bypassed
                        && diag
                }
                Err(_) => false,
            };
            s.add(
                "P02-BY-01 初始化失败旁路：bypass 步读输入槽写输出槽（下游消费面）且显性记录",
                ok,
                "旁路写 inputs[0] 会让下游读未初始化 RT（表现=效果偶发失效，最难归因）",
            );
        }

        // -- P02-AT-01 帧边界原子换绑 + 风暴去抖 ------------------------------
        {
            let mut ex = PostFxExecutor::new();
            let req1 = ex.request_recompile(clean_spec());
            let before = ex.active_plan().is_none();
            // 同图重复请求必须落在**编译尚未换绑**的窗口内才去抖——换绑完成后
            // 执行器回到 idle，同图请求按语义是新请求（TS 版 idle 判定同款）。
            let req2 = ex.request_recompile(clean_spec());
            let debounced = matches!(req2, Ok(RecompileAck::Debounced));
            let swapped1 = ex.begin_frame();
            let after = ex.active_plan().is_some();
            let swapped2 = ex.begin_frame();
            s.add(
                "P02-AT-01 请求不即换、帧内同图重请求去抖、帧边界原子换绑一次、二次帧边界无重复换绑",
                matches!(req1, Ok(RecompileAck::Queued))
                    && before
                    && debounced
                    && swapped1
                    && after
                    && !swapped2,
                "半新半旧禁止：swap 是帧边界单次替换；去抖比 lastSpec 防「重复入队风暴」",
            );
        }

        // -- P02-CAP-01 存活上限（能力常量单源 16）---------------------------
        {
            let mut nodes = Vec::new();
            for i in 0..17 {
                nodes.push(mk(&format!("n{}", i), &format!("o{}", i), &["sceneColor"], false));
            }
            let big = GraphSpec {
                graph: EffectGraph { nodes, edges: Vec::new() },
                external_slots: vec!["sceneColor".to_string()],
                toggles: Vec::new(),
                in_place_allowed: Vec::new(),
            };
            let r = validate_graph(&big);
            s.add(
                "P02-CAP-01 十七节点图超存活上限 16 被拒（能力常量与 F2001 单源）",
                matches!(&r, Err(e) if e.code == ExecDiagCode::LiveEffectLimitExceeded),
                "上限对齐 F2001 能力声明；RT 池容量同源",
            );
        }

        s
    }
}
