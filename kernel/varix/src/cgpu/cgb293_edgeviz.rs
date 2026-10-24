// cgb293_edgeviz.rs ↔ CGPU-F0293 依赖边可视化
//
// 判据原文：CGPU-F0293-J1 三型、权重映射、环提示、过滤、判据。
// 难点分析：三型边（帧内实线/跨帧弧线/等待虚线闪烁）的视觉区分要与 F0292 双冗余
//          纪律对齐；边过滤是大图降噪的关键——按类型/账户过滤。
// 纪律约束：三型区分；权重→线宽映射；环检测提示；过滤降噪。
// 成本记账：O(edges) 着色；过滤 O(edges)。

#![allow(dead_code)]

use alloc::vec::Vec;

pub type NodeId = u32;
pub type EdgeId = u32;

/// 边类型（三型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// 帧内依赖（实线）。
    IntraFrame,
    /// 跨帧依赖（泳道弧线）。
    CrossFrame,
    /// 等待边（虚线闪烁）。
    Waiting,
}

impl EdgeKind {
    /// 线型码（0=实 1=弧 2=虚）。
    pub fn stroke(self) -> u8 {
        match self {
            EdgeKind::IntraFrame => 0,
            EdgeKind::CrossFrame => 1,
            EdgeKind::Waiting => 2,
        }
    }
    /// 是否闪烁。
    pub fn animated(self) -> bool {
        matches!(self, EdgeKind::Waiting)
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            EdgeKind::IntraFrame => "帧内依赖",
            EdgeKind::CrossFrame => "跨帧依赖",
            EdgeKind::Waiting => "等待边",
        }
    }
}

/// 边定义。
#[derive(Debug, Clone)]
pub struct Edge {
    pub id: EdgeId,
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    /// 权重（数据量或等待时长 → 线宽）。
    pub weight: f32,
    /// 所属账户（过滤用）。
    pub account: u32,
    /// 是否在环上（F0231 静态环警告）。
    pub in_cycle: bool,
}

/// 边渲染样式。
#[derive(Debug, Clone, Copy)]
pub struct EdgeStyle {
    pub edge: EdgeId,
    /// 线宽（1.0~5.0，权重映射）。
    pub width: f32,
    /// 线型码。
    pub stroke: u8,
    /// 是否闪烁。
    pub animated: bool,
    /// 是否环警告（红色加粗）。
    pub cycle_warn: bool,
    /// RGB 色。
    pub rgb: u32,
}

/// 过滤条件。
#[derive(Debug, Clone, Copy, Default)]
pub struct EdgeFilter {
    /// 允许的边型（None=全允许）。
    pub kind: Option<EdgeKind>,
    /// 允许的账户（None=全允许）。
    pub account: Option<u32>,
    /// 最小权重（降噪）。
    pub min_weight: f32,
}

/// 边可视化引擎。
pub struct EdgeViz {
    edges: Vec<Edge>,
    filter: EdgeFilter,
    /// 权重→线宽映射的最大权重（归一化用）。
    max_weight: f32,
}

impl EdgeViz {
    /// 新建。
    pub fn new() -> Self {
        EdgeViz {
            edges: Vec::new(),
            filter: EdgeFilter::default(),
            max_weight: 1.0,
        }
    }

    /// 添加边。
    pub fn add(&mut self, edge: Edge) {
        self.max_weight = self.max_weight.max(edge.weight);
        self.edges.push(edge);
    }

    /// 设置过滤器。
    pub fn set_filter(&mut self, filter: EdgeFilter) {
        self.filter = filter;
    }

    /// 权重→线宽（1.0~5.0 归一化）。
    fn map_width(&self, weight: f32) -> f32 {
        if self.max_weight <= 0.0 {
            return 1.0;
        }
        1.0 + (weight / self.max_weight) * 4.0
    }

    /// 应用过滤。
    fn passes(&self, e: &Edge) -> bool {
        if let Some(k) = self.filter.kind {
            if e.kind != k {
                return false;
            }
        }
        if let Some(a) = self.filter.account {
            if e.account != a {
                return false;
            }
        }
        if e.weight < self.filter.min_weight {
            return false;
        }
        true
    }

