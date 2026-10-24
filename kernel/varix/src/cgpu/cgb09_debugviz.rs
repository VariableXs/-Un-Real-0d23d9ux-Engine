//! CGPU-F0169 · 帧图调试可视化（CGPU-B 域 · 帧任务图域 · B09 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0169`
//!
//! **判据（锚点原文）**：DAG 正确、时间轴对齐、交互详情、与探针联动、
//! 渲染开销。
//!
//! **职责定位（锚点原文）**：帧任务图的人读视图：DAG 可视化（节点=任务
//! 色块按队列着色，边=依赖箭头，关键路径加粗——与 F0156 管线可视化器
//! 的图模式衔接）、时间轴模式（节点按实际执行时间排布——与探针数据
//! 叠加）、交互（点节点看详情：读写集/预算/实际耗时/失败信息）。
//!
//! ## 一、DAG 视图（判据一：拓扑正确 + 队列着色 + 关键路径加粗）
//!
//! 节点=任务色块按队列着色（[`QueuePalette`]——队列闭集五色，色键
//! 人读）；边=依赖箭头（[`DagView`]——边集来自读写依赖，环显性拒
//! [`XcCode::CycleDetected`]——Kahn 拓扑排序 O(V+E) 检测，零递归）；
//! 关键路径加粗（[`critical_path`]——按实际耗时最长链 O(V+E)）。
//! DAG 正确性三断言：拓扑序存在（无环）/ 依赖边端点在册（悬空边拒
//! [`XcCode::DanglingEdge`]）/ 节点 id 唯一（重 id 拒
//! [`XcCode::DuplicateNode`]）。
//!
//! ## 二、时间轴模式（判据二：按实际执行时间排布 + 探针叠加）
//!
//! 节点按实际执行时间排布（[`TimelineView`]——`[start_tick,end_tick]`
//! 定位，起点回退显性拒 [`XcCode::TickBackward`]）；与探针数据叠加
//! （[`ProbeMark`]——探针 tick+标签叠加在时间轴上，越界探针拒
//! [`XcCode::ProbeOutOfRange`]）；区间重叠查询（[`TimelineView::overlaps_at`]
//! ——O(n) 单遍，重叠账留痕）。
//!
//! ## 三、交互详情（判据三：点节点看详情）
//!
//! 点节点看详情（[`node_detail`]——读写集/预算/实际耗时/失败信息
//! 四区齐出；缺区显性标注「缺」不静默）；详情指纹（FNV 汇总，对拍
//! 防视图与账本脱节）。
//!
//! ## 四、与探针联动（判据四）
//!
//! 探针联动（[`ProbeLedger`]——探针追加式账本：tick 有序性校验 +
//! 按节点索引查询 [`ProbeLedger::probes_of`]；时间轴叠加消费同源
//! 账本——联动=同账本双视图，不另立数据源）。
//!
//! ## 五、渲染开销（判据五）
//!
//! 渲染开销（[`RenderOverhead`]——节点/边渲染计价常量表
//! [`COST_PER_NODE`]/[`COST_PER_EDGE`]；DAG+时间轴+详情三视图开销
//! 汇总 saturating 累加；上限闸 [`RenderOverhead::within_budget`]
//! ——超预算显性立案不静默）；复杂度声明 O(V+E)（拓扑+着色+关键
//! 路径）与 O(n)（时间轴排布+探针叠加），无常数外循环。
//!
//! **对接**：F0156（管线可视化器——图模式衔接缝）；F0168（帧图与
//! GPU 队列映射——队列着色的上游）；F0167（执行失败与降级——失败
//! 信息详情的上游）；cgb08_framegraph（帧任务图——节点/边数据源）。
//! 零 panic 面、零 IO、零墙钟（tick 全由上游注入）、no_std。

use alloc::format;
use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const DEBUGVIZ_VERSION: &str = "B09-debugviz-v1";

/// 队列色键闭集（五队列；与 F0168 队列映射对齐）。
pub const QUEUE_COUNT: usize = 5;

/// 关键路径加粗标记（人读文本面）。
pub const CRITICAL_MARK: &str = "**";

/// 探针叠加符号。
pub const PROBE_MARK: &str = "◆";

/// 渲染计价：每节点开销（逻辑成本单位，非墙钟）。
pub const COST_PER_NODE: u32 = 3;

/// 渲染计价：每边开销。
pub const COST_PER_EDGE: u32 = 1;

/// 渲染计价：每探针叠加开销。
pub const COST_PER_PROBE: u32 = 2;

