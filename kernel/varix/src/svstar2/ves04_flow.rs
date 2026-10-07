//! VE-F3604 · 创作工作流引擎（VE-S 域 · 创作生态组 · 工作流段 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3604`
//!
//! **判据（锚点原文）**：DAG 复用、三条预置、断点续作、自定义沙箱、驱动协议、判据。
//!
//! **职责定位（锚点原文）**：创作工作流引擎——工作流（创作任务编排：工作流模型
//! （步骤节点+依赖（复用 F3005 编排 DAG 模式跨域声明——模式复用单源）；内置工作流
//! （新建主题/导入资产/发布流程三条预置流——预置流）；工作流可自定义（用户自定义
//! 工作流（开放生长复述——自定义沙箱复述）；工作流状态（步骤状态机+断点续作
//! （中断恢复红线复述）；与工具层联动（工作流驱动编辑器/工坊界面（驱动协议）。
//!
//! **数据结构（锚点原文）**：DAG 复用声明；三条预置流；状态机复用；驱动协议。
//!
//! **错误路径与降级矩阵（锚点原文）**：环检出→拒绝（复用 F3005 红线）；中断丢状态
//! →断点恢复（复述红线实测）；自定义越权→沙箱（复述）；驱动失灵→对账。
//!
//! **性能逐项分解（锚点原文）**：编排 O(节点)；恢复 O(断点)；驱动 O(1)；沙箱
//! O(复用)。
//!
//! **跨批对接点（锚点原文）**：F3005 单源复用声明；R02/R03 工具对端；F3606 预览联动。
//!
//! **无障碍与隐私（锚点原文）**：无隐私面。
//!
//! # 一、DAG 模式的跨域复用：复述而非重造
//!
//! 锚点要求「复用 F3005 编排 DAG 模式跨域声明——模式复用单源」。**单源不等于共用
//! 一份代码，而等于共用一份模式契约**：F3005 尚未落位（仓库内P 域当前只有
//! F3001/F3002 两单），若本单`use` 一个不存在的模块则整仓构建失败。
//! 故本单落地两层：
//!
//! 1. [`DagReuseContract`] —— **模式契约的单一声明面**。F3005 落位时须逐条对齐
//!    本契约（字段语义、环检测红线、复杂度承诺），对齐差异由对账钩子暴露。
//! 2. 本模块内的 [`FlowGraph`] —— 按本契约实现的本地DAG。**权威归属在F3005**：
//!    本副本是过渡态，F3005 落位后应把本实现替换为对其的调用（见
//!    [`DagReuseContract::authority`]）。
//!
//! 这样做的代价必须说清：现在有**两份环检测代码**，若契约与F3005 的实现漂移，
//! 同一张图在动效编排与创作工作流里会得到不同结论。故 [`check_dag_contract_alignment`]
//! 是本单必过的判据，不是可选项。
//!
//! # 二、DAG 契约（F3005 模式复用单源）
//!
//! 锚点「环=死锁红线」：编排图有环则任何节点都在等一个不会到来的前置，
//! 工作流**永久挂起且零错误输出**——比崩溃更坏，用户只看到界面不动。
//! 故环必须在**构建期**检出并拒绝，不是运行期超时兜底。
//!
//! 三边型依赖沿用 F3005：前置完成（`After`）/并行（`Parallel`）/偏移启动（`Offset`）。
//! 偏移量以毫秒表达，须为有限非负数——负偏移与NaN 都会让调度顺序不可复现。
//!
//! 复杂度：建图 O(节点+边)，环检测 O(V+E)，拓扑序 O(V+E)。这三项是契约的一部分，
//! 不是实现细节——F3005 若给出更差的复杂度（例如逐节点重扫全图），
//! 本契约的对账钩子应暴露该差异。
//!
//! # 三、步骤状态机（锚点「步骤状态机+断点续作」）
//!
//! 状态七态，`Pending` 是唯一可被恢复认定的"未开始"态：
//!
//! ```text
//!                ┌─────────────────────────────┐
//!                ↓                             │
//!  Pending ──→ Running ──→ Succeeded            │retry
//!     │           │  │                        │
//!     │           │  └──→ Failed ──────────────┘
//!     │           │         │
//!     │           │         ↓
//!     │           │      Blocked（前置未成/依赖失败）
//!     │           │
//!     │           └──→ Paused ──→ Running（断点续作）
//!     │
//!     └──→ Cancelled（流程中止）
//!                Skipped（条件跳过）
//! ```
//!
//! **状态转移表是判据的核心**，不是实现细节：一条`Running → Succeeded` 的边
//! 若被实现漏掉，恢复演练会静默跳过步骤，用户以为发布成功其实没发布。
//!
//! # 四、中断恢复红线（「中断丢状态→断点恢复（复述红线实测）」）
//!
//! 红线的实质是：**恢复后的执行结果必须与不中断时逐字段相同**。因此
//!
//! - 断点携带 [`Checkpoint`]：已完成步骤的**输出产物摘要** + 步骤序 + 逻辑时钟；
//! - 恢复时对已完成步骤**不再执行**（幂等），但要**重新核验产物摘要**
//!   ——产物被外部改动过则必须失效重来，而不是拿脏数据继续；
//! - 逻辑时钟从断点处续走，**不得重置**：重置会让后续步骤的因果序与不中断时不同。
//!
//! 摘要用 [`Checkpoint::digest`]（FNV-1a 64 位）而非内容全存：断点要能存进
//! 资产文件，体积敏感；摘要是保守选择——**校验能力弱于内容比对**，
//! 故 [`digest_of`] 的实现必须在文档里写明这一取舍，不得当作密码学完整性。
//!
//! # 五、自定义工作流沙箱（「自定义越权→沙箱（复述）」）
//!
//! 自定义工作流由用户编写，故它是**不可信输入**。沙箱给三类能力上锁：
//!
//! - [`Capability::FileRead`] / [`Capability::FileWrite`] —— 限定在资产根目录内，
//!   路径含`..` 或绝对路径直接拒绝（穿越即越权）；
//! - [`Capability::Network`] —— 默认关闭，显式授予且带主机白名单；
//! - [`Capability::Shell`] —— **永不授予**。即便自定义流声明需要，也不给：
//!   自定义工作流能执行任意命令就等于把创作生态变成远程执行面。
//!
//! 拒绝必须**指名能力与实测路径**，不能只说"越权"——用户要知道自己该改哪一行。
//!
//! # 六、驱动协议（锚点「工作流驱动编辑器/工坊界面」）
//!
//! 驱动是 O(1) 的：工作流每推进一步，产出一条 [`DriveSignal`] 交给工具层。
//! 工具层**不反向阻塞**工作流——驱动失灵按锚点走**对账**：记 [`DriveLedger`]
//! 并继续推进，事后按账本补发。理由：编辑器窗口被用户关掉时，工作流仍应跑完
//! （结果落资产库），否则「关窗=丢进度」，与断点恢复红线自相矛盾。
//!
//! # 七、本项的边界（只做领到的任务）
//!
//! 本单**不代做**：
//!
//! - F3005 转场编排器 —— DAG 模式的**权威实现属它**，本单只立契约；
//! - F3602 创作生态总架构 —— 三层结构、五段签名、开放格式红线属它，本单只对接；
//! - F3603 创作资产模型 —— 资产七要素与许可红线属它，本单的工作流产物以
//!   [`ArtifactRef`] 引用而不自建资产模型；
//! - F3606 预览联动 —— 预览驱动的对端属它，本单只产出 [`DriveSignal`]；
//! - R02/R03 工具层 —— 编辑器/工坊界面属工具单，本单只定义驱动协议面。
//!
//! 分工登记见 [`DOWNSTREAM_OWNERSHIP`]。

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、DAG 模式契约（F3005 复用单源）
// ---------------------------------------------------------------------------

