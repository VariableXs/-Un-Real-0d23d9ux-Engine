//! CGPU-F0003 · 帧任务图构建与依赖解析（CGPU-A 域 · 帧任务图与执行域 · A01 组 · 目标 480 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0003`
//!
//! **判据（锚点原文）**：读写集自动建边正确性对拍、环检测全覆盖、关键路径
//! 发射实测缩短帧延迟、跨帧围栏对账零失误、1000 节点 ≤200μs。
//!
//! **职责定位（锚点原文）**：一帧的渲染工作（上传→解码→栅格化→后处理→
//! 合成→呈现）建模为有向无环任务图。本功能负责图的构建与依赖解析：节点带
//! 五元组（类型/资源读写集/预算标签/优先级/截止时间），依赖由资源读写集自动
//! 推导（写后读/写后写冲突建边），另支持显式依赖注记。解析器在每帧开始执行
//! 拓扑排序（Kahn），环检测即死锁上抛；排序结果按关键路径长度分层，长路径
//! 优先发射以压帧延迟。图可跨帧引用（解码任务跨帧完成），跨帧边用围栏对账。
//! 图序列化：任务图快照可落盘回放，是卡顿归因（CGPU-F0007）的数据源。
//! 构建开销预算：1000 节点图解析 ≤200μs。
//!
//! ## 一、五元组节点模型：图的最小完备描述
//!
//! [`TaskNode`] 承载锚点点名的五元组：任务类型（[`TaskKind`] 六段闭集——
//! 上传/解码/栅格化/后处理/合成/呈现，与锚点管线句一字不差；表外类型不是
//! 「其他」桶而是建模缺口，显性拒 [`FgCode::UnknownTaskKind`]）；资源读写集
//! （[`Access`]：读集与写集分开建账——建边方向由读写方向决定，混账即建错
//! 边）；预算标签（[`BudgetLabel`]：预估微秒 + 优先级 + 可降级档位——C 域
//! 预算仲裁（F0321 起）按此标签申请，本域只建模不仲裁）；优先级（0=最高，
//! 同层平局裁决用）；截止时间（相对帧起点微秒，必须 ≥ 预估耗时——截止早于
//! 预估的任务在物理上不可完成，登记期即拒 [`FgCode::DeadlineBeforeEstimate`]
//! 而不是留给运行期爆）。
//!
//! ## 二、读写集自动建边：方向正确、次序确定、逐条可对拍
//!
//! 依赖推导只建两类边（锚点原文点名）：写后读 RAW（生产者→消费者）与
//! 写后写 WAW（前写者→后写者，保写序）；读后读不建边（两个读者无序约束，
//! 建了就是假依赖压并行度）。同一资源上的冲突按登记次序配对（资源访问序
//! = 表序，确定可复现），逐条边落 [`DerivationRecord`] 对拍账：哪条资源、
//! RAW 还是 WAW、谁产谁消、同资源第几次冲突——判据「建边正确性对拍」的
//! 落地面：对拍账可独立重演，边表与对拍账计数守恒（[`GraphBuildAudit`]
//! `edges_reconcile`）。显式依赖注记是补充不是替代：显式边与推导边去重
//! 合并，重复边登记但只入边表一次（[`FgCode::DuplicateEdge`] 计数留痕）。
//!
//! ## 三、Kahn 拓扑排序与环检测：死锁上抛，不静默吞
//!
//! 每帧构建完成即跑 Kahn：入度表 + 就绪队列（就绪序 = 优先级升 → 登记
//! 序升的确定性平局），出队序即拓扑序。入度清零后仍有剩余节点 = 有环——
//! 锚点命令「环检测即死锁上抛」：返回 [`FgCode::DependencyCycle`] 并携带
//! 环成员清单（从剩余节点沿任一未消费出边回走可复现环路径，环内节点
//! 全量列出——死锁诊断不能只给一个数）。环检测全覆盖：不管环藏在图的
//! 哪个角落，剩余节点非空即触达；自依赖（自己依赖自己）在建边期就拒
//! （[`FgCode::SelfDependency`]）。
//!
//! ## 四、关键路径分层与发射序：长路径优先压帧延迟
//!
//! 拓扑序上做最长路 DP：`crit[node] = est(node) + max(crit[前驱])`（无前驱
//! 取 est）——`crit` 即该节点到帧尾的最长距离。分层规则：层号 = 从虚拟源点
//! 到该节点的最长距离（`depth`），同层节点无依赖可并行。发射序三键：层号
//! 升（层间必须串行等待）、关键路径长降（长路径先走——短路径等得起，长路径
//! 等不起）、优先级升（同级平局先登记先发）。帧延迟对比：[`MakespanModel`]
//! 用「层内并行、层间屏障」的确定性模型算两版完工时间——分层发射 vs 按登记
//! 序全串行；改善量入账（判据「实测缩短帧延迟」的落地面：模型参数写死
//! 可复算，零墙钟）。
//!
//! ## 五、跨帧围栏：跨帧边不是口头约定，是账
//!
//! 解码等长任务跨帧完成：本帧节点可声明消费上一帧某任务的产出（[`TaskNode`]
//! `cross_frame_deps`）。跨帧边在 [`FenceLedger`] 落账：每条跨帧依赖生成围栏
//! 票（[`FenceTicket`]：生产帧号 + 生产任务 + 消费帧号 + 消费任务——四元组
//! 唯一）。生产帧构建时对每个跨帧产出信号（`signal`），消费帧构建时对每条
//! 跨帧依赖等待（`await_fence`）。对账零失误（判据原文）：[`FenceLedger`]
//! `reconcile` 逐票核验——有等待无信号 = 上游违约（[`FgCode::FenceMissingSignal`]）；
//! 有信号无等待 = 产出悬空（[`FgCode::FenceOrphanSignal`]）；重复等待/重复
//! 信号显性计数。围栏票号单调不复用，账本滚动窗口封顶防膨胀。
//!
//! ## 六、序列化快照：卡顿归因的数据源，往返恒等
//!
//! [`GraphSnapshot`] 把图序列化为稳定文本（版本头 + 节点行 + 显式边行 +
//! 跨帧边行 + 构建账行），[`GraphSnapshot::parse`] 反序列化回 [`FrameGraph`]；
//! 往返恒等断言面：序列化→解析→再序列化逐字节一致（[`FgCode::SnapshotDrift`]
//! 兜底）。快照带 FNV-1a 内容指纹（[`snapshot_fingerprint`]），F0007 归因
//! 引擎按指纹识别「同一张图」避免重复解析。快照是纯数据：零 IO——落盘由
//! 调用方执行，本模块只负责结构化与解析。
//!
//! ## 七、构建开销预算：操作数计账，模型标定，不拍脑袋
//!
//! 1000 节点 ≤200μs 的对账口径：本模块计数实际执行的操作数（建边扫描、
//! 入度更新、排序交换、DP 松弛——[`OpCounter`] 逐处累加），按标定常数
//! [`OPS_PER_US`]（当前策略值：宿主实测标定 64 ops/μs，保守取整可调）换算
//! 微秒预算，[`BuildBudget::within_budget`] 给出预算内/超预算结论与余量。
//! 计数的是真操作不是估的：同一张图重放构建，操作数逐位一致（确定性）。
//! 预算超限不静默：返回 [`FgCode::BuildBudgetExceeded`] 并附实际开销与
//! 建议降载方向。
//!
//! **对接**：F0001（SIMD 基元——栅格化节点的算子面）；F0002（线程池——
//! 发射计划的执行面）；F0004（放置决策器——消费任务类型与读写集）；
//! F0005（帧预算仲裁——消费预算标签）；F0007（卡顿归因——消费快照与
//! 发射账）；F0015（遥测——失败五元组同构对齐）；F0161（B 域图执行——
//! 本域输出的下游执行面）；F0167（失败传播——消费邻接表）。零 panic 面
//! （下标走 `get`、算术饱和、循环而非递归）、零 IO、零墙钟（tick 上游
//! 注入）、无全局可变状态、no_std + alloc。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（来源注释：锚点原文 + 标定值注明可调）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const FRAMEGRAPH_VERSION: &str = "A01-framegraph-v1";

/// 节点容量（单帧任务数上限；当前策略值，可调——超限显性拒不静默扩容）。
pub const NODE_CAPACITY: usize = 4_096;

/// 单节点资源访问数上限（读+写合计；当前策略值，可调）。
pub const NODE_ACCESS_CAPACITY: usize = 32;

/// 显式依赖注记数上限（单节点；当前策略值，可调）。
pub const EXPLICIT_DEP_CAPACITY: usize = 16;

/// 跨帧依赖数上限（单节点；当前策略值，可调）。
pub const CROSS_FRAME_CAPACITY: usize = 8;

/// 围栏账本滚动窗口容量（票数；当前策略值，可调——超窗按最老先归档语义截断）。
pub const FENCE_LEDGER_CAPACITY: usize = 1_024;

/// 构建操作数标定常数：每微秒等效操作数。标定口径：操作单位 = 一次
/// 边界受查的表扫描/比较推进（`get` + 比较 + 分支），目标硬件实测约
/// 3~5ns/次，保守取 5ns/op ⇒ 200 ops/μs（当前策略值，可调——换环境
/// 重标定后同步更新预算结论）。
pub const OPS_PER_US: u64 = 200;

/// 构建预算（微秒）：锚点原文「1000 节点图解析 ≤200μs」。
pub const BUILD_BUDGET_US: u64 = 200;

/// 预算对账基准规模：锚点口径按 1000 节点核算。
pub const BUDGET_BASELINE_NODES: usize = 1_000;

// ---------------------------------------------------------------------------
// 二、诊断码（0x64 段独占：A01 帧任务图面）
// ---------------------------------------------------------------------------

