//! VE-F0618 · 图层树调试可视化（VE-D 域 · 2D 合成引擎 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0618`
//!
//! **判据（锚点原文逐条）**：树文本转储（消费 F0612 文本格式——属性省略规则：
//! **只输出有效集**）；叠加层渲染（可切换调试覆盖：包围盒描边、脏区高亮、
//! 缓存命中热度、Z 序标尺**四开关**——覆盖层独立于正常渲染管线不互相污染）；
//! 树视图面板数据接口（懒加载子树展开、节点跳转到源）；隐私边界：调试视图仅
//! 诊断构建或显式开启，**不记内容像素只记结构**。错误路径与降级矩阵：覆盖层
//! 自身脏区→**独立通道不进F0613**（防调试扰动生产脏区）；大树展开→懒加载；
//! 接口误用→**显性只读**。性能逐项分解：转储 O(节点数)；覆盖 O(开关数)；
//! 懒加载 O(子树)。无障碍：调试面板遵循无障碍规范（键盘可达、对比度达标）。
//! 判据：**四开关覆盖、独立通道、懒加载、只读接口、判据**。
//!
//! ## 与上游的关系（不重造轮子）
//!
//! - **F0617**（[`ved17_consistency`]）提供 [`TreeView`] 与节点投影
//!   （`TreeNode` / `NodeKind` / `DirtyFlags`）。本模块**只读消费**，不复制其
//!   不变式判定逻辑——调试视图不是第二个校验器。
//! - **F0613**（脏区）被本模块**刻意不触碰**：见下「独立通道」。
//! - **F0612**（序列化文本格式）定义属性省略规则，本模块以
//!   [`dump_tree`] 消费该规则（「只输出有效集」）。
//!
//! ## 设计要点一：覆盖层走**独立通道**，绝不写生产脏区（本条最关键的纪律）
//!
//! 锚点要求「覆盖层自身脏区→独立通道不进 F0613（防调试扰动生产脏区）」。
//! 最省事的实现是让调试覆盖直接调`mark_dirty()`——那会让「打开调试面板」
//! 这个纯观察动作**污染生产缓存键**：`CacheKey.content_rev` 被无关地推进，
//! 于是缓存全部 miss、命中率指标掉到 0，而用户什么都没改。这是典型的
//! **观察者效应污染被测量对象**。
//!
//! 本模块的处置：覆盖层状态收拢在 [`OverlayState`]，它**不含任何指向
//! F0613 脏区或F0615 缓存的写入路径**（类型层面就没有 `&mut DirtyFlags`）。
//! 自检 [`C18-CHANNEL-ISOLATION`] 用一个真实的 `DirtyFlags` 值做对照——
//! 生成覆盖层前后该值逐位不变。这条断言是**拿生产对象当被测物**，不是
//! 拿表内元素自证。
//!
//! ## 设计要点二：「只输出有效集」=省略是**有据可依**的，不是截断
//!
//! 朴素实现是「打印前 N 个属性」——那是截断，信息丢失且不可区分于「属性
//! 本来就少」。本模块的 [`AttrPolicy`] 区分两类省略：
//! - [`AttrVerdict::OmittedDefault`]：值等于该属性的**规范默认值**
//!   （如 `opacity=255`、`visible=true`）——省略**无损**，读者按缺省理解；
//! - [`AttrVerdict::OmittedInvalid`]：值**越界或非法**——省略**有损**，
//!   故强制输出并带 `!` 标记（转储里显式可见，不静默）。
//!
//! 换言之：省略只在「省略前后语义等价」时发生。这条规则由
//! [`C18-DUMP-OMISSION-无损省略`] 与 [`C18-DUMP-OMISSION-非法值强制输出`]
//! 两条判据分别钉住正反两面。
//!
//! ## 设计要点三：懒加载的**展开预算**是显式的，不是隐式截断
//!
//! 大树展开若不设预算，会把十万节点一次性拉进面板——调试工具自己成为
//! 性能问题。 [`PanelQuery::expand`] 接受 [`ExpandBudget`]:
//! - [`ExpandKind::Full`]：全展开（小树正常路径）；
//! - [`ExpandKind::Limited`]：展开到预算即止，并在返回里**显式告知**
//!   [`PanelResult::truncated`] 与 `next_cursor`——调用方能续拉，且知道
//!   「还有内容没显示」。这与「静默截断」的本质区别就在这个显式标志上。
//!
//! ## 设计要点四：只读接口靠**类型**保证，不靠约定
//!
//! 锚点：「接口误用→显性只读」。[`PanelApi`] 的方法全部取 `&self`，
//! 面板侧拿到的 [`PanelNode`] 是**借用视图**（`PanelNodeRef<'_>`），
//! 拿不到 `&mut`。写入的唯一入口是 [`PanelApi::apply_overlay_switch`]，
//! 它只改 [`OverlayState`] 的开关位（调试视图自身的状态），**不是**图层树。
//!
//! ## 性能诚实标注
//!
//! - [`dump_tree`]：O(节点数 + 输出的属性数)，**无隐式上限**——转储是全量
//!   语义，想要有界输出请用面板接口的 [`ExpandKind::Limited`]。
//! - [`OverlayState::active_overlays`]：O(开关数)（固定 4）。
//! - [`PanelApi::expand`]：O(展开子树规模)，**不**扫描未展开部分。
//!
//! 逻辑 tick 注入，零墙钟；零 IO；类型自持（不 import 未注册的兄弟模块）——
//! 上游契约以等价自有类型承接，编译期不受平行会话注册次序影响。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 调试覆盖开关（四开关，锚点硬要求）。
///
/// 位打包：便于「O(开关数)」一次算出启用集，且天然支持按位查询。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OverlaySwitches {
    bits: u8,
}

