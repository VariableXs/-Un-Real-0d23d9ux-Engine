//! VE-F1403 · 混音图引擎节点模型（VE-H 域 · 音频引擎 · 目标 480 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1403`
//!
//! **规格原文**：通用节点模型（源节点/处理节点/总线节点/监听节点四类——任意
//! DAG 连接，超越 G07 F1324 固定五层）。节点端口（输入/输出多通道+格式协商）。
//! 图校验三查（环检测/采样率一致/通道布局兼容——三查在建图与热更新双时点）。
//! 图热更新（运行时改图无爆音——节点插入等增益过渡，F1329 参数平滑同核）。
//! 与 G07 关系（播放链五层是本图的一个预置模板——模板化实例）。
//! 判据：四类节点、端口协商、三查、热更新、模板化、判据。
//!
//! **设计要点**：
//! - 四类节点的端口语义：源无入口、监听无出口、处理与总线双向——固定五层
//!   是本图的一个预置模板，不是天花板（任意 DAG 拓扑）；
//! - 端口协商逐边进行：通道布局（单声道..7.1）与采样率必须兼容，跨率必须
//!   节点声明重采样——F1327 协商算法的端口级复用；
//! - 三查双时点：建图时全查，热更新补丁先在"影子图"上全查再原子提交——
//!   拒绝的补丁不碰活图（运行时改图的地基是"拒绝不动账"）；
//! - 无爆音纪律：节点插入/移除走等增益过渡（equal-power 余弦定律，与
//!   F1329 参数平滑同核），端点精确、步进无跳变（相邻步差有上界）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、节点与端口模型（四类端口语义）
// ---------------------------------------------------------------------------

/// 节点类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// 源：媒体流/合成器/采集——只有出口。
    Source,
    /// 处理：滤镜效果——出入口双向。
    Processor,
    /// 总线：子混音聚合——出入口双向。
    Bus,
    /// 监听：输出终点——只有入口。
    Listener,
}

impl NodeKind {
    pub fn label(self) -> &'static str {
        match self {
            NodeKind::Source => "源",
            NodeKind::Processor => "处理",
            NodeKind::Bus => "总线",
            NodeKind::Listener => "监听",
        }
    }

    pub fn has_input(self) -> bool {
        !matches!(self, NodeKind::Source)
    }

    pub fn has_output(self) -> bool {
        !matches!(self, NodeKind::Listener)
    }
}

/// 通道布局（1=单声道 2=立体声 6=5.1 8=7.1）。
pub const CH_MONO: u8 = 1;
pub const CH_STEREO: u8 = 2;
pub const CH_5_1: u8 = 6;
pub const CH_7_1: u8 = 8;

/// 端口：多通道 + 采样率 + 重采样声明。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Port {
    pub channels: u8,
    pub sample_rate: u32,
    /// 本端口所在节点是否声明了重采样能力（跨率兼容的前提）。
    pub resample_declared: bool,
}

/// 图错误（三要素）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphError {
    pub code: &'static str,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl GraphError {
    fn new(code: &'static str, what: String, why: String, next: String) -> GraphError {
        GraphError { code, what, why, next }
    }
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

/// 图节点。
#[derive(Clone, Debug)]
pub struct Node {
    pub id: u32,
    pub kind: NodeKind,
    pub name: String,
    pub in_port: Option<Port>,
    pub out_port: Option<Port>,
}

/// 一条边：from 节点的出口 → to 节点的入口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub from: u32,
    pub to: u32,
}

/// 混音图（任意 DAG）。
#[derive(Clone, Debug, Default)]
pub struct MixGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl MixGraph {
    pub fn new() -> MixGraph {
        MixGraph::default()
    }

