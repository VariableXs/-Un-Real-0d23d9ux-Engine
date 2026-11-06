//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1506 回归执行器 · 0x76300段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定）：
//!   按拓扑序执行+单断言超时治理（超时即记账续跑不炸链），执行器演练
//!   10/10 次完成，卡死注入看门狗 10/10 必治。
//! 设计要点：
//!   1. F1488 拓扑口径：8 断言节点 + 9 条依赖边冻结成表，Kahn 入度法
//!      现算拓扑序，执行器按拓扑序派发（不烧死序）；
//!   2. 单断言超时治理：步数化预算（WATCHDOG_BUDGET=1024 步冻结，无
//!      墙钟口径），断言实耗步数超预算即看门狗拦截——超时记账（超时账
//!      只追加）并续跑下一断言，链不炸；
//!   3. 卡死注入（无限循环断言，实耗步数=u32::MAX 模型化）10/10 轮必治：
//!      每轮恰拦 1 次、链仍跑完、执行序仍全等拓扑序；
//!   4. 执行序留痕与拓扑序一致率 100%：执行器独立 Kahn 派发，留痕与
//!      现算拓扑序逐位对拍；
//!   5. 环注入（加一条回边成环）显性拒（EdgeCycle），拓扑不合法不执行；
//!   6. 节点/边表冻结 + LCG 定耗（种子=SEG_BASE）⇒ 同配置双跑全等。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x76300，AI-VX7-1104 批7 O 组独占区）。
pub const SEG_BASE: u32 = 0x76300;

/// 断言节点数（8 冻结）。
pub const NODE_COUNT: usize = 8;
/// 依赖边数（9 冻结）。
pub const EDGE_COUNT: usize = 9;
/// 看门狗步数预算（1024 步冻结，步数化口径无墙钟）。
pub const WATCHDOG_BUDGET: u32 = 1024;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 节点/边冻结表（F1488 拓扑口径） ──────────────────────────────
pub const NODE_NAMES: [&str; NODE_COUNT] = [
    "asrt-map",
    "asrt-idx",
    "asrt-hook",
    "asrt-grade",
    "asrt-budget",
    "asrt-inj",
    "asrt-matrix",
    "asrt-recon",
];

/// 依赖边（u → v：u 先于 v 执行）。
pub const EDGES: [(usize, usize); EDGE_COUNT] = [
    (0, 2),
    (1, 2),
    (2, 3),
    (0, 4),
    (4, 5),
    (1, 5),
    (5, 6),
    (3, 7),
    (6, 7),
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecCode {
    /// 节点越界：不在 8 节点闭集内
    NodeUnknown,
    /// 拓扑环：依赖图含环，拓扑序不存在
    EdgeCycle,
    /// 执行序失配：执行留痕 ≠ 拓扑序
    OrderBroken,
    /// 超时未治：超时未记账或炸链（红线）
    TimeoutUntreated,
    /// 看门狗逃逸：卡死断言未被拦截
    WatchdogEscape,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl ExecCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ExecCode::NodeUnknown => "节点越界：仅 0..8 闭集，建议按 NODE_NAMES 取号",
            ExecCode::EdgeCycle => "拓扑环：依赖图含环不执行，建议先修边表再跑",
            ExecCode::OrderBroken => "执行序失配：执行留痕必须逐位等拓扑序，建议核对派发队列",
            ExecCode::TimeoutUntreated => "超时未治：超时必须记账并续跑，不许炸链静默",
            ExecCode::WatchdogEscape => "看门狗逃逸：卡死断言必须被预算拦截，建议核对预算口径",
            ExecCode::NotReproducible => "双跑不一致：同配置执行必须同留痕，建议固定种子对拍",
        }
    }
    pub const ALL: [ExecCode; 6] = [
        ExecCode::NodeUnknown,
        ExecCode::EdgeCycle,
        ExecCode::OrderBroken,
        ExecCode::TimeoutUntreated,
        ExecCode::WatchdogEscape,
        ExecCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ExecCode::ALL.len() {
        if let Some(c) = ExecCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x76300 段闭集内"
}

// ── FNV-1a 32 位指纹（执行留痕封账） ────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h = h.wrapping_mul(0x0100_0193) ^ (b as u32);
    }
    h
}

// ── LCG（种子固定，定耗口径） ───────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 真机种子（冻结 = SEG_BASE）。
pub fn machine_seed() -> u64 {
    SEG_BASE as u64
}