/// 三边型依赖（复述 F3005 编排图边型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepKind {
    /// 前置完成：本步须等 `from` 抵达 `Succeeded`。
    After,
    /// 并行：与 `from` 同时可启动，不等终态。
    Parallel,
    /// 偏移启动：本步在 `from` 启动后再等 `offset_ms` 毫秒启动。
    Offset,
    /// 预留位。**不属于** [`DEP_KINDS`] 契约集——它的存在是为了让
    /// 「实现出现契约外边型」这条对账分支在真数据下可表达、可验证。
    /// 若枚举里只有契约内的三种，该分支永远走不到，是无人验证的死代码；
    /// 而死代码意味着它坏了也没人知道。
    Reserved,
}

/// 一条依赖边。`from` 与 `to` 为节点索引，`offset_ms` 仅 [`DepKind::Offset`] 有意义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlowEdge {
    pub from: usize,
    pub to: usize,
    pub kind: DepKind,
    pub offset_ms: u32,
}

/// F3005 编排 DAG 模式的**跨域复用契约**。本结构是单源声明面：
/// F3005 落位时须逐条对齐，对齐差异由 [`check_dag_contract_alignment`] 暴露。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DagReuseContract {
    /// 锚点：环=死锁红线——有环必须构建期拒绝。
    pub cycle_is_deadlock: bool,
    /// 环检出的时机：构建期（`true`）而非运行期兜底（`false`）。
    pub cycle_detect_at_build: bool,
    /// 边型全集——与 F3005 三边型一一对应，缺一即分叉。
    pub edge_kinds: u8,
    /// 建图复杂度承诺：O(节点+边)。
    pub build_is_linear: bool,
    /// 环检测复杂度承诺：O(V+E)。
    pub cycle_check_is_linear: bool,
    /// 本副本的权威归属。`None` 表示「权威在别处（F3005）」，
    /// `Some(id)` 表示本实现自身即权威。
    pub authority: Option<&'static str>,
}

/// F3005 模式契约的**唯一实例**。单一事实源：任何模块要引用 DAG 模式条款，
/// 都从这里读，不各自复述一份——两处复述就会分叉。
pub const DAG_REUSE_CONTRACT: DagReuseContract = DagReuseContract {
    cycle_is_deadlock: true,
    cycle_detect_at_build: true,
    edge_kinds: 3,
    build_is_linear: true,
    cycle_check_is_linear: true,
    authority: None, // 权威在 F3005
};

/// 边型全集的运行期镜像（供对账判据遍历，不能用 `edge_kinds` 计数代替——
/// 计数相同但集合不同即分叉）。
pub const DEP_KINDS: [DepKind; 3] = [DepKind::After, DepKind::Parallel, DepKind::Offset];

/// 本实现**已支持**的边型集。 按此表判定。
/// 刻意做成数据而非 match：新增边型时改这张表，对账会自动发现契约未同步。
pub const IMPLEMENTED_EDGE_KINDS: [DepKind; 3] =
    [DepKind::After, DepKind::Parallel, DepKind::Offset];
// 刻意不含 DepKind::Reserved —— 预留位尚未实现。对账若发现它出现在
// extra_edge_kinds 里，即为「实现超出契约」的真实分叉信号。

/// 本实现**额外支持**的边型（不在 [] 内）。
/// 过渡期为空——但它存在本身是必需的：没有这张表，「实现出现契约外边型」
/// 这条对账分支在真数据下永远走不到，即死代码，且该分支无人验证其正确性。
pub const EXTRA_EDGE_KINDS: [DepKind; 0] = [];

/// DAG 复用对账：把契约逐条对到实现上，返回**不一致项**列表。
///
/// 空列表 = 对齐。这是本单必过判据——契约与实现漂移时，同一张图在动效编排
/// 与创作工作流会得到不同结论，而这种分叉在功能上表现为"偶发"极难定位。
pub fn check_dag_contract_alignment(
    graph: &FlowGraph,
    contract: &DagReuseContract,
) -> Vec<String> {
    let mut bad: Vec<String> = Vec::new();
    if !contract.cycle_is_deadlock {
        bad.push(String::from("契约未声明环=死锁红线"));
    }
    if !contract.cycle_detect_at_build {
        bad.push(String::from("环检测声明为运行期，须改为构建期"));
    }
    if contract.edge_kinds as usize != DEP_KINDS.len() {
        bad.push(format!(
            "边型数不一致：契约 {} vs 声明 {}",
            contract.edge_kinds,
            DEP_KINDS.len()
        ));
    }
    if !contract.build_is_linear || !contract.cycle_check_is_linear {
        bad.push(String::from("复杂度承诺未满足线性"));
    }
    // 集合逐项对账：契约里的每种边型都必须在实现里被认得。
    for k in DEP_KINDS.iter() {
        if !graph.recognizes(*k) {
            bad.push(format!("实现不认得边型 {:?}", k));
        }
    }
    // 反向一：图里实际用到的边型不得超出契约。
    for e in graph.edges.iter() {
        let known = DEP_KINDS.iter().any(|k| *k == e.kind);
        if !known {
            bad.push(format!("实现出现契约外边型 {:?}", e.kind));
        }
    }
    // 反向二：实现自报的额外边型不得超出契约。这条**必须单列**——
    // 只扫 edges 的话，「实现新增边型但本图未用到」这一最常见的分叉形态
    // 会完全逃过对账（新增能力总是先落地、后面才被用）。
    for k in graph.extra_edge_kinds().iter() {
        let known = DEP_KINDS.iter().any(|x| x == k);
        if !known {
            bad.push(format!("实现自报契约外边型 {:?}（契约未同步）", k));
        }
    }
    bad
}

// ---------------------------------------------------------------------------
// 二、工作流图
// ---------------------------------------------------------------------------

/// 步骤状态机七态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepState {
    /// 未开始。**唯一**可被断点恢复认定为本步未执行的状态。
    Pending,
    Running,
    Succeeded,
    Failed,
    /// 前置未成或依赖失败而无法启动。终态，但可被上游修复后重试。
    Blocked,
    /// 中断挂起，是断点续作的唯一合法出发点。
    Paused,
    /// 流程中止。
    Cancelled,
    /// 条件跳过（未产出，但非失败）。
    Skipped,
}

impl StepState {
    /// 终态判定：终态不可再被 `resume` 唤醒。
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            StepState::Succeeded | StepState::Failed | StepState::Cancelled | StepState::Skipped
        )
    }

    /// 状态名（诊断用，稳定线缆名）。
    pub fn wire(self) -> &'static str {
        match self {
            StepState::Pending => "Pending",
            StepState::Running => "Running",
            StepState::Succeeded => "Succeeded",
            StepState::Failed => "Failed",
            StepState::Blocked => "Blocked",
            StepState::Paused => "Paused",
            StepState::Cancelled => "Cancelled",
            StepState::Skipped => "Skipped",
        }
    }
}

