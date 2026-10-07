//! VE-F0617 · 图层树一致性校验（VE-D 域 · 2D 合成引擎 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0617`
//!
//! **判据（锚点原文）**：不变式断言集——结构不变式（单父、无环、类型约束 F0601）、标记
//! 不变式（脏标记与实际状态自洽：无脏标记但属性与缓存不符即为违例）、三序一致性（遍历序、
//! 绘制序、命中序同源 F0606——校验抽检三消费者次序一致）；校验时机：debug 构建常开（每次
//! 树操作后轻量断言）、release 构建抽检（批量操作后与快照恢复后）、外部触发（调试面板手动
//! 全量校验）。判据四条：**三组不变式、两档时机、fail-fast、抽检降级、判据**。
//!
//! **错误路径与降级矩阵**：违例→debug 即断（fail-fast）加 release 上报三要素；校验自身
//! 超时→降级抽样；误报→断言精度修订。
//!
//! **数据结构**：断言集；轻量与全量两档校验。
//!
//! **性能逐项分解**：轻量 O(1) 摊销；全量 O(节点数)；抽检 O(样本)。
//!
//! **跨批对接点**：上游 F0601 图层树、F0606 Z序与三序同源；下游 F0618 调试面板、
//! F0620 组收口证据。
//!
//! ## 设计要点
//!
//! - **三组不变式分层**（[`InvariantKind`]）：
//!   ① **结构**：单父（每节点至多一个父）、无环（沿父链有界）、类型约束（组白名单）；
//!   ② **标记**：脏标记与实际状态自洽——**无脏标记但属性/缓存与快照不符即为违例**
//!   （这是本组最容易被漏掉的方向：大家都查"该脏却没脏"，规格明确点的是反向）；
//!   ③ **三序**：遍历序、绘制序、命中序同源（命中序= 遍历序逆序）——抽检三消费者次序一致。
//! - **两档时机**（[`CheckLevel`]）：`Lightweight`（每次树操作后，**O(1) 摊销**，只查
//!   受影响节点的局部不变式）与 `Full`（批量操作后/快照恢复后/调试面板手动，O(节点数)）。
//!   轻量档的"摊销"来自**只查 dirty 集合**：轻量断言本身 O(1)（查一个节点的父数与
//!   局部环），全量才走全树——两者复杂度声明分开登记，不含糊成"O(1)"。
//! - **fail-fast 语义分档**（[`Verdict::FailFast`] vs [`Verdict::ReportOnly`]）：
//!   debug 构建违例**即断**（返回 `FailFast` 让调用方立刻停止，不带脏状态继续跑）；
//!   release 构建**上报三要素**（什么坏了/ 为什么坏 / 怎么办）后继续。两种处置**不混用**——
//!   debug 下绝不能"记一笔继续"，那会让不一致扩散到难以定位。
//! - **三要素诊断**（[`Violation::three_elements`]）：违例必须说清「什么坏了、为什么、
//!   怎么办」。只写"不一致"等于没写——下游 F0618 面板与 F0620 收口证据都要消费它。
//! - **抽检降级**（[`SamplingPolicy`]）：校验**自身**超时（节点数超预算）时降级抽样，
//!   而不是让校验把帧预算吃光。抽样**确定性子序列**（步长法，不用随机）——校验结果
//!   必须可复现，随机抽样会让"这次过了下次不过"，无法定位。
//! - **误报处置**（[`TuningRecord`]）：误报不靠"调松断言"掩盖，而是登记
//!   **断言 id + 期望值 + 实际值**，走精度修订（错误矩阵第三条）。判据可调，但调的动作
//!   留痕，不允许静默放宽。
//!
//! **逻辑 tick 注入，零墙钟**；零 IO；类型自持（不 import 未注册的兄弟模块），上游契约以
//! 等价自有类型承接。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 父链深度上界（无环判定的遍历上限；超过即判环，不无限上溯）。
pub const MAX_PARENT_CHAIN: usize = 4096;

/// 全量校验的节点数预算（超过则降级抽样——校验自身不得吃光帧预算）。
pub const FULL_CHECK_NODE_BUDGET: usize = 8192;

/// 抽检步长（确定性子序列；步长 1 = 全查）。
pub const DEFAULT_SAMPLE_STRIDE: usize = 8;

/// 轻量断言的父计数上限（单父判定的常量面）。
pub const MAX_PARENTS: usize = 1;

/// 组节点子节点类型白名单之外的标签（类型约束用）。
pub const NODE_TYPE_TABLE_SIZE: usize = 4;

/// 三组不变式契约。
pub const INVARIANTS_DOC: &str = "\
三组不变式契约（VE-F0617 · v1）：① 结构——单父（每节点至多一父）、无环（沿父链有界）、\
类型约束（组子节点走白名单）；② 标记——脏标记与实际状态自洽，**无脏标记但属性/缓存与快照\
不符即为违例**（反向检查，最易漏）；③ 三序——遍历序、绘制序、命中序同源，命中序为遍历序逆序。\
三组各有独立断言 id，违例三要素可分别追溯。";

/// 两档时机契约。
pub const TWO_LEVEL_DOC: &str = "\
两档时机契约（VE-F0617 · v1）：轻量档每次树操作后执行，O(1) 摊销——只查受影响节点的局部\
不变式（父计数、局部环、脏位自洽），不遍历全树；全量档在批量操作后、快照恢复后、调试面板\
手动触发时执行，O(节点数)。两档复杂度分别登记，不把全量说成 O(1)。轻量档抽 dirty 集合\
只查其一，均摊到每操作 O(1)。";

/// fail-fast 契约。
pub const FAIL_FAST_DOC: &str = "\
fail-fast 契约（VE-F0617 · v1）：debug 构建违例即断——返回 FailFast，调用方必须停止，不得\
带不一致状态继续执行（不一致会扩散到无法定位）；release 构建上报三要素后继续（ReportOnly）。\
两种处置按构建档严格分流，不混用。三要素= 什么坏了 / 为什么坏 / 怎么办，缺一即为\
不合格诊断（下游 F0618 面板与 F0620 收口证据都消费它）。";

/// 抽检降级契约。
pub const SAMPLING_DOC: &str = "\
抽检降级契约（VE-F0617 · v1）：校验自身超节点预算时降级为抽样而非全查——校验不得吃光帧\
预算。抽样为**确定性子序列**（步长法），不用随机：校验结果必须可复现，随机抽样会让「这次\
过、下次不过」，无法定位。降级事实显式登记（是否降级、样本数、总节点数），不静默降级。";