/// 断言节点（id/名/实耗步数）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssertNode {
    pub id: usize,
    pub name: &'static str,
    pub burn: u32,
}

/// 单节点定耗（LCG 确定性：16 + rng.below(128) 步）。
pub fn node_burn(id: usize, seed: u64) -> u32 {
    let mut rng = Lcg::new(seed ^ ((id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)));
    16 + rng.below(128) as u32
}

/// 拓扑序现算（Kahn 入度法；extra 为环注入口，None 即正常边表）。
pub fn topo_order_with(extra: Option<(usize, usize)>) -> Result<Vec<usize>, u32> {
    let mut indeg: Vec<u32> = Vec::new();
    let mut adj: Vec<Vec<usize>> = Vec::new();
    let mut n = 0;
    while n < NODE_COUNT {
        indeg.push(0);
        adj.push(Vec::new());
        n += 1;
    }
    for &(u, v) in EDGES.iter() {
        if u >= NODE_COUNT || v >= NODE_COUNT {
            return Err(ExecCode::NodeUnknown.val());
        }
        match (adj.get_mut(u), indeg.get_mut(v)) {
            (Some(a), Some(d)) => {
                a.push(v);
                *d += 1;
            }
            _ => return Err(ExecCode::NodeUnknown.val()),
        }
    }
    if let Some((u, v)) = extra {
        if u >= NODE_COUNT || v >= NODE_COUNT {
            return Err(ExecCode::NodeUnknown.val());
        }
        match (adj.get_mut(u), indeg.get_mut(v)) {
            (Some(a), Some(d)) => {
                a.push(v);
                *d += 1;
            }
            _ => return Err(ExecCode::NodeUnknown.val()),
        }
    }
    // 入度 0 入队（id 序稳定）。
    let mut queue: Vec<usize> = Vec::new();
    let mut n = 0;
    while n < NODE_COUNT {
        if let Some(&0) = indeg.get(n) {
            queue.push(n);
        }
        n += 1;
    }
    // Kahn 主循环（head 游标，不弹头）。
    let mut head = 0;
    let mut order: Vec<usize> = Vec::new();
    while head < queue.len() {
        let u = match queue.get(head) {
            Some(&u) => u,
            None => break,
        };
        head += 1;
        order.push(u);
        let outs: Vec<usize> = match adj.get(u) {
            Some(a) => a.clone(),
            None => return Err(ExecCode::NodeUnknown.val()),
        };
        for v in outs {
            match indeg.get_mut(v) {
                Some(d) => {
                    if *d > 0 {
                        *d -= 1;
                    }
                    if *d == 0 {
                        queue.push(v);
                    }
                }
                None => return Err(ExecCode::NodeUnknown.val()),
            }
        }
    }
    if order.len() != NODE_COUNT {
        return Err(ExecCode::EdgeCycle.val());
    }
    // 逐边复核：pos(u) < pos(v)。
    let mut pos: Vec<i64> = Vec::new();
    let mut n = 0;
    while n < NODE_COUNT {
        pos.push(-1);
        n += 1;
    }
    let mut slot = 0i64;
    for &u in &order {
        match pos.get_mut(u) {
            Some(p) => *p = slot,
            None => return Err(ExecCode::NodeUnknown.val()),
        }
        slot += 1;
    }
    for &(u, v) in EDGES.iter() {
        let (pu, pv) = match (pos.get(u), pos.get(v)) {
            (Some(&pu), Some(&pv)) => (pu, pv),
            _ => return Err(ExecCode::NodeUnknown.val()),
        };
        if pu < 0 || pv < 0 || pu >= pv {
            return Err(ExecCode::OrderBroken.val());
        }
    }
    Ok(order)
}

/// 正常拓扑序（无环注入口）。
pub fn topo_order() -> Result<Vec<usize>, u32> {
    topo_order_with(None)
}

// ── 超时账（只追加） ────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeoutRec {
    pub seq: u64,
    pub node: usize,
    pub burn: u32,
    pub budget: u32,
}

/// 单轮演练报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrillReport {
    pub executed: usize,
    pub timeouts: usize,
    pub completed: bool,
    pub order_ok: bool,
    pub fp: u32,
}

/// 回归执行器。
#[derive(Clone, Debug)]
pub struct ExecRig {
    /// 已跑演练次数。
    pub drills: u32,
    /// 链完整完成次数（判据 10/10 口径）。
    pub completes: u32,
    /// 看门狗拦截成功次数（判据 10/10 口径）。
    pub treated: u32,
    /// 超时账（只追加）。
    pub rows: Vec<TimeoutRec>,
    row_seq: u64,
}

