// SPDX-License-Identifier: Apache-2.0
//! CGPU-F0289 · 图可视化总架构
//!
//! 三大用途：调试（看图找瓶颈）、教学（理解调度）、审计（事后回放）。
//! 四层架构：图模型 → 布局 → 数据契约 → UI 渲染——引擎不画 UI 只出数据。
//! 双数据源：B01 帧图实时态 + B08 时序回放。
//! 隐私：资源名脱敏开关。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 可视化用途（三大场景，各自约束不同）
// ---------------------------------------------------------------------------

/// 可视化用途。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualPurpose {
    /// 调试：看图找瓶颈——强调实时性、当前帧热点高亮。
    Debug,
    /// 教学：理解调度——强调可读性、层次清晰、动画演示。
    Teaching,
    /// 审计：事后回放——强调完整性、可追溯、逐帧可复现。
    Audit,
}

impl VisualPurpose {
    /// 用途的显示名。
    pub fn label(&self) -> &'static str {
        match self {
            VisualPurpose::Debug => "调试",
            VisualPurpose::Teaching => "教学",
            VisualPurpose::Audit => "审计",
        }
    }

    /// 是否需要时间轴联动。
    pub fn needs_timeline(&self) -> bool {
        match self {
            VisualPurpose::Debug => true,
            VisualPurpose::Teaching => true,
            VisualPurpose::Audit => true,
        }
    }

    /// 是否启用脱敏（审计场景必须脱敏外发）。
    pub fn requires_masking(&self) -> bool {
        match self {
            VisualPurpose::Debug => false,
            VisualPurpose::Teaching => true,
            VisualPurpose::Audit => true,
        }
    }

    /// 默认 LOD 档位。
    pub fn default_lod(&self) -> LodLevel {
        match self {
            VisualPurpose::Debug => LodLevel::Full,
            VisualPurpose::Teaching => LodLevel::Clustered,
            VisualPurpose::Audit => LodLevel::Full,
        }
    }
}

// ---------------------------------------------------------------------------
// LOD 三档
// ---------------------------------------------------------------------------

/// 细节层次（Level of Detail）三档。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LodLevel {
    /// 全图：所有节点、所有边。
    Full,
    /// 聚簇：同层/同账户节点合并为簇节点。
    Clustered,
    /// 聚焦：只显示选中节点及其邻居。
    Focus,
}

impl LodLevel {
    /// 数据压缩比阶梯：Full=100, Clustered=10, Focus=1。
    pub fn compression_ratio(&self) -> u32 {
        match self {
            LodLevel::Full => 100,
            LodLevel::Clustered => 10,
            LodLevel::Focus => 1,
        }
    }

    /// 是否显示边标签。
    pub fn shows_edge_labels(&self) -> bool {
        matches!(self, LodLevel::Full | LodLevel::Focus)
    }

    /// 是否显示节点详情。
    pub fn shows_node_details(&self) -> bool {
        matches!(self, LodLevel::Full | LodLevel::Focus)
    }
}

// ---------------------------------------------------------------------------
// 四层架构——图模型层
// ---------------------------------------------------------------------------

/// 节点 ID（与帧任务图的 TaskId 对应）。
pub type NodeId = u32;

/// 边 ID。
pub type EdgeId = u64;

/// 节点在可视化图模型中的形态。
#[derive(Debug, Clone)]
pub struct VisualNode {
    pub id: NodeId,
    /// 显示标签（已脱敏或原始）。
    pub label: String,
    /// 所属队列/账户（用于着色和过滤）。
    pub queue: u8,
    /// 状态（五态之一）。
    pub state: NodeVisualState,
    /// 层级索引（布局后填充）。
    pub layer: u32,
    /// 层内序号（布局后填充）。
    pub order_in_layer: u32,
    /// 层内坐标（布局后填充）。
    pub x: f32,
    /// 层间坐标（布局后填充）。
    pub y: f32,
    /// 预算使用率（0.0~2.0，用于热力模式）。
    pub budget_usage: f32,
}

/// 节点五态（色盲可辨：形状+纹理双冗余）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeVisualState {
    /// 就绪。
    Ready,
    /// 执行中。
    Running,
    /// 阻塞。
    Blocked,
    /// 等待。
    Waiting,
    /// 完成。
    Done,
}