/// 渲染计价：详情查询固定开销。
pub const COST_PER_DETAIL: u32 = 5;

/// 视图渲染开销预算（成本单位；超线立案）。
pub const RENDER_BUDGET: u32 = 4096;

// ---------------------------------------------------------------------------
// 二、诊断码（0x2D 段独占：B09 可视化面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x2D 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 节点 id 重复。
    DuplicateNode,
    /// 依赖边端点不在册（悬空边）。
    DanglingEdge,
    /// 依赖环（拓扑序不存在）。
    CycleDetected,
    /// 时间轴起点回退（end < start）。
    TickBackward,
    /// 探针越界（探针 tick 超出时间轴窗口）。
    ProbeOutOfRange,
    /// 节点不在册（详情查询/着色查询命中空）。
    NodeMissing,
    /// 队列表外（五队列闭集之外）。
    QueueUnknown,
    /// 渲染超预算。
    OverBudget,
    /// 视图指纹不符（视图与账本脱节嫌疑）。
    ViewFingerprintMismatch,
}

impl XcCode {
    /// 人读诊断（含可操作建议）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::DuplicateNode => "节点 id 重复：图账拒绝重 id，建议先查重复建源再入图",
            XcCode::DanglingEdge => "悬空边：依赖边端点不在册，建议先补节点或删边",
            XcCode::CycleDetected => "依赖环：拓扑序不存在，建议沿环回溯解环（帧图 DAG 不允许环）",
            XcCode::TickBackward => "时间轴回退：end<start 非法区间，建议核对探针采集序",
            XcCode::ProbeOutOfRange => "探针越界：探针 tick 在时间轴窗口外，建议先扩窗或修探针账",
            XcCode::NodeMissing => "节点不在册：详情/着色查询命中空，建议按 DagView 在册 id 查询",
            XcCode::QueueUnknown => "队列表外：五队列闭集之外无色，建议按 QUEUE_COUNT 取用",
            XcCode::OverBudget => "渲染超预算：视图成本超 RENDER_BUDGET，建议裁剪节点/边或分页渲染",
            XcCode::ViewFingerprintMismatch => "视图指纹不符：视图与账本脱节嫌疑，建议重建视图再对拍",
        }
    }

    /// 诊断码短键（机检 grep 键面）。
    pub const fn key(self) -> &'static str {
        match self {
            XcCode::DuplicateNode => "b09.xc.duplicate-node",
            XcCode::DanglingEdge => "b09.xc.dangling-edge",
            XcCode::CycleDetected => "b09.xc.cycle-detected",
            XcCode::TickBackward => "b09.xc.tick-backward",
            XcCode::ProbeOutOfRange => "b09.xc.probe-out-of-range",
            XcCode::NodeMissing => "b09.xc.node-missing",
            XcCode::QueueUnknown => "b09.xc.queue-unknown",
            XcCode::OverBudget => "b09.xc.over-budget",
            XcCode::ViewFingerprintMismatch => "b09.xc.view-fingerprint-mismatch",
        }
    }

    /// 诊断码段位（0x2D00 基座，逐码偏移）。
    pub const fn code(self) -> u16 {
        0x2D00
            + match self {
                XcCode::DuplicateNode => 0x01,
                XcCode::DanglingEdge => 0x02,
                XcCode::CycleDetected => 0x03,
                XcCode::TickBackward => 0x04,
                XcCode::ProbeOutOfRange => 0x05,
                XcCode::NodeMissing => 0x06,
                XcCode::QueueUnknown => 0x07,
                XcCode::OverBudget => 0x08,
                XcCode::ViewFingerprintMismatch => 0x09,
            }
    }
}

