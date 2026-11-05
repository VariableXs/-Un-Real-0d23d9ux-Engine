//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1526 批间依赖闭环核验（B31–B40 十批依赖图无悬空） · 0x78480
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡逐字锚定，AI-VX8-1104 · B02 · 卡 3）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F1526-J1 收官段依赖图闭环（十批依赖边逐边核验，
//!     悬空引用 10/10 必抓），环检测零环断言，依赖图可导出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 节点闭集：B31..=B40 十批（收官段），批号冻结，图内边只允许
//!     前向依赖（被依赖批号 < 依赖批号），前向序保证 DAG 可构造；
//!   · 冻结边表：b31 根批（无依赖），b32→31，b33→31，b34→32/33，
//!     b35→31/34，b36→33，b37→34/35，b38→36，b39→37/38，b40→35/39，
//!     合计 14 边逐边核验；
//!   · 悬空引用：边的任一端点不在节点闭集即 DanglingRef，注入
//!     10 轮 × 三路（悬空/回边/重边）全部必抓；
//!   · 零环断言：Kahn 入度拓扑排序全节点消解即零环，残余入度 > 0
//!     即 CycleFound；回边（to > from）前置拒（EdgeBack）；
//!   · 依赖图可导出：导出行按登记序拼装 "B<from>→B<to>;" 非空且
//!     行数 == 边数；
//!   · 双跑一致 + 段位守卫（0x78480 窗口）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78480，AI-VX8-1104 · B02 · 卡 3，派工卡逐字给定）。
pub const SEG_BASE: u32 = 0x78480;
/// 本批（B02 八卡）诊断段窗口下界。
pub const SEG_WIN_LO: u32 = 0x78400;
/// 本批（B02 八卡）诊断段窗口上界。
pub const SEG_WIN_HI: u32 = 0x785C0;

/// 节点批号下界（B31，冻结）。
pub const NODE_LO: u8 = 31;
/// 节点批号上界（B40，冻结）。
pub const NODE_HI: u8 = 40;
/// 节点数（十批，冻结）。
pub const NODE_COUNT: usize = 10;
/// 冻结边数（收官段依赖图，冻结 14）。
pub const EDGE_COUNT: usize = 14;
/// 悬空注入轮数（判据"10/10 必抓"）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口，0x78481..0x7848C） ───────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepCode {
    /// 批号越界（不在 B31..=B40）
    NodeOver,
    /// 自依赖边（from == to）
    EdgeSelf,
    /// 回边（to > from，破坏前向序）
    EdgeBack,
    /// 悬空引用（边端点不在节点闭集）
    DanglingRef,
    /// 环（拓扑排序未消解全节点）
    CycleFound,
    /// 重复边（同 from→to 已登记）
    DupEdge,
    /// 导出为空（依赖图必须可导出）
    ExportEmpty,
    /// 封图后改图（seal 之后仍尝试加边）
    Sealed,
    /// 图面缩水（注销/删边请求，图只增不减）
    GraphShrink,
    /// 双跑不一致（复现红线）
    NotReproducible,
    /// 覆盖不足（十批未逐批核验到）
    CoverageShort,
    /// 守卫失守（段位/闭集守卫失败）
    GuardBad,
}

impl DepCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DepCode::NodeOver => "批号越界：只在 B31..=B40 闭集",
            DepCode::EdgeSelf => "自依赖边：批不依赖自身",
            DepCode::EdgeBack => "回边：依赖只许前向（被依赖批号更小）",
            DepCode::DanglingRef => "悬空引用：边端点不在节点闭集，先补节点",
            DepCode::CycleFound => "环：拓扑排序有残余入度，拆环后重核",
            DepCode::DupEdge => "重复边：同 from→to 唯一",
            DepCode::ExportEmpty => "导出为空：依赖图必须可导出成行",
            DepCode::Sealed => "封图后改图：闭环封印，加边口已闭",
            DepCode::GraphShrink => "图面缩水：图只增不减，删边一律拒",
            DepCode::NotReproducible => "双跑不一致：核验摘要两跑应同值",
            DepCode::CoverageShort => "覆盖不足：十批逐批核验缺批",
            DepCode::GuardBad => "守卫失守：段位或闭集守卫失败，勿改窗口",
        }
    }
    pub const ALL: [DepCode; 12] = [
        DepCode::NodeOver,
        DepCode::EdgeSelf,
        DepCode::EdgeBack,
        DepCode::DanglingRef,
        DepCode::CycleFound,
        DepCode::DupEdge,
        DepCode::ExportEmpty,
        DepCode::Sealed,
        DepCode::GraphShrink,
        DepCode::NotReproducible,
        DepCode::CoverageShort,
        DepCode::GuardBad,
    ];
}

