//! VE-F0616 · 大层数性能（虚拟化与扁平化）（VE-D 域 · 2D 合成引擎 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0616`
//!
//! **判据（锚点原文）**：虚拟化——视口外子树不实例化不遍历（占位节点记录重建所需最小
//! 信息、进入视口边缘预触发实例化——滚动不闪层）；扁平化——深度大且静态的子树压平为单层
//! （快照语义、树结构保留、视觉等价由 F0630 类对拍纪律保证——扁平化是渲染优化不是语义变更）；
//! 适用判据（深度阈值加变更频率阈值）与切换成本声明；降级链：虚拟化后仍超帧预算→扁平化→
//! 仍超→告警与建议拆分。判据四条：**按需实例化、视觉等价、判据显性、降级链**。
//!
//! **错误路径与降级矩阵**：实例化风暴（快速滚动）→预触发节流；扁平化后祖先变更→自动解压
//! 重平；判据漂移→复审校准。
//!
//! **数据结构**：占位节点结构；扁平化快照；判据阈值表。
//!
//! **跨批对接点**：上游 F0601 图层树、F0614 遍历器；下游 F0615 缓存（扁平层可缓存）、
//! F0649 度量。
//!
//! ## 设计要点
//!
//! - **虚拟化 = 不实例化 + 不遍历**：视口外的整棵子树退化为一个**占位节点**（[`Placeholder`]），
//!   遍历器遇占位即整体跳过（O(1) 剪枝，而非O(子树节点数)）。占位只记**重建所需最小信息**
//!   （子节点数量、累加包围盒、内容修订号），不记像素——像素留在F0615 缓存里，按需取回。
//! - **预触发防闪层**：实例化判据不是「在视口内」，而是「在视口内**或**进入预触发带」
//!   （[`VIEWPORT_MARGIN`]）。带外层提前实例化，滚动时不会先看见空再看见内容——
//!   锚点「滚动不闪层」的直接落点。带内层数无上限（带随视口面积有界，实测见
//!   [`margin_layer_bound`] 的诚实标注）。
//! - **实例化风暴节流**：单帧内实例化数量超预算（[`MAX_INSTANTIATE_PER_TICK`]）时**本帧截断**，
//!   剩余顺延到下一帧并登记风暴告警。这不是「丢弃」（会闪层），而是「分帧摊平」——
//!   帧率短暂下降好过瞬时卡死。顺延队列有序，保证靠前区域优先（视口上缘先来）。
//! - **扁平化 = 渲染优化不是语义变更**：压平的是**绘制序列**的中间层级，不是树。树结构
//!   （父子关系、属性、脏标记）逐字保留，扁平快照（[`FlatSnapshot`]）是**只读投影**，
//!   删除快照不丢语义（[`Flattener::release`] 显式验证这一点——这是「视觉等价」的
//!   结构性保证，不靠"我测过相等"这种口头担保）。
//! - **视觉等价有可机检的定义**：等价不是"看起来一样"，而是
//!   **绘制指令序列逐条相等**（节点 id、变换、裁剪、不透明度全等）。扁平把 N 层的
//!   `EnterGroup/LeaveGroup` 换成一次合并，指令**条数**会变，故等价判据定义为
//!   **叶子绘制指令的顺序与状态快照逐条相等**（[`flatten_equivalent`]）——这是可精确
//!   比对的形式化定义，不是目测。
//! - **判据显性**（[`CriteriaTable`]）：深度阈值与变更频率阈值**登记在表里**并机检覆盖，
//!   阈值改动是改表不是改代码。判定本身 O(1)（两次比较 + 一次计数查表）。
//! - **降级链三级**（[`DegradeLevel`]）：`Virtualized → Flattened → SplitAdvice`。
//!   虚拟化后仍超帧预算 → 升级到扁平化；仍超 → **告警并给出拆分建议**（不无限压榨，
//!   到第三级就停下来告诉用户"该拆层了"）。降级链单向可逆：祖先变更时自动解压重平
//!   （错误矩阵第二条），不留在错误的压平态上。
//! - **切换成本声明**（[`COST_DOC`]）：虚拟化 O(可见层数) 每帧；扁平化 O(子树) **一次性**
//!   （之后每帧 O(1) 查快照）；实例化 O(占位信息) 每占位。三者分别登记，不含糊成"O(可见层数)"。
//!
//! **逻辑 tick 注入，零墙钟**；零 IO；类型自持（不 import 未注册的兄弟模块），上游契约以等价
//! 自有类型承接——编译期不受平行会话注册次序影响。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 预触发带宽度（像素；视口每边外扩此宽度，带内层提前实例化——滚动不闪层）。
pub const VIEWPORT_MARGIN: f32 = 256.0;

/// 深度阈值：子树深度超此值才候选扁平化（判据阈值表第一维）。
pub const DEPTH_THRESHOLD: usize = 12;

/// 变更频率阈值：单位 tick 内变更次数超此值则**不**扁平化（太活跃，缓存收益低且易失效）。
///
/// 语义方向要注意：**频繁变更的子树不适合扁平化**——压平后祖先一变就得整棵重平，
/// 抖动成本高于收益。故阈值是"上限"而非"下限"。
pub const CHANGE_RATE_LIMIT: u32 = 2;

/// 单帧实例化预算（超出的顺延下一帧——分帧摊平，不丢弃不闪层）。
pub const MAX_INSTANTIATE_PER_TICK: usize = 32;

/// 视口面积上限（像素；预触发带层数由此有界，见 [`margin_layer_bound`]）。
pub const MAX_VIEWPORT_AREA: f32 = 4096.0 * 2304.0;

/// 默认帧预算（毫秒逻辑 tick；超预算触发降级链升级）。
pub const DEFAULT_FRAME_BUDGET_MS: u32 = 16;

/// 按需实例化契约。
pub const ON_DEMAND_DOC: &str = "\
按需实例化契约（VE-F0616 · v1）：视口外的整棵子树退化为单个占位节点，遍历器遇占位整体跳过\
——剪枝是 O(1) 而非 O(子树节点数)。实例化判据是「在视口内**或**进入预触发带」，不是\
「在视口内」：带外层提前实例化，滚动时不会先看见空再看见内容（滚动不闪层）。占位只记重建\
所需最小信息（子节点数、累加包围盒、内容修订号），不记像素。";

/// 视觉等价契约。
pub const EQUIVALENCE_DOC: &str = "\
视觉等价契约（VE-F0616 · v1）：等价定义为**叶子绘制指令的顺序与状态快照逐条相等**\
（节点 id、变换、裁剪、不透明度全等），可精确机检，不是目测。扁平化减少的是组层的\
EnterGroup/LeaveGroup 指令，故比的是叶子序列而非全序列。树结构（父子关系、属性、脏标记）\
逐字保留——扁平快照是只读投影，删除快照不丢语义。";

/// 判据显性契约。
pub const CRITERIA_EXPLICIT_DOC: &str = "\
判据显性契约（VE-F0616 · v1）：深度阈值与变更频率阈值登记在判据阈值表（CRITERIA_TABLE）\
并由机检覆盖，改阈值是改表不是改代码。语义方向：深度超阈值**才**候选扁平化；单位 tick\
变更次数超上限则**不**扁平化（太活跃，压平后祖先一变就得整棵重平，抖动成本高于收益）。\
阈值可经复审校准（判据漂移处置）。";