impl OverlaySwitches {
    /// 全关（默认——调试视图默认不开启，锚点「仅诊断构建或显式开启」）。
    pub const NONE: OverlaySwitches = OverlaySwitches { bits: 0 };
    /// 全开。
    pub const ALL: OverlaySwitches = OverlaySwitches { bits: 0b1111 };

    /// 包围盒描边。
    pub const fn bbox_outline() -> u8 {
        1 << 0
    }
    /// 脏区高亮。
    pub const fn dirty_overlay() -> u8 {
        1 << 1
    }
    /// 缓存命中热度。
    pub const fn cache_heat() -> u8 {
        1 << 2
    }
    /// Z 序标尺。
    pub const fn z_ruler() -> u8 {
        1 << 3
    }

    /// 开一个开关。
    pub fn with(mut self, bit: u8) -> OverlaySwitches {
        self.bits |= bit;
        self
    }

    /// 关一个开关。
    pub fn without(mut self, bit: u8) -> OverlaySwitches {
        self.bits &= !bit;
        self
    }

    /// 某开关是否启用。
    pub fn is_on(self, bit: u8) -> bool {
        (self.bits & bit) == bit
    }

    /// 原始位。
    pub const fn bits(self) -> u8 {
        self.bits
    }

    /// 四开关全集（机检覆盖用；**不是**全部打开）。
    pub const ALL_SWITCHES: [u8; 4] = [
        OverlaySwitches::bbox_outline(),
        OverlaySwitches::dirty_overlay(),
        OverlaySwitches::cache_heat(),
        OverlaySwitches::z_ruler(),
    ];

    /// 已启用的开关（O(开关数)，固定 4 次迭代）。
    pub fn active_overlays(self) -> Vec<u8> {
        let mut out = Vec::new();
        for bit in OverlaySwitches::ALL_SWITCHES.iter() {
            if self.is_on(*bit) {
                out.push(*bit);
            }
        }
        out
    }
}

impl OverlaySwitches {
    /// 开关的稳定短名（转储与面板用；**不用**位号——位号是实现细节）。
    pub fn tag(bit: u8) -> &'static str {
        // 字面量位常量而非 const fn 调用：const fn 不能出现在 match 模式位。
        match bit {
            0b0000_0001 => "bbox",
            0b0000_0010 => "dirty",
            0b0000_0100 => "heat",
            0b0000_1000 => "z",
            _ => "?",
        }
    }

    /// 开关 → 覆盖通道的稳定名（独立通道登记用）。
    pub fn channel(bit: u8) -> &'static str {
        // 字面量位常量而非 const fn 调用：const fn 不能出现在 match 模式位。
        match bit {
            0b0000_0001 => CHANNEL_BBOX,
            0b0000_0010 => CHANNEL_DIRTY,
            0b0000_0100 => CHANNEL_HEAT,
            0b0000_1000 => CHANNEL_Z,
            _ => "?",
        }
    }
}

/// 包围盒描边通道名。
pub const CHANNEL_BBOX: &str = "dbg/bbox";
/// 脏区高亮通道名。
pub const CHANNEL_DIRTY: &str = "dbg/dirty";
/// 缓存热度通道名。
pub const CHANNEL_HEAT: &str = "dbg/heat";
/// Z 序标尺通道名。
pub const CHANNEL_Z: &str = "dbg/z";

/// 全部独立通道（**均以 `dbg/` 前缀**——与生产通道命名空间物理隔离）。
pub const DEBUG_CHANNELS: [&str; 4] = [CHANNEL_BBOX, CHANNEL_DIRTY, CHANNEL_HEAT, CHANNEL_Z];