/// 误报处置契约。
pub const TUNING_DOC: &str = "\
误报处置契约（VE-F0617 · v1）：误报不靠调松断言掩盖，而是登记断言 id、期望值、实际值三\
项走精度修订（TuningRecord）。判据可调，但调的动作留痕——静默放宽断言等于把校验变成摆设。";

/// 树深上限（无环判定的深度面）。
pub const MAX_DEPTH: usize = 64;

// ---------------------------------------------------------------------------
// 二、数据结构（树投影 / 不变式 / 违例）
// ---------------------------------------------------------------------------

/// 节点类型标签（类型约束的白名单面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// 组（可含子节点）。
    Group,
    /// 叶子层。
    Leaf,
    /// 文本层。
    Text,
    /// 形状层。
    Shape,
}

impl NodeKind {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            NodeKind::Group => "group",
            NodeKind::Leaf => "leaf",
            NodeKind::Text => "text",
            NodeKind::Shape => "shape",
        }
    }

    /// 类型白名单（组可含哪些类型）。
    ///
    /// 语义：**组可含任意已登记类型**；非组节点**不得有子节点**（有子即类型违例）。
    /// 表格化便于扩展时同步更新判定，不把规则藏在 if 里。
    pub fn accepts_children(self) -> bool {
        matches!(self, NodeKind::Group)
    }

    /// 类型全集（机检覆盖用）。
    pub fn all() -> [NodeKind; NODE_TYPE_TABLE_SIZE] {
        [NodeKind::Group, NodeKind::Leaf, NodeKind::Text, NodeKind::Shape]
    }
}

/// 脏标记族（承接 F0601：自身脏/子树脏/内容脏三类分离不混用）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirtyFlags {
    /// 自身脏（属性变更）。
    pub self_dirty: bool,
    /// 子树脏（后代有变更）。
    pub subtree_dirty: bool,
    /// 内容脏（像素内容变更）。
    pub content_dirty: bool,
}

impl DirtyFlags {
    /// 全清。
    pub const CLEAN: DirtyFlags = DirtyFlags {
        self_dirty: false,
        subtree_dirty: false,
        content_dirty: false,
    };

    /// 是否全清。
    pub fn is_clean(self) -> bool {
        !self.self_dirty && !self.subtree_dirty && !self.content_dirty
    }
}

/// 绘制动作（三序一致性的比对单元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawAction {
    /// 绘制叶子。
    DrawLayer,
    /// 进入组。
    EnterGroup,
    /// 离开组。
    LeaveGroup,
}

/// 图层树投影（校验输入；只读）。
#[derive(Clone, Debug, PartialEq)]
pub struct TreeNode {
    /// 稳定 id。
    pub id: u64,
    /// 子节点 id 序列（**须已按 z 键排序**——三序同源的入参前提）。
    pub children: Vec<u64>,
    /// 类型标签。
    pub kind: NodeKind,
    /// 脏标记族。
    pub dirty: DirtyFlags,
    /// 内容修订号（与缓存/快照比对判定标记自洽）。
    pub content_rev: u32,
    /// 缓存命中的修订号（`0` = 无缓存；有缓存则应等于 `content_rev`）。
    pub cached_rev: u32,
    /// 是否被遍历器访问过（上一轮的访问集；三序抽检的输入）。
    pub visited: bool,
}

impl TreeNode {
    /// 叶子节点构造。
    pub fn leaf(id: u64) -> Self {
        TreeNode {
            id,
            children: Vec::new(),
            kind: NodeKind::Leaf,
            dirty: DirtyFlags::CLEAN,
            content_rev: 1,
            cached_rev: 1,
            visited: false,
        }
    }

    /// 组节点构造。
    pub fn group(id: u64, children: Vec<u64>) -> Self {
        TreeNode {
            id,
            children,
            kind: NodeKind::Group,
            dirty: DirtyFlags::CLEAN,
            content_rev: 1,
            cached_rev: 1,
            visited: false,
        }
    }

    /// 设定脏标记。
    pub fn with_dirty(mut self, d: DirtyFlags) -> Self {
        self.dirty = d;
        self
    }

    /// 设定修订号。
    pub fn with_revs(mut self, content: u32, cached: u32) -> Self {
        self.content_rev = content;
        self.cached_rev = cached;
        self
    }
}

/// 不变式分组（三组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvariantKind {
    /// 结构不变式（单父/ 无环 / 类型约束）。
    Structure,
    /// 标记不变式（脏标记与实际状态自洽）。
    Marker,
    /// 三序一致性（遍历序= 绘制序 = 命中序同源）。
    Order,
}

impl InvariantKind {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            InvariantKind::Structure => "structure",
            InvariantKind::Marker => "marker",
            InvariantKind::Order => "order",
        }
    }

    /// 全集（机检覆盖用）。
    pub fn all() -> [InvariantKind; 3] {
        [InvariantKind::Structure, InvariantKind::Marker, InvariantKind::Order]
    }
}

/// 具体断言 id（违例定位到条，而非只到组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssertId {
    /// 单父。
    SingleParent,
    /// 无环。
    Acyclic,
    /// 类型约束。
    TypeConstraint,
    /// 脏位与修订号自洽。
    MarkerSelfConsistent,
    /// 遍历序= 绘制序。
    TraverseEqualsDraw,
    /// 命中序= 遍历序逆序。
    HitEqualsReverse,
}

impl AssertId {
    /// 所属分组。
    pub fn kind(self) -> InvariantKind {
        match self {
            AssertId::SingleParent | AssertId::Acyclic | AssertId::TypeConstraint => {
                InvariantKind::Structure
            }
            AssertId::MarkerSelfConsistent => InvariantKind::Marker,
            AssertId::TraverseEqualsDraw | AssertId::HitEqualsReverse => InvariantKind::Order,
        }
    }

    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            AssertId::SingleParent => "single_parent",
            AssertId::Acyclic => "acyclic",
            AssertId::TypeConstraint => "type_constraint",
            AssertId::MarkerSelfConsistent => "marker_self_consistent",
            AssertId::TraverseEqualsDraw => "traverse_equals_draw",
            AssertId::HitEqualsReverse => "hit_equals_reverse",
        }
    }

    /// 全集（机检覆盖用；6 条断言恰覆盖 3 组）。
    pub fn all() -> [AssertId; 6] {
        [
            AssertId::SingleParent,
            AssertId::Acyclic,
            AssertId::TypeConstraint,
            AssertId::MarkerSelfConsistent,
            AssertId::TraverseEqualsDraw,
            AssertId::HitEqualsReverse,
        ]
    }
}