/// 诊断码查表（段外码返回统一文案，闭集返回）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DepCode::ALL.len() {
        if DepCode::ALL[i].val() == code {
            return DepCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x78480 段闭集内"
}

// ── 域模型：依赖边与依赖图 ───────────────────────────────────────
/// 依赖边：from 批依赖 to 批（前向序：to < from）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepEdge {
    /// 依赖方批号（31..=40）
    pub from: u8,
    /// 被依赖方批号（31..=40，且 < from）
    pub to: u8,
}

/// 批间依赖图（有向无环，节点闭集 B31..B40）。
///
/// 不变式：
///   G1: 边端点全在节点闭集（无悬空）；
///   G2: 全部边前向（to < from，零环的构造性保证）；
///   G3: 边唯一（同 from→to 不重）；
///   G4: 图只增不减，封图后加边口闭。
#[derive(Clone, Debug)]
pub struct DepGraph {
    edges: Vec<DepEdge>,
    pub total: u32,
    pub peak_total: u32,
    pub sealed: bool,
}

impl DepGraph {
    pub fn new() -> DepGraph {
        DepGraph { edges: Vec::new(), total: 0, peak_total: 0, sealed: false }
    }

    /// 节点合法性（批号闭集）。
    pub fn node_ok(b: u8) -> bool {
        b >= NODE_LO && b <= NODE_HI
    }

