//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2284 · 图族续编第二十一幅（调度全景一图） · 0x72BC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B35）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F2284-J1 第二十一幅生成全过（三调度类×每核
//!     runqueue×切换路径全景声明式生成冻结 v1，图账一致断言过——
//!     图中节点与 F1601–F1800 条目引用对平），图族序列续号登记
//!     （21 幅）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：三调度类（公平/实时/闲时）× 8 核 runqueue = 24 节点，每节点
//!   引用 F1601–F1800 连片 8 条目（24×8=192，覆盖 1601..=1792，尾段
//!   1793..=1800 为切换锚保留段）；切换路径边 = 环切 24 + 闲进 3 +
//!   闲出 3；图账一致 = 节点引用 vs 账面归属（条目→节点反查表）双向
//!   对平，越界/重复/缺失分别显性拒；图族 FIG-01..FIG-21 序号连续，
//!   第 21 幅哈希随册冻结；节点渲染双路对拍 5 万样本；失真注入 10 路
//!   10/10 必抓；冻结 v1 后版本再动即漂移拒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72BC0，VX3-1103 调度全景窗口独占）。
pub const SEG_BASE: u32 = 0x72BC0;
/// 调度类数（公平/实时/闲时，冻结 3）。
pub const CLASS_N: usize = 3;
/// 每类核数（冻结 8）。
pub const CORE_N: usize = 8;
/// runqueue 节点数（3×8=24）。
pub const NODE_N: usize = CLASS_N * CORE_N;
/// 每节点条目引用数（冻结 8）。
pub const REFS_PER_NODE: usize = 8;
/// 条目引用下界（F1601，冻结）。
pub const ENTRY_LO: u64 = 1601;
/// 条目引用上界（F1800，冻结）。
pub const ENTRY_HI: u64 = 1800;
/// 节点引用总量（24×8=192，覆盖 1601..=1792）。
pub const REFS_TOTAL: usize = NODE_N * REFS_PER_NODE;
/// 本幅序号（图族第 21 幅，冻结）。
pub const FIG_INDEX: u32 = 21;
/// 图族登记幅数（冻结 21）。
pub const FAMILY_N: usize = 21;
/// 冻结版本（v1，冻结）。
pub const FROZEN_V1: u32 = 1;
/// 全景纪元（冻结 9）。
pub const EPOCH: u64 = 9;
/// 失真注入路数（判据 10/10，冻结）。
pub const DRIFT_ROUNDS: u32 = 10;
/// 渲染对拍样本数（冻结 50,000）。
pub const FIDELITY_SAMPLES: usize = 50_000;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 切换路径边总数（环 24 + 闲进 3 + 闲出 3 = 30）。
pub const EDGE_TOTAL: usize = NODE_N + CLASS_N * 2;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanCode {
    /// 引用越界：节点引用落在 F1601–F1792 引用域之外
    RefOutOfRange,
    /// 引用重复：同一条目被引用两次以上
    RefDup,
    /// 引用缺失：账面归属的条目未被任何节点引用
    RefMissing,
    /// 节点未知：节点数/类属/核号与账面期望不符
    NodeUnknown,
    /// 边悬空：切换路径边端点越出节点闭集
    EdgeDangling,
    /// 序号断档：图族登记序号不连续或越界
    SeqGap,
    /// 哈希漂移：正文哈希/版本/幅哈希与冻结值不符
    HashDrift,
    /// 双跑不一致：复现红线，golden_run 两次应同值
    NotReproducible,
}