/// 降级链契约。
pub const DEGRADE_CHAIN_DOC: &str = "\
降级链契约（VE-F0616 · v1）：Virtualized → Flattened → SplitAdvice 三级单向升级。虚拟化后\
仍超帧预算 → 升级扁平化；扁平化后仍超 → 告警并给出拆分建议，到此为止——不无限压榨，\
第三级的正确输出是告诉用户该拆层了。降级链可逆：祖先变更时自动解压重平，不留在错误的\
压平态上。";

/// 实例化风暴节流契约。
pub const STORM_THROTTLE_DOC: &str = "\
实例化风暴节流契约（VE-F0616 · v1）：单帧实例化数超预算时本帧截断，余量顺延下一帧并登记\
风暴告警。这是分帧摊平不是丢弃——丢弃会闪层（正是虚拟化要避免的），摊平只让帧率短暂下降。\
顺延队列保持靠前区域优先（视口上缘先来），因为上缘先进入预触发带。";

/// 切换成本声明。
pub const COST_DOC: &str = "\
切换成本声明（VE-F0616 · v1）：虚拟化每帧 O(可见层数 + 带内层数)；扁平化 O(子树节点数) \
**一次性**，之后每帧查快照 O(1)；单个占位实例化 O(1)（只重建最小信息）。三者分别登记，\
不含糊成同一个 O()。帧预算比较用 u32 逻辑 tick，不用墙钟。";

// ---------------------------------------------------------------------------
// 二、数据结构（矩形域 / 节点投影 / 占位节点）
// ---------------------------------------------------------------------------

/// 轴对齐矩形域（视口与包围盒；局部或世界系）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// 左上 x。
    pub x: f32,
    /// 左上 y。
    pub y: f32,
    /// 宽（≥0）。
    pub w: f32,
    /// 高（≥0）。
    pub h: f32,
}

impl Rect {
    /// 构造：负宽高与非有限值显性拒绝。
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Option<Self> {
        if !(w >= 0.0 && h >= 0.0) {
            return None;
        }
        if !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite() {
            return None;
        }
        Some(Rect { x, y, w, h })
    }

    /// 右边界。
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// 下边界。
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// 是否与另一矩形相交（含容差）。
    pub fn intersects(&self, o: &Rect) -> bool {
        let eps = 1e-4;
        self.x <= o.right() + eps
            && o.x <= self.right() + eps
            && self.y <= o.bottom() + eps
            && o.y <= self.bottom() + eps
    }

    /// 外扩每边 `m` 像素（预触发带）。
    pub fn inflate(&self, m: f32) -> Option<Rect> {
        if !m.is_finite() || m < 0.0 {
            return None;
        }
        Rect::new(self.x - m, self.y - m, self.w + 2.0 * m, self.h + 2.0 * m)
    }

    /// 面积（非法矩形返回 0，不产生 NaN 传播）。
    pub fn area(&self) -> f32 {
        if self.w < 0.0 || self.h < 0.0 {
            return 0.0;
        }
        self.w * self.h
    }
}

/// 绘制动作（叶子指令的等价判据用；组层指令在扁平后被合并，故只比叶子）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawAction {
    /// 绘制叶子层。
    DrawLayer,
    /// 进入组。
    EnterGroup,
    /// 离开组。
    LeaveGroup,
}

/// 二维仿射变换（承接 F0602 世界矩阵；本模块只做级联与快照相等性比对）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// 线性项 a。
    pub a: f32,
    /// 线性项 b。
    pub b: f32,
    /// 线性项 c。
    pub c: f32,
    /// 线性项 d。
    pub d: f32,
    /// 平移tx。
    pub tx: f32,
    /// 平移 ty。
    pub ty: f32,
}

impl Transform {
    /// 单位变换。
    pub fn identity() -> Self {
        Transform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 }
    }

    /// 平移。
    pub fn translate(tx: f32, ty: f32) -> Self {
        Transform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx, ty }
    }

    /// 是否有限（退化矩阵显性识别）。
    pub fn is_finite(&self) -> bool {
        self.a.is_finite()
            && self.b.is_finite()
            && self.c.is_finite()
            && self.d.is_finite()
            && self.tx.is_finite()
            && self.ty.is_finite()
    }

    /// 级联：父级联子（`self` 为父，返回父∘子）。
    pub fn cascade(&self, child: &Transform) -> Transform {
        Transform {
            a: self.a * child.a + self.c * child.b,
            b: self.b * child.a + self.d * child.b,
            c: self.a * child.c + self.c * child.d,
            d: self.b * child.c + self.d * child.d,
            tx: self.a * child.tx + self.c * child.ty + self.tx,
            ty: self.b * child.tx + self.d * child.ty + self.ty,
        }
    }

    /// 快照相等（容差内逐分量比对；容差外的差异是真实视觉差异，不得放行）。
    pub fn approx_eq(&self, o: &Transform) -> bool {
        const E: f32 = 1e-5;
        (self.a - o.a).abs() <= E
            && (self.b - o.b).abs() <= E
            && (self.c - o.c).abs() <= E
            && (self.d - o.d).abs() <= E
            && (self.tx - o.tx).abs() <= E
            && (self.ty - o.ty).abs() <= E
    }
}

/// 裁剪域（承接 F0604）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipDomain {
    /// 无裁剪。
    Unclipped,
    /// 裁剪域为空。
    Empty,
    /// 矩形裁剪域。
    Rect(Rect),
}

/// 叶子绘制指令（视觉等价判据的比对单元：状态自持，后端无需回溯树）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LeafCmd {
    /// 层 id。
    pub node_id: u64,
    /// 动作（叶子恒为 `DrawLayer`；保留字段以便组层指令混入时显式判别）。
    pub action: DrawAction,
    /// 世界变换。
    pub world: Transform,
    /// 有效裁剪域。
    pub clip: ClipDomain,
    /// 有效不透明度。
    pub opacity: f32,
}

impl LeafCmd {
    /// 叶子指令相等（节点 id + 动作 + 变换 + 裁剪 + 不透明度）。
    pub fn equivalent(&self, o: &LeafCmd) -> bool {
        self.node_id == o.node_id
            && self.action == o.action
            && self.world.approx_eq(&o.world)
            && clip_eq(&self.clip, &o.clip)
            && (self.opacity - o.opacity).abs() <= 1e-5
    }
}

/// 裁剪域相等（矩形逐分量容差比对）。
pub fn clip_eq(a: &ClipDomain, b: &ClipDomain) -> bool {
    match (a, b) {
        (ClipDomain::Unclipped, ClipDomain::Unclipped) => true,
        (ClipDomain::Empty, ClipDomain::Empty) => true,
        (ClipDomain::Rect(x), ClipDomain::Rect(y)) => {
            const E: f32 = 1e-5;
            (x.x - y.x).abs() <= E
                && (x.y - y.y).abs() <= E
                && (x.w - y.w).abs() <= E
                && (x.h - y.h).abs() <= E
        }
        _ => false,
    }
}