    /// 加边（逐边核验：越界/自依赖/回边/重边四查）。
    pub fn add_edge(&mut self, from: u8, to: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(DepCode::Sealed.val());
        }
        if !DepGraph::node_ok(from) || !DepGraph::node_ok(to) {
            return Err(DepCode::DanglingRef.val());
        }
        if from == to {
            return Err(DepCode::EdgeSelf.val());
        }
        if to > from {
            return Err(DepCode::EdgeBack.val());
        }
        let mut i = 0;
        while i < self.edges.len() {
            if let Some(e) = self.edges.get(i) {
                if e.from == from && e.to == to {
                    return Err(DepCode::DupEdge.val());
                }
            }
            i += 1;
        }
        self.edges.push(DepEdge { from, to });
        self.total += 1;
        self.peak_total = self.peak_total.max(self.total);
        Ok(())
    }

    /// 悬空核验：逐边查端点闭集。
    pub fn check_dangling(&self) -> Result<u32, u32> {
        let mut checked = 0u32;
        let mut i = 0;
        while i < self.edges.len() {
            if let Some(e) = self.edges.get(i) {
                if !DepGraph::node_ok(e.from) || !DepGraph::node_ok(e.to) {
                    return Err(DepCode::DanglingRef.val());
                }
                checked += 1;
            }
            i += 1;
        }
        Ok(checked)
    }

    /// 某批的依赖列表（被依赖方批号升序）。
    pub fn deps_of(&self, from: u8) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < self.edges.len() {
            if let Some(e) = self.edges.get(i) {
                if e.from == from {
                    v.push(e.to);
                }
            }
            i += 1;
        }
        v
    }

    /// 批覆盖核验：十批逐批出现（根批允许零依赖，但必须显性登记根标记）。
    /// roots：显性根批集（b31 一批）。
    pub fn check_coverage(&self, roots: &[u8]) -> Result<u32, u32> {
        let mut covered = 0u32;
        let mut b = NODE_LO;
        while b <= NODE_HI {
            let has_deps = !self.deps_of(b).is_empty();
            let mut is_root = false;
            let mut i = 0;
            while i < roots.len() {
                if let Some(r) = roots.get(i) {
                    if *r == b {
                        is_root = true;
                    }
                }
                i += 1;
            }
            if has_deps || is_root {
                covered += 1;
            }
            b += 1;
        }
        if covered != NODE_COUNT as u32 {
            return Err(DepCode::CoverageShort.val());
        }
        Ok(covered)
    }

    /// 零环断言（Kahn 入度拓扑排序）：
    ///   入度数组按边累计，逐轮消解入度为 0 的节点并把后继入度减 1，
    ///   消解数 == 节点数即零环，否则 CycleFound。
    pub fn check_acyclic(&self) -> Result<u32, u32> {
        // 先做悬空/回边前置核验。
        self.check_dangling()?;
        let mut indeg = [0u32; NODE_COUNT];
        let mut i = 0;
        while i < self.edges.len() {
            if let Some(e) = self.edges.get(i) {
                if e.to > e.from {
                    return Err(DepCode::EdgeBack.val());
                }
                // 依赖方（from）入度 +1：被依赖方（to）须先消解。
                let fi = (e.from - NODE_LO) as usize;
                let d = match indeg.get_mut(fi) {
                    Some(d) => d,
                    None => return Err(DepCode::NodeOver.val()),
                };
                *d = d.saturating_add(1);
            }
            i += 1;
        }
        let mut removed = [false; NODE_COUNT];
        let mut solved = 0u32;
        let mut pass = 0;
        while pass < NODE_COUNT {
            let mut k = 0;
            while k < NODE_COUNT {
                let idx = NODE_LO + k as u8;
                let already = match removed.get(k) {
                    Some(x) => *x,
                    None => true,
                };
                if already {
                    k += 1;
                    continue;
                }
                let deg = match indeg.get(k) {
                    Some(x) => *x,
                    None => 0,
                };
                if deg == 0 {
                    // 消解 idx：把所有 idx→j 边的 j 入度减 1。
                    let mut e2 = 0;
                    while e2 < self.edges.len() {
                        if let Some(e) = self.edges.get(e2) {
                            if e.to == idx {
                                let fi = (e.from - NODE_LO) as usize;
                                if let Some(d) = indeg.get_mut(fi) {
                                    *d = d.saturating_sub(1);
                                }
                            }
                        }
                        e2 += 1;
                    }
                    if let Some(x) = removed.get_mut(k) {
                        *x = true;
                    }
                    solved += 1;
                }
                k += 1;
            }
            pass += 1;
        }
        if solved != NODE_COUNT as u32 {
            return Err(DepCode::CycleFound.val());
        }
        Ok(solved)
    }

    /// 依赖图可导出：按登记序拼装 "B<from>→B<to>;" 行，非空且行数 == 边数。
    pub fn export_graph(&self) -> Result<(String, u32), u32> {
        if self.edges.is_empty() {
            return Err(DepCode::ExportEmpty.val());
        }
        let mut s = String::new();
        let mut lines = 0u32;
        let mut i = 0;
        while i < self.edges.len() {
            if let Some(e) = self.edges.get(i) {
                let line = format!("B{}→B{};", e.from, e.to);
                if line.is_empty() {
                    return Err(DepCode::ExportEmpty.val());
                }
                s.push_str(&line);
                lines += 1;
            }
            i += 1;
        }
        if lines != self.total {
            return Err(DepCode::ExportEmpty.val());
        }
        Ok((s, lines))
    }

    /// 删边请求：一律显性拒（图只增不减）。
    pub fn try_remove_edge(&self, _from: u8, _to: u8) -> Result<(), u32> {
        Err(DepCode::GraphShrink.val())
    }

    /// 封图（不可逆终态）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

// ── 冻结边表（收官段 B31–B40，14 边逐边给定） ───────────────────
/// 依赖对列表：冻结规格（b31 根批无依赖；其余逐批给定）。
pub fn spec_edges() -> Vec<(u8, u8)> {
    let mut v: Vec<(u8, u8)> = Vec::new();
    v.push((32, 31));
    v.push((33, 31));
    v.push((34, 32));
    v.push((34, 33));
    v.push((35, 31));
    v.push((35, 34));
    v.push((36, 33));
    v.push((37, 34));
    v.push((37, 35));
    v.push((38, 36));
    v.push((39, 37));
    v.push((39, 38));
    v.push((40, 35));
    v.push((40, 39));
    v
}

/// 显性根批集（b31 无依赖，作根标记）。
pub fn spec_roots() -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(NODE_LO);
    v
}

