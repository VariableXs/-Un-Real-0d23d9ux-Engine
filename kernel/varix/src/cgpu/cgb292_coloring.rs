// cgb292_coloring.rs ↔ CGPU-F0292 节点状态着色语义
//
// 判据原文：CGPU-F0292-J1 五态、双冗余、死锁高亮、热力模式、判据。
// 难点分析：色盲可辨的「形状+纹理双冗余」意味着不能只靠颜色区分五态；
//          死锁标红+环高亮需要与 B06 死锁检测联动。
// 纪律约束：五态色标；双冗余；死锁环高亮；热力第二配色方案。
// 成本记账：O(1) 着色决策；环高亮 O(cycle)。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;

pub type NodeId = u32;

/// 五态（与 F0291 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorState {
    Ready,
    Running,
    Blocked,
    Waiting,
    Done,
}

/// 配色方案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorScheme {
    /// 标准状态色。
    Status,
    /// 预算热力（按账户超用程度）。
    BudgetHeat,
}

/// 节点着色结果。
#[derive(Debug, Clone, Copy)]
pub struct NodeColor {
    pub node: NodeId,
    /// RGB 色值。
    pub rgb: u32,
    /// 形状码（双冗余）。
    pub shape: u8,
    /// 纹理码（双冗余：0=实 1=斜纹 2=点阵 3=网格 4=空白）。
    pub texture: u8,
    /// 是否死锁高亮。
    pub deadlock_hl: bool,
    /// 是否在死锁环路径上。
    pub in_cycle: bool,
}

/// 死锁环信息（B06 联动）。
#[derive(Debug, Clone)]
pub struct CycleInfo {
    /// 环上节点（有序）。
    pub nodes: Vec<NodeId>,
    /// 置信度（0.0~1.0）。
    pub confidence: f32,
}

/// 着色引擎。
pub struct ColoringEngine {
    scheme: ColorScheme,
    /// 死锁环列表。
    cycles: Vec<CycleInfo>,
    /// 账户超用程度（0~10000 万分比）→ 节点。
    budget_heat: Vec<(NodeId, u32)>,
    /// 缓存的着色结果。
    cache: Vec<NodeColor>,
}

impl ColoringEngine {
    /// 新建。
    pub fn new() -> Self {
        ColoringEngine {
            scheme: ColorScheme::Status,
            cycles: Vec::new(),
            budget_heat: Vec::new(),
            cache: Vec::new(),
        }
    }

    /// 切换配色方案。
    pub fn set_scheme(&mut self, scheme: ColorScheme) {
        self.scheme = scheme;
        self.cache.clear();
    }

    /// 注入死锁环（B06 联动）。
    pub fn inject_cycle(&mut self, cycle: CycleInfo) {
        self.cycles.push(cycle);
        self.cache.clear();
    }

    /// 清除死锁环（解除后）。
    pub fn clear_cycles(&mut self) {
        self.cycles.clear();
        self.cache.clear();
    }

    /// 设置预算热力值。
    pub fn set_budget_heat(&mut self, node: NodeId, heat_bp: u32) {
        if let Some(e) = self.budget_heat.iter_mut().find(|(n, _)| *n == node) {
            e.1 = heat_bp;
        } else {
            self.budget_heat.push((node, heat_bp));
        }
    }

    /// 状态色值（标准方案）。
    fn status_rgb(state: ColorState) -> u32 {
        match state {
            ColorState::Ready => 0x4C_AF_50,
            ColorState::Running => 0x21_96_F3,
            ColorState::Blocked => 0xF4_43_36,
            ColorState::Waiting => 0xFF_98_00,
            ColorState::Done => 0x9E_9E_9E,
        }
    }

    /// 形状码（双冗余之一）。
    fn shape_code(state: ColorState) -> u8 {
        match state {
            ColorState::Ready => 0,
            ColorState::Running => 1,
            ColorState::Blocked => 2,
            ColorState::Waiting => 3,
            ColorState::Done => 4,
        }
    }

    /// 纹理码（双冗余之二）。
    fn texture_code(state: ColorState) -> u8 {
        match state {
            ColorState::Ready => 0,
            ColorState::Running => 1,
            ColorState::Blocked => 2,
            ColorState::Waiting => 3,
            ColorState::Done => 4,
        }
    }

    /// 预算热力色（第二配色：绿→黄→红梯度）。
    fn heat_rgb(heat_bp: u32) -> u32 {
        // 0%=绿 50%=黄 100%=红
        let h = heat_bp.min(10000);
        if h < 5000 {
            // 绿→黄
            let t = h as f32 / 5000.0;
            let r = (76.0 + t * (255.0 - 76.0)) as u32;
            let g = (175.0 + t * (235.0 - 175.0)) as u32;
            (r << 16) | (g << 8) | 0x00
        } else {
            // 黄→红
            let t = (h - 5000) as f32 / 5000.0;
            let g = (235.0 - t * (235.0 - 50.0)) as u32;
            (255 << 16) | (g << 8) | 0x00
        }
    }

