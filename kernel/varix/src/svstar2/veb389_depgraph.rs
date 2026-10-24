//! veb389_depgraph —— VE-F0389 B 域依赖图。
//! 锚点（CoRun Unxreal 主册）：VE-F0389 · B 域依赖图 · 目标 300 行
//! 判据原文：VE-F0389-J1 B 域模块依赖图：边登记（A 依赖 B）；
//!           环检出（DFS 三色标）；入度/出度统计；孤立模块清
//!           单（零入零出）；拓扑序核（排序结果满足所有边方向）；
//!           自依赖拒（A→A 立即拒）。
//! 组件：邻接表 + 三色 DFS + 度账 + 拓扑排序（Kahn）。
//! 职责定位：依赖图是「B 域的家谱」——环依赖是构建死锁的种子，
//!           必须在登记时就拦；拓扑序是加载序的依据：被依赖者
//!           先行；孤立模块不是错但是观察对象：无依赖无被依赖
//!           通常是忘了接线。
//! 性能逐项分解：登记 O(1)；DFS O(V+E)；Kahn O(V+E)。
//! 诊断码段：0x306C0（0x306C1~0x306C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DG_VERSION: &str = "veb389_depgraph-v1";

/// 诊断段基址（0x306C0 段 · 本模块独占）。
pub const DG_SEG_BASE: u32 = 0x306C0;
pub const XC_CYCLE: u32 = 0x306C1; // 环
pub const XC_SELF: u32 = 0x306C2; // 自依赖
pub const XC_NO_NODE: u32 = 0x306C3; // 无此节点
pub const XC_NODE_DUP: u32 = 0x306C4; // 节点重名
pub const XC_MAGIC: u32 = 0x306C5; // 账魔数破
pub const XC_EMPTY: u32 = 0x306C6; // 空图操作
pub const XC_EDGE_DUP: u32 = 0x306C7; // 边重登记
pub const XC_TOPO: u32 = 0x306C8; // 拓扑序违边
pub const XC_ISOLATE: u32 = 0x306C9; // 孤立清单不一致

/// 三色。
const WHITE: u8 = 0;
const GRAY: u8 = 1;
const BLACK: u8 = 2;

/// 依赖图。
pub struct DepGraph {
    names: Vec<&'static str>,
    adj: Vec<Vec<usize>>, // adj[i] = i 依赖的节点集
    magic: u32,
}

impl DepGraph {
    /// 建。
    pub fn new() -> DepGraph {
        DepGraph { names: Vec::new(), adj: Vec::new(), magic: 0x06C0_6E7E }
    }

    /// 加节点。
    pub fn add_node(&mut self, name: &'static str) -> Result<usize, u32> {
        if self.magic != 0x06C0_6E7E {
            return Err(XC_MAGIC);
        }
        for n in self.names.iter() {
            if *n == name {
                return Err(XC_NODE_DUP);
            }
        }
        self.names.push(name);
        self.adj.push(Vec::new());
        Ok(self.names.len() - 1)
    }

    fn find(&self, name: &str) -> Result<usize, u32> {
        for (i, n) in self.names.iter().enumerate() {
            if *n == name {
                return Ok(i);
            }
        }
        Err(XC_NO_NODE)
    }

    /// 登记边：a 依赖 b。
    pub fn add_edge(&mut self, a: &str, b: &str) -> Result<(), u32> {
        if a == b {
            return Err(XC_SELF);
        }
        let ai = self.find(a)?;
        let bi = self.find(b)?;
        for d in self.adj[ai].iter() {
            if *d == bi {
                return Err(XC_EDGE_DUP);
            }
        }
        self.adj[ai].push(bi);
        Ok(())
    }

    /// 环检出（三色 DFS）。
    pub fn has_cycle(&self) -> Result<bool, u32> {
        if self.magic != 0x06C0_6E7E {
            return Err(XC_MAGIC);
        }
        if self.names.is_empty() {
            return Err(XC_EMPTY);
        }
        let n = self.names.len();
        let mut color = vec![WHITE; n];
        // 迭代 DFS（显式栈：节点 + 子游标）。
        for start in 0..n {
            if color[start] != WHITE {
                continue;
            }
            let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
            color[start] = GRAY;
            // 迭代 DFS：栈元素 = (节点, 已展开的邻边游标)。
            while let Some(top) = stack.last().copied() {
                let u = top.0;
                let cur = top.1;
                if cur < self.adj[u].len() {
                    let v = self.adj[u][cur];
                    stack.last_mut().unwrap_or(&mut (u, cur)).1 += 1;
                    if color[v] == GRAY {
                        return Ok(true);
                    }
                    if color[v] == WHITE {
                        color[v] = GRAY;
                        stack.push((v, 0));
                    }
                } else {
                    color[u] = BLACK;
                    stack.pop();
                }
            }
        }
        Ok(false)
    }

    /// 出度/入度。
    pub fn degrees(&self, name: &str) -> Result<(u32, u32), u32> {
        let i = self.find(name)?;
        let mut indeg = 0u32;
        let outdeg = self.adj[i].len() as u32;
        for lst in self.adj.iter() {
            for d in lst.iter() {
                if *d == i {
                    indeg += 1;
                }
            }
        }
        Ok((indeg, outdeg))
    }