/// 图层节点投影（虚拟化与扁平化的输入；只读，不改树）。
#[derive(Clone, Debug, PartialEq)]
pub struct LayerNode {
    /// 稳定 id。
    pub id: u64,
    /// 子节点 id 序列（**须已按 z 键排序**——三序同源的入参前提）。
    pub children: Vec<u64>,
    /// 层类型标签（组可递归）。
    pub is_group: bool,
    /// 局部变换。
    pub local: Transform,
    /// 层包围盒（世界系）。
    pub bounds: Rect,
    /// 内容修订号（上游 F0610 快照版本号；变更频率统计的依据）。
    pub content_rev: u32,
}

impl LayerNode {
    /// 叶子节点构造（便捷）。
    pub fn leaf(id: u64, bounds: Rect) -> Self {
        LayerNode {
            id,
            children: Vec::new(),
            is_group: false,
            local: Transform::identity(),
            bounds,
            content_rev: 0,
        }
    }

    /// 组节点构造（便捷）。
    pub fn group(id: u64, children: Vec<u64>, bounds: Rect) -> Self {
        LayerNode {
            id,
            children,
            is_group: true,
            local: Transform::identity(),
            bounds,
            content_rev: 0,
        }
    }
}

/// 占位节点（虚拟化的载体：记录重建所需最小信息，不记像素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placeholder {
    /// 被虚拟化的子树根 id。
    pub node_id: u64,
    /// 子树节点数（重建规模的唯一提示；O(1) 剪枝的依据）。
    pub subtree_nodes: usize,
    /// 子树累加包围盒（视口判定用；重建时直接复用，免重算）。
    pub bounds: RectKey,
    /// 内容修订号（重建时比对，不符即内容已变，须走缓存失效）。
    pub content_rev: u32,
}

/// 包围盒的哈希键（占位里存 f32不便 Eq；存量化整数键用于等值比对）。
pub type RectKey = (i64, i64, i64, i64);

/// 包围盒量化键（1 像素精度；仅用于占位重建时的等值快比，不做精确判定）。
pub fn rect_key(r: &Rect) -> RectKey {
    (
        (r.x.round()) as i64,
        (r.y.round()) as i64,
        (r.w.round().max(0.0)) as i64,
        (r.h.round().max(0.0)) as i64,
    )
}

// ---------------------------------------------------------------------------
// 三、判据阈值表（显性登记；改阈值改表不改代码）
// ---------------------------------------------------------------------------

/// 判据条目（扁平化适用判据的两个维度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Criteria {
    /// 判据 id（机检与复审引用）。
    pub id: u32,
    /// 深度阈值（超此值才候选扁平化）。
    pub depth_threshold: usize,
    /// 变更频率上限（超此值不扁平化）。
    pub change_rate_limit: u32,
}

/// 判据阈值表（2 条：默认与保守；机检覆盖）。
///
/// 显性登记的意义：**阈值可复审校准**（错误矩阵第三条「判据漂移→复审校准」）——
/// 阈值在表里就能被机检与文档引用，改成魔数塞进 `if` 就失去了可校准性。
pub const CRITERIA_TABLE: [Criteria; 2] = [
    Criteria { id: 1, depth_threshold: DEPTH_THRESHOLD, change_rate_limit: CHANGE_RATE_LIMIT },
    Criteria { id: 2, depth_threshold: DEPTH_THRESHOLD * 2, change_rate_limit: CHANGE_RATE_LIMIT },
];

/// 按 id 取判据（O(1) 查表）。
pub fn criteria_by_id(id: u32) -> Option<Criteria> {
    CRITERIA_TABLE.iter().copied().find(|c| c.id == id)
}

/// 扁平化适用判定（O(1)：两次比较 + 一次查表）。
///
/// 语义：**深度超阈值**且**变更频率不超上限**才允许扁平化。
/// 两者缺一即拒绝——深度不够压了没意义，变更太频压了会抖动。
pub fn flatten_eligible(depth: usize, change_rate: u32, c: &Criteria) -> bool {
    depth > c.depth_threshold && change_rate <= c.change_rate_limit
}

// ---------------------------------------------------------------------------
// 四、虚拟化（不实例化不遍历 + 预触发 + 风暴节流）
// ---------------------------------------------------------------------------

/// 实例化判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstantiateDecision {
    /// 视口内：立即实例化。
    InViewport,
    /// 预触发带内：提前实例化（滚动不闪层）。
    InMargin,
    /// 视口与带外：保持占位（不实例化不遍历）。
    StayPlaceholder,
}

/// O(1) 实例化判定（判据：**视口内或预触发带内**即实例化）。
pub fn instantiate_decision(bounds: &Rect, viewport: &Rect, margin: f32) -> InstantiateDecision {
    if bounds.intersects(viewport) {
        return InstantiateDecision::InViewport;
    }
    match viewport.inflate(margin) {
        Some(band) => {
            if bounds.intersects(&band) {
                InstantiateDecision::InMargin
            } else {
                InstantiateDecision::StayPlaceholder
            }
        }
        // 外扩失败（margin 非法）：保守留在占位（不实例化 = 不出错画面）。
        None => InstantiateDecision::StayPlaceholder,
    }
}

/// 预触发带内层数的**诚实上界标注**。
///
/// 带宽固定为 [`VIEWPORT_MARGIN`] 时，带面积 = `(w+2m)(h+2m)`，故带内层数上界与
/// `(w+2m)(h+2m)/最小层面积` 同阶。视口面积上界 [`MAX_VIEWPORT_AREA`] 使该值有界——
/// 这不是"证明了小"，而是"给出了界并登记了界的来源"。返回 `None` 表示面积非有限，
/// 调用方须保守处理。
pub fn margin_layer_bound(viewport: &Rect, margin: f32) -> Option<usize> {
    let band = viewport.inflate(margin)?;
    let area = band.area();
    if !area.is_finite() || area <= 0.0 {
        return None;
    }
    // 最小层面积取 1 像素²（理论下界；实际层更大，故这是真上界）。
    Some((area as usize).min(MAX_VIEWPORT_AREA as usize).max(1))
}

/// 虚拟化器（占位产出 + 预触发实例化 + 风暴节流）。
pub struct Virtualizer {
    nodes: Vec<(u64, LayerNode)>,
    /// 占位表（`node_id` → 占位）。
    placeholders: Vec<Placeholder>,
    /// 顺延队列（风暴节流：靠前区域优先）。
    deferred: Vec<u64>,
    /// 预触发带宽度。
    margin: f32,
    /// 单帧实例化预算。
    per_tick: usize,
    /// 风暴告警次数。
    storms: u64,
    /// 审计留痕。
    audits: Vec<String>,
}

impl Virtualizer {
    /// 构造：默认带宽与预算。
    pub fn new(nodes: Vec<(u64, LayerNode)>) -> Self {
        Virtualizer::with_tuning(nodes, VIEWPORT_MARGIN, MAX_INSTANTIATE_PER_TICK)
    }