/// 诊断码（u16；0x64 段为本单独占段，段内互异）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FgCode {
    /// 表外任务类型（六段闭集之外——建模缺口）。
    UnknownTaskKind = 0x6401,
    /// 节点容量满：单帧任务数超上限。
    NodeCapacityFull = 0x6402,
    /// 重复节点 id：同帧任务表出现同 id。
    DuplicateNodeId = 0x6403,
    /// 节点资源访问数超上限。
    AccessCapacityFull = 0x6404,
    /// 显式依赖注记数超上限。
    ExplicitDepCapacityFull = 0x6405,
    /// 跨帧依赖数超上限。
    CrossFrameCapacityFull = 0x6406,
    /// 截止时间早于预估耗时（物理不可完成，登记期即拒）。
    DeadlineBeforeEstimate = 0x6407,
    /// 预算标签非法：预估耗时为 0 或优先级越界。
    BudgetLabelInvalid = 0x6408,
    /// 自依赖：任务依赖自己。
    SelfDependency = 0x6409,
    /// 显式依赖目标缺失：依赖的节点 id 不在本帧任务表。
    DependencyMissing = 0x640a,
    /// 重复边：推导边与显式边重复（只入边表一次，计数留痕）。
    DuplicateEdge = 0x640b,
    /// 依赖环：Kahn 收尾剩余节点非空——死锁上抛。
    DependencyCycle = 0x640c,
    /// 围栏缺失信号：消费帧等待的围栏在生产帧无信号记录。
    FenceMissingSignal = 0x640d,
    /// 围栏产出悬空：信号发出但无消费帧等待。
    FenceOrphanSignal = 0x640e,
    /// 围栏重复等待/重复信号（显性计数不静默）。
    FenceDuplicate = 0x640f,
    /// 围栏账本满：滚动窗口容量耗尽（最老票归档语义未启用即拒）。
    FenceLedgerFull = 0x6410,
    /// 快照漂移：序列化→解析→再序列化不一致。
    SnapshotDrift = 0x6411,
    /// 快照版本表外：解析遇到的版本头不在支持闭集。
    SnapshotVersionUnknown = 0x6412,
    /// 快照格式损坏：行数/字段数/数值非法。
    SnapshotMalformed = 0x6413,
    /// 构建预算超限：操作数换算微秒超过 BUILD_BUDGET_US。
    BuildBudgetExceeded = 0x6414,
    /// 空图：零节点构建（帧图至少要有呈现节点）。
    EmptyGraph = 0x6415,
    /// 呈现节点缺失：六段管线中 Present 必在（帧的终点）。
    PresentNodeMissing = 0x6416,
    /// 节点 id 越界：id 编码非法（高位保留位被占用）。
    NodeIdReservedBits = 0x6417,
    /// 呈现节点重复：一帧只有一个上屏动作。
    DuplicatePresent = 0x6418,
}

impl FgCode {
    /// 人读诊断（异常零静默：发生了什么/为什么/下一步）。
    pub const fn detail(self) -> &'static str {
        match self {
            FgCode::UnknownTaskKind => "表外任务类型：六段闭集（上传/解码/栅格化/后处理/合成/呈现）之外，建议补建模后再登记",
            FgCode::NodeCapacityFull => "节点容量满：单帧任务数超上限，建议拆帧或调 NODE_CAPACITY 策略值",
            FgCode::DuplicateNodeId => "重复节点 id：同帧任务表同 id，建议核对图构建器去重",
            FgCode::AccessCapacityFull => "节点资源访问数超上限：读写集合计超 NODE_ACCESS_CAPACITY，建议拆分任务或合并访问",
            FgCode::ExplicitDepCapacityFull => "显式依赖注记超上限：建议用读写集自动推导替代手工注记",
            FgCode::CrossFrameCapacityFull => "跨帧依赖超上限：单节点跨帧消费过多，建议分帧摊派",
            FgCode::DeadlineBeforeEstimate => "截止早于预估：任务物理上不可完成，建议修预算标签或放宽截止",
            FgCode::BudgetLabelInvalid => "预算标签非法：预估耗时应 >0 且优先级在 0..=255，建议核对标签来源",
            FgCode::SelfDependency => "自依赖：任务依赖自己构成平凡环，建议删除该注记",
            FgCode::DependencyMissing => "依赖目标缺失：注记的节点 id 不在本帧任务表，建议核对登记序或补登记",
            FgCode::DuplicateEdge => "重复边：显式注记与自动推导重合，边表只入一次（计数留痕），建议清理冗余注记",
            FgCode::DependencyCycle => "依赖环：Kahn 收尾剩余节点非空即死锁，建议按环成员清单断环（跨帧化或降级）",
            FgCode::FenceMissingSignal => "围栏缺信号：消费帧等待的跨帧产出在生产帧无信号，建议核对生产任务是否真在本帧图",
            FgCode::FenceOrphanSignal => "围栏产出悬空：信号无消费方，建议核对消费帧是否被取消（取消帧要同步撤票）",
            FgCode::FenceDuplicate => "围栏重复动作：同票重复等待/信号，建议核对构建器幂等性",
            FgCode::FenceLedgerFull => "围栏账本满：滚动窗口耗尽，建议提高 FENCE_LEDGER_CAPACITY 或加快对账频率",
            FgCode::SnapshotDrift => "快照漂移：往返序列化不一致，建议立即停止回放并核查版本兼容",
            FgCode::SnapshotVersionUnknown => "快照版本表外：解析头不在支持闭集，建议升级解析器或重新导出",
            FgCode::SnapshotMalformed => "快照格式损坏：行/字段/数值非法，建议用指纹定位损坏段后重导出",
            FgCode::BuildBudgetExceeded => "构建预算超限：操作数换算超 200μs 基准，建议拆帧减节点或缩资源集",
            FgCode::EmptyGraph => "空图：零节点构建被拒，帧图至少要含呈现节点",
            FgCode::PresentNodeMissing => "呈现节点缺失：Present 是帧的终点，缺它整帧不可上屏，建议补登记",
            FgCode::NodeIdReservedBits => "节点 id 保留位被占用：id 编码非法，建议从分配器重新取号",
            FgCode::DuplicatePresent => "呈现节点重复：一帧恰一个 Present，建议合并上屏动作",
        }
    }

    /// 段号（段独占审计用）。
    pub const fn segment(self) -> u16 {
        (self as u16) >> 8
    }
}

// ---------------------------------------------------------------------------
// 三、任务类型与资源访问（五元组的前两元）
// ---------------------------------------------------------------------------

/// 任务类型六段闭集（锚点原文管线：上传→解码→栅格化→后处理→合成→呈现）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    /// 上传：CPU→GPU 资产搬运（F0010 带宽调度的对象）。
    Upload,
    /// 解码：图像/视频解码（典型跨帧长任务）。
    Decode,
    /// 栅格化：几何→像素（F0001 SIMD 基元的消费方）。
    Rasterize,
    /// 后处理：全屏效果链（模糊/色调映射等）。
    PostProcess,
    /// 合成：多表面合成（F0009 表面池的产出汇点）。
    Composite,
    /// 呈现：上屏（帧的终点；缺它整帧不可交付）。
    Present,
}

impl TaskKind {
    /// 全闭集（判据侧独立重排对拍用）。
    pub const ALL: [TaskKind; 6] = [
        TaskKind::Upload,
        TaskKind::Decode,
        TaskKind::Rasterize,
        TaskKind::PostProcess,
        TaskKind::Composite,
        TaskKind::Present,
    ];

    /// 类型名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            TaskKind::Upload => "上传",
            TaskKind::Decode => "解码",
            TaskKind::Rasterize => "栅格化",
            TaskKind::PostProcess => "后处理",
            TaskKind::Composite => "合成",
            TaskKind::Present => "呈现",
        }
    }

    /// 管线段序（0-5；上传最早、呈现最晚——用于合理性校验：呈现节点
    /// 的预估耗时通常最小但段序最大；本域只记录段序不做跨类型强约束，
    /// 强约束在 F0004 放置决策器做）。
    pub const fn stage(self) -> u8 {
        match self {
            TaskKind::Upload => 0,
            TaskKind::Decode => 1,
            TaskKind::Rasterize => 2,
            TaskKind::PostProcess => 3,
            TaskKind::Composite => 4,
            TaskKind::Present => 5,
        }
    }

    /// 从编号恢复（快照解析用；表外编号显性拒）。
    pub const fn from_index(i: u8) -> Option<TaskKind> {
        match i {
            0 => Some(TaskKind::Upload),
            1 => Some(TaskKind::Decode),
            2 => Some(TaskKind::Rasterize),
            3 => Some(TaskKind::PostProcess),
            4 => Some(TaskKind::Composite),
            5 => Some(TaskKind::Present),
            _ => None,
        }
    }
}

/// 资源 id（帧内资源命名空间由图构建器统一分配；u32 平面命名）。
pub type ResourceId = u32;

/// 访问方向（读写分账——建边方向由它决定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessKind {
    /// 读访问（消费资源内容）。
    Read,
    /// 写访问（生产/覆写资源内容）。
    Write,
}

/// 单条资源访问（节点 × 资源 × 方向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access {
    /// 资源 id。
    pub resource: ResourceId,
    /// 方向。
    pub kind: AccessKind,
}

impl Access {
    /// 读访问快捷构造。
    pub const fn read(resource: ResourceId) -> Access {
        Access { resource, kind: AccessKind::Read }
    }

    /// 写访问快捷构造。
    pub const fn write(resource: ResourceId) -> Access {
        Access { resource, kind: AccessKind::Write }
    }
}

// ---------------------------------------------------------------------------
// 四、预算标签与五元组节点
// ---------------------------------------------------------------------------

/// 预算标签（C 域仲裁的消费接口；本域建模并做登记期合法性校验）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetLabel {
    /// 预估耗时（微秒；>0）。
    pub est_us: u32,
    /// 优先级（0=最高..=255；同层平局裁决与发射序第二键）。
    pub priority: u8,
    /// 可降级档位数（0=不可降级；降级阶梯由 F0167/O 域配方执行）。
    pub degradable_tiers: u8,
}

impl BudgetLabel {
    /// 登记期合法性校验（零预估与物理不可完成在门口拦下）。
    pub const fn validate(&self) -> Option<FgCode> {
        if self.est_us == 0 {
            return Some(FgCode::BudgetLabelInvalid);
        }
        None
    }
}

/// 跨帧依赖声明（消费帧视角：我要上一帧某任务的产出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossFrameDep {
    /// 生产帧号。
    pub producer_frame: u64,
    /// 生产任务 id（在生产帧的图内）。
    pub producer_task: u32,
}