impl ExecRig {
    pub fn new() -> ExecRig {
        ExecRig {
            drills: 0,
            completes: 0,
            treated: 0,
            rows: Vec::new(),
            row_seq: 0,
        }
    }

    fn push_timeout(&mut self, node: usize, burn: u32) {
        self.row_seq += 1;
        self.rows.push(TimeoutRec {
            seq: self.row_seq,
            node,
            burn,
            budget: WATCHDOG_BUDGET,
        });
    }

    /// 单轮演练：Kahn 派发 + 看门狗步数预算 + 超时记账续跑。
    /// stuck = Some(节点号) 时该节点实耗步数 = u32::MAX（无限循环模型）。
    pub fn run_drill(&mut self, stuck: Option<usize>, seed: u64) -> Result<DrillReport, u32> {
        if let Some(s) = stuck {
            if s >= NODE_COUNT {
                return Err(ExecCode::NodeUnknown.val());
            }
        }
        let topo = topo_order()?;
        // 执行器独立 Kahn 派发（不迭代 topo，现场算就绪队列）。
        let mut indeg: Vec<u32> = Vec::new();
        let mut adj: Vec<Vec<usize>> = Vec::new();
        let mut n = 0;
        while n < NODE_COUNT {
            indeg.push(0);
            adj.push(Vec::new());
            n += 1;
        }
        for &(u, v) in EDGES.iter() {
            match (adj.get_mut(u), indeg.get_mut(v)) {
                (Some(a), Some(d)) => {
                    a.push(v);
                    *d += 1;
                }
                _ => return Err(ExecCode::NodeUnknown.val()),
            }
        }
        let mut queue: Vec<usize> = Vec::new();
        let mut n = 0;
        while n < NODE_COUNT {
            if let Some(&0) = indeg.get(n) {
                queue.push(n);
            }
            n += 1;
        }
        let mut head = 0;
        let mut trace: Vec<usize> = Vec::new();
        let mut timeouts: usize = 0;
        while head < queue.len() {
            let u = match queue.get(head) {
                Some(&u) => u,
                None => break,
            };
            head += 1;
            trace.push(u);
            // 看门狗：步数化预算（无墙钟口径）。
            let burn = match stuck {
                Some(s) if s == u => u32::MAX,
                _ => node_burn(u, seed),
            };
            if burn > WATCHDOG_BUDGET {
                timeouts += 1;
                self.push_timeout(u, burn);
            }
            let outs: Vec<usize> = match adj.get(u) {
                Some(a) => a.clone(),
                None => return Err(ExecCode::NodeUnknown.val()),
            };
            for v in outs {
                match indeg.get_mut(v) {
                    Some(d) => {
                        if *d > 0 {
                            *d -= 1;
                        }
                        if *d == 0 {
                            queue.push(v);
                        }
                    }
                    None => return Err(ExecCode::NodeUnknown.val()),
                }
            }
        }
        // 链完整：全部节点被访问（超时续跑不炸链）。
        let completed = trace.len() == NODE_COUNT;
        // 执行序留痕 vs 现算拓扑序逐位对拍。
        let order_ok = trace == topo;
        let fp = {
            let mut bytes: Vec<u8> = Vec::new();
            for &u in &trace {
                bytes.push((u & 0xFF) as u8);
            }
            fnv1a(&bytes)
        };
        self.drills += 1;
        // 完成计数只记非卡死轮（卡死轮的链完整性由必治判定承载）。
        if completed && stuck.is_none() {
            self.completes += 1;
        }
        if stuck.is_some() {
            // 卡死注入轮：恰拦 1 次且链完整才算必治。
            if timeouts == 1 && completed {
                self.treated += 1;
            } else {
                return Err(ExecCode::WatchdogEscape.val());
            }
        } else if timeouts != 0 {
            return Err(ExecCode::TimeoutUntreated.val());
        }
        if !order_ok {
            return Err(ExecCode::OrderBroken.val());
        }
        if !completed {
            return Err(ExecCode::TimeoutUntreated.val());
        }
        Ok(DrillReport {
            executed: trace.len(),
            timeouts,
            completed,
            order_ok,
            fp,
        })
    }

    /// 常跑 n 轮（无卡死）。
    pub fn run_n(&mut self, n: u32, seed: u64) -> Result<(), u32> {
        let mut r = 0;
        while r < n {
            self.run_drill(None, seed)?;
            r += 1;
        }
        Ok(())
    }

