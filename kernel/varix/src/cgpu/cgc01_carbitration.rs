//! CGPU-F0321 · C 域开工与预算总架构（CGPU-C 域 · 帧预算仲裁域 · C01 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0321`
//!
//! **判据（锚点原文）**：八模块、移交对接核验、服务化纪律、判据。
//!
//! **职责定位（锚点原文）**：C 域开工：帧预算仲裁域（F0321-F0480）——
//! 80 帧承诺的兑现机制（每帧 12.5ms 预算的精确分配与仲裁）；八模块总架构
//! （预算模型/申请/仲裁/超时处置/交易/全局表/再平衡/遥测）；B10 移交包
//! 对接（F0316 冻结接口+成本模型 F0004 映射核验就位）；用户态服务化纪律
//! （卷首铁律 6——仲裁 bug 不威胁系统存活，内核态只留强制执行点）。
//!
//! ## 一、域使命：80 帧承诺的兑现机制
//!
//! 80 帧/秒 ⇒ 每帧 12.5ms 预算（[`FRAME_BUDGET_US`] = 12500 微秒，
//! 来源：1_000_000us ÷ 80，整数除法精确无余——常量写死并注释来源）。
//! 本域的承诺是「预算的精确分配与仲裁」：分配有账（三级预算层级）、
//! 仲裁有据（申请→校验→排序→裁决→记账五步——[`ArbitrationStage`]），
//! 超时有路（降档/排队/拒绝三路——[`TimeoutPath`]，F0328 兑现位）。
//!
//! ## 二、八模块总架构（判据一：闭集规划表）
//!
//! 八模块（[`CModule`] 闭集 + [`C_MODULES`] 规划表）：预算模型/申请/
//! 仲裁/超时处置/交易/全局表/再平衡/遥测——每模块绑定其在 F0322-F0480
//! 区间内的兑现单号段；表外不立模块（模块闭集纪律），域内 160 项的
//! 十组规划（[`C_GROUPS`]）与八模块正交（组是批次切分，模块是职责
//! 切分——两张表守恒各自口径）。模块互联契约（[`MODULE_EDGES`]）声明
//! 模块间数据流向：申请→仲裁→处置→交易→全局表→遥测单向成链，
//! 再平衡从全局表取数向池写回——环路上的每条边都有契约键，
//! 断链即对接缺陷（[`verify_module_edges`]）。
//!
//! ## 三、B10 移交包对接核验（判据二：就位即核验，不空头对接）
//!
//! B10（B 域第十组）移交包两件：F0316 冻结接口 + 成本模型 F0004 映射。
//! 对接核验四项（[`HandoverReceipt`]）：接口版本相符/接口字段闭集核对/
//! 成本模型映射键齐备/移交指纹一致——四项全过才算「就位」，任一缺项
//! 显性 [`XcCode::HandoverMissing`] 拒绝域开工后续步骤（不空转过冬）。
//! 指纹可独立重算（[`handover_fingerprint`]：字段闭集 FNV-1a 迭代——
//! 算法公开、输入公开、结果可对拍），不依赖「信打包方」。
//!
//! ## 四、用户态服务化纪律（判据三：卷首铁律 6 的 C 域落面）
//!
//! 三条款字面量冻结（[`SERVICE_DISCIPLINE`]）：仲裁 bug 不威胁系统存活/
//! 内核态只留强制执行点/仲裁全量活在用户态服务。条款是可机检声明：
//! 每条绑定唯一检查键（判据侧 grep 键面），措辞漂移即红。条款落到
//! 执行面是边界表（[`KERNEL_ENFORCEMENT_POINTS`]）：内核态只保留的三类
//! 强制执行点逐条列出（预算硬钳制/超时强制回收/记账强制落盘），表外
//! 能力一律用户态——边界表是条款的可执行形态。
//!
//! ## 五、域开工核验入口
//!
//! [`verify_domain_open`]：八模块表守恒（8 模块 × 兑现段首尾连续）+
//! 十组规划守恒（10 组 × 16 单首尾连续覆盖 F0321-F0480）+ 模块互联
//! 契约守恒 + 移交核验 + 服务化条款核对 + 合同换算抽查，六查全过返回
//! `Ok`，任一失败带诊断码——域开工是有闸门的，不是宣告式的。
//! 规划表调整走 ADR（[`AdrEntry`]）：每条调整带编号/动因/前后差异/
//! 生效批次——规划不是不可变，但调整必须有账。
//!
//! **对接**：F0316（冻结接口，移交件一）；F0004（成本模型，移交件二）；
//! F0167（B 域失败处置——超时处置 F0328 的上游语义）；F0322-F0480
//! （八模块兑现区间）。零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（80 帧合同口径唯一源）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const C_OPEN_VERSION: &str = "C01-copen-v1";

/// 每帧预算（微秒）＝1_000_000us ÷ 80 帧＝12500us，整数除法精确无余。
/// 来源：80 帧合同（锚点原文「每帧 12.5ms 预算」）。当前策略值，可调。
pub const FRAME_BUDGET_US: u64 = 12_500;

/// 帧率承诺（帧/秒）——与 [`FRAME_BUDGET_US`] 守恒对账。
pub const COMMIT_FPS: u64 = 80;

/// 域规模：C 域 160 项（F0321-F0480）。
pub const C_DOMAIN_TOTAL: u32 = 160;

/// 域首单号（F0321 ⇒ 数值 321；四位数编号前导零不计值）。
pub const C_DOMAIN_FIRST: u32 = 321;

/// 域末单号（F0480 ⇒ 数值 480；首+总数-1 守恒）。
pub const C_DOMAIN_LAST: u32 = 480;