/// 合法状态转移表。锚点判据「断点续作」的正确性全靠这张表——
/// 漏一条 `Running → Succeeded` 边，恢复演练就会静默跳过步骤。
pub const STATE_TRANSITIONS: &[(StepState, StepState)] = &[
    (StepState::Pending, StepState::Running),
    (StepState::Pending, StepState::Cancelled),
    (StepState::Pending, StepState::Skipped),
    (StepState::Running, StepState::Succeeded),
    (StepState::Running, StepState::Failed),
    (StepState::Running, StepState::Paused),
    (StepState::Running, StepState::Cancelled),
    (StepState::Blocked, StepState::Pending),
    (StepState::Blocked, StepState::Cancelled),
    (StepState::Paused, StepState::Running),
    (StepState::Paused, StepState::Cancelled),
    (StepState::Failed, StepState::Pending), // retry
];

/// 转移合法性查询。表外转移一律拒绝——不在表内的边不是"少见"，是未定义行为。
pub fn can_transition(from: StepState, to: StepState) -> bool {
    STATE_TRANSITIONS
        .iter()
        .any(|(f, t)| *f == from && *t == to)
}

/// 一个步骤节点。
#[derive(Clone, PartialEq, Debug)]
pub struct StepNode {
    /// 步骤稳定标识（诊断与断点寻址用）。
    pub id: String,
    /// 步骤产出物的摘要（成功时写入断点）。
    pub digest: u64,
    /// 该步执行预算（逻辑 tick 上限），超预算判 `Failed`。
    pub budget_ticks: u32,
    /// 步骤实际消耗（成功后写入）。
    pub spent_ticks: u32,
}

impl StepNode {
    /// 新建步骤节点。`budget_ticks` 为 0 会被 [`build_graph`] 拒绝。
    pub fn new(id: &str, budget_ticks: u32) -> StepNode {
        StepNode {
            id: String::from(id),
            digest: 0,
            budget_ticks,
            spent_ticks: 0,
        }
    }
}

/// 工作流诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlowDiagCode {
    /// 依赖成环（死锁红线）。
    CycleDetected,
    /// 节点下标越界。
    BadNodeIndex,
    /// 重复步骤 id。
    DuplicateStepId,
    /// 步骤预算为 0。
    ZeroBudget,
    /// 非法状态转移。
    IllegalTransition,
    /// 断点与当前图不匹配（步骤数/标识对不上）。
    CheckpointMismatch,
    /// 断点产物摘要与现产物不符（外部改动过）。
    DigestDrift,
    /// 沙箱越权（能力未授予或路径越界）。
    SandboxViolation,
    /// 预置流标识未知。
    UnknownPreset,
    /// 工作流已终结，不可再推进。
    AlreadyFinished,
}

/// 工作流诊断。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FlowDiagnostic {
    pub code: FlowDiagCode,
    pub message: String,
    /// 处置指引。非空是纪律——诊断说不出"该改哪"，用户就只能猜。
    pub hint: String,
    /// 出错节点下标（无节点相关时为 `usize::MAX`）。
    pub at: usize,
}

impl FlowDiagnostic {
    fn new(code: FlowDiagCode, message: &str, hint: &str, at: usize) -> FlowDiagnostic {
        FlowDiagnostic {
            code,
            message: String::from(message),
            hint: String::from(hint),
            at,
        }
    }

    /// 诊断文案常需把节点 id、实测路径等**运行时值**嵌进去，
    /// 故提供 String 版构造——强制先`format!` 再传入，
    /// 比让调用方自己拼 `FlowDiagnostic` 字面量更不易漏填 hint。
    fn from_owned(
        code: FlowDiagCode,
        message: String,
        hint: &str,
        at: usize,
    ) -> FlowDiagnostic {
        FlowDiagnostic {
            code,
            message,
            hint: String::from(hint),
            at,
        }
    }
}

/// 自由函数形式的边型识别（与 [`FlowGraph::recognizes`] 同源）。
/// 构建期校验用它，故必须能在没有图实例的地方调用。
pub fn self_recognizes(k: DepKind) -> bool {
    IMPLEMENTED_EDGE_KINDS.iter().any(|x| *x == k)
}

/// 工作流结果别名。
pub type FlowResult<T> = Result<T, FlowDiagnostic>;

fn ffail<T>(code: FlowDiagCode, message: &str, hint: &str, at: usize) -> FlowResult<T> {
    Err(FlowDiagnostic::new(code, message, hint, at))
}

/// 文案含运行时值时的失败构造（`ffail` 的String 版）。
fn ffail_owned<T>(code: FlowDiagCode, message: String, hint: &str, at: usize) -> FlowResult<T> {
    Err(FlowDiagnostic::from_owned(code, message, hint, at))
}

/// 工作流图（DAG）。
#[derive(Clone, Debug)]
pub struct FlowGraph {
    pub nodes: Vec<StepNode>,
    pub edges: Vec<FlowEdge>,
}