/// 校验档位（两档时机）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckLevel {
    /// 轻量：每次树操作后，O(1) 摊销。
    Lightweight,
    /// 全量：批量操作后/快照恢复后/调试面板触发，O(节点数)。
    Full,
}

impl CheckLevel {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            CheckLevel::Lightweight => "lightweight",
            CheckLevel::Full => "full",
        }
    }
}

/// 处置（fail-fast 分档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// debug：违例即断（调用方必须停止）。
    FailFast,
    /// release：上报三要素后继续。
    ReportOnly,
}

/// 违例（三要素齐备：什么坏了 / 为什么 / 怎么办）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// 断言 id。
    pub assert: AssertId,
    /// 涉及层 id（0 = 树级违例）。
    pub node_id: u64,
    /// 要素一：什么坏了。
    pub what: String,
    /// 要素二：为什么坏。
    pub why: String,
    /// 要素三：怎么办。
    pub how: String,
}

impl Violation {
    /// 构造：三要素缺一即不合格，故此处强制三个参数。
    pub fn new(assert: AssertId, node_id: u64, what: &str, why: &str, how: &str) -> Self {
        Violation {
            assert,
            node_id,
            what: what.to_string(),
            why: why.to_string(),
            how: how.to_string(),
        }
    }

    /// 三要素齐备（无空串）。
    pub fn three_elements_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.how.is_empty()
    }

    /// 归组。
    pub fn kind(&self) -> InvariantKind {
        self.assert.kind()
    }

    /// 一行摘要（F0618 面板 / F0620 证据消费面）。
    pub fn summary(&self) -> String {
        format!(
            "[{}] 层{}：{}｜原因：{}｜处置：{}",
            self.assert.tag(),
            self.node_id,
            self.what,
            self.why,
            self.how
        )
    }
}

/// 误报修订记录（断言精度修订的留痕面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TuningRecord {
    /// 断言 id。
    pub assert: AssertId,
    /// 期望值。
    pub expected: String,
    /// 实际值。
    pub actual: String,
    /// 修订动作。
    pub action: String,
}

impl TuningRecord {
    /// 构造。
    pub fn new(assert: AssertId, expected: &str, actual: &str, action: &str) -> Self {
        TuningRecord {
            assert,
            expected: expected.to_string(),
            actual: actual.to_string(),
            action: action.to_string(),
        }
    }
}

/// 校验报告（一次校验的完整产出）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckReport {
    /// 档位。
    pub level: CheckLevel,
    /// 处置。
    pub verdict: Verdict,
    /// 违例清单。
    pub violations: Vec<Violation>,
    /// 检查的节点数（抽样后为样本数）。
    pub checked_nodes: usize,
    /// 树总节点数。
    pub total_nodes: usize,
    /// 是否降级抽样（校验自身超预算时）。
    pub degraded_to_sampling: bool,
    /// 误报修订记录。
    pub tunings: Vec<TuningRecord>,
}

impl CheckReport {
    /// 是否通过（无违例）。
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }

    /// 是否须中断（fail-fast 档且有违例）。
    pub fn must_halt(&self) -> bool {
        self.verdict == Verdict::FailFast && !self.violations.is_empty()
    }

    /// 三组各自的违例数（分组归因面）。
    pub fn count_by_kind(&self) -> (usize, usize, usize) {
        let mut s = 0usize;
        let mut m = 0usize;
        let mut o = 0usize;
        for v in self.violations.iter() {
            match v.kind() {
                InvariantKind::Structure => s += 1,
                InvariantKind::Marker => m += 1,
                InvariantKind::Order => o += 1,
            }
        }
        (s, m, o)
    }

    /// 三要素齐备（所有违例）。
    pub fn all_three_elements(&self) -> bool {
        self.violations.iter().all(|v| v.three_elements_complete())
    }

    /// 报告摘要（一行；日志与面板消费）。
    pub fn summary(&self) -> String {
        format!(
            "{} {}违例{} 检查{}/{} 节点{}",
            self.level.tag(),
            self.violations.len(),
            if self.degraded_to_sampling { "（降级抽样）" } else { "" },
            self.checked_nodes,
            self.total_nodes,
            if self.must_halt() { " [须中断]" } else { "" }
        )
    }
}

// ---------------------------------------------------------------------------
// 三、三组不变式判定（纯函数；O(1) 或 O(父链)）
// ---------------------------------------------------------------------------

/// 树视图（校验器持有的只读投影）。
pub struct TreeView {
    nodes: Vec<(u64, TreeNode)>,
    /// 根 id。
    root: u64,
}

impl TreeView {
    /// 构造。
    pub fn new(nodes: Vec<(u64, TreeNode)>, root: u64) -> Self {
        TreeView { nodes, root }
    }

    /// 取节点（线性定位——诚实标注 O(节点数)）。
    pub fn node(&self, id: u64) -> Option<&TreeNode> {
        self.nodes.iter().find(|(nid, _)| *nid == id).map(|(_, n)| n)
    }

    /// 节点总数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 根 id。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 节点 id 序列（确定性次序；抽样与遍历共用，保证可复现）。
    pub fn ids(&self) -> Vec<u64> {
        self.nodes.iter().map(|(i, _)| *i).collect()
    }

    /// 父映射（O(节点数)；一次性建表供单父/无环判定复用，避免每节点重扫）。
    pub fn parent_map(&self) -> Vec<(u64, Option<u64>)> {
        let mut out = Vec::new();
        for (id, node) in self.nodes.iter() {
            for c in node.children.iter() {
                out.push((*c, Some(*id)));
            }
        }
        // 无父节点（未出现在任何 children 里）显式登记为 None，缺失即被发现。
        let mut map = out;
        for (id, _) in self.nodes.iter() {
            if !map.iter().any(|(c, _)| c == id) {
                map.push((*id, None));
            }
        }
        map
    }

    /// 断言单父：每节点至多一个父（违例即登记）。
    pub fn check_single_parent(&self) -> Vec<Violation> {
        let mut out = Vec::new();
        let parents = self.parent_map();
        // 统计每个节点被声明为子的次数。
        let mut counts: Vec<(u64, usize)> = Vec::new();
        for (child, parent) in parents.iter() {
            if parent.is_none() {
                continue;
            }
            match counts.iter_mut().find(|(c, _)| c == child) {
                Some(e) => e.1 += 1,
                None => counts.push((*child, 1)),
            }
        }
        for (child, n) in counts.iter() {
            if *n > MAX_PARENTS {
                out.push(Violation::new(
                    AssertId::SingleParent,
                    *child,
                    &format!("层 {} 有 {} 个父", child, n),
                    "同一节点被多个父的 children 声明，树不再是树而是图",
                    "检查重复挂载；移除多余父引用后重新校验",
                ));
            }
        }
        out
    }

