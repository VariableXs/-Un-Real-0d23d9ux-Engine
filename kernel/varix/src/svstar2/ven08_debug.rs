//! VE-F2608 · 控件树调试数据（VE-N 域 · UI 内核架构与控件树组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2608`
//!
//! **判据（锚点原文）**：三负载、按需拉取、协议家族、N 段注册、判据。
//!
//! 本条是 N 域（控件树）的**可视化面**，是 F2608/F2408/F2108 调试数据模式在
//! N 域的复刻（锚点原文点名）。对上（VE-Studio UI 编辑器）提供四类调试负载，
//! 对下（内核）只读消费 F2602 的树与 F2606 的失效计数。**它不做任何树变更
//! 的裁决**——树合法性是 F2602 的职责，本条只负责「把已经成立的树，按需、
//! 可视、口径唯一地端出去」。
//!
//! 1. **三负载**（判据一）。锚点点名对接 VE-Studio 的三负载，逐个落地为
//!    独立数据结构，**三者的采样纪律各不相同**（这是最容易写混的地方）：
//!    - **树结构流** `TreePick`：锚点子树的层级序列（类型/ID/父子/状态）。
//!      **按需拉取**（F2013 拾取家族）：调用方给「锚点节点 + 预算」，本条
//!      只返回预算内那段——这是「零常驻」的实现方式。
//!    - **属性检查流** `PropInspect`：选中节点属性实时值（F2502 编辑契约
//!      家族对接位）。**非有限值钳制**：NaN 绝不进编辑器面板——一个 NaN
//!      会顺着数值输入框污染整条属性链（同 F2408 曲线 NaN 纪律）。
//!    - **状态流** `StateStream`：节点视觉状态标记的**定容环形缓冲**。
//!      环形而非增长队列：调试流的价值在「最近」，不在「全量」。
//!
//! 2. **按需拉取**（判据二）。树洪水（万节点全画）的防线是两级：
//!    - **LOD 抽稀**：预算耗尽即停交付，但**锚点必须钉死**（`pinned
//!      anchor`）。锚点若也被裁掉，编辑器选中的节点就从画布上消失了——
//!      这是树可视最经典的「选中即蒸发」故障。抽稀后 `total_subtree`
//!      （真实子树规模）与 `vertices.len()`（实际交付点数）**分别如实
//!      记账**，绘制端才知道自己拿到的是抽稀视图。
//!    - **交付有界**：交付顶点数恒 ≤ 预算（交付面分配有界）；全树扫描
//!      只做计数（`total_subtree` 是 u32），**不复制顶点数据、不进交付
//!      面**——扫描用显式栈（与 ven02 `tree_metrics` 同一形态，瞬时栈
//!      不常驻）。洪水防线守的是**常驻与交付面**，循环次数不是敌人。
//!
//! 3. **协议家族**（判据三）。F1946 协议家族复用：属性编辑指令 → 生效 ACK
//!    （F2502 编辑契约家族——锚点原文点名）。锚点要求「属性实时调优 <1 帧」。
//!    **关于「帧」的诚实标注**：内核 no_std 无墙钟，本条以**真实工作单元
//!    计数**（`work_units`，由状态重算/脏传播/子表写的实际操作数累加，
//!    **不是自证式常数**）作为「<1 帧」的机检口径；墙钟口径由基准条目在
//!    目标机器定标。这与 F2408 对同一承诺的处理同一纪律：可机检的先机检，
//!    不可在本条定标的如实移交。
//!
//!    ACK 有**两态**（Applied / Rejected）。二者对序号水位线的处理
//!    **刻意不同**：生效推进 `last_seq`；**因重放/序号回退而拒的不推进**
//!    ——被拒的指令没有生效过，推进水位线会让同一指令二次被拒于「已见过」，
//!    掩盖真实原因。但**因参数非法（槽位越界/重排下标越界）而拒的必须推进**：
//!    那条指令已被消费方读懂并作废，不推进会让它被无限重投。
//!    调优延迟超标（工作单元超一帧预算）→ **P1 立案**（锚点原文
//!    「调优延迟超标→F1949 口径立案」——立案不是记一笔，是进 P1 计数）。
//!
//! 4. **N 段注册**（判据四）。F1764 负载族 N 段四类型注册（树结构/属性/
//!    状态/统计——**负载族扩展第十一度**，锚点原文声明）。**信封漂移 →
//!    对账拦截**：`Envelope::checksum()` 是对（kind, schema, byte_len, seq）
//!    的 FNV-1a 摘要，注册时把摘要存进 `declared`；`reconcile()` 重算并
//!    比对，不一致即判漂移。**漂移的后果不是「记一笔」而是「拦截」**：
//!    `EnvelopeRegistry` 置 `intercepted` 后，`take()` 对所有类型一律返回
//!    `None`——漂移的信封不得再被消费端取用。只加计数器不拦截，等于把
//!    红线降级成日志。
//!
//! 5. **发行版剔除零成本**（判据五）。`StripGuard` 的零成本是**可证伪的**：
//!    `payload_builds` 在 `Release` 下**恒为 0——包括被强行尝试的那次**
//!    （强行构建只会记 `forced_attempts` 与 P1，不递增 `payload_builds`）。
//!    光断「Release 下为 0」是恒真弱门禁：删掉整个字段照样通过。
//!    判据必须**双向验证**：`Debug` 档下 `payload_builds` 必须**真的递增**
//!    （对照组证明计数器不是死的），`Release` 档下连尝试路径也不递增。
//!    剔除失败 → P1（家族纪律第十一度）。
//!
//! 跨批对接：F2602 控件树（结构/状态/度量取数单源，不另造树）；F2604 属性
//!   值域（检查流复用其封闭值集，**不另造属性类型**）；F2606 失效计数
//!   （`CommitReport` 直读，秒级聚合）；F2013 拾取家族；F1946/F2502 调优
//!   协议家族；F1764 负载族 N 段；F2616 遥测（统计口径联动）。
//!
//! 诊断码段：`0x2E00 | n+1`，**专属 VE-N/F2608**。N 域段链：F2605=0x2B、
//!   F2606=0x2C、F2607=0x2D、**F2608=0x2E**（F2607 注释中「溢出撞下游域」
//!   的下游域即本条——它把基数低位留空给序号，就是为了不撞进来）。
//!
//! 零 panic 面、零 IO、无全局可变状态（所有状态由调用方持有并显式传入）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::ven02_tree::{ControlTree, PropertyKey, StateSignals, VisualState};
use super::ven04_prop::PropValue;
use super::ven06_incr::CommitReport;

// ---------------------------------------------------------------------------
// 一、诊断家族（自有码段 0x2Exx；0x2Dxx 归 F2607，0x2Cxx 归 F2606，0x2Bxx 归 F2605）
// ---------------------------------------------------------------------------

/// 诊断码（N 域 F2608 专属段：`0x2E00 | n+1`）。
///
/// **基数必须取低 4 位为 0 的整段起点**：`|` 只置位不清位，基数带低位
/// 会令相邻码塌陷成同一个（F2605 已踩过：`0x2B03 | n+1` 让 n=0..3
/// 全撞成 `0x2B03`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum N8DiagCode {
    /// 拾取锚点不在树内（显性拒绝，不静默给空图）。
    PickAnchorAbsent,
    /// 拾取预算耗尽，已按 LOD 抽稀交付。
    PickBudgetExhausted,
    /// 树洪水：子树规模超预算，已抽稀（交付 ≤ 预算，锚点钉死）。
    PickTreeFloodDecimated,
    /// 属性值非有限（NaN/Inf），已钳制后才进检查面板。
    PropNonFiniteClamped,
    /// 属性检查预算耗尽（属性条数超面板预算）。
    PropInspectBudgetExhausted,
    /// 状态流环形缓冲已覆盖最旧样本。
    StateStreamOverwritten,
    /// 统计洪水，已降档（折叠聚合）。
    StatsFloodDowngrade,
    /// 比率类统计量分母为零，口径未定义（**不等于 0**）。
    StatsRatioUndefined,
    /// 失效计数异常（applied > attempts——计数器被写坏）。
    InvalCounterAnomaly,
    /// 调优指令序号回退（重放或乱序），已拒。
    TuneSeqRegressed,
    /// 调优生效工作单元超一帧预算（F1949 口径立案——P1）。
    TuneWorkOverBudget,
    /// 调优目标槽位不存在。
    TuneTargetAbsent,
    /// 子节点重排下标越界。
    TuneReorderOutOfRange,
    /// 信封漂移：重算摘要与注册时声明不符。
    EnvelopeDrift,
    /// 信封类型未注册。
    EnvelopeUnknownPayload,
    /// 发行版构建下强行请求调试负载（剔除失败）。
    StripFailed,
}

