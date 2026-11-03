//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1488 · 断言依赖序登记（执行序拓扑冻结） · 0x75C40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批5-K）：
//!   断言间依赖拓扑三态登记（前置/互斥/独立），拓扑无环断言，序执行
//!   演练 10/10 按拓扑序完成，环依赖注入（A→B→A）10/10 必抓
//! 设计要点：
//!   1. 三态闭集：前置（Prereq，必须有先于后）/ 互斥（Mutex，不得同
//!      波执行）/ 独立（Independent，无约束）三态一张边表登记；
//!   2. 拓扑无环：Kahn 算法只在前置边上跑入度归零，若可排节点耗尽而
//!      未排满 ⇒ 环 ⇒ CycleDetected 显性拒；同级取最小 id 保证确定序；
//!   3. 层级账：拓扑序逐节点落层级（level = 前置父最大层 + 1），互斥
//!      对不同层级即不同波，同层互斥对 ⇒ MutexConflict 显性拒；
//!   4. 序执行演练：10 断言语料按拓扑序逐个执行，执行前逐条复核其
//!      全部前置父已执行（演练即席复核，不信任排序器），10/10 完成；
//!   5. 登记簿状态机：Open → Sealed 单向冻结，封账后登记显性拒
//!      （SealedLocked），倒退无门；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75C40，AI-VX7-1104 批5-K 独占槽）。
pub const SEG_BASE: u32 = 0x75C40;

// ── 冻结口径 ────────────────────────────────────────────────────
/// 语料断言数（冻结 10：演练 10/10 语义）。
pub const CORPUS_N: usize = 10;
/// 节点 id 下界（1 起编号）。
pub const NODE_ID_MIN: u32 = 1;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TopoCode {
    /// 边端点引用未登记节点
    UnknownNode,
    /// 节点重复登记
    DuplicateNode,
    /// 拓扑有环（前置依赖成环）
    CycleDetected,
    /// 互斥对同层（同波冲突）
    MutexConflict,
    /// 封账后登记（状态机倒退显性拒）
    SealedLocked,
    /// 拓扑序破坏（后置先于前置执行）
    TopoOrderBreach,
    /// 演练漏执行（有断言未完成）
    DrillLeak,
    /// 双跑不一致
    NotReproducible,
}

impl TopoCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TopoCode::UnknownNode => "边端点未登记：建议先登节点再连边，杜绝悬空引用",
            TopoCode::DuplicateNode => "节点重复登记：建议登记前按 id 查重，重复即拒",
            TopoCode::CycleDetected => "拓扑有环：前置依赖成环无法排序，建议拆环或改独立边",
            TopoCode::MutexConflict => "互斥对同层：两断言被排在同波，建议拉开层级或改独立边",
            TopoCode::SealedLocked => "封账后登记：登记簿已冻结，建议新开一代登记簿再登记",
            TopoCode::TopoOrderBreach => "拓扑序破坏：后置断言先于前置执行，建议按拓扑序重排",
            TopoCode::DrillLeak => "演练漏执行：有断言未完成，建议核对演练步数与节点数",
            TopoCode::NotReproducible => "双跑不一致：复现红线，建议固定种子后重跑两遍对账",
        }
    }
    pub const ALL: [TopoCode; 8] = [
        TopoCode::UnknownNode,
        TopoCode::DuplicateNode,
        TopoCode::CycleDetected,
        TopoCode::MutexConflict,
        TopoCode::SealedLocked,
        TopoCode::TopoOrderBreach,
        TopoCode::DrillLeak,
        TopoCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回；段外码显性报未知）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TopoCode::ALL.len() {
        if let Some(c) = TopoCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75C40 段闭集内"
}

// ── 依赖边三态闭集 ──────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// 前置：from 必须先于 to 完成
    Prereq,
    /// 互斥：from 与 to 不得同波执行
    Mutex,
    /// 独立：无执行序约束
    Independent,
}

impl EdgeKind {
    pub fn val(self) -> u32 {
        self as u32
    }
    pub const ALL: [EdgeKind; 3] = [EdgeKind::Prereq, EdgeKind::Mutex, EdgeKind::Independent];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsrtNode {
    pub id: u32,
    pub seg: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub from: u32,
    pub to: u32,
    pub kind: EdgeKind,
}

// ── 登记簿（状态机 Open → Sealed 单向冻结） ─────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegPhase {
    /// 开放登记
    Open,
    /// 已封账（拓扑冻结）
    Sealed,
}