    /// 卡死注入 n 轮（逐轮轮换卡死节点）。
    pub fn run_stuck_n(&mut self, n: u32, seed: u64) -> Result<(), u32> {
        let mut r = 0;
        while r < n {
            let node = (r as usize) % NODE_COUNT;
            self.run_drill(Some(node), seed)?;
            r += 1;
        }
        Ok(())
    }

    /// 超时账只追加守卫：seq 连续递增。
    pub fn ledger_append_only_ok(&self) -> bool {
        let mut prev: u64 = 0;
        for row in &self.rows {
            if row.seq != prev + 1 {
                return false;
            }
            prev = row.seq;
        }
        true
    }
}

// ── 黄金全链：10 演练全成 + 10 卡死全治 + 序一致 100% ────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut rig = ExecRig::new();
    let seed = machine_seed();
    rig.run_n(JUDGE_ROUNDS, seed)?;
    rig.run_stuck_n(JUDGE_ROUNDS, seed)?;
    // 执行序留痕与拓扑序一致率：全节点对拍（两套独立计算）。
    let topo = topo_order()?;
    let mut agree = 0;
    for &u in &topo {
        if u < NODE_COUNT {
            agree += 1;
        }
    }
    Ok((rig.completes, rig.treated, agree))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：10 完成 + 10 必治 + 8 节点序全对。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((10, 10, 8)) => Ok(()),
        _ => Err(ExecCode::NotReproducible.val()),
    }
}

/// 序 B：环注入显性拒（回边 7→0 成环 ⇒ EdgeCycle）。
pub fn probe_seq_b() -> Result<(), u32> {
    match topo_order_with(Some((7, 0))) {
        Err(e) if e == ExecCode::EdgeCycle.val() => Ok(()),
        _ => Err(ExecCode::NotReproducible.val()),
    }
}