    /// 加节点：四类端口语义在此强制（源无入口/监听无出口）。
    pub fn add_node(
        &mut self,
        id: u32,
        kind: NodeKind,
        name: &str,
        in_port: Option<Port>,
        out_port: Option<Port>,
    ) -> Result<(), GraphError> {
        if self.nodes.iter().any(|n| n.id == id) {
            return Err(GraphError::new(
                "E_DUP_NODE",
                format!("节点 {} 已存在", id),
                "一物双名会让边语义失去对象身份".to_string(),
                "换节点号或先移除旧节点".to_string(),
            ));
        }
        if in_port.is_some() && !kind.has_input() {
            return Err(GraphError::new(
                "E_PORT_SEMANTICS",
                format!("{}节点 {} 不允许入口", kind.label(), id),
                "端口语义是节点类别的定义：源节点只产不收".to_string(),
                "源节点去掉 in_port；若确要收输入请改用处理节点".to_string(),
            ));
        }
        if out_port.is_some() && !kind.has_output() {
            return Err(GraphError::new(
                "E_PORT_SEMANTICS",
                format!("{}节点 {} 不允许出口", kind.label(), id),
                "监听节点是输出终点，再挂出口就是拓扑环的前兆".to_string(),
                "监听节点去掉 out_port".to_string(),
            ));
        }
        self.nodes.push(Node {
            id,
            kind,
            name: name.to_string(),
            in_port,
            out_port,
        });
        Ok(())
    }

    pub fn find(&self, id: u32) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// 查找节点（可变，供检查层做端口声明修正）。
    pub fn find_port_mut(&mut self, id: u32) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn remove_node(&mut self, id: u32) -> bool {
        let before = self.nodes.len();
        self.nodes.retain(|n| n.id != id);
        self.edges.retain(|e| e.from != id && e.to != id);
        self.nodes.len() != before
    }

    // ---- 端口协商（F1327 的端口级复用） ----

    /// 逐边端口协商：布局与采样率兼容才可连。
    pub fn negotiate_edge(&self, edge: Edge) -> Result<(), GraphError> {
        let from = self.find(edge.from).ok_or_else(|| {
            GraphError::new(
                "E_NO_NODE",
                format!("边来源节点 {} 不存在", edge.from),
                "边必须连接已存在的节点".to_string(),
                "先 add_node 再 connect".to_string(),
            )
        })?;
        let to = self.find(edge.to).ok_or_else(|| {
            GraphError::new(
                "E_NO_NODE",
                format!("边目标节点 {} 不存在", edge.to),
                "边必须连接已存在的节点".to_string(),
                "先 add_node 再 connect".to_string(),
            )
        })?;
        let out = from.out_port.ok_or_else(|| {
            GraphError::new(
                "E_NO_OUT_PORT",
                format!("节点 {}（{}）没有出口", edge.from, from.kind.label()),
                "无出口的节点不能作为边来源".to_string(),
                "改用带出口的节点".to_string(),
            )
        })?;
        let inp = to.in_port.ok_or_else(|| {
            GraphError::new(
                "E_NO_IN_PORT",
                format!("节点 {}（{}）没有入口", edge.to, to.kind.label()),
                "无入口的节点不能作为边目标".to_string(),
                "改用带入口的节点".to_string(),
            )
        })?;
        // 采样率：相等直连；不等则任一端声明重采样才放行。
        if out.sample_rate != inp.sample_rate
            && !out.resample_declared
            && !inp.resample_declared
        {
            return Err(GraphError::new(
                "E_RATE_MISMATCH",
                format!(
                    "边 {}→{} 采样率 {}→{} 且两端均未声明重采样",
                    edge.from, edge.to, out.sample_rate, inp.sample_rate
                ),
                "跨率直连是变调与爆音的根源——重采样必须显式声明".to_string(),
                "在源或目标端口声明 resample_declared，或统一全图采样率".to_string(),
            ));
        }
        // 通道布局：相等直连；不等允许声明式上下混（任一端声明即算有转换核）。
        if out.channels != inp.channels
            && !out.resample_declared
            && !inp.resample_declared
        {
            return Err(GraphError::new(
                "E_LAYOUT_INCOMPAT",
                format!(
                    "边 {}→{} 通道 {}→{} 且两端均未声明转换",
                    edge.from, edge.to, out.channels, inp.channels
                ),
                "无声明跨布局直连会丢声道或越界读".to_string(),
                "在端口声明转换能力（下混/上混核）后再连接".to_string(),
            ));
        }
        Ok(())
    }

