//! CGPU-F0172 · 帧图确定性（判据段 0x0460）
//! 帧图构建与执行的确定性纪律：图结构确定（同输入状态→同图拓扑——
//! 确定性是回放/对拍的前提）、执行序确定（拓扑排序结果确定——同优先级
//! 按稳定序）、调度确定（线程调度不影响像素结果——任务间无共享可变
//! 状态依赖）。确定性验证：同场景同线程数多次执行的图结构哈希与像素
//! 输出全同。判据：图哈希一致、执行序确定、像素一致、多线程无关、
//! 破坏即🔴。
//!
//! 诊断码段 0x0460（本模块独占 16 码：0x0460~0x046F）。
//! no_std + alloc 零 unwrap，全部 Result 化，错误即返回无 panic 路径。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0460;

/// 稳定序的优先级平手规则（判据「同优先级按稳定序」）：先按序号，
/// 再按名字指纹——两级平手保证任意构建次序下排序结果唯一。
pub const TIEBREAK_LEVELS: u8 = 2;

/// 图节点（确定性视角的最小面）：任务号、优先级、名字指纹、
/// 输出资源指纹。哈希折并只用这四元——输入状态即四元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DetNode {
    /// 任务号。
    pub task: u32,
    /// 优先级（大者先发）。
    pub prio: u8,
    /// 名字指纹。
    pub name_fp: u64,
    /// 输出资源指纹。
    pub out_fp: u64,
}

impl DetNode {
    /// 节点校验：名字与输出指纹非零（零指纹 = 未定名节点，破坏
    /// 确定性的第一来源——匿名节点的排序位次不稳定）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.name_fp == 0 {
            return Err(SEG_BASE);
        }
        if self.out_fp == 0 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// 依赖边（确定性视角）：from 必须先于 to。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DetEdge {
    /// 前置任务。
    pub from: u32,
    /// 后继任务。
    pub to: u32,
}

impl DetEdge {
    /// 边校验：自环直接拒（自环是确定性的死敌——任何排序都无解）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.from == self.to {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
}

/// FNV-1a 64（工程域统一指纹函数）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性帧图：节点集 + 边集 + 已算出的结构哈希缓存。
pub struct DetFrameGraph {
    /// 节点集（构建次序无关——哈希与排序都按稳定键）。
    nodes: Vec<DetNode>,
    /// 边集。
    edges: Vec<DetEdge>,
    /// 结构哈希缓存（None = 未算）。
    hash_cache: Option<u64>,
}

impl DetFrameGraph {
    /// 空图。
    pub fn new() -> Self {
        DetFrameGraph {
            nodes: Vec::new(),
            edges: Vec::new(),
            hash_cache: None,
        }
    }

    /// 加节点（重复任务号拒——任务号是稳定键的唯一性来源）。
    pub fn add_node(&mut self, n: DetNode) -> Result<(), u16> {
        n.validate()?;
        if self.nodes.iter().any(|x| x.task == n.task) {
            return Err(SEG_BASE + 3);
        }
        self.hash_cache = None;
        self.nodes.push(n);
        Ok(())
    }

    /// 加边（端点必须在图内——悬挂边使哈希依赖注册次序）。
    pub fn add_edge(&mut self, e: DetEdge) -> Result<(), u16> {
        e.validate()?;
        if !self.nodes.iter().any(|n| n.task == e.from)
            || !self.nodes.iter().any(|n| n.task == e.to)
        {
            return Err(SEG_BASE + 4);
        }
        if self
            .edges
            .iter()
            .any(|x| x.from == e.from && x.to == e.to)
        {
            return Err(SEG_BASE + 5);
        }
        self.hash_cache = None;
        self.edges.push(e);
        Ok(())
    }

    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 边数。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 图结构哈希（判据「图哈希一致」核心面）：节点按 task 序、
    /// 边按 (from,to) 序折入 FNV 链——与插入次序完全无关，同
    /// 输入状态两次构建哈希必然一致。
    pub fn structure_hash(&mut self) -> Result<u64, u16> {
        if let Some(h) = self.hash_cache {
            return Ok(h);
        }
        if self.nodes.is_empty() {
            return Err(SEG_BASE + 6);
        }
        let mut ns: Vec<&DetNode> = self.nodes.iter().collect();
        ns.sort_by_key(|n| n.task);
        let mut es: Vec<&DetEdge> = self.edges.iter().collect();
        es.sort_by_key(|e| (e.from, e.to));
        let mut acc: u64 = 0xcbf29ce484222325;
        for n in ns {
            acc = acc.wrapping_mul(0x100000001b3) ^ n.task as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ n.prio as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ n.name_fp;
            acc = acc.wrapping_mul(0x100000001b3) ^ n.out_fp;
        }
        for e in es {
            acc = acc.wrapping_mul(0x100000001b3) ^ e.from as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ e.to as u64;
        }
        if acc == 0 {
            return Err(SEG_BASE + 7);
        }
        self.hash_cache = Some(acc);
        Ok(acc)
    }