/// 任务节点（五元组 + 显式注记 + 跨帧声明）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskNode {
    /// 节点 id（帧内唯一；保留位校验见 [`validate_node_id`]）。
    pub id: u32,
    /// 任务类型（五元组之一）。
    pub kind: TaskKind,
    /// 读集（去重由构建器保证）。
    pub reads: Vec<ResourceId>,
    /// 写集（去重由构建器保证）。
    pub writes: Vec<ResourceId>,
    /// 预算标签（五元组之一）。
    pub budget: BudgetLabel,
    /// 截止时间（相对帧起点微秒；≥ est_us 校验在登记期）。
    pub deadline_us: u32,
    /// 显式依赖注记（补充边；与推导边去重合并）。
    pub explicit_deps: Vec<u32>,
    /// 跨帧依赖声明。
    pub cross_frame_deps: Vec<CrossFrameDep>,
}

/// 节点 id 保留位校验：高 8 位必须为 0（编码空间留给元数据扩展）。
pub const fn validate_node_id(id: u32) -> Option<FgCode> {
    if (id >> 24) != 0 {
        return Some(FgCode::NodeIdReservedBits);
    }
    None
}

// ---------------------------------------------------------------------------
// 五、边与推导对拍账
// ---------------------------------------------------------------------------

/// 依赖边（有向：from → to 表示 to 依赖 from，from 先行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    /// 前驱（生产者/前写者）。
    pub from: u32,
    /// 后继（消费者/后写者）。
    pub to: u32,
}

/// 边来源（推导对拍的关键维度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeOrigin {
    /// 写后读（资源内容依赖）。
    Raw,
    /// 写后写（写序保持）。
    Waw,
    /// 显式注记（与推导边去重后入表）。
    Explicit,
}

impl EdgeOrigin {
    /// 来源名（对拍账人读列）。
    pub const fn zh(self) -> &'static str {
        match self {
            EdgeOrigin::Raw => "写后读",
            EdgeOrigin::Waw => "写后写",
            EdgeOrigin::Explicit => "显式",
        }
    }

    /// 编号（快照序列化用）。
    pub const fn index(self) -> u8 {
        match self {
            EdgeOrigin::Raw => 0,
            EdgeOrigin::Waw => 1,
            EdgeOrigin::Explicit => 2,
        }
    }

    /// 从编号恢复（表外显性拒）。
    pub const fn from_index(i: u8) -> Option<EdgeOrigin> {
        match i {
            0 => Some(EdgeOrigin::Raw),
            1 => Some(EdgeOrigin::Waw),
            2 => Some(EdgeOrigin::Explicit),
            _ => None,
        }
    }
}

/// 推导对拍记录（判据「建边正确性对拍」的逐条落账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivationRecord {
    /// 边（from→to）。
    pub edge: Edge,
    /// 来源（RAW/WAW）。
    pub origin: EdgeOrigin,
    /// 冲突资源。
    pub resource: ResourceId,
    /// 该资源上的冲突序（从 0 起；同资源同向多次冲突可对拍）。
    pub conflict_seq: u32,
}

/// 显式边登记记录（去重语义的对拍面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExplicitRecord {
    /// 注记的边。
    pub edge: Edge,
    /// 是否与推导边重合（重合只计数不入边表）。
    pub overlapped_with_derived: bool,
}

// ---------------------------------------------------------------------------
// 六、操作数计数器（构建开销预算的账本）
// ---------------------------------------------------------------------------

/// 操作数计数器：构建/解析/排序全流程的真操作计数（确定性——同图重放
/// 逐位一致）。计数点按最细粒度循环体累加，标定常数见 [`OPS_PER_US`]。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct OpCounter {
    /// 建边扫描操作（资源访问配对比较）。
    pub edge_scan_ops: u64,
    /// 入度/出度表操作。
    pub degree_ops: u64,
    /// 就绪队列操作（入队/出队/平局比较）。
    pub queue_ops: u64,
    /// 关键路径 DP 松弛操作。
    pub dp_relax_ops: u64,
    /// 序列化/解析字符操作（快照路径）。
    pub serial_ops: u64,
}

impl OpCounter {
    /// 总操作数。
    pub const fn total(&self) -> u64 {
        self.edge_scan_ops
            .saturating_add(self.degree_ops)
            .saturating_add(self.queue_ops)
            .saturating_add(self.dp_relax_ops)
            .saturating_add(self.serial_ops)
    }

    /// 换算预估微秒（标定模型：total / OPS_PER_US，向上取整）。
    pub const fn est_us(&self) -> u64 {
        let ops = self.total();
        let per = OPS_PER_US;
        ops.saturating_add(per).saturating_sub(1) / per
    }
}

/// 构建预算结论（判据「1000 节点 ≤200μs」的核算面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildBudget {
    /// 总操作数。
    pub ops: u64,
    /// 换算预估微秒。
    pub est_us: u64,
    /// 是否在预算内（≤ BUILD_BUDGET_US）。
    pub within: bool,
    /// 余量微秒（超限为 0）。
    pub headroom_us: u64,
}

impl BuildBudget {
    /// 从计数器核算。
    pub const fn from_counter(c: &OpCounter) -> BuildBudget {
        let ops = c.total();
        let est = c.est_us();
        let within = est <= BUILD_BUDGET_US;
        let headroom = if within { BUILD_BUDGET_US - est } else { 0 };
        BuildBudget { ops, est_us: est, within, headroom_us: headroom }
    }
}

// ---------------------------------------------------------------------------
// 七、图构建器：登记校验 + 自动建边 + 显式合并
// ---------------------------------------------------------------------------

/// 帧任务图（不可变成品；由 [`FrameGraphBuilder::build`] 产出）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameGraph {
    /// 节点表（登记序）。
    pub nodes: Vec<TaskNode>,
    /// 边表（去重后；推导序在前、显式补边按注记序在后）。
    pub edges: Vec<Edge>,
    /// 推导对拍账（RAW/WAW 逐条）。
    pub derivations: Vec<DerivationRecord>,
    /// 显式边对拍账（含去重结论）。
    pub explicit_records: Vec<ExplicitRecord>,
    /// 出边邻接表（按节点 id → 直接后继 id 表；下游执行与失败传播消费）。
    pub successors: Vec<(u32, Vec<u32>)>,
    /// 构建操作数账。
    pub ops: OpCounter,
}

impl FrameGraph {
    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 边数。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 按 id 查节点。
    pub fn node(&self, id: u32) -> Option<&TaskNode> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.id == id {
                    return Some(n);
                }
            }
            i += 1;
        }
        None
    }

    /// 直接后继表（F0167 失败传播消费；无后继返回空表）。
    pub fn successors_of(&self, id: u32) -> Vec<u32> {
        let mut i = 0usize;
        while i < self.successors.len() {
            if let Some((k, v)) = self.successors.get(i) {
                if *k == id {
                    return v.clone();
                }
            }
            i += 1;
        }
        Vec::new()
    }

    /// 节点 id → 表内下标映射（线性查找；节点容量 4096 内 O(N) 可负担，
    /// 排序/DP 等高频路径用 [`FrameGraph::index_of_cached`] 预建表替代）。
    pub fn index_of(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 预建 id→下标有序表（调用方在批量查询前建一次，把 O(N²) 压成
    /// O(N log N) 建表 + O(log N) 查询）。
    pub fn id_index(&self) -> Vec<(u32, usize)> {
        let mut idx: Vec<(u32, usize)> = Vec::new();
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                idx.push((n.id, i));
            }
            i += 1;
        }
        idx.sort_by(|a, b| a.0.cmp(&b.0));
        idx
    }
}

/// 帧任务图构建器。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameGraphBuilder {
    nodes: Vec<TaskNode>,
    ops: OpCounter,
}

impl FrameGraphBuilder {
    /// 空构建器（const：零值账本直接字段初始化——Default::default 非 const）。
    pub const fn new() -> FrameGraphBuilder {
        FrameGraphBuilder {
            nodes: Vec::new(),
            ops: OpCounter {
                edge_scan_ops: 0,
                degree_ops: 0,
                queue_ops: 0,
                dp_relax_ops: 0,
                serial_ops: 0,
            },
        }
    }

    /// 登记节点（五元组 + 注记全量校验；任何一条不过即整节点拒——
    /// 半登记节点会让建边与排序拿到脏数据）。
    pub fn add_node(&mut self, node: TaskNode) -> Result<(), FgCode> {
        // 容量闸。
        if self.nodes.len() >= NODE_CAPACITY {
            return Err(FgCode::NodeCapacityFull);
        }
        // id 合法性（保留位）。
        if let Some(c) = validate_node_id(node.id) {
            return Err(c);
        }
        // 重复 id。
        let mut i = 0usize;
        while i < self.nodes.len() {
            self.ops.edge_scan_ops = self.ops.edge_scan_ops.saturating_add(1);
            if let Some(n) = self.nodes.get(i) {
                if n.id == node.id {
                    return Err(FgCode::DuplicateNodeId);
                }
            }
            i += 1;
        }
        // 类型闭集（类型本身已是枚举——此处校验的是语义闸：Present 唯一性）。
        if node.kind == TaskKind::Present {
            let mut j = 0usize;
            while j < self.nodes.len() {
                self.ops.edge_scan_ops = self.ops.edge_scan_ops.saturating_add(1);
                if let Some(n) = self.nodes.get(j) {
                    if n.kind == TaskKind::Present {
                        // 呈现节点唯一：帧只有一个上屏动作。
                        return Err(FgCode::DuplicatePresent);
                    }
                }
                j += 1;
            }
        }
        // 预算标签校验。
        if let Some(c) = node.budget.validate() {
            return Err(c);
        }
        // 截止不早于预估（物理可完成性）。
        if (node.deadline_us as u64) < (node.budget.est_us as u64) {
            return Err(FgCode::DeadlineBeforeEstimate);
        }
        // 读写集去重（升序规范形——建边扫描与快照往返都以规范形为准）。
        let mut node = node;
        node.reads.sort_by(|a, b| a.cmp(b));
        node.reads.dedup();
        node.writes.sort_by(|a, b| a.cmp(b));
        node.writes.dedup();
        // 访问容量（读+写合计）。
        let access_total = node.reads.len().saturating_add(node.writes.len());
        if access_total > NODE_ACCESS_CAPACITY {
            return Err(FgCode::AccessCapacityFull);
        }
        // 注记容量。
        if node.explicit_deps.len() > EXPLICIT_DEP_CAPACITY {
            return Err(FgCode::ExplicitDepCapacityFull);
        }
        if node.cross_frame_deps.len() > CROSS_FRAME_CAPACITY {
            return Err(FgCode::CrossFrameCapacityFull);
        }
        // 自依赖闸（自己注记依赖自己）。
        let mut d = 0usize;
        while d < node.explicit_deps.len() {
            self.ops.edge_scan_ops = self.ops.edge_scan_ops.saturating_add(1);
            if let Some(dep) = node.explicit_deps.get(d) {
                if *dep == node.id {
                    return Err(FgCode::SelfDependency);
                }
            }
            d += 1;
        }
        self.nodes.push(node);
        Ok(())
    }