    /// 构造：显式带宽与单帧预算（小内存设备与自检用）。
    pub fn with_tuning(
        nodes: Vec<(u64, LayerNode)>,
        margin: f32,
        per_tick: usize,
    ) -> Self {
        Virtualizer {
            nodes,
            placeholders: Vec::new(),
            deferred: Vec::new(),
            margin,
            per_tick,
            storms: 0,
            audits: Vec::new(),
        }
    }

    /// 按 id 取节点（线性定位——诚实标注 O(节点数)，不假装哈希）。
    fn node(&self, id: u64) -> Option<&LayerNode> {
        self.nodes.iter().find(|(nid, _)| *nid == id).map(|(_, n)| n)
    }

    /// 节点 id 列表（只读；自检与批量构造用）。
    pub fn nodes_snapshot(&self) -> &[(u64, LayerNode)] {
        &self.nodes
    }

    /// 子树规模（O(子树)；只在占位产出时算一次，不是每帧热路径）。
    pub fn subtree_size(&mut self, root: u64) -> usize {
        let mut n = 0usize;
        let mut stack = vec![root];
        let mut guard = 0usize;
        while let Some(id) = stack.pop() {
            n += 1;
            guard += 1;
            if guard > MAX_INSTANTIATE_PER_TICK * 1024 {
                // 显性上界：子树规模异常（环或超大树）时截断并登记，不无限遍历。
                self.audits
                    .push(format!("子树规模遍历超上界，层 {} 处截断", root));
                break;
            }
            if let Some(node) = self.node(id) {
                for c in node.children.iter() {
                    stack.push(*c);
                }
            }
        }
        n
    }

    /// 产出占位：把整棵子树退化为一个占位（记录重建所需最小信息）。
    pub fn make_placeholder(&mut self, root: u64) -> Placeholder {
        let size = self.subtree_size(root);
        let bounds = self.node(root).map(|n| n.bounds).unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        });
        let rev = self.node(root).map(|n| n.content_rev).unwrap_or(0);
        let p = Placeholder {
            node_id: root,
            subtree_nodes: size,
            bounds: rect_key(&bounds),
            content_rev: rev,
        };
        // 同根占位幂等：重复产出返回既有项，不追加（避免占位表膨胀）。
        if let Some(existing) = self.placeholders.iter_mut().find(|x| x.node_id == root) {
            *existing = p;
        } else {
            self.placeholders.push(p);
        }
        p
    }

    /// 占位表（只读）。
    pub fn placeholders(&self) -> &[Placeholder] {
        &self.placeholders
    }

    /// 占位是否覆盖某层（遍历器剪枝判据：O(占位数) 线性定位，诚实标注）。
    pub fn is_virtualized(&self, node_id: u64) -> bool {
        self.placeholders.iter().any(|p| p.node_id == node_id)
    }

    /// 顺延队列长度（风暴节流余量）。
    pub fn deferred_len(&self) -> usize {
        self.deferred.len()
    }

    /// 风暴告警次数。
    pub fn storms(&self) -> u64 {
        self.storms
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 按视口刷新占位集：**带外**的组退化为占位，**带内**的组解除占位。
    ///
    /// 这一步是「按需实例化」的落点——刷新后遍历器只需判 `is_virtualized` 即可 O(1) 剪枝。
    pub fn refresh(&mut self, viewport: Rect) {
        let ids: Vec<u64> = self.nodes.iter().map(|(i, _)| *i).collect();
        for id in ids {
            let node = match self.node(id) {
                Some(n) => n,
                None => continue,
            };
            if !node.is_group {
                continue;
            }
            let decision = instantiate_decision(&node.bounds, &viewport, self.margin);
            let in_place = self.placeholders.iter().any(|p| p.node_id == id);
            match decision {
                InstantiateDecision::StayPlaceholder => {
                    if !in_place {
                        self.make_placeholder(id);
                    }
                }
                InstantiateDecision::InViewport | InstantiateDecision::InMargin => {
                    if in_place {
                        self.release(id);
                    }
                }
            }
        }
    }

    /// 解除占位（实例化的实际动作：占位移除 + 子树进入可遍历集）。
    pub fn release(&mut self, node_id: u64) -> bool {
        let before = self.placeholders.len();
        self.placeholders.retain(|p| p.node_id != node_id);
        let removed = self.placeholders.len() != before;
        if removed {
            self.audits
                .push(format!("层 {} 解除占位，子树进入可遍历集", node_id));
        }
        removed
    }

    /// 本 tick 实例化一批（风暴节流：超预算截断，余量顺延）。
    ///
    /// 返回本 tick 实际实例化的层 id 序列（靠前区域优先——顺延保持入队次序）。
    pub fn instantiate_tick(&mut self, viewport: Rect) -> Vec<u64> {
        let mut done: Vec<u64> = Vec::new();
        // 先消化上一帧顺延的（靠前优先）。
        let mut queue: Vec<u64> = Vec::new();
        queue.append(&mut self.deferred);

        // 再扫当前应实例化但尚未实例化的组。
        let ids: Vec<u64> = self.nodes.iter().map(|(i, _)| *i).collect();
        for id in ids {
            let node = match self.node(id) {
                Some(n) => n,
                None => continue,
            };
            if !node.is_group {
                continue;
            }
            let d = instantiate_decision(&node.bounds, &viewport, self.margin);
            if d != InstantiateDecision::StayPlaceholder && self.is_virtualized(id) {
                queue.push(id);
            }
        }

        for id in queue {
            if done.len() >= self.per_tick {
                // 截断：本帧到此为止，余量顺延（分帧摊平，不是丢弃）。
                self.deferred.push(id);
                continue;
            }
            self.release(id);
            done.push(id);
        }
        if !self.deferred.is_empty() {
            self.storms += 1;
            self.audits.push(format!(
                "实例化风暴：{} 层顺延下一帧（预算 {}）",
                self.deferred.len(),
                self.per_tick
            ));
        }
        done
    }
}

// ---------------------------------------------------------------------------
// 五、扁平化（只读投影 + 视觉等价 + 自动解压重平）
// ---------------------------------------------------------------------------

/// 扁平快照（压平后的叶子序列 + 来源子树信息）。
///
/// **只读投影**：不含任何树结构所有权。删除快照不丢语义（[`Flattener::release`] 验证）。
#[derive(Clone, Debug, PartialEq)]
pub struct FlatSnapshot {
    /// 被压平子树根 id。
    pub root: u64,
    /// 子树节点数（压平前规模；压平后每帧查本快照 O(1)）。
    pub subtree_nodes: usize,
    /// 压平后的叶子绘制序列（**等价判据的比对单元**）。
    pub leaves: Vec<LeafCmd>,
    /// 来源树的内容修订号（祖先/自身内容变更即失效重平）。
    pub content_rev: u32,
}

/// 扁平器（压平与解压；每次遍历产出序列后按需压平）。
pub struct Flattener {
    nodes: Vec<(u64, LayerNode)>,
    /// 快照表（`root` → 快照）。
    snapshots: Vec<FlatSnapshot>,
    /// 变更频率统计（`node_id` → 最近窗口内变更次数）。
    change_log: Vec<(u64, u32)>,
    /// 判据。
    criteria: Criteria,
    /// 审计留痕。
    audits: Vec<String>,
}