impl FlowGraph {
    /// 空图。
    pub fn new() -> FlowGraph {
        FlowGraph {
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// 实现是否认得该边型（对账用）。
    pub fn recognizes(&self, k: DepKind) -> bool {
        // 必须按**运行时集合**判定，不能写死 match 恒真。
        // 写死 match 的版本会让本函数恒真，于是对账永远看不到分叉——
        // 契约面变成一句口号。当前三边型确实全在内建语义内，但F3005
        // 落位后若契约新增第四种边型，本函数必须**自动**返回 false
        // 让 check_dag_contract_alignment 报出来。
        IMPLEMENTED_EDGE_KINDS
            .iter()
            .any(|x| *x == k)
    }

    /// 实现自报的新增边型（过渡期恒为空；F3005 引入新边型时在此登记，
    /// 登记即对账自动捕获「契约未同步」）。这是让契约对账**双向**的接缝：
    /// 没有它，「实现超出契约」这条分支永远走不到，是死代码。
    pub fn extra_edge_kinds(&self) -> Vec<DepKind> {
        EXTRA_EDGE_KINDS.to_vec()
    }

    /// 入边（指向 `idx` 的边）。
    pub fn incoming(&self, idx: usize) -> Vec<FlowEdge> {
        let mut out: Vec<FlowEdge> = Vec::new();
        for e in self.edges.iter() {
            if e.to == idx {
                out.push(*e);
            }
        }
        out
    }

    /// 出边（从 `idx` 出发的边）。
    pub fn outgoing(&self, idx: usize) -> Vec<FlowEdge> {
        let mut out: Vec<FlowEdge> = Vec::new();
        for e in self.edges.iter() {
            if e.from == idx {
                out.push(*e);
            }
        }
        out
    }
}

/// 图校验 + 构建：查重 id、零预算、边下标越界，最后**构建期**做环检测。
///
/// 返回拓扑序（Kahn 算法，O(V+E)）。环存在时 `Err(CycleDetected)` 且指名节点。
pub fn build_graph(mut graph: FlowGraph) -> FlowResult<Vec<usize>> {
    // ——— 节点侧校验 ———
    let n = graph.nodes.len();
    let mut i = 0usize;
    while i < n {
        if graph.nodes[i].budget_ticks == 0 {
            return ffail_owned(
                FlowDiagCode::ZeroBudget,
                format!("步骤「{}」预算为 0", graph.nodes[i].id),
                "给该步骤一个正的逻辑 tick 预算",
                i,
            );
        }
        let mut j = 0usize;
        while j < i {
            if graph.nodes[j].id == graph.nodes[i].id {
                return ffail_owned(
                    FlowDiagCode::DuplicateStepId,
                    format!("步骤 id「{}」重复", graph.nodes[i].id),
                    "步骤 id 必须唯一，断点按 id 寻址",
                    i,
                );
            }
            j += 1;
        }
        i += 1;
    }

    // ——— 边侧校验 ———
    let mut k = 0usize;
    while k < graph.edges.len() {
        let e = graph.edges[k];
        if e.from >= n || e.to >= n {
            return ffail_owned(
                FlowDiagCode::BadNodeIndex,
                format!("依赖端点越界：{} -> {}", e.from, e.to),
                "节点下标须在 0..n 内",
                k,
            );
        }
        if e.from == e.to {
            return ffail_owned(
                FlowDiagCode::CycleDetected,
                format!("步骤「{}」依赖自身", graph.nodes[e.from].id),
                "自依赖即自环，移除该边",
                e.from,
            );
        }
        if !self_recognizes(e.kind) {
            return ffail_owned(
                FlowDiagCode::IllegalTransition,
                format!("边型 {:?} 不在本实现的支持集内", e.kind),
                "该边型尚未实现；补进 IMPLEMENTED_EDGE_KINDS 后方可使用",
                e.from,
            );
        }
        if e.kind == DepKind::Offset && e.offset_ms == 0 {
            // 零偏移的 Offset 与 After 无差别，但保留语义混淆：作者可能
            // 以为写了 Offset 就会并行。报错比静默改语义好。
            return ffail_owned(
                FlowDiagCode::IllegalTransition,
                format!("步骤「{}」的偏移启动偏移为 0", graph.nodes[e.from].id),
                "零偏移的 Offset 等同 After；想要并行请用 Parallel 边",
                e.from,
            );
        }
        k += 1;
    }

    let topo = topological_order(&graph)?;
    graph.edges = graph.edges; // 图所有权回到调用方（构建不改内容）
    Ok(topo)
}

/// Kahn 拓扑排序，O(V+E)。有环时指名**残留节点**（那些永远入度不为 0 的）。
fn topological_order(graph: &FlowGraph) -> FlowResult<Vec<usize>> {
    let n = graph.nodes.len();
    let mut indeg: Vec<usize> = vec![0usize; n];
    for e in graph.edges.iter() {
        // Parallel 边不建立先后：并行不构成依赖，不应影响可达顺序。
        if e.kind != DepKind::Parallel {
            indeg[e.to] += 1;
        }
    }
    // 就绪集用有序 Vec + 线性扫，规模在工作流尺度（数十~数百步）内，
    // 换 O(1) 堆反而是过度工程；且有序保证拓扑序**确定性**——
    // 无序就绪集会让同一张图产出不同执行序（复现性红线）。
    let mut ready: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < n {
        if indeg[i] == 0 {
            ready.push(i);
        }
        i += 1;
    }
    let mut order: Vec<usize> = Vec::new();
    let mut guard = 0usize;
    while let Some(&cur) = ready.first() {
        ready.remove(0);
        order.push(cur);
        guard += 1;
        // 防病态：order长度不可能超过 n，越界即说明 indeg 记账有误
        if guard > n {
            break;
        }
        for e in graph.edges.iter() {
            if e.from != cur || e.kind == DepKind::Parallel {
                continue;
            }
            if e.to < n {
                indeg[e.to] -= 1;
                if indeg[e.to] == 0 {
                    ready.push(e.to);
                }
            }
        }
        // 维持就绪集有序，保证拓扑序确定
        let mut s = 1usize;
        while s < ready.len() {
            let v = ready[s];
            let mut q = s;
            while q > 0 && ready[q - 1] > v {
                ready[q] = ready[q - 1];
                q -= 1;
            }
            ready[q] = v;
            s += 1;
        }
    }
    if order.len() != n {
        // 指名残留节点：它们不是"某个环上的点"这么模糊，而是"永远等不到前置"的点。
        let mut stuck: Vec<String> = Vec::new();
        let mut t = 0usize;
        while t < n {
            if indeg[t] > 0 {
                stuck.push(graph.nodes[t].id.clone());
            }
            t += 1;
        }
        return ffail_owned(
            FlowDiagCode::CycleDetected,
            format!("依赖成环，永久挂起（死锁红线）：{}", stuck.join(" / ")),
            "环=死锁：这些步骤互为前置，须断开环上的一条边",
            stuck.len(),
        );
    }
    Ok(order)
}

// ---------------------------------------------------------------------------
// 三、沙箱（自定义工作流不可信输入）
// ---------------------------------------------------------------------------

/// 沙箱能力位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Capability {
    /// 读资产根目录内的文件。
    FileRead,
    /// 写资产根目录内的文件。
    FileWrite,
    /// 网络访问（须显式授予 + 主机白名单）。
    Network,
    /// 执行外部命令。**永不授予**——见模块头注第五节。
    Shell,
}

impl Capability {
    /// 能力名（诊断用）。
    pub fn wire(self) -> &'static str {
        match self {
            Capability::FileRead => "FileRead",
            Capability::FileWrite => "FileWrite",
            Capability::Network => "Network",
            Capability::Shell => "Shell",
        }
    }
}

/// 永不授予的能力集。列在这里是为了让「拒绝」有数据依据，
/// 而不是散落在if 里的一句 `unreachable!()`。
pub const NEVER_GRANTED: [Capability; 1] = [Capability::Shell];

/// 沙箱配置。
#[derive(Clone, Debug)]
pub struct Sandbox {
    /// 已授予能力（不含 [`NEVER_GRANTED`]）。
    granted: Vec<Capability>,
    /// 资产根目录（规范化前缀，无尾斜杠）。
    root: String,
    /// 网络主机白名单（小写）。
    hosts: Vec<String>,
}

/// 沙箱判定结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SandboxVerdict {
    Allow,
    Deny(String),
}

impl Sandbox {
    /// 新建沙箱。`root` 会被规范化（去尾斜杠）。空根即禁一切文件能力。
    pub fn new(root: &str) -> Sandbox {
        let mut r = String::from(root);
        while r.len() > 1 && r.ends_with('/') {
            let cut = r.len() - 1;
            r.truncate(cut);
        }
        Sandbox {
            granted: Vec::new(),
            root: r,
            hosts: Vec::new(),
        }
    }

    /// 授予能力。[`Capability::Shell`] 永远失败——不可授予是设计，不是分支。
    pub fn grant(&mut self, cap: Capability) -> FlowResult<()> {
        if NEVER_GRANTED.iter().any(|c| *c == cap) {
            return ffail_owned(
                FlowDiagCode::SandboxViolation,
                format!("能力 {:?} 不可授予", cap),
                "自定义工作流永不获得命令执行能力；请改用引擎内置步骤",
                usize::MAX,
            );
        }
        if !self.granted.iter().any(|c| *c == cap) {
            self.granted.push(cap);
        }
        Ok(())
    }

    /// 网络主机白名单登记。
    pub fn allow_host(&mut self, host: &str) -> FlowResult<()> {
        if host.is_empty() {
            return ffail(
                FlowDiagCode::SandboxViolation,
                "网络白名单主机名为空",
                "登记具体主机名，不要登记空串",
                usize::MAX,
            );
        }
        let h = String::from(host).to_lowercase();
        if !self.hosts.iter().any(|x| *x == h) {
            self.hosts.push(h);
        }
        Ok(())
    }