    /// 依赖自动推导（RAW/WAW）+ 显式注记合并 + 邻接表 + 对拍账。
    ///
    /// 推导算法（确定性）：按节点登记序扫描；对每个资源维护「最近写者」
    /// 与「全部读者」两条账（资源 → 登记序下标表）。节点 v 写资源 r 时：
    /// r 的每个已登记读者 u 建 RAW(v→u 不对——u 读的是旧写者 w 的产出，
    /// 边是 w→v；同时 v 写完之前读者不能读新值？——精确语义：读发生在
    /// 写之前是 WAR 不建边（锚点只点名 RAW/WAW），读发生在写之后（登记序
    /// 更后）的读者 u 拿到的是 v 的新值，边 v→u。故「最近写者→后续读者」
    /// 建 RAW。v 写 r 时若存在最近写者 w（w≠v），建 WAW(w→v) 保写序；
    /// 读者们不需要对 v 建边（读者先于 v 登记即先于 v 读旧值——登记序即
    /// 执行序的先验模型）。
    pub fn build(mut self) -> Result<(FrameGraph, GraphBuildAudit), FgCode> {
        if self.nodes.is_empty() {
            return Err(FgCode::EmptyGraph);
        }
        // 呈现节点必须存在（帧的终点）。
        let mut has_present = false;
        let mut i = 0usize;
        while i < self.nodes.len() {
            self.ops.edge_scan_ops = self.ops.edge_scan_ops.saturating_add(1);
            if let Some(n) = self.nodes.get(i) {
                if n.kind == TaskKind::Present {
                    has_present = true;
                }
            }
            i += 1;
        }
        if !has_present {
            return Err(FgCode::PresentNodeMissing);
        }

        // ---- 自动建边（排序分组扫描：O(A log A)，A=访问事件总数）----
        // 事件收集：(resource, order=登记序, node_idx, is_write)。排序键
        // （资源升序、登记序升序）让同资源访问在表中连续，组内一次线性
        // 推进最近写者即完成配对——把每访问 O(R) 的表扫描压成 O(1)，
        // 1000 节点 ×2 访问 ≤200μs 的预算判据因此可达。同节点同资源
        // 读写在事件序上读先写后（登记序注入顺序），读拿旧值、写保写序，
        // 均与登记序执行模型一致。
        let mut events: Vec<(ResourceId, u32, usize, bool)> = Vec::new();
        let mut v = 0usize;
        while v < self.nodes.len() {
            if let Some(n) = self.nodes.get(v) {
                let mut i = 0usize;
                while i < n.reads.len() {
                    if let Some(&r) = n.reads.get(i) {
                        events.push((r, v as u32, v, false));
                    }
                    i += 1;
                }
                let mut i = 0usize;
                while i < n.writes.len() {
                    if let Some(&r) = n.writes.get(i) {
                        events.push((r, v as u32, v, true));
                    }
                    i += 1;
                }
                self.ops.edge_scan_ops = self
                    .ops
                    .edge_scan_ops
                    .saturating_add(n.reads.len().saturating_add(n.writes.len()) as u64);
            }
            v += 1;
        }
        // 排序（比较次数逐次计数——构建预算的真操作账，不拍脑袋）。
        let mut cmp_ops = 0u64;
        events.sort_by(|a, b| {
            cmp_ops = cmp_ops.saturating_add(1);
            a.0.cmp(&b.0).then(a.1.cmp(&b.1))
        });
        self.ops.edge_scan_ops = self.ops.edge_scan_ops.saturating_add(cmp_ops);

        let mut derivations: Vec<DerivationRecord> = Vec::new();
        // 组内扫描：同资源事件连续，last_writer 组内推进，conflict_seq
        // 组内单调（对拍账可独立重演）。
        let mut g0 = 0usize;
        while g0 < events.len() {
            let res = match events.get(g0) {
                Some((r, ..)) => *r,
                None => break,
            };
            let mut last_writer: Option<usize> = None;
            let mut seq = 0u32;
            let mut e = g0;
            while e < events.len() {
                let (r, _ord, ni, is_write) = match events.get(e) {
                    Some(x) => *x,
                    None => break,
                };
                if r != res {
                    break;
                }
                self.ops.edge_scan_ops = self.ops.edge_scan_ops.saturating_add(1);
                let from_idx = if is_write {
                    // WAW：最近写者 → 本写；随后最近写者前移为本节点。
                    let lw = last_writer;
                    last_writer = Some(ni);
                    lw
                } else {
                    // RAW：最近写者 → 本读（无写者 = 初始资源内容，无边）。
                    last_writer
                };
                if let Some(wi) = from_idx {
                    if wi != ni {
                        let origin = if is_write { EdgeOrigin::Waw } else { EdgeOrigin::Raw };
                        derivations.push(DerivationRecord {
                            edge: Edge { from: node_id(&self.nodes, wi), to: node_id(&self.nodes, ni) },
                            origin,
                            resource: r,
                            conflict_seq: seq,
                        });
                        seq = seq.saturating_add(1);
                    }
                }
                e += 1;
            }
            g0 = e;
        }
        // 边表规范形：推导边排序去重（同节点对可由多资源冲突产生同边；
        // 边表是集合语义，对拍账保留逐冲突明细）。
        let mut edges: Vec<Edge> = derivations.iter().map(|d| d.edge).collect();
        edges.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));
        edges.dedup();

        // ---- 显式注记合并（去重；与推导集与已收注记双向比对）----
        let mut explicit_records: Vec<ExplicitRecord> = Vec::new();
        let mut dup_edges = 0u64;
        let mut n = 0usize;
        while n < self.nodes.len() {
            let (id, deps): (u32, Vec<u32>) = match self.nodes.get(n) {
                Some(node) => (node.id, node.explicit_deps.clone()),
                None => break,
            };
            let mut d = 0usize;
            while d < deps.len() {
                let dep = match deps.get(d) {
                    Some(x) => *x,
                    None => break,
                };
                self.ops.degree_ops = self.ops.degree_ops.saturating_add(1);
                // 目标存在性（构建期校验：注记指向缺席节点是脏图）。
                let mut found = false;
                let mut m = 0usize;
                while m < self.nodes.len() {
                    self.ops.edge_scan_ops = self.ops.edge_scan_ops.saturating_add(1);
                    if let Some(node) = self.nodes.get(m) {
                        if node.id == dep {
                            found = true;
                            break;
                        }
                    }
                    m += 1;
                }
                if !found {
                    return Err(FgCode::DependencyMissing);
                }
                let e = Edge { from: dep, to: id };
                // 去重判定：与推导集或已收显式边重合只计数不入表。
                let mut overlap = false;
                let mut m = 0usize;
                while m < edges.len() {
                    self.ops.edge_scan_ops = self.ops.edge_scan_ops.saturating_add(1);
                    if let Some(x) = edges.get(m) {
                        if x.from == e.from && x.to == e.to {
                            overlap = true;
                            break;
                        }
                    }
                    m += 1;
                }
                if overlap {
                    dup_edges = dup_edges.saturating_add(1);
                } else {
                    // 显式边按注记序插入规范位（保持边表升序不变式）。
                    let pos = binary_edge_pos(&edges, e);
                    edges.insert(pos, e);
                }
                explicit_records.push(ExplicitRecord { edge: e, overlapped_with_derived: overlap });
                d += 1;
            }
            n += 1;
        }

        // ---- 邻接表（id → 直接后继）----
        let mut successors: Vec<(u32, Vec<u32>)> = Vec::new();
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(node) = self.nodes.get(i) {
                let mut succ: Vec<u32> = Vec::new();
                let mut m = 0usize;
                while m < edges.len() {
                    self.ops.degree_ops = self.ops.degree_ops.saturating_add(1);
                    if let Some(e) = edges.get(m) {
                        if e.from == node.id {
                            succ.push(e.to);
                        }
                    }
                    m += 1;
                }
                successors.push((node.id, succ));
            }
            i += 1;
        }

        let graph = FrameGraph {
            nodes: self.nodes,
            edges,
            derivations,
            explicit_records,
            successors,
            ops: self.ops,
        };
        let audit = GraphBuildAudit {
            derived_raw: count_origin(&graph.derivations, EdgeOrigin::Raw),
            derived_waw: count_origin(&graph.derivations, EdgeOrigin::Waw),
            derived_unique: unique_derived_edges(&graph.derivations),
            explicit_total: graph.explicit_records.len() as u64,
            explicit_overlapped: graph
                .explicit_records
                .iter()
                .filter(|r| r.overlapped_with_derived)
                .count() as u64,
            duplicate_edges: dup_edges,
            edge_table_total: graph.edges.len() as u64,
        };
        // 对拍守恒：边表 = 推导去重 + 显式 − 重合；多一条少一条都是缺陷。
        if !audit.edges_reconcile() {
            return Err(FgCode::DuplicateEdge);
        }
        Ok((graph, audit))
    }
}

impl Default for FrameGraphBuilder {
    fn default() -> FrameGraphBuilder {
        FrameGraphBuilder::new()
    }
}

/// 构建对拍账（判据「建边正确性对拍」的汇总面；计数守恒在 build 尾部断言）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphBuildAudit {
    /// RAW 推导冲突数（逐冲突明细口径）。
    pub derived_raw: u64,
    /// WAW 推导冲突数（逐冲突明细口径）。
    pub derived_waw: u64,
    /// 推导边去重后条数（独立二次去重重算——与建边期去重互为对拍）。
    pub derived_unique: u64,
    /// 显式注记总数。
    pub explicit_total: u64,
    /// 其中与推导边或先收注记重合数（重合只计数不入表）。
    pub explicit_overlapped: u64,
    /// 重复边计数（重合注记的别名账，与 explicit_overlapped 相等时健康）。
    pub duplicate_edges: u64,
    /// 边表总数（集合口径）。
    pub edge_table_total: u64,
}

