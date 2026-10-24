//! CGPU-F0183 · 跨服务任务图（判据段 0x0510）
//! 任务图跨服务边界：vxweb 任务的图节点（网页渲染作为图节点——节点
//! 执行在 vxweb 服务）、跨服务边（vxweb 产出 → 合成器消费的跨进程
//! 依赖——围栏传播 F0062 对接）、跨服务预算协调（vxweb 消耗的预算在
//! 帧图中记账）。多服务图的统一调度视角（一个帧图包含跨服务节点——
//! 调度器按服务分派）。判据：跨服务节点、围栏传播、预算记账、统一
//! 视角、联调用例。
//!
//! 诊断码段 0x0510（本模块独占：0x0510~0x051F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0510;

/// 服务种类（判据「统一视角」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ServiceKind {
    /// 本内核合成服务。
    Kernel,
    /// vxweb 网页渲染服务。
    VxWeb,
    /// 外部输入服务。
    ExtIn,
}

impl ServiceKind {
    /// 序号。
    pub fn rank(self) -> u8 {
        match self {
            ServiceKind::Kernel => 0,
            ServiceKind::VxWeb => 1,
            ServiceKind::ExtIn => 2,
        }
    }
}

/// 跨服务节点（判据「跨服务节点」载体）：节点 + 执行所在服务。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct XNode {
    /// 任务号。
    pub task: u32,
    /// 执行服务。
    pub svc: ServiceKind,
    /// 产出资源指纹。
    pub out_fp: u64,
}

impl XNode {
    /// 校验：任务号与产出指纹非零。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 || self.out_fp == 0 {
            return Err(SEG_BASE);
        }
        Ok(())
    }
}

/// 跨服务边（判据「围栏传播」载体）：跨进程依赖必须带围栏。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct XEdge {
    /// 前驱（供给方）。
    pub from: u32,
    /// 后继（消费方）。
    pub to: u32,
    /// 围栏号（F0062 对接）。
    pub fence: u64,
    /// 是否跨服务边（同服务边 fence 可为 0，跨服务必须非零）。
    pub cross: bool,
}

impl XEdge {
    /// 校验（判据「围栏传播」核心）：跨服务边必须有围栏号——
    /// 无围栏跨进程依赖 = 脏读（消费到半成品）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.from == self.to || self.from == 0 || self.to == 0 {
            return Err(SEG_BASE + 1);
        }
        if self.cross && self.fence == 0 {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
}

/// 跨服务帧图（判据「统一视角」载体）：调度器唯一账本。
pub struct XFrameGraph {
    /// 节点集。
    pub nodes: Vec<XNode>,
    /// 边集。
    pub edges: Vec<XEdge>,
    /// 每服务预算记账（服务序号 → 已耗微秒）。
    pub budget_ledger: [(u32, u64); 3],
}

impl XFrameGraph {
    /// 空图。
    pub fn new() -> Self {
        XFrameGraph {
            nodes: Vec::new(),
            edges: Vec::new(),
            budget_ledger: [(0, 0), (1, 0), (2, 0)],
        }
    }

    /// 加节点（重复拒）。
    pub fn add_node(&mut self, n: XNode) -> Result<(), u16> {
        n.validate()?;
        if self.nodes.iter().any(|x| x.task == n.task) {
            return Err(SEG_BASE + 3);
        }
        self.nodes.push(n);
        Ok(())
    }

    /// 加边（端点在图内 + 跨服务自动判定 + 围栏校验）。
    pub fn add_edge(&mut self, mut e: XEdge) -> Result<(), u16> {
        let f = self.nodes.iter().find(|n| n.task == e.from).ok_or(SEG_BASE + 4)?;
        let t = self.nodes.iter().find(|n| n.task == e.to).ok_or(SEG_BASE + 4)?;
        e.cross = f.svc != t.svc;
        e.validate()?;
        if self.edges.iter().any(|x| x.from == e.from && x.to == e.to) {
            return Err(SEG_BASE + 5);
        }
        self.edges.push(e);
        Ok(())
    }

    /// 预算记账（判据「预算记账」核心面）：按服务记账，单笔
    /// 记账不得超服务预算（超 = 该服务越权，记账拒绝）。
    pub fn charge(&mut self, svc: ServiceKind, us: u64, cap_us: u64) -> Result<u64, u16> {
        if us == 0 {
            return Err(SEG_BASE + 6);
        }
        let slot = &mut self.budget_ledger[svc.rank() as usize];
        if slot.0 != svc.rank() as u32 {
            return Err(SEG_BASE + 7);
        }
        let new = slot.1 + us;
        if new > cap_us {
            return Err(SEG_BASE + 8);
        }
        slot.1 = new;
        Ok(new)
    }