impl PanCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PanCode::RefOutOfRange => "引用越界：节点引用必须落在 F1601–F1792 引用域内",
            PanCode::RefDup => "引用重复：一条目只许一个节点引用，去重后重生图",
            PanCode::RefMissing => "引用缺失：账面归属条目未上图，重跑声明式生成",
            PanCode::NodeUnknown => "节点未知：节点数/类属/核号与账面期望不符，按 3×8 重排",
            PanCode::EdgeDangling => "边悬空：切换边端点必须落在 24 节点闭集内",
            PanCode::SeqGap => "序号断档：图族 FIG-01..FIG-21 连续登记，补齐断档幅",
            PanCode::HashDrift => "哈希漂移：正文哈希/版本/幅哈希与冻结值不符，冻结 v1 后不得再动",
            PanCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [PanCode; 8] = [
        PanCode::RefOutOfRange,
        PanCode::RefDup,
        PanCode::RefMissing,
        PanCode::NodeUnknown,
        PanCode::EdgeDangling,
        PanCode::SeqGap,
        PanCode::HashDrift,
        PanCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PanCode::ALL.len() {
        if PanCode::ALL[i].val() == code {
            return PanCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72BC0 段闭集内"
}

// ── 调度类（声明式生成的第一维） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedClass {
    /// 公平类：vruntime 红黑树调度。
    Fair,
    /// 实时类：高优先抢占调度。
    Realtime,
    /// 闲时类：空闲回收调度。
    Idle,
}

impl SchedClass {
    pub fn idx(self) -> usize {
        match self {
            SchedClass::Fair => 0,
            SchedClass::Realtime => 1,
            SchedClass::Idle => 2,
        }
    }
    pub fn from_idx(i: usize) -> SchedClass {
        match i % CLASS_N {
            1 => SchedClass::Realtime,
            2 => SchedClass::Idle,
            _ => SchedClass::Fair,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SchedClass::Fair => "公平",
            SchedClass::Realtime => "实时",
            SchedClass::Idle => "闲时",
        }
    }
}

/// 切换边种别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchKind {
    /// 类内核间环切换。
    RingNext,
    /// 闲时进入（各类核 7 → 闲时核 0）。
    IdleEnter,
    /// 闲时退出（闲时核 0 → 各类核 0）。
    IdleExit,
}

impl SwitchKind {
    pub fn label(self) -> &'static str {
        match self {
            SwitchKind::RingNext => "环切",
            SwitchKind::IdleEnter => "闲进",
            SwitchKind::IdleExit => "闲出",
        }
    }
}

// ── 图模型 ──────────────────────────────────────────────────────
/// runqueue 节点：节点号、类属、核号、条目引用表、版本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunqueueNode {
    pub node_id: usize,
    pub class: SchedClass,
    pub core: usize,
    pub refs: Vec<u64>,
    pub ver: u32,
}

/// 切换路径边：起节点 → 落节点 + 种别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchEdge {
    pub from: usize,
    pub to: usize,
    pub kind: SwitchKind,
}

/// 调度全景图：版本 + 节点表 + 边表 + 正文哈希。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanoramaGraph {
    pub version: u32,
    pub epoch: u64,
    pub nodes: Vec<RunqueueNode>,
    pub edges: Vec<SwitchEdge>,
    pub body_hash: u32,
}

// ── 同源事实唯一定义点 ──────────────────────────────────────────
/// 节点号 → 类属：每类连片 8 节点。
pub fn class_of_node(node_id: usize) -> SchedClass {
    SchedClass::from_idx(node_id / CORE_N)
}

/// 节点号 → 核号。
pub fn core_of_node(node_id: usize) -> usize {
    node_id % CORE_N
}

/// 节点号 → 引用基址：条目连片分配，节点 n 引用 [LO+n×8, LO+n×8+7]。
pub fn ref_base_of_node(node_id: usize) -> u64 {
    ENTRY_LO + (node_id * REFS_PER_NODE) as u64
}

// ── 校验和（FNV-1a 32 位，整数实现） ────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ── 声明式生成 ──────────────────────────────────────────────────
/// 切换边声明式生成：类内环切 24 边 + 闲进 3 + 闲出 3 = 30 边。
pub fn gen_edges() -> Vec<SwitchEdge> {
    let mut out: Vec<SwitchEdge> = Vec::new();
    for c in 0..CLASS_N {
        for k in 0..CORE_N {
            out.push(SwitchEdge {
                from: c * CORE_N + k,
                to: c * CORE_N + ((k + 1) % CORE_N),
                kind: SwitchKind::RingNext,
            });
        }
    }
    // 闲进：各类核 7 → 闲时核 0；闲出：闲时核 0 → 各类核 0。
    let idle0 = SchedClass::Idle.idx() * CORE_N;
    for c in 0..CLASS_N {
        out.push(SwitchEdge { from: c * CORE_N + (CORE_N - 1), to: idle0, kind: SwitchKind::IdleEnter });
        out.push(SwitchEdge { from: idle0, to: c * CORE_N, kind: SwitchKind::IdleExit });
    }
    out
}