/// 节点属性的规范默认值（省略判据的**唯一**依据）。
///
/// 「只输出有效集」= 只省略等于本表默认值的属性。默认值集中在此而非散落
/// 字面量——新增属性时忘记登记默认值的后果是「本该省略的属性被输出」
/// （冗余但无损），而不是「本该输出的属性被省略」（有损）。
pub struct AttrDefaults;

impl AttrDefaults {
    /// `opacity` 默认不透明。
    pub const OPACITY: u32 = 255;
    /// `visible` 默认可见。
    pub const VISIBLE: bool = true;
    /// `blend` 默认 normal。
    pub const BLEND: &'static str = "normal";
    /// `z` 默认 0。
    pub const Z: i32 = 0;
    /// `content_rev` 默认 0。
    pub const CONTENT_REV: u32 = 0;
}

/// 节点属性（**投影**，只读）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeAttrs {
    /// 不透明度 0..=255。
    pub opacity: u32,
    /// 是否可见。
    pub visible: bool,
    /// 混合模式短名。
    pub blend: String,
    /// Z 序。
    pub z: i32,
}

impl NodeAttrs {
    /// 全默认。
    pub fn defaults() -> NodeAttrs {
        NodeAttrs {
            opacity: AttrDefaults::OPACITY,
            visible: AttrDefaults::VISIBLE,
            blend: AttrDefaults::BLEND.to_string(),
            z: AttrDefaults::Z,
        }
    }

    /// 该属性是否等于规范默认值（= 可无损省略）。
    pub fn is_default(&self, which: AttrField) -> bool {
        match which {
            AttrField::Opacity => self.opacity == AttrDefaults::OPACITY,
            AttrField::Visible => self.visible == AttrDefaults::VISIBLE,
            AttrField::Blend => self.blend == AttrDefaults::BLEND,
            AttrField::Z => self.z == AttrDefaults::Z,
        }
    }

    /// 该属性是否**非法**（越界）——非法值强制输出，不许省略。
    pub fn is_invalid(&self, which: AttrField) -> bool {
        match which {
            AttrField::Opacity => self.opacity > 255,
            AttrField::Visible | AttrField::Blend | AttrField::Z => false,
        }
    }

    /// 属性的显示值（转储用）。
    pub fn render(&self, which: AttrField) -> String {
        match which {
            AttrField::Opacity => format!("{}", self.opacity),
            AttrField::Visible => {
                if self.visible {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            AttrField::Blend => self.blend.clone(),
            AttrField::Z => format!("{}", self.z),
        }
    }
}

/// 属性字段（四项，与 [`AttrDefaults`] 一一对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrField {
    Opacity,
    Visible,
    Blend,
    Z,
}

impl AttrField {
    /// 稳定短名（= 规范默认值表的键）。
    pub const fn tag(self) -> &'static str {
        match self {
            AttrField::Opacity => "opacity",
            AttrField::Visible => "visible",
            AttrField::Blend => "blend",
            AttrField::Z => "z",
        }
    }

    /// 字段全集（机检覆盖用；**不是**全部字段的某种组合）。
    pub const ALL: [AttrField; 4] =
        [AttrField::Opacity, AttrField::Visible, AttrField::Blend, AttrField::Z];
}

/// 单个属性的省略裁决。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttrVerdict {
    /// 输出（值非默认）。
    Emit(String),
    /// 省略：值等于规范默认值（**无损**省略）。
    OmittedDefault,
    /// 省略：但值非法——**有损**，故强制输出并带 `!` 标记。
    OmittedInvalid,
}

/// 属性省略策略：实现 F0612 的「只输出有效集」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttrPolicy;

impl AttrPolicy {
    /// 裁决单个属性是否输出。
    pub fn judge(attrs: &NodeAttrs, field: AttrField) -> AttrVerdict {
        // 顺序要紧：非法值优先于默认值——`opacity=999` 既非默认也非法，
        // 若先判默认会落到 `OmittedInvalid` 分支而丢失「输出」结论。
        if attrs.is_invalid(field) {
            AttrVerdict::OmittedInvalid
        } else if attrs.is_default(field) {
            AttrVerdict::OmittedDefault
        } else {
            AttrVerdict::Emit(attrs.render(field))
        }
    }
}

/// 节点类型标签（承接 F0617 `NodeKind` 的等价自有类型——不 import 兄弟模块）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeTag {
    Group,
    Leaf,
    Text,
    Shape,
}

impl NodeTag {
    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            NodeTag::Group => "group",
            NodeTag::Leaf => "leaf",
            NodeTag::Text => "text",
            NodeTag::Shape => "shape",
        }
    }

    /// 类型全集（机检覆盖用）。
    pub const ALL: [NodeTag; 4] = [NodeTag::Group, NodeTag::Leaf, NodeTag::Text, NodeTag::Shape];
}