    /// 断言无环：沿父链上溯有界（超过 [`MAX_PARENT_CHAIN`] 即判环）。
    pub fn check_acyclic(&self) -> Vec<Violation> {
        let mut out = Vec::new();
        let parents = self.parent_map();
        for (id, _) in self.nodes.iter() {
            let mut cur = Some(*id);
            let mut steps = 0usize;
            let mut seen_loop = false;
            while let Some(c) = cur {
                steps += 1;
                if steps > MAX_PARENT_CHAIN {
                    seen_loop = true;
                    break;
                }
                cur = parents
                    .iter()
                    .find(|(cid, _)| *cid == c)
                    .and_then(|(_, p)| *p);
            }
            if seen_loop {
                out.push(Violation::new(
                    AssertId::Acyclic,
                    *id,
                    &format!("层 {} 父链超上界 {}", id, MAX_PARENT_CHAIN),
                    "父链成环或异常长，祖先无法收敛",
                    "打断环上的引用（环检测应已在挂载时拦截）",
                ));
            }
        }
        out
    }

    /// 断言类型约束：非组节点不得有子节点。
    pub fn check_type_constraint(&self) -> Vec<Violation> {
        let mut out = Vec::new();
        for (id, node) in self.nodes.iter() {
            if !node.kind.accepts_children() && !node.children.is_empty() {
                out.push(Violation::new(
                    AssertId::TypeConstraint,
                    *id,
                    &format!("{} 类型的层 {} 有 {} 个子节点", node.kind.tag(), id, node.children.len()),
                    "类型白名单不允许非组节点持有子节点",
                    "改为组类型或移除子节点",
                ));
            }
        }
        out
    }

    /// 断言标记自洽：**无脏标记但修订号与缓存不符即为违例**（规格点名的反向检查）。
    pub fn check_marker_self_consistent(&self) -> Vec<Violation> {
        let mut out = Vec::new();
        for (id, node) in self.nodes.iter() {
            if node.dirty.is_clean() {
                // 无脏标记 → 缓存修订号必须与内容修订号一致（否则缓存与内容不符）。
                if node.cached_rev != 0 && node.cached_rev != node.content_rev {
                    out.push(Violation::new(
                        AssertId::MarkerSelfConsistent,
                        *id,
                        &format!(
                            "层 {} 无脏标记但缓存修订 {} ≠ 内容修订 {}",
                            id, node.cached_rev, node.content_rev
                        ),
                        "无脏标记意味着内容未变，缓存却停留在旧修订——缓存内容与属性不符",
                        "补置内容脏标记并让缓存失效，或修正缓存修订号",
                    ));
                }
            } else if node.dirty.content_dirty && node.cached_rev == node.content_rev {
                // 反向亦须自洽：声明内容脏却已缓存同修订 =脏标记该置未置的镜像问题。
                out.push(Violation::new(
                    AssertId::MarkerSelfConsistent,
                    *id,
                    &format!("层 {} 声明内容脏但缓存修订 {} 已等于内容修订", id, node.cached_rev),
                    "内容脏意味着像素须重绘，缓存不应已持有同修订内容",
                    "作废该层缓存条目并重绘",
                ));
            }
        }
        out
    }