#[derive(Clone, Debug, Default)]
pub struct TopoRegistry {
    pub nodes: Vec<AsrtNode>,
    pub edges: Vec<Edge>,
    pub phase: Option<RegPhase>,
    pub seal_count: u32,
}

impl TopoRegistry {
    pub fn new() -> TopoRegistry {
        TopoRegistry { nodes: Vec::new(), edges: Vec::new(), phase: Some(RegPhase::Open), seal_count: 0 }
    }

    fn ensure_open(&self) -> Result<(), u32> {
        match self.phase {
            Some(RegPhase::Open) => Ok(()),
            _ => Err(TopoCode::SealedLocked.val()),
        }
    }

    /// 登记节点（封账前；id 查重）。
    pub fn add_node(&mut self, id: u32, seg: u32) -> Result<(), u32> {
        self.ensure_open()?;
        if id < NODE_ID_MIN {
            return Err(TopoCode::UnknownNode.val());
        }
        let mut i = 0;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.id == id {
                    return Err(TopoCode::DuplicateNode.val());
                }
            }
            i += 1;
        }
        self.nodes.push(AsrtNode { id, seg });
        Ok(())
    }

    /// 登记边（三态；端点必须在簿）。
    pub fn add_edge(&mut self, from: u32, to: u32, kind: EdgeKind) -> Result<(), u32> {
        self.ensure_open()?;
        if !self.has_node(from) || !self.has_node(to) {
            return Err(TopoCode::UnknownNode.val());
        }
        self.edges.push(Edge { from, to, kind });
        Ok(())
    }

    fn has_node(&self, id: u32) -> bool {
        let mut i = 0;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.id == id {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 封账（单向：Open → Sealed，倒退无门）。
    pub fn seal(&mut self) -> Result<(), u32> {
        self.ensure_open()?;
        self.phase = Some(RegPhase::Sealed);
        self.seal_count = self.seal_count.saturating_add(1);
        Ok(())
    }
}

// ── Kahn 拓扑排序（前置边；同级取最小 id；返回（序，层）双账） ──
#[derive(Clone, Debug, Default)]
pub struct TopoResult {
    /// 拓扑序（节点 id 按执行次序）。
    pub order: Vec<u32>,
    /// 每节点层级（与 order 同序）。
    pub levels: Vec<u32>,
}

pub fn topo_sort(reg: &TopoRegistry) -> Result<TopoResult, u32> {
    let n = reg.nodes.len();
    if n == 0 {
        return Ok(TopoResult::default());
    }
    // 入度与邻接（前置边 only），下标与 reg.nodes 对齐。
    let mut indeg: Vec<usize> = Vec::new();
    let mut parents: Vec<Vec<u32>> = Vec::new();
    let mut k = 0;
    while k < n {
        indeg.push(0usize);
        parents.push(Vec::new());
        k += 1;
    }
    let mut e = 0;
    while e < reg.edges.len() {
        if let Some(edge) = reg.edges.get(e) {
            if edge.kind == EdgeKind::Prereq {
                // from 是 to 的前置父：to 入度 +1，from 记入 to 的父表。
                let mut ti = None;
                let mut fi = None;
                let mut k = 0;
                while k < n {
                    if let Some(nd) = reg.nodes.get(k) {
                        if nd.id == edge.to {
                            ti = Some(k);
                        }
                        if nd.id == edge.from {
                            fi = Some(k);
                        }
                    }
                    k += 1;
                }
                match (ti, fi) {
                    (Some(t), Some(f)) => {
                        if let Some(slot) = indeg.get_mut(t) {
                            *slot = slot.saturating_add(1);
                        }
                        let fid = match reg.nodes.get(f) {
                            Some(x) => x.id,
                            None => 0,
                        };
                        if let Some(slot) = parents.get_mut(t) {
                            slot.push(fid);
                        }
                    }
                    _ => return Err(TopoCode::UnknownNode.val()),
                }
            }
        }
        e += 1;
    }
    // Kahn：每轮取入度 0 的最小 id（确定序）。
    let mut done: Vec<bool> = Vec::new();
    let mut level_of: Vec<u32> = Vec::new();
    let mut k = 0;
    while k < n {
        done.push(false);
        level_of.push(0);
        k += 1;
    }
    let mut order: Vec<u32> = Vec::new();
    let mut levels: Vec<u32> = Vec::new();
    let mut steps = 0usize;
    while steps < n {
        // 找最小 id 的可排节点。
        let mut pick: Option<usize> = None;
        let mut k = 0;
        while k < n {
            if let (Some(d), Some(ind)) = (done.get(k), indeg.get(k)) {
                if !*d && *ind == 0 {
                    match pick {
                        None => pick = Some(k),
                        Some(p) => {
                            let pid = match reg.nodes.get(p) {
                                Some(x) => x.id,
                                None => 0,
                            };
                            let kid = match reg.nodes.get(k) {
                                Some(x) => x.id,
                                None => 0,
                            };
                            if kid < pid {
                                pick = Some(k);
                            }
                        }
                    }
                }
            }
            k += 1;
        }
        let p = match pick {
            Some(p) => p,
            None => return Err(TopoCode::CycleDetected.val()),
        };
        // 层级 = 前置父最大层 + 1。
        let mut lv = 0u32;
        if let Some(pa) = parents.get(p) {
            let mut j = 0;
            while j < pa.len() {
                if let Some(pid) = pa.get(j) {
                    let mut q = 0;
                    while q < n {
                        if let Some(nd) = reg.nodes.get(q) {
                            if nd.id == *pid {
                                if let Some(l) = level_of.get(q) {
                                    if *l + 1 > lv {
                                        lv = *l + 1;
                                    }
                                }
                            }
                        }
                        q += 1;
                    }
                }
                j += 1;
            }
        }
        // 消边：所有以本节点为父的后继入度 -1。
        let nid = match reg.nodes.get(p) {
            Some(x) => x.id,
            None => 0,
        };
        let mut k = 0;
        while k < n {
            if let Some(pa) = parents.get(k) {
                let mut has = false;
                let mut j = 0;
                while j < pa.len() {
                    if let Some(x) = pa.get(j) {
                        if *x == nid {
                            has = true;
                        }
                    }
                    j += 1;
                }
                if has {
                    if let Some(ind) = indeg.get_mut(k) {
                        *ind = ind.saturating_sub(1);
                    }
                }
            }
            k += 1;
        }
        if let Some(d) = done.get_mut(p) {
            *d = true;
        }
        if let Some(l) = level_of.get_mut(p) {
            *l = lv;
        }
        order.push(nid);
        levels.push(lv);
        steps += 1;
    }
    if order.len() != n {
        return Err(TopoCode::CycleDetected.val());
    }
    Ok(TopoResult { order, levels })
}

/// 互斥对同层检查（拓扑成功后跑）：同层互斥 ⇒ MutexConflict。
pub fn mutex_check(reg: &TopoRegistry, topo: &TopoResult) -> Result<(), u32> {
    let mut e = 0;
    while e < reg.edges.len() {
        if let Some(edge) = reg.edges.get(e) {
            if edge.kind == EdgeKind::Mutex {
                let la = level_of(topo, edge.from);
                let lb = level_of(topo, edge.to);
                if la == lb {
                    return Err(TopoCode::MutexConflict.val());
                }
            }
        }
        e += 1;
    }
    Ok(())
}

/// 查节点层级（order 中找不到返回 0 层）。
pub fn level_of(topo: &TopoResult, id: u32) -> u32 {
    let mut i = 0;
    while i < topo.order.len() {
        if let Some(x) = topo.order.get(i) {
            if *x == id {
                if let Some(l) = topo.levels.get(i) {
                    return *l;
                }
            }
        }
        i += 1;
    }
    0
}

// ── 序执行演练（拓扑序逐个执行，执行前复核前置父全完成） ────────
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrillReport {
    /// 已执行节点 id（按执行序）。
    pub executed: Vec<u32>,
    /// 执行序合法（全部前置父先于后置完成）。
    pub order_ok: bool,
    /// 全部节点完成（10/10 语义）。
    pub complete: bool,
}

pub fn drill_run(reg: &TopoRegistry) -> Result<DrillReport, u32> {
    let topo = topo_sort(reg)?;
    let n = reg.nodes.len();
    let mut done_ids: Vec<u32> = Vec::new();
    let mut order_ok = true;
    let mut i = 0;
    while i < topo.order.len() {
        let nid = match topo.order.get(i) {
            Some(x) => *x,
            None => break,
        };
        // 复核：本节点全部前置父必须已在 done_ids。
        let mut e = 0;
        while e < reg.edges.len() {
            if let Some(edge) = reg.edges.get(e) {
                if edge.kind == EdgeKind::Prereq && edge.to == nid {
                    let mut found = false;
                    let mut j = 0;
                    while j < done_ids.len() {
                        if let Some(x) = done_ids.get(j) {
                            if *x == edge.from {
                                found = true;
                            }
                        }
                        j += 1;
                    }
                    if !found {
                        order_ok = false;
                    }
                }
            }
            e += 1;
        }
        done_ids.push(nid);
        i += 1;
    }
    if done_ids.len() != n {
        return Err(TopoCode::DrillLeak.val());
    }
    Ok(DrillReport { executed: done_ids, order_ok, complete: true })
}

// ── 10 断言机器语料（冻结：前置链 + 互斥对 + 独立边） ────────────
/// 前置边冻结表：(from, to)。
pub const PREREQ_EDGES: [(u32, u32); 10] = [
    (1, 3),
    (2, 3),
    (3, 5),
    (4, 5),
    (4, 6),
    (5, 7),
    (6, 7),
    (7, 9),
    (8, 9),
    (9, 10),
];
/// 互斥对冻结表（层级必须错开）。
pub const MUTEX_EDGES: [(u32, u32); 3] = [(1, 7), (2, 5), (6, 8)];
/// 独立边冻结表（无约束，登记备查）。
pub const INDEPENDENT_EDGES: [(u32, u32); 3] = [(1, 2), (4, 8), (8, 6)];

/// 机器语料登记簿（10 节点，登记后封账）。
pub fn corpus_registry() -> Result<TopoRegistry, u32> {
    let mut reg = TopoRegistry::new();
    let mut id = NODE_ID_MIN;
    while id < (CORPUS_N as u32) + NODE_ID_MIN {
        reg.add_node(id, SEG_BASE)?;
        id += 1;
    }
    let mut i = 0;
    while i < PREREQ_EDGES.len() {
        if let Some((a, b)) = PREREQ_EDGES.get(i) {
            reg.add_edge(*a, *b, EdgeKind::Prereq)?;
        }
        i += 1;
    }
    let mut i = 0;
    while i < MUTEX_EDGES.len() {
        if let Some((a, b)) = MUTEX_EDGES.get(i) {
            reg.add_edge(*a, *b, EdgeKind::Mutex)?;
        }
        i += 1;
    }
    let mut i = 0;
    while i < INDEPENDENT_EDGES.len() {
        if let Some((a, b)) = INDEPENDENT_EDGES.get(i) {
            reg.add_edge(*a, *b, EdgeKind::Independent)?;
        }
        i += 1;
    }
    reg.seal()?;
    Ok(reg)
}

// ── LCG（种子写死 = SEG_BASE） ─────────────────────────────────
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

// ── 注入检出账（三路：环依赖路 / 互斥同波路 / 封账倒退路） ──────
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
                *t = t.saturating_add(1);
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t == 0 || c != t {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 环依赖登记簿（A→B→A 前置双边，注入用）。
pub fn cycle_registry(a: u32, b: u32) -> Result<TopoRegistry, u32> {
    let mut reg = TopoRegistry::new();
    reg.add_node(a, SEG_BASE)?;
    reg.add_node(b, SEG_BASE)?;
    reg.add_edge(a, b, EdgeKind::Prereq)?;
    reg.add_edge(b, a, EdgeKind::Prereq)?;
    reg.seal()?;
    Ok(reg)
}

/// 互斥同波登记簿（两节点同层 + 互斥边，注入用）。
pub fn mutex_clash_registry(a: u32, b: u32) -> Result<TopoRegistry, u32> {
    let mut reg = TopoRegistry::new();
    reg.add_node(a, SEG_BASE)?;
    reg.add_node(b, SEG_BASE)?;
    reg.add_edge(a, b, EdgeKind::Mutex)?;
    reg.seal()?;
    Ok(reg)
}

// ── 黄金全链：语料登记→封账→拓扑→互斥→演练 10/10→注入三路必抓 ──
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let reg = corpus_registry()?;
    let topo = topo_sort(&reg)?;
    if topo.order.len() != CORPUS_N {
        return Err(TopoCode::DrillLeak.val());
    }
    // 拓扑序合法性独立复核：每条前置边 from 位置 < to 位置。
    let mut e = 0;
    while e < reg.edges.len() {
        if let Some(edge) = reg.edges.get(e) {
            if edge.kind == EdgeKind::Prereq {
                let mut pf = None;
                let mut pt = None;
                let mut i = 0;
                while i < topo.order.len() {
                    if let Some(x) = topo.order.get(i) {
                        if *x == edge.from {
                            pf = Some(i);
                        }
                        if *x == edge.to {
                            pt = Some(i);
                        }
                    }
                    i += 1;
                }
                match (pf, pt) {
                    (Some(f), Some(t)) => {
                        if f >= t {
                            return Err(TopoCode::TopoOrderBreach.val());
                        }
                    }
                    _ => return Err(TopoCode::DrillLeak.val()),
                }
            }
        }
        e += 1;
    }
    mutex_check(&reg, &topo)?;
    let drill = drill_run(&reg)?;
    if !drill.order_ok || !drill.complete || drill.executed.len() != 10 {
        return Err(TopoCode::DrillLeak.val());
    }
    // 注入三路：环依赖 / 互斥同波 / 封账后登记。
    let cyc = cycle_registry(101, 102)?;
    let cyc_hit = match topo_sort(&cyc) {
        Err(e) => e == TopoCode::CycleDetected.val(),
        Ok(_) => false,
    };
    let clash = mutex_clash_registry(201, 202)?;
    let clash_hit = match topo_sort(&clash) {
        Ok(t) => mutex_check(&clash, &t).is_err(),
        Err(_) => false,
    };
    let mut sealed = corpus_registry()?;
    let sealed_hit = sealed.add_node(999, SEG_BASE).is_err();
    if !(cyc_hit && clash_hit && sealed_hit) {
        return Err(TopoCode::NotReproducible.val());
    }
    Ok((
        drill.executed.len() as u32,
        (cyc_hit as u32) + (clash_hit as u32) + (sealed_hit as u32),
        reg.seal_count,
    ))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（演练 10/10 + 注入 3 路全抓 + 封账 1 次）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((10, 3, 1)) => Ok(()),
        _ => Err(TopoCode::NotReproducible.val()),
    }
}