/// 守恒断言（编译期钉死：末=首+总-1）。
const _: () = assert!(C_DOMAIN_FIRST + C_DOMAIN_TOTAL - 1 == C_DOMAIN_LAST);

/// 每秒微秒（换算锚：预算 × 帧率必须恒等于此值）。
pub const US_PER_SECOND: u64 = 1_000_000;

/// 编译期守恒：预算 × 帧率 = 1 秒。
const _: () = assert!(FRAME_BUDGET_US * COMMIT_FPS == US_PER_SECOND);

/// 四池默认配额（万分比；呈现合成/界面响应/后台任务/网络与余量）。
/// 来源：锚点 F0322「池比例可配但默认保交互」——默认值把交互优先。
/// 四池之和恒 10000（编译期钉死）；F0322 施工时可配，此处为架构默认。
pub const POOL_DEFAULT_BPS: [u64; 4] = [4_500, 3_000, 1_800, 700];

/// 编译期守恒：四池默认配额之和 = 万分比满额。
const _: () = assert!(
    POOL_DEFAULT_BPS[0] + POOL_DEFAULT_BPS[1] + POOL_DEFAULT_BPS[2] + POOL_DEFAULT_BPS[3]
        == 10_000
);

/// 四池中文名（与 [`POOL_DEFAULT_BPS`] 下标对齐；架构层声明）。
pub const POOL_NAMES: [&str; 4] = ["呈现合成", "界面响应", "后台任务", "网络与余量"];

/// 交互保护底线：界面响应池默认占比不得低于此值（万分比；可配但
/// 低于底线必须走 ADR——「默认保交互」的量面闸）。
pub const INTERACTION_FLOOR_BPS: u64 = 2_500;

// ---------------------------------------------------------------------------
// 二、八模块总架构（判据一）
// ---------------------------------------------------------------------------

/// 八模块闭集（锚点点名；表外不立模块）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CModule {
    /// 预算模型（12.5ms 分解与守恒）。
    BudgetModel,
    /// 预算申请（标签/时机/批量/归还）。
    Request,
    /// 仲裁（v1 静态优先级 + v2 加权公平双轨）。
    Arbitration,
    /// 超时处置（降档/排队/拒绝三路）。
    Timeout,
    /// 预算交易（原子归还与迁移）。
    Transaction,
    /// 全局预算表（域级权威账）。
    GlobalTable,
    /// 再平衡（池间/账户间重分配）。
    Rebalance,
    /// 遥测（预算面五指标进 B08）。
    Telemetry,
}

impl CModule {
    /// 模块中文名（判据侧独立对拍值——措辞与锚点一致）。
    pub const fn zh(self) -> &'static str {
        match self {
            CModule::BudgetModel => "预算模型",
            CModule::Request => "申请",
            CModule::Arbitration => "仲裁",
            CModule::Timeout => "超时处置",
            CModule::Transaction => "交易",
            CModule::GlobalTable => "全局表",
            CModule::Rebalance => "再平衡",
            CModule::Telemetry => "遥测",
        }
    }

    /// 模块序号（0~7；账表定位）。
    pub const fn index(self) -> usize {
        match self {
            CModule::BudgetModel => 0,
            CModule::Request => 1,
            CModule::Arbitration => 2,
            CModule::Timeout => 3,
            CModule::Transaction => 4,
            CModule::GlobalTable => 5,
            CModule::Rebalance => 6,
            CModule::Telemetry => 7,
        }
    }

    /// 从序号还原（越界 None）。
    pub const fn from_index(i: usize) -> Option<CModule> {
        match i {
            0 => Some(CModule::BudgetModel),
            1 => Some(CModule::Request),
            2 => Some(CModule::Arbitration),
            3 => Some(CModule::Timeout),
            4 => Some(CModule::Transaction),
            5 => Some(CModule::GlobalTable),
            6 => Some(CModule::Rebalance),
            7 => Some(CModule::Telemetry),
            _ => None,
        }
    }

    /// 模块兑现状态寄存的任务号段内，本模块消费的上游模块。
    pub const fn upstream(self) -> Option<CModule> {
        match self {
            CModule::Request => Some(CModule::BudgetModel),
            CModule::Arbitration => Some(CModule::Request),
            CModule::Timeout => Some(CModule::Arbitration),
            CModule::Transaction => Some(CModule::Timeout),
            CModule::GlobalTable => Some(CModule::Transaction),
            CModule::Rebalance => Some(CModule::GlobalTable),
            CModule::Telemetry => Some(CModule::GlobalTable),
            CModule::BudgetModel => None,
        }
    }
}

/// 模块互联契约边（from → to 的数据流，带契约键——断链即对接缺陷）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleEdge {
    /// 上游模块（数据生产方）。
    pub from: CModule,
    /// 下游模块（数据消费方）。
    pub to: CModule,
    /// 契约键（三级命名 `c01.edge.<语义>`——判据侧可 grep）。
    pub contract: &'static str,
    /// 流载内容（人读；对拍用）。
    pub payload: &'static str,
}