/// 按冻结规格建图。
pub fn build_graph() -> Result<DepGraph, u32> {
    let mut g = DepGraph::new();
    let spec = spec_edges();
    let mut i = 0;
    while i < spec.len() {
        if let Some((f, t)) = spec.get(i) {
            g.add_edge(*f, *t)?;
        }
        i += 1;
    }
    if g.total != EDGE_COUNT as u32 {
        return Err(DepCode::CoverageShort.val());
    }
    Ok(g)
}

// ── 核验摘要（供双跑一致对比） ──────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepSummary {
    pub edges: u32,
    pub dangling_checked: u32,
    pub acyclic_solved: u32,
    pub coverage: u32,
    pub export_lines: u32,
    pub digest: u32,
}

/// FNV-1a 摘要卷积（按登记序对边卷积）。
pub fn digest_edge(h: u32, e: &DepEdge) -> u32 {
    let mut x = h;
    x = x.wrapping_mul(16777619).wrapping_add(e.from as u32);
    x = x.wrapping_mul(16777619).wrapping_add(e.to as u32);
    x
}

/// 独立建图核验一轮：建图 → 逐边核验 → 零环 → 覆盖 → 导出。
pub fn run_verify_round() -> Result<DepSummary, u32> {
    let g = build_graph()?;
    let checked = g.check_dangling()?;
    let solved = g.check_acyclic()?;
    let roots = spec_roots();
    let cov = g.check_coverage(&roots)?;
    let (_txt, lines) = g.export_graph()?;
    let mut h: u32 = 2166136261;
    let spec = spec_edges();
    let mut i = 0;
    while i < spec.len() {
        if let Some((f, t)) = spec.get(i) {
            h = digest_edge(h, &DepEdge { from: *f, to: *t });
        }
        i += 1;
    }
    Ok(DepSummary {
        edges: g.total,
        dangling_checked: checked,
        acyclic_solved: solved,
        coverage: cov,
        export_lines: lines,
        digest: h,
    })
}

// ── LCG（确定性伪随机，种子固定） ───────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 悬空注入检出账（三路：悬空路/回边路/重边路） ─────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            if let Some(t) = self.tries.get_mut(slot) {
                *t += 1;
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c += 1;
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            let t = match self.tries.get(i) {
                Some(x) => *x,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 悬空注入：每轮打一路违约，逐边核验必须当场抓（10/10）。
/// 路 0：悬空引用——端点指向闭集外批（B30 与 B41 轮换）；
/// 路 1：回边——to > from（B31→B32 逆序）；
/// 路 2：重边——同 from→to 重复登记。
pub fn inject_dangling(rounds: u32) -> Result<InjectLedger, u32> {
    let mut acc = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let from = NODE_LO + rng.below(NODE_COUNT as u64) as u8;
        // 路 0：悬空引用（B30 / B41 / 越界批号）。
        let mut g0 = DepGraph::new();
        let bad_lo = NODE_LO.saturating_sub(1);
        let bad_hi = NODE_HI.saturating_add(1);
        let r0a = g0.add_edge(from, bad_lo);
        let r0b = g0.add_edge(from, bad_hi);
        acc.record(0, r0a == Err(DepCode::DanglingRef.val()) && r0b == Err(DepCode::DanglingRef.val()));
        // 路 1：回边（B31 → B32 逆序必拒；自依赖对照必拒）。
        let mut g1 = DepGraph::new();
        let r1a = g1.add_edge(NODE_LO, NODE_LO + 1);
        let r1b = g1.add_edge(NODE_LO, NODE_LO);
        acc.record(1, r1a == Err(DepCode::EdgeBack.val()) && r1b == Err(DepCode::EdgeSelf.val()));
        // 路 2：重边（同一条登记两次；from 避开 B31 以免撞自依赖口径）。
        let from2 = if from == NODE_LO { NODE_LO + 1 } else { from };
        let mut g2 = DepGraph::new();
        let _ = g2.add_edge(from2, NODE_LO);
        let r2 = g2.add_edge(from2, NODE_LO);
        acc.record(2, r2 == Err(DepCode::DupEdge.val()));
        r += 1;
    }
    Ok(acc)
}