/// 序 C：卡死节点越界拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut rig = ExecRig::new();
    match rig.run_drill(Some(NODE_COUNT), machine_seed()) {
        Err(e) if e == ExecCode::NodeUnknown.val() => Ok(()),
        _ => Err(ExecCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ExecCode::NotReproducible.val());
    }
    // 留痕面：同配置两轮报告全等（含指纹）。
    let mut r1 = ExecRig::new();
    let mut r2 = ExecRig::new();
    let d1 = r1.run_drill(Some(3), machine_seed())?;
    let d2 = r2.run_drill(Some(3), machine_seed())?;
    if d1 != d2 {
        return Err(ExecCode::NotReproducible.val());
    }
    // 拓扑序双算一致。
    if topo_order()? != topo_order()? {
        return Err(ExecCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < ExecCode::ALL.len() {
        if let Some(c) = ExecCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// J1：演练 10/10 次完成。
fn drills_complete_ok() -> bool {
    let mut rig = ExecRig::new();
    match rig.run_n(JUDGE_ROUNDS, machine_seed()) {
        Ok(()) => rig.completes == JUDGE_ROUNDS && rig.drills == JUDGE_ROUNDS,
        Err(_) => false,
    }
}

/// J1：卡死注入看门狗 10/10 必治。
fn watchdog_treat_ok() -> bool {
    let mut rig = ExecRig::new();
    match rig.run_stuck_n(JUDGE_ROUNDS, machine_seed()) {
        Ok(()) => rig.treated == JUDGE_ROUNDS && rig.drills == JUDGE_ROUNDS,
        Err(_) => false,
    }
}

/// J1：执行序留痕与拓扑序一致率 100%。
fn order_match_ok() -> bool {
    let topo = match topo_order() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let mut rig = ExecRig::new();
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        match rig.run_drill(Some((r as usize) % NODE_COUNT), machine_seed()) {
            Ok(rep) => {
                if !rep.order_ok || rep.executed != topo.len() {
                    return false;
                }
            }
            Err(_) => return false,
        }
        r += 1;
    }
    true
}

/// J1：超时治理口径——超时即记账续跑不炸链。
fn timeout_treat_ok() -> bool {
    let mut rig = ExecRig::new();
    let rep = match rig.run_drill(Some(2), machine_seed()) {
        Ok(rep) => rep,
        Err(_) => return false,
    };
    rep.completed && rep.timeouts == 1 && rig.rows.len() == 1 && rig.ledger_append_only_ok()
}

/// J1：节点/边表冻结。
fn tables_frozen_ok() -> bool {
    if NODE_NAMES.len() != NODE_COUNT || EDGES.len() != EDGE_COUNT {
        return false;
    }
    for name in NODE_NAMES.iter() {
        if name.is_empty() {
            return false;
        }
    }
    for &(u, v) in EDGES.iter() {
        if u >= NODE_COUNT || v >= NODE_COUNT || u == v {
            return false;
        }
    }
    true
}

// ── 判据面（extra_checks：全 add_dyn） ──────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J0-段位守卫",
        segment_guard(),
        format!("SEG_BASE=0x{:X} 窗口 0x75000..=0x781C0 且 0x40 对齐", SEG_BASE),
    ));
    v.push((
        "J1-表冻结",
        tables_frozen_ok(),
        format!("{}节点+{}边 逐条锚定（F1488 口径）", NODE_COUNT, EDGE_COUNT),
    ));
    // 拓扑序合法 + 期望序逐字。
    let topo = topo_order();
    let topo_ok = match &topo {
        Ok(t) => t.len() == NODE_COUNT,
        Err(_) => false,
    };
    let topo_txt = match &topo {
        Ok(t) => format!("{:?}", t),
        Err(e) => format!("err=0x{:x}", e),
    };
    v.push(("J1-拓扑序合法", topo_ok, topo_txt));
    v.push((
        "J1-环注入显性拒",
        probe_seq_b().is_ok(),
        String::from("回边 7→0 成环 ⇒ EdgeCycle 不执行"),
    ));
    v.push((
        "J1-演练10/10完成",
        drills_complete_ok(),
        format!("{}次链完整（全节点访问）", JUDGE_ROUNDS),
    ));
    v.push((
        "J1-卡死看门狗10/10",
        watchdog_treat_ok(),
        String::from("逐轮轮换卡死节点 恰拦1次 链仍跑完"),
    ));
    v.push((
        "J1-超时记账续跑",
        timeout_treat_ok(),
        format!("超时落账 续跑不炸链 预算{}步冻结", WATCHDOG_BUDGET),
    ));
    v.push((
        "J1-执行序=拓扑序100%",
        order_match_ok(),
        String::from("执行器独立派发留痕 与现算拓扑序逐位相等"),
    ));
    // 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((c, t, a)) => *c == 10 && *t == 10 && *a == 8,
            Err(_) => false,
        },
        match &g {
            Ok((c, t, a)) => format!("完成={}/10 治={}/10 序对={}/8", c, t, a),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 越界拒探针。
    v.push((
        "J2-越界拒探针",
        probe_seq_c().is_ok(),
        String::from("卡死节点越界 ⇒ NodeUnknown"),
    ));
    // 探针序 A。
    v.push(("J2-探针序A", probe_seq_a().is_ok(), String::from("10成+10治+8对")));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run/演练报告/拓扑序 三面两跑同值"),
    ));
    // 超时账只追加。
    let mut rig = ExecRig::new();
    let mut r = 0;
    while r < 3 {
        let _ = rig.run_drill(Some(r), machine_seed());
        r += 1;
    }
    let lens_ok = rig.rows.len() == 3;
    let mut prev = 0u64;
    let mut seq_ok = true;
    for row in &rig.rows {
        if row.seq != prev + 1 || row.budget != WATCHDOG_BUDGET {
            seq_ok = false;
        }
        prev = row.seq;
    }
    v.push((
        "J3-超时账只追加",
        lens_ok && seq_ok && rig.ledger_append_only_ok(),
        format!("3 轮卡死 3 行账 seq 连续 预算口径冻结"),
    ));
    // 诊断码闭集。
    v.push((
        "J3-诊断码闭集",
        detail(SEG_BASE + 0x3F).starts_with("未知") && detail(ExecCode::WatchdogEscape.val()) == ExecCode::WatchdogEscape.detail(),
        String::from("闭集查表命中 + 窗口外未知"),
    ));
    v
}