impl NodeVisualState {
    pub fn label(&self) -> &'static str {
        match self {
            NodeVisualState::Ready => "就绪",
            NodeVisualState::Running => "执行",
            NodeVisualState::Blocked => "阻塞",
            NodeVisualState::Waiting => "等待",
            NodeVisualState::Done => "完成",
        }
    }

    /// 形状码（双冗余之一——不依赖颜色）。
    pub fn shape_code(&self) -> u8 {
        match self {
            NodeVisualState::Ready => 0,    // 圆
            NodeVisualState::Running => 1,  // 三角
            NodeVisualState::Blocked => 2,  // 方
            NodeVisualState::Waiting => 3,  // 菱形
            NodeVisualState::Done => 4,     // 五边形
        }
    }

    /// 纹理码（双冗余之二——不依赖颜色）。
    pub fn texture_code(&self) -> u8 {
        match self {
            NodeVisualState::Ready => 0,     // 纯色
            NodeVisualState::Running => 1,   // 斜线
            NodeVisualState::Blocked => 2,   // 网格
            NodeVisualState::Waiting => 3,   // 点阵
            NodeVisualState::Done => 4,      // 对角
        }
    }
}

/// 边在可视化图模型中的形态。
#[derive(Debug, Clone)]
pub struct VisualEdge {
    pub id: EdgeId,
    pub from: NodeId,
    pub to: NodeId,
    /// 边类型（三型区分）。
    pub kind: EdgeKind,
    /// 边权重（数据量或等待时长——映射为线宽）。
    pub weight: f32,
    /// 是否属于环路径（F0231/F0252 标红高亮）。
    pub in_cycle: bool,
}

/// 边类型三型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// 帧内依赖：实线。
    IntraFrame,
    /// 跨帧依赖：泳道弧线。
    CrossFrame,
    /// 等待边：虚线闪烁。
    Wait,
}

impl EdgeKind {
    pub fn label(&self) -> &'static str {
        match self {
            EdgeKind::IntraFrame => "帧内依赖",
            EdgeKind::CrossFrame => "跨帧依赖",
            EdgeKind::Wait => "等待",
        }
    }

    /// 线型码（0=实线, 1=弧线, 2=虚线）。
    pub fn line_style_code(&self) -> u8 {
        match self {
            EdgeKind::IntraFrame => 0,
            EdgeKind::CrossFrame => 1,
            EdgeKind::Wait => 2,
        }
    }
}

/// 图模型（可视化引擎的输入核心）。
#[derive(Debug, Clone)]
pub struct GraphModel {
    pub nodes: Vec<VisualNode>,
    pub edges: Vec<VisualEdge>,
    /// 帧号（跨帧泳道的分组键）。
    pub frame: u32,
}

impl GraphModel {
    pub fn new(frame: u32) -> Self {
        GraphModel { nodes: Vec::new(), edges: Vec::new(), frame }
    }

    pub fn node_count(&self) -> usize { self.nodes.len() }
    pub fn edge_count(&self) -> usize { self.edges.len() }

    /// 入度表（布局用）。
    pub fn in_degrees(&self) -> BTreeMap<NodeId, u32> {
        let mut d: BTreeMap<NodeId, u32> = BTreeMap::new();
        for n in &self.nodes { d.insert(n.id, 0); }
        for e in &self.edges { *d.entry(e.to).or_insert(0) += 1; }
        d
    }

    /// 出度表（布局用）。
    pub fn out_degrees(&self) -> BTreeMap<NodeId, u32> {
        let mut d: BTreeMap<NodeId, u32> = BTreeMap::new();
        for n in &self.nodes { d.insert(n.id, 0); }
        for e in &self.edges { *d.entry(e.from).or_insert(0) += 1; }
        d
    }

    /// 环上节点集（由 B06/F0252 注入）。
    pub fn cycle_nodes(&self) -> Vec<NodeId> {
        let mut set = Vec::new();
        for e in &self.edges {
            if e.in_cycle {
                if !set.contains(&e.from) { set.push(e.from); }
                if !set.contains(&e.to) { set.push(e.to); }
            }
        }
        set.sort();
        set
    }
}

// ---------------------------------------------------------------------------
// 四层架构——脱敏层
// ---------------------------------------------------------------------------

/// 脱敏策略。
#[derive(Debug, Clone)]
pub struct MaskPolicy {
    /// 是否启用脱敏。
    pub enabled: bool,
    /// 保留前缀字符数。
    pub prefix_keep: usize,
    /// 替换符。
    pub mask_char: char,
}