// ── 黄金全链：建图 14 边 → 逐边核验 → 零环 → 覆盖 → 导出 → 四拒 ─
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut g = DepGraph::new();
    let spec = spec_edges();
    let mut i = 0;
    while i < spec.len() {
        if let Some((f, t)) = spec.get(i) {
            g.add_edge(*f, *t)?;
        }
        i += 1;
    }
    let checked = g.check_dangling()?;
    let solved = g.check_acyclic()?;
    let roots = spec_roots();
    let cov = g.check_coverage(&roots)?;
    let (txt, lines) = g.export_graph()?;
    if txt.is_empty() || lines != EDGE_COUNT as u32 {
        return Err(DepCode::ExportEmpty.val());
    }
    // 四拒口径：删边拒 / 重边拒 / 回边拒 / 越界拒。
    let shrink = g.try_remove_edge(32, 31) == Err(DepCode::GraphShrink.val());
    let dup = g.add_edge(32, 31) == Err(DepCode::DupEdge.val());
    let back = g.add_edge(NODE_LO, NODE_LO + 1) == Err(DepCode::EdgeBack.val());
    let over = g.add_edge(NODE_HI + 1, NODE_LO) == Err(DepCode::DanglingRef.val());
    // 封图后再加边：拒（Sealed）。
    g.seal();
    let sealed_rej = g.add_edge(40, 31).is_err();
    let flags = shrink as u32 + dup as u32 + back as u32 + over as u32;
    Ok((flags + sealed_rej as u32, checked, cov * 100 + solved * 10 + lines as u32 % 10))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金全链 + 双跑一致。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((5, val, _)) if val == EDGE_COUNT as u32 => {
            let a = run_verify_round()?;
            let b = run_verify_round()?;
            if a == b {
                Ok(())
            } else {
                Err(DepCode::NotReproducible.val())
            }
        }
        _ => Err(DepCode::NotReproducible.val()),
    }
}

/// 序 B：注入面三路（悬空/回边/重边）逐条显性抓。
pub fn probe_seq_b() -> Result<(), u32> {
    let inj = inject_dangling(JUDGE_ROUNDS)?;
    if inj.all_caught() {
        Ok(())
    } else {
        Err(DepCode::DanglingRef.val())
    }
}

/// 序 C：冻结常量 + 冻结边表对账 + 段位守卫。
pub fn probe_seq_c() -> Result<(), u32> {
    let spec = spec_edges();
    let mut spec_ok = spec.len() == EDGE_COUNT;
    let mut i = 0;
    while i < spec.len() {
        if let Some((f, t)) = spec.get(i) {
            if !DepGraph::node_ok(*f) || !DepGraph::node_ok(*t) || t >= f {
                spec_ok = false;
            }
        }
        i += 1;
    }
    match (
        NODE_LO == 31,
        NODE_HI == 40,
        NODE_COUNT == 10,
        EDGE_COUNT == 14,
        JUDGE_ROUNDS == 10,
        spec_ok,
        segment_guard(),
    ) {
        (true, true, true, true, true, true, true) => Ok(()),
        _ => Err(DepCode::GuardBad.val()),
    }
}

/// 段位守卫：诊断码全部落在本卡 0x78480 窗口内 + 批窗口对齐。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DepCode::ALL.len() {
        let v = DepCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    let align = SEG_BASE % 0x40 == 0;
    let in_win = SEG_BASE >= SEG_WIN_LO && SEG_BASE <= SEG_WIN_HI;
    align && in_win
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = run_verify_round()?;
    let b = run_verify_round()?;
    if a != b || a.digest != b.digest {
        return Err(DepCode::NotReproducible.val());
    }
    Ok(())
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let sum = run_verify_round()?;
    if sum.edges != EDGE_COUNT as u32 || sum.acyclic_solved != NODE_COUNT as u32 {
        return Err(DepCode::CoverageShort.val());
    }
    Ok(format!(
        "状态=已完成 edges={} solved={} cov={} export_lines={} digest=0x{:x}",
        sum.edges, sum.acyclic_solved, sum.coverage, sum.export_lines, sum.digest
    ))
}