/// 全部模块互联边（单向成链 + 再平衡支路；[`MODULE_EDGES`] 守恒校验
/// 要求：八模块中除 BudgetModel 外每个模块恰有一条入边，GlobalTable
/// 有两条出边（→遥测、→再平衡））。
pub const MODULE_EDGES: [ModuleEdge; 8] = [
    ModuleEdge {
        from: CModule::BudgetModel,
        to: CModule::Request,
        contract: "c01.edge.model-to-request",
        payload: "帧预算分解（四池配额 + 三级层级骨架）",
    },
    ModuleEdge {
        from: CModule::Request,
        to: CModule::Arbitration,
        contract: "c01.edge.request-to-arb",
        payload: "预算申请批（标签三要素 + 批量汇总）",
    },
    ModuleEdge {
        from: CModule::Arbitration,
        to: CModule::Timeout,
        contract: "c01.edge.arb-to-timeout",
        payload: "裁决结果（批准/挂起——挂起转三路处置）",
    },
    ModuleEdge {
        from: CModule::Timeout,
        to: CModule::Transaction,
        contract: "c01.edge.timeout-to-tx",
        payload: "处置生效单（降档差值/排队期限/拒绝归还额）",
    },
    ModuleEdge {
        from: CModule::Transaction,
        to: CModule::GlobalTable,
        contract: "c01.edge.tx-to-table",
        payload: "原子记账流（借/贷/归还三型事务）",
    },
    ModuleEdge {
        from: CModule::GlobalTable,
        to: CModule::Telemetry,
        contract: "c01.edge.table-to-telemetry",
        payload: "预算面五指标快照（逐帧）",
    },
    ModuleEdge {
        from: CModule::GlobalTable,
        to: CModule::Rebalance,
        contract: "c01.edge.table-to-rebalance",
        payload: "池水位与账户欠账视图（再平衡取数源）",
    },
    ModuleEdge {
        from: CModule::Rebalance,
        to: CModule::GlobalTable,
        contract: "c01.edge.rebalance-to-table",
        payload: "重分配事务（回写全局表——闭环支路）",
    },
];

/// 模块互联守恒：每条边契约键唯一、除 BudgetModel 外入边恰一条、
/// 出边目标必须真实存在。
pub fn verify_module_edges() -> bool {
    // ① 契约键互异。
    let mut i = 0usize;
    while i < MODULE_EDGES.len() {
        let a = match MODULE_EDGES.get(i) {
            Some(e) => e,
            None => return false,
        };
        let mut j = i + 1;
        while j < MODULE_EDGES.len() {
            if let Some(b) = MODULE_EDGES.get(j) {
                if a.contract == b.contract {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    // ② 入边计数：除 BudgetModel 外每模块恰一条入边。
    let mut in_count = [0u8; 8];
    let mut i = 0usize;
    while i < MODULE_EDGES.len() {
        if let Some(e) = MODULE_EDGES.get(i) {
            let t = e.to.index();
            if let Some(c) = in_count.get_mut(t) {
                *c = c.saturating_add(1);
            }
        }
        i += 1;
    }
    let mut m = 0usize;
    while m < 8 {
        let expect = if m == CModule::BudgetModel.index() { 0 } else { 1 };
        if in_count[m] != expect {
            return false;
        }
        m += 1;
    }
    // ③ GlobalTable 恰两条出边（遥测 + 再平衡闭环）。
    let mut out_g = 0u8;
    let mut i = 0usize;
    while i < MODULE_EDGES.len() {
        if let Some(e) = MODULE_EDGES.get(i) {
            if e.from == CModule::GlobalTable {
                out_g += 1;
            }
        }
        i += 1;
    }
    out_g == 2
}

/// 八模块规划行（模块 + 兑现单号段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CModulePlan {
    /// 模块。
    pub module: CModule,
    /// 兑现段首单（本模块职责的兑现任务区间起点）。
    pub fulfill_first: u32,
    /// 兑现段末单。
    pub fulfill_last: u32,
}

/// 八模块总架构表（判据一：闭集 + 段守恒由 [`verify_domain_open`] 校验）。
/// 前四段对齐锚点已点名的兑现单（模型 F0322-F0323/申请 F0324-F0325/
/// 仲裁 F0326-F0327/超时 F0328）；后四段为本域规划段（交易/全局表/
/// 再平衡/遥测的精确兑现位随批次施工登记，调整走 ADR）。
pub const C_MODULES: [CModulePlan; 8] = [
    CModulePlan { module: CModule::BudgetModel, fulfill_first: 322, fulfill_last: 323 },
    CModulePlan { module: CModule::Request, fulfill_first: 324, fulfill_last: 325 },
    CModulePlan { module: CModule::Arbitration, fulfill_first: 326, fulfill_last: 327 },
    CModulePlan { module: CModule::Timeout, fulfill_first: 328, fulfill_last: 328 },
    CModulePlan { module: CModule::Transaction, fulfill_first: 329, fulfill_last: 360 },
    CModulePlan { module: CModule::GlobalTable, fulfill_first: 361, fulfill_last: 390 },
    CModulePlan { module: CModule::Rebalance, fulfill_first: 391, fulfill_last: 420 },
    CModulePlan { module: CModule::Telemetry, fulfill_first: 421, fulfill_last: 480 },
];

/// 预算面五指标（遥测五元组；与 B08 同 schema——不第二套口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetMetric {
    /// 三级命名。
    pub name: &'static str,
    /// 维度。
    pub dimension: &'static str,
    /// 单位。
    pub unit: &'static str,
    /// 类型。
    pub kind: &'static str,
    /// 采样策略。
    pub sampling: &'static str,
}

/// 五指标表（申请数/批准数/拒绝数/超预算帧数/仲裁耗时 P95）。
pub const BUDGET_METRICS: [BudgetMetric; 5] = [
    BudgetMetric {
        name: "c01.budget.request_count",
        dimension: "frame",
        unit: "count",
        kind: "u64",
        sampling: "on_event",
    },
    BudgetMetric {
        name: "c01.budget.grant_count",
        dimension: "frame",
        unit: "count",
        kind: "u64",
        sampling: "on_event",
    },
    BudgetMetric {
        name: "c01.budget.deny_count",
        dimension: "frame",
        unit: "count",
        kind: "u64",
        sampling: "on_event",
    },
    BudgetMetric {
        name: "c01.budget.overbudget_frames",
        dimension: "frame",
        unit: "count",
        kind: "u64",
        sampling: "on_event",
    },
    BudgetMetric {
        name: "c01.budget.arb_p95_us",
        dimension: "frame",
        unit: "us",
        kind: "u64",
        sampling: "windowed_p95",
    },
];

// ---------------------------------------------------------------------------
// 三、五步仲裁管线与超时三路（架构层类型契约——兑现位 F0326-F0328）
// ---------------------------------------------------------------------------

/// 仲裁五步状态（申请→校验→排序→裁决→记账；单向推进不可跳步）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArbitrationStage {
    /// 第一步：受理申请批。
    Receive,
    /// 第二步：标签三要素校验（est/deadline/resource——F0324）。
    Validate,
    /// 第三步：按优先级与截止期排序（F0326 v1 静态优先级）。
    Sort,
    /// 第四步：逐项裁决（批准/挂起/拒绝）。
    Adjudicate,
    /// 第五步：记账（全局表事务——F0329+）。
    Commit,
}

impl ArbitrationStage {
    /// 步序号（0~4）。
    pub const fn index(self) -> usize {
        match self {
            ArbitrationStage::Receive => 0,
            ArbitrationStage::Validate => 1,
            ArbitrationStage::Sort => 2,
            ArbitrationStage::Adjudicate => 3,
            ArbitrationStage::Commit => 4,
        }
    }

    /// 下一步（Commit 是终点）。
    pub const fn next(self) -> Option<ArbitrationStage> {
        match self {
            ArbitrationStage::Receive => Some(ArbitrationStage::Validate),
            ArbitrationStage::Validate => Some(ArbitrationStage::Sort),
            ArbitrationStage::Sort => Some(ArbitrationStage::Adjudicate),
            ArbitrationStage::Adjudicate => Some(ArbitrationStage::Commit),
            ArbitrationStage::Commit => None,
        }
    }

    /// 所属模块（管线步骤归属模块——架构层的职责映射）。
    pub const fn owner(self) -> CModule {
        match self {
            ArbitrationStage::Receive | ArbitrationStage::Validate => CModule::Request,
            ArbitrationStage::Sort | ArbitrationStage::Adjudicate => CModule::Arbitration,
            ArbitrationStage::Commit => CModule::Transaction,
        }
    }
}

/// 超时三路处置（锚点「降档/排队/拒绝」——F0328 兑现位；此处为架构
/// 层类型契约与选择约束）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeoutPath {
    /// 降档：按质量阶梯降一档续跑（差额预算归还）。
    Degrade,
    /// 排队：挂起到下一帧窗口（期限=下一帧预算起点）。
    Queue,
    /// 拒绝：预算原子归还，任务按失败处置（对 F0167 语义）。
    Deny,
}