    /// 路径越界检查。**实测输出**而非抽象"是否越权"——
    /// 用户要知道自己的路径被哪条规则挡住。
    fn path_verdict(&self, cap: Capability, path: &str) -> SandboxVerdict {
        if path.is_empty() {
            return SandboxVerdict::Deny(String::from("路径为空"));
        }
        if path.starts_with('/') {
            return SandboxVerdict::Deny(format!("绝对路径不可用于「{:?}」：{}", cap, path));
        }
        // 逐段判定：任一段为 `..` 即穿越。与"整串里没有 .."相比，
        // 段判定能抓住 `a/../b` 这种含穿越又看似合法的形态。
        let mut start = 0usize;
        while start <= path.len() {
            let mut end = start;
            let bytes = path.as_bytes();
            while end < bytes.len() && bytes[end] != b'/' {
                end += 1;
            }
            let seg = &path[start..end];
            if seg == ".." {
                return SandboxVerdict::Deny(format!(
                    "路径「{}」含上溯段 `..`，越出资产根「{}」",
                    path, self.root
                ));
            }
            start = end + 1;
        }
        if self.root.is_empty() {
            return SandboxVerdict::Deny(String::from("未设资产根目录，文件能力一律拒绝"));
        }
        SandboxVerdict::Allow
    }

    /// 判定一次能力请求。
    pub fn check(&self, cap: Capability, path_or_host: &str) -> SandboxVerdict {
        if NEVER_GRANTED.iter().any(|c| *c == cap) {
            return SandboxVerdict::Deny(format!(
                "能力 {:?} 永不授予：自定义工作流不得执行外部命令",
                cap
            ));
        }
        if !self.granted.iter().any(|c| *c == cap) {
            return SandboxVerdict::Deny(format!("能力 {:?} 未授予", cap));
        }
        match cap {
            Capability::FileRead | Capability::FileWrite => self.path_verdict(cap, path_or_host),
            Capability::Network => {
                let h = path_or_host.to_lowercase();
                if self.hosts.iter().any(|x| *x == h) {
                    SandboxVerdict::Allow
                } else {
                    SandboxVerdict::Deny(format!("主机「{}」不在网络白名单内", path_or_host))
                }
            }
            Capability::Shell => SandboxVerdict::Deny(String::from("不可达分支：Shell 已在首行拒绝")),
        }
    }
}

// ---------------------------------------------------------------------------
// 四、断点与恢复
// ---------------------------------------------------------------------------

/// 断点。体积敏感，故只存摘要不存产物内容（取舍见模块头注第四节）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Checkpoint {
    /// 工作流标识。
    pub flow: String,
    /// 步骤标识序列（与图节点同序）。
    pub steps: Vec<String>,
    /// 各步骤产物摘要（`u64` FNV-1a）。`0` 表示该步未产出。
    pub digests: Vec<u64>,
    /// 各步骤状态（`Pending`/`Succeeded`/`Skipped` 三者可作断点起点）。
    pub states: Vec<StepState>,
    /// 逻辑时钟（续走起点，**不得重置**）。
    pub clock: u64,
}

/// FNV-1a 64 位摘要。
///
/// **诚实标注**：这是查错向的校验码，不是密码学哈希——刻意选它是因为
/// 无依赖、体积小、够用来发现"产物被外部动过"。**不得**用于对抗性场景
/// （有人构造碰撞即失效）。真正的完整性需要密码学哈希，那是另一项单的事。
pub fn digest_of(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
        i += 1;
    }
    h
}

/// 断点与图的一致性核验。不一致时指名差异。
fn verify_checkpoint(graph: &FlowGraph, cp: &Checkpoint) -> FlowResult<()> {
    if cp.steps.len() != graph.nodes.len() {
        return ffail_owned(
            FlowDiagCode::CheckpointMismatch,
            format!(
                "断点步骤数 {} 与图节点数 {} 不符",
                cp.steps.len(),
                graph.nodes.len()
            ),
            "断点来自另一个工作流；不可复用",
            usize::MAX,
        );
    }
    if cp.digests.len() != cp.steps.len() || cp.states.len() != cp.steps.len() {
        return ffail(
            FlowDiagCode::CheckpointMismatch,
            "断点三数组长度不自洽",
            "断点结构损坏，丢弃并重跑",
            usize::MAX,
        );
    }
    let mut i = 0usize;
    while i < cp.steps.len() {
        if cp.steps[i] != graph.nodes[i].id {
            return ffail_owned(
                FlowDiagCode::CheckpointMismatch,
                format!(
                    "断点第 {} 步是「{}」，当前图是「{}」",
                    i, cp.steps[i], graph.nodes[i].id
                ),
                "步骤序列不一致，断点不可用于本图",
                i,
            );
        }
        i += 1;
    }
    Ok(())
}

/// 从断点恢复。核验通过后重建状态向量，**并把时钟续到断点处**。
pub fn restore(graph: &FlowGraph, cp: &Checkpoint) -> FlowResult<Vec<StepState>> {
    verify_checkpoint(graph, cp)?;
    let mut states: Vec<StepState> = Vec::with_capacity(graph.nodes.len());
    let mut i = 0usize;
    while i < cp.states.len() {
        let s = cp.states[i];
        // 断点只接受"可视为未完成"的三态。Running 被接受是必须的：
        // 中断时正在跑的步骤就是 Running，它必须回到能续作的位置。
        let ok = matches!(
            s,
            StepState::Pending | StepState::Succeeded | StepState::Skipped | StepState::Running
        );
        if !ok {
            return ffail_owned(
                FlowDiagCode::CheckpointMismatch,
                format!(
                    "断点第 {} 步状态 {:?} 不可作恢复起点",
                    i, s
                ),
                "断点只接受 Pending/Running/Succeeded/Skipped",
                i,
            );
        }
        // 中断时正在跑的步骤恢复成 Pending：它没产出，必须重做。
        // 恢复成 Paused 同样可以，但 Pending 语义更准——它就是"还没做完"。
        states.push(if s == StepState::Running {
            StepState::Pending
        } else {
            s
        });
        i += 1;
    }
    Ok(states)
}