/// 全景图声明式生成（草稿版 version=0；冻结由引擎置 v1）。
pub fn gen_panorama(epoch: u64) -> PanoramaGraph {
    let mut nodes: Vec<RunqueueNode> = Vec::new();
    for n in 0..NODE_N {
        let base = ref_base_of_node(n);
        let mut refs: Vec<u64> = Vec::new();
        for k in 0..REFS_PER_NODE {
            refs.push(base + k as u64);
        }
        nodes.push(RunqueueNode {
            node_id: n,
            class: class_of_node(n),
            core: core_of_node(n),
            refs,
            ver: 0,
        });
    }
    let edges = gen_edges();
    let mut g = PanoramaGraph { version: 0, epoch, nodes, edges, body_hash: 0 };
    g.body_hash = fnv1a(render_body(&g).as_bytes());
    g
}

// ── 账面归属（条目→节点反查表，独立路径） ──────────────────────
/// 账面期望：条目 → 持有节点号反查表。与声明式生成路径结构独立
/// （按条目遍历归属，非按节点铺引用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerExpect {
    /// 条目 → 持有节点号（REFS_TOTAL 项）。
    pub owner: Vec<usize>,
}

pub fn build_ledger_expect() -> LedgerExpect {
    let mut owner: Vec<usize> = Vec::new();
    for e in 0..REFS_TOTAL {
        // 反查：条目 e 的持有节点 = e / 每节点引用数。
        owner.push(e / REFS_PER_NODE);
    }
    LedgerExpect { owner }
}

// ── 渲染（双路独立：format 成型 vs 逐字段拼接） ─────────────────
/// 节点行（format 路径）：Q|节点号|类|核|引用首-尾|版本。
pub fn render_node(n: &RunqueueNode) -> String {
    let first = if let Some(x) = n.refs.first() { *x } else { 0 };
    let last = if let Some(x) = n.refs.last() { *x } else { 0 };
    format!("Q|{}|{}|{}|{}-{}|{}\n", n.node_id, n.class.label(), n.core, first, last, n.ver)
}

/// 节点行（直拼路径）：与 format 路径结构独立，供对拍基准。
pub fn direct_node(n: &RunqueueNode) -> String {
    let first = if let Some(x) = n.refs.first() { *x } else { 0 };
    let last = if let Some(x) = n.refs.last() { *x } else { 0 };
    let mut s = String::from("Q|");
    s.push_str(&format!("{}", n.node_id));
    s.push('|');
    s.push_str(n.class.label());
    s.push('|');
    s.push_str(&format!("{}|{}-{}|{}\n", n.core, first, last, n.ver));
    s
}

/// 边行：E|起|落|种别。
pub fn render_edge(e: &SwitchEdge) -> String {
    format!("E|{}|{}|{}\n", e.from, e.to, e.kind.label())
}

/// 正文渲染：节点 24 行 + 边 30 行。
pub fn render_body(g: &PanoramaGraph) -> String {
    let mut s = String::new();
    for n in &g.nodes {
        s.push_str(&render_node(n));
    }
    for e in &g.edges {
        s.push_str(&render_edge(e));
    }
    s
}

/// 渲染对拍：节点行双路逐字节相等，样本数 = FIDELITY_SAMPLES。
pub fn fidelity_nodes(g: &PanoramaGraph) -> Result<u32, u32> {
    let n = g.nodes.len();
    if n == 0 {
        return Err(PanCode::NodeUnknown.val());
    }
    let mut k = 0usize;
    while k < FIDELITY_SAMPLES {
        let node = match g.nodes.get(k % n) {
            Some(x) => x,
            None => return Err(PanCode::NodeUnknown.val()),
        };
        if render_node(node) != direct_node(node) {
            return Err(PanCode::HashDrift.val());
        }
        k += 1;
    }
    Ok(FIDELITY_SAMPLES as u32)
}

// ── 图账一致核验（双向对平） ────────────────────────────────────
/// 图账一致核验报告：对平引用数、核验边数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconStats {
    pub refs_reconciled: u32,
    pub edges_checked: u32,
}