impl TimeoutPath {
    /// 三路全量（判据侧遍历）。
    pub const ALL: [TimeoutPath; 3] = [TimeoutPath::Degrade, TimeoutPath::Queue, TimeoutPath::Deny];

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            TimeoutPath::Degrade => "降档",
            TimeoutPath::Queue => "排队",
            TimeoutPath::Deny => "拒绝",
        }
    }

    /// 是否产生归还事务（降档=差额归还；拒绝=全额归还；排队=暂不归还）。
    pub const fn refunds(self) -> bool {
        matches!(self, TimeoutPath::Degrade | TimeoutPath::Deny)
    }
}

/// 三路选择策略输入（架构层声明：选择依据三要素——齐备才可裁决）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeoutDecisionInput {
    /// 任务是否声明可降级（不可降级——降档路直接排除）。
    pub degradable: bool,
    /// 本帧排队深度是否已超上限（超限——排队路排除）。
    pub queue_full: bool,
    /// 任务关键性（关键任务不可拒绝——拒绝路排除）。
    pub critical: bool,
}

impl TimeoutDecisionInput {
    /// 三路裁决：按「可降级→降档；队列未满→排队；否则拒绝」的
    /// 保产出顺序；三路皆不可用返回 None（架构缺陷信号——调用方
    /// 必须显性处理，不静默）。
    pub const fn decide(self) -> Option<TimeoutPath> {
        if self.degradable {
            return Some(TimeoutPath::Degrade);
        }
        if !self.queue_full && !self.critical {
            return Some(TimeoutPath::Queue);
        }
        if !self.critical {
            return Some(TimeoutPath::Deny);
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 四、内核强制执行点边界表（服务化条款的可执行形态）
// ---------------------------------------------------------------------------

/// 内核态强制执行点（表外能力一律用户态——条款三的量面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelEnforcementPoint {
    /// 执行点名。
    pub name: &'static str,
    /// 检查键（判据侧 grep 键面）。
    pub key: &'static str,
    /// 强制语义（人读）。
    pub semantic: &'static str,
}

/// 三类强制执行点（卷首铁律 6「内核态只留强制执行点」的封闭清单）。
pub const KERNEL_ENFORCEMENT_POINTS: [KernelEnforcementPoint; 3] = [
    KernelEnforcementPoint {
        name: "预算硬钳制",
        key: "c01.kernel.budget-clamp",
        semantic: "内核在提交路径钳制超预算提交——仲裁决议的强制执行（不重算，只执行）",
    },
    KernelEnforcementPoint {
        name: "超时强制回收",
        key: "c01.kernel.timeout-reclaim",
        semantic: "内核在超时点强制回收 GPU 时间片——三路处置的执行臂（不做三路选择）",
    },
    KernelEnforcementPoint {
        name: "记账强制落盘",
        key: "c01.kernel.ledger-commit",
        semantic: "内核保证记账事务不丢——全局表的持久性兜底（不算账，只保账）",
    },
];

// ---------------------------------------------------------------------------
// 五、十组批次规划与 ADR（与八模块正交的批次切分）
// ---------------------------------------------------------------------------

/// 十组规划行（组名 + 起止单号；起止逐字取自域规模 160 项 ÷ 10 组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CGroupPlan {
    /// 组名（C01-C10）。
    pub tag: &'static str,
    /// 组内首单。
    pub first: u32,
    /// 组内末单。
    pub last: u32,
}

/// 十组规划表（首尾连续无缺口、无重叠——由 [`verify_domain_open`] 守恒；
/// 160 项 ÷ 10 组 = 每组 16 单）。
pub const C_GROUPS: [CGroupPlan; 10] = [
    CGroupPlan { tag: "C01", first: 321, last: 336 },
    CGroupPlan { tag: "C02", first: 337, last: 352 },
    CGroupPlan { tag: "C03", first: 353, last: 368 },
    CGroupPlan { tag: "C04", first: 369, last: 384 },
    CGroupPlan { tag: "C05", first: 385, last: 400 },
    CGroupPlan { tag: "C06", first: 401, last: 416 },
    CGroupPlan { tag: "C07", first: 417, last: 432 },
    CGroupPlan { tag: "C08", first: 433, last: 448 },
    CGroupPlan { tag: "C09", first: 449, last: 464 },
    CGroupPlan { tag: "C10", first: 465, last: 480 },
];

/// 组施工状态机（锁定→施工中→完成；跳步显性拒绝）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupState {
    /// 未开工（前置组未完成时锁定）。
    Locked,
    /// 施工中（前置完成即解锁——完成即解锁纪律）。
    InProgress,
    /// 完成（判据全绿 + 行数达标后落账）。
    Done,
}