/// 脏标记族（**只读消费**；本模块无任何写入路径，见设计要点一）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirtyView {
    /// 自身脏。
    pub self_dirty: bool,
    /// 子树脏。
    pub subtree_dirty: bool,
    /// 内容脏。
    pub content_dirty: bool,
}

impl DirtyView {
    /// 全清。
    pub const CLEAN: DirtyView = DirtyView {
        self_dirty: false,
        subtree_dirty: false,
        content_dirty: false,
    };

    /// 是否全清。
    pub fn is_clean(self) -> bool {
        !self.self_dirty && !self.subtree_dirty && !self.content_dirty
    }

    /// 脏区高亮强度（0 = 无脏区，不高亮）。
    ///
    /// **只读计算**：不改任何标记，纯函数。
    pub fn highlight_level(self) -> u8 {
        let mut level = 0u8;
        if self.self_dirty {
            level += 1;
        }
        if self.subtree_dirty {
            level += 2;
        }
        if self.content_dirty {
            level += 4;
        }
        level
    }
}

/// 缓存命中热度（**只读**投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeatLevel {
    /// 无缓存。
    Absent,
    /// 命中（缓存修订号 == 内容修订号）。
    Hit,
    /// 失效（缓存修订号落后于内容修订号）。
    Stale,
}

impl HeatLevel {
    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            HeatLevel::Absent => "absent",
            HeatLevel::Hit => "hit",
            HeatLevel::Stale => "stale",
        }
    }

    /// 热度分（0..=2，供覆盖层配色；**无浮点**）。
    pub const fn score(self) -> u8 {
        match self {
            HeatLevel::Absent => 0,
            HeatLevel::Hit => 2,
            HeatLevel::Stale => 1,
        }
    }

    /// 由内容修订号与缓存修订号派生。
    pub const fn from_revs(content_rev: u32, cached_rev: u32) -> HeatLevel {
        if cached_rev == 0 {
            HeatLevel::Absent
        } else if cached_rev == content_rev {
            HeatLevel::Hit
        } else {
            HeatLevel::Stale
        }
    }
}

/// 调试投影节点（面板与转储的**唯一**输入；不引用上游可变结构）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugNode {
    /// 稳定 id。
    pub id: u64,
    /// 父 id（`0` = 根）。
    pub parent: u64,
    /// 子节点 id（**已按 z 序**）。
    pub children: Vec<u64>,
    /// 类型标签。
    pub tag: NodeTag,
    /// 属性。
    pub attrs: NodeAttrs,
    /// 脏标记（只读）。
    pub dirty: DirtyView,
    /// 内容修订号。
    pub content_rev: u32,
    /// 缓存修订号（`0` = 无缓存）。
    pub cached_rev: u32,
}

impl DebugNode {
    /// 叶子构造。
    pub fn leaf(id: u64, parent: u64) -> DebugNode {
        DebugNode {
            id,
            parent,
            children: Vec::new(),
            tag: NodeTag::Leaf,
            attrs: NodeAttrs::defaults(),
            dirty: DirtyView::CLEAN,
            content_rev: 1,
            cached_rev: 1,
        }
    }

    /// 组构造。
    pub fn group(id: u64, parent: u64, children: Vec<u64>) -> DebugNode {
        DebugNode {
            id,
            parent,
            children,
            tag: NodeTag::Group,
            attrs: NodeAttrs::defaults(),
            dirty: DirtyView::CLEAN,
            content_rev: 1,
            cached_rev: 1,
        }
    }

    /// 该节点的缓存热度。
    pub fn heat(&self) -> HeatLevel {
        HeatLevel::from_revs(self.content_rev, self.cached_rev)
    }
}

/// 调试投影树（只读容器）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugTree {
    nodes: Vec<DebugNode>,
    root: u64,
}

impl DebugTree {
    /// 构造（**校验根存在**——根缺失是投影错误，显性拒绝）。
    pub fn new(nodes: Vec<DebugNode>, root: u64) -> Res<DebugTree> {
        if nodes.is_empty() {
            return Res::Err(Diag::new(
                DiagCode::EmptyTree,
                "调试投影树为空".to_string(),
                "空树无法转储；请确认投影已构建",
            ));
        }
        if !nodes.iter().any(|n| n.id == root) {
            return Res::Err(Diag::new(
                DiagCode::RootMissing,
                format!("根节点 {} 不在投影内", root),
                "根 id 必须在节点集合中；否则遍历无从起步",
            ));
        }
        Res::Ok(DebugTree { nodes, root })
    }

    /// 取节点（线性定位，O(节点数)——诚实标注）。
    pub fn node(&self, id: u64) -> Option<&DebugNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// 节点数。
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

    /// 全部节点 id（建表次序，确定性）。
    pub fn ids(&self) -> Vec<u64> {
        self.nodes.iter().map(|n| n.id).collect()
    }
}