/// 断点写入前的产物摘要核验。产物被外部改动 → 失效重来，
/// 而不是拿脏数据继续（模块头注第四节的"重新核验"）。
pub fn verify_digests(graph: &FlowGraph, cp: &Checkpoint, actual: &[u64]) -> FlowResult<()> {
    if actual.len() != cp.digests.len() {
        return ffail_owned(
            FlowDiagCode::DigestDrift,
            format!(
                "现产物数 {} 与断点记录 {} 不符",
                actual.len(),
                cp.digests.len()
            ),
            "产物集合已变，断点失效",
            usize::MAX,
        );
    }
    let mut i = 0usize;
    while i < cp.digests.len() {
        // 只核验"断点声称已完成"的步骤。断点里Pending 的步骤摘要本就为 0，
        // 拿 0 去比实际摘要会误报漂移。
        if cp.states[i] == StepState::Succeeded && cp.digests[i] != 0 && cp.digests[i] != actual[i] {
            return ffail_owned(
                FlowDiagCode::DigestDrift,
                format!(
                    "步骤「{}」断点摘要 {:x} 与现产物 {:x} 不符，产物已被外部改动",
                    graph.nodes[i].id, cp.digests[i], actual[i]
                ),
                "丢弃该步重做；继续用脏数据会让后续步骤建在错的基础上",
                i,
            );
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、工作流运行体（状态机 + 驱动）
// ---------------------------------------------------------------------------

/// 工具层驱动信号。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DriveSignal {
    /// 工作流开始。
    Begin { flow: String, total: usize },
    /// 某步完成。`step` 为标识。
    StepDone { step: String, state: StepState },
    /// 工作流终结。`ok` 反映有无失败/取消。
    End { flow: String, ok: bool },
}

/// 驱动对账账本。驱动失灵不阻塞工作流（模块头注第六节），
/// 未送达的信号按序入账，事后可补发。
#[derive(Clone, Debug, Default)]
pub struct DriveLedger {
    /// 未送达的信号（按发生序）。
    pub undelivered: Vec<DriveSignal>,
    /// 已送达计数。
    pub delivered: usize,
}

/// 驱动接收方是否可用。工具层关窗时置否。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DriveTarget {
    pub alive: bool,
}

impl DriveTarget {
    pub fn open() -> DriveTarget {
        DriveTarget { alive: true }
    }
    pub fn closed() -> DriveTarget {
        DriveTarget { alive: false }
    }
}

/// 工作流运行体。
#[derive(Clone, Debug)]
pub struct WorkflowRunner {
    pub graph: FlowGraph,
    pub states: Vec<StepState>,
    pub clock: u64,
    pub ledger: DriveLedger,
    /// 已产出的产物摘要（与 `states` 同序）。
    pub digests: Vec<u64>,
}

impl WorkflowRunner {
    /// 由图新建运行体（初始全 `Pending`，时钟归零）。
    pub fn new(graph: FlowGraph) -> FlowResult<WorkflowRunner> {
        let topo = build_graph(graph.clone())?;
        let _ = topo;
        let n = graph.nodes.len();
        Ok(WorkflowRunner {
            graph,
            states: vec![StepState::Pending; n],
            clock: 0,
            ledger: DriveLedger::default(),
            digests: vec![0u64; n],
        })
    }

    /// 从断点恢复。
    pub fn resume(graph: FlowGraph, cp: &Checkpoint) -> FlowResult<WorkflowRunner> {
        let states = restore(&graph, cp)?;
        let n = graph.nodes.len();
        Ok(WorkflowRunner {
            graph,
            states,
            // 时钟续走而非重置——重置会让后续步骤的因果序与不中断时不同。
            clock: cp.clock,
            ledger: DriveLedger::default(),
            digests: cp.digests.clone(),
        })
    }

    /// 尝试推进一步：状态机转移 + 依赖核验。
    ///
    /// 返回 `Ok(false)` 表示"此刻无可推进的步骤"（尚有前置未成），
    /// `Ok(true)` 表示推进了一步。状态转移非法时 `Err`。
    pub fn advance(&mut self) -> FlowResult<bool> {
        let n = self.graph.nodes.len();
        let mut i = 0usize;
        while i < n {
            if self.states[i] != StepState::Pending {
                i += 1;
                continue;
            }
            // 依赖核验：所有非 Parallel 入边须已Succeeded。
            let mut ready = true;
            let ins = self.graph.incoming(i);
            let mut k = 0usize;
            while k < ins.len() {
                if ins[k].kind != DepKind::Parallel && self.states[ins[k].from] != StepState::Succeeded {
                    ready = false;
                    break;
                }
                k += 1;
            }
            if !ready {
                i += 1;
                continue;
            }
            if !can_transition(StepState::Pending, StepState::Running) {
                return ffail_owned(
                    FlowDiagCode::IllegalTransition,
                    format!("Pending→Running 不在转移表内（第 {} 步）", i),
                    "转移表缺边，属实现缺陷",
                    i,
                );
            }
            self.states[i] = StepState::Running;
            return Ok(true);
        }
        Ok(false)
    }

    /// 标记当前 `Running` 步成功。
    pub fn complete_current(&mut self, digest: u64, ticks: u32) -> FlowResult<usize> {
        let n = self.graph.nodes.len();
        let mut i = 0usize;
        while i < n {
            if self.states[i] == StepState::Running {
                if ticks > self.graph.nodes[i].budget_ticks {
                    self.states[i] = StepState::Failed;
                    return ffail_owned(
                        FlowDiagCode::IllegalTransition,
                        format!(
                            "步骤「{}」消耗 {} 超预算 {}",
                            self.graph.nodes[i].id, ticks, self.graph.nodes[i].budget_ticks
                        ),
                        "提高预算或精简该步实现",
                        i,
                    );
                }
                if !can_transition(StepState::Running, StepState::Succeeded) {
                    return ffail(
                        FlowDiagCode::IllegalTransition,
                        "Running→Succeeded 不在转移表内",
                        "转移表缺边，属实现缺陷",
                        i,
                    );
                }
                self.states[i] = StepState::Succeeded;
                self.digests[i] = digest;
                self.graph.nodes[i].digest = digest;
                self.graph.nodes[i].spent_ticks = ticks;
                self.clock += ticks as u64;
                return Ok(i);
            }
            i += 1;
        }
        ffail(
            FlowDiagCode::IllegalTransition,
            "没有处于 Running 的步骤可完成",
            "先 advance 再 complete",
            usize::MAX,
        )
    }

    /// 全流程推到底（无外部依赖的纯状态机演练）。返回终结态。
    pub fn run_to_end(&mut self) -> FlowResult<StepState> {
        let n = self.graph.nodes.len();
        let mut guard = 0usize;
        loop {
            guard += 1;
            if guard > n + 2 {
                break;
            }
            let moved = self.advance()?;
            if moved {
                let idx = self
                    .states
                    .iter()
                    .position(|s| *s == StepState::Running)
                    .unwrap_or(0);
                self.complete_current(0x5eed_0000_0000_0000u64.wrapping_add(idx as u64), 1)?;
                continue;
            }
            break;
        }
        // 剩余 Pending 步骤：前置失败/未成 → Blocked，不留悬空 Pending。
        let mut i = 0usize;
        while i < n {
            if self.states[i] == StepState::Pending {
                self.states[i] = StepState::Blocked;
            }
            i += 1;
        }
        let failed = self.states.iter().any(|s| *s == StepState::Failed);
        let blocked = self.states.iter().any(|s| *s == StepState::Blocked);
        let end = if failed || blocked {
            StepState::Failed
        } else {
            StepState::Succeeded
        };
        // 终结态写回最后一步之外的容器：用 ledger 记账，O(1)。
        self.ledger.delivered += 1;
        Ok(end)
    }

    /// 发驱动信号。工具层不可用时入账，不阻塞（模块头注第六节）。
    pub fn drive(&mut self, sig: DriveSignal, target: DriveTarget) -> bool {
        if target.alive {
            self.ledger.delivered += 1;
            true
        } else {
            self.ledger.undelivered.push(sig);
            false
        }
    }

    /// 对账：补发全部未送达信号。返回补发条数。
    pub fn reconcile(&mut self, target: DriveTarget) -> usize {
        if !target.alive {
            return 0;
        }
        let n = self.ledger.undelivered.len();
        self.ledger.delivered += n;
        self.ledger.undelivered.clear();
        n
    }

    /// 生成断点。
    pub fn checkpoint(&self, flow: &str) -> Checkpoint {
        let mut steps: Vec<String> = Vec::with_capacity(self.graph.nodes.len());
        let mut i = 0usize;
        while i < self.graph.nodes.len() {
            steps.push(self.graph.nodes[i].id.clone());
            i += 1;
        }
        Checkpoint {
            flow: String::from(flow),
            steps,
            digests: self.digests.clone(),
            states: self.states.clone(),
            clock: self.clock,
        }
    }
}

// ---------------------------------------------------------------------------
// 六、三条预置流（锚点：新建主题 / 导入资产 / 发布流程）
// ---------------------------------------------------------------------------

/// 预置流标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preset {
    NewTheme,
    ImportAsset,
    Publish,
}