impl N8DiagCode {
    /// 错误码（`0x2E00 | n+1` 段，专属 VE-N/F2608）。
    pub const fn code(self) -> u16 {
        0x2E00 | (self as u16) + 1
    }

    /// 线缆名。
    pub const fn as_str(self) -> &'static str {
        match self {
            N8DiagCode::PickAnchorAbsent => "PICK_ANCHOR_ABSENT",
            N8DiagCode::PickBudgetExhausted => "PICK_BUDGET_EXHAUSTED",
            N8DiagCode::PickTreeFloodDecimated => "PICK_TREE_FLOOD_DECIMATED",
            N8DiagCode::PropNonFiniteClamped => "PROP_NON_FINITE_CLAMPED",
            N8DiagCode::PropInspectBudgetExhausted => "PROP_INSPECT_BUDGET_EXHAUSTED",
            N8DiagCode::StateStreamOverwritten => "STATE_STREAM_OVERWRITTEN",
            N8DiagCode::StatsFloodDowngrade => "STATS_FLOOD_DOWNGRADE",
            N8DiagCode::StatsRatioUndefined => "STATS_RATIO_UNDEFINED",
            N8DiagCode::InvalCounterAnomaly => "INVAL_COUNTER_ANOMALY",
            N8DiagCode::TuneSeqRegressed => "TUNE_SEQ_REGRESSED",
            N8DiagCode::TuneWorkOverBudget => "TUNE_WORK_OVER_BUDGET",
            N8DiagCode::TuneTargetAbsent => "TUNE_TARGET_ABSENT",
            N8DiagCode::TuneReorderOutOfRange => "TUNE_REORDER_OUT_OF_RANGE",
            N8DiagCode::EnvelopeDrift => "ENVELOPE_DRIFT",
            N8DiagCode::EnvelopeUnknownPayload => "ENVELOPE_UNKNOWN_PAYLOAD",
            N8DiagCode::StripFailed => "STRIP_FAILED",
        }
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            N8DiagCode::PickAnchorAbsent => "拾取锚点不在树内",
            N8DiagCode::PickBudgetExhausted => "拾取预算耗尽，按 LOD 抽稀",
            N8DiagCode::PickTreeFloodDecimated => "树洪水，已抽稀交付",
            N8DiagCode::PropNonFiniteClamped => "属性值非有限，已钳制",
            N8DiagCode::PropInspectBudgetExhausted => "属性检查预算耗尽",
            N8DiagCode::StateStreamOverwritten => "状态流已覆盖最旧样本",
            N8DiagCode::StatsFloodDowngrade => "统计洪水，已降档",
            N8DiagCode::StatsRatioUndefined => "统计比率分母为零，口径未定义",
            N8DiagCode::InvalCounterAnomaly => "失效计数异常（applied 大于 attempts）",
            N8DiagCode::TuneSeqRegressed => "调优指令序号回退，已拒",
            N8DiagCode::TuneWorkOverBudget => "调优工作单元超一帧预算",
            N8DiagCode::TuneTargetAbsent => "调优目标槽位不存在",
            N8DiagCode::TuneReorderOutOfRange => "子节点重排下标越界",
            N8DiagCode::EnvelopeDrift => "信封漂移",
            N8DiagCode::EnvelopeUnknownPayload => "信封类型未注册",
            N8DiagCode::StripFailed => "发行版强行请求调试负载",
        }
    }

    /// 成因（为什么会出现——排查入口）。
    pub const fn cause(self) -> &'static str {
        match self {
            N8DiagCode::PickAnchorAbsent => "锚点 id 拼写错误或节点已被移除；查编辑器选中态与树版本",
            N8DiagCode::PickBudgetExhausted => "预算给小了；或子树确实太大，按需拉取在正常工作",
            N8DiagCode::PickTreeFloodDecimated => "万节点子树全画的防线生效；绘制端应按抽稀视图渲染",
            N8DiagCode::PropNonFiniteClamped => "上游算出 NaN/Inf；查属性引擎钳制段（F2604）",
            N8DiagCode::PropInspectBudgetExhausted => "属性条数超面板预算；面板应分页",
            N8DiagCode::StateStreamOverwritten => "消费端采样慢于生产；环形容器按设计覆盖",
            N8DiagCode::StatsFloodDowngrade => "样本触顶触发折叠降档；口径已切换到粗分辨率",
            N8DiagCode::StatsRatioUndefined => "分母为零（无帧耗时/无失效尝试）；口径未定义不是 0",
            N8DiagCode::InvalCounterAnomaly => "失效计数器被写坏；查 F2606 提交路径",
            N8DiagCode::TuneSeqRegressed => "编辑器重发旧指令；被拒属正常，勿重置水位线",
            N8DiagCode::TuneWorkOverBudget => "脏传播规模过大；查选中节点是否为高枝锚点",
            N8DiagCode::TuneTargetAbsent => "槽位映射过期（节点已删）；编辑器应先刷新选中态",
            N8DiagCode::TuneReorderOutOfRange => "重排下标超子节点数；查面板缓存是否过期",
            N8DiagCode::EnvelopeDrift => "信封字段被注册后改写；查构造口是否被绕过",
            N8DiagCode::EnvelopeUnknownPayload => "消费端拿到未注册类型；查 N 段注册表",
            N8DiagCode::StripFailed => "发行构建不应请求调试负载；查调用方档位宏",
        }
    }

    /// 全部码（供家族完整性判据）。
    pub const ALL: [N8DiagCode; 16] = [
        N8DiagCode::PickAnchorAbsent,
        N8DiagCode::PickBudgetExhausted,
        N8DiagCode::PickTreeFloodDecimated,
        N8DiagCode::PropNonFiniteClamped,
        N8DiagCode::PropInspectBudgetExhausted,
        N8DiagCode::StateStreamOverwritten,
        N8DiagCode::StatsFloodDowngrade,
        N8DiagCode::StatsRatioUndefined,
        N8DiagCode::InvalCounterAnomaly,
        N8DiagCode::TuneSeqRegressed,
        N8DiagCode::TuneWorkOverBudget,
        N8DiagCode::TuneTargetAbsent,
        N8DiagCode::TuneReorderOutOfRange,
        N8DiagCode::EnvelopeDrift,
        N8DiagCode::EnvelopeUnknownPayload,
        N8DiagCode::StripFailed,
    ];
}

/// 诊断严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 记账，不阻断。
    Minor,
    /// 显性告警：条件成立即记录，无需人工介入。
    Major,
    /// 立案：需要人看一眼。
    P1,
}

/// 一条诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 诊断码。
    pub code: N8DiagCode,
    /// 严重度。
    pub severity: Severity,
}