// ── 判据面（extra_checks：13 条动态明细） ───────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. J1 主判据：收官段依赖图闭环（14 边逐边核验）。
    let g = golden_run();
    v.push((
        "J1-依赖图闭环",
        match &g {
            Ok((flags, checked, _)) => *flags == 5 && *checked == EDGE_COUNT as u32,
            Err(_) => false,
        },
        match &g {
            Ok((flags, checked, extra)) => format!("flags={} checked={} extra={}", flags, checked, extra),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 2. J1 悬空注入 10/10。
    let inj = inject_dangling(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-悬空注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 3. J1 零环断言（Kahn 全节点消解）。
    let acyclic = {
        let g2 = build_graph();
        match g2 {
            Ok(gg) => gg.check_acyclic() == Ok(NODE_COUNT as u32),
            Err(_) => false,
        }
    };
    v.push((
        "J1-零环断言",
        acyclic,
        String::from("Kahn 拓扑十节点全消解"),
    ));
    // 4. J1 依赖图可导出（行数 == 14，非空）。
    let export = {
        let g3 = build_graph();
        match g3 {
            Ok(gg) => match gg.export_graph() {
                Ok((txt, lines)) => !txt.is_empty() && lines == EDGE_COUNT as u32,
                Err(_) => false,
            },
            Err(_) => false,
        }
    };
    v.push((
        "J1-依赖图可导出",
        export,
        String::from("14 行 B<from>→B<to>; 非空可导"),
    ));
    // 5. 覆盖核验：十批逐批核验到（根批显性标记）。
    let cov = {
        let g4 = build_graph();
        match g4 {
            Ok(gg) => {
                let roots = spec_roots();
                gg.check_coverage(&roots) == Ok(NODE_COUNT as u32)
            }
            Err(_) => false,
        }
    };
    v.push((
        "J1-十批覆盖",
        cov,
        String::from("B31..B40 逐批有依赖或显性根标记"),
    ));
    // 6. 边界：批号闭集 B31..B40，B30/B41 悬空拒。
    let node_edge = {
        let mut g5 = DepGraph::new();
        let r30 = g5.add_edge(32, NODE_LO.saturating_sub(1));
        let r41 = g5.add_edge(32, NODE_HI.saturating_add(1));
        let rok = g5.add_edge(NODE_HI, NODE_LO);
        r30 == Err(DepCode::DanglingRef.val())
            && r41 == Err(DepCode::DanglingRef.val())
            && rok.is_ok()
    };
    v.push((
        "边界-批号闭集",
        node_edge,
        String::from("B30/B41 悬空拒，B40→B31 前向收"),
    ));
    // 7. 边界：自依赖与回边前置拒。
    let dir_edge = {
        let mut g6 = DepGraph::new();
        let rself = g6.add_edge(NODE_LO, NODE_LO);
        let rback = g6.add_edge(NODE_LO, NODE_HI);
        rself == Err(DepCode::EdgeSelf.val()) && rback == Err(DepCode::EdgeBack.val())
    };
    v.push((
        "边界-自依赖与回边",
        dir_edge,
        String::from("自依赖拒，回边（31→40）拒"),
    ));
    // 8. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 9. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("run_verify_round 两跑摘要同值"),
    ));
    // 10. 只增不减：删边路径一律显性拒。
    let mono = {
        let g7 = build_graph();
        match g7 {
            Ok(gg) => gg.try_remove_edge(32, 31) == Err(DepCode::GraphShrink.val()),
            Err(_) => false,
        }
    };
    v.push((
        "J2-图只增不减",
        mono,
        String::from("删边请求一律拒（GraphShrink）"),
    ));
    // 11. 封图冻结：封后加边全拒，账面不动。
    let sealed = {
        let mut g8 = DepGraph::new();
        let _ = g8.add_edge(32, 31);
        let before = g8.total;
        g8.seal();
        let _ = g8.add_edge(33, 31);
        g8.sealed && g8.total == before
    };
    v.push((
        "J2-封图冻结",
        sealed,
        String::from("封图后加边口闭，账面不动"),
    ));
    // 12. 写回摘要非空。
    let wb = writeback_summary();
    v.push((
        "J3-写回摘要",
        match &wb {
            Ok(s) => !s.is_empty(),
            Err(_) => false,
        },
        match &wb {
            Ok(s) => s.clone(),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 13. 段位守卫 + 诊断闭集查表。
    let guard = {
        let mut closed_ok = true;
        let mut i = 0;
        while i < DepCode::ALL.len() {
            let code = DepCode::ALL[i].val();
            if detail(code) == "未知诊断码：不在 0x78480 段闭集内" {
                closed_ok = false;
            }
            i += 1;
        }
        closed_ok && segment_guard()
    };
    v.push((
        "J4-段位守卫",
        guard,
        String::from("0x78480 窗口 + 12 码闭集查表"),
    ));
    v
}

pub fn run_unx1526_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1526 批间依赖闭环核验（B31–B40 十批依赖图无悬空）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_spec_edges_count_and_direction() {
        let spec = spec_edges();
        assert_eq!(spec.len(), EDGE_COUNT);
        let mut i = 0;
        while i < spec.len() {
            if let Some((f, t)) = spec.get(i) {
                assert!(DepGraph::node_ok(*f));
                assert!(DepGraph::node_ok(*t));
                assert!(t < f, "前向序：B{}→B{}", f, t);
            }
            i += 1;
        }
    }

    #[test]
    fn t02_build_graph_and_full_verify() {
        match golden_run() {
            Ok((flags, checked, _)) => {
                assert_eq!(flags, 5);
                assert_eq!(checked, EDGE_COUNT as u32);
            }
            Err(e) => assert!(false, "黄金全链应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t03_dangling_inject_10_of_10_caught() {
        match inject_dangling(JUDGE_ROUNDS) {
            Ok(inj) => {
                assert_eq!(inj.tries, [10, 10, 10]);
                assert_eq!(inj.caught, [10, 10, 10]);
                assert!(inj.all_caught());
            }
            Err(e) => assert!(false, "注入应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t04_acyclic_zero_cycles() {
        let g = build_graph();
        assert!(g.is_ok());
        if let Ok(gg) = g {
            assert_eq!(gg.check_acyclic(), Ok(NODE_COUNT as u32));
        }
    }

    #[test]
    fn t05_export_non_empty_line_count() {
        let g = build_graph();
        assert!(g.is_ok());
        if let Ok(gg) = g {
            match gg.export_graph() {
                Ok((txt, lines)) => {
                    assert!(!txt.is_empty());
                    assert_eq!(lines, EDGE_COUNT as u32);
                    assert!(txt.starts_with("B32→B31;"));
                }
                Err(e) => assert!(false, "导出应成: err=0x{:x}", e),
            }
        }
    }

    #[test]
    fn t06_dangling_ref_rejected() {
        let mut g = DepGraph::new();
        assert_eq!(g.add_edge(32, 30), Err(DepCode::DanglingRef.val()));
        assert_eq!(g.add_edge(32, 41), Err(DepCode::DanglingRef.val()));
        assert_eq!(g.add_edge(30, 31), Err(DepCode::DanglingRef.val()));
        assert_eq!(g.add_edge(41, 40), Err(DepCode::DanglingRef.val()));
    }

    #[test]
    fn t07_self_and_back_edges_rejected() {
        let mut g = DepGraph::new();
        assert_eq!(g.add_edge(31, 31), Err(DepCode::EdgeSelf.val()));
        assert_eq!(g.add_edge(31, 32), Err(DepCode::EdgeBack.val()));
        assert_eq!(g.add_edge(39, 40), Err(DepCode::EdgeBack.val()));
        assert!(g.add_edge(40, 39).is_ok());
    }

    #[test]
    fn t08_dup_edge_rejected() {
        let mut g = DepGraph::new();
        assert!(g.add_edge(32, 31).is_ok());
        assert_eq!(g.add_edge(32, 31), Err(DepCode::DupEdge.val()));
    }

    #[test]
    fn t09_double_run_identical() {
        let a = run_verify_round();
        let b = run_verify_round();
        match (a, b) {
            (Ok(x), Ok(y)) => assert_eq!(x, y),
            _ => assert!(false, "双跑应成"),
        }
    }

    #[test]
    fn t10_seal_and_shrink_rejected() {
        let mut g = DepGraph::new();
        let _ = g.add_edge(32, 31);
        g.seal();
        assert_eq!(g.add_edge(33, 31), Err(DepCode::Sealed.val()));
        assert_eq!(g.try_remove_edge(32, 31), Err(DepCode::GraphShrink.val()));
        assert_eq!(g.total, 1);
    }

    #[test]
    fn t11_segment_guard_and_closed_set() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(DepCode::ALL.len(), 12);
        for c in DepCode::ALL {
            assert_ne!(detail(c.val()), "未知诊断码：不在 0x78480 段闭集内");
        }
    }

    #[test]
    fn t12_probe_checks_all_green() {
        let set = run_unx1526_checks();
        assert_eq!(set.failed_count(), 0);
        assert!(set.all_passed());
    }
}