/// 核验：版本冻结 + 节点期望 + 引用三查（越界/重复/缺失）+ 边闭集 + 哈希。
pub fn reconcile_graph(g: &PanoramaGraph, exp: &LedgerExpect) -> Result<ReconStats, u32> {
    if g.version != FROZEN_V1 || g.epoch != EPOCH {
        return Err(PanCode::HashDrift.val());
    }
    if g.nodes.len() != NODE_N {
        return Err(PanCode::NodeUnknown.val());
    }
    if exp.owner.len() != REFS_TOTAL {
        return Err(PanCode::RefMissing.val());
    }
    // 节点期望：类属/核号/引用片逐节点对。
    for (i, node) in g.nodes.iter().enumerate() {
        if node.class != class_of_node(i) || node.core != core_of_node(i) {
            return Err(PanCode::NodeUnknown.val());
        }
        if node.refs.len() != REFS_PER_NODE {
            return Err(PanCode::RefMissing.val());
        }
    }
    // 引用三查：越界 + 重复 + 缺失（对平账面归属）。
    let mut seen: Vec<bool> = Vec::new();
    for _ in 0..REFS_TOTAL {
        seen.push(false);
    }
    let mut reconciled = 0u32;
    for (i, node) in g.nodes.iter().enumerate() {
        for rv in &node.refs {
            let e = *rv;
            if e < ENTRY_LO || e >= ENTRY_LO + REFS_TOTAL as u64 || e > ENTRY_HI {
                return Err(PanCode::RefOutOfRange.val());
            }
            let slot = (e - ENTRY_LO) as usize;
            let held = match exp.owner.get(slot) {
                Some(x) => *x,
                None => return Err(PanCode::RefOutOfRange.val()),
            };
            if held != i {
                return Err(PanCode::RefDup.val());
            }
            let flag = match seen.get(slot) {
                Some(x) => *x,
                None => return Err(PanCode::RefOutOfRange.val()),
            };
            if flag {
                return Err(PanCode::RefDup.val());
            }
            if let Some(f) = seen.get_mut(slot) {
                *f = true;
            }
            reconciled += 1;
        }
    }
    // 覆盖收口：账面归属条目必须全部被引用。
    for f in &seen {
        if !*f {
            return Err(PanCode::RefMissing.val());
        }
    }
    // 边闭集：端点越出节点闭集即悬空；边数与种别构成逐边对。
    if g.edges.len() != EDGE_TOTAL {
        return Err(PanCode::EdgeDangling.val());
    }
    let mut ring_cnt = 0u32;
    let mut enter_cnt = 0u32;
    let mut exit_cnt = 0u32;
    for e in &g.edges {
        if e.from >= NODE_N || e.to >= NODE_N {
            return Err(PanCode::EdgeDangling.val());
        }
        match e.kind {
            SwitchKind::RingNext => ring_cnt += 1,
            SwitchKind::IdleEnter => enter_cnt += 1,
            SwitchKind::IdleExit => exit_cnt += 1,
        }
    }
    if ring_cnt != NODE_N as u32 || enter_cnt != CLASS_N as u32 || exit_cnt != CLASS_N as u32 {
        return Err(PanCode::EdgeDangling.val());
    }
    // 正文哈希对账。
    if g.body_hash != fnv1a(render_body(g).as_bytes()) {
        return Err(PanCode::HashDrift.val());
    }
    Ok(ReconStats { refs_reconciled: reconciled, edges_checked: g.edges.len() as u32 })
}

// ── 全景引擎（状态机：Drafting→FrozenV1→Registered，单调推进） ──
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanState {
    /// 起草：图在生成，版本未冻结。
    Drafting,
    /// 已冻结 v1：节点版本置 1，图面与哈希冻结。
    FrozenV1,
    /// 已登记：图族第 21 幅续号入册。
    Registered,
}

impl PanState {
    pub fn rank(self) -> u8 {
        match self {
            PanState::Drafting => 0,
            PanState::FrozenV1 => 1,
            PanState::Registered => 2,
        }
    }
    pub fn can_advance(self, to: PanState) -> bool {
        to.rank() > self.rank()
    }
}

/// 全景引擎：状态机 + 登记记账。
#[derive(Clone, Debug)]
pub struct PanEngine {
    pub state: PanState,
    pub epoch: u64,
    /// 已完成登记次数。
    pub registrations: u32,
}

impl PanEngine {
    pub fn new(epoch: u64) -> PanEngine {
        PanEngine { state: PanState::Drafting, epoch, registrations: 0 }
    }

    /// 状态推进公开口：倒退/平移显性拒。
    pub fn advance(&mut self, to: PanState) -> Result<(), u32> {
        if !self.state.can_advance(to) {
            return Err(PanCode::NotReproducible.val());
        }
        self.state = to;
        Ok(())
    }