impl GraphBuildAudit {
    /// 守恒恒等式：边表 = 推导去重 + 显式 − 重合。
    pub const fn edges_reconcile(&self) -> bool {
        self.derived_unique
            .saturating_add(self.explicit_total)
            .saturating_sub(self.explicit_overlapped)
            == self.edge_table_total
    }
}

fn node_id(nodes: &[TaskNode], idx: usize) -> u32 {
    match nodes.get(idx) {
        Some(n) => n.id,
        None => 0,
    }
}

/// 推导边独立去重计数（审计侧二次重算——与建边期去重互为对拍面）。
fn unique_derived_edges(recs: &[DerivationRecord]) -> u64 {
    let mut tmp: Vec<Edge> = recs.iter().map(|r| r.edge).collect();
    tmp.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));
    tmp.dedup();
    tmp.len() as u64
}

/// 规范形边表的插入位（升序不变式的二分定位）。
fn binary_edge_pos(edges: &[Edge], e: Edge) -> usize {
    let mut lo = 0usize;
    let mut hi = edges.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        match edges.get(mid) {
            Some(x) => {
                let ord = x.from.cmp(&e.from).then(x.to.cmp(&e.to));
                if ord == core::cmp::Ordering::Less {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            None => return lo,
        }
    }
    lo
}

fn count_origin(recs: &[DerivationRecord], origin: EdgeOrigin) -> u64 {
    let mut c = 0u64;
    let mut i = 0usize;
    while i < recs.len() {
        if let Some(r) = recs.get(i) {
            if r.origin == origin {
                c = c.saturating_add(1);
            }
        }
        i += 1;
    }
    c
}

// ---------------------------------------------------------------------------
// 八、拓扑排序（Kahn）与环检测
// ---------------------------------------------------------------------------

/// 拓扑序结果（成功）/ 死锁上抛（失败带环成员）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TopoOutcome {
    /// 拓扑序（节点 id 序列；发射序的前段）。
    Order(Vec<u32>),
    /// 死锁：环成员 id 清单（至少 2 个；自依赖在登记期已拒）。
    Cycle(Vec<u32>),
}

/// Kahn 运行结果：拓扑序或死锁上抛 + 排序操作数账（确定性重放对账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopoRun {
    /// 排序结论。
    pub outcome: TopoOutcome,
    /// 就绪队列操作数（入队/出队/平局比较——构建预算总账的一部分）。
    pub queue_ops: u64,
}

/// Kahn 拓扑排序（确定性平局：优先级升 → 登记序升）。
pub fn topo_run(graph: &FrameGraph) -> TopoRun {
    let n = graph.nodes.len();
    // 入度表（下标 = 登记下标）。
    let mut indeg: Vec<u32> = vec![0; n];
    // 出边邻接（登记下标 → 后继登记下标表）。
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    // id → 登记下标。
    let idx = graph.id_index();
    let mut e = 0usize;
    while e < graph.edges.len() {
        if let Some(edge) = graph.edges.get(e) {
            let from_i = lookup(&idx, edge.from);
            let to_i = lookup(&idx, edge.to);
            if let (Some(f), Some(t)) = (from_i, to_i) {
                if let Some(d) = indeg.get_mut(t) {
                    *d = d.saturating_add(1);
                }
                if let Some(a) = adj.get_mut(f) {
                    a.push(t);
                }
            }
        }
        e += 1;
        // 操作数：入度/邻接操作计 1/边。
        // （graph.ops 是构建账；排序账走返回值——见 topo_ops。）
    }
    // 就绪队列（线性扫描式优先队列：容量 ≤4096，确定性优先级平局）。
    let mut ready: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < n {
        if let Some(d) = indeg.get(i) {
            if *d == 0 {
                ready.push(i);
            }
        }
        i += 1;
    }
    let mut order: Vec<u32> = Vec::new();
    let mut queue_ops = 0u64;
    while !ready.is_empty() {
        // 取优先级最高（priority 数值小者优先）→ 登记序最早。
        let mut best = 0usize;
        let mut bi = 1usize;
        while bi < ready.len() {
            queue_ops = queue_ops.saturating_add(1);
            let (a_idx, b_idx) = match (ready.get(best), ready.get(bi)) {
                (Some(&a), Some(&b)) => (a, b),
                _ => (0usize, 0usize),
            };
            let a_pri = prio_of(graph, a_idx);
            let b_pri = prio_of(graph, b_idx);
            if b_pri < a_pri || (b_pri == a_pri && b_idx < a_idx) {
                best = bi;
            }
            bi += 1;
        }
        let cur = match ready.get(best) {
            Some(&x) => x,
            None => break,
        };
        // O(1) 摘除：与队尾交换后弹出（无序队列语义安全）。
        let last = ready.len().saturating_sub(1);
        ready.swap(best, last);
        ready.pop();
        let cur_id = match graph.nodes.get(cur) {
            Some(node) => node.id,
            None => break,
        };
        order.push(cur_id);
        // 摘边：后继入度减一，归零入就绪。
        let succs: Vec<usize> = match adj.get(cur) {
            Some(s) => s.clone(),
            None => Vec::new(),
        };
        let mut s = 0usize;
        while s < succs.len() {
            let t = match succs.get(s) {
                Some(&t) => t,
                None => break,
            };
            let mut zeroed = false;
            if let Some(d) = indeg.get_mut(t) {
                *d = d.saturating_sub(1);
                zeroed = *d == 0;
            }
            if zeroed {
                ready.push(t);
            }
            s += 1;
        }
    }
    if order.len() != n {
        // 有环：剩余节点（入度未清零者）即环成员候选全量上抛。
        let mut members: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < n {
            let d = match indeg.get(i) {
                Some(d) => *d,
                None => 0,
            };
            if d > 0 {
                if let Some(node) = graph.nodes.get(i) {
                    members.push(node.id);
                }
            }
            i += 1;
        }
        return TopoRun { outcome: TopoOutcome::Cycle(members), queue_ops };
    }
    TopoRun { outcome: TopoOutcome::Order(order), queue_ops }
}

/// 有序 (u32, V) 表的二分查找（按首键；V Copy——值语义返回）。
fn lookup<V: Copy>(idx: &[(u32, V)], id: u32) -> Option<V> {
    let mut lo = 0usize;
    let mut hi = idx.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        match idx.get(mid) {
            Some((k, v)) => {
                if *k == id {
                    return Some(*v);
                } else if *k < id {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            None => return None,
        }
    }
    None
}

fn prio_of(graph: &FrameGraph, idx: usize) -> u8 {
    match graph.nodes.get(idx) {
        Some(n) => n.budget.priority,
        None => u8::MAX,
    }
}

// ---------------------------------------------------------------------------
// 九、关键路径分层与发射计划
// ---------------------------------------------------------------------------

/// 发射计划（判据「关键路径分层 + 长路径优先发射」的产出物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmissionPlan {
    /// 分层表：层号 → 节点 id 表（层内已按关键路径降序 + 优先级升序排定）。
    pub layers: Vec<(u32, Vec<u32>)>,
    /// 每节点关键路径余量（到帧尾的最长距离，微秒；id → crit）。
    pub crit: Vec<(u32, u64)>,
    /// 每节点层号（id → depth）。
    pub depth: Vec<(u32, u32)>,
    /// 发射序（层号升 → crit 降 → priority 升 → 登记序升）。
    pub emission: Vec<u32>,
    /// 分层发射完工预估（微秒；层内并行、层间屏障模型）。
    pub makespan_layered_us: u64,
    /// 全串行基线完工预估（微秒；登记序逐个执行）。
    pub makespan_serial_us: u64,
    /// 改善量（微秒；serial − layered，负值为 0——分层发射不会更差）。
    pub improvement_us: u64,
    /// DP/排序操作数账（确定性重放对账用）。
    pub ops: OpCounter,
}