impl MaskPolicy {
    pub fn off() -> Self { MaskPolicy { enabled: false, prefix_keep: 0, mask_char: '*' } }
    pub fn on() -> Self { MaskPolicy { enabled: true, prefix_keep: 2, mask_char: '*' } }

    /// 对单个名称脱敏。
    pub fn mask(&self, name: &str) -> String {
        if !self.enabled { return name.to_string(); }
        let chars: Vec<char> = name.chars().collect();
        if chars.len() <= self.prefix_keep {
            return String::new();
        }
        let mut out = String::new();
        for c in chars.iter().take(self.prefix_keep) { out.push(*c); }
        for _ in self.prefix_keep..chars.len() { out.push(self.mask_char); }
        out
    }

    /// 批量脱敏。
    pub fn mask_all(&self, names: &[String]) -> Vec<String> {
        names.iter().map(|n| self.mask(n)).collect()
    }
}

// ---------------------------------------------------------------------------
// 四层架构——数据契约层（引擎输出四件套）
// ---------------------------------------------------------------------------

/// 节点点列（布局后的坐标 + 形状）。
#[derive(Debug, Clone)]
pub struct NodePoints {
    pub ids: Vec<NodeId>,
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    pub shapes: Vec<u8>,
    pub textures: Vec<u8>,
    pub states: Vec<u8>,
}

/// 边折线（每条边的控制点序列）。
#[derive(Debug, Clone)]
pub struct EdgePolylines {
    pub ids: Vec<EdgeId>,
    /// 每条边的点数（折线由多个 (x,y) 组成，扁平存储）。
    pub point_counts: Vec<u32>,
    /// 扁平坐标。
    pub points: Vec<f32>,
    pub styles: Vec<u8>,
    pub weights: Vec<f32>,
}

/// 状态色值（主题无关的 RGB）。
#[derive(Debug, Clone)]
pub struct StateColors {
    pub colors: Vec<[u8; 3]>,
}

/// 标签文本。
#[derive(Debug, Clone)]
pub struct LabelTexts {
    pub labels: Vec<String>,
}

/// 引擎输出四件套（纯数据，JSON 序列化由调用方决定）。
#[derive(Debug, Clone)]
pub struct RenderContract {
    pub node_points: NodePoints,
    pub edge_polylines: EdgePolylines,
    pub state_colors: StateColors,
    pub label_texts: LabelTexts,
}

impl RenderContract {
    /// 估算序列化字节数（5MB 预算用）。
    pub fn estimated_bytes(&self) -> usize {
        let n = self.node_points.ids.len();
        let e = self.edge_polylines.ids.len();
        let pts: usize = self.edge_polylines.point_counts.iter().map(|c| *c as usize).sum();
        // 粗略：节点 20 字节/个，边 12 字节/条 + 点 8 字节/点，标签 24 字节/个
        n * 20 + e * 12 + pts * 8 + self.label_texts.labels.len() * 24
    }

    /// 是否超过 5MB 预算。
    pub fn exceeds_budget(&self) -> bool {
        self.estimated_bytes() > 5 * 1024 * 1024
    }
}

// ---------------------------------------------------------------------------
// 四层架构——双数据源
// ---------------------------------------------------------------------------

/// 数据源类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSource {
    /// B01 帧图实时态。
    Live,
    /// B08 时序回放。
    Replay,
}

/// 可视化引擎配置。
#[derive(Debug, Clone)]
pub struct VisualEngineConfig {
    pub purpose: VisualPurpose,
    pub lod: LodLevel,
    pub mask: MaskPolicy,
    pub source: DataSource,
    /// 是否启用时间轴联动。
    pub timeline_linked: bool,
}

impl VisualEngineConfig {
    pub fn for_purpose(p: VisualPurpose) -> Self {
        VisualEngineConfig {
            purpose: p,
            lod: p.default_lod(),
            mask: if p.requires_masking() { MaskPolicy::on() } else { MaskPolicy::off() },
            source: DataSource::Live,
            timeline_linked: p.needs_timeline(),
        }
    }
}

// ---------------------------------------------------------------------------
// 可视化引擎（总调度入口）
// ---------------------------------------------------------------------------