/// 序 B：空簿拓扑（0 节点返回空序，不炸不环）。
pub fn probe_seq_b() -> Result<(), u32> {
    let reg = TopoRegistry::new();
    let topo = topo_sort(&reg)?;
    if topo.order.is_empty() && topo.levels.is_empty() {
        Ok(())
    } else {
        Err(TopoCode::NotReproducible.val())
    }
}

/// 序 C：悬空边显性拒（端点未登记）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut reg = TopoRegistry::new();
    reg.add_node(1, SEG_BASE)?;
    match reg.add_edge(1, 404, EdgeKind::Prereq) {
        Err(e) if e == TopoCode::UnknownNode.val() => Ok(()),
        _ => Err(TopoCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 3 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let base = 300u32 + (rng.below(1000) as u32) * 2;
        // 路 1：环依赖（A→B→A）。
        let cyc = cycle_registry(base, base + 1)?;
        led.record(0, match topo_sort(&cyc) {
            Err(e) => e == TopoCode::CycleDetected.val(),
            Ok(_) => false,
        });
        // 路 2：互斥同波（两节点同层互斥）。
        let clash = mutex_clash_registry(base + 2, base + 3)?;
        led.record(1, match topo_sort(&clash) {
            Ok(t) => mutex_check(&clash, &t).is_err(),
            Err(_) => false,
        });
        // 路 3：封账倒退（封账后登记显性拒）。
        let mut sealed = cycle_registry(base + 4, base + 5)?;
        led.record(2, sealed.add_node(base + 6, SEG_BASE).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TopoCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在 0x75000~0x781C0 的 0x40 对齐窗口） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < TopoCode::ALL.len() {
        if let Some(c) = TopoCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 诊断码闭集对账 ─────────────────────────────────────────────
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < TopoCode::ALL.len() {
        if let Some(c) = TopoCode::ALL.get(i) {
            if detail(c.val()) != c.detail() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 三态闭集对账（ALL 数组 3 态无重复） ────────────────────────
fn edge_kind_closure_ok() -> bool {
    if EdgeKind::ALL.len() != 3 {
        return false;
    }
    let mut i = 0;
    while i < EdgeKind::ALL.len() {
        let mut j = i + 1;
        while j < EdgeKind::ALL.len() {
            if let (Some(a), Some(b)) = (EdgeKind::ALL.get(i), EdgeKind::ALL.get(j)) {
                if a == b {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── LCG 确定性 ─────────────────────────────────────────────────
fn lcg_deterministic_ok() -> bool {
    let mut x = Lcg::new(SEG_BASE as u64);
    let mut y = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < 16 {
        if x.next() != y.next() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：≥12 条 add_dyn） ──────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 口径冻结。
    v.push((
        "J1-口径冻结",
        CORPUS_N == 10 && NODE_ID_MIN == 1 && PREREQ_EDGES.len() == 10,
        format!("语料 {} 断言 + 前置边 {} 条", CORPUS_N, PREREQ_EDGES.len()),
    ));
    // 2. 三态闭集。
    v.push((
        "J1-三态闭集",
        edge_kind_closure_ok(),
        format!("前置/互斥/独立 {} 态无重复", EdgeKind::ALL.len()),
    ));
    // 3. 语料账（10 节点 + 16 边）。
    let reg = corpus_registry();
    v.push((
        "J1-语料账",
        match &reg {
            Ok(r) => r.nodes.len() == 10 && r.edges.len() == 16 && r.phase == Some(RegPhase::Sealed),
            Err(_) => false,
        },
        match &reg {
            Ok(r) => format!("nodes={} edges={} 已封账", r.nodes.len(), r.edges.len()),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 4. 拓扑无环 + 层级账。
    let topo = match &reg {
        Ok(r) => topo_sort(r),
        Err(_) => Err(TopoCode::UnknownNode.val()),
    };
    v.push((
        "J1-拓扑无环层级账",
        match &topo {
            Ok(t) => t.order.len() == 10 && t.levels.len() == 10,
            Err(_) => false,
        },
        match &topo {
            Ok(t) => format!("order={} 首层={} 末层={}", t.order.len(), t.levels.first().copied().unwrap_or(0), t.levels.last().copied().unwrap_or(0)),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 5. 互斥对异层。
    let mutex_ok = match (&reg, &topo) {
        (Ok(r), Ok(t)) => mutex_check(r, t).is_ok(),
        _ => false,
    };
    v.push((
        "J1-互斥对异层",
        mutex_ok,
        String::from("3 互斥对全部不同波"),
    ));
    // 6. 演练 10/10 按拓扑序。
    let drill = match &reg {
        Ok(r) => drill_run(r),
        Err(_) => Err(TopoCode::UnknownNode.val()),
    };
    v.push((
        "J1-演练10/10拓扑序",
        match &drill {
            Ok(d) => d.complete && d.order_ok && d.executed.len() == 10,
            Err(_) => false,
        },
        match &drill {
            Ok(d) => format!("executed={} order_ok={}", d.executed.len(), d.order_ok),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 7. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((done, caught, seals)) => *done == 10 && *caught == 3 && *seals == 1,
            Err(_) => false,
        },
        match &g {
            Ok((done, caught, seals)) => format!("done={} caught={} seals={}", done, caught, seals),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 8. 环依赖注入 10/10（三路注入账）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-注入三路10/10",
        match &inj {
            Ok(l) => l.all_caught() && l.tries.first() == Some(&JUDGE_ROUNDS),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("t={:?} c={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 9. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b空簿={} c悬空拒={}", a, b, c),
    ));
    // 10. 诊断码闭集对账。
    v.push((
        "J1-诊断码闭集对账",
        code_closure_ok(),
        format!("{} 码全查表通过", TopoCode::ALL.len()),
    ));
    // 11. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 12. LCG 确定性。
    v.push((
        "J2-LCG确定性",
        lcg_deterministic_ok(),
        String::from("同种子 16 步逐位一致"),
    ));
    // 13. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x75C40 窗口 0x40 对齐区间三闸"),
    ));
    v
}

pub fn run_unx1488_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1488 断言依赖序登记（执行序拓扑冻结）");
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
    fn golden_run_green() {
        match golden_run() {
            Ok((done, caught, seals)) => {
                assert_eq!(done, 10);
                assert_eq!(caught, 3);
                assert_eq!(seals, 1);
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn frozen_consts() {
        assert_eq!(CORPUS_N, 10);
        assert_eq!(PREREQ_EDGES.len(), 10);
        assert_eq!(MUTEX_EDGES.len(), 3);
        assert_eq!(INDEPENDENT_EDGES.len(), 3);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn cycle_injection_caught() {
        let reg = match cycle_registry(101, 102) {
            Ok(r) => r,
            Err(e) => {
                assert!(false, "cycle reg err=0x{:x}", e);
                return;
            }
        };
        match topo_sort(&reg) {
            Err(e) => assert_eq!(e, TopoCode::CycleDetected.val()),
            Ok(_) => assert!(false, "A→B→A 环未检出"),
        }
    }

    #[test]
    fn mutex_clash_caught() {
        let reg = match mutex_clash_registry(201, 202) {
            Ok(r) => r,
            Err(e) => {
                assert!(false, "mutex reg err=0x{:x}", e);
                return;
            }
        };
        let topo = match topo_sort(&reg) {
            Ok(t) => t,
            Err(e) => {
                assert!(false, "mutex topo err=0x{:x}", e);
                return;
            }
        };
        match mutex_check(&reg, &topo) {
            Err(e) => assert_eq!(e, TopoCode::MutexConflict.val()),
            Ok(()) => assert!(false, "同层互斥未拒"),
        }
    }

    #[test]
    fn sealed_locked_monotonic() {
        let mut reg = corpus_registry().ok().unwrap_or_else(|| TopoRegistry::new());
        // 封账后一切登记显性拒（状态机单向）。
        assert!(reg.add_node(888, SEG_BASE).is_err());
        assert!(reg.add_edge(1, 2, EdgeKind::Independent).is_err());
        // 再封账也是倒退（已封账不得重复封）。
        assert!(reg.seal().is_err());
    }

    #[test]
    fn topo_order_respects_prereqs() {
        let reg = corpus_registry().ok().unwrap_or_else(|| TopoRegistry::new());
        let topo = match topo_sort(&reg) {
            Ok(t) => t,
            Err(e) => {
                assert!(false, "topo err=0x{:x}", e);
                return;
            }
        };
        // 每条前置边 from 必须排在 to 之前。
        let mut i = 0;
        while i < PREREQ_EDGES.len() {
            if let Some((a, b)) = PREREQ_EDGES.get(i) {
                let pa = topo.order.iter().position(|x| x == a);
                let pb = topo.order.iter().position(|x| x == b);
                match (pa, pb) {
                    (Some(fa), Some(fb)) => assert!(fa < fb, "前置边 {}→{} 序破坏", a, b),
                    _ => assert!(false, "节点缺席拓扑序"),
                }
            }
            i += 1;
        }
    }

    #[test]
    fn drill_report_10_of_10() {
        let reg = corpus_registry().ok().unwrap_or_else(|| TopoRegistry::new());
        let d = match drill_run(&reg) {
            Ok(d) => d,
            Err(e) => {
                assert!(false, "drill err=0x{:x}", e);
                return;
            }
        };
        assert!(d.complete);
        assert!(d.order_ok);
        assert_eq!(d.executed.len(), 10);
        assert_eq!(d.executed.first().copied(), Some(1));
        assert_eq!(d.executed.last().copied(), Some(10));
    }

    #[test]
    fn duplicate_node_caught() {
        let mut reg = TopoRegistry::new();
        assert!(reg.add_node(1, SEG_BASE).is_ok());
        match reg.add_node(1, SEG_BASE) {
            Err(e) => assert_eq!(e, TopoCode::DuplicateNode.val()),
            Ok(()) => assert!(false, "重复节点未拒"),
        }
    }

    #[test]
    fn inject_rounds_tally() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(e) => {
                assert!(false, "inject err=0x{:x}", e);
                return;
            }
        };
        assert!(led.all_caught());
        assert_eq!(led.tries.get(0), Some(&JUDGE_ROUNDS));
        assert_eq!(led.tries.get(1), Some(&JUDGE_ROUNDS));
        assert_eq!(led.tries.get(2), Some(&JUDGE_ROUNDS));
    }

    #[test]
    fn probe_sequences() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn reproducible_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(code_closure_ok());
        assert!(lcg_deterministic_ok());
        assert_eq!(detail(TopoCode::CycleDetected.val()), TopoCode::CycleDetected.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }
}