    /// 服务普查（判据「统一视角」可见性）：三服务各多少节点。
    pub fn svc_census(&self) -> Result<[usize; 3], u16> {
        let mut out = [0usize; 3];
        for n in &self.nodes {
            out[n.svc.rank() as usize] += 1;
        }
        if out[0] + out[1] + out[2] != self.nodes.len() {
            return Err(SEG_BASE + 9);
        }
        Ok(out)
    }

    /// 跨服务边清单（判据「围栏传播」投影）：全部跨进程依赖。
    pub fn cross_edges(&self) -> Vec<&XEdge> {
        self.edges.iter().filter(|e| e.cross).collect()
    }

    /// 围栏唯一性（判据「围栏传播」强化面）：一个围栏号只许一条
    /// 跨服务边占用（围栏复用 = 传播语义混乱）。
    pub fn fence_unique(&self) -> Result<(), u16> {
        let xs = self.cross_edges();
        for i in 0..xs.len() {
            for j in (i + 1)..xs.len() {
                if xs[i].fence == xs[j].fence {
                    return Err(SEG_BASE + 10);
                }
            }
        }
        Ok(())
    }
}

/// 调度分派投影（判据「统一视角」调度面）：按服务分组产出
/// 分派清单——调度器对每服务发一份子计划。
pub fn dispatch_plan(g: &XFrameGraph) -> Result<[(u8, Vec<u32>); 3], u16> {
    if g.nodes.is_empty() {
        return Err(SEG_BASE + 11);
    }
    let mut out: [(u8, Vec<u32>); 3] = [
        (0, Vec::new()),
        (1, Vec::new()),
        (2, Vec::new()),
    ];
    for n in &g.nodes {
        out[n.svc.rank() as usize].1.push(n.task);
    }
    for slot in &mut out {
        slot.1.sort_unstable();
    }
    Ok(out)
}

/// 跨服务拓扑校验（判据「联调用例」面）：跨服务边的前驱必须
/// 真产出后继消费的资源（指纹对账——供给语义不成立的连线是死边）。
pub fn supply_check(g: &XFrameGraph) -> Result<(), u16> {
    for e in &g.edges {
        let f = g
            .nodes
            .iter()
            .find(|n| n.task == e.from)
            .ok_or(SEG_BASE + 12)?;
        let t = g
            .nodes
            .iter()
            .find(|n| n.task == e.to)
            .ok_or(SEG_BASE + 12)?;
        if t.out_fp != f.out_fp {
            // 消费方产出与供给方不一致：仅当消费方是 Kernel 合成
            // 节点（多输入合成）时允许多供给；否则死边。
            if t.svc != ServiceKind::Kernel {
                return Err(SEG_BASE + 13);
            }
        }
        let _ = f;
    }
    Ok(())
}

/// 跨服务联调用例（判据「联调用例」一站式）：vxweb 渲染 → 围栏 →
/// 内核合成，全链贯通校验。
pub fn joint_use_case() -> Result<(), u16> {
    let mut g = XFrameGraph::new();
    g.add_node(XNode {
        task: 1,
        svc: ServiceKind::VxWeb,
        out_fp: 0xAA,
    })?;
    g.add_node(XNode {
        task: 2,
        svc: ServiceKind::VxWeb,
        out_fp: 0xBB,
    })?;
    g.add_node(XNode {
        task: 3,
        svc: ServiceKind::Kernel,
        out_fp: 0xCC,
    })?;
    g.add_edge(XEdge {
        from: 1,
        to: 3,
        fence: 0xF1,
        cross: true,
    })?;
    g.add_edge(XEdge {
        from: 2,
        to: 3,
        fence: 0xF2,
        cross: true,
    })?;
    g.fence_unique()?;
    supply_check(&g)?;
    let census = g.svc_census()?;
    if census[1] != 2 || census[0] != 1 {
        return Err(SEG_BASE + 14);
    }
    let plan = dispatch_plan(&g)?;
    if plan[1].1.len() != 2 {
        return Err(SEG_BASE + 15);
    }
    Ok(())
}