    /// 稳定执行序（判据「执行序确定」核心面）：Kahn 分层 + 层内
    /// 按 (prio 降序, name_fp 升序) 两级平手——同输入永远同输出。
    /// 有环返回 Err（环图无确定执行序）。
    pub fn stable_order(&self) -> Result<Vec<u32>, u16> {
        let n = self.nodes.len();
        if n == 0 {
            return Err(SEG_BASE + 8);
        }
        let mut indeg: Vec<u32> = vec![0; n];
        let idx = |t: u32| -> Option<usize> { self.nodes.iter().position(|x| x.task == t) };
        for e in &self.edges {
            let (a, b) = match (idx(e.from), idx(e.to)) {
                (Some(a), Some(b)) => (a, b),
                _ => return Err(SEG_BASE + 9),
            };
            indeg[b] += 1;
        }
        let mut ready: Vec<usize> = Vec::new();
        for i in 0..n {
            if indeg[i] == 0 {
                ready.push(i);
            }
        }
        let mut order: Vec<u32> = Vec::new();
        while !ready.is_empty() {
            // 层内稳定序：prio 降序，平手 name_fp 升序，再平手 task 升序。
            ready.sort_by(|&a, &b| {
                let (na, nb) = (&self.nodes[a], &self.nodes[b]);
                nb.prio
                    .cmp(&na.prio)
                    .then(na.name_fp.cmp(&nb.name_fp))
                    .then(na.task.cmp(&nb.task))
            });
            let layer: Vec<usize> = ready.drain(..).collect();
            for i in layer {
                order.push(self.nodes[i].task);
                for e in &self.edges {
                    if e.from == self.nodes[i].task {
                        let j = idx(e.to).ok_or(SEG_BASE + 9)?;
                        indeg[j] -= 1;
                        if indeg[j] == 0 {
                            ready.push(j);
                        }
                    }
                }
            }
        }
        if order.len() != n {
            return Err(SEG_BASE + 10);
        }
        Ok(order)
    }

    /// 环检测（确定性前置）：有环即无确定执行序。
    pub fn has_cycle(&self) -> bool {
        self.stable_order().is_err()
    }
}

/// 像素输出指纹记录（判据「像素一致」验证面）：一次执行产出的
/// 输出资源指纹折并。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PixelFingerprint {
    /// 场景号。
    pub scene: u32,
    /// 线程数（确定性必须与线程数无关）。
    pub threads: u16,
    /// 像素输出折并指纹。
    pub pixel_fp: u64,
}

impl PixelFingerprint {
    /// 记录校验。
    pub fn validate(&self) -> Result<(), u16> {
        if self.pixel_fp == 0 {
            return Err(SEG_BASE + 11);
        }
        Ok(())
    }
}

/// 确定性验证台账：同场景多次执行的（结构哈希, 像素指纹）对账。
pub struct DetVerifyLedger {
    /// 对账行：场景 → 结构哈希 → 像素指纹 → 线程数。
    rows: Vec<(u32, u64, PixelFingerprint)>,
    /// 已立案的破坏事件（判据「破坏即🔴」）。
    breaks: Vec<(u32, u16)>,
}

impl DetVerifyLedger {
    /// 空台账。
    pub fn new() -> Self {
        DetVerifyLedger {
            rows: Vec::new(),
            breaks: Vec::new(),
        }
    }

    /// 提交一次执行的观测（判据「图哈希一致」「像素一致」对账面）：
    /// 同场景已有不同结构哈希 → 立案；同哈希但像素指纹不同 → 立案；
    /// 否则入账。
    pub fn observe(
        &mut self,
        scene: u32,
        struct_hash: u64,
        pixel: PixelFingerprint,
    ) -> Result<(), u16> {
        if struct_hash == 0 {
            return Err(SEG_BASE + 12);
        }
        pixel.validate()?;
        for (s, h, p) in &self.rows {
            if *s != scene {
                continue;
            }
            if *h != struct_hash {
                self.breaks.push((scene, SEG_BASE + 13));
                return Err(SEG_BASE + 13);
            }
            if p.pixel_fp != pixel.pixel_fp {
                self.breaks.push((scene, SEG_BASE + 14));
                return Err(SEG_BASE + 14);
            }
        }
        self.rows.push((scene, struct_hash, pixel));
        Ok(())
    }