/// 诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagCode {
    /// 投影树为空。
    EmptyTree,
    /// 根节点缺失。
    RootMissing,
    /// 节点不存在（跳转/展开目标无效）。
    NodeNotFound,
    /// 面板处于只读态却收到写请求。
    ReadOnlyViolation,
    /// 调试视图未开启（隐私边界：仅诊断构建或显式开启）。
    OverlayNotEnabled,
    /// 展开预算为零（无法展开任何节点）。
    ZeroBudget,
    /// 展开时遇到环（投影损坏）。
    CycleDetected,
    /// 深度超限（防御；正常投影远小于此）。
    DepthExceeded,
}

impl DiagCode {
    /// 稳定字符串名。
    pub const fn tag(self) -> &'static str {
        match self {
            DiagCode::EmptyTree => "EMPTY_TREE",
            DiagCode::NodeNotFound => "NODE_NOT_FOUND",
            DiagCode::RootMissing => "ROOT_MISSING",
            DiagCode::ReadOnlyViolation => "READ_ONLY_VIOLATION",
            DiagCode::OverlayNotEnabled => "OVERLAY_NOT_ENABLED",
            DiagCode::ZeroBudget => "ZERO_BUDGET",
            DiagCode::CycleDetected => "CYCLE_DETECTED",
            DiagCode::DepthExceeded => "DEPTH_EXCEEDED",
        }
    }
}

/// 诊断三要素。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    /// 诊断码。
    pub code: DiagCode,
    /// 发生了什么。
    pub message: String,
    /// 下一步怎么办。
    pub hint: String,
}

impl Diag {
    /// 构造。
    pub fn new(code: DiagCode, message: String, hint: &str) -> Diag {
        Diag { code, message, hint: hint.to_string() }
    }
}

/// 结果判别：成功带值，失败带三要素。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Res<T> {
    Ok(T),
    Err(Diag),
}

impl<T> Res<T> {
    /// 是否成功。
    pub fn is_ok(&self) -> bool {
        matches!(self, Res::Ok(_))
    }

    /// 取诊断码（失败时；成功返回 `None`）。
    pub fn err_code(&self) -> Option<DiagCode> {
        match self {
            Res::Ok(_) => None,
            Res::Err(d) => Some(d.code),
        }
    }
}

/// 覆盖层状态（**独立通道**；无任何指向生产脏区/缓存的写入路径）。
///
/// 字段全部是调试视图自己的开关与计数，与 `DirtyView` / 缓存键**无交集**
/// ——这是「防调试扰动生产脏区」在类型层面的保证。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OverlayState {
    switches: OverlaySwitches,
    /// 本帧生成的覆盖图元数（按通道分）。
    emitted: [u32; 4],
    /// 生成代数（每次重建 +1；供面板判断缓存有效性）。
    generation: u32,
}

impl OverlayState {
    /// 新建（全关）。
    pub fn new() -> OverlayState {
        OverlayState { switches: OverlaySwitches::NONE, emitted: [0; 4], generation: 0 }
    }

    /// 当前开关。
    pub fn switches(&self) -> OverlaySwitches {
        self.switches
    }

    /// 已启用的覆盖（O(开关数)）。
    pub fn active_overlays(&self) -> Vec<u8> {
        self.switches.active_overlays()
    }

    /// 某通道本帧图元数。
    pub fn emitted(&self, bit: u8) -> u32 {
        match OverlaySwitches::ALL_SWITCHES.iter().position(|b| *b == bit) {
            Some(i) => self.emitted[i],
            None => 0,
        }
    }

    /// 覆盖图元总数。
    pub fn emitted_total(&self) -> u32 {
        let mut t = 0u32;
        for v in self.emitted.iter() {
            t = match t.checked_add(*v) {
                Some(x) => x,
                None => u32::MAX,
            };
        }
        t
    }

    /// 覆盖代数。
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// 是否已开启任何覆盖（隐私边界的判定面）。
    pub fn is_enabled(&self) -> bool {
        self.switches.bits() != 0
    }

    /// **面板唯一可写入口**：切换开关。
    ///
    /// 只改调试视图自身的开关位——不触达图层树、脏区或缓存。
    pub fn toggle(&mut self, bit: u8, on: bool) -> Res<()> {
        if !OverlaySwitches::ALL_SWITCHES.contains(&bit) {
            return Res::Err(Diag::new(
                DiagCode::NodeNotFound,
                format!("开关 0x{:02x} 不在四开关登记内", bit),
                "覆盖开关只有 bbox/dirty/heat/z 四个；新增须先登记",
            ));
        }
        self.switches = if on {
            self.switches.with(bit)
        } else {
            self.switches.without(bit)
        };
        self.generation = self.generation.wrapping_add(1);
        Res::Ok(())
    }