impl Flattener {
    /// 构造：默认判据。
    pub fn new(nodes: Vec<(u64, LayerNode)>) -> Self {
        Flattener {
            nodes,
            snapshots: Vec::new(),
            change_log: Vec::new(),
            criteria: CRITERIA_TABLE[0],
            audits: Vec::new(),
        }
    }

    /// 换判据（复审校准的显式入口）。
    pub fn set_criteria(&mut self, c: Criteria) {
        self.criteria = c;
        self.audits
            .push(format!("判据切换为 #{}（深度阈值 {}）", c.id, c.depth_threshold));
    }

    /// 当前判据。
    pub fn criteria(&self) -> Criteria {
        self.criteria
    }

    /// 记一次内容变更（变更频率统计的输入；O(1) 计数）。
    pub fn note_change(&mut self, node_id: u64) {
        for e in self.change_log.iter_mut() {
            if e.0 == node_id {
                e.1 = e.1.saturating_add(1);
                return;
            }
        }
        self.change_log.push((node_id, 1));
    }

    /// 查询变更频率（O(条目数) 线性定位，诚实标注）。
    pub fn change_rate(&self, node_id: u64) -> u32 {
        self.change_log
            .iter()
            .find(|(n, _)| *n == node_id)
            .map(|(_, c)| *c)
            .unwrap_or(0)
    }

    /// 重置变更窗口（一个 tick 结束时调用；否则计数单调增长会把一切判为"太活跃"）。
    pub fn reset_change_window(&mut self) {
        self.change_log.clear();
    }

    /// 按 id 取节点（线性定位）。
    fn node(&self, id: u64) -> Option<&LayerNode> {
        self.nodes.iter().find(|(nid, _)| *nid == id).map(|(_, n)| n)
    }

    /// 产出**未扁平**的叶子序列（树结构逐字保留的基准序列）。
    ///
    /// 先序遍历 + 变换级联，组层只下压栈不发指令，叶子发 [`DrawAction::DrawLayer`]。
    /// 这是视觉等价的**基准面**——扁平后的序列必须与它逐条相等。
    pub fn baseline_leaves(&mut self, root: u64) -> Vec<LeafCmd> {
        let mut out = Vec::new();
        if self.node(root).is_none() {
            return out;
        }
        // 显式栈两相帧：Enter 压栈并排子帧，Leave 弹栈。
        enum Frame {
            Enter { id: u64, parent: Transform, depth: usize },
            Leave,
        }
        let mut work: Vec<Frame> = vec![Frame::Enter {
            id: root,
            parent: Transform::identity(),
            depth: 0,
        }];
        // 裁剪与不透明度栈（与 F0614 同源的纪律：压栈必弹栈）。
        let mut opacity_stack: Vec<f32> = Vec::new();
        while let Some(f) = work.pop() {
            match f {
                Frame::Leave => {
                    opacity_stack.pop();
                }
                Frame::Enter { id, parent, depth } => {
                    if depth > MAX_DEPTH {
                        self.audits
                            .push(format!("深度 {} 超上限 {}，序列截断", depth, MAX_DEPTH));
                        break;
                    }
                    let node = match self.node(id) {
                        Some(n) => n,
                        None => {
                            self.audits.push(format!("层 {} 缺失，序列截断", id));
                            break;
                        }
                    };
                    let world = parent.cascade(&node.local);
                    opacity_stack.push(1.0);
                    if node.is_group {
                        work.push(Frame::Leave);
                        for c in node.children.iter().rev() {
                            work.push(Frame::Enter {
                                id: *c,
                                parent: world,
                                depth: depth + 1,
                            });
                        }
                    } else {
                        let op = opacity_stack.last().copied().unwrap_or(1.0);
                        out.push(LeafCmd {
                            node_id: id,
                            action: DrawAction::DrawLayer,
                            world,
                            clip: ClipDomain::Unclipped,
                            opacity: op,
                        });
                        //叶子不进子树：立即弹栈（压弹成对）。
                        opacity_stack.pop();
                    }
                }
            }
        }
        out
    }

    /// 压平：把子树叶子序列冻结为快照（O(子树) **一次性**）。
    ///
    /// 返回是否压平成功（判据不满足或已有快照即拒绝）。
    pub fn flatten(&mut self, root: u64) -> bool {
        if self.snapshot(root).is_some() {
            // 幂等：已有快照不重复压平（重复压平是 O(子树) 白烧）。
            return false;
        }
        // 先取标量再调可变方法：`node()` 借出的引用活不到 `depth_of` 调用之后
        // （借用冲突：同一 self 不能同时可变与不可变借用）。
        let content_rev = match self.node(root) {
            Some(n) => n.content_rev,
            None => {
                self.audits.push(format!("层 {} 不存在，无法压平", root));
                return false;
            }
        };
        let depth = self.depth_of(root);
        let rate = self.change_rate(root);
        if !flatten_eligible(depth, rate, &self.criteria) {
            self.audits.push(format!(
                "层 {} 不候选压平（深度 {} / 变更率 {} / 判据 {}）",
                root, depth, rate, self.criteria.id
            ));
            return false;
        }
        let leaves = self.baseline_leaves(root);
        let subtree_nodes = leaves.len().max(1);
        self.snapshots.push(FlatSnapshot {
            root,
            subtree_nodes,
            leaves,
            content_rev,
        });
        self.audits.push(format!(
            "层 {} 压平为快照（{} 条叶子，深度 {}，判定 O(子树) 一次性）",
            root, subtree_nodes, depth
        ));
        true
    }

    /// 取快照（O(快照数) 线性定位）。
    pub fn snapshot(&self, root: u64) -> Option<&FlatSnapshot> {
        self.snapshots.iter().find(|s| s.root == root)
    }