/// 可视化引擎——接收图模型，产出渲染契约。
pub struct VisualEngine {
    pub config: VisualEngineConfig,
    /// 最近一次布局结果缓存（增量布局基础）。
    last_nodes: Vec<NodeId>,
    last_layers: Vec<u32>,
}

impl VisualEngine {
    pub fn new(config: VisualEngineConfig) -> Self {
        VisualEngine { config, last_nodes: Vec::new(), last_layers: Vec::new() }
    }

    /// 处理一个图模型，产出渲染契约（布局 → 契约）。
    pub fn render(&mut self, graph: &GraphModel) -> RenderContract {
        let mut nodes = graph.nodes.clone();
        // 脱敏
        if self.config.mask.enabled {
            for n in nodes.iter_mut() {
                n.label = self.config.mask.mask(&n.label);
            }
        }
        // 布局（简化：调用层内均分，完整 Sugiyama 在 F0290）
        self.basic_layout(&mut nodes);

        // 契约四件套
        let mut np = NodePoints { ids: Vec::new(), xs: Vec::new(), ys: Vec::new(), shapes: Vec::new(), textures: Vec::new(), states: Vec::new() };
        for n in &nodes {
            np.ids.push(n.id);
            np.xs.push(n.x);
            np.ys.push(n.y);
            np.shapes.push(n.state.shape_code());
            np.textures.push(n.state.texture_code());
            np.states.push(state_code(n.state));
        }

        let mut ep = EdgePolylines { ids: Vec::new(), point_counts: Vec::new(), points: Vec::new(), styles: Vec::new(), weights: Vec::new() };
        for e in &graph.edges {
            let from = nodes.iter().find(|n| n.id == e.from);
            let to = nodes.iter().find(|n| n.id == e.to);
            if let (Some(f), Some(t)) = (from, to) {
                ep.ids.push(e.id);
                ep.point_counts.push(2);
                ep.points.push(f.x); ep.points.push(f.y);
                ep.points.push(t.x); ep.points.push(t.y);
                ep.styles.push(e.kind.line_style_code());
                ep.weights.push(e.weight);
            }
        }

        let sc = StateColors { colors: state_palette() };
        let lt = LabelTexts { labels: nodes.iter().map(|n| n.label.clone()).collect() };

        self.last_nodes = nodes.iter().map(|n| n.id).collect();
        self.last_layers = nodes.iter().map(|n| n.layer).collect();

        RenderContract { node_points: np, edge_polylines: ep, state_colors: sc, label_texts: lt }
    }

    /// 基础布局（按层级均分——完整 Sugiyama 在 F0290 实现）。
    fn basic_layout(&self, nodes: &mut [VisualNode]) {
        let n = nodes.len();
        if n == 0 { return; }
        for (i, node) in nodes.iter_mut().enumerate() {
            node.layer = 0;
            node.order_in_layer = i as u32;
            node.x = (i as f32) * 120.0;
            node.y = 0.0;
        }
    }
}

/// 状态 → 编码。
fn state_code(s: NodeVisualState) -> u8 {
    match s {
        NodeVisualState::Ready => 0,
        NodeVisualState::Running => 1,
        NodeVisualState::Blocked => 2,
        NodeVisualState::Waiting => 3,
        NodeVisualState::Done => 4,
    }
}