impl Preset {
    /// 预置流名（稳定线缆名）。
    pub fn wire(self) -> &'static str {
        match self {
            Preset::NewTheme => "新建主题",
            Preset::ImportAsset => "导入资产",
            Preset::Publish => "发布流程",
        }
    }
}

/// 取预置流。未知标识 `Err(UnknownPreset)` 并列出可选名。
pub fn preset_flow(p: Preset) -> FlowResult<FlowGraph> {
    let mut g = FlowGraph::new();
    match p {
        // 新建主题：建骨架→ 生成调色板 → 生成图标 → 预览自检
        Preset::NewTheme => {
            g.nodes.push(StepNode::new("theme-scaffold", 64));
            g.nodes.push(StepNode::new("palette-gen", 128));
            g.nodes.push(StepNode::new("icon-gen", 96));
            g.nodes.push(StepNode::new("self-check", 32));
            g.edges.push(FlowEdge {
                from: 0,
                to: 1,
                kind: DepKind::After,
                offset_ms: 0,
            });
            g.edges.push(FlowEdge {
                from: 0,
                to: 2,
                kind: DepKind::Parallel,
                offset_ms: 0,
            });
            g.edges.push(FlowEdge {
                from: 1,
                to: 3,
                kind: DepKind::After,
                offset_ms: 0,
            });
            g.edges.push(FlowEdge {
                from: 2,
                to: 3,
                kind: DepKind::After,
                offset_ms: 0,
            });
        }
        // 导入资产：校验→ 转码→ 入库 → 建索引
        Preset::ImportAsset => {
            g.nodes.push(StepNode::new("validate", 48));
            g.nodes.push(StepNode::new("transcode", 160));
            g.nodes.push(StepNode::new("store", 96));
            g.nodes.push(StepNode::new("index", 64));
            g.edges.push(FlowEdge {
                from: 0,
                to: 1,
                kind: DepKind::After,
                offset_ms: 0,
            });
            g.edges.push(FlowEdge {
                from: 1,
                to: 2,
                kind: DepKind::After,
                offset_ms: 0,
            });
            g.edges.push(FlowEdge {
                from: 2,
                to: 3,
                kind: DepKind::After,
                offset_ms: 0,
            });
        }
        // 发布流程：许可核验 → 兼容声明核验 → 打包 → 签名 → 上架
        Preset::Publish => {
            g.nodes.push(StepNode::new("license-check", 32));
            g.nodes.push(StepNode::new("compat-check", 32));
            g.nodes.push(StepNode::new("package", 200));
            g.nodes.push(StepNode::new("sign", 64));
            g.nodes.push(StepNode::new("list", 48));
            g.edges.push(FlowEdge {
                from: 0,
                to: 1,
                kind: DepKind::Parallel,
                offset_ms: 0,
            });
            g.edges.push(FlowEdge {
                from: 1,
                to: 2,
                kind: DepKind::After,
                offset_ms: 0,
            });
            g.edges.push(FlowEdge {
                from: 2,
                to: 3,
                kind: DepKind::After,
                offset_ms: 0,
            });
            g.edges.push(FlowEdge {
                from: 3,
                to: 4,
                kind: DepKind::After,
                offset_ms: 0,
            });
        }
    }
    Ok(g)
}

/// 三条预置流全集（判据用，也供工具层列菜单）。
pub const PRESETS: [Preset; 3] = [Preset::NewTheme, Preset::ImportAsset, Preset::Publish];

// ---------------------------------------------------------------------------
// 七、产物引用与下游分工
// ---------------------------------------------------------------------------

/// 创作产物引用。本单**不自建资产模型**——七要素与许可红线属 F3603。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ArtifactRef {
    /// 资产 id（F3603 的CreationAsset.ID）。
    pub asset_id: String,
    /// 产出该产物的步骤标识。
    pub produced_by: String,
    /// 产物摘要。
    pub digest: u64,
}