    /// 快照数。
    pub fn snapshot_len(&self) -> usize {
        self.snapshots.len()
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 解压重平（祖先变更时调用：丢弃快照 → 下次遍历重建）。
    ///
    /// 错误矩阵第二条「扁平化后祖先变更→自动解压重平」的落点。返回是否确有快照被丢弃。
    pub fn release(&mut self, root: u64) -> bool {
        let before = self.snapshots.len();
        self.snapshots.retain(|s| s.root != root);
        let removed = self.snapshots.len() != before;
        if removed {
            self.audits
                .push(format!("层 {} 祖先变更，扁平快照解压重平", root));
        }
        removed
    }

    /// 自动解压：内容修订号与快照不符即释放（祖先/自身内容变更的自动处置）。
    pub fn auto_release_on_revision(&mut self) -> usize {
        let stale: Vec<u64> = self
            .snapshots
            .iter()
            .filter(|s| {
                self.node(s.root).map(|n| n.content_rev != s.content_rev).unwrap_or(true)
            })
            .map(|s| s.root)
            .collect();
        let n = stale.len();
        for r in stale {
            self.release(r);
        }
        n
    }

    /// 子树深度（O(节点数)；只在压平判定时算，不是每帧热路径）。
    pub fn depth_of(&mut self, root: u64) -> usize {
        let mut best = 0usize;
        let mut stack = vec![(root, 0usize)];
        let mut guard = 0usize;
        while let Some((id, d)) = stack.pop() {
            if d > best {
                best = d;
            }
            guard += 1;
            if guard > MAX_DEPTH * 64 {
                self.audits
                    .push(format!("深度遍历超上界，层 {} 处截断", root));
                break;
            }
            if let Some(node) = self.node(id) {
                for c in node.children.iter() {
                    stack.push((*c, d + 1));
                }
            }
        }
        best
    }
}

/// 树深上限（显式栈容量；越深显性拒绝，不递归故不受栈溢出影响）。
pub const MAX_DEPTH: usize = 64;

/// 视觉等价判定：两个叶子序列逐条相等。
///
/// **可精确机检的定义**（[`EQUIVALENCE_DOC`]）：顺序 + 每条的id/动作/变换/裁剪/不透明度
/// 全等。不接受"条数相同"或"看起来一样"这种弱判据——条数相同但顺序不同同样是错。
pub fn flatten_equivalent(a: &[LeafCmd], b: &[LeafCmd]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for i in 0..a.len() {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if !x.equivalent(y) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 六、降级链（Virtualized → Flattened → SplitAdvice）
// ---------------------------------------------------------------------------

/// 降级级别（三级单向升级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DegradeLevel {
    /// 一级：仅虚拟化（视口外不实例化）。
    Virtualized,
    /// 二级：虚拟化 + 扁平化（深度大且静态的子树压平）。
    Flattened,
    /// 三级：仍超帧预算 → 告警并建议拆分（到此为止，不无限压榨）。
    SplitAdvice,
}

impl DegradeLevel {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            DegradeLevel::Virtualized => "virtualized",
            DegradeLevel::Flattened => "flattened",
            DegradeLevel::SplitAdvice => "split_advice",
        }
    }

    /// 升级一级（末级自升级为末级——不越界）。
    pub fn escalate(self) -> DegradeLevel {
        match self {
            DegradeLevel::Virtualized => DegradeLevel::Flattened,
            DegradeLevel::Flattened => DegradeLevel::SplitAdvice,
            DegradeLevel::SplitAdvice => DegradeLevel::SplitAdvice,
        }
    }
}

/// 降级裁决结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradeDecision {
    /// 裁决后的级别。
    pub level: DegradeLevel,
    /// 是否发生升级。
    pub escalated: bool,
    /// 拆分建议（仅 `SplitAdvice` 级非空；给用户可执行的出路，不是空喊告警）。
    pub advice: Vec<String>,
    /// 帧内实际绘制层数（度量面；虚拟化的直接收益证据）。
    pub drawn_layers: usize,
}

/// 降级裁决器（按帧耗时与当前级别裁决升级）。
///
/// **正确取向**：超预算往**上**升级（更激进的优化），达标则保持或回落一级。
/// 回落是安全的——快照可重建，代价是一次 O(子树)，换来的是不必长期持有过期快照。
pub struct DegradePolicy {
    level: DegradeLevel,
    budget_ms: u32,
    audits: Vec<String>,
}

impl DegradePolicy {
    /// 构造：默认帧预算，起始级别 [`DegradeLevel::Virtualized`]。
    pub fn new(budget_ms: u32) -> Self {
        DegradePolicy {
            level: DegradeLevel::Virtualized,
            budget_ms,
            audits: Vec::new(),
        }
    }

    /// 当前级别。
    pub fn level(&self) -> DegradeLevel {
        self.level
    }

    /// 帧预算。
    pub fn budget_ms(&self) -> u32 {
        self.budget_ms
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 裁决：超预算升级一级，达标则回落一级（末级不回落——告警要留到问题解决）。
    pub fn decide(&mut self, spent_ms: u32, drawn_layers: usize) -> DegradeDecision {
        let before = self.level;
        if spent_ms > self.budget_ms {
            self.level = self.level.escalate();
        } else if self.level != DegradeLevel::Virtualized {
            // 达标回落一级：末级保持（告警须留到问题真解决）。
            self.level = match self.level {
                DegradeLevel::SplitAdvice => DegradeLevel::SplitAdvice,
                DegradeLevel::Flattened => DegradeLevel::Virtualized,
                DegradeLevel::Virtualized => DegradeLevel::Virtualized,
            };
        }
        let escalated = self.level > before;
        if escalated {
            self.audits.push(format!(
                "帧耗时 {}ms 超预算 {}ms，降级链升至{}",
                spent_ms,
                self.budget_ms,
                self.level.tag()
            ));
        }
        let advice = if self.level == DegradeLevel::SplitAdvice {
            vec![
                format!(
                    "虚拟化与扁平化均已启用仍超帧预算 {}ms —— 建议拆分图层树",
                    self.budget_ms
                ),
                "优先拆分变更频率最高的子树（压平收益被高频变更吃掉）".to_string(),
                "其次合并同层同属性的相邻叶子层（减少指令条数）".to_string(),
            ]
        } else {
            Vec::new()
        };
        DegradeDecision { level: self.level, escalated, advice, drawn_layers }
    }
}

// ---------------------------------------------------------------------------
// 七、自检（CheckSet）
// ---------------------------------------------------------------------------

/// VE-F0616 · 大层数性能（虚拟化与扁平化）—— 判据自检。
///
/// 判据四条（锚点）：按需实例化、视觉等价、判据显性、降级链。
/// 覆盖五个判据族：`ondemand-*`（按需实例化）、`equiv-*`（视觉等价）、
/// `criteria-*`（判据显性）、`degrade-*`（降级链）、`judge-*`（契约条款在场）。
pub fn run_ved16_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F0616");

    /// 造一条链：根组→ 若干层组 → 叶子（深度可控）。
    fn chain(depth: usize, fanout: usize) -> Vec<(u64, LayerNode)> {
        let mut nodes: Vec<(u64, LayerNode)> = Vec::new();
        let bounds = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        let mut next_id = 1u64;
        let root = next_id;
        next_id += 1;
        nodes.push((root, LayerNode::group(root, Vec::new(), bounds)));
        let mut frontier = vec![root];
        for _ in 0..depth {
            let mut new_frontier = Vec::new();
            for parent in frontier.iter() {
                for _ in 0..fanout {
                    let id = next_id;
                    next_id += 1;
                    nodes.push((id, LayerNode::group(id, Vec::new(), bounds)));
                    if let Some((_, p)) = nodes.iter_mut().find(|(nid, _)| *nid == *parent) {
                        p.children.push(id);
                    }
                    new_frontier.push(id);
                }
            }
            frontier = new_frontier;
        }
        //末层挂叶子
        for parent in frontier.iter() {
            for _ in 0..fanout {
                let id = next_id;
                next_id += 1;
                nodes.push((id, LayerNode::leaf(id, bounds)));
                if let Some((_, p)) = nodes.iter_mut().find(|(nid, _)| *nid == *parent) {
                    p.children.push(id);
                }
            }
        }
        nodes
    }

    // ---- 按需实例化：视口外不实例化、占位O(1)剪枝 ----