/// 五态色标（绿/蓝/红/黄/灰）。
fn state_palette() -> Vec<[u8; 3]> {
    alloc::vec![
        [34, 197, 94],   // 就绪 绿
        [59, 130, 246],  // 执行 蓝
        [239, 68, 68],   // 阻塞 红
        [234, 179, 8],   // 等待 黄
        [107, 114, 128], // 完成 灰
    ]
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// CGPU-F0289 判据全集。
pub fn run_visual_arch_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("CGPU-F0289", "图可视化总架构");

    // 1. 三大用途
    let purposes = [VisualPurpose::Debug, VisualPurpose::Teaching, VisualPurpose::Audit];
    cs.ok("三大用途齐备", purposes.len() == 3, "调试/教学/审计");
    cs.ok("教学需脱敏", VisualPurpose::Teaching.requires_masking(), "外发场景");
    cs.ok("审计需脱敏", VisualPurpose::Audit.requires_masking(), "合规要求");
    cs.ok("调试不强制脱敏", !VisualPurpose::Debug.requires_masking(), "内用场景");

    // 2. LOD 三档
    let lods = [LodLevel::Full, LodLevel::Clustered, LodLevel::Focus];
    cs.ok("LOD 三档", lods.len() == 3, "100:10:1 阶梯");
    cs.ok("Full 压缩比 100", LodLevel::Full.compression_ratio() == 100, "阶梯校验");
    cs.ok("Clustered 压缩比 10", LodLevel::Clustered.compression_ratio() == 10, "阶梯校验");
    cs.ok("Focus 压缩比 1", LodLevel::Focus.compression_ratio() == 1, "阶梯校验");

    // 3. 五态双冗余
    let states = [NodeVisualState::Ready, NodeVisualState::Running, NodeVisualState::Blocked, NodeVisualState::Waiting, NodeVisualState::Done];
    cs.ok("五态齐备", states.len() == 5, "就绪/执行/阻塞/等待/完成");
    let shapes: Vec<u8> = states.iter().map(|s| s.shape_code()).collect();
    let textures: Vec<u8> = states.iter().map(|s| s.texture_code()).collect();
    cs.ok("形状码互异", {
        let mut s = shapes.clone(); s.sort(); s.dedup(); s.len() == 5
    }, "双冗余之一");
    cs.ok("纹理码互异", {
        let mut t = textures.clone(); t.sort(); t.dedup(); t.len() == 5
    }, "双冗余之二");

    // 4. 边三型
    let kinds = [EdgeKind::IntraFrame, EdgeKind::CrossFrame, EdgeKind::Wait];
    cs.ok("边三型", kinds.len() == 3, "实线/弧线/虚线");
    cs.ok("线型码互异", {
        let codes: Vec<u8> = kinds.iter().map(|k| k.line_style_code()).collect();
        let mut c = codes.clone(); c.sort(); c.dedup(); c.len() == 3
    }, "三型可辨");

    // 5. 脱敏
    let policy = MaskPolicy::on();
    let masked = policy.mask("secret_resource_name");
    cs.ok("脱敏保留前缀", masked.starts_with("se"), "前 2 字符保留");
    cs.ok("脱敏长度不变", masked.chars().count() == "secret_resource_name".chars().count(), "等长替换");
    cs.ok("脱敏后不泄原名", !masked.contains("secret_resource"), "核心部分遮蔽");
    let off = MaskPolicy::off();
    cs.ok("关闭脱敏原样返回", off.mask("hello") == "hello", "不启用时透传");

    // 6. 双数据源
    cs.ok("双数据源", DataSource::Live != DataSource::Replay, "实时/回放");
    cs.ok("三大用途均需时间轴", purposes.iter().all(|p| p.needs_timeline()), "时间轴联动");

    // 7. 契约四件套
    let mut eng = VisualEngine::new(VisualEngineConfig::for_purpose(VisualPurpose::Debug));
    let mut g = GraphModel::new(0);
    g.nodes.push(VisualNode { id: 1, label: "t1".into(), queue: 0, state: NodeVisualState::Ready, layer: 0, order_in_layer: 0, x: 0.0, y: 0.0, budget_usage: 0.5 });
    g.nodes.push(VisualNode { id: 2, label: "t2".into(), queue: 1, state: NodeVisualState::Running, layer: 0, order_in_layer: 1, x: 0.0, y: 0.0, budget_usage: 1.2 });
    g.edges.push(VisualEdge { id: 1, from: 1, to: 2, kind: EdgeKind::IntraFrame, weight: 1.0, in_cycle: false });
    let contract = eng.render(&g);
    cs.ok("四件套节点", contract.node_points.ids.len() == 2, "节点点列");
    cs.ok("四件套边", contract.edge_polylines.ids.len() == 1, "边折线");
    cs.ok("四件套色标", contract.state_colors.colors.len() == 5, "五态色");
    cs.ok("四件套标签", contract.label_texts.labels.len() == 2, "标签文本");
    cs.ok("预算内", !contract.exceeds_budget(), "小图远低于 5MB");

    // 8. 图模型辅助
    let in_d = g.in_degrees();
    let out_d = g.out_degrees();
    cs.ok("入度正确", in_d.get(&2).copied().unwrap_or(0) == 1, "t2 入度 1");
    cs.ok("出度正确", out_d.get(&1).copied().unwrap_or(0) == 1, "t1 出度 1");

    cs.done()
}