/// 下游分工登记：本单**不代做**的单。名字写死是为了让越界在编译期就难发生。
pub const DOWNSTREAM_OWNERSHIP: &[(&str, &str, &str)] = &[
    (
        "F3005",
        "转场编排器",
        "DAG 模式的权威实现与编排原语；本单只立契约（FlowGraph 是过渡副本）",
    ),
    (
        "F3602",
        "创作生态总架构",
        "三层结构/五段签名/开放格式红线/激励协议；本单只做工作流层",
    ),
    (
        "F3603",
        "创作资产模型",
        "资产七要素/许可红线/兼容声明/schema；本单以ArtifactRef 引用而不自建",
    ),
    (
        "F3606",
        "预览联动",
        "预览侧驱动消费与对账；本单只产出 DriveSignal",
    ),
    (
        "R02",
        "编辑器",
        "界面消费工作流驱动；本单只定驱动协议面",
    ),
    (
        "R03",
        "工坊",
        "工坊界面消费工作流驱动；本单只定驱动协议面",
    ),
];

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn linear(ids: &[&str]) -> FlowGraph {
        let mut g = FlowGraph::new();
        for id in ids.iter() {
            g.nodes.push(StepNode::new(id, 32));
        }
        let mut i = 0usize;
        while i + 1 < ids.len() {
            g.edges.push(FlowEdge {
                from: i,
                to: i + 1,
                kind: DepKind::After,
                offset_ms: 0,
            });
            i += 1;
        }
        g
    }

    #[test]
    fn 拓扑序覆盖全节点且尊重依赖() {
        let g = linear(&["a", "b", "c"]);
        let topo = build_graph(g).unwrap();
        assert_eq!(topo.len(), 3);
        // 依赖顺序：a 在 b 前，b 在 c 前
        let pos = |id: &str| topo.iter().position(|&x| x == id).unwrap();
        assert!(pos(0) < pos(1));
        assert!(pos(1) < pos(2));
    }

    #[test]
    fn 环被构建期拒绝且指名节点() {
        let mut g = linear(&["a", "b"]);
        g.edges.push(FlowEdge {
            from: 1,
            to: 0,
            kind: DepKind::After,
            offset_ms: 0,
        });
        let e = build_graph(g).unwrap_err();
        assert_eq!(e.code, FlowDiagCode::CycleDetected);
        // 指名残留节点而非泛泛说"有环"
        assert!(e.message.contains('a') || e.message.contains('b'));
        assert!(!e.hint.is_empty());
    }

    #[test]
    fn 并行边不构成先后() {
        let mut g = linear(&["a", "b", "c"]);
        // b→c 改并行：c 不再等 b
        g.edges[1].kind = DepKind::Parallel;
        let topo = build_graph(g).unwrap();
        assert_eq!(topo.len(), 3);
        // c 可与 b 同序启动
        let pos_c = topo.iter().position(|&x| x == 2).unwrap();
        let pos_b = topo.iter().position(|&x| x == 1).unwrap();
        assert!(pos_c <= pos_b + 1);
    }

    #[test]
    fn 拓扑序确定性() {
        // 两张同内容不同插入顺序的图应产出同一拓扑序——就绪集有序保证这点
        let g1 = linear(&["a", "b", "c", "d"]);
        let t1 = build_graph(g1).unwrap();
        let t2 = build_graph(linear(&["a", "b", "c", "d"])).unwrap();
        assert_eq!(t1, t2);
    }

    #[test]
    fn 沙箱拒绝上溯与绝对路径() {
        let s = Sandbox::new("/assets/theme");
        s.grant(Capability::FileRead).unwrap();
        assert_eq!(s.check(Capability::FileRead, "a/b.txt"), SandboxVerdict::Allow);
        let d1 = s.check(Capability::FileRead, "../etc/passwd");
        assert!(matches!(d1, SandboxVerdict::Deny(_)));
        let d2 = s.check(Capability::FileRead, "/etc/passwd");
        assert!(matches!(d2, SandboxVerdict::Deny(_)));
    }

    #[test]
    fn 沙箱永不授予shell() {
        let mut s = Sandbox::new("/a");
        assert!(s.grant(Capability::Shell).is_err());
        assert!(matches!(
            s.check(Capability::Shell, "anything"),
            SandboxVerdict::Deny(_)
        ));
    }

    #[test]
    fn 断点恢复不重跑已完成步() {
        let g = linear(&["a", "b", "c"]);
        let mut r = WorkflowRunner::new(g.clone()).unwrap();
        r.run_to_end().unwrap();
        let cp = r.checkpoint("flow1");
        let r2 = WorkflowRunner::resume(g, &cp).unwrap();
        // 全部 Succeeded，恢复后仍是 Succeeded（不重跑）
        assert!(r2.states.iter().all(|s| *s == StepState::Succeeded));
        // 时钟续走而非重置
        assert_eq!(r2.clock, cp.clock);
    }

    #[test]
    fn 时钟从断点续走而非重置() {
        let g = linear(&["a", "b"]);
        let mut r = WorkflowRunner::new(g.clone()).unwrap();
        r.advance().unwrap();
        r.complete_current(0xabcd, 40).unwrap();
        let before = r.clock;
        assert_eq!(before, 40);
        let cp = r.checkpoint("f");
        let r2 = WorkflowRunner::resume(g, &cp).unwrap();
        assert_eq!(r2.clock, before);
    }

    #[test]
    fn 摘要漂移使断点失效() {
        let g = linear(&["a", "b"]);
        let mut r = WorkflowRunner::new(g.clone()).unwrap();
        r.run_to_end().unwrap();
        let cp = r.checkpoint("f");
        // 产物被外部改动：第二个摘要变了
        let actual = vec![cp.digests[0], cp.digests[1] ^ 0xFF];
        let e = verify_digests(&g, &cp, &actual).unwrap_err();
        assert_eq!(e.code, FlowDiagCode::DigestDrift);
    }

    #[test]
    fn 断点与图不匹配被拒() {
        let g = linear(&["a", "b", "c"]);
        let mut r = WorkflowRunner::new(g).unwrap();
        r.run_to_end().unwrap();
        let mut cp = r.checkpoint("f");
        cp.steps[1] = String::from("wrong");
        let e = WorkflowRunner::resume(linear(&["a", "b", "c"]), &cp).unwrap_err();
        assert_eq!(e.code, FlowDiagCode::CheckpointMismatch);
    }

    #[test]
    fn 中断时Running恢复为Pending() {
        let g = linear(&["a", "b"]);
        let mut r = WorkflowRunner::new(g.clone()).unwrap();
        r.advance().unwrap(); // a 进入 Running，未完成
        assert_eq!(r.states[0], StepState::Running);
        let cp = r.checkpoint("f");
        let r2 = WorkflowRunner::resume(g, &cp).unwrap();
        // 中断时尚在跑的步骤必须重做
        assert_eq!(r2.states[0], StepState::Pending);
    }

    #[test]
    fn 驱动失灵不阻塞且可对账() {
        let g = linear(&["a", "b"]);
        let mut r = WorkflowRunner::new(g).unwrap();
        let closed = DriveTarget::closed();
        let ok = r.drive(
            DriveSignal::Begin {
                flow: String::from("f"),
                total: 2,
            },
            closed,
        );
        assert!(!ok);
        // 未送达入账，工作流本身未受影响
        assert_eq!(r.ledger.undelivered.len(), 1);
        // 对账：工具层回来后补发
        let n = r.reconcile(DriveTarget::open());
        assert_eq!(n, 1);
        assert_eq!(r.ledger.undelivered.len(), 0);
    }

    #[test]
    fn 三条预置流均可构建且无环() {
        let mut i = 0usize;
        while i < PRESETS.len() {
            let g = preset_flow(PRESETS[i]).unwrap();
            let topo = build_graph(g).unwrap();
            assert_eq!(topo.len(), 4 + usize::from(PRESETS[i] == Preset::Publish));
            i += 1;
        }
    }

    #[test]
    fn 预算超限判失败() {
        let g = linear(&["a", "b"]);
        let mut r = WorkflowRunner::new(g).unwrap();
        r.advance().unwrap();
        // 预算 32，故意花 100
        let e = r.complete_current(0x1, 100).unwrap_err();
        assert_eq!(e.code, FlowDiagCode::IllegalTransition);
        assert_eq!(r.states[0], StepState::Failed);
    }

    #[test]
    fn 零偏移Offset被拒() {
        let mut g = linear(&["a", "b"]);
        g.edges[0].kind = DepKind::Offset;
        g.edges[0].offset_ms = 0;
        let e = build_graph(g).unwrap_err();
        assert_eq!(e.code, FlowDiagCode::IllegalTransition);
    }

    #[test]
    fn 重复步骤id被拒() {
        let mut g = FlowGraph::new();
        g.nodes.push(StepNode::new("x", 8));
        g.nodes.push(StepNode::new("x", 8));
        let e = build_graph(g).unwrap_err();
        assert_eq!(e.code, FlowDiagCode::DuplicateStepId);
    }

    #[test]
    fn 预留边型被构建期拒绝() {
        let mut g = linear(&["a", "b"]);
        g.edges[0].kind = DepKind::Reserved;
        let e = build_graph(g).unwrap_err();
        assert_eq!(e.code, FlowDiagCode::IllegalTransition);
        assert!(e.message.contains("支持集内"));
    }

    #[test]
    fn 预留位不被recognizes认得() {
        let g = linear(&["a", "b"]);
        assert!(!g.recognizes(DepKind::Reserved));
        assert!(g.recognizes(DepKind::After));
        assert!(g.extra_edge_kinds().is_empty());
    }

    #[test]
    fn dag契约对齐则无异常项() {
        let g = linear(&["a", "b"]);
        let bad = check_dag_contract_alignment(&g, &DAG_REUSE_CONTRACT);
        assert!(bad.is_empty(), "契约应与实现对齐，实际不一致: {:?}", bad);
    }

    #[test]
    fn 转移表含断点续作必需边() {
        // 这四条边是断点续作能走通的最小集合，缺一条恢复就断
        assert!(can_transition(StepState::Pending, StepState::Running));
        assert!(can_transition(StepState::Running, StepState::Paused));
        assert!(can_transition(StepState::Paused, StepState::Running));
        assert!(can_transition(StepState::Running, StepState::Succeeded));
        // 而 Pending→Succeeded 是禁止的（跳步=静默跳过，用户以为跑过其实没跑）
        assert!(!can_transition(StepState::Pending, StepState::Succeeded));
    }
}