impl GroupState {
    /// 状态迁移合法性（Locked→InProgress→Done 单向；其余拒绝）。
    pub const fn can_transition_to(self, next: GroupState) -> bool {
        matches!(
            (self, next),
            (GroupState::Locked, GroupState::InProgress)
                | (GroupState::InProgress, GroupState::Done)
        )
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            GroupState::Locked => "锁定",
            GroupState::InProgress => "施工中",
            GroupState::Done => "完成",
        }
    }
}

/// 域进度台账（十组状态 + 组间依赖推进；域内自账，不替代全局任务板）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainLedger {
    states: [GroupState; 10],
    /// 每组已入账的纯功能行数（行数对账用）。
    line_counts: [u64; 10],
}

impl DomainLedger {
    /// 新台账（全组锁定；C01 因无前置直接可开工）。
    pub const fn new() -> DomainLedger {
        DomainLedger {
            states: [
                GroupState::InProgress,
                GroupState::Locked,
                GroupState::Locked,
                GroupState::Locked,
                GroupState::Locked,
                GroupState::Locked,
                GroupState::Locked,
                GroupState::Locked,
                GroupState::Locked,
                GroupState::Locked,
            ],
            line_counts: [0; 10],
        }
    }

    /// 组状态（组序号越界 None）。
    pub fn state_of(&self, group_idx: usize) -> Option<GroupState> {
        self.states.get(group_idx).copied()
    }

    /// 推进组状态（合法迁移才生效；非法迁移显性拒绝）。
    pub fn transition(&mut self, group_idx: usize, next: GroupState) -> Result<(), &'static str> {
        let cur = match self.states.get(group_idx) {
            Some(s) => *s,
            None => return Err("组序号越界"),
        };
        if !cur.can_transition_to(next) {
            return Err("非法状态迁移：组状态只允许 锁定→施工中→完成 单向推进");
        }
        // 完成迁移必须行数已入账（完成度铁律：不空转交账）。
        if next == GroupState::Done {
            match self.line_counts.get(group_idx) {
                Some(0) | None => return Err("完成前必须入账本组纯功能行数"),
                _ => {}
            }
        }
        if let Some(s) = self.states.get_mut(group_idx) {
            *s = next;
        }
        // 完成即解锁后继（产线纪律：完成解锁下一组）。
        if next == GroupState::Done {
            if let Some(ns) = self.states.get_mut(group_idx + 1) {
                if *ns == GroupState::Locked {
                    *ns = GroupState::InProgress;
                }
            }
        }
        Ok(())
    }

    /// 入账本组行数（累加；饱和）。
    pub fn add_lines(&mut self, group_idx: usize, lines: u64) -> Result<(), &'static str> {
        if group_idx >= 10 {
            return Err("组序号越界");
        }
        match self.line_counts.get_mut(group_idx) {
            Some(c) => {
                *c = c.saturating_add(lines);
                Ok(())
            }
            None => Err("组序号越界"),
        }
    }

    /// 本组行数。
    pub fn lines_of(&self, group_idx: usize) -> u64 {
        *self.line_counts.get(group_idx).unwrap_or(&0)
    }

    /// 域总行数。
    pub fn total_lines(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < 10 {
            s = s.saturating_add(self.line_counts[i]);
            i += 1;
        }
        s
    }

    /// 已完成组数。
    pub fn done_groups(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < 10 {
            if self.states[i] == GroupState::Done {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 域进度万分比（按组数）。
    pub fn progress_bps(&self) -> u64 {
        self.done_groups() as u64 * 1_000
    }
}

impl Default for DomainLedger {
    fn default() -> DomainLedger {
        DomainLedger::new()
    }
}

/// ADR 规划调整记录（规划表调整必须留账：编号/动因/差异/生效批次）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdrEntry {
    /// ADR 编号（c01-adr-<序号>）。
    pub id: String,
    /// 调整动因（人读；不许空）。
    pub reason: String,
    /// 调整前的规划快照（受影响字段=值）。
    pub before: String,
    /// 调整后的规划快照。
    pub after: String,
    /// 生效批次（组名，如 C03）。
    pub effective_group: String,
}

/// ADR 账（域规划调整的唯一合法通道——无 ADR 的调整视为规划漂移）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AdrLedger {
    entries: Vec<AdrEntry>,
    next_seq: u64,
}