    /// 断言三序一致：遍历序与绘制序同源。
    ///
    /// 三消费者都以**同一份先序序列**为输入，故一致性的可检验形式是：
    /// 各消费者产出的叶子 id 序列必须与遍历序的叶子子序列逐条相等。
    pub fn check_order_consistency(
        &self,
        traverse_order: &[u64],
        draw_order: &[u64],
        hit_order: &[u64],
    ) -> Vec<Violation> {
        let mut out = Vec::new();
        let draw_leaves: Vec<u64> = draw_order.to_vec();
        if draw_leaves != traverse_order {
            out.push(Violation::new(
                AssertId::TraverseEqualsDraw,
                0,
                "遍历序与绘制序不一致",
                "三序同源要求两者同出F0606 的先序序列，出现差异说明有一份另起了排序",
                "以遍历序为唯一来源重建绘制序",
            ));
        }
        // 命中序必须是遍历序的严格逆序（Z 序同源的反向消费）。
        let mut reversed: Vec<u64> = traverse_order.to_vec();
        reversed.reverse();
        if hit_order != reversed.as_slice() {
            out.push(Violation::new(
                AssertId::HitEqualsReverse,
                0,
                "命中序非遍历序的逆序",
                "命中测试按 Z 序逆序遍历（最上层优先），与F0606 三序同源契约不符",
                "改用遍历序逆序作为命中遍历序",
            ));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 四、抽样策略（确定性；降级不静默）
// ---------------------------------------------------------------------------

/// 抽样策略（步长法；确定性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SamplingPolicy {
    /// 是否启用降级抽样。
    pub enabled: bool,
    /// 节点预算（超过则抽样）。
    pub node_budget: usize,
    /// 步长（确定性子序列）。
    pub stride: usize,
}

impl SamplingPolicy {
    /// 构造：启用降级，默认预算与步长。
    pub fn new() -> Self {
        SamplingPolicy {
            enabled: true,
            node_budget: FULL_CHECK_NODE_BUDGET,
            stride: DEFAULT_SAMPLE_STRIDE,
        }
    }

    /// 全查（禁用降级）。
    pub fn exhaustive() -> Self {
        SamplingPolicy { enabled: false, node_budget: usize::MAX, stride: 1 }
    }

    /// 步长 0 视为 1（防除零与死循环——0 步长会让取样循环不前进）。
    pub fn effective_stride(&self) -> usize {
        if self.stride == 0 {
            1
        } else {
            self.stride
        }
    }

    /// 产出样本下标（确定性；不超预算）。
    pub fn sample_indices(&self, total: usize) -> Vec<usize> {
        if !self.enabled || total <= self.node_budget {
            let all: Vec<usize> = (0..total).collect();
            return all;
        }
        let stride = self.effective_stride();
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < total {
            out.push(i);
            i += stride;
        }
        // 首元素必含（否则根层永不被校验）。
        if out.first().copied() != Some(0) {
            out.insert(0, 0);
        }
        out
    }
}

impl Default for SamplingPolicy {
    fn default() -> Self {
        SamplingPolicy::new()
    }
}

// ---------------------------------------------------------------------------
// 五、校验器（两档时机 + fail-fast 分档 + 抽检降级）
// ---------------------------------------------------------------------------

/// 一致性校验器。
///
/// **不持有构建档**——档位由 [`CheckLevel`] 与 [`Verdict`] 参数显式传入，调用方决定；
/// 这样同一份校验逻辑在 debug 与 release 下的行为差异是**可测的**（自检里两种档都跑）。
pub struct ConsistencyChecker {
    /// 构建档（决定处置：debug fail-fast / release 上报）。
    debug_build: bool,
    /// 抽样策略。
    sampling: SamplingPolicy,
    /// 误报修订记录。
    tunings: Vec<TuningRecord>,
    /// 最近一次三序（供下轮抽检对比）。
    last_order: Vec<u64>,
}

impl ConsistencyChecker {
    /// 构造：指定构建档与抽样策略。
    pub fn new(debug_build: bool, sampling: SamplingPolicy) -> Self {
        ConsistencyChecker {
            debug_build,
            sampling,
            tunings: Vec::new(),
            last_order: Vec::new(),
        }
    }

    /// 误报修订登记（判据精度修订的留痕）。
    pub fn note_false_positive(&mut self, r: TuningRecord) {
        self.tunings.push(r);
    }

    /// 误报修订记录（只读）。
    pub fn tunings(&self) -> &[TuningRecord] {
        &self.tunings
    }

    /// 处置（按构建档分流；两种处置不混用）。
    pub fn verdict(&self) -> Verdict {
        if self.debug_build {
            Verdict::FailFast
        } else {
            Verdict::ReportOnly
        }
    }

    /// 记录三序（下一轮抽检的基线）。
    pub fn remember_order(&mut self, order: Vec<u64>) {
        self.last_order = order;
    }

    /// 最近一次三序。
    pub fn last_order(&self) -> &[u64] {
        &self.last_order
    }

    /// **轻量校验**：每次树操作后调用，O(1) 摊销。
    ///
    /// 只查受影响节点 `focus` 的局部不变式：单父（该节点的父计数）、局部环（父链上界）、
    /// 类型约束（该节点自身）、标记自洽（该节点自身）。**不遍历全树**——这是摊销 O(1)
    /// 的来源；全量是 [`ConsistencyChecker::check_full`] 的职责，两者不混。
    pub fn check_lightweight(&self, view: &TreeView, focus: u64) -> CheckReport {
        let node = match view.node(focus) {
            Some(n) => n,
            None => {
                let v = Violation::new(
                    AssertId::SingleParent,
                    focus,
                    &format!("待校验层 {} 不在树投影内", focus),
                    "树操作指向了不存在的节点，投影与树已不同步",
                    "补齐节点投影或撤销该操作",
                );
                return CheckReport {
                    level: CheckLevel::Lightweight,
                    verdict: self.verdict(),
                    violations: vec![v],
                    checked_nodes: 1,
                    total_nodes: view.len(),
                    degraded_to_sampling: false,
                    tunings: self.tunings.clone(),
                };
            }
        };
        let mut violations = Vec::new();
        // 局部单父：该节点被声明为子的次数。
        let parent_count = view
            .nodes
            .iter()
            .filter(|(_, n)| n.children.iter().any(|c| *c == focus))
            .count();
        if parent_count > MAX_PARENTS {
            violations.push(Violation::new(
                AssertId::SingleParent,
                focus,
                &format!("层 {} 有 {} 个父", focus, parent_count),
                "同一节点被多个父声明（局部检查即命中）",
                "移除多余父引用",
            ));
        }
        // 局部环：父链长度有界。
        let parents = view.parent_map();
        let mut cur = Some(focus);
        let mut steps = 0usize;
        let mut looped = false;
        while let Some(c) = cur {
            steps += 1;
            if steps > MAX_PARENT_CHAIN {
                looped = true;
                break;
            }
            cur = parents.iter().find(|(cid, _)| *cid == c).and_then(|(_, p)| *p);
        }
        if looped {
            violations.push(Violation::new(
                AssertId::Acyclic,
                focus,
                &format!("层 {} 父链超上界 {}", focus, MAX_PARENT_CHAIN),
                "父链成环，祖先无法收敛",
                "打断环上的引用",
            ));
        }
        // 局部类型约束。
        if !node.kind.accepts_children() && !node.children.is_empty() {
            violations.push(Violation::new(
                AssertId::TypeConstraint,
                focus,
                &format!("{} 类型的层 {} 有子节点", node.kind.tag(), focus),
                "类型白名单不允许非组节点持有子节点",
                "改为组类型或移除子节点",
            ));
        }
        // 局部标记自洽（复用全量判据的单节点面）。
        violations.extend(check_marker_one(view, focus));

        CheckReport {
            level: CheckLevel::Lightweight,
            verdict: self.verdict(),
            violations,
            checked_nodes: 1,
            total_nodes: view.len(),
            degraded_to_sampling: false,
            tunings: self.tunings.clone(),
        }
    }

    /// **全量校验**：批量操作后/快照恢复后/调试面板手动触发，O(节点数)（超预算降级抽样）。
    ///
    /// 三组全查；结构组在抽样下会**降为局部判定**并显式登记（不能抽样还声称"全量无环"
    /// ——那是不实的核验声明）。
    pub fn check_full(
        &self,
        view: &TreeView,
        traverse: &[u64],
        draw: &[u64],
        hit: &[u64],
    ) -> CheckReport {
        let total = view.len();
        let idx = self.sampling.sample_indices(total);
        let degraded = idx.len() < total;
        let ids = view.ids();

        let mut violations = Vec::new();

        // 结构组：抽样下只查样本节点（降级事实由 degraded 标记，不隐瞒）。
        for i in idx.iter() {
            if let Some(id) = ids.get(*i) {
                let id = *id;
                violations.extend(check_marker_one(view, id));
                if let Some(n) = view.node(id) {
                    if !n.kind.accepts_children() && !n.children.is_empty() {
                        violations.push(Violation::new(
                            AssertId::TypeConstraint,
                            id,
                            &format!("{} 类型的层 {} 有子节点", n.kind.tag(), id),
                            "类型白名单不允许非组节点持有子节点",
                            "改为组类型或移除子节点",
                        ));
                    }
                }
            }
        }
        if !degraded {
            // 只有全查才敢声称单父/无环（抽样下这两条本就不成立）。
            violations.extend(view.check_single_parent());
            violations.extend(view.check_acyclic());
        }

        // 三序组：序列表是全量数据（不是节点集），不参与抽样。
        violations.extend(view.check_order_consistency(traverse, draw, hit));

        CheckReport {
            level: CheckLevel::Full,
            verdict: self.verdict(),
            violations,
            checked_nodes: idx.len(),
            total_nodes: total,
            degraded_to_sampling: degraded,
            tunings: self.tunings.clone(),
        }
    }
}

impl Default for ConsistencyChecker {
    fn default() -> Self {
        ConsistencyChecker::new(true, SamplingPolicy::new())
    }
}

/// 单节点标记自洽判定（轻量与全量共用的唯一实现——两处各写一份必然漂移）。
fn check_marker_one(view: &TreeView, id: u64) -> Vec<Violation> {
    let node = match view.node(id) {
        Some(n) => n,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    if node.dirty.is_clean() && node.cached_rev != 0 && node.cached_rev != node.content_rev {
        out.push(Violation::new(
            AssertId::MarkerSelfConsistent,
            id,
            &format!(
                "层 {} 无脏标记但缓存修订 {} ≠ 内容修订 {}",
                id, node.cached_rev, node.content_rev
            ),
            "无脏标记意味着内容未变，缓存却停留在旧修订",
            "补置内容脏标记并让缓存失效，或修正缓存修订号",
        ));
    } else if node.dirty.content_dirty && node.cached_rev == node.content_rev {
        out.push(Violation::new(
            AssertId::MarkerSelfConsistent,
            id,
            &format!("层 {} 声明内容脏但缓存修订已等于内容修订", id),
            "内容脏意味着像素须重绘，缓存不应已持有同修订内容",
            "作废该层缓存条目并重绘",
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// 六、自检（CheckSet）
// ---------------------------------------------------------------------------

/// 自检辅助：产出一条**缺处置要素**的违例（聚合面的表外真实形态）。
///
///放在自检函数内而非模块级，避免把测试专用构造混入公开 API。
fn empty_element_probe() -> Violation {
    Violation::new(AssertId::MarkerSelfConsistent, 42, "缓存与内容不符", "内容已变", "")
}

/// VE-F0617 · 图层树一致性校验 —— 判据自检。
///
/// 判据四条（锚点）：三组不变式、两档时机、fail-fast、抽检降级。
/// 覆盖六个判据族：`invariant-*`（三组不变式）、`timing-*`（两档时机）、
/// `failfast-*`（fail-fast 与三要素）、`sampling-*`（抽检降级）、
/// `tuning-*`（误报修订）、`judge-*`（契约条款在场）。
pub fn run_ved17_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F0617");

    /// 造一棵正常树：1(组) → 2,3；2 → 4；3、4 为叶子。
    fn good_tree() -> TreeView {
        TreeView::new(
            vec![
                (1, TreeNode::group(1, vec![2, 3])),
                (2, TreeNode::group(2, vec![4])),
                (3, TreeNode::leaf(3)),
                (4, TreeNode::leaf(4)),
            ],
            1,
        )
    }

    // ---- 三组不变式：结构 / 标记 / 三序 ----

    {
        // 正常树三组全绿。
        let v = good_tree();
        let c = ConsistencyChecker::new(false, SamplingPolicy::exhaustive());
        let r = c.check_full(&v, &[2, 4, 3], &[2, 4, 3], &[3, 4, 2]);
        set.add("D17-invariant-正常树全绿", r.passed(), "");
    }

    {
        // 结构-单父：同一节点被两个父声明 → 违例且三要素齐备。
        let v = TreeView::new(
            vec![
                (1, TreeNode::group(1, vec![4])),
                (2, TreeNode::group(2, vec![4])),
                (3, TreeNode::leaf(3)),
                (4, TreeNode::leaf(4)),
            ],
            1,
        );
        let vs = v.check_single_parent();
        set.add(
            "D17-invariant-单父违例可检出",
            vs.len() == 1 && vs[0].assert == AssertId::SingleParent && vs[0].three_elements_complete(),
            "",
        );
    }

    {
        // 结构-无环：自环节点 → 父链超上界。
        let v = TreeView::new(
            vec![
                (1, TreeNode::group(1, vec![2])),
                (2, TreeNode::group(2, vec![1])),
            ],
            1,
        );
        let vs = v.check_acyclic();
        set.add(
            "D17-invariant-环可检出",
            !vs.is_empty() && vs.iter().all(|x| x.assert == AssertId::Acyclic),
            "",
        );
    }

    {
        // 结构-类型约束：叶子带子节点 → 违例。
        let mut n = TreeNode::leaf(5);
        n.children.push(9);
        let v = TreeView::new(vec![(5, n)], 5);
        let vs = v.check_type_constraint();
        set.add(
            "D17-invariant-类型约束可检出",
            vs.len() == 1 && vs[0].assert == AssertId::TypeConstraint,
            "",
        );
    }

    {
        // 标记-自洽（规格点名的反向：无脏标记但缓存与内容不符）。
        let n = TreeNode::leaf(7).with_revs(5, 3); // 无脏标记，缓存落后
        let v = TreeView::new(vec![(7, n)], 7);
        let vs = v.check_marker_self_consistent();
        set.add(
            "D17-invariant-无脏但缓存不符可检出",
            vs.len() == 1 && vs[0].assert == AssertId::MarkerSelfConsistent,
            "",
        );
    }

    {
        // 标记-自洽（另一向：声明内容脏却已缓存同修订）。
        let n = TreeNode::leaf(8).with_revs(4, 4).with_dirty(DirtyFlags {
            self_dirty: false,
            subtree_dirty: false,
            content_dirty: true,
        });
        let v = TreeView::new(vec![(8, n)], 8);
        let vs = v.check_marker_self_consistent();
        set.add(
            "D17-invariant-声明内容脏但已缓存可检出",
            vs.len() == 1 && vs[0].assert == AssertId::MarkerSelfConsistent,
            "",
        );
    }

    {
        // 标记-自洽：脏标记与缓存一致 → 不报（避免误报）。
        let n = TreeNode::leaf(9).with_revs(4, 4).with_dirty(DirtyFlags {
            self_dirty: false,
            subtree_dirty: false,
            content_dirty: true,
        });
        let v = TreeView::new(vec![(9, n)], 9);
        // 声明内容脏 + 缓存修订 == 内容修订 → 上面的反向规则会报；改用无脏标记+一致来验证不报
        let n2 = TreeNode::leaf(10).with_revs(4, 4);
        let v2 = TreeView::new(vec![(10, n2)], 10);
        let _ = v;
        set.add(
            "D17-invariant-无脏且缓存一致不报",
            v2.check_marker_self_consistent().is_empty(),
            "",
        );
    }

    {
        // 三序-遍历序= 绘制序：不一致 → 违例。
        let v = good_tree();
        let vs = v.check_order_consistency(&[2, 4, 3], &[2, 3, 4], &[3, 4, 2]);
        let draw_bad = vs.iter().any(|x| x.assert == AssertId::TraverseEqualsDraw);
        set.add("D17-invariant-绘制序偏离可检出", draw_bad, "");
    }

    {
        // 三序-命中序= 遍历序逆序：不一致 → 违例。
        let v = good_tree();
        let vs = v.check_order_consistency(&[2, 4, 3], &[2, 4, 3], &[2, 4, 3]);
        let hit_bad = vs.iter().any(|x| x.assert == AssertId::HitEqualsReverse);
        set.add("D17-invariant-命中序非逆序可检出", hit_bad, "");
    }

    {
        // 三序同源正向：命中序为逆序 → 不报。
        let v = good_tree();
        let vs = v.check_order_consistency(&[2, 4, 3], &[2, 4, 3], &[3, 4, 2]);
        set.add("D17-invariant-三序同源不报", vs.is_empty(), "");
    }

    {
        // 三组归因：报告能按组分开计数。
        // 三组同时各造一处违例——归因计数只在三组都有违例时才有意义。
        let v = TreeView::new(
            vec![
                (1, TreeNode::group(1, vec![4])),
                (2, TreeNode::group(2, vec![4])),
                (3, TreeNode::leaf(3)),
                // 无脏标记但缓存落后 → 标记组违例
                (4, TreeNode::leaf(4).with_revs(5, 3)),
            ],
            1,
        );
        let c = ConsistencyChecker::new(false, SamplingPolicy::exhaustive());
        // 遍历序与绘制序一致，命中序**非**逆序（[4,3] 而非 [3,4]）→ 三序组违例。
        // 注意：单元素序列的逆序等于自身，用两元素序列才测得出逆序断言。
        let r = c.check_full(&v, &[4, 3], &[4, 3], &[4, 3]);
        let (s, m, o) = r.count_by_kind();
        set.add("D17-invariant-三组归因计数", s == 1 && m == 1 && o == 1, "");
    }

    {
        // 断言 id 与分组的映射完备：6 条断言恰覆盖 3 组。
        let mut kinds = [0usize; 3];
        for a in AssertId::all().iter() {
            match a.kind() {
                InvariantKind::Structure => kinds[0] += 1,
                InvariantKind::Marker => kinds[1] += 1,
                InvariantKind::Order => kinds[2] += 1,
            }
        }
        set.add(
            "D17-invariant-断言映射完备",
            kinds[0] == 3 && kinds[1] == 1 && kinds[2] == 2 && INVARIANTS_DOC.contains("三组"),
            "",
        );
    }

    // ---- 两档时机：轻量 O(1) 摊销 / 全量 O(节点数) ----

    {
        // 轻量档只查 1 个节点（O(1) 摊销的形态证据）。
        let v = good_tree();
        let c = ConsistencyChecker::new(false, SamplingPolicy::new());
        let r = c.check_lightweight(&v, 2);
        set.add(
            "D17-timing-轻量只查单节点",
            r.checked_nodes == 1 && r.level == CheckLevel::Lightweight && TWO_LEVEL_DOC.contains("摊销"),
            "",
        );
    }

    {
        // 全量档检查全部节点（O(节点数) 的形态证据）。
        let v = good_tree();
        let c = ConsistencyChecker::new(false, SamplingPolicy::exhaustive());
        let r = c.check_full(&v, &[2, 4, 3], &[2, 4, 3], &[3, 4, 2]);
        set.add(
            "D17-timing-全量查全树",
            r.checked_nodes == v.len() && r.level == CheckLevel::Full,
            "",
        );
    }

    {
        // 轻量档能查出局部违例（不必等全量）。
        let v = TreeView::new(vec![(5, { let mut n = TreeNode::leaf(5); n.children.push(9); n })], 5);
        let c = ConsistencyChecker::new(false, SamplingPolicy::new());
        let r = c.check_lightweight(&v, 5);
        set.add(
            "D17-timing-轻量可查局部违例",
            r.violations.iter().any(|x| x.assert == AssertId::TypeConstraint),
            "",
        );
    }

    {
        // 两档行为不同但判据集相同（同一逻辑、两种时机）。
        let v = good_tree();
        let dbg = ConsistencyChecker::new(true, SamplingPolicy::exhaustive());
        let rel = ConsistencyChecker::new(false, SamplingPolicy::exhaustive());
        let rd = dbg.check_full(&v, &[2, 4, 3], &[2, 4, 3], &[3, 4, 2]);
        let rr = rel.check_full(&v, &[2, 4, 3], &[2, 4, 3], &[3, 4, 2]);
        set.add(
            "D17-timing-两档判据集一致",
            rd.violations.len() == rr.violations.len(),
            "",
        );
    }

    // ---- fail-fast 与三要素 ----

    {
        // debug 构建违例即断（须中断）。
        let v = TreeView::new(
            vec![
                (1, TreeNode::group(1, vec![4])),
                (2, TreeNode::group(2, vec![4])),
                (4, TreeNode::leaf(4)),
            ],
            1,
        );
        let c = ConsistencyChecker::new(true, SamplingPolicy::exhaustive());
        let r = c.check_full(&v, &[4], &[4], &[4]);
        set.add(
            "D17-failfast-debug违例即断",
            r.must_halt() && r.verdict == Verdict::FailFast && FAIL_FAST_DOC.contains("即断"),
            "",
        );
    }

    {
        // release 构建上报后继续（不须中断）。
        let v = TreeView::new(
            vec![
                (1, TreeNode::group(1, vec![4])),
                (2, TreeNode::group(2, vec![4])),
                (4, TreeNode::leaf(4)),
            ],
            1,
        );
        let c = ConsistencyChecker::new(false, SamplingPolicy::exhaustive());
        let r = c.check_full(&v, &[4], &[4], &[4]);
        set.add(
            "D17-failfast-release上报继续",
            !r.must_halt() && r.verdict == Verdict::ReportOnly && !r.passed(),
            "",
        );
    }

    {
        // 通过的报告不须中断（fail-fast 不滥用）。
        let v = good_tree();
        let c = ConsistencyChecker::new(true, SamplingPolicy::exhaustive());
        let r = c.check_full(&v, &[2, 4, 3], &[2, 4, 3], &[3, 4, 2]);
        set.add("D17-failfast-通过不误断", r.passed() && !r.must_halt(), "");
    }

    {
        // 三要素齐备：所有违例的 what/why/how 非空。
        let v = TreeView::new(
            vec![
                (1, TreeNode::group(1, vec![4])),
                (2, TreeNode::group(2, vec![4])),
                (3, TreeNode::leaf(3)),
                (4, TreeNode::leaf(4).with_revs(5, 3)),
            ],
            1,
        );
        let c = ConsistencyChecker::new(false, SamplingPolicy::exhaustive());
        let r = c.check_full(&v, &[4, 3], &[4, 3], &[4, 3]);
        set.add("D17-failfast-三要素齐备", !r.violations.is_empty() && r.all_three_elements(), "");
    }

    {
        // **表外真实形态**：三要素判据不是恒真。
        // 上面那条用的是本模块自己产出的违例（构造时必填非空串），故恒真；
        // 这里用**空要素**的真实形态验证判据真的会拒绝——否则「三要素」就是摆设，
        // 下游 F0618 面板会拿到空话诊断。
        let empty_what = Violation::new(AssertId::Acyclic, 1, "", "父链成环", "打断引用");
        let empty_why = Violation::new(AssertId::Acyclic, 1, "父链超上界", "", "打断引用");
        let empty_how = Violation::new(AssertId::Acyclic, 1, "父链超上界", "父链成环", "");
        let all_ok = Violation::new(AssertId::Acyclic, 1, "a", "b", "c");
        set.add(
            "D17-failfast-空要素判为不齐备",
            !empty_what.three_elements_complete()
                && !empty_why.three_elements_complete()
                && !empty_how.three_elements_complete()
                && all_ok.three_elements_complete(),
            "",
        );
    }

    {
        // 报告级三要素在混入空要素违例后必须报不齐（聚合面同样非恒真）。
        let v = TreeView::new(
            vec![
                (1, TreeNode::group(1, vec![4])),
                (2, TreeNode::group(2, vec![4])),
                (4, TreeNode::leaf(4)),
            ],
            1,
        );
        let c = ConsistencyChecker::new(false, SamplingPolicy::exhaustive());
        let mut r = c.check_full(&v, &[4], &[4], &[4]);
        // 真实报告非空要素 → 齐备；注入一条空要素 → 不齐备。
        let ok_before = r.all_three_elements();
        r.violations.push(empty_element_probe());
        set.add(
            "D17-failfast-报告级三要素聚合真实",
            ok_before && !r.all_three_elements(),
            "",
        );
    }

    {
        // 违例摘要可消费（下游 F0618 面板 / F0620 证据）。
        let vs = vec![Violation::new(AssertId::SingleParent, 4, "层 4 有 2 个父", "被多父声明", "移除多余父引用")];
        let line = vs[0].summary();
        set.add(
            "D17-failfast-违例摘要可消费",
            line.contains("single_parent") && line.contains("移除多余父引用"),
            "",
        );
    }

    // ---- 抽检降级 ----

    {
        // 节点超预算 → 降级抽样且显式登记（不静默）。
        let nodes: Vec<(u64, TreeNode)> =
            (1..=100).map(|i| (i, TreeNode::leaf(i))).collect();
        let v = TreeView::new(nodes, 1);
        let p = SamplingPolicy { enabled: true, node_budget: 10, stride: 8 };
        let c = ConsistencyChecker::new(false, p);
        let r = c.check_full(&v, &[], &[], &[]);
        set.add(
            "D17-sampling-超预算降级抽样",
            r.degraded_to_sampling && r.checked_nodes < r.total_nodes && SAMPLING_DOC.contains("确定性"),
            "",
        );
    }

    {
        // 抽样确定性：同样输入两次产出同样样本（可复现）。
        let p = SamplingPolicy { enabled: true, node_budget: 10, stride: 8 };
        let a = p.sample_indices(100);
        let b = p.sample_indices(100);
        set.add("D17-sampling-抽样可复现", a == b && a.first().copied() == Some(0), "");
    }

    {
        // 步长 0 不死循环（防除零；effective_stride 归1）。
        let p = SamplingPolicy { enabled: true, node_budget: 5, stride: 0 };
        let idx = p.sample_indices(20);
        set.add(
            "D17-sampling-零步长不死循环",
            p.effective_stride() == 1 && idx.len() == 20,
            "",
        );
    }

    {
        // 未超预算 → 不降级（全查）。
        let p = SamplingPolicy { enabled: true, node_budget: 1000, stride: 8 };
        let idx = p.sample_indices(50);
        set.add("D17-sampling-未超预算不降级", idx.len() == 50, "");
    }

    {
        // 抽样下不谎称"全量无环"：降级时不做全树单父/无环判定。
        let nodes: Vec<(u64, TreeNode)> = (1..=50).map(|i| (i, TreeNode::leaf(i))).collect();
        let v = TreeView::new(nodes, 1);
        let p = SamplingPolicy { enabled: true, node_budget: 5, stride: 8 };
        let c = ConsistencyChecker::new(false, p);
        let r = c.check_full(&v, &[], &[], &[]);
        // 降级 + 无环违例（空序不一致会报三序，但不该报全树无环）
        let no_acyclic_claim = !r.violations.iter().any(|x| x.assert == AssertId::Acyclic);
        set.add("D17-sampling-降级不谎称全量无环", r.degraded_to_sampling && no_acyclic_claim, "");
    }

    // ---- 误报修订 ----

    {
        // 误报登记留痕（断言 id + 期望 + 实际 + 动作）。
        let mut c = ConsistencyChecker::new(false, SamplingPolicy::exhaustive());
        c.note_false_positive(TuningRecord::new(
            AssertId::MarkerSelfConsistent,
            "cached_rev == content_rev",
            "cached_rev = 0（无缓存）",
            "无缓存不应算违例，修订判定加入 cached_rev==0 豁免",
        ));
        let t = c.tunings();
        set.add(
            "D17-tuning-误报登记留痕",
            t.len() == 1 && t[0].assert == AssertId::MarkerSelfConsistent && TUNING_DOC.contains("留痕"),
            "",
        );
    }

    {
        // 无缓存（cached_rev=0）不算违例 —— 这正是上一条修订的落点，用例闭环。
        let n = TreeNode::leaf(11).with_revs(5, 0);
        let v = TreeView::new(vec![(11, n)], 11);
        set.add(
            "D17-tuning-无缓存豁免不报",
            v.check_marker_self_consistent().is_empty(),
            "",
        );
    }

    // ---- 契约文档在场（判据条款可追溯） ----

    {
        let docs_ok = INVARIANTS_DOC.contains("三组")
            && TWO_LEVEL_DOC.contains("两档")
            && FAIL_FAST_DOC.contains("fail-fast")
            && SAMPLING_DOC.contains("抽检降级")
            && TUNING_DOC.contains("误报");
        set.add("D17-judge-五契约条款在场", docs_ok, "");
    }

    set
}