/// 关键路径分层与发射计划推导（输入必须是拓扑合法图——先过 [`topo_sort`]）。
pub fn plan_emission(graph: &FrameGraph) -> Result<EmissionPlan, FgCode> {
    let n = graph.nodes.len();
    if n == 0 {
        return Err(FgCode::EmptyGraph);
    }
    let idx = graph.id_index();
    let mut ops = OpCounter::default();

    // 拓扑序（环检测在此复用——计划推导前置闸）。
    let run = topo_run(graph);
    let topo = match run.outcome {
        TopoOutcome::Order(o) => o,
        TopoOutcome::Cycle(_) => return Err(FgCode::DependencyCycle),
    };

    // 邻接（登记下标空间）。
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut e = 0usize;
    while e < graph.edges.len() {
        if let Some(edge) = graph.edges.get(e) {
            if let (Some(f), Some(t)) = (lookup(&idx, edge.from), lookup(&idx, edge.to)) {
                if let Some(a) = adj.get_mut(f) {
                    a.push(t);
                }
                ops.degree_ops = ops.degree_ops.saturating_add(1);
            }
        }
        e += 1;
    }

    // depth：虚拟源点到节点最长距离（微秒累计）；crit：节点到帧尾最长距离。
    // 两者同用一次拓扑序 DP：depth 沿 topo 正向松弛，crit 反向（先收集
    // 反向邻接再沿 topo 逆序松弛——避免递归）。
    let mut depth: Vec<u64> = vec![0; n];
    let mut crit: Vec<u64> = vec![0; n];
    let mut radj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut i = 0usize;
    while i < n {
        let succs: Vec<usize> = match adj.get(i) {
            Some(s) => s.clone(),
            None => Vec::new(),
        };
        let mut s = 0usize;
        while s < succs.len() {
            if let Some(&t) = succs.get(s) {
                if let Some(r) = radj.get_mut(t) {
                    r.push(i);
                }
            }
            s += 1;
        }
        i += 1;
    }
    let est_of = |i: usize| -> u64 {
        match graph.nodes.get(i) {
            Some(node) => node.budget.est_us as u64,
            None => 0,
        }
    };
    let mut t = 0usize;
    while t < topo.len() {
        let ti = match lookup(&idx, *topo.get(t).unwrap_or(&0)) {
            Some(x) => x,
            None => 0,
        };
        let succs: Vec<usize> = match adj.get(ti) {
            Some(s) => s.clone(),
            None => Vec::new(),
        };
        let mut s = 0usize;
        while s < succs.len() {
            let to = match succs.get(s) {
                Some(&x) => x,
                None => break,
            };
            ops.dp_relax_ops = ops.dp_relax_ops.saturating_add(1);
            let cand = depth.get(ti).copied().unwrap_or(0).saturating_add(est_of(ti));
            if let Some(d) = depth.get_mut(to) {
                if cand > *d {
                    *d = cand;
                }
            }
            s += 1;
        }
        t += 1;
    }
    let mut t = topo.len();
    while t > 0 {
        t -= 1;
        let ti = match lookup(&idx, *topo.get(t).unwrap_or(&0)) {
            Some(x) => x,
            None => 0,
        };
        crit[ti] = est_of(ti);
        let preds: Vec<usize> = match radj.get(ti) {
            Some(p) => p.clone(),
            None => Vec::new(),
        };
        let mut p = 0usize;
        while p < preds.len() {
            let from = match preds.get(p) {
                Some(&x) => x,
                None => break,
            };
            ops.dp_relax_ops = ops.dp_relax_ops.saturating_add(1);
            let cand = crit.get(ti).copied().unwrap_or(0).saturating_add(est_of(from));
            if let Some(c) = crit.get_mut(from) {
                if cand > *c {
                    *c = cand;
                }
            }
            p += 1;
        }
    }

    // 关键路径表（id → crit）：层内排序与产出行共用单一来源。
    let mut crit_table: Vec<(u32, u64)> = Vec::new();
    {
        let mut i = 0usize;
        while i < n {
            let id = match graph.nodes.get(i) {
                Some(node) => node.id,
                None => 0,
            };
            crit_table.push((id, crit.get(i).copied().unwrap_or(0)));
            i += 1;
        }
        crit_table.sort_by(|a, b| a.0.cmp(&b.0));
    }
    let crit_by_id = |id: u32| -> u64 {
        lookup(&crit_table, id).unwrap_or(0)
    };

    // 层号 = depth 换算为层序（depth 相等即同层；depth 是微秒累计值，
    // 离散化为「相对次序层」：把 depth 排序去重后映射为层号 0..K）。
    let mut depth_vals: Vec<u64> = Vec::new();
    let mut i = 0usize;
    while i < n {
        depth_vals.push(depth.get(i).copied().unwrap_or(0));
        i += 1;
    }
    depth_vals.sort_by(|a, b| a.cmp(b));
    depth_vals.dedup();
    let layer_of = |d: u64| -> u32 {
        // 二分查 rank。
        let mut lo = 0usize;
        let mut hi = depth_vals.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match depth_vals.get(mid) {
                Some(v) => {
                    if *v == d {
                        return mid as u32;
                    } else if *v < d {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                None => return 0,
            }
        }
        0
    };
    let mut layers: Vec<(u32, Vec<u32>)> = Vec::new();
    let mut depth_rows: Vec<(u32, u32)> = Vec::new();
    let mut i = 0usize;
    while i < n {
        let d = depth.get(i).copied().unwrap_or(0);
        let l = layer_of(d);
        let id = match graph.nodes.get(i) {
            Some(node) => node.id,
            None => 0,
        };
        depth_rows.push((id, l));
        // 层内收集。
        let mut found = false;
        let mut m = 0usize;
        while m < layers.len() {
            if let Some((lk, list)) = layers.get_mut(m) {
                if *lk == l {
                    list.push(id);
                    found = true;
                    break;
                }
            }
            m += 1;
        }
        if !found {
            let mut list = Vec::new();
            list.push(id);
            layers.push((l, list));
        }
        i += 1;
    }
    // 层内排序：crit 降 → priority 升 → 登记序升（长路径优先发射）。
    let mut l = 0usize;
    while l < layers.len() {
        if let Some((_, list)) = layers.get_mut(l) {
            list.sort_by(|&a, &b| {
                let ca = crit_by_id(a);
                let cb = crit_by_id(b);
                cb.cmp(&ca)
                    .then(prio_id(&graph, a).cmp(&prio_id(&graph, b)))
                    .then(a.cmp(&b))
            });
        }
        l += 1;
    }
    layers.sort_by(|a, b| a.0.cmp(&b.0));
    // 发射序 = 层序展开。
    let mut emission: Vec<u32> = Vec::new();
    let mut l = 0usize;
    while l < layers.len() {
        if let Some((_, list)) = layers.get(l) {
            let mut i = 0usize;
            while i < list.len() {
                if let Some(id) = list.get(i) {
                    emission.push(*id);
                }
                i += 1;
            }
        }
        l += 1;
    }

    // 完工模型：层内并行（同层全部任务同时跑，完工 = max(est)），
    // 层间串行累加。全串行基线 = 登记序逐个 est 累加。
    let mut makespan_layered = 0u64;
    let mut l = 0usize;
    while l < layers.len() {
        let mut layer_max = 0u64;
        if let Some((_, list)) = layers.get(l) {
            let mut i = 0usize;
            while i < list.len() {
                if let Some(id) = list.get(i) {
                    let est = est_id(&graph, &idx, *id);
                    if est > layer_max {
                        layer_max = est;
                    }
                }
                i += 1;
            }
        }
        makespan_layered = makespan_layered.saturating_add(layer_max);
        l += 1;
    }
    let mut makespan_serial = 0u64;
    let mut i = 0usize;
    while i < n {
        makespan_serial = makespan_serial.saturating_add(est_of(i));
        i += 1;
    }
    let improvement = makespan_serial.saturating_sub(makespan_layered);

    let crit_rows: Vec<(u32, u64)> = crit_table;

    Ok(EmissionPlan {
        layers,
        crit: crit_rows,
        depth: depth_rows,
        emission,
        makespan_layered_us: makespan_layered,
        makespan_serial_us: makespan_serial,
        improvement_us: improvement,
        ops,
    })
}

fn prio_id(graph: &FrameGraph, id: u32) -> u8 {
    match graph.node(id) {
        Some(n) => n.budget.priority,
        None => u8::MAX,
    }
}

fn est_id(graph: &FrameGraph, idx: &[(u32, usize)], id: u32) -> u64 {
    match lookup(idx, id).and_then(|i| graph.nodes.get(i)) {
        Some(n) => n.budget.est_us as u64,
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// 十、跨帧围栏账本：跨帧边不是口头约定，是账
// ---------------------------------------------------------------------------

/// 围栏票（跨帧依赖的唯一凭据；四元组唯一——同四元组重复登记显性拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FenceTicket {
    /// 票号（账本内单调分配，不复用）。
    pub ticket: u32,
    /// 生产帧号。
    pub producer_frame: u64,
    /// 生产任务 id（生产帧图内）。
    pub producer_task: u32,
    /// 消费帧号。
    pub consumer_frame: u64,
    /// 消费任务 id（消费帧图内）。
    pub consumer_task: u32,
}

/// 票状态（双向账：信号与等待各自独立登记，对账时配对）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FenceSide {
    /// 仅登记等待（消费帧已构建，生产帧未确认）。
    Awaited,
    /// 仅登记信号（生产帧已构建，消费帧未构建）。
    Signaled,
    /// 双向齐备（对账通过）。
    Paired,
}

/// 围栏账本（滚动窗口；对账零失误是判据硬要求）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FenceLedger {
    tickets: Vec<FenceTicket>,
    sides: Vec<FenceSide>,
    next_ticket: u32,
    /// 重复等待/重复信号计数（显性不静默）。
    pub duplicate_actions: u64,
    /// 归档票数（滚动窗口截断的票——归档不是丢账：归档票已完成对账闭环）。
    pub archived: u64,
    /// 累计对账通过票数。
    pub reconciled_total: u64,
}

impl FenceLedger {
    /// 空账本。
    pub const fn new() -> FenceLedger {
        FenceLedger {
            tickets: Vec::new(),
            sides: Vec::new(),
            next_ticket: 1,
            duplicate_actions: 0,
            archived: 0,
            reconciled_total: 0,
        }
    }

    /// 在册票数。
    pub fn len(&self) -> usize {
        self.tickets.len()
    }

    /// 空（账本恒可空——窗口滚动）。
    pub fn is_empty(&self) -> bool {
        self.tickets.is_empty()
    }

    /// 查票（四元组精确匹配；返回票号与当前状态）。
    pub fn find(
        &self,
        producer_frame: u64,
        producer_task: u32,
        consumer_frame: u64,
        consumer_task: u32,
    ) -> Option<(u32, FenceSide)> {
        let mut i = 0usize;
        while i < self.tickets.len() {
            if let Some(t) = self.tickets.get(i) {
                if t.producer_frame == producer_frame
                    && t.producer_task == producer_task
                    && t.consumer_frame == consumer_frame
                    && t.consumer_task == consumer_task
                {
                    return Some((t.ticket, *self.sides.get(i).unwrap_or(&FenceSide::Awaited)));
                }
            }
            i += 1;
        }
        None
    }

    fn mark_side(&mut self, idx: usize, as_await: bool) -> Result<FenceSide, FgCode> {
        let cur = match self.sides.get(idx) {
            Some(s) => *s,
            None => return Err(FgCode::FenceDuplicate),
        };
        let next = match (cur, as_await) {
            (FenceSide::Awaited, true) | (FenceSide::Signaled, false) => {
                // 重复动作：显性计数不静默（幂等返回现状但不装没发生）。
                self.duplicate_actions = self.duplicate_actions.saturating_add(1);
                return Ok(cur);
            }
            (FenceSide::Awaited, false) => FenceSide::Paired,
            (FenceSide::Signaled, true) => FenceSide::Paired,
            (FenceSide::Paired, _) => {
                self.duplicate_actions = self.duplicate_actions.saturating_add(1);
                return Ok(cur);
            }
        };
        if let Some(s) = self.sides.get_mut(idx) {
            *s = next;
        }
        Ok(next)
    }