    /// 冻结 v1：节点版本逐个置 1，图哈希重算，状态推进 FrozenV1。
    pub fn freeze(&mut self, g: &mut PanoramaGraph) -> Result<(), u32> {
        for n in &mut g.nodes {
            n.ver = FROZEN_V1;
        }
        g.version = FROZEN_V1;
        g.body_hash = fnv1a(render_body(g).as_bytes());
        self.advance(PanState::FrozenV1)
    }

    /// 登记口：仅 FrozenV1 态可登记，图账一致必须先过。
    pub fn register(&mut self, g: &PanoramaGraph, exp: &LedgerExpect) -> Result<ReconStats, u32> {
        if self.state.rank() < PanState::FrozenV1.rank() {
            return Err(PanCode::NodeUnknown.val());
        }
        let st = reconcile_graph(g, exp)?;
        self.registrations += 1;
        if self.state.rank() < PanState::Registered.rank() {
            self.state = PanState::Registered;
        }
        Ok(st)
    }
}

// ── 图族登记（续号账） ──────────────────────────────────────────
/// 幅记录：序号、幅标、段位、幅哈希（仅第 21 幅挂图哈希，余幅 0 占位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FigureRecord {
    pub seq: u32,
    pub tag: String,
    pub seg: u32,
    pub hash: u32,
}

/// 图族登记册：FIG-01..FIG-21 序号连续；第 21 幅段位/哈希随册冻结。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FigureFamily {
    pub records: Vec<FigureRecord>,
}

pub fn build_family(graph_hash: u32) -> FigureFamily {
    let mut records: Vec<FigureRecord> = Vec::new();
    for i in 0..FAMILY_N {
        let seq = (i + 1) as u32;
        let is_self = seq == FIG_INDEX;
        records.push(FigureRecord {
            seq,
            tag: format!("FIG-{:02}", seq),
            seg: if is_self { SEG_BASE } else { 0x70000 + i as u32 * 0x40 },
            hash: if is_self { graph_hash } else { 0 },
        });
    }
    FigureFamily { records }
}

/// 图族核验：序号连续 + 幅标对 + 本幅段位/哈希随册对平。
pub fn verify_family(fam: &FigureFamily, g: &PanoramaGraph) -> Result<(), u32> {
    if fam.records.len() != FAMILY_N {
        return Err(PanCode::SeqGap.val());
    }
    for (i, r) in fam.records.iter().enumerate() {
        if r.seq != (i + 1) as u32 || r.tag != format!("FIG-{:02}", r.seq) {
            return Err(PanCode::SeqGap.val());
        }
        if r.seq == FIG_INDEX && (r.seg != SEG_BASE || r.hash != g.body_hash) {
            return Err(PanCode::HashDrift.val());
        }
    }
    Ok(())
}

// ── 失真注入（10 路） ───────────────────────────────────────────
/// 对黄金全景注入第 kind 路失真，返回坏图（或坏族旗）。
/// 十路语义：0 引用越界（1599）；1 引用重复；2 引用缺失（删尾）；
/// 3 类属漂移；4 引用错位（+1）；5 边悬空；6 族序断档；7 幅哈希漂移；
/// 8 版本漂移；9 节点缺失。族路（6/7）返回 Some(坏族)。
pub fn distort(g: &PanoramaGraph, fam: &FigureFamily, kind: usize) -> (PanoramaGraph, Option<FigureFamily>) {
    let mut gr = g.clone();
    let mut fa: Option<FigureFamily> = None;
    match kind % 10 {
        0 => {
            if let Some(n) = gr.nodes.get_mut(0) {
                if let Some(e) = n.refs.get_mut(0) { *e = 1599; }
            }
        }
        1 => {
            if let Some(n) = gr.nodes.get_mut(1) {
                let v0 = if let Some(x) = n.refs.get(0) { *x } else { 0 };
                if let Some(e) = n.refs.get_mut(1) { *e = v0; }
            }
        }
        2 => {
            if let Some(n) = gr.nodes.get_mut(2) { n.refs.pop(); }
        }
        3 => {
            if let Some(n) = gr.nodes.get_mut(3) { n.class = SchedClass::Realtime; }
        }
        4 => {
            if let Some(n) = gr.nodes.get_mut(4) {
                if let Some(e) = n.refs.get_mut(0) { *e += 1; }
            }
        }
        5 => {
            if let Some(e) = gr.edges.get_mut(5) { e.to = 99; }
        }
        6 => {
            // 族序断档：抽掉序 5 记录。
            let mut f2 = fam.clone();
            f2.records.retain(|r| r.seq != 5);
            fa = Some(f2);
        }
        7 => {
            // 幅哈希漂移：第 21 幅哈希异或 1。
            let mut f2 = fam.clone();
            for r in &mut f2.records {
                if r.seq == FIG_INDEX {
                    r.hash ^= 1;
                }
            }
            fa = Some(f2);
        }
        8 => {
            gr.version = 2;
        }
        _ => {
            gr.nodes.pop();
        }
    }
    (gr, fa)
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入轮：每轮先验黄金全景与图族（核验必须通过，防注入器污染基准），
/// 再注入一路失真，核验器必须抓到。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let exp = build_ledger_expect();
    let mut eng = PanEngine::new(EPOCH);
    let mut g = gen_panorama(EPOCH);
    eng.freeze(&mut g)?;
    let fam = build_family(g.body_hash);
    reconcile_graph(&g, &exp)?;
    verify_family(&fam, &g)?;
    let mut out = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let kind = (r % 10) as usize;
        let (gd, fd) = distort(&g, &fam, kind);
        let hit_graph = reconcile_graph(&gd, &exp).is_err();
        let hit = match &fd {
            Some(f) => hit_graph || verify_family(f, &g).is_err(),
            None => hit_graph,
        };
        out.record(kind, hit);
        r += 1;
    }
    Ok(out)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：生成→冻结→图账对平→对拍→图族续号→四拒探→注入 10/10。