/// 跨服务帧图深度扩展面。
impl XFrameGraph {
    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 预算水位（判据「预算记账」可见性）：各服务已耗/上限比。
    pub fn budget_watermarks(&self, caps: [u64; 3]) -> Result<[u32; 3], u16> {
        let mut out = [0u32; 3];
        for i in 0..3 {
            if caps[i] == 0 {
                return Err(SEG_BASE + 16);
            }
            out[i] = ((self.budget_ledger[i].1 * 1000) / caps[i]) as u32;
        }
        Ok(out)
    }

    /// 跨服务依赖深度（判据「围栏传播」量化面）：跨服务链最长
    /// 深度（每跨一次服务 = 一次进程边界往返，深度越大帧延迟越高）。
    pub fn cross_depth(&self) -> Result<usize, u16> {
        if self.nodes.is_empty() {
            return Err(SEG_BASE + 17);
        }
        let mut depth = 0usize;
        let mut frontier: Vec<u32> = Vec::new();
        // 起点：无跨服务入边的 VxWeb 节点。
        for n in &self.nodes {
            if n.svc == ServiceKind::VxWeb {
                let has_cross_in = self.edges.iter().any(|e| {
                    e.cross && e.to == n.task
                });
                if !has_cross_in {
                    frontier.push(n.task);
                }
            }
        }
        let mut seen: Vec<u32> = frontier.clone();
        while !frontier.is_empty() {
            let mut next: Vec<u32> = Vec::new();
            for f in &frontier {
                for e in &self.edges {
                    if e.from == *f && !seen.contains(&e.to) {
                        if e.cross {
                            depth += 1;
                        }
                        seen.push(e.to);
                        next.push(e.to);
                    }
                }
            }
            frontier = next;
            if depth > self.nodes.len() {
                return Err(SEG_BASE + 18);
            }
        }
        Ok(depth)
    }

    /// 脏边清扫（判据「联调用例」卫生面）：供给语义不成立的死边
    /// 清单（诊断投影，不自动删——删边须走增量纪律）。
    pub fn dead_edges(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for (i, e) in self.edges.iter().enumerate() {
            let f = self.nodes.iter().find(|n| n.task == e.from);
            let t = self.nodes.iter().find(|n| n.task == e.to);
            if let (Some(f), Some(t)) = (f, t) {
                if f.out_fp != t.out_fp && t.svc != ServiceKind::Kernel {
                    out.push(i as u32);
                }
            }
        }
        out
    }
}


// ===== 深化块（cgb183_xservice · 跨服务深化）：补充审计面，与主体判据同源同纪律 =====
pub mod deepen_xservice {
    //! 跨服务深化 —— 深化审计面：纯函数 + 常量表，零外部依赖。

    use alloc::vec;
    use alloc::vec::Vec;
    use crate::checks::CheckSet;

    /// 跨服务调用账：每次调用记录 (目标, 时长, 结果)，超时与失败分账。
    pub struct XServiceLedger {
        pub ok: u32,
        pub timeout: u32,
        pub refused: u32,
        pub budget_steps: u64,
    }

    impl XServiceLedger {
        pub fn new(budget_steps: u64) -> Self {
            XServiceLedger { ok: 0, timeout: 0, refused: 0, budget_steps }
        }

        /// 记一笔：结果闭集 0 成功 / 1 超时 / 2 拒绝，越界即拒。
        pub fn record(&mut self, verdict: u8, cost: u64) -> Result<(), u16> {
            match verdict {
                0 => self.ok += 1,
                1 => self.timeout += 1,
                2 => self.refused += 1,
                _ => return Err(1),
            }
            if cost > self.budget_steps {
                return Err(2);
            }
            self.budget_steps -= cost;
            Ok(())
        }

        /// 三账守恒：账面总笔数 == 成功+超时+拒绝（由构造保证，此处给快照）。
        pub fn total(&self) -> u32 {
            self.ok + self.timeout + self.refused
        }

        /// 超时率（千分比）：无调用显性 None。
        pub fn timeout_permille(&self) -> Option<u32> {
            let t = self.total();
            if t == 0 {
                return None;
            }
            Some((self.timeout * 1000) / t)
        }
    }

    pub fn run_deepen_checks() -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0183-深化");
        let mut l = XServiceLedger::new(1000);
        let _ = l.record(0, 100);
        let _ = l.record(1, 50);
        let _ = l.record(2, 10);
        cs.add("结果闭集", l.record(7, 1).is_err(), "");
        cs.add("预算扣减", l.record(0, 900).is_err() == false, "");
        cs.add("超支拒", l.record(0, 10).is_err(), "");
        cs.add("守恒快照", l.total() == 4, "");
        cs.add("超时率", l.timeout_permille().is_some(), "");
        cs
    }
}