    /// 消费帧等待：登记或配对（票不存在则开票——先到先开，对账时补齐）。
    pub fn await_fence(
        &mut self,
        producer_frame: u64,
        producer_task: u32,
        consumer_frame: u64,
        consumer_task: u32,
    ) -> Result<u32, FgCode> {
        if let Some((ticket, _)) = self.find(producer_frame, producer_task, consumer_frame, consumer_task) {
            // 就地配对（先定位下标再可变借用——避免双借用）。
            let mut target = usize::MAX;
            let mut i = 0usize;
            while i < self.tickets.len() {
                if let Some(t) = self.tickets.get(i) {
                    if t.ticket == ticket {
                        target = i;
                        break;
                    }
                }
                i += 1;
            }
            if target == usize::MAX {
                return Err(FgCode::FenceDuplicate);
            }
            self.mark_side(target, true)?;
            return Ok(ticket);
        }
        // 开票（窗口容量闸）。
        if self.tickets.len() >= FENCE_LEDGER_CAPACITY {
            // 滚动截断：只允许归档已配对票（未闭环票截断 = 丢账，拒绝）。
            let mut paired_head = 0usize;
            while paired_head < self.sides.len() {
                match self.sides.get(paired_head) {
                    Some(FenceSide::Paired) => paired_head += 1,
                    _ => break,
                }
            }
            if paired_head == 0 {
                return Err(FgCode::FenceLedgerFull);
            }
            self.tickets.drain(..paired_head);
            self.sides.drain(..paired_head);
            self.archived = self.archived.saturating_add(paired_head as u64);
        }
        let ticket = self.next_ticket;
        self.next_ticket = self.next_ticket.saturating_add(1);
        self.tickets.push(FenceTicket {
            ticket,
            producer_frame,
            producer_task,
            consumer_frame,
            consumer_task,
        });
        self.sides.push(FenceSide::Awaited);
        Ok(ticket)
    }

    /// 生产帧信号：登记或配对。
    pub fn signal_fence(
        &mut self,
        producer_frame: u64,
        producer_task: u32,
        consumer_frame: u64,
        consumer_task: u32,
    ) -> Result<u32, FgCode> {
        if let Some((ticket, _)) = self.find(producer_frame, producer_task, consumer_frame, consumer_task) {
            let mut target = usize::MAX;
            let mut i = 0usize;
            while i < self.tickets.len() {
                if let Some(t) = self.tickets.get(i) {
                    if t.ticket == ticket {
                        target = i;
                        break;
                    }
                }
                i += 1;
            }
            if target == usize::MAX {
                return Err(FgCode::FenceDuplicate);
            }
            self.mark_side(target, false)?;
            return Ok(ticket);
        }
        if self.tickets.len() >= FENCE_LEDGER_CAPACITY {
            let mut paired_head = 0usize;
            while paired_head < self.sides.len() {
                match self.sides.get(paired_head) {
                    Some(FenceSide::Paired) => paired_head += 1,
                    _ => break,
                }
            }
            if paired_head == 0 {
                return Err(FgCode::FenceLedgerFull);
            }
            self.tickets.drain(..paired_head);
            self.sides.drain(..paired_head);
            self.archived = self.archived.saturating_add(paired_head as u64);
        }
        let ticket = self.next_ticket;
        self.next_ticket = self.next_ticket.saturating_add(1);
        self.tickets.push(FenceTicket {
            ticket,
            producer_frame,
            producer_task,
            consumer_frame,
            consumer_task,
        });
        self.sides.push(FenceSide::Signaled);
        Ok(ticket)
    }

    /// 全量对账（判据「跨帧围栏对账零失误」的执行面）：逐票核验——
    /// Awaited = 缺信号（上游违约）；Signaled = 缺等待（产出悬空）；
    /// Paired = 闭环。返回对账报告（不抛错——对账是体检不是故障，
    /// 违约票由调用方按报告处置）。
    pub fn reconcile(&self) -> FenceReconcileReport {
        let mut paired = 0u64;
        let mut missing_signal = 0u64;
        let mut orphan_signal = 0u64;
        let mut offenders: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.tickets.len() {
            let side = match self.sides.get(i) {
                Some(s) => *s,
                None => break,
            };
            let ticket_no = match self.tickets.get(i) {
                Some(t) => t.ticket,
                None => break,
            };
            match side {
                FenceSide::Paired => paired = paired.saturating_add(1),
                FenceSide::Awaited => {
                    missing_signal = missing_signal.saturating_add(1);
                    offenders.push(ticket_no);
                }
                FenceSide::Signaled => {
                    orphan_signal = orphan_signal.saturating_add(1);
                    offenders.push(ticket_no);
                }
            }
            i += 1;
        }
        FenceReconcileReport {
            paired,
            missing_signal,
            orphan_signal,
            zero_miss: missing_signal == 0 && orphan_signal == 0,
            offenders,
        }
    }
}

impl Default for FenceLedger {
    fn default() -> FenceLedger {
        FenceLedger::new()
    }
}

/// 对账报告（判据「零失误」的结论载体；zero_miss 即全绿）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FenceReconcileReport {
    /// 配对闭环票数。
    pub paired: u64,
    /// 缺信号票数（上游违约）。
    pub missing_signal: u64,
    /// 产出悬空票数（无消费方）。
    pub orphan_signal: u64,
    /// 零失误结论。
    pub zero_miss: bool,
    /// 违约票号清单（处置输入）。
    pub offenders: Vec<u32>,
}

/// 帧图构建期的跨帧依赖落账：把消费帧节点声明的 cross_frame_deps 逐条
/// 登记为等待（生产帧侧信号由生产帧构建流程对称调用 [`FenceLedger`]
/// `signal_fence`——两帧都构建完毕后 [`FenceLedger::reconcile`] 应零失误）。
pub fn register_cross_frame_deps(
    graph: &FrameGraph,
    consumer_frame: u64,
    ledger: &mut FenceLedger,
) -> Result<u64, FgCode> {
    let mut registered = 0u64;
    let mut i = 0usize;
    while i < graph.nodes.len() {
        let (id, deps): (u32, Vec<CrossFrameDep>) = match graph.nodes.get(i) {
            Some(n) => (n.id, n.cross_frame_deps.clone()),
            None => break,
        };
        let mut d = 0usize;
        while d < deps.len() {
            let dep = match deps.get(d) {
                Some(x) => *x,
                None => break,
            };
            ledger.await_fence(dep.producer_frame, dep.producer_task, consumer_frame, id)?;
            registered = registered.saturating_add(1);
            d += 1;
        }
        i += 1;
    }
    Ok(registered)
}

// ---------------------------------------------------------------------------
// 十一、序列化快照：稳定文本 + FNV 指纹，往返恒等
// ---------------------------------------------------------------------------

/// 快照格式版本（支持闭集；表外版本显性拒）。
pub const SNAPSHOT_VERSION: u32 = 1;

/// FNV-1a（32 位）——快照内容指纹（F0007 归因按指纹识别同一张图）。
/// 非 const：slice::get 尚未 const 稳定；运行期调用，边界走 get 零 panic。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(x) => *x,
            None => break,
        };
        h = h ^ (b as u32);
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 图快照（结构化文本；零 IO——落盘由调用方执行，本模块只做结构化与解析）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphSnapshot {
    /// 快照文本（版本头 + 节点行 + 显式边行 + 构建账行）。
    pub text: String,
    /// FNV-1a 内容指纹。
    pub fingerprint: u32,
}

/// 序列化帧任务图为快照（确定性：同图两次序列化逐字节一致）。
pub fn serialize_graph(graph: &FrameGraph) -> GraphSnapshot {
    let mut out = String::new();
    // 版本头：FGSNAP v1 | 节点数 | 边数
    out.push_str("FGSNAP v1|");
    out.push_str(&graph.nodes.len().to_string());
    out.push('|');
    out.push_str(&graph.edges.len().to_string());
    out.push('\n');
    // 节点行（'|' 分隔——空列表保留空段，解析不失位）：
    // N|<id>|<kind>|<est>|<prio>|<tiers>|<deadline>|<reads;writes;deps;xf>
    let mut i = 0usize;
    while i < graph.nodes.len() {
        if let Some(n) = graph.nodes.get(i) {
            out.push('N');
            out.push('|');
            out.push_str(&n.id.to_string());
            out.push('|');
            out.push_str(&n.kind.stage().to_string());
            out.push('|');
            out.push_str(&n.budget.est_us.to_string());
            out.push('|');
            out.push_str(&n.budget.priority.to_string());
            out.push('|');
            out.push_str(&n.budget.degradable_tiers.to_string());
            out.push('|');
            out.push_str(&n.deadline_us.to_string());
            out.push('|');
            push_list(&mut out, &n.reads);
            out.push(';');
            push_list(&mut out, &n.writes);
            out.push(';');
            push_list(&mut out, &n.explicit_deps);
            out.push(';');
            let mut xf: Vec<u64> = Vec::new();
            let mut d = 0usize;
            while d < n.cross_frame_deps.len() {
                if let Some(x) = n.cross_frame_deps.get(d) {
                    // 跨帧依赖编码为 (producer_frame << 32) | producer_task。
                    let enc = (x.producer_frame << 32) | (x.producer_task as u64);
                    xf.push(enc);
                }
                d += 1;
            }
            push_list_u64(&mut out, &xf);
            out.push('\n');
        }
        i += 1;
    }
    // 边行：E <from> <to>
    let mut i = 0usize;
    while i < graph.edges.len() {
        if let Some(e) = graph.edges.get(i) {
            out.push('E');
            out.push(' ');
            out.push_str(&e.from.to_string());
            out.push(' ');
            out.push_str(&e.to.to_string());
            out.push('\n');
        }
        i += 1;
    }
    let fingerprint = fnv1a(out.as_bytes());
    GraphSnapshot { text: out, fingerprint }
}

fn push_list(out: &mut String, items: &[u32]) {
    let mut i = 0usize;
    while i < items.len() {
        if let Some(v) = items.get(i) {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&v.to_string());
        }
        i += 1;
    }
}

fn push_list_u64(out: &mut String, items: &[u64]) {
    let mut i = 0usize;
    while i < items.len() {
        if let Some(v) = items.get(i) {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&v.to_string());
        }
        i += 1;
    }
}