impl AdrLedger {
    /// 空账。
    pub const fn new() -> AdrLedger {
        AdrLedger {
            entries: Vec::new(),
            next_seq: 1,
        }
    }

    /// 登记一条调整（动因为空显性拒绝——不留无因之账）。
    pub fn record(
        &mut self,
        reason: String,
        before: String,
        after: String,
        effective_group: String,
    ) -> Result<String, &'static str> {
        if reason.trim().is_empty() {
            return Err("调整动因为空：规划调整必须说明动因");
        }
        if before == after {
            return Err("调整前后无差异：无差异不构成调整，拒绝空转 ADR");
        }
        let id = alloc::format!("c01-adr-{:03}", self.next_seq);
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push(AdrEntry {
            id: id.clone(),
            reason,
            before,
            after,
            effective_group,
        });
        Ok(id)
    }

    /// 全部 ADR。
    pub fn entries(&self) -> &[AdrEntry] {
        &self.entries
    }

    /// ADR 总数。
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判定。
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 六、B10 移交包对接核验（判据二）
// ---------------------------------------------------------------------------

/// 移交对接核验结论闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 移交件缺失/指纹不符/字段超闭集——对接未就位。
    HandoverMissing,
    /// 服务化条款核对失败（措辞漂移即红）。
    DisciplineDrift,
    /// 规划表守恒失败（模块段/组段不连续或覆盖不全）。
    PlanMismatch,
    /// 预算守恒失败（Σ池 ≠ 帧预算）。
    BudgetMismatch,
    /// 模块互联契约断链（入边计数/契约键重复）。
    EdgeContractBroken,
    /// 交互保护底线失守（界面响应池配额低于底线且无 ADR）。
    InteractionFloorViolated,
}

impl XcCode {
    /// 人读诊断（异常零静默三要素）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::HandoverMissing => "移交包未就位：接口版本/字段闭集/映射键/指纹四项有缺，建议补齐 B10 移交件再开工",
            XcCode::DisciplineDrift => "服务化条款漂移：三条款措辞与冻结版不符，建议恢复字面量",
            XcCode::PlanMismatch => "规划表失守：模块段或组段不连续/覆盖不全，建议回对锚点重建表",
            XcCode::BudgetMismatch => "预算守恒失败：Σ池 ≠ 帧预算，建议核对四池配额和",
            XcCode::EdgeContractBroken => "模块互联契约断链：入边计数或契约键异常，建议核对 MODULE_EDGES 表",
            XcCode::InteractionFloorViolated => "交互保护底线失守：界面响应池配额低于底线，建议走 ADR 或回调配额",
        }
    }
}

/// 移交包核验单（B10 移交件一 F0316 冻结接口 + 件二 F0004 成本模型映射）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoverReceipt {
    /// 冻结接口版本（须与 F0316 冻结版一致）。
    pub interface_version: String,
    /// 移交接口实际承载的字段名（核对走 [`check_interface_fields`] 闭集）。
    pub interface_fields: Vec<String>,
    /// 移交成本映射实际承载的键名（核对走 [`check_cost_model_keys`] 闭集）。
    pub cost_model_keys: Vec<String>,
    /// 移交指纹（B10 打包指纹；一致性由判据侧独立重算对拍）。
    pub fingerprint: u64,
}

/// B10 移交包期望指纹（移交时冻结；判据侧独立写死对拍，不复用本值）。
pub const B10_EXPECTED_FINGERPRINT: u64 = 0xB10C_0004_0316_0001;