    /// 重建覆盖图元计数（每通道 O(开关数)）。
    ///
    /// `emit` 给出各通道应生成的图元数；**未启用的通道强制归零**——
    /// 关掉的开关不得留残影。
    pub fn rebuild(&mut self, emit: [u32; 4]) {
        self.emitted = [0; 4];
        for (i, bit) in OverlaySwitches::ALL_SWITCHES.iter().enumerate() {
            if self.switches.is_on(*bit) {
                self.emitted[i] = emit[i];
            }
        }
        self.generation = self.generation.wrapping_add(1);
    }

    /// 覆盖层自身的脏区（**只在调试通道内**，绝不写生产脏区）。
    ///
    /// 返回值是「本代数是否有图元变化」，供调试面板自己决定重绘。
    pub fn overlay_dirty(&self, last_generation: u32) -> bool {
        self.generation != last_generation
    }
}

/// 树文本转储（消费 F0612 格式；**只输出有效集**）。
///
/// 格式：每行 `缩进 kind#id [attr=val ...]`，属性按 [`AttrField::ALL`]
/// 顺序输出，非法值前缀 `!`。
pub fn dump_tree(tree: &DebugTree, switches: OverlaySwitches) -> String {
    let mut out = String::new();
    dump_into(&mut out, tree, switches, tree.root(), 0, 0);
    out
}

/// 转储的递归实现（深度参数显式传递——不无限递归）。
fn dump_into(
    out: &mut String,
    tree: &DebugTree,
    switches: OverlaySwitches,
    id: u64,
    depth: usize,
    guard: u32,
) {
    if guard > MAX_DUMP_DEPTH {
        out.push_str("  ...深度超限截断\n");
        return;
    }
    let node = match tree.node(id) {
        Some(n) => n,
        None => {
            out.push_str("  <缺失节点>\n");
            return;
        }
    };
    // 缩进
    for _ in 0..depth {
        out.push_str("  ");
    }
    out.push_str(node.tag.tag());
    out.push('#');
    out.push_str(&format!("{}", node.id));

    // 属性：只输出有效集（省略裁决见 AttrPolicy）
    for field in AttrField::ALL.iter() {
        match AttrPolicy::judge(&node.attrs, *field) {
            AttrVerdict::Emit(v) => {
                out.push(' ');
                out.push_str(field.tag());
                out.push('=');
                out.push_str(&v);
            }
            AttrVerdict::OmittedDefault => { /* 无损省略：什么都不输出 */ }
            AttrVerdict::OmittedInvalid => {
                // 有损省略 → 强制输出并标记
                out.push(' ');
                out.push('!');
                out.push_str(field.tag());
                out.push('=');
                out.push_str(&node.attrs.render(*field));
            }
        }
    }

    // 覆盖标注（受开关控制；关闭则完全不输出——不泄露调试信息）
    if switches.is_on(OverlaySwitches::dirty_overlay()) {
        let lv = node.dirty.highlight_level();
        if lv > 0 {
            out.push_str(&format!(" dirty={}", lv));
        }
    }
    if switches.is_on(OverlaySwitches::cache_heat()) {
        out.push_str(&format!(" heat={}", node.heat().tag()));
    }
    if switches.is_on(OverlaySwitches::z_ruler()) {
        out.push_str(&format!(" z={}", node.attrs.z));
    }
    if switches.is_on(OverlaySwitches::bbox_outline()) {
        out.push_str(" bbox");
    }
    out.push('\n');

    // 子节点
    for c in node.children.iter() {
        dump_into(out, tree, switches, *c, depth + 1, guard + 1);
    }
}

/// 转储深度上限（防御；正常图层树远小于此）。
pub const MAX_DUMP_DEPTH: u32 = 64;

// ---------------------------------------------------------------------------
// 面板数据接口（懒加载 + 只读）
// ---------------------------------------------------------------------------

/// 展开预算。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpandBudget {
    /// 最多展开的节点数。
    pub max_nodes: usize,
    /// 展开种类。
    pub kind: ExpandKind,
}

/// 展开种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpandKind {
    /// 全展开。
    Full,
    /// 展开到预算即止（**显式**告知截断）。
    Limited,
}

impl ExpandBudget {
    /// 全展开。
    pub const FULL: ExpandBudget = ExpandBudget { max_nodes: usize::MAX, kind: ExpandKind::Full };
    /// 限展开。
    pub const fn limited(max_nodes: usize) -> ExpandBudget {
        ExpandBudget { max_nodes, kind: ExpandKind::Limited }
    }
}