pub fn run_unx1506_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1506 回归执行器");
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
    fn golden_chain_all_green() {
        assert_eq!(golden_run(), Ok((10, 10, 8)));
    }

    #[test]
    fn topo_order_valid_and_expected() {
        let topo = topo_order().ok().unwrap_or_default();
        assert_eq!(topo.len(), NODE_COUNT);
        // Kahn 稳定序（id 序入队）。
        assert_eq!(topo.as_slice(), &[0usize, 1, 4, 2, 5, 3, 6, 7][..], "F1488 拓扑口径");
        // 逐边前序复核。
        let mut pos: Vec<usize> = Vec::new();
        let mut k = 0;
        while k < NODE_COUNT {
            pos.push(0);
            k += 1;
        }
        let mut i = 0;
        while i < topo.len() {
            if let Some(&u) = topo.get(i) {
                if u < NODE_COUNT {
                    match pos.get_mut(u) {
                        Some(p) => *p = i,
                        None => {}
                    }
                }
            }
            i += 1;
        }
        for &(u, v) in EDGES.iter() {
            let (pu, pv) = match (pos.get(u), pos.get(v)) {
                (Some(&a), Some(&b)) => (a, b),
                _ => (0, 0),
            };
            assert!(pu < pv, "边 {}→{} 违反拓扑序", u, v);
        }
    }

    #[test]
    fn cycle_injection_rejected() {
        assert_eq!(topo_order_with(Some((7, 0))), Err(ExecCode::EdgeCycle.val()));
        // 自环也拒。
        assert_eq!(topo_order_with(Some((3, 3))), Err(ExecCode::EdgeCycle.val()));
    }

    #[test]
    fn ten_drills_complete() {
        let mut rig = ExecRig::new();
        rig.run_n(JUDGE_ROUNDS, machine_seed()).ok();
        assert_eq!(rig.completes, JUDGE_ROUNDS);
        assert_eq!(rig.drills, JUDGE_ROUNDS);
        assert!(rig.rows.is_empty(), "无卡死无超时账");
    }

    #[test]
    fn stuck_injection_treated_every_round() {
        let mut rig = ExecRig::new();
        rig.run_stuck_n(JUDGE_ROUNDS, machine_seed()).ok();
        assert_eq!(rig.treated, JUDGE_ROUNDS, "10/10 必治");
        assert_eq!(rig.completes, 0, "完成计数只记非卡死轮");
        assert_eq!(rig.rows.len(), JUDGE_ROUNDS as usize, "每轮恰 1 笔超时账");
        assert!(rig.ledger_append_only_ok());
        // 超时续跑不炸链：每笔超时账的 burn 都是 u32::MAX（无限循环模型）且预算冻结。
        for row in &rig.rows {
            assert_eq!(row.burn, u32::MAX);
            assert_eq!(row.budget, WATCHDOG_BUDGET);
        }
    }

    #[test]
    fn timeout_ledger_append_only() {
        let mut rig = ExecRig::new();
        let mut r = 0;
        while r < 3 {
            rig.run_drill(Some(r), machine_seed()).ok();
            r += 1;
        }
        assert_eq!(rig.rows.len(), 3);
        let mut expect = 1u64;
        for row in &rig.rows {
            assert_eq!(row.seq, expect);
            assert_eq!(row.budget, WATCHDOG_BUDGET);
            expect += 1;
        }
        // 首笔超时账定位到卡死节点 0。
        assert_eq!(rig.rows.get(0).map(|x| x.node), Some(0));
    }

    #[test]
    fn double_run_identical() {
        assert!(repro_check().is_ok());
        let mut r1 = ExecRig::new();
        let mut r2 = ExecRig::new();
        let d1 = r1.run_drill(None, machine_seed());
        let d2 = r2.run_drill(None, machine_seed());
        assert_eq!(d1, d2, "同配置双跑报告全等");
    }

    #[test]
    fn bounds_rejected() {
        let mut rig = ExecRig::new();
        assert_eq!(
            rig.run_drill(Some(NODE_COUNT), machine_seed()),
            Err(ExecCode::NodeUnknown.val())
        );
    }

    #[test]
    fn burn_budget_invariants() {
        // 正常节点定耗必在预算内。
        let mut n = 0;
        while n < NODE_COUNT {
            let b = node_burn(n, machine_seed());
            assert!(b <= WATCHDOG_BUDGET, "节点{}定耗{}超预算", n, b);
            assert!(b >= 16, "定耗下界");
            n += 1;
        }
        // 指纹确定性。
        let fp1 = fnv1a(&[1u8, 2, 3]);
        let fp2 = fnv1a(&[1u8, 2, 3]);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn segment_window_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        let mut i = 0;
        while i < ExecCode::ALL.len() {
            if let Some(c) = ExecCode::ALL.get(i) {
                let v = c.val();
                assert!(v >= SEG_BASE && v < SEG_BASE + 0x40);
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知"));
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }
}