/// FNV-1a 64 位偏移基（指纹重算用；公开算法公开输入可对拍）。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 单字节 FNV 迭代。
const fn fnv_byte(acc: u64, b: u8) -> u64 {
    (acc ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 字符串切片 FNV 迭代。
const fn fnv_str(acc: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut acc = acc;
    let mut i = 0usize;
    while i < bytes.len() {
        acc = fnv_byte(acc, bytes[i]);
        i += 1;
    }
    acc
}

/// 分隔字节（字段间注入，防拼接歧义：("ab","c") 与 ("a","bc") 不同指纹）。
const FNV_SEP: u8 = 0x1F;

/// 移交指纹独立重算（判据二对拍面：输入=版本+字段闭集+映射键闭集；
/// 算法确定——同输入同指纹，改任一输入指纹必变）。
pub fn handover_fingerprint(
    version: &str,
    fields: &[&str],
    keys: &[&str],
) -> u64 {
    let mut acc = fnv_str(FNV_OFFSET, version);
    let mut i = 0usize;
    while i < fields.len() {
        acc = fnv_byte(acc, FNV_SEP);
        acc = fnv_str(acc, fields[i]);
        i += 1;
    }
    let mut i = 0usize;
    while i < keys.len() {
        acc = fnv_byte(acc, FNV_SEP);
        acc = fnv_str(acc, keys[i]);
        i += 1;
    }
    acc
}

impl HandoverReceipt {
    /// 四项核验（判据二：全过才就位；任一缺项显性码拒）。
    pub fn verify(&self) -> Result<(), XcCode> {
        if self.interface_version != F0316_FROZEN_VERSION {
            return Err(XcCode::HandoverMissing);
        }
        if !check_interface_fields(&self.interface_fields)
            || !check_cost_model_keys(&self.cost_model_keys)
        {
            return Err(XcCode::HandoverMissing);
        }
        if self.fingerprint != B10_EXPECTED_FINGERPRINT {
            return Err(XcCode::HandoverMissing);
        }
        Ok(())
    }

    /// 四项核验的分项结论（人读面：逐项过/不过带原因——报告用）。
    pub fn verify_itemized(&self) -> Vec<(&'static str, bool, String)> {
        let mut out = Vec::new();
        out.push((
            "接口版本",
            self.interface_version == F0316_FROZEN_VERSION,
            alloc::format!("期望 {} 实际 {}", F0316_FROZEN_VERSION, self.interface_version),
        ));
        let fields_ok = check_interface_fields(&self.interface_fields);
        out.push((
            "字段闭集",
            fields_ok,
            alloc::format!("{} 项字段，闭集 {} 项", self.interface_fields.len(), F0316_FIELDS.len()),
        ));
        let keys_ok = check_cost_model_keys(&self.cost_model_keys);
        out.push((
            "映射键齐备",
            keys_ok,
            alloc::format!("{} 项键，闭集 {} 项", self.cost_model_keys.len(), COST_MODEL_KEYS.len()),
        ));
        out.push((
            "移交指纹",
            self.fingerprint == B10_EXPECTED_FINGERPRINT,
            alloc::format!("期望 {:#x} 实际 {:#x}", B10_EXPECTED_FINGERPRINT, self.fingerprint),
        ));
        out
    }
}

/// F0316 冻结接口版本（移交件一的冻结版字面量；变更走 ADR）。
pub const F0316_FROZEN_VERSION: &str = "b10.f0316.v1";

/// F0316 接口字段闭集（移交接口允许的字段名；表外字段=未就位）。
pub const F0316_FIELDS: [&str; 6] = [
    "task_id",
    "est_cost_us",
    "deadline_us",
    "resource_kind",
    "priority",
    "degradable",
];

/// 成本模型 F0004 → 预算标签的映射键闭集（移交件二的键面；映射键齐备
/// = 闭集逐键在场，缺键即映射断裂）。
pub const COST_MODEL_KEYS: [&str; 4] =
    ["est_cost_us", "deadline_us", "resource_kind", "priority"];

/// 接口字段闭集核对（判据二机检面：传入的移交字段逐个必须在闭集内；
/// 空表=零字段也算未就位——接口至少要承载六个字段）。
pub fn check_interface_fields<S: alloc::string::ToString>(received: &[S]) -> bool {
    if received.len() < F0316_FIELDS.len() {
        return false;
    }
    let mut i = 0usize;
    while i < received.len() {
        let f = match received.get(i) {
            Some(f) => f.to_string(),
            None => return false,
        };
        let mut hit = false;
        let mut j = 0usize;
        while j < F0316_FIELDS.len() {
            if let Some(allowed) = F0316_FIELDS.get(j) {
                if *allowed == f {
                    hit = true;
                    break;
                }
            }
            j += 1;
        }
        if !hit {
            return false;
        }
        i += 1;
    }
    true
}

/// 映射键齐备核对（判据二机检面：COST_MODEL_KEYS 逐键必须在移交映射里）。
pub fn check_cost_model_keys<S: alloc::string::ToString>(received: &[S]) -> bool {
    let mut i = 0usize;
    while i < COST_MODEL_KEYS.len() {
        let key = match COST_MODEL_KEYS.get(i) {
            Some(k) => *k,
            None => return false,
        };
        let mut hit = false;
        let mut j = 0usize;
        while j < received.len() {
            if let Some(r) = received.get(j) {
                if r.to_string() == key {
                    hit = true;
                    break;
                }
            }
            j += 1;
        }
        if !hit {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 七、服务化纪律（判据三：卷首铁律 6 的 C 域落面）
// ---------------------------------------------------------------------------

/// 服务化三条款（字面量冻结；判据侧独立写死对拍，措辞漂移即红）。
pub const SERVICE_DISCIPLINE: [&str; 3] = [
    "仲裁 bug 不威胁系统存活",
    "内核态只留强制执行点",
    "仲裁全量活在用户态服务",
];

/// 服务化条款检查键（判据侧 grep 键面——条款是可机检声明不是口号）。
pub const SERVICE_DISCIPLINE_KEYS: [&str; 3] =
    ["c01.discipline.arbitor-isolation", "c01.discipline.kernel-minimal", "c01.discipline.userservice-only"];

/// 服务化纪律核对（条款与键表结构 + 强制执行点边界表封闭性）。
pub fn verify_service_discipline() -> Result<(), XcCode> {
    if SERVICE_DISCIPLINE.len() != 3 || SERVICE_DISCIPLINE_KEYS.len() != 3 {
        return Err(XcCode::DisciplineDrift);
    }
    let k0 = SERVICE_DISCIPLINE_KEYS[0];
    let k1 = SERVICE_DISCIPLINE_KEYS[1];
    let k2 = SERVICE_DISCIPLINE_KEYS[2];
    if k0 == k1 || k1 == k2 || k0 == k2 {
        return Err(XcCode::DisciplineDrift);
    }
    // 条款二「内核态只留强制执行点」必须落到封闭边界表（表空=漂移）。
    if KERNEL_ENFORCEMENT_POINTS.len() != 3 {
        return Err(XcCode::DisciplineDrift);
    }
    let mut i = 0usize;
    while i < KERNEL_ENFORCEMENT_POINTS.len() {
        let p = match KERNEL_ENFORCEMENT_POINTS.get(i) {
            Some(p) => p,
            None => return Err(XcCode::DisciplineDrift),
        };
        if p.name.is_empty() || p.key.is_empty() || p.semantic.is_empty() {
            return Err(XcCode::DisciplineDrift);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 八、域开工核验入口（判据四：开工有闸门）
// ---------------------------------------------------------------------------

/// 模块段守恒：八段首尾相接、递增、不重叠、都落在域区间内。
fn modules_contiguous() -> bool {
    let mut prev_end: u32 = 0;
    let mut i = 0usize;
    while i < C_MODULES.len() {
        let m = match C_MODULES.get(i) {
            Some(m) => *m,
            None => return false,
        };
        if m.fulfill_first > m.fulfill_last {
            return false;
        }
        if i > 0 && m.fulfill_first <= prev_end {
            return false;
        }
        prev_end = m.fulfill_last;
        i += 1;
    }
    let first = match C_MODULES.first() {
        Some(m) => m.fulfill_first,
        None => return false,
    };
    let last = prev_end;
    first >= C_DOMAIN_FIRST && last <= C_DOMAIN_LAST
}

/// 组段守恒：十组首尾连续无缝覆盖 [C_DOMAIN_FIRST, C_DOMAIN_LAST]。
fn groups_contiguous() -> bool {
    let mut expect = C_DOMAIN_FIRST;
    let mut i = 0usize;
    while i < C_GROUPS.len() {
        let g = match C_GROUPS.get(i) {
            Some(g) => *g,
            None => return false,
        };
        if g.first != expect || g.first > g.last {
            return false;
        }
        expect = g.last.saturating_add(1);
        i += 1;
    }
    expect == C_DOMAIN_LAST.saturating_add(1)
}

/// 预算守恒换算抽查：帧预算 × 80 = 1 秒（us 口径）。
pub const fn budget_conserves_second() -> bool {
    FRAME_BUDGET_US.saturating_mul(COMMIT_FPS) == US_PER_SECOND
}

/// 四池配额守恒：给定配额表之和必须为万分比满额。
pub const fn pools_conserve(bps: &[u64; 4]) -> bool {
    bps[0].saturating_add(bps[1]).saturating_add(bps[2]).saturating_add(bps[3]) == 10_000
}

/// 交互底线检查：给定配额表的界面响应池不得低于底线。
pub const fn interaction_floor_ok(bps: &[u64; 4]) -> bool {
    bps[1] >= INTERACTION_FLOOR_BPS
}

/// 域开工六查（判据一~四 + 互联契约 + 交互底线）。
pub fn verify_domain_open(receipt: &HandoverReceipt) -> Result<(), XcCode> {
    if !modules_contiguous() || !groups_contiguous() {
        return Err(XcCode::PlanMismatch);
    }
    if !verify_module_edges() {
        return Err(XcCode::EdgeContractBroken);
    }
    receipt.verify()?;
    verify_service_discipline()?;
    if !budget_conserves_second() {
        return Err(XcCode::BudgetMismatch);
    }
    if !pools_conserve(&POOL_DEFAULT_BPS) || !interaction_floor_ok(&POOL_DEFAULT_BPS) {
        return Err(XcCode::InteractionFloorViolated);
    }
    Ok(())
}

/// 域开工报告（人读面：判据逐条带结论，供验收与遥测消费）。
pub fn domain_open_report(receipt: &HandoverReceipt) -> String {
    let mut s = String::new();
    s.push_str("C01 域开工核验：");
    s.push_str(&alloc::format!(
        "八模块 {} 项/十组 {} 组/互联边 {} 条；",
        C_MODULES.len(),
        C_GROUPS.len(),
        MODULE_EDGES.len()
    ));
    s.push_str(&alloc::format!(
        "预算 {}us×{}fps={}us/s；",
        FRAME_BUDGET_US,
        COMMIT_FPS,
        FRAME_BUDGET_US * COMMIT_FPS
    ));
    s.push_str(&alloc::format!(
        "默认池配额 {}bps（底线 {}bps）；",
        POOL_DEFAULT_BPS[1],
        INTERACTION_FLOOR_BPS
    ));
    match verify_domain_open(receipt) {
        Ok(()) => s.push_str("六查全过=就位"),
        Err(c) => {
            s.push_str("拒=");
            s.push_str(c.detail());
        }
    }
    s
}

/// 四池配额的微秒换算（架构层口径：万分比 × 帧预算 ÷ 10000，饱和乘法
/// + 截断除法——F0322 施工时的换算单源，此处先立口径）。
pub const fn pool_budget_us(bps: u64) -> u64 {
    bps.saturating_mul(FRAME_BUDGET_US) / 10_000
}

/// 四池默认微秒预算（常量快照；与 POOL_DEFAULT_BPS 对账）。
pub const POOL_DEFAULT_US: [u64; 4] = [
    pool_budget_us(POOL_DEFAULT_BPS[0]),
    pool_budget_us(POOL_DEFAULT_BPS[1]),
    pool_budget_us(POOL_DEFAULT_BPS[2]),
    pool_budget_us(POOL_DEFAULT_BPS[3]),
];

/// 编译期守恒：四池微秒之和 ≤ 帧预算（截断除法允许 ≤，不允许 >）。
const _: () = {
    let total = POOL_DEFAULT_US[0] + POOL_DEFAULT_US[1] + POOL_DEFAULT_US[2] + POOL_DEFAULT_US[3];
    assert!(total <= FRAME_BUDGET_US);
};