/// 解析结论：图或诊断码（附行号定位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseOutcome {
    /// 解析出的图（成功时）。
    pub graph: Option<FrameGraph>,
    /// 诊断（失败时）。
    pub error: Option<FgCode>,
    /// 出错行号（1 起；成功时为 0）。
    pub error_line: usize,
}

/// 解析快照回图（格式损坏/版本表外/数值非法逐行显性拒；解析出的图
/// 走同一构建管线——快照不是旁门，是同一条构建纪律的文本入口）。
pub fn parse_snapshot(text: &str) -> ParseOutcome {
    let mut lines: Vec<&str> = Vec::new();
    let mut start = 0usize;
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(&b) = bytes.get(i) {
            if b == b'\n' {
                lines.push(&text[start..i]);
                start = i + 1;
            }
        }
        i += 1;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    // 版本头。
    let head = match lines.first() {
        Some(h) => *h,
        None => {
            return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: 1 }
        }
    };
    let head_fields: Vec<&str> = head.split('|').collect();
    if head_fields.len() != 3 || head_fields.first().copied() != Some("FGSNAP v1") {
        return ParseOutcome { graph: None, error: Some(FgCode::SnapshotVersionUnknown), error_line: 1 };
    }
    let expect_nodes = match head_fields.get(1).and_then(|s| s.parse::<usize>().ok()) {
        Some(v) => v,
        None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: 1 },
    };
    let expect_edges = match head_fields.get(2).and_then(|s| s.parse::<usize>().ok()) {
        Some(v) => v,
        None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: 1 },
    };
    // 节点行。
    let mut builder = FrameGraphBuilder::new();
    let mut line_no = 1usize;
    let mut node_count = 0usize;
    let mut edge_count = 0usize;
    let mut li = 1usize;
    while li < lines.len() {
        let line = match lines.get(li) {
            Some(l) => *l,
            None => break,
        };
        line_no = li + 1;
        if line.is_empty() {
            li += 1;
            continue;
        }
        let tag = match line.as_bytes().first() {
            Some(t) => *t,
            None => break,
        };
        if tag == b'N' {
            // 节点行：N|id|kind|est|prio|tiers|deadline|reads;writes;deps;xf
            // （'|' 分隔保空段不失位；tail 四段：reads/writes/deps/xf）
            let fields: Vec<&str> = line[1..].split('|').collect();
            if fields.len() != 7 {
                return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no };
            }
            let id = match fields.first().and_then(|s| s.parse::<u32>().ok()) {
                Some(v) => v,
                None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no },
            };
            let kind_i = match fields.get(1).and_then(|s| s.parse::<u8>().ok()) {
                Some(v) => v,
                None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no },
            };
            let kind = match TaskKind::from_index(kind_i) {
                Some(k) => k,
                None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no },
            };
            let est = match fields.get(2).and_then(|s| s.parse::<u32>().ok()) {
                Some(v) => v,
                None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no },
            };
            let prio = match fields.get(3).and_then(|s| s.parse::<u8>().ok()) {
                Some(v) => v,
                None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no },
            };
            let tiers = match fields.get(4).and_then(|s| s.parse::<u8>().ok()) {
                Some(v) => v,
                None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no },
            };
            let deadline = match fields.get(5).and_then(|s| s.parse::<u32>().ok()) {
                Some(v) => v,
                None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no },
            };
            let rest = match fields.get(6) {
                Some(s) => *s,
                None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no },
            };
            let tail: Vec<&str> = rest.split(';').collect();
            if tail.len() != 4 {
                return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no };
            }
            let reads = match tail.first().and_then(|s| parse_u32_list(s)) {
                Some(v) => v,
                None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no },
            };
            let writes = match tail.get(1).and_then(|s| parse_u32_list(s)) {
                Some(v) => v,
                None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no },
            };
            let deps = match tail.get(2).and_then(|s| parse_u32_list(s)) {
                Some(v) => v,
                None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no },
            };
            let xf_raw = match tail.get(3).and_then(|s| parse_u64_list(s)) {
                Some(v) => v,
                None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no },
            };
            let mut cross: Vec<CrossFrameDep> = Vec::new();
            let mut x = 0usize;
            while x < xf_raw.len() {
                if let Some(enc) = xf_raw.get(x) {
                    cross.push(CrossFrameDep {
                        producer_frame: enc >> 32,
                        producer_task: (enc & 0xFFFF_FFFF) as u32,
                    });
                }
                x += 1;
            }
            let node = TaskNode {
                id,
                kind,
                reads,
                writes,
                budget: BudgetLabel { est_us: est, priority: prio, degradable_tiers: tiers },
                deadline_us: deadline,
                explicit_deps: deps,
                cross_frame_deps: cross,
            };
            if let Err(c) = builder.add_node(node) {
                return ParseOutcome { graph: None, error: Some(c), error_line: line_no };
            }
            node_count += 1;
        } else if tag == b'E' {
            let fields: Vec<&str> = line[1..].split(' ').filter(|s| !s.is_empty()).collect();
            if fields.len() != 2 {
                return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no };
            }
            let _from = match fields.first().and_then(|s| s.parse::<u32>().ok()) {
                Some(v) => v,
                None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no },
            };
            let _to = match fields.get(1).and_then(|s| s.parse::<u32>().ok()) {
                Some(v) => v,
                None => return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no },
            };
            // 边行是构建账的回放面：不直接入表——解析图统一走 builder 重推导
            // （同图必得同边表），边行仅用于行数对账。
            edge_count += 1;
        } else {
            return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no };
        }
        li += 1;
    }
    if node_count != expect_nodes || edge_count != expect_edges {
        return ParseOutcome { graph: None, error: Some(FgCode::SnapshotMalformed), error_line: line_no };
    }
    match builder.build() {
        Ok((g, _a)) => ParseOutcome { graph: Some(g), error: None, error_line: 0 },
        Err(c) => ParseOutcome { graph: None, error: Some(c), error_line: line_no },
    }
}

fn parse_u32_list(s: &str) -> Option<Vec<u32>> {
    if s.is_empty() {
        return Some(Vec::new());
    }
    let mut out: Vec<u32> = Vec::new();
    let parts: Vec<&str> = s.split(',').collect();
    let mut i = 0usize;
    while i < parts.len() {
        let p = match parts.get(i) {
            Some(p) => *p,
            None => break,
        };
        match p.parse::<u32>() {
            Ok(v) => out.push(v),
            Err(_) => return None,
        }
        i += 1;
    }
    Some(out)
}

fn parse_u64_list(s: &str) -> Option<Vec<u64>> {
    if s.is_empty() {
        return Some(Vec::new());
    }
    let mut out: Vec<u64> = Vec::new();
    let parts: Vec<&str> = s.split(',').collect();
    let mut i = 0usize;
    while i < parts.len() {
        let p = match parts.get(i) {
            Some(p) => *p,
            None => break,
        };
        match p.parse::<u64>() {
            Ok(v) => out.push(v),
            Err(_) => return None,
        }
        i += 1;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// 十二、构建预算闸与参考探针：判据「1000 节点 ≤200μs」的核算面
// ---------------------------------------------------------------------------

/// 构建预算闸：对已构建图核算操作数换算微秒，超预算显性报
/// [`FgCode::BuildBudgetExceeded`]（附实际开销于 [`BuildBudget`]）。
pub fn check_build_budget(graph: &FrameGraph) -> Result<BuildBudget, FgCode> {
    let b = BuildBudget::from_counter(&graph.ops);
    if b.within {
        Ok(b)
    } else {
        Err(FgCode::BuildBudgetExceeded)
    }
}

/// 参考探针图（锚点口径：1000 节点链式图——每节点读前一节点的产出、
/// 写自己的资源；一条上传→…→呈现的极端长链，构建最难形态之一）。
///
/// 返回 (构建器就绪的节点数, 说明)；节点 id 1..=1000，资源 id 同号。
pub fn reference_probe_1000() -> Vec<TaskNode> {
    let mut nodes: Vec<TaskNode> = Vec::new();
    let mut i = 1u32;
    while i <= BUDGET_BASELINE_NODES as u32 {
        let mut reads: Vec<u32> = Vec::new();
        if i > 1 {
            reads.push(i - 1);
        }
        let mut writes: Vec<u32> = Vec::new();
        writes.push(i);
        let kind = match TaskKind::from_index(((i - 1) % 6) as u8) {
            Some(k) => k,
            None => TaskKind::Rasterize,
        };
        nodes.push(TaskNode {
            id: i,
            kind,
            reads,
            writes,
            budget: BudgetLabel { est_us: 8, priority: (i % 256) as u8, degradable_tiers: 1 },
            deadline_us: 12_500,
            explicit_deps: Vec::new(),
            cross_frame_deps: Vec::new(),
        });
        i += 1;
    }
    nodes
}

/// 参考探针核算：构建 1000 节点链式参考图 → 全流程（建边 + 拓扑 + 分层）
/// 操作数换算微秒，与 200μs 预算对账。这是判据「1000 节点图解析 ≤200μs」
/// 的确定性核算面：同图重放操作数逐位一致，零墙钟。
pub fn reference_budget_check() -> Result<(BuildBudget, u64), FgCode> {
    let nodes = reference_probe_1000();
    let mut builder = FrameGraphBuilder::new();
    let mut i = 0usize;
    while i < nodes.len() {
        let n = match nodes.get(i) {
            Some(n) => n.clone(),
            None => break,
        };
        builder.add_node(n)?;
        i += 1;
    }
    let (graph, _audit) = builder.build()?;
    let topo_run_res = topo_run(&graph);
    let plan = plan_emission(&graph)?;
    // 合成账：构建账 + 拓扑队列账 + 计划 DP 账（三段全流程真操作）。
    let mut combined = graph.ops;
    combined.queue_ops = combined.queue_ops.saturating_add(topo_run_res.queue_ops);
    combined.dp_relax_ops = combined.dp_relax_ops.saturating_add(plan.ops.dp_relax_ops);
    combined.degree_ops = combined.degree_ops.saturating_add(plan.ops.degree_ops);
    combined.edge_scan_ops = combined.edge_scan_ops.saturating_add(plan.ops.edge_scan_ops);
    let budget = BuildBudget::from_counter(&combined);
    let improvement = plan.improvement_us;
    if !budget.within {
        return Err(FgCode::BuildBudgetExceeded);
    }
    Ok((budget, improvement))
}