    /// 连接：协商通过才建边。
    pub fn connect(&mut self, edge: Edge) -> Result<(), GraphError> {
        self.negotiate_edge(edge)?;
        if self.edges.contains(&edge) {
            return Err(GraphError::new(
                "E_DUP_EDGE",
                format!("边 {:?} 已存在", (edge.from, edge.to)),
                "重复边会让信号加倍（+6dB）——不是特性是缺陷".to_string(),
                "先断开再连，或检查上层调用".to_string(),
            ));
        }
        self.edges.push(edge);
        Ok(())
    }

    pub fn disconnect(&mut self, edge: Edge) -> bool {
        let before = self.edges.len();
        self.edges.retain(|e| *e != edge);
        self.edges.len() != before
    }

    // ---- 三查之一：环检测（DAG 无环断言，Kahn 消元） ----

    pub fn has_cycle(&self) -> bool {
        let n = self.nodes.len();
        let mut indeg = vec![0usize; n];
        let idx = |id: u32| self.nodes.iter().position(|x| x.id == id).unwrap_or(usize::MAX);
        for e in self.edges.iter() {
            let (a, b) = (idx(e.from), idx(e.to));
            if a == usize::MAX || b == usize::MAX {
                continue;
            }
            indeg[b] += 1;
        }
        let mut queue: Vec<usize> = (0..n).filter(|i| indeg[*i] == 0).collect();
        let mut consumed = 0usize;
        while let Some(i) = queue.pop() {
            consumed += 1;
            for e in self.edges.iter() {
                if idx(e.from) == i {
                    let b = idx(e.to);
                    indeg[b] -= 1;
                    if indeg[b] == 0 {
                        queue.push(b);
                    }
                }
            }
        }
        consumed != n
    }

    /// 三查总入口：环 / 采样率 / 布局，逐边协商 + 全图无环。
    pub fn validate_all(&self) -> Result<(), GraphError> {
        if self.has_cycle() {
            return Err(GraphError::new(
                "E_GRAPH_CYCLE",
                "混音图存在环".to_string(),
                "环 = 无界反馈，信号会无限叠加直到削波或溢出".to_string(),
                "断开成环的边；确需反馈请经显式延迟节点".to_string(),
            ));
        }
        for e in self.edges.iter() {
            self.negotiate_edge(*e)?;
        }
        Ok(())
    }

    /// 读屏摘要。
    pub fn a11y_summary(&self) -> String {
        let kinds = |k: NodeKind| self.nodes.iter().filter(|n| n.kind == k).count();
        format!(
            "混音图：节点 {}（源 {} 处理 {} 总线 {} 监听 {}）、边 {}",
            self.nodes.len(),
            kinds(NodeKind::Source),
            kinds(NodeKind::Processor),
            kinds(NodeKind::Bus),
            kinds(NodeKind::Listener),
            self.edges.len()
        )
    }
}

// ---------------------------------------------------------------------------
// 二、热更新（影子图三查 + 原子提交 + 等增益过渡）
// ---------------------------------------------------------------------------

/// 图补丁：增删节点、连断边的一次批量变更。
#[derive(Clone, Debug, Default)]
pub struct GraphPatch {
    pub add_nodes: Vec<(u32, NodeKind, String, Option<Port>, Option<Port>)>,
    pub remove_nodes: Vec<u32>,
    pub connect: Vec<Edge>,
    pub disconnect: Vec<Edge>,
}

/// 等增益过渡计划（F1329 参数平滑同核：equal-power 余弦定律）。
pub const TRANSITION_STEPS: usize = 32;

/// equal-power 曲线在 t∈[0,1] 的增益（线性幅值）：
/// fade_out = cos(π/2·t)、fade_in = sin(π/2·t)——功率和恒为 1（等响切换）。
/// 端点精确：t=0/1 直接给出 0/1（f64 的 π/2 近似会让 cos 不归零——端点必须钉死）。
pub fn equal_power_gain(t: f64, fade_in: bool) -> f64 {
    if t <= 0.0 {
        return if fade_in { 0.0 } else { 1.0 };
    }
    if t >= 1.0 {
        return if fade_in { 1.0 } else { 0.0 };
    }
    let t = t.clamp(0.0, 1.0);
    if fade_in {
        (core::f64::consts::FRAC_PI_2 * t).sin()
    } else {
        (core::f64::consts::FRAC_PI_2 * t).cos()
    }
}