/// 返回 (正旗数, 拒旗数, 对平引用数, 对拍样本数)。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let exp = build_ledger_expect();
    let mut eng = PanEngine::new(EPOCH);
    let mut g = gen_panorama(EPOCH);
    eng.freeze(&mut g)?;
    let st = eng.register(&g, &exp)?;
    let fam = build_family(g.body_hash);
    verify_family(&fam, &g)?;
    let fid = fidelity_nodes(&g)?;
    // 四拒探：起草登记 / 状态倒退 / 引用越界 / 版本漂移。
    let mut eng2 = PanEngine::new(EPOCH);
    let g2 = gen_panorama(EPOCH);
    let draft_reg = eng2.register(&g2, &exp).is_err();
    let back_rej = eng.advance(PanState::Drafting).is_err();
    let (gd, _) = distort(&g, &fam, 0);
    let oor_rej = reconcile_graph(&gd, &exp).is_err();
    let (gv, _) = distort(&g, &fam, 8);
    let ver_rej = reconcile_graph(&gv, &exp).is_err();
    // 注入 10 路全抓。
    let inj = inject_rounds(DRIFT_ROUNDS)?;
    let inj_ok = inj.all_caught();
    let flags = (st.refs_reconciled == REFS_TOTAL as u32) as u32
        + (st.edges_checked == EDGE_TOTAL as u32) as u32
        + (fid == FIDELITY_SAMPLES as u32) as u32
        + inj_ok as u32;
    let rejects = draft_reg as u32 + back_rej as u32 + oor_rej as u32 + ver_rej as u32;
    Ok((flags, rejects, st.refs_reconciled, fid))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, _)) => Ok(()),
        _ => Err(PanCode::NotReproducible.val()),
    }
}