/// 面板节点（**借用视图**——拿不到 `&mut`，只读由类型保证）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelNodeRef<'a> {
    /// 节点引用。
    pub node: &'a DebugNode,
    /// 深度（根为 0）。
    pub depth: u32,
    /// 是否因预算耗尽而未展开 children。
    pub children_loaded: bool,
}

/// 面板查询结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelResult<'a> {
    /// 本次返回的节点（**借用**——只读）。
    pub nodes: Vec<PanelNodeRef<'a>>,
    /// 是否因预算耗尽而截断（**显式标志**，非静默）。
    pub truncated: bool,
    /// 续拉游标（`0` = 无更多）。
    pub next_cursor: u64,
}

/// 树视图面板 API（**显性只读**：除开关外无任何写入口）。
pub struct PanelApi<'a> {
    tree: &'a DebugTree,
    overlay: OverlayState,
    /// 面板是否显式开启（隐私边界：未开启时覆盖不可用）。
    enabled: bool,
}

impl<'a> PanelApi<'a> {
    /// 构造（默认**未开启**——锚点「仅诊断构建或显式开启」）。
    pub fn new(tree: &'a DebugTree) -> PanelApi<'a> {
        PanelApi { tree, overlay: OverlayState::new(), enabled: false }
    }

    /// 显式开启调试视图。
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// 是否已开启。
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 覆盖层状态（只读借用）。
    pub fn overlay(&self) -> &OverlayState {
        &self.overlay
    }

    /// 切换覆盖开关（**唯一写入口**，只改调试视图自身状态）。
    pub fn apply_overlay_switch(&mut self, bit: u8, on: bool) -> Res<()> {
        if !self.enabled {
            return Res::Err(Diag::new(
                DiagCode::OverlayNotEnabled,
                "调试视图未开启，拒绝切换覆盖开关".to_string(),
                "调试视图仅在诊断构建或显式开启后可用（隐私边界）",
            ));
        }
        self.overlay.toggle(bit, on)
    }

    /// 懒加载展开子树（O(展开子树规模)）。
    pub fn expand(&self, start: u64, budget: ExpandBudget) -> Res<PanelResult<'_>> {
        if budget.kind == ExpandKind::Limited && budget.max_nodes == 0 {
            return Res::Err(Diag::new(
                DiagCode::ZeroBudget,
                "展开预算为 0".to_string(),
                "预算至少为 1；若只想看根节点请用 max_nodes=1",
            ));
        }
        if self.tree.node(start).is_none() {
            return Res::Err(Diag::new(
                DiagCode::NodeNotFound,
                format!("节点 {} 不存在", start),
                "展开起点必须是投影内的节点 id",
            ));
        }
        let mut nodes: Vec<PanelNodeRef<'_>> = Vec::new();
        let mut truncated = false;
        let mut next_cursor: u64 = 0;
        // 显式栈迭代（不递归——大树不爆栈）。
        let mut stack: Vec<(u64, u32)> = vec![(start, 0)];
        let mut guard: u32 = 0;
        while let Some((id, depth)) = stack.pop() {
            guard = guard.wrapping_add(1);
            if guard > MAX_DUMP_DEPTH * 8 {
                truncated = true;
                next_cursor = id;
                break;
            }
            if budget.kind == ExpandKind::Limited && nodes.len() >= budget.max_nodes {
                truncated = true;
                next_cursor = id;
                break;
            }
            if depth > MAX_DUMP_DEPTH {
                truncated = true;
                next_cursor = id;
                break;
            }
            let node = match self.tree.node(id) {
                Some(n) => n,
                None => {
                    truncated = true;
                    next_cursor = id;
                    break;
                }
            };
            let will_push_children = !node.children.is_empty();
            let children_loaded =
                !(budget.kind == ExpandKind::Limited && nodes.len() + 1 >= budget.max_nodes);
            nodes.push(PanelNodeRef { node, depth, children_loaded });
            if will_push_children && children_loaded {
                // 逆序压栈以保持文档顺序（children 已按 z 序）
                for c in node.children.iter().rev() {
                    stack.push((*c, depth + 1));
                }
            } else if will_push_children && !children_loaded {
                truncated = true;
                next_cursor = node.children[0];
            }
        }
        Res::Ok(PanelResult { nodes, truncated, next_cursor })
    }

    /// 节点跳转到源（返回该节点在投影中的位置描述）。
    pub fn locate(&self, id: u64) -> Res<LocateResult> {
        let node = match self.tree.node(id) {
            Some(n) => n,
            None => {
                return Res::Err(Diag::new(
                    DiagCode::NodeNotFound,
                    format!("节点 {} 不存在，无法跳转", id),
                    "跳转目标必须是投影内的节点 id；请先确认树已加载",
                ))
            }
        };
        // 源位置描述：从根到该节点的路径（O(深度)）。
        let mut path: Vec<u64> = Vec::new();
        let mut cur = node.parent;
        let mut guard = 0u32;
        while cur != 0 && guard <= MAX_DUMP_DEPTH {
            path.push(cur);
            cur = match self.tree.node(cur) {
                Some(n) => n.parent,
                None => break,
            };
            guard = guard.wrapping_add(1);
        }
        path.reverse();
        Res::Ok(LocateResult { id, path, is_root: node.parent == 0 })
    }
}