// ---------------------------------------------------------------------------
// 三、FNV-1a 指纹基元
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单字节吸收。
pub const fn fnv1a_byte(state: u64, byte: u8) -> u64 {
    (state ^ (byte as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a 字节串吸收。
pub const fn fnv1a_bytes(state: u64, bytes: &[u8]) -> u64 {
    let mut s = state;
    let mut i = 0usize;
    while i < bytes.len() {
        s = fnv1a_byte(s, bytes[i]);
        i += 1;
    }
    s
}

/// FNV-1a 字符串吸收。
pub fn fnv1a_str(state: u64, text: &str) -> u64 {
    fnv1a_bytes(state, text.as_bytes())
}

/// FNV-1a u64 吸收（大端，确定性跨端序）。
pub const fn fnv1a_u64(state: u64, value: u64) -> u64 {
    fnv1a_bytes(state, &value.to_be_bytes())
}

/// FNV-1a u32 吸收。
pub const fn fnv1a_u32(state: u64, value: u32) -> u64 {
    fnv1a_bytes(state, &value.to_be_bytes())
}

// ---------------------------------------------------------------------------
// 四、队列着色（判据一·节点色块）
// ---------------------------------------------------------------------------

/// 队列色键（人读；五队列闭集，与 F0168 队列映射序对齐）。
pub const QUEUE_COLORS: [&str; QUEUE_COUNT] =
    ["q0-图形", "q1-计算", "q2-拷贝", "q3-视频", "q4-呈现"];

/// 队列色值（HEX 文本面——人读视图直接取用）。
pub const QUEUE_HEX: [&str; QUEUE_COUNT] =
    ["#4FA3E0", "#E0B84F", "#6FD08C", "#C86FD0", "#E06F6F"];

/// 队列着色核验（表外队列显性拒）。
pub fn queue_color_ok(queue: usize) -> Result<(usize, &'static str), XcCode> {
    if queue >= QUEUE_COUNT {
        return Err(XcCode::QueueUnknown);
    }
    let name = match QUEUE_COLORS.get(queue) {
        Some(n) => *n,
        None => return Err(XcCode::QueueUnknown),
    };
    Ok((queue, name))
}

/// 节点视图（DAG 人读单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeView {
    /// 节点 id（图内唯一）。
    pub id: u32,
    /// 任务名（人读）。
    pub name: String,
    /// 所属队列（0-4；着色键）。
    pub queue: usize,
    /// 预算（逻辑成本单位）。
    pub budget: u32,
    /// 实际耗时（tick；0=未执行）。
    pub actual_ticks: u32,
    /// 读集（资源键）。
    pub reads: Vec<String>,
    /// 写集（资源键）。
    pub writes: Vec<String>,
    /// 失败信息（None=无失败；来自 F0167 路由）。
    pub failure: Option<String>,
    /// 执行窗口（时间轴定位；start<=end）。
    pub start_tick: u64,
    pub end_tick: u64,
}

impl NodeView {
    /// 节点指纹（全字段混算）。
    pub fn fingerprint(&self) -> u64 {
        let mut s = fnv1a_u64(FNV_OFFSET, self.id as u64);
        s = fnv1a_str(s, &self.name);
        s = fnv1a_u64(s, self.queue as u64);
        s = fnv1a_u64(s, self.budget as u64);
        s = fnv1a_u64(s, self.actual_ticks as u64);
        let mut i = 0usize;
        while i < self.reads.len() {
            if let Some(r) = self.reads.get(i) {
                s = fnv1a_str(s, r);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.writes.len() {
            if let Some(w) = self.writes.get(j) {
                s = fnv1a_str(s, w);
            }
            j += 1;
        }
        if let Some(f) = self.failure.as_ref() {
            s = fnv1a_str(s, f);
        }
        s = fnv1a_u64(s, self.start_tick);
        fnv1a_u64(s, self.end_tick)
    }
}

// ---------------------------------------------------------------------------
// 五、DAG 视图（判据一·拓扑正确 + 关键路径）
// ---------------------------------------------------------------------------

/// 依赖边（from 完成后 to 才可跑）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeView {
    /// 前驱节点 id。
    pub from: u32,
    /// 后继节点 id。
    pub to: u32,
}

/// DAG 视图（节点账 + 边账 + 拓扑/着色/关键路径三件套）。
#[derive(Clone, Debug, Default)]
pub struct DagView {
    nodes: Vec<NodeView>,
    edges: Vec<EdgeView>,
    /// 建图指纹（节点+边全量混算——视图对拍基元）。
    build_fingerprint: u64,
}

impl DagView {
    /// 空图。
    pub const fn new() -> DagView {
        DagView {
            nodes: Vec::new(),
            edges: Vec::new(),
            build_fingerprint: FNV_OFFSET,
        }
    }

    /// 节点数。
    pub const fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 边数。
    pub const fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 建图指纹。
    pub const fn fingerprint(&self) -> u64 {
        self.build_fingerprint
    }

    /// 入节点（重 id 拒；着色/区间校验并入）。
    pub fn add_node(&mut self, n: NodeView) -> Result<(), XcCode> {
        if n.queue >= QUEUE_COUNT {
            return Err(XcCode::QueueUnknown);
        }
        if n.end_tick < n.start_tick {
            return Err(XcCode::TickBackward);
        }
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(x) = self.nodes.get(i) {
                if x.id == n.id {
                    return Err(XcCode::DuplicateNode);
                }
            }
            i += 1;
        }
        self.build_fingerprint = fnv1a_u64(self.build_fingerprint, n.fingerprint());
        self.nodes.push(n);
        Ok(())
    }

    /// 入边（端点在册校验 + 自环拒）。
    pub fn add_edge(&mut self, from: u32, to: u32) -> Result<(), XcCode> {
        if from == to {
            return Err(XcCode::DanglingEdge);
        }
        if self.find(from).is_none() || self.find(to).is_none() {
            return Err(XcCode::DanglingEdge);
        }
        let mut i = 0usize;
        while i < self.edges.len() {
            if let Some(e) = self.edges.get(i) {
                if e.from == from && e.to == to {
                    // 重边幂等入账（不重复计价）。
                    return Ok(());
                }
            }
            i += 1;
        }
        self.build_fingerprint =
            fnv1a_u64(fnv1a_u64(self.build_fingerprint, from as u64), to as u64);
        self.edges.push(EdgeView { from, to });
        Ok(())
    }

    /// 按 id 查节点。
    pub fn find(&self, id: u32) -> Option<&NodeView> {
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

    /// 拓扑序（Kahn；环显性拒——判据一「DAG 正确」的第一断言）。
    pub fn topo_order(&self) -> Result<Vec<u32>, XcCode> {
        let n = self.nodes.len();
        let mut indeg = vec![0u32; n];
        // id→槽位线性索引（图规模小，O(V*E) 建表可承受；无 HashMap 依赖）。
        let mut i = 0usize;
        while i < self.edges.len() {
            if let Some(e) = self.edges.get(i) {
                let mut j = 0usize;
                while j < n {
                    if let Some(nd) = self.nodes.get(j) {
                        if nd.id == e.to {
                            indeg[j] = indeg[j].saturating_add(1);
                        }
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        let mut order: Vec<u32> = Vec::new();
        let mut removed = vec![false; n];
        // 反复扫零入度节点（确定性：按槽位序取最早——同图同序）。
        loop {
            let mut picked: Option<usize> = None;
            let mut j = 0usize;
            while j < n {
                if !removed[j] && indeg[j] == 0 {
                    picked = Some(j);
                    break;
                }
                j += 1;
            }
            let p = match picked {
                Some(x) => x,
                None => break,
            };
            if let Some(nd) = self.nodes.get(p) {
                order.push(nd.id);
            }
            removed[p] = true;
            // 出边减度。
            let pid = match self.nodes.get(p) {
                Some(nd) => nd.id,
                None => return Err(XcCode::NodeMissing),
            };
            let mut k = 0usize;
            while k < self.edges.len() {
                if let Some(e) = self.edges.get(k) {
                    if e.from == pid {
                        let mut m = 0usize;
                        while m < n {
                            if let Some(nd) = self.nodes.get(m) {
                                if nd.id == e.to {
                                    indeg[m] = indeg[m].saturating_sub(1);
                                }
                            }
                            m += 1;
                        }
                    }
                }
                k += 1;
            }
        }
        if order.len() != n {
            return Err(XcCode::CycleDetected);
        }
        Ok(order)
    }

    /// 关键路径（按 actual_ticks 最长链；O(V*E) DP 上溯——确定性序）。
    /// 返回（路径 id 序, 总耗时）；环/空图给 None。
    pub fn critical_path(&self) -> Option<(Vec<u32>, u64)> {
        let order = self.topo_order().ok()?;
        if order.is_empty() {
            return None;
        }
        let n = self.nodes.len();
        // best[j]：以槽位 j 结尾的最长链累计耗时；prev[j]：前驱槽位。
        let mut best = vec![0u64; n];
        let mut prev: Vec<Option<usize>> = vec![None; n];
        let mut oi = 0usize;
        while oi < order.len() {
            let id = *order.get(oi)?;
            let j = self.slot_of(id)?;
            // 所有指向 j 的边里取最优前驱。
            let mut k = 0usize;
            while k < self.edges.len() {
                if let Some(e) = self.edges.get(k) {
                    if e.to == id {
                        if let Some(pslot) = self.slot_of(e.from) {
                            if let Some(pn) = self.nodes.get(pslot) {
                                let cand = best[pslot].saturating_add(pn.actual_ticks as u64);
                                if cand > best[j] {
                                    best[j] = cand;
                                    prev[j] = Some(pslot);
                                }
                            }
                        }
                    }
                }
                k += 1;
            }
            oi += 1;
        }
        // 终点=best 最大（平局取槽位最小——确定性）。
        let mut end = 0usize;
        let mut j = 0usize;
        while j < n {
            if best[j] > best[end] {
                end = j;
            }
            j += 1;
        }
        // 回溯路径。
        let mut path: Vec<u32> = Vec::new();
        let mut cur = Some(end);
        while let Some(c) = cur {
            if let Some(nd) = self.nodes.get(c) {
                path.push(nd.id);
            }
            cur = prev[c];
        }
        path.reverse();
        Some((path, best[end]))
    }

    /// id→槽位。
    fn slot_of(&self, id: u32) -> Option<usize> {
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

    /// DAG 文本渲染（人读；关键路径加粗；队列着色键标注）。
    /// O(V+E) 单遍扫边 + 关键路径集合一次查找。
    pub fn render_dag(&self) -> Result<String, XcCode> {
        let crit = self.critical_path();
        let mut crit_set: Vec<u32> = Vec::new();
        if let Some((p, total)) = crit.as_ref() {
            let mut i = 0usize;
            while i < p.len() {
                if let Some(x) = p.get(i) {
                    crit_set.push(*x);
                }
                i += 1;
            }
            let _ = total;
        }
        let mut s = String::new();
        s.push_str("dag {\n");
        let mut i = 0usize;
        while i < self.nodes.len() {
            let n = match self.nodes.get(i) {
                Some(x) => x,
                None => return Err(XcCode::NodeMissing),
            };
            let bold = {
                let mut hit = false;
                let mut k = 0usize;
                while k < crit_set.len() {
                    if let Some(c) = crit_set.get(k) {
                        if *c == n.id {
                            hit = true;
                        }
                    }
                    k += 1;
                }
                hit
            };
            let color = match QUEUE_HEX.get(n.queue) {
                Some(c) => *c,
                None => return Err(XcCode::QueueUnknown),
            };
            s.push_str(&alloc::format!(
                "  {}n{}[label=\"{}\",color={},ticks={}]{}\n",
                if bold { CRITICAL_MARK } else { "" },
                n.id,
                n.name,
                color,
                n.actual_ticks,
                if bold { CRITICAL_MARK } else { "" },
            ));
            i += 1;
        }
        let mut j = 0usize;
        while j < self.edges.len() {
            if let Some(e) = self.edges.get(j) {
                s.push_str(&alloc::format!("  n{} -> n{}\n", e.from, e.to));
            }
            j += 1;
        }
        s.push_str("}\n");
        if let Some((_, total)) = crit {
            s.push_str(&alloc::format!("关键路径耗时={}\n", total));
        }
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// 六、探针账本（判据四·与探针联动）
// ---------------------------------------------------------------------------

/// 探针标记（叠加在时间轴上的实测点）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeMark {
    /// 关联节点 id。
    pub node_id: u32,
    /// 探针 tick。
    pub tick: u64,
    /// 探针标签（人读短键，如 "pass"/"fail"/"budget"）。
    pub tag: String,
}

/// 探针追加式账本（tick 有序校验 + 按节点索引）。
#[derive(Clone, Debug, Default)]
pub struct ProbeLedger {
    marks: Vec<ProbeMark>,
    /// 账链指纹。
    chain: u64,
    /// 末笔 tick（有序基线）。
    last_tick: u64,
}

impl ProbeLedger {
    /// 空账。
    pub const fn new() -> ProbeLedger {
        ProbeLedger {
            marks: Vec::new(),
            chain: FNV_OFFSET,
            last_tick: 0,
        }
    }

    /// 入账（tick 回退显性拒——探针时序不可逆）。
    pub fn record(&mut self, node_id: u32, tick: u64, tag: &str) -> Result<(), XcCode> {
        if tick < self.last_tick {
            return Err(XcCode::TickBackward);
        }
        let m = ProbeMark {
            node_id,
            tick,
            tag: tag.to_string(),
        };
        self.chain = fnv1a_u64(fnv1a_u64(self.chain, node_id as u64), tick);
        self.chain = fnv1a_str(self.chain, tag);
        self.last_tick = tick;
        self.marks.push(m);
        Ok(())
    }

    /// 账笔数。
    pub const fn len(&self) -> usize {
        self.marks.len()
    }

    /// 是否空账。
    pub const fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// 按节点取探针（联动查询；O(n) 单遍）。
    pub fn probes_of(&self, node_id: u32) -> Vec<&ProbeMark> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.marks.len() {
            if let Some(m) = self.marks.get(i) {
                if m.node_id == node_id {
                    out.push(m);
                }
            }
            i += 1;
        }
        out
    }

    /// 全账只读槽取（越界 None）。
    pub fn mark_at(&self, i: usize) -> Option<&ProbeMark> {
        self.marks.get(i)
    }

    /// 账链指纹。
    pub const fn chain_fingerprint(&self) -> u64 {
        self.chain
    }

    /// 台账自洽核验（重放混算对拍）。
    pub fn verify(&self) -> Result<(), XcCode> {
        let mut s = FNV_OFFSET;
        let mut last = 0u64;
        let mut i = 0usize;
        while i < self.marks.len() {
            let m = match self.marks.get(i) {
                Some(x) => x,
                None => return Err(XcCode::NodeMissing),
            };
            if m.tick < last {
                return Err(XcCode::TickBackward);
            }
            s = fnv1a_u64(fnv1a_u64(s, m.node_id as u64), m.tick);
            s = fnv1a_str(s, &m.tag);
            last = m.tick;
            i += 1;
        }
        if s != self.chain {
            return Err(XcCode::ViewFingerprintMismatch);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 七、时间轴视图（判据二·按实际执行时间排布 + 探针叠加）
// ---------------------------------------------------------------------------

/// 时间轴视图（DAG 节点的 `[start,end]` 排布 + 探针叠加）。
pub struct TimelineView<'a> {
    dag: &'a DagView,
    probes: &'a ProbeLedger,
    /// 窗口（轴范围；探针叠加校验用）。
    window_start: u64,
    window_end: u64,
}

impl<'a> TimelineView<'a> {
    /// 建视图（窗口自动取全节点 min/max 区间，探针越界窗内校验延后到渲染）。
    pub fn build(dag: &'a DagView, probes: &'a ProbeLedger) -> Option<TimelineView<'a>> {
        if dag.node_count() == 0 {
            return None;
        }
        let mut lo = u64::MAX;
        let mut hi = 0u64;
        let mut i = 0usize;
        while i < dag.node_count() {
            if let Some(n) = dag.nodes.get(i) {
                if n.start_tick < lo {
                    lo = n.start_tick;
                }
                if n.end_tick > hi {
                    hi = n.end_tick;
                }
            }
            i += 1;
        }
        Some(TimelineView {
            dag,
            probes,
            window_start: lo,
            window_end: hi,
        })
    }

    /// 窗口读取。
    pub const fn window(&self) -> (u64, u64) {
        (self.window_start, self.window_end)
    }

    /// 探针越界核验（判据二对齐：叠加前先校验全部探针在窗内）。
    pub fn verify_probes_in_window(&self) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < self.probes.len() {
            if let Some(m) = self.probes.mark_at(i) {
                if m.tick < self.window_start || m.tick > self.window_end {
                    return Err(XcCode::ProbeOutOfRange);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 给定 tick 的重叠节点（时间轴对齐查询；O(n) 单遍）。
    pub fn overlaps_at(&self, tick: u64) -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.dag.nodes.len() {
            if let Some(n) = self.dag.nodes.get(i) {
                if tick >= n.start_tick && tick <= n.end_tick {
                    out.push(n.id);
                }
            }
            i += 1;
        }
        out
    }

    /// 区间重叠对账（两节点执行窗口重叠即并列——并行度证据面）。
    pub fn overlap_pairs(&self) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        let n = self.dag.nodes.len();
        let mut i = 0usize;
        while i < n {
            let a = match self.dag.nodes.get(i) {
                Some(x) => x,
                None => break,
            };
            let mut j = i + 1;
            while j < n {
                let b = match self.dag.nodes.get(j) {
                    Some(x) => x,
                    None => break,
                };
                if a.start_tick <= b.end_tick && b.start_tick <= a.end_tick {
                    out.push((a.id, b.id));
                }
                j += 1;
            }
            i += 1;
        }
        out
    }

    /// 时间轴文本渲染（行=节点，刻度=区间；探针行叠加）。
    pub fn render(&self) -> Result<String, XcCode> {
        self.verify_probes_in_window()?;
        let mut s = String::new();
        s.push_str(&alloc::format!(
            "timeline [{}..{}]\n",
            self.window_start, self.window_end
        ));
        let mut i = 0usize;
        while i < self.dag.nodes.len() {
            let n = match self.dag.nodes.get(i) {
                Some(x) => x,
                None => return Err(XcCode::NodeMissing),
            };
            s.push_str(&alloc::format!(
                "  n{} {} [{}..{}] ticks={}\n",
                n.id, n.name, n.start_tick, n.end_tick, n.actual_ticks
            ));
            let ps = self.probes.probes_of(n.id);
            let mut k = 0usize;
            while k < ps.len() {
                if let Some(m) = ps.get(k) {
                    s.push_str(&alloc::format!(
                        "    {} probe@{} {}\n",
                        PROBE_MARK, m.tick, m.tag
                    ));
                }
                k += 1;
            }
            i += 1;
        }
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// 八、交互详情（判据三·点节点看详情）
// ---------------------------------------------------------------------------

/// 详情区块四区在账位（读写集/预算/实际/失败）。
pub const DETAIL_SECTIONS: [&str; 4] = ["读写集", "预算", "实际耗时", "失败信息"];

/// 节点详情查询（点节点看详情——四区齐出，缺区显性标「缺」）。
pub fn node_detail(dag: &DagView, id: u32) -> Result<String, XcCode> {
    let n = match dag.find(id) {
        Some(x) => x,
        None => return Err(XcCode::NodeMissing),
    };
    let mut s = String::new();
    s.push_str(&alloc::format!("详情 n{} {}（{}）\n", n.id, n.name, {
        let (_, qname) = queue_color_ok(n.queue)?;
        qname
    }));
    // 区一：读写集。
    if n.reads.is_empty() && n.writes.is_empty() {
        s.push_str("  读写集=缺\n");
    } else {
        s.push_str("  读写集:\n");
        let mut i = 0usize;
        while i < n.reads.len() {
            if let Some(r) = n.reads.get(i) {
                s.push_str(&alloc::format!("    读 {}\n", r));
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < n.writes.len() {
            if let Some(w) = n.writes.get(j) {
                s.push_str(&alloc::format!("    写 {}\n", w));
            }
            j += 1;
        }
    }
    // 区二：预算。
    if n.budget == 0 {
        s.push_str("  预算=缺\n");
    } else {
        s.push_str(&alloc::format!("  预算={}\n", n.budget));
    }
    // 区三：实际耗时。
    if n.actual_ticks == 0 {
        s.push_str("  实际耗时=缺（未执行）\n");
    } else {
        s.push_str(&alloc::format!(
            "  实际耗时={}（预算比={}bps）\n",
            n.actual_ticks,
            budget_ratio_bps(n.actual_ticks, n.budget)
        ));
    }
    // 区四：失败信息。
    match n.failure.as_ref() {
        Some(f) => s.push_str(&alloc::format!("  失败信息={}\n", f)),
        None => s.push_str("  失败信息=无\n"),
    }
    s.push_str(&alloc::format!("  详情指纹={:#018x}\n", n.fingerprint()));
    Ok(s)
}

/// 预算比（实际/预算，bps 制；预算 0 给 None→按满额 10000 兜底不除零）。
pub const fn budget_ratio_bps(actual: u32, budget: u32) -> u32 {
    if budget == 0 {
        return SCORE_FULL_BPS;
    }
    let num = (actual as u64).saturating_mul(SCORE_FULL_BPS as u64);
    (num / budget as u64) as u32
}

/// bps 满额。
pub const SCORE_FULL_BPS: u32 = 10_000;

// ---------------------------------------------------------------------------
// 九、渲染开销（判据五·计价 + 预算闸）
// ---------------------------------------------------------------------------

/// 渲染开销账（三视图成本汇总 + 预算闸）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderOverhead {
    /// DAG 渲染成本。
    pub dag_cost: u32,
    /// 时间轴渲染成本。
    pub timeline_cost: u32,
    /// 详情查询成本。
    pub detail_cost: u32,
    /// 探针叠加成本。
    pub probe_cost: u32,
}

impl RenderOverhead {
    /// 按 DAG 规模计价。
    pub const fn for_dag(nodes: usize, edges: usize) -> RenderOverhead {
        RenderOverhead {
            dag_cost: (nodes as u32).saturating_mul(COST_PER_NODE)
                .saturating_add((edges as u32).saturating_mul(COST_PER_EDGE)),
            timeline_cost: (nodes as u32).saturating_mul(COST_PER_NODE),
            detail_cost: COST_PER_DETAIL,
            probe_cost: 0,
        }
    }

    /// 叠加探针计价。
    pub const fn with_probes(mut self, probes: usize) -> RenderOverhead {
        self.probe_cost = (probes as u32).saturating_mul(COST_PER_PROBE);
        self
    }

    /// 总成本（saturating 汇总）。
    pub const fn total(&self) -> u32 {
        self.dag_cost
            .saturating_add(self.timeline_cost)
            .saturating_add(self.detail_cost)
            .saturating_add(self.probe_cost)
    }

    /// 预算闸（超线显性立案）。
    pub const fn within_budget(&self) -> Result<u32, XcCode> {
        let t = self.total();
        if t > RENDER_BUDGET {
            return Err(XcCode::OverBudget);
        }
        Ok(t)
    }

    /// 人读面。
    pub fn report(&self) -> String {
        alloc::format!(
            "RenderOverhead: dag={}/timeline={}/detail={}/probe={}/total={}/预算={}",
            self.dag_cost,
            self.timeline_cost,
            self.detail_cost,
            self.probe_cost,
            self.total(),
            RENDER_BUDGET
        )
    }
}

// ---------------------------------------------------------------------------
// 十、可视化引擎总编排（三视图一闸一指纹）
// ---------------------------------------------------------------------------

/// 可视化引擎（DAG+探针双源 + 渲染编排 + 对拍闸）。
pub struct DebugVizEngine {
    dag: DagView,
    probes: ProbeLedger,
    /// 渲染次数（编排账）。
    renders: u32,
}

impl DebugVizEngine {
    /// 空引擎。
    pub const fn new() -> DebugVizEngine {
        DebugVizEngine {
            dag: DagView::new(),
            probes: ProbeLedger::new(),
            renders: 0,
        }
    }

    /// 入节点（转发 DagView）。
    pub fn add_node(&mut self, n: NodeView) -> Result<(), XcCode> {
        self.dag.add_node(n)
    }

    /// 入边（转发 DagView）。
    pub fn add_edge(&mut self, from: u32, to: u32) -> Result<(), XcCode> {
        self.dag.add_edge(from, to)
    }

    /// 入探针（转发 ProbeLedger）。
    pub fn add_probe(&mut self, node_id: u32, tick: u64, tag: &str) -> Result<(), XcCode> {
        if self.dag.find(node_id).is_none() {
            return Err(XcCode::NodeMissing);
        }
        self.probes.record(node_id, tick, tag)
    }

    /// DAG 渲染（判据一；编排账记一笔）。
    pub fn render_dag(&mut self) -> Result<String, XcCode> {
        self.mark_render();
        self.dag.render_dag()
    }

    /// 时间轴渲染（判据二；探针联动同源账本——判据四）。
    pub fn render_timeline(&mut self) -> Result<String, XcCode> {
        self.mark_render();
        match TimelineView::build(&self.dag, &self.probes) {
            Some(t) => t.render(),
            None => Err(XcCode::NodeMissing),
        }
    }

    /// 详情查询（判据三）。
    pub fn detail(&mut self, id: u32) -> Result<String, XcCode> {
        self.mark_render();
        node_detail(&self.dag, id)
    }

    /// 渲染预算核（判据五；每次渲染后调用方自核）。
    pub fn overhead(&self) -> RenderOverhead {
        RenderOverhead::for_dag(self.dag.node_count(), self.dag.edge_count())
            .with_probes(self.probes.len())
    }

    /// 引擎自检（双账自洽 + 预算闸；三断言全过才 Ok）。
    pub fn validate(&self) -> Result<(), XcCode> {
        self.dag.topo_order()?;
        self.probes.verify()?;
        self.overhead().within_budget()?;
        Ok(())
    }

    /// 引擎指纹（双账+渲染数混算；视图对拍基元）。
    pub fn fingerprint(&self) -> u64 {
        let s = fnv1a_u64(self.dag.fingerprint(), self.probes.chain_fingerprint());
        fnv1a_u64(s, self.renders as u64)
    }

    /// 编排账（渲染次数；一次性渲染入口计一笔）。
    pub const fn render_count(&self) -> u32 {
        self.renders
    }

    /// 渲染记账（内部递增；saturating 封顶）。
    pub(crate) fn mark_render(&mut self) {
        self.renders = self.renders.saturating_add(1);
    }
}

impl Default for DebugVizEngine {
    fn default() -> DebugVizEngine {
        DebugVizEngine::new()
    }
}