/// 诊断袋（**P1 是可查询的一等公民**，不另立类型——避免同一事实两处记账）。
#[derive(Clone, Debug, Default)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    /// 新建空袋。
    #[allow(clippy::new_without_default)]
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new() }
    }

    /// 记一条（记账级）。
    pub fn push(&mut self, code: N8DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::Minor });
    }

    /// 记一条显性告警。
    pub fn push_major(&mut self, code: N8DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::Major });
    }

    /// 记一条立案。
    pub fn push_p1(&mut self, code: N8DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::P1 });
    }

    /// 全部诊断。
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 某码出现次数。
    pub fn count(&self, code: N8DiagCode) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].code == code {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 某严重度条数。
    pub fn count_severity(&self, sev: Severity) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].severity == sev {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 是否出现过某码。
    pub fn has(&self, code: N8DiagCode) -> bool {
        self.count(code) > 0
    }

    /// P1 条数（立案数——剔除失败/延迟超标/漂移都必须进这里）。
    pub fn p1_count(&self) -> usize {
        self.count_severity(Severity::P1)
    }

    /// 人话渲染（非空即「诊断面不是哑巴」的可机检证据）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.items.len() {
            let d = self.items[i];
            let _ = s.push_str(&format!("[{:?}/{}] {}\n", d.severity, d.code.code(), d.code.label()));
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 二、树结构流（按需拉取 + LOD 抽稀 + 锚点钉死）
// ---------------------------------------------------------------------------

/// 单次拾取交付顶点数上限（编辑器一次性缓冲尺度）。
pub const MAX_TREE_VERTICES: usize = 128;

/// 扫描防环护栏（与 ven02 树遍历同一量纲——树不变量已保证无环，
/// 护栏只防「不变量被破坏时的调试面自己也跟着挂」）。
const SCAN_GUARD: usize = 4096;

/// 一个交付顶点（编辑器树图的绘制数据）。
#[derive(Clone, Debug, PartialEq)]
pub struct NodeVertex {
    /// 节点 id。
    pub id: String,
    /// 控件种类标签（复用 F2602 `kind`，不另造分类）。
    pub kind: String,
    /// 距锚点深度（锚点为 0）。
    pub depth: u32,
    /// 父顶点在本次交付序列中的下标（锚点为 `None`）。
    ///
    /// BFS 交付序保证「父先于子」：父顶点若没进预算，其子也进不来——
    /// 所以 `Some(pos)` 恒指向本序列内已交付的顶点，绘制端无需二次查表。
    pub parent_pos: Option<u16>,
    /// 真实子节点数（**抽稀前**——交付不全时绘制端据此显示「还有 N 子」）。
    pub child_count: u32,
    /// 当前视觉状态（复用 F2602 状态机求值结果，不重算）。
    pub state: VisualState,
}

/// 一次子树拾取的结果。
#[derive(Clone, Debug, PartialEq)]
pub struct TreePick {
    /// 锚点 id（**恒已交付**——钉死纪律）。
    pub anchor: String,
    /// 交付顶点（BFS 序，长度 ≤ 预算）。
    pub vertices: Vec<NodeVertex>,
    /// 锚点真实子树规模（**抽稀前**，含锚点自身）。
    pub total_subtree: u32,
    /// 本次实际考察的节点数（**真实工作单元**，非自证式算术）。
    pub scanned: u32,
    /// 是否发生了 LOD 抽稀。
    pub decimated: bool,
}

impl TreePick {
    /// 交付顶点数。
    pub fn delivered(&self) -> usize {
        self.vertices.len()
    }

    /// 被抽稀掉的节点数（`total - delivered`；未抽稀时为 0）。
    pub fn dropped(&self) -> u32 {
        self.total_subtree.saturating_sub(self.vertices.len() as u32)
    }

    /// 抽稀比（整数口径 `(分子, 分母)`，避免浮点——判据要精确对账）。
    pub fn decimation_ratio(&self) -> (u32, u32) {
        (self.total_subtree, self.vertices.len() as u32)
    }

    /// 空拾取（各拒绝路径共用一个构造，避免三处各写一遍）。
    fn empty(anchor: &str) -> TreePick {
        TreePick {
            anchor: String::from(anchor),
            vertices: Vec::new(),
            total_subtree: 0,
            scanned: 0,
            decimated: false,
        }
    }
}

/// 子树规模全量扫描（**只计数不分配**——洪水防线的物质基础）。
///
/// 显式栈 + 护栏：F2602 不变量保证无环，护栏只防「不变量被破坏时调试面
/// 跟着挂」——调试面的第一美德是**自己先不倒**。
fn count_subtree(tree: &ControlTree, root: &str) -> u32 {
    let mut total = 0u32;
    let mut stack: Vec<String> = Vec::new();
    stack.push(String::from(root));
    let mut guard = 0usize;
    while let Some(cur) = stack.pop() {
        guard += 1;
        if guard > SCAN_GUARD {
            break;
        }
        if let Some(n) = tree.raw(&cur) {
            total = total.saturating_add(1);
            let mut i = 0usize;
            while i < n.children.len() {
                stack.push(n.children[i].clone());
                i += 1;
            }
        }
    }
    total
}

/// **按需拾取**：只返回锚点子树中预算内的前缀（BFS 序），超预算抽稀。
///
/// **锚点钉死纪律**：锚点顶点**必进**输出。锚点若被预算裁掉，编辑器
/// 选中的节点从画布上消失——选中即蒸发。BFS 序天然「父先于子」，
/// 交付序列中每个 `parent_pos` 恒指向本序列内已交付的顶点。
///
/// `budget == 0` 时返回空并记 `PICK_BUDGET_EXHAUSTED`——**不静默给全量**
/// （那正是洪水场景要防的事）。锚点不存在时显性拒绝并记
/// `PICK_ANCHOR_ABSENT`——不静默给空图（空图与「树真没了」是两回事）。
pub fn pick_subtree(
    tree: &ControlTree,
    anchor: &str,
    budget: usize,
    bag: &mut DiagBag,
) -> TreePick {
    if tree.raw(anchor).is_none() {
        bag.push_major(N8DiagCode::PickAnchorAbsent);
        return TreePick::empty(anchor);
    }

    // 全量规模先记账（只计数不分配——这就是「零常驻」的实现方式）。
    let total_subtree = count_subtree(tree, anchor);

    if budget == 0 {
        bag.push_major(N8DiagCode::PickBudgetExhausted);
        return TreePick { anchor: String::from(anchor), vertices: Vec::new(), total_subtree, scanned: total_subtree, decimated: false };
    }

    let decimated = total_subtree as usize > budget;
    if decimated {
        bag.push(N8DiagCode::PickBudgetExhausted);
        bag.push_major(N8DiagCode::PickTreeFloodDecimated);
    }

    // BFS 交付：父先于子；预算耗尽即停交付（扫描已完成，规模账已记全）。
    let mut vertices: Vec<NodeVertex> = Vec::new();
    // (节点 id, 深度, 父顶点下标)
    let mut queue: Vec<(String, u32, Option<u16>)> = Vec::new();
    queue.push((String::from(anchor), 0u32, None));
    let mut guard = 0usize;
    let mut qi = 0usize;
    while qi < queue.len() {
        guard += 1;
        if guard > SCAN_GUARD {
            break;
        }
        if vertices.len() >= budget {
            break;
        }
        let (cur, depth, parent_pos) = {
            let item = &queue[qi];
            (item.0.clone(), item.1, item.2)
        };
        qi += 1;
        if let Some(n) = tree.raw(&cur) {
            let my_pos = vertices.len() as u16;
            vertices.push(NodeVertex {
                id: cur.clone(),
                kind: String::from(""),
                depth,
                parent_pos,
                child_count: n.children.len() as u32,
                state: n.state,
            });
            // kind 单独填（借不可变借读，避开可变/不可变同时借用）。
            if let Some(last) = vertices.last_mut() {
                last.kind = n.kind.clone();
            }
            let mut i = 0usize;
            while i < n.children.len() {
                queue.push((n.children[i].clone(), depth + 1, Some(my_pos)));
                i += 1;
            }
        }
    }

    let scanned = total_subtree;
    TreePick { anchor: String::from(anchor), vertices, total_subtree, scanned, decimated }
}

// ---------------------------------------------------------------------------
// 三、属性检查流（选中节点属性实时值 · 非有限钳制）
// ---------------------------------------------------------------------------

/// 属性检查交付上限（属性面板一屏尺度）。
pub const MAX_PROP_CELLS: usize = 32;

/// 摘要渲染最大字符数（面板单元不吞整段文本——超长截断显性标注）。
pub const PROP_SUMMARY_MAX: usize = 24;

/// 一个属性面板单元。
#[derive(Clone, Debug, PartialEq)]
pub struct PropCell {
    /// 属性键（复用 F2602 封闭键集，不另造键）。
    pub key: PropertyKey,
    /// 类型标签字符串（显式映射，不用 Debug 格式化）。
    pub type_tag: &'static str,
    /// 值摘要（**有界渲染**，超长截断加省略号——面板不吞整段文本）。
    pub summary: String,
    /// 数值是否被钳制过（非有限值 → 0）。
    pub clamped: bool,
}

/// 一次属性检查的结果。
#[derive(Clone, Debug, PartialEq)]
pub struct PropInspect {
    /// 交付单元（长度 ≤ 预算）。
    pub cells: Vec<PropCell>,
    /// 属性总条数（**抽稀前**）。
    pub total_props: u32,
    /// 本次实际考察的条数（真实工作单元）。
    pub scanned: u32,
    /// 非有限值被钳制的个数。
    pub clamped: u32,
    /// 是否发生了预算抽稀。
    pub decimated: bool,
}

impl PropInspect {
    /// 交付单元数。
    pub fn delivered(&self) -> usize {
        self.cells.len()
    }

    /// 被抽稀掉的条数。
    pub fn dropped(&self) -> u32 {
        self.total_props.saturating_sub(self.cells.len() as u32)
    }
}

/// 值摘要渲染（有界；非有限数值钳 0 并如实标记）。
fn summarize_value(key: PropertyKey, value: &PropValue, summary_max: usize) -> (PropCell, bool) {
    let (type_tag, body, clamped): (&'static str, String, bool) = match value {
        PropValue::Bool(b) => ("bool", String::from(if *b { "true" } else { "false" }), false),
        PropValue::Number(v) => {
            if v.is_finite() {
                ("number", format!("{}", v), false)
            } else {
                // NaN/Inf 绝不进面板——钳 0 + 计数（与 F2408 曲线 NaN 同纪律）。
                ("number", String::from("0"), true)
            }
        }
        PropValue::Text(t) => ("text", t.clone(), false),
        PropValue::Color(c) => ("color", format!("{:08x}", c), false),
    };
    // 有界截断：超长截断加省略号（省略号占 1 位，原文最多 summary_max-1）。
    let summary = if body.chars().count() > summary_max {
        let mut s = String::new();
        let mut used = 0usize;
        let mut it = body.chars();
        while used + 1 < summary_max {
            match it.next() {
                Some(ch) => {
                    s.push(ch);
                    used += 1;
                }
                None => break,
            }
        }
        s.push('…');
        s
    } else {
        String::from(body)
    };
    (PropCell { key, type_tag, summary, clamped }, clamped)
}

/// **属性检查**：把选中节点的属性快照端给面板，超预算抽稀。
///
/// `cells` 是 `(键, 值)` 快照（由调用方从 F2604 属性引擎抄出——本条不回查
/// 属性引擎，检查面只读快照，避免与属性写路径竞争）。非有限数值钳 0 +
/// 计数 + 告警三件齐做，不静默。
pub fn inspect_props(
    cells: &[(PropertyKey, PropValue)],
    budget: usize,
    bag: &mut DiagBag,
) -> PropInspect {
    let total = cells.len() as u32;
    if budget == 0 {
        bag.push_major(N8DiagCode::PropInspectBudgetExhausted);
        return PropInspect {
            cells: Vec::new(),
            total_props: total,
            scanned: total,
            clamped: 0,
            decimated: false,
        };
    }
    let decimated = cells.len() > budget;
    if decimated {
        bag.push(N8DiagCode::PropInspectBudgetExhausted);
    }
    let mut out: Vec<PropCell> = Vec::new();
    let mut clamped_total = 0u32;
    let mut i = 0usize;
    while i < cells.len() && out.len() < budget {
        let (cell, was_clamped) = summarize_value(cells[i].0, &cells[i].1, PROP_SUMMARY_MAX);
        if was_clamped {
            clamped_total += 1;
        }
        out.push(cell);
        i += 1;
    }
    if clamped_total > 0 {
        bag.push_major(N8DiagCode::PropNonFiniteClamped);
    }
    PropInspect {
        cells: out,
        total_props: total,
        scanned: total,
        clamped: clamped_total,
        decimated,
    }
}

// ---------------------------------------------------------------------------
// 四、状态流（定容环形缓冲 · 覆盖即记账）
// ---------------------------------------------------------------------------

/// 状态流默认容量。
pub const STATE_STREAM_CAP: usize = 256;

/// 一个状态流样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateSample {
    /// 帧号（采样时刻的帧计数，由调用方给）。
    pub frame: u32,
    /// 节点槽位（编辑器侧槽位映射的下标，非树内 id——槽位由 [`TreeMirror`] 持有）。
    pub node_slot: u32,
    /// 视觉状态（复用 F2602 状态机，不重算）。
    pub state: VisualState,
}