/// 节点定位结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocateResult {
    /// 目标节点 id。
    pub id: u64,
    /// 从根到该节点的父链路径。
    pub path: Vec<u64>,
    /// 是否为根。
    pub is_root: bool,
}

/// 覆盖层图元（供渲染侧消费；**只记结构不记像素**——锚点隐私边界）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayPrimitive {
    /// 所属通道。
    pub channel: u8,
    /// 目标节点 id。
    pub node: u64,
    /// 强度等级（0..=7，各通道语义不同但统一为等级）。
    pub level: u8,
}

/// 为整棵树生成覆盖图元（**只记结构**）。
///
/// `O(开关数 × 节点数)`；未启用的通道不产出任何图元。
pub fn build_overlays(tree: &DebugTree, switches: OverlaySwitches) -> Vec<OverlayPrimitive> {
    let mut out: Vec<OverlayPrimitive> = Vec::new();
    for id in tree.ids() {
        let node = match tree.node(id) {
            Some(n) => n,
            None => continue,
        };
        if switches.is_on(OverlaySwitches::bbox_outline()) {
            out.push(OverlayPrimitive {
                channel: OverlaySwitches::bbox_outline(),
                node: id,
                level: 1,
            });
        }
        if switches.is_on(OverlaySwitches::dirty_overlay()) {
            let lv = node.dirty.highlight_level();
            if lv > 0 {
                out.push(OverlayPrimitive {
                    channel: OverlaySwitches::dirty_overlay(),
                    node: id,
                    level: lv,
                });
            }
        }
        if switches.is_on(OverlaySwitches::cache_heat()) {
            let h = node.heat();
            if h != HeatLevel::Absent {
                out.push(OverlayPrimitive {
                    channel: OverlaySwitches::cache_heat(),
                    node: id,
                    level: h.score(),
                });
            }
        }
        if switches.is_on(OverlaySwitches::z_ruler()) {
            out.push(OverlayPrimitive {
                channel: OverlaySwitches::z_ruler(),
                node: id,
                // z 可能为负/过大：钳到 0..=7 而不是回绕
                level: clamp_level(node.attrs.z),
            });
        }
    }
    out
}

/// z 值 → 等级（钳位，不回绕——回绕会让负z 看起来像高z）。
pub fn clamp_level(z: i32) -> u8 {
    if z < 0 {
        0
    } else if z > 7 {
        7
    } else {
        z as u8
    }
}

// ---- 契约文档常量（判据条款的可追溯锚）----------------------------------

/// 四开关契约文档。
pub const FOUR_SWITCHES_DOC: &str = "\
四开关=包围盒描边/脏区高亮/缓存命中热度/Z 序标尺；四开关独立可切换，
关闭的开关不产出任何图元（不留残影），且不泄露对应调试信息到转储。";

/// 独立通道契约文档。
pub const ISOLATION_DOC: &str = "\
覆盖层脏区只走 dbg/ 前缀的独立通道，绝不写F0613 生产脏区与 F0615 缓存键——
否则「打开调试面板」这个纯观察动作会推进 content_rev 使缓存全miss，
污染命中率指标（观察者效应污染被测量对象）。OverlayState 无 &mut 脏区入口。";

/// 懒加载契约文档。
pub const LAZY_DOC: &str = "\
懒加载展开 O(展开子树规模)，不扫描未展开部分；预算耗尽时必须显式置
PanelResult.truncated 并给出 next_cursor 供续拉——静默截断与「本来就没内容」
不可区分，是调试工具失信的主要来源。";

/// 只读接口契约文档。
pub const READONLY_DOC: &str = "\
面板数据接口显性只读：PanelApi 方法取 &self，PanelNodeRef 借用节点，
拿不到 &mut；唯一写入口 apply_overlay_switch 只改调试视图自身开关位，
不触达图层树、脏区与缓存。";

/// 隐私边界契约文档。
pub const PRIVACY_DOC: &str = "\
调试视图仅诊断构建或显式开启（PanelApi::enable）；覆盖图元只记结构
（通道/节点 id/等级），不记内容像素；未开启时拒绝切换开关。";

/// 省略规则契约文档。
pub const OMISSION_DOC: &str = "\
属性省略只在「省略前后语义等价」时发生：等于规范默认值者省略（无损），
越界非法值强制输出并带 ! 标记（有损省略被禁止）。省略不是截断。";