    /// 多线程无关审计（判据「多线程无关」专项面）：同场景必须
    /// 至少两个不同线程数的观测且像素全同——只测单线程数不算
    /// 通过多线程无关判据。
    pub fn thread_independence(&self, scene: u32) -> Result<(), u16> {
        let mut thread_sets: Vec<u16> = Vec::new();
        let mut fps: Vec<u64> = Vec::new();
        for (s, _, p) in &self.rows {
            if *s == scene {
                if !thread_sets.contains(&p.threads) {
                    thread_sets.push(p.threads);
                }
                if !fps.contains(&p.pixel_fp) {
                    fps.push(p.pixel_fp);
                }
            }
        }
        if fps.is_empty() {
            return Err(SEG_BASE + 15);
        }
        if fps.len() > 1 {
            return Err(SEG_BASE + 16);
        }
        if thread_sets.len() < 2 {
            return Err(SEG_BASE + 17);
        }
        Ok(())
    }

    /// 台账绿判（判据汇总）：无破坏立案 + 至少一场完成对账。
    pub fn green(&self) -> Result<(), u16> {
        if !self.breaks.is_empty() {
            return Err(SEG_BASE + 18);
        }
        if self.rows.is_empty() {
            return Err(SEG_BASE + 19);
        }
        Ok(())
    }

    /// 破坏事件数。
    pub fn break_count(&self) -> usize {
        self.breaks.len()
    }
}

/// 执行序确定性对拍（独立函数面）：两次排序结果全等才确定。
pub fn order_deterministic(a: &[u32], b: &[u32]) -> Result<(), u16> {
    if a.len() != b.len() || a.is_empty() {
        return Err(SEG_BASE + 20);
    }
    for i in 0..a.len() {
        if a[i] != b[i] {
            return Err(SEG_BASE + 21);
        }
    }
    Ok(())
}

/// 调度无关性断言（判据「调度确定」的静态面）：给定执行序中
/// 任意相邻对，若 b 依赖 a 则合法；若 a 依赖 b 则排序违反拓扑。
pub fn schedule_independence(g: &DetFrameGraph, order: &[u32]) -> Result<(), u16> {
    let n = g.node_count();
    if order.len() != n {
        return Err(SEG_BASE + 22);
    }
    for e in &g.edges {
        let pa = order.iter().position(|t| *t == e.from).ok_or(SEG_BASE + 23)?;
        let pb = order.iter().position(|t| *t == e.to).ok_or(SEG_BASE + 23)?;
        if pa >= pb {
            return Err(SEG_BASE + 24);
        }
    }
    Ok(())
}

/// 共享可变状态扫描（判据「任务间无共享可变状态依赖」静态面）：
/// 两个节点写同一输出资源且无序约束 = 数据竞争，确定性破坏。
pub fn shared_mutable_scan(g: &DetFrameGraph) -> Result<(), u16> {
    for i in 0..g.nodes.len() {
        for j in (i + 1)..g.nodes.len() {
            let (a, b) = (&g.nodes[i], &g.nodes[j]);
            if a.out_fp == b.out_fp {
                let ordered = g
                    .edges
                    .iter()
                    .any(|e| (e.from == a.task && e.to == b.task) || (e.from == b.task && e.to == a.task));
                if !ordered {
                    return Err(SEG_BASE + 25);
                }
            }
        }
    }
    Ok(())
}

/// 双跑对拍全流程（判据「确定性验证」一站式）：同场景构建两次
/// 图（输入状态相同）→ 结构哈希对拍 → 执行序对拍 → 像素指纹对拍。
pub fn double_run_audit(
    build: impl Fn() -> DetFrameGraph,
    pixel_a: PixelFingerprint,
    pixel_b: PixelFingerprint,
) -> Result<u64, u16> {
    let mut g1 = build();
    let mut g2 = build();
    let h1 = g1.structure_hash()?;
    let h2 = g2.structure_hash()?;
    if h1 != h2 {
        return Err(SEG_BASE + 26);
    }
    let o1 = g1.stable_order()?;
    let o2 = g2.stable_order()?;
    order_deterministic(&o1, &o2)?;
    if pixel_a.pixel_fp != pixel_b.pixel_fp {
        return Err(SEG_BASE + 27);
    }
    schedule_independence(&g1, &o1)?;
    shared_mutable_scan(&g1)?;
    Ok(h1)
}