/// 状态流：定容环，**只保最近**。
///
/// 为什么环形而不是增长队列：调试流的消费端是「当前帧附近的状态」，
/// 旧样本没有价值；环形把常驻内存钉死在 `cap × sizeof(StateSample)`，
/// 与帧数、运行时长**无关**——这是「零常驻」的第二道保障。
#[derive(Clone, Debug)]
pub struct StateStream {
    buf: Vec<StateSample>,
    head: usize,
    filled: usize,
    overwritten: u32,
}

impl StateStream {
    /// 新建（容量一次分配，此后不再增长）。
    #[allow(clippy::new_without_default)]
    pub fn new() -> StateStream {
        StateStream::with_capacity(STATE_STREAM_CAP)
    }

    /// 指定容量。
    pub fn with_capacity(cap: usize) -> StateStream {
        let cap = cap.max(1);
        let mut buf: Vec<StateSample> = Vec::new();
        let mut i = 0usize;
        while i < cap {
            buf.push(StateSample { frame: 0, node_slot: 0, state: VisualState::Normal });
            i += 1;
        }
        StateStream { buf, head: 0, filled: 0, overwritten: 0 }
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// 有效样本数。
    pub fn len(&self) -> usize {
        self.filled
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// 被覆盖的样本累计数。
    pub fn overwritten(&self) -> u32 {
        self.overwritten
    }

    /// 推入一个样本。返回 `false` 表示**覆盖了最旧样本**（已记账）。
    pub fn push(&mut self, sample: StateSample, bag: &mut DiagBag) -> bool {
        let cap = self.buf.len();
        let mut idx = self.head;
        if idx >= cap {
            idx = 0;
        }
        match self.buf.get_mut(idx) {
            Some(slot) => *slot = sample,
            None => return false,
        }
        self.head = idx + 1;
        if self.head >= cap {
            self.head = 0;
        }
        if self.filled < cap {
            self.filled += 1;
            true
        } else {
            self.overwritten += 1;
            bag.push(N8DiagCode::StateStreamOverwritten);
            false
        }
    }

    /// 最新样本（`None` 当且仅当流为空）。
    pub fn latest(&self) -> Option<&StateSample> {
        if self.filled == 0 {
            return None;
        }
        let cap = self.buf.len();
        let mut idx = if self.head == 0 { cap - 1 } else { self.head - 1 };
        if idx >= cap {
            idx = cap - 1;
        }
        self.buf.get(idx)
    }

    /// 某节点槽位在窗内的状态迁移次数（状态机可视的机检口径）。
    pub fn transitions_of(&self, node_slot: u32) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.filled {
            let cap = self.buf.len().max(1);
            let idx = (self.head + cap - 1 - i) % cap;
            let prev_idx = (idx + cap - 1) % cap;
            if let (Some(cur), Some(prev)) = (self.buf.get(idx), self.buf.get(prev_idx)) {
                if cur.node_slot == node_slot
                    && prev.node_slot == node_slot
                    && i + 1 < self.filled
                    && cur.state != prev.state
                {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 五、树统计（三度量 + 每帧失效计数 · 秒级聚合 · 洪水降档）
// ---------------------------------------------------------------------------

/// 统计窗样本上限（超此即降档）。
pub const STAT_WINDOW_MAX_SAMPLES: u32 = 256;

/// 百万分比（比率类统计量的整数口径——避免浮点除法的舍入争议）。
pub const PPM: u32 = 1_000_000;

/// 一帧统计观测（单帧增量）。
///
/// 三规模度量直读 F2602 `tree_metrics`（**取数单源**——本条不重写树遍历）；
/// 失效三计数直读 F2606 `CommitReport`（**取数单源**——口径与增量器一致）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameSample {
    /// 节点总数。
    pub node_count: u32,
    /// 最大深度。
    pub max_depth: u32,
    /// 最大兄弟数（最宽处）。
    pub max_width: u32,
    /// 本帧失效入队尝试次数（`CommitReport::attempts`）。
    pub invalid_attempts: u32,
    /// 本帧失效实际应用次数（`CommitReport::applied`）。
    pub invalid_applied: u32,
    /// 本帧因合并省掉的次数（`CommitReport::merged_away`）。
    pub invalid_merged: u32,
}

/// 统计窗（秒级聚合的累加体）。
#[derive(Clone, Copy, Debug, Default)]
pub struct StatWindow {
    node_count: u32,
    peak_node_count: u32,
    peak_depth: u32,
    peak_width: u32,
    invalid_attempts: u64,
    invalid_applied: u64,
    invalid_merged: u64,
    samples: u32,
    downgrades: u32,
    /// 降档步长：分辨率已折半的次数。
    stride: u32,
}

/// 统计摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatSummary {
    /// 当前节点总数。
    pub node_count: u32,
    /// 窗内峰值节点数。
    pub peak_node_count: u32,
    /// 窗内峰值深度。
    pub peak_depth: u32,
    /// 窗内峰值宽度。
    pub peak_width: u32,
    /// 失效合并率（百万分比；`None` = 口径未定义，**不是 0**）。
    pub merge_ppm: Option<u32>,
    /// 失效应用率（百万分比；`None` = 口径未定义）。
    pub applied_ppm: Option<u32>,
    /// 参与聚合的样本数。
    pub samples: u32,
    /// 降档次数。
    pub downgrades: u32,
    /// 降档步长（1 = 未降档）。
    pub stride: u32,
}

impl StatWindow {
    /// 新建空窗。
    pub fn new() -> StatWindow {
        StatWindow { stride: 1, ..Default::default() }
    }

    /// 观测一帧。返回 `true` 表示**本次触发了降档**。
    ///
    /// **降档语义**：样本数触顶时**折叠**累加体（各累加量折半、样本数折半、
    /// 步长翻倍），而不是「新旧分辨率混在一窗里」。混档会让「这一窗代表
    /// 多长时间」变得不可回答——那正是统计口径唯一性要防的事。
    ///
    /// **计数器异常前置**：`invalid_applied > invalid_attempts` 是计数器
    /// 被写坏的铁证（应用数不可能超过尝试数），进 P1——带着坏数据聚合
    /// 出来的比率没有意义，先报账再聚合。
    pub fn observe(&mut self, sample: FrameSample, bag: &mut DiagBag) -> bool {
        if sample.invalid_applied > sample.invalid_attempts {
            bag.push_p1(N8DiagCode::InvalCounterAnomaly);
        }
        self.node_count = sample.node_count;
        if sample.node_count > self.peak_node_count {
            self.peak_node_count = sample.node_count;
        }
        if sample.max_depth > self.peak_depth {
            self.peak_depth = sample.max_depth;
        }
        if sample.max_width > self.peak_width {
            self.peak_width = sample.max_width;
        }
        self.invalid_attempts = self.invalid_attempts.saturating_add(sample.invalid_attempts as u64);
        self.invalid_applied = self.invalid_applied.saturating_add(sample.invalid_applied as u64);
        self.invalid_merged = self.invalid_merged.saturating_add(sample.invalid_merged as u64);
        self.samples = self.samples.saturating_add(1);
        if self.samples >= STAT_WINDOW_MAX_SAMPLES {
            self.fold();
            bag.push(N8DiagCode::StatsFloodDowngrade);
            return true;
        }
        false
    }

    /// 折叠降档：分辨率减半，覆盖时长加倍。
    fn fold(&mut self) {
        self.invalid_attempts /= 2;
        self.invalid_applied /= 2;
        self.invalid_merged /= 2;
        self.samples /= 2;
        // 降档后峰值口径改为「当前窗内峰值」——历史峰值已被折半语义覆盖，
        // 继续挂着旧峰值会与新分辨率不同量纲（口径不得漂移）。
        self.peak_node_count = self.node_count;
        self.downgrades = self.downgrades.saturating_add(1);
        self.stride = self.stride.saturating_mul(2);
    }

    /// 窗内有效样本数。
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// 降档次数。
    pub fn downgrades(&self) -> u32 {
        self.downgrades
    }

    /// 降档步长。
    pub fn stride(&self) -> u32 {
        self.stride
    }

    /// 摘要（口径未定义的比率返回 `None` 并记账，**绝不返回 0 冒充**）。
    pub fn summary(&self, bag: &mut DiagBag) -> StatSummary {
        let merge = if self.invalid_attempts == 0 {
            bag.push_major(N8DiagCode::StatsRatioUndefined);
            None
        } else {
            let v = (self.invalid_merged as u128 * PPM as u128) / (self.invalid_attempts as u128);
            Some(if v > u32::MAX as u128 { u32::MAX } else { v as u32 })
        };
        let applied = if self.invalid_attempts == 0 {
            bag.push_major(N8DiagCode::StatsRatioUndefined);
            None
        } else {
            let v = (self.invalid_applied as u128 * PPM as u128) / (self.invalid_attempts as u128);
            Some(if v > u32::MAX as u128 { u32::MAX } else { v as u32 })
        };
        StatSummary {
            node_count: self.node_count,
            peak_node_count: self.peak_node_count,
            peak_depth: self.peak_depth,
            peak_width: self.peak_width,
            merge_ppm: merge,
            applied_ppm: applied,
            samples: self.samples,
            downgrades: self.downgrades,
            stride: self.stride,
        }
    }
}

// ---------------------------------------------------------------------------
// 六、调参协议（F1946 家族：属性编辑指令 → 生效 ACK；F2502 编辑契约）
// ---------------------------------------------------------------------------

/// 一帧工作单元预算（60Hz 口径：16_667µs 的机检替身）。
///
/// **诚实标注**：本条以工作单元代替墙钟，**不是**「已经测得 <1 帧」。
/// 墙钟定标由基准条目在目标机器完成（与 F2408 对同一承诺的处理同纪律）。
pub const TUNE_WORK_BUDGET: u32 = 64;

/// 调优指令。
///
/// **`node` 是编辑器槽位下标**（非树内 id 字符串——字符串比较每帧做
/// 千次是编辑器卡顿的经典来源；槽位映射由 [`TreeMirror`] 单源持有）。
#[derive(Clone, Debug, PartialEq)]
pub enum TuneOp {
    /// 改节点状态信号（触发 F2602 状态机重算）。
    SetSignals {
        /// 节点槽位。
        node: u32,
        /// 新信号。
        signals: StateSignals,
    },
    /// 写节点属性值（键取 F2602 封闭键集；值取 F2604 封闭值集）。
    SetProp {
        /// 节点槽位。
        node: u32,
        /// 属性键。
        key: PropertyKey,
        /// 新值。
        value: PropValue,
    },
    /// 子节点重排（把 `from` 位的子节点移到 `to` 位）。
    ReorderChild {
        /// 节点槽位。
        node: u32,
        /// 原位置。
        from: u32,
        /// 新位置。
        to: u32,
    },
}

/// 调优指令信封（带序号，ACK 按序号对账）。
#[derive(Clone, Debug, PartialEq)]
pub struct TuneCommand {
    /// 指令序号（**必须严格递增**）。
    pub seq: u64,
    /// 指令体。
    pub op: TuneOp,
}

/// 生效回执（**两态**：生效 / 拒绝）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TuneAck {
    /// 已生效，附**真实工作单元数**与是否超预算。
    Applied {
        /// 对应指令序号。
        seq: u64,
        /// 工作单元数。
        work_units: u32,
        /// 是否超一帧预算。
        over_budget: bool,
    },
    /// 已拒绝，附拒绝码。
    Rejected {
        /// 对应指令序号。
        seq: u64,
        /// 拒绝码。
        code: N8DiagCode,
    },
}

impl TuneAck {
    /// 是否生效。
    pub const fn is_applied(&self) -> bool {
        matches!(self, TuneAck::Applied { .. })
    }

    /// 是否**因序号回退而拒**（与「参数非法而拒」是两种语义，判据分开钉）。
    pub const fn is_seq_reject(&self) -> bool {
        matches!(self, TuneAck::Rejected { code: N8DiagCode::TuneSeqRegressed, .. })
    }

    /// 是否**生效但超一帧工作预算**。
    ///
    /// 此前这个事实只躺在 `Applied { over_budget }` 的变体字段里，
    /// 调用方不逐字段拆变体就问不到——而「这次超标没有」是调优面板上
    /// 必问的一句话，不该逼每个调用方去 `matches!` 一次。
    /// 恒假是错的（被拒时本就无从谈超标），恒真也是错的。
    pub const fn is_over_budget(&self) -> bool {
        matches!(self, TuneAck::Applied { over_budget: true, .. })
    }

    /// 本次生效的**真实工作单元数**（被拒时为 `0`）。
    ///
    /// 与 [`TuneAck::is_over_budget`] 同理：不拆变体就取不到「花了多少」，
    /// 于是「拿工作量做面板排序」这类需求会被逼去重算一遍子树规模。
    pub const fn work_units(&self) -> u32 {
        match self {
            TuneAck::Applied { work_units, .. } => *work_units,
            TuneAck::Rejected { .. } => 0,
        }
    }

    /// 序号。
    pub const fn seq(&self) -> u64 {
        match self {
            TuneAck::Applied { seq, .. } => *seq,
            TuneAck::Rejected { seq, .. } => *seq,
        }
    }
}

/// 单槽镜像（编辑器侧被调节点的本地视图）。
#[derive(Clone, Debug, Default)]
pub struct NodeMirror {
    /// 树内节点 id（槽位映射）。
    pub id: String,
    /// 属性值表（槽位本地——写路径的镜像，读面板直接吃）。
    pub values: Vec<(PropertyKey, PropValue)>,
    /// 可视脏标记（调优后须重取树结构流/状态流）。
    pub dirty: bool,
}

/// 调优镜像：编辑器侧被调的树状态（**由调用方持有，无全局态**）。
///
/// 树本体是 F2602 [`ControlTree`]——**复用内核真类型，不另造树**；
/// 槽位映射是编辑器侧概念（id ↔ 下标），单源持有避免两套映射漂移。
#[derive(Clone, Debug)]
pub struct TreeMirror {
    /// 树本体（F2602 真类型）。
    pub tree: ControlTree,
    /// 槽位映射（槽位下标 → 节点 id）。
    pub slots: Vec<NodeMirror>,
    /// 累计交付工作单元（**实测**，供消费端核对）。
    pub total_delivered: u32,
}

/// 调优通道。
#[derive(Clone, Copy, Debug, Default)]
pub struct TuneChannel {
    /// 已接受的最高序号（0 = 尚无）。
    pub last_seq: u64,
    /// 生效计数。
    pub applied: u32,
    /// 拒绝计数。
    pub rejected: u32,
    /// 超预算计数。
    pub over_budget: u32,
    /// 累计工作单元。
    pub total_work: u64,
}

/// 槽位 → 节点 id（越界返回 `None`，不 panic）。
fn slot_id(mirror: &TreeMirror, node: u32) -> Option<String> {
    mirror.slots.get(node as usize).map(|m| m.id.clone())
}

/// 子树规模（脏传播的真实成本——复用 [`count_subtree`]，只计数不分配）。
fn dirty_propagation_cost(tree: &ControlTree, root: &str) -> u32 {
    count_subtree(tree, root)
}

/// 应用一条调优指令。
///
/// **工作单元是实测的**：`SetSignals` 会触发该节点子树的状态可视脏传播，
/// 其成本 = 子树规模（真实节点数，非常数）；`ReorderChild` 的成本 =
/// 子节点表长（真实扫描量）。改一个高枝节点的信号之所以可能超帧，
/// 正是因为它要把整棵子树标脏。
///
/// **序号水位线的两种语义**（刻意不同，判据分开钉）：
/// - 生效 → 推进 `last_seq`；
/// - **重放/序号回退而拒 → 不推进**（未生效，不该推进，否则二次被拒于
///   「已见过」，掩盖真实原因）；
/// - **参数非法（槽位越界/重排下标越界）而拒 → 推进**（该指令已被消费方
///   作废，不推进会被无限重投）。
pub fn apply_tune(
    ch: &mut TuneChannel,
    mirror: &mut TreeMirror,
    cmd: TuneCommand,
    bag: &mut DiagBag,
) -> TuneAck {
    if cmd.seq <= ch.last_seq {
        ch.rejected = ch.rejected.saturating_add(1);
        bag.push_major(N8DiagCode::TuneSeqRegressed);
        return TuneAck::Rejected { seq: cmd.seq, code: N8DiagCode::TuneSeqRegressed };
    }

    let mut work: u32 = 1;
    match cmd.op {
        TuneOp::SetSignals { node, signals } => {
            let id = match slot_id(mirror, node) {
                Some(id) => id,
                None => {
                    // 参数非法而拒：**推进水位线**（该指令已被消费方作废）。
                    ch.last_seq = cmd.seq;
                    ch.rejected = ch.rejected.saturating_add(1);
                    bag.push_major(N8DiagCode::TuneTargetAbsent);
                    return TuneAck::Rejected { seq: cmd.seq, code: N8DiagCode::TuneTargetAbsent };
                }
            };
            // 状态重算走 F2602 状态机单源（`set_state`），不本地重写优先级。
            let applied = super::ven02_tree::set_state(&mut mirror.tree, &id, signals);
            if applied.is_err() {
                ch.last_seq = cmd.seq;
                ch.rejected = ch.rejected.saturating_add(1);
                bag.push_major(N8DiagCode::TuneTargetAbsent);
                return TuneAck::Rejected { seq: cmd.seq, code: N8DiagCode::TuneTargetAbsent };
            }
            // 脏传播：子树全体标脏（真实成本 = 子树规模）。
            work = work.saturating_add(dirty_propagation_cost(&mirror.tree, &id));
            let mut i = 0usize;
            while i < mirror.slots.len() {
                if let Some(nm) = mirror.slots.get_mut(i) {
                    if nm.id == id || is_descendant(&mirror.tree, &id, &nm.id) {
                        nm.dirty = true;
                    }
                }
                i += 1;
            }
        }
        TuneOp::SetProp { node, key, value } => {
            let slot = mirror.slots.get_mut(node as usize);
            match slot {
                Some(nm) => {
                    // 树内必须真有这个键（F2602 键集封闭——写不存在的键是
                    // 面板与树的映射漂移，显性拒绝）。
                    let key_ok = match mirror.tree.raw(&nm.id) {
                        Some(n) => {
                            let mut found = false;
                            let mut i = 0usize;
                            while i < n.property_keys.len() {
                                if n.property_keys[i] == key {
                                    found = true;
                                }
                                i += 1;
                            }
                            found
                        }
                        None => false,
                    };
                    if !key_ok {
                        ch.last_seq = cmd.seq;
                        ch.rejected = ch.rejected.saturating_add(1);
                        bag.push_major(N8DiagCode::TuneTargetAbsent);
                        return TuneAck::Rejected { seq: cmd.seq, code: N8DiagCode::TuneTargetAbsent };
                    }
                    // 数值钳制：非有限值不进镜像表（与检查流同纪律）。
                    let v = match &value {
                        PropValue::Number(x) => {
                            if x.is_finite() {
                                value.clone()
                            } else {
                                bag.push_major(N8DiagCode::PropNonFiniteClamped);
                                PropValue::Number(0.0)
                            }
                        }
                        _ => value.clone(),
                    };
                    // 同键覆盖，无键追加（槽位本地表保持键唯一）。
                    let mut replaced = false;
                    let mut i = 0usize;
                    while i < nm.values.len() {
                        if nm.values[i].0 == key {
                            nm.values[i].1 = v.clone();
                            replaced = true;
                        }
                        i += 1;
                    }
                    if !replaced {
                        nm.values.push((key, v));
                    }
                    nm.dirty = true;
                    work = 1;
                }
                None => {
                    ch.last_seq = cmd.seq;
                    ch.rejected = ch.rejected.saturating_add(1);
                    bag.push_major(N8DiagCode::TuneTargetAbsent);
                    return TuneAck::Rejected { seq: cmd.seq, code: N8DiagCode::TuneTargetAbsent };
                }
            }
        }
        TuneOp::ReorderChild { node, from, to } => {
            let id = match slot_id(mirror, node) {
                Some(id) => id,
                None => {
                    ch.last_seq = cmd.seq;
                    ch.rejected = ch.rejected.saturating_add(1);
                    bag.push_major(N8DiagCode::TuneTargetAbsent);
                    return TuneAck::Rejected { seq: cmd.seq, code: N8DiagCode::TuneTargetAbsent };
                }
            };
            let target = mirror.tree.raw_mut(&id);
            match target {
                Some(n) => {
                    let len = n.children.len();
                    let f = from as usize;
                    let t = to as usize;
                    if f >= len || t >= len {
                        ch.last_seq = cmd.seq;
                        ch.rejected = ch.rejected.saturating_add(1);
                        bag.push_major(N8DiagCode::TuneReorderOutOfRange);
                        return TuneAck::Rejected {
                            seq: cmd.seq,
                            code: N8DiagCode::TuneReorderOutOfRange,
                        };
                    }
                    // remove+insert（有界：子表长即工作单元）。
                    let moved = n.children.remove(f);
                    n.children.insert(t, moved);
                    work = len as u32;
                    if let Some(nm) = mirror.slots.get_mut(node as usize) {
                        nm.dirty = true;
                    }
                }
                None => {
                    ch.last_seq = cmd.seq;
                    ch.rejected = ch.rejected.saturating_add(1);
                    bag.push_major(N8DiagCode::TuneTargetAbsent);
                    return TuneAck::Rejected { seq: cmd.seq, code: N8DiagCode::TuneTargetAbsent };
                }
            }
        }
    }

    mirror.total_delivered = mirror.total_delivered.saturating_add(work);
    ch.last_seq = cmd.seq;
    ch.applied = ch.applied.saturating_add(1);
    ch.total_work = ch.total_work.saturating_add(work as u64);
    let over = work > TUNE_WORK_BUDGET;
    if over {
        ch.over_budget = ch.over_budget.saturating_add(1);
        // F1949 口径立案（锚点原文：调优延迟超标→立案——P1 一等公民）。
        bag.push_p1(N8DiagCode::TuneWorkOverBudget);
    }
    TuneAck::Applied { seq: cmd.seq, work_units: work, over_budget: over }
}

/// `maybe_desc` 是否为 `ancestor` 的真后代（不含自身；树不变量已保证无环，
/// 护栏只防不变量被破坏时本函数跟着挂）。
fn is_descendant(tree: &ControlTree, ancestor: &str, maybe_desc: &str) -> bool {
    let mut stack: Vec<String> = Vec::new();
    if let Some(n) = tree.raw(ancestor) {
        let mut i = 0usize;
        while i < n.children.len() {
            stack.push(n.children[i].clone());
            i += 1;
        }
    }
    let mut guard = 0usize;
    while let Some(cur) = stack.pop() {
        guard += 1;
        if guard > SCAN_GUARD {
            return false;
        }
        if cur == maybe_desc {
            return true;
        }
        if let Some(n) = tree.raw(&cur) {
            let mut i = 0usize;
            while i < n.children.len() {
                stack.push(n.children[i].clone());
                i += 1;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// 七、信封 N 段注册（F1764 负载族 · 四类型 + 漂移对账拦截 · 第十一度）
// ---------------------------------------------------------------------------

/// N 段负载类型（四类型：树结构/属性检查/状态/统计）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadKind {
    /// 树结构流负载。
    TreeStructure,
    /// 属性检查流负载。
    PropInspect,
    /// 状态流负载。
    VisualState,
    /// 统计负载。
    Stats,
}

impl PayloadKind {
    /// 全部类型（N 段取值域）。
    pub const ALL: [PayloadKind; 4] = [
        PayloadKind::TreeStructure,
        PayloadKind::PropInspect,
        PayloadKind::VisualState,
        PayloadKind::Stats,
    ];

    /// 线上编码（**显式映射**，不用 `as u16`——枚举判别值不是线上值）。
    pub const fn wire(self) -> u16 {
        match self {
            PayloadKind::TreeStructure => 0x01,
            PayloadKind::PropInspect => 0x02,
            PayloadKind::VisualState => 0x03,
            PayloadKind::Stats => 0x04,
        }
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            PayloadKind::TreeStructure => "树结构流",
            PayloadKind::PropInspect => "属性检查流",
            PayloadKind::VisualState => "状态流",
            PayloadKind::Stats => "树统计",
        }
    }

    /// 按线上编码反查。
    pub fn from_wire(w: u16) -> Option<PayloadKind> {
        PayloadKind::ALL.iter().copied().find(|k| k.wire() == w)
    }
}

/// 信封 schema 版本（N 段 v1）。
pub const ENVELOPE_SCHEMA: u16 = 1;

/// N 段注册表容量（四类型）。
pub const N_SEGMENT_SLOTS: usize = 4;

/// 一个负载信封。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Envelope {
    /// 负载类型。
    pub kind: PayloadKind,
    /// schema 版本。
    pub schema: u16,
    /// 载荷字节数。
    pub byte_len: u32,
    /// 序号（重注册即递增，用于对账）。
    pub seq: u32,
    /// 注册时声明的摘要（**对账基准**）。
    pub declared: u32,
}

/// FNV-1a 单步。
const fn fnv_step(h: u32, byte: u32) -> u32 {
    (h ^ byte).wrapping_mul(0x0100_0193)
}

impl Envelope {
    /// 造信封并写入自摘要（注册面唯一构造口——`declared` 不许外部乱填）。
    pub fn new(kind: PayloadKind, byte_len: u32, seq: u32) -> Envelope {
        let mut e = Envelope { kind, schema: ENVELOPE_SCHEMA, byte_len, seq, declared: 0 };
        e.declared = e.checksum();
        e
    }

    /// 重算摘要（对四字段 + 标签长度的 FNV-1a）。
    pub fn checksum(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        h = fnv_step(h, self.kind.wire() as u32);
        h = fnv_step(h, self.schema as u32);
        h = fnv_step(h, self.byte_len);
        h = fnv_step(h, self.seq);
        h = fnv_step(h, self.kind.label().len() as u32);
        h
    }

    /// 是否漂移（重算摘要 ≠ 声明摘要）。
    pub fn drifted(&self) -> bool {
        self.checksum() != self.declared
    }
}

/// N 段信封注册表。
#[derive(Clone, Debug)]
pub struct EnvelopeRegistry {
    slots: Vec<Option<Envelope>>,
    epoch: u32,
    drift_count: u32,
    intercepted: bool,
}

impl EnvelopeRegistry {
    /// 新建（四槽空表）。
    #[allow(clippy::new_without_default)]
    pub fn new() -> EnvelopeRegistry {
        let mut slots: Vec<Option<Envelope>> = Vec::new();
        let mut i = 0usize;
        while i < N_SEGMENT_SLOTS {
            slots.push(None);
            i += 1;
        }
        EnvelopeRegistry { slots, epoch: 0, drift_count: 0, intercepted: false }
    }

    /// 注册一个信封（**重注册即替换**）。返回是否成功。
    pub fn register(&mut self, env: Envelope) -> bool {
        let wire = env.kind.wire();
        if wire == 0 || wire as usize > N_SEGMENT_SLOTS {
            return false;
        }
        self.epoch = self.epoch.wrapping_add(1);
        match self.slots.get_mut((wire - 1) as usize) {
            Some(cell) => {
                *cell = Some(env);
                true
            }
            None => false,
        }
    }

    /// 只读查表（**不受拦截影响**——对账自身要能看到信封）。
    pub fn peek(&self, kind: PayloadKind) -> Option<&Envelope> {
        let wire = kind.wire();
        if wire == 0 || wire as usize > N_SEGMENT_SLOTS {
            return None;
        }
        self.slots.get((wire - 1) as usize).and_then(|c| c.as_ref())
    }

    /// 消费端取用（**拦截态下一律 `None`**——漂移信封不得被消费）。
    pub fn take(&self, kind: PayloadKind) -> Option<&Envelope> {
        if self.intercepted {
            return None;
        }
        self.peek(kind)
    }

    /// 已注册类型数。
    pub fn registered(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 注册代数。
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// 累计漂移次数。
    pub fn drift_count(&self) -> u32 {
        self.drift_count
    }

    /// 是否处于拦截态。
    pub fn intercepted(&self) -> bool {
        self.intercepted
    }

    /// 显式解除拦截（**必须由人确认后调用**，不随注册自动清除）。
    pub fn clear_intercept(&mut self) {
        self.intercepted = false;
    }

    /// 对账：重算全部信封摘要。返回漂移数；**有漂移即置拦截**。
    pub fn reconcile(&mut self, bag: &mut DiagBag) -> u32 {
        let mut drifted = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            let bad = match self.slots.get(i).and_then(|c| c.as_ref()) {
                Some(e) => e.drifted(),
                None => false,
            };
            if bad {
                drifted += 1;
            }
            i += 1;
        }
        if drifted > 0 {
            self.drift_count = self.drift_count.saturating_add(drifted);
            self.intercepted = true;
            bag.push_p1(N8DiagCode::EnvelopeDrift);
        }
        drifted
    }

    /// 注册表是否齐备（四类型全注册）。
    pub fn complete(&self) -> bool {
        self.registered() == N_SEGMENT_SLOTS
    }
}

// ---------------------------------------------------------------------------
// 八、发行版剔除（零成本，可证伪）
// ---------------------------------------------------------------------------

/// 构建档位。
///
/// `Default` 取 `Debug`：**调试是安全默认**——想要「零成本剔除」必须
/// 显式写 `BuildProfile::Release`。反过来的默认（发行默认）会让忘配的
/// 发行构建悄悄带上全部调试负载。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BuildProfile {
    /// 开发形态：调试负载可用。
    #[default]
    Debug,
    /// 发行形态：调试负载**零成本剔除**。
    Release,
}

/// 剔除守卫。
#[derive(Clone, Copy, Debug, Default)]
pub struct StripGuard {
    /// 构建档位。
    pub profile: BuildProfile,
    /// 已构建的调试负载数（**Release 档下恒为 0，含强行尝试路径**）。
    pub payload_builds: u64,
    /// 被强行请求的次数（Release 档下只增此项）。
    pub forced_attempts: u64,
}

impl StripGuard {
    /// 构造。
    pub const fn new(profile: BuildProfile) -> StripGuard {
        StripGuard { profile, payload_builds: 0, forced_attempts: 0 }
    }

    /// 当前档位是否允许调试负载。
    pub const fn payloads_enabled(&self) -> bool {
        matches!(self.profile, BuildProfile::Debug)
    }

    /// 请求构建一个调试负载。返回是否真的构建。
    ///
    /// **零成本的物质保证**：Release 档下 `payload_builds` **不递增**——
    /// 不是「构建完再抹掉」，而是**根本不进入构建**。强行请求只记
    /// `forced_attempts` 与一条 P1（剔除失败——家族纪律第十一度）。
    pub fn try_build(&mut self, bag: &mut DiagBag) -> bool {
        if self.payloads_enabled() {
            self.payload_builds += 1;
            true
        } else {
            self.forced_attempts += 1;
            bag.push_p1(N8DiagCode::StripFailed);
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 九、族声明与冒烟
// ---------------------------------------------------------------------------

/// 家族声明一致性（判据用）。
pub fn family_is_consistent() -> bool {
    PayloadKind::ALL.len() == N_SEGMENT_SLOTS && N8DiagCode::ALL.len() == 16
}

/// 四类型码互异（`wire()` 显式映射的机检——防两类型同码）。
pub fn wires_unique() -> bool {
    let mut seen: Vec<u16> = Vec::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < PayloadKind::ALL.len() {
        let w = PayloadKind::ALL[i].wire();
        if seen.contains(&w) {
            ok = false;
        } else {
            seen.push(w);
        }
        i += 1;
    }
    ok
}

/// 诊断码互异且全在 0x2E 段（码段专属的机检——防撞 F2605/F2606/F2607）。
pub fn codes_unique_in_segment() -> bool {
    let mut seen: Vec<u16> = Vec::new();
    let mut i = 0usize;
    while i < N8DiagCode::ALL.len() {
        let c = N8DiagCode::ALL[i].code();
        if c < 0x2E01 || c > 0x2EFF || seen.contains(&c) {
            return false;
        }
        seen.push(c);
        i += 1;
    }
    true
}

/// 码标签互异（防两码共用一句人话——那是最难查的一类缺陷）。
pub fn labels_unique() -> bool {
    let mut i = 0usize;
    while i < N8DiagCode::ALL.len() {
        let mut j = i + 1;
        while j < N8DiagCode::ALL.len() {
            if N8DiagCode::ALL[i].label() == N8DiagCode::ALL[j].label() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 线缆名互异（同上，另一个正交维度）。
pub fn wire_names_unique() -> bool {
    let mut i = 0usize;
    while i < N8DiagCode::ALL.len() {
        let mut j = i + 1;
        while j < N8DiagCode::ALL.len() {
            if N8DiagCode::ALL[i].as_str() == N8DiagCode::ALL[j].as_str() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 描述。
pub fn describe() -> String {
    let mut s = String::new();
    let _ = s.push_str("控件树调试数据：\n");
    let _ = s.push_str("· 三负载：树结构流（按需拉取 + LOD 抽稀，锚点钉死）、属性检查流（非有限钳制 + 有界摘要）、状态流（定容环形）。\n");
    let _ = s.push_str("· 树洪水两级防线：预算截停（锚点必进，否则选中即蒸发）+ 扫描只计数不分配（规模账如实记全）。\n");
    let _ = s.push_str("· 树统计口径唯一：三度量直读 F2602、失效三计数直读 F2606；分母为零返回「未定义」而非 0；applied>attempts 进 P1。\n");
    let _ = s.push_str("· 调优协议走 F1946 家族：指令 → 生效 ACK，工作单元为实测值；重放不改序号水位线，参数拒则改；超标 P1 立案。\n");
    let _ = s.push_str("· 信封 N 段四类型注册（负载族第十一度）：重算摘要对账，漂移即拦截（消费端一律取不到），不只记一笔。\n");
    let _ = s.push_str("· 发行版剔除零成本：Release 档连强行尝试都不递增构建计数。\n");
    s
}

/// 冒烟：拾一段子树 + 一次属性检查 + 一帧统计。
pub fn smoke() -> String {
    let tree = match ControlTree::new("root") {
        Ok(t) => t,
        Err(_) => return String::from("冒烟失败：树建不起来\n"),
    };
    let mut bag = DiagBag::new();
    let pick = pick_subtree(&tree, "root", 4, &mut bag);
    let cells: Vec<(PropertyKey, PropValue)> = Vec::new();
    let insp = inspect_props(&cells, 8, &mut bag);
    let mut win = StatWindow::new();
    let mut report = CommitReport::new();
    report.attempts = 10;
    report.applied = 6;
    report.merged_away = 4;
    let _ = win.observe(
        FrameSample {
            node_count: tree.size() as u32,
            max_depth: 0,
            max_width: 0,
            invalid_attempts: report.attempts as u32,
            invalid_applied: report.applied as u32,
            invalid_merged: report.merged_away as u32,
        },
        &mut bag,
    );
    let sum = win.summary(&mut bag);
    let mut s = String::new();
    let _ = s.push_str(&format!(
        "拾取 锚点={} 交付={}/{} 抽稀={} 诊断={}\n",
        pick.anchor,
        pick.delivered(),
        pick.total_subtree,
        pick.decimated,
        bag.len()
    ));
    let _ = s.push_str(&format!(
        "属性 交付={}/{} 钳制={} 统计 节点={} 合并率={:?} 样本={}\n",
        insp.delivered(),
        insp.total_props,
        insp.clamped,
        sum.node_count,
        sum.merge_ppm,
        sum.samples
    ));
    s
}