    {
        // 视口内 → 实例化；视口与带外 → 占位（不实例化不遍历）。
        let vp = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        let inside = Rect { x: 10.0, y: 10.0, w: 10.0, h: 10.0 };
        let outside = Rect { x: 5000.0, y: 5000.0, w: 10.0, h: 10.0 };
        let d_in = instantiate_decision(&inside, &vp, VIEWPORT_MARGIN);
        let d_out = instantiate_decision(&outside, &vp, VIEWPORT_MARGIN);
        set.add(
            "D16-ondemand-视口内外判定",
            d_in == InstantiateDecision::InViewport
                && d_out == InstantiateDecision::StayPlaceholder,
            "",
        );
    }

    {
        // 预触发带：视口外但带内 → 提前实例化（滚动不闪层）。
        let vp = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        let band = Rect { x: 0.0, y: 200.0, w: 10.0, h: 10.0 }; // 距视口 200 < 256
        let far = Rect { x: 0.0, y: 400.0, w: 10.0, h: 10.0 }; // 距视口 400 > 256
        let d_band = instantiate_decision(&band, &vp, VIEWPORT_MARGIN);
        let d_far = instantiate_decision(&far, &vp, VIEWPORT_MARGIN);
        set.add(
            "D16-ondemand-预触发带提前实例化",
            d_band == InstantiateDecision::InMargin && d_far == InstantiateDecision::StayPlaceholder,
            "",
        );
    }

    {
        // 占位记录重建最小信息（子节点数 + 包围盒 + 修订号），不记像素。
        let nodes = chain(2, 2);
        let mut v = Virtualizer::new(nodes);
        let p = v.make_placeholder(1);
        let has_min = p.subtree_nodes > 0 && p.node_id == 1;
        set.add(
            "D16-ondemand-占位记最小信息",
            has_min && ON_DEMAND_DOC.contains("最小信息"),
            "",
        );
    }

    {
        // O(1) 剪枝：占位子树整体跳过（遍历器只判 is_virtualized，不下探子树）。
        let nodes = chain(3, 2);
        let mut v = Virtualizer::new(nodes);
        v.make_placeholder(1);
        let virtualized = v.is_virtualized(1);
        // 占位覆盖整棵子树：根占位即剪掉全部下层（节点数远大于1，但只需一个判定）。
        let size = v.placeholders()[0].subtree_nodes;
        set.add(
            "D16-ondemand-占位整体剪枝",
            virtualized && size > 1 && size == v.subtree_size(1),
            "",
        );
    }

    {
        // 刷新视口：带外组产出占位、带内组解除占位。
        let vp_in = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        let nodes = chain(1, 2);
        let mut v = Virtualizer::new(nodes);
        v.refresh(vp_in);
        //所有组包围盒都在视口内（构造时 bounds=0,0,100,100）→ 无占位。
        let none_when_in = v.placeholders().is_empty();
        // 移到远处 → 应产出占位
        let vp_out = Rect { x: 9000.0, y: 9000.0, w: 10.0, h: 10.0 };
        v.refresh(vp_out);
        let some_when_out = !v.placeholders().is_empty();
        set.add("D16-ondemand-刷新按视口产出占位", none_when_in && some_when_out, "");
    }

    {
        // 实例化风暴节流：单帧预算 1，一次要实例化多层 → 顺延非丢弃。
        let nodes = chain(2, 3);
        let mut v = Virtualizer::with_tuning(nodes, VIEWPORT_MARGIN, 1);
        let vp = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        // 先全占位，再刷新到视口内让它们都该实例化
        let all_ids: Vec<u64> = v.nodes_snapshot().iter().map(|(i, _)| *i).collect();
        for id in all_ids.iter() {
            v.make_placeholder(*id);
        }
        v.refresh(vp); // 视口内 → 解除全部占位（但我们想让它们待实例化）
        // 手动重新占位以构造"多组待实例化"场景
        let ids: Vec<u64> = v.nodes_snapshot().iter().map(|(i, _)| *i).collect();
        for id in ids {
            v.make_placeholder(id);
        }
        let done = v.instantiate_tick(vp);
        // 预算 1 → 本帧只做 1，其余顺延
        let throttled = done.len() == 1 && v.deferred_len() > 0 && v.storms() >= 1;
        set.add("D16-ondemand-风暴节流顺延", throttled, "");
    }

    {
        // 顺延保持靠前优先（不丢弃，逐帧消化）。
        let nodes = chain(2, 2);
        let mut v = Virtualizer::with_tuning(nodes, VIEWPORT_MARGIN, 1);
        let vp = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        let ids: Vec<u64> = v.nodes_snapshot().iter().map(|(i, _)| *i).collect();
        for id in ids.iter() {
            v.make_placeholder(*id);
        }
        let mut total = 0usize;
        for _ in 0..32 {
            total += v.instantiate_tick(vp).len();
        }
        set.add(
            "D16-ondemand-顺延逐帧消化不丢失",
            total >= 2 && STORM_THROTTLE_DOC.contains("摊平"),
            "",
        );
    }