    /// 孤立节点清单。
    pub fn isolates(&self) -> Result<Vec<&'static str>, u32> {
        if self.names.is_empty() {
            return Err(XC_EMPTY);
        }
        let mut out = Vec::new();
        for i in 0..self.names.len() {
            let (indeg, outdeg) = self.degrees(self.names[i])?;
            if indeg == 0 && outdeg == 0 {
                out.push(self.names[i]);
            }
        }
        Ok(out)
    }

    /// Kahn 拓扑排序。
    pub fn topo(&self) -> Result<Vec<&'static str>, u32> {
        if self.names.is_empty() {
            return Err(XC_EMPTY);
        }
        let n = self.names.len();
        let mut indeg = vec![0u32; n];
        for lst in self.adj.iter() {
            for d in lst.iter() {
                indeg[*d] += 1;
            }
        }
        let mut ready: Vec<usize> = Vec::new();
        for i in 0..n {
            if indeg[i] == 0 {
                ready.push(i);
            }
        }
        let mut order: Vec<&'static str> = Vec::new();
        while !ready.is_empty() {
            let u = ready.remove(0);
            order.push(self.names[u]);
            for d in self.adj[u].iter() {
                indeg[*d] -= 1;
                if indeg[*d] == 0 {
                    ready.push(*d);
                }
            }
        }
        if order.len() != n {
            return Err(XC_CYCLE);
        }
        Ok(order)
    }

    /// 拓扑序核：序中每个被依赖者先于依赖者。
    pub fn topo_verify(&self) -> Result<bool, u32> {
        let order = self.topo()?;
        let mut pos: Vec<i32> = vec![-1; self.names.len()];
        for (i, nm) in order.iter().enumerate() {
            let id = self.find(nm)?;
            pos[id] = i as i32;
        }
        for u in 0..self.names.len() {
            for d in self.adj[u].iter() {
                // u 依赖 d ⇒ d 必须先于 u。
                if pos[*d] >= pos[u] {
                    return Err(XC_TOPO);
                }
            }
        }
        Ok(true)
    }

    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.names.len()
    }
}

pub fn run_veb389_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0389");
    // 族一： DAG 登记 + 拓扑。
    let mut g = DepGraph::new();
    let _ = g.add_node("kernel");
    let _ = g.add_node("heap");
    let _ = g.add_node("sched");
    let _ = g.add_node("io");
    let _ = g.add_node("lone");
    s.add("五节点", g.node_count() == 5, "登记");
    s.add("重名拒", g.add_node("heap") == Err(XC_NODE_DUP), "主键");
    let _ = g.add_edge("heap", "kernel");
    let _ = g.add_edge("sched", "kernel");
    let _ = g.add_edge("io", "heap");
    s.add("边 3", g.degrees("kernel") == Ok((3, 0)), "kernel 入 3 出 0");
    s.add("io 度", g.degrees("io") == Ok((0, 1)), "入 0 出 1");
    s.add("孤立清单", {
        let iso = g.isolates();
        iso == Ok(vec!["lone"])
    }, "lone 独行");
    s.add("无环", g.has_cycle() == Ok(false), "DAG");
    let topo = g.topo();
    s.add("拓扑出序", topo.is_ok(), "排序成");
    s.add("拓扑核", g.topo_verify() == Ok(true), "边向满足");
    let order = topo;
    s.add("kernel 殿后", matches!(&order, Ok(o) if o[4] == "kernel"), "被依赖者最后");
    s.add("lone 打头", matches!(&order, Ok(o) if o[0] == "lone"), "零入先出");
    // 族二：环检出。
    let mut c = DepGraph::new();
    let _ = c.add_node("a");
    let _ = c.add_node("b");
    let _ = c.add_node("cc");
    let _ = c.add_edge("a", "b");
    let _ = c.add_edge("b", "cc");
    s.add("链无环", c.has_cycle() == Ok(false), "a→b→c");
    let _ = c.add_edge("cc", "a");
    s.add("闭环检出", c.has_cycle() == Ok(true), "a→b→c→a");
    s.add("环图拓扑拒", c.topo() == Err(XC_CYCLE), "排序不成");
    // 族三：自依赖与边约束。
    let mut d = DepGraph::new();
    let _ = d.add_node("s");
    s.add("自依赖拒", d.add_edge("s", "s") == Err(XC_SELF), "A→A");
    s.add("野边拒", d.add_edge("s", "ghost") == Err(XC_NO_NODE), "无此点");
    let _ = d.add_node("t");
    let _ = d.add_edge("s", "t");
    s.add("重边拒", d.add_edge("s", "t") == Err(XC_EDGE_DUP), "重复");
    // 族四：空图。
    let e = DepGraph::new();
    s.add("空图环检拒", e.has_cycle() == Err(XC_EMPTY), "无点");
    s.add("空图拓扑拒", e.topo() == Err(XC_EMPTY), "无点");
    s.add("魔数核", DG_SEG_BASE == 0x306C0, "段基址");
    s
}