    /// 生成样式（含环警告）。
    pub fn styles(&self) -> Vec<EdgeStyle> {
        self.edges
            .iter()
            .filter(|e| self.passes(e))
            .map(|e| EdgeStyle {
                edge: e.id,
                width: self.map_width(e.weight),
                stroke: e.kind.stroke(),
                animated: e.kind.animated(),
                cycle_warn: e.in_cycle,
                rgb: if e.in_cycle {
                    0xFF_00_00 // 环警告红
                } else {
                    match e.kind {
                        EdgeKind::IntraFrame => 0x60_7D_8B,
                        EdgeKind::CrossFrame => 0x7B_1F_A2,
                        EdgeKind::Waiting => 0xFF_98_00,
                    }
                },
            })
            .collect()
    }

    /// 环警告边列表。
    pub fn cycle_edges(&self) -> Vec<&Edge> {
        self.edges.iter().filter(|e| e.in_cycle).collect()
    }

    /// 可见边数（过滤后）。
    pub fn visible_count(&self) -> usize {
        self.edges.iter().filter(|e| self.passes(e)).count()
    }
    /// 总边数。
    pub fn total_count(&self) -> usize {
        self.edges.len()
    }
}

impl Default for EdgeViz {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_cgb293_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0293 依赖边可视化");

    let mut v = EdgeViz::new();
    v.add(Edge {
        id: 1,
        from: 1,
        to: 2,
        kind: EdgeKind::IntraFrame,
        weight: 1.0,
        account: 1,
        in_cycle: false,
    });
    v.add(Edge {
        id: 2,
        from: 2,
        to: 3,
        kind: EdgeKind::CrossFrame,
        weight: 5.0,
        account: 1,
        in_cycle: false,
    });
    v.add(Edge {
        id: 3,
        from: 3,
        to: 1,
        kind: EdgeKind::Waiting,
        weight: 10.0,
        account: 2,
        in_cycle: true,
    });

    // --- 三型 ---
    let styles = v.styles();
    cs.add("三型边", styles.len() == 3, "三条边");
    let strokes: Vec<u8> = styles.iter().map(|s| s.stroke).collect();
    cs.add(
        "三型线型各异",
        strokes.iter().collect::<alloc::collections::BTreeSet<_>>().len() == 3,
        "实/弧/虚",
    );

    // --- 权重映射线宽 ---
    let w_min = styles.iter().map(|s| s.width).fold(f32::MAX, f32::min);
    let w_max = styles.iter().map(|s| s.width).fold(f32::MIN, f32::max);
    cs.add("权重→线宽", w_max > w_min, "重边更粗");

    // --- 闪烁 ---
    let waiting = styles.iter().find(|s| s.edge == 3).unwrap();
    cs.add("等待边闪烁", waiting.animated, "animated=true");

    // --- 环警告 ---
    cs.add("环警告红", styles.iter().find(|s| s.edge == 3).unwrap().rgb == 0xFF_00_00, "红色");
    cs.add("环警告标记", styles.iter().find(|s| s.edge == 3).unwrap().cycle_warn, "warn=true");
    cs.add("环边列表 1 条", v.cycle_edges().len() == 1, "一条环边");

    // --- 过滤：按类型 ---
    v.set_filter(EdgeFilter {
        kind: Some(EdgeKind::Waiting),
        account: None,
        min_weight: 0.0,
    });
    cs.add("类型过滤", v.visible_count() == 1, "只剩等待边");

    // --- 过滤：按账户 ---
    v.set_filter(EdgeFilter {
        kind: None,
        account: Some(1),
        min_weight: 0.0,
    });
    cs.add("账户过滤", v.visible_count() == 2, "账户1 两条");

    // --- 过滤：最小权重 ---
    v.set_filter(EdgeFilter {
        kind: None,
        account: None,
        min_weight: 4.0,
    });
    cs.add("权重过滤", v.visible_count() == 2, "≥4.0 两条");

    // --- 边型名称 ---
    cs.add("边型命名", EdgeKind::Waiting.name() == "等待边", "三型命名");

    cs
}