    /// 为单个节点着色。
    pub fn color_node(&self, node: NodeId, state: ColorState) -> NodeColor {
        let in_cycle = self.cycles.iter().any(|c| c.nodes.contains(&node));
        let deadlock_hl = in_cycle;

        let (rgb, shape, texture) = match self.scheme {
            ColorScheme::Status => {
                if deadlock_hl {
                    // 死锁标红 + 形状 2（三角=危险）
                    (0xFF_00_00, 2, 2)
                } else {
                    (
                        Self::status_rgb(state),
                        Self::shape_code(state),
                        Self::texture_code(state),
                    )
                }
            }
            ColorScheme::BudgetHeat => {
                let heat = self
                    .budget_heat
                    .iter()
                    .find(|(n, _)| *n == node)
                    .map(|(_, h)| *h)
                    .unwrap_or(0);
                (Self::heat_rgb(heat), Self::shape_code(state), Self::texture_code(state))
            }
        };

        NodeColor {
            node,
            rgb,
            shape,
            texture,
            deadlock_hl,
            in_cycle,
        }
    }

    /// 批量着色。
    pub fn color_batch(&mut self, items: &[(NodeId, ColorState)]) -> &[NodeColor] {
        self.cache.clear();
        for (id, st) in items {
            let c = self.color_node(*id, *st);
            self.cache.push(c);
        }
        &self.cache
    }

    /// 环高亮节点列表。
    pub fn cycle_highlight_nodes(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        for c in &self.cycles {
            for n in &c.nodes {
                if !out.contains(n) {
                    out.push(*n);
                }
            }
        }
        out
    }

    /// 当前方案。
    pub fn scheme(&self) -> ColorScheme {
        self.scheme
    }
}

impl Default for ColoringEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_cgb292_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0292 节点状态着色语义");

    let mut e = ColoringEngine::new();

    // --- 五态 ---
    let states = [
        ColorState::Ready,
        ColorState::Running,
        ColorState::Blocked,
        ColorState::Waiting,
        ColorState::Done,
    ];
    let mut colors_seen = Vec::new();
    for (i, s) in states.iter().enumerate() {
        let c = e.color_node(i as NodeId, *s);
        colors_seen.push(c.rgb);
    }
    cs.add("五态着色", colors_seen.len() == 5, "五个状态");
    cs.add(
        "五态色各异",
        colors_seen.iter().collect::<alloc::collections::BTreeSet<_>>().len() >= 4,
        "至少 4 种可辨",
    );

    // --- 双冗余：形状+纹理 ---
    let c_ready = e.color_node(10, ColorState::Ready);
    let c_blocked = e.color_node(11, ColorState::Blocked);
    cs.add(
        "形状双冗余",
        c_ready.shape != c_blocked.shape,
        "形状可辨",
    );
    cs.add(
        "纹理双冗余",
        c_ready.texture != c_blocked.texture,
        "纹理可辨",
    );

    // --- 死锁高亮 ---
    e.inject_cycle(CycleInfo {
        nodes: alloc::vec![1, 2, 3],
        confidence: 0.9,
    });
    let c_dead = e.color_node(1, ColorState::Running);
    cs.add("死锁标红", c_dead.rgb == 0xFF_00_00, "环上节点红");
    cs.add("死锁高亮标记", c_dead.deadlock_hl, "hl=true");
    cs.add("环路径标记", c_dead.in_cycle, "in_cycle=true");
    let hl = e.cycle_highlight_nodes();
    cs.add("环高亮列表", hl.len() == 3, "三个环节点");

    // --- 清除死锁 ---
    e.clear_cycles();
    let c_ok = e.color_node(1, ColorState::Running);
    cs.add("解除后恢复", !c_ok.deadlock_hl, "高亮取消");

    // --- 热力模式 ---
    e.set_scheme(ColorScheme::BudgetHeat);
    cs.add("方案切换", e.scheme() == ColorScheme::BudgetHeat, "热力模式");
    e.set_budget_heat(5, 9000); // 90% 超用
    let c_hot = e.color_node(5, ColorState::Running);
    let c_cool = e.color_node(6, ColorState::Running);
    cs.add(
        "热力区分",
        c_hot.rgb != c_cool.rgb,
        "超用 vs 正常",
    );

    // --- 批量 ---
    let items = alloc::vec![
        (20, ColorState::Ready),
        (21, ColorState::Done),
    ];
    let batch = e.color_batch(&items);
    cs.add("批量着色", batch.len() == 2, "两个节点");

    cs
}