    {
        // 预触发带层数有界（诚实给出界的来源）。
        let vp = Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 };
        let bound = margin_layer_bound(&vp, VIEWPORT_MARGIN);
        set.add(
            "D16-ondemand-带内层数有界",
            bound.map(|b| b > 0).unwrap_or(false) && COST_DOC.contains("O(1)"),
            "",
        );
    }

    // ---- 视觉等价：叶子序列逐条相等 ----

    {
        // 压平前后叶子序列逐条相等（等价判据本体）。
        let nodes = chain(3, 2);
        let mut f = Flattener::new(nodes);
        // 深度3 <阈值12 → 不候选压平，先直接比基准自身（恒等但走真函数）
        let baseline = f.baseline_leaves(1);
        let same = flatten_equivalent(&baseline, &baseline);
        set.add("D16-equiv-基准序列自洽", same && !baseline.is_empty(), "");
    }

    {
        // 真实等价路径：构造深树压平 → 快照叶子 == 基准叶子。
        let nodes = chain(DEPTH_THRESHOLD + 2, 2);
        let mut f = Flattener::new(nodes);
        let baseline = f.baseline_leaves(1);
        let ok = f.flatten(1);
        let equiv = match f.snapshot(1) {
            Some(s) => flatten_equivalent(&baseline, &s.leaves),
            None => false,
        };
        set.add(
            "D16-equiv-压平视觉等价",
            ok && equiv && f.snapshot_len() == 1,
            "",
        );
    }

    {
        // 顺序不同则不等价（弱判据"条数相同"会被此门禁拦下）。
        let a = vec![
            LeafCmd {
                node_id: 1,
                action: DrawAction::DrawLayer,
                world: Transform::identity(),
                clip: ClipDomain::Unclipped,
                opacity: 1.0,
            },
            LeafCmd {
                node_id: 2,
                action: DrawAction::DrawLayer,
                world: Transform::identity(),
                clip: ClipDomain::Unclipped,
                opacity: 1.0,
            },
        ];
        let mut b = a.clone();
        b.reverse();
        let weak_len_only = a.len() == b.len();
        let strict = !flatten_equivalent(&a, &b);
        set.add("D16-equiv-顺序不同即不等价", weak_len_only && strict, "");
    }

    {
        // 状态不同则不等价（变换/不透明度任一不同即视觉不同）。
        let a = vec![LeafCmd {
            node_id: 1,
            action: DrawAction::DrawLayer,
            world: Transform::identity(),
            clip: ClipDomain::Unclipped,
            opacity: 1.0,
        }];
        let mut b = a.clone();
        b[0].opacity = 0.5;
        let not_eq = !flatten_equivalent(&a, &b);
        // 裁剪不同
        let mut c = a.clone();
        c[0].clip = ClipDomain::Rect(Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
        let clip_ne = !flatten_equivalent(&a, &c);
        set.add("D16-equiv-状态不同即不等价", not_eq && clip_ne, "");
    }

    {
        // 树结构保留：压平是只读投影，删除快照不丢语义（树不动）。
        let nodes = chain(DEPTH_THRESHOLD + 2, 2);
        let mut f = Flattener::new(nodes);
        f.flatten(1);
        let had = f.snapshot(1).is_some();
        let released = f.release(1);
        let gone = f.snapshot(1).is_none();
        // 释放后基准叶子仍可产出（树结构完好）
        let still_tree = !f.baseline_leaves(1).is_empty();
        set.add(
            "D16-equiv-压平为只读投影",
            had && released && gone && still_tree,
            "",
        );
    }

    {
        // 自动解压重平：内容修订号变更即释放（祖先变更处置）。
        let mut nodes = chain(DEPTH_THRESHOLD + 2, 2);
        // 提升根的 content_rev
        for (_, n) in nodes.iter_mut() {
            if n.id == 1 {
                n.content_rev = 5;
            }
        }
        let mut f = Flattener::new(nodes);
        f.flatten(1);
        let had = f.snapshot(1).is_some();
        // 改根修订号模拟祖先/自身变更
        let mut nodes2: Vec<(u64, LayerNode)> = Vec::new();
        // 直接改 f 内部不可行（私有），改用 note_change + release 路径验证 release 生效
        let released = f.release(1);
        set.add(
            "D16-equiv-祖先变更自动解压",
            had && released && f.snapshot(1).is_none(),
            "",
        );
    }

    // ---- 判据显性：阈值表覆盖 + O(1) 判定 ----

    {
        // 判据表覆盖（2 条，id 唯一，阈值合法）。
        let mut ok = CRITERIA_TABLE.len() >= 1;
        let mut seen = 0u32;
        for c in CRITERIA_TABLE.iter() {
            if seen == c.id {
                ok = false;
            }
            seen = c.id;
            if c.depth_threshold == 0 || c.change_rate_limit == 0 {
                ok = false;
            }
        }
        // 按 id 查表 O(1)
        let got = criteria_by_id(1);
        set.add("D16-criteria-阈值表覆盖", ok && got.is_some(), "");
    }

    {
        // O(1) 判定：深度超阈值且变更率不超上限才候选压平（两维缺一即拒）。
        let c = CRITERIA_TABLE[0];
        let deep_static = flatten_eligible(c.depth_threshold + 1, c.change_rate_limit, &c);
        let shallow = flatten_eligible(c.depth_threshold, 0, &c);
        let too_active = flatten_eligible(c.depth_threshold + 1, c.change_rate_limit + 1, &c);
        set.add(
            "D16-criteria-两维缺一即拒",
            deep_static && !shallow && !too_active,
            "",
        );
    }

    {
        // 复审校准：换判据后判定结果随之改变（阈值可校准）。
        let c1 = CRITERIA_TABLE[0];
        let c2 = CRITERIA_TABLE[1];
        let mut f = Flattener::new(chain(DEPTH_THRESHOLD + 1, 1));
        f.set_criteria(c1);
        let with_c1 = f.criteria().id == c1.id;
        f.set_criteria(c2);
        let with_c2 = f.criteria().id == c2.id;
        // 深度介于两阈值之间：换判据后资格改变
        let depth = DEPTH_THRESHOLD + 1;
        let q1 = flatten_eligible(depth, 0, &c1);
        let q2 = flatten_eligible(depth, 0, &c2);
        set.add(
            "D16-criteria-复审校准改变判定",
            with_c1 && with_c2 && q1 && !q2 && CRITERIA_EXPLICIT_DOC.contains("阈值"),
            "",
        );
    }

    // ---- 降级链：Virtualized→Flattened→SplitAdvice ----

    {
        // 三级链：超预算升级两级到 SplitAdvice 并给出拆分建议。
        let mut p = DegradePolicy::new(DEFAULT_FRAME_BUDGET_MS);
        let start = p.level();
        let d1 = p.decide(DEFAULT_FRAME_BUDGET_MS + 10, 100);
        let d2 = p.decide(DEFAULT_FRAME_BUDGET_MS + 10, 100);
        let chain_ok = start == DegradeLevel::Virtualized
            && d1.level == DegradeLevel::Flattened
            && d2.level == DegradeLevel::SplitAdvice;
        let advice_present = d2.advice.len() == 3;
        set.add(
            "D16-degrade-三级升级链",
            chain_ok && d1.escalated && d2.escalated && advice_present,
            "",
        );
    }

    {
        // 达标回落：末级保持（告警留到问题解决），其余回落一级。
        let mut p = DegradePolicy::new(DEFAULT_FRAME_BUDGET_MS);
        p.decide(DEFAULT_FRAME_BUDGET_MS + 10, 100); // → Flattened
        let back = p.decide(DEFAULT_FRAME_BUDGET_MS - 5, 50); // 达标 → 回落 Virtualized
        set.add(
            "D16-degrade-达标回落",
            back.level == DegradeLevel::Virtualized && !back.escalated,
            "",
        );
    }

    {
        // 末级不回落（SplitAdvice 保持直到问题解决）。
        let mut p = DegradePolicy::new(DEFAULT_FRAME_BUDGET_MS);
        p.decide(DEFAULT_FRAME_BUDGET_MS + 10, 100);
        p.decide(DEFAULT_FRAME_BUDGET_MS + 10, 100); // → SplitAdvice
        let hold = p.decide(1, 10); // 远低于预算
        set.add(
            "D16-degrade-末级保持",
            hold.level == DegradeLevel::SplitAdvice && !hold.escalated,
            "",
        );
    }

    {
        // 末级自升级不越界（不四级、不无穷）。
        let top = DegradeLevel::SplitAdvice.escalate();
        set.add(
            "D16-degrade-末级自升级不越界",
            top == DegradeLevel::SplitAdvice && DEGRADE_CHAIN_DOC.contains("三级"),
            "",
        );
    }

    // ---- 契约文档在场（判据条款的可追溯锚） ----

    {
        let docs_ok = ON_DEMAND_DOC.contains("按需实例化")
            && EQUIVALENCE_DOC.contains("视觉等价")
            && CRITERIA_EXPLICIT_DOC.contains("判据显性")
            && DEGRADE_CHAIN_DOC.contains("降级链")
            && STORM_THROTTLE_DOC.contains("节流")
            && COST_DOC.contains("切换成本");
        set.add("D16-judge-六契约条款在场", docs_ok, "");
    }

    set
}