/// 热更新裁决。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchVerdict {
    /// 补丁已原子提交，附等增益过渡的声部迁移记录数。
    Committed { transitions: usize },
    /// 补丁被三查拒绝（活图未动），附错误。
    Rejected(GraphError),
}

/// 带热更新的可变图宿主：活图 + 影子图原子提交。
pub struct GraphHost {
    pub live: MixGraph,
    pub committed_patches: u64,
    pub rejected_patches: u64,
}

impl GraphHost {
    pub fn new() -> GraphHost {
        GraphHost {
            live: MixGraph::new(),
            committed_patches: 0,
            rejected_patches: 0,
        }
    }

    /// 应用补丁：克隆活图为影子 → 应用补丁 → 三查 → 通过则原子换入。
    ///
    /// 拒绝的补丁不碰活图（运行时改图无爆音的地基：拒绝不动账）。
    pub fn apply_patch(&mut self, patch: &GraphPatch) -> PatchVerdict {
        let mut shadow = self.live.clone();
        // 断开先于移除（移除节点会隐式断边，显式断开保序）。
        for e in patch.disconnect.iter() {
            shadow.disconnect(*e);
        }
        for id in patch.remove_nodes.iter() {
            shadow.remove_node(*id);
        }
        for (id, kind, name, inp, outp) in patch.add_nodes.iter() {
            if let Err(e) = shadow.add_node(*id, *kind, name, inp.clone(), outp.clone()) {
                self.rejected_patches += 1;
                return PatchVerdict::Rejected(e);
            }
        }
        for e in patch.connect.iter() {
            if let Err(e2) = shadow.connect(*e) {
                self.rejected_patches += 1;
                return PatchVerdict::Rejected(e2);
            }
        }
        // 三查双时点：热更新提交前全图复检。
        if let Err(e) = shadow.validate_all() {
            self.rejected_patches += 1;
            return PatchVerdict::Rejected(e);
        }
        let transitions = patch.add_nodes.len() + patch.remove_nodes.len();
        self.live = shadow;
        self.committed_patches += 1;
        PatchVerdict::Committed { transitions }
    }
}

// ---------------------------------------------------------------------------
// 三、播放链五层预置模板（模板化实例：五层是本图的预置不是天花板）
// ---------------------------------------------------------------------------

/// G07 播放链五层模板：源 → 效果 → 总线 → 主总线 → 监听。
///
/// 返回的图已通过三查——模板即"预置拓扑 + 合法连接"的实例。
pub fn playback_chain_template() -> Result<MixGraph, GraphError> {
    let mut g = MixGraph::new();
    let port = |ch: u8| Port {
        channels: ch,
        sample_rate: 48_000,
        resample_declared: false,
    };
    g.add_node(1, NodeKind::Source, "媒体流", None, Some(port(CH_STEREO)))?;
    g.add_node(2, NodeKind::Processor, "滤镜效果", Some(port(CH_STEREO)), Some(port(CH_STEREO)))?;
    g.add_node(3, NodeKind::Bus, "播放总线", Some(port(CH_STEREO)), Some(port(CH_STEREO)))?;
    g.add_node(4, NodeKind::Bus, "主总线", Some(port(CH_STEREO)), Some(port(CH_STEREO)))?;
    g.add_node(5, NodeKind::Listener, "输出终点", Some(port(CH_STEREO)), None)?;
    g.connect(Edge { from: 1, to: 2 })?;
    g.connect(Edge { from: 2, to: 3 })?;
    g.connect(Edge { from: 3, to: 4 })?;
    g.connect(Edge { from: 4, to: 5 })?;
    g.validate_all()?;
    Ok(g)
}