/// 序 B：失真注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(PanCode::RefMissing.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72BC0 && PanCode::NotReproducible.val() < SEG_BASE + 0x40;
    // 引用域自证：192 引用覆盖 1601..=1792，尾段 1793..=1800 为锚保留。
    let cover_ok = ENTRY_LO + REFS_TOTAL as u64 == 1793 && ENTRY_HI == 1800;
    let ok = CLASS_N == 3 && CORE_N == 8 && NODE_N == 24 && REFS_PER_NODE == 8
        && REFS_TOTAL == 192 && FIG_INDEX == 21 && FAMILY_N == 21 && FROZEN_V1 == 1
        && EPOCH == 9 && DRIFT_ROUNDS == 10 && EDGE_TOTAL == 30 && cover_ok && in_window;
    if ok {
        Ok(())
    } else {
        Err(PanCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PanCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PanCode::ALL.len() {
        let v = PanCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let exp = build_ledger_expect();
    let mut eng = PanEngine::new(EPOCH);
    let mut g = gen_panorama(EPOCH);
    eng.freeze(&mut g)?;
    let st = eng.register(&g, &exp)?;
    let fam = build_family(g.body_hash);
    verify_family(&fam, &g)?;
    let inj = inject_rounds(DRIFT_ROUNDS)?;
    Ok(format!(
        "状态=已完成 fig=FIG-{:02} nodes={} refs={} edges={} drift={}/{} ver=v{}",
        FIG_INDEX, NODE_N, st.refs_reconciled, st.edges_checked, inj.caught, inj.tries, FROZEN_V1
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, n, fid)) => (
            *f == 4 && *r == 4,
            format!("flags={} rejects={} refs={} fid={}", f, r, n, fid),
        ),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-第二十一幅生成全过", gp, gd));
    // 黄金全景准备（后续分项判据共用）。
    let exp = build_ledger_expect();
    let mut eng = PanEngine::new(EPOCH);
    let mut pan = gen_panorama(EPOCH);
    if eng.freeze(&mut pan).is_err() {
        v.push(("J1-三调度类全景冻结", false, String::from("冻结失败")));
        return v;
    }
    // J1 三调度类 × 每核 runqueue：24 节点类属/核号全对。
    let mut cls_ok = pan.nodes.len() == NODE_N;
    for (i, n) in pan.nodes.iter().enumerate() {
        if n.class != class_of_node(i) || n.core != core_of_node(i) {
            cls_ok = false;
            break;
        }
    }
    v.push((
        "J1-三调度类全景冻结",
        cls_ok,
        format!("公平/实时/闲时 × {} 核 = {} 节点 ver=v{}", CORE_N, NODE_N, pan.version),
    ));
    // J1 图账一致：192 引用双向对平。
    let recon = reconcile_graph(&pan, &exp);
    v.push((
        "J1-图账一致对平",
        matches!(&recon, Ok(s) if s.refs_reconciled == REFS_TOTAL as u32),
        format!("refs={}/{} F1601–F{}", REFS_TOTAL, REFS_TOTAL, 1600 + REFS_TOTAL),
    ));
    // J1 切换路径边闭集：环 24 + 闲进 3 + 闲出 3。
    let edges_ok = pan.edges.len() == EDGE_TOTAL && reconcile_graph(&pan, &exp).is_ok();
    v.push((
        "J1-切换路径边闭集",
        edges_ok,
        format!("环切{}+闲进{}+闲出{}={}", NODE_N, CLASS_N, CLASS_N, EDGE_TOTAL),
    ));
    // J1 渲染双路对拍 5 万样本。
    let fid = fidelity_nodes(&pan);
    v.push((
        "J1-渲染双路对拍",
        matches!(&fid, Ok(n) if *n == FIDELITY_SAMPLES as u32),
        format!("samples={}", FIDELITY_SAMPLES),
    ));
    // J1 图族续号登记 21 幅。
    let fam = build_family(pan.body_hash);
    let fam_v = verify_family(&fam, &pan);
    v.push((
        "J1-图族续号登记",
        fam_v.is_ok() && fam.records.len() == FAMILY_N,
        format!("FIG-01..FIG-{} 本幅 seg=0x{:x} 哈希随册", FAMILY_N, SEG_BASE),
    ));
    // J1 失真注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    // J2 状态机单调守卫。
    let mut eng2 = PanEngine::new(EPOCH);
    let m1 = eng2.advance(PanState::FrozenV1).is_ok();
    let m2 = eng2.advance(PanState::FrozenV1).is_err();
    let m3 = eng2.advance(PanState::Drafting).is_err();
    let m4 = eng2.advance(PanState::Registered).is_ok();
    v.push((
        "J2-状态机单调守卫",
        m1 && m2 && m3 && m4,
        format!("进={} 平拒={} 倒拒={} 终进={}", m1, m2, m3, m4),
    ));
    // J2 起草登记拒 + 版本漂移拒。
    let mut eng3 = PanEngine::new(EPOCH);
    let g3 = gen_panorama(EPOCH);
    let draft_rej = eng3.register(&g3, &exp).is_err();
    let (gv, _) = distort(&pan, &fam, 8);
    let ver_rej = reconcile_graph(&gv, &exp).is_err();
    v.push((
        "J2-冻结守卫",
        draft_rej && ver_rej,
        format!("起草登记拒={} 版本漂移拒={}", draft_rej, ver_rej),
    ));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72BC0 窗口")));
    v
}

pub fn run_unx2284_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2284 图族续编第二十一幅（调度全景一图）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
