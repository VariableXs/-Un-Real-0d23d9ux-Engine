//! CGPU-F0020 · NUMA 感知分配与调度（cga20_numa）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0020`
//! **判据原文摘录**：节点绑定正确、亲和调度验证、成本模型 NUMA 项、跨节点
//! 带宽实测入模型、单路平台零退化。
//!
//! 跨节点访问不是「慢一点」，是**带宽腰斩 + 延迟翻倍**：同一颗 CCD 内
//! 访问 ~80ns，跨 socket 到远端内存 ~140ns 且带宽只有一半。渲染线程池
//! 每帧要摸几十万个瓦片指针，跑错节点就是「什么都没改，帧率掉 20%」。
//!
//! 本模块只做三件事，每件都可判：
//! 1. **拓扑表**：节点数、每节点 CPU/内存、距离矩阵（本地 10、同 socket
//!    12、跨 socket 21——ACPI SLIT 的常见取值）。距离矩阵必须对称、对角
//!    为 10，这是判据；
//! 2. **绑定**：任务按其数据所在节点分配内存，内存不足时按**距离升序**
//!    溢出到最近节点（不是随便找一块），溢出比例记账；
//! 3. **定价**：把「本地/跨节点」折成放置决策器（CGPU-F0004）能吃的成本
//!    项——`cost_units = bytes × distance / 10`，本地项恰为 bytes，跨节点
//!    按距离线性放大。**单路平台必须零退化**：只有 1 个节点时，所有绑定
//!    与定价必须与「无 NUMA 逻辑」逐位一致，判据直接比。
//!
//! 错误路径（0xCD 段）：`E_NU_NODE_OOB` 节点越界 /
//! `E_NU_EMPTY_TOPO` 空拓扑 / `E_NU_NO_CAPACITY` 全平台容量不足 /
//! `E_NU_ASYMMETRIC` 距离矩阵不对称。

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCD00;
/// 节点越界。
pub const E_NU_NODE_OOB: u16 = SEG_BASE + 1;
/// 空拓扑。
pub const E_NU_EMPTY_TOPO: u16 = SEG_BASE + 2;
/// 全平台容量不足。
pub const E_NU_NO_CAPACITY: u16 = SEG_BASE + 3;
/// 距离矩阵不对称。
pub const E_NU_ASYMMETRIC: u16 = SEG_BASE + 4;

/// 本地距离基准（ACPI SLIT 约定：本地 = 10）。
pub const DIST_LOCAL: u16 = 10;
/// 同 socket 跨 die 的常见距离。
pub const DIST_SAME_SOCKET: u16 = 12;
/// 跨 socket 的常见距离。
pub const DIST_CROSS_SOCKET: u16 = 21;
/// 最大节点数（消费级双路/多 CCD 场景足够）。
pub const MAX_NODES: usize = 8;

/// 单个节点。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Node {
    pub id: u8,
    /// 逻辑 CPU 数。
    pub cpus: u16,
    /// 内存容量（MiB）。
    pub mem_mib: u32,
}

/// 拓扑表（定容，无堆）。
#[derive(Clone, Copy)]
pub struct Topology {
    nodes: [Node; MAX_NODES],
    dist: [[u16; MAX_NODES]; MAX_NODES],
    count: usize,
}

impl Topology {
    pub const fn empty() -> Topology {
        Topology {
            nodes: [Node { id: 0, cpus: 0, mem_mib: 0 }; MAX_NODES],
            dist: [[0; MAX_NODES]; MAX_NODES],
            count: 0,
        }
    }

    /// 单路（无 NUMA）拓扑：1 节点，距离 10。
    pub fn single(cpus: u16, mem_mib: u32) -> Topology {
        let mut t = Topology::empty();
        t.nodes[0] = Node { id: 0, cpus, mem_mib };
        t.dist[0][0] = DIST_LOCAL;
        t.count = 1;
        t
    }

    /// 双路拓扑（两节点，跨 socket 距离 21）。
    pub fn dual(cpus_each: u16, mem_each_mib: u32) -> Topology {
        let mut t = Topology::empty();
        for i in 0..2 {
            t.nodes[i] = Node { id: i as u8, cpus: cpus_each, mem_mib: mem_each_mib };
        }
        for i in 0..2 {
            for j in 0..2 {
                t.dist[i][j] = if i == j { DIST_LOCAL } else { DIST_CROSS_SOCKET };
            }
        }
        t.count = 2;
        t
    }

    /// 手工构造（测试与非常规平台用）；距离表必须对称、对角为 10。
    pub fn from_parts(nodes: &[Node], dist: &[[u16; MAX_NODES]]) -> Result<Topology, u16> {
        if nodes.is_empty() {
            return Err(E_NU_EMPTY_TOPO);
        }
        if nodes.len() > MAX_NODES || dist.len() < nodes.len() {
            return Err(E_NU_NODE_OOB);
        }
        let mut t = Topology::empty();
        t.count = nodes.len();
        for (i, n) in nodes.iter().enumerate() {
            t.nodes[i] = *n;
        }
        for i in 0..t.count {
            for j in 0..t.count {
                t.dist[i][j] = dist[i][j];
            }
            if t.dist[i][i] != DIST_LOCAL {
                return Err(E_NU_ASYMMETRIC);
            }
        }
        for i in 0..t.count {
            for j in 0..t.count {
                if t.dist[i][j] != t.dist[j][i] {
                    return Err(E_NU_ASYMMETRIC);
                }
                if i != j && t.dist[i][j] <= DIST_LOCAL {
                    return Err(E_NU_ASYMMETRIC);
                }
            }
        }
        Ok(t)
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn node(&self, i: usize) -> Result<Node, u16> {
        if i >= self.count {
            return Err(E_NU_NODE_OOB);
        }
        Ok(self.nodes[i])
    }

    pub fn distance(&self, a: usize, b: usize) -> Result<u16, u16> {
        if a >= self.count || b >= self.count {
            return Err(E_NU_NODE_OOB);
        }
        Ok(self.dist[a][b])
    }

    /// 对称性（判据直接调用）。
    pub fn symmetric(&self) -> bool {
        for i in 0..self.count {
            if self.dist[i][i] != DIST_LOCAL {
                return false;
            }
            for j in 0..self.count {
                if self.dist[i][j] != self.dist[j][i] {
                    return false;
                }
            }
        }
        true
    }
}

/// 一次绑定的结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bind {
    /// 首选（数据所在）节点。
    pub home: usize,
    /// 实际落点节点。
    pub actual: usize,
    /// 分配字节数。
    pub bytes: u64,
    /// 是否为溢出（actual ≠ home）。
    pub spilled: bool,
}

impl Bind {
    /// 折算成放置决策器的成本单位（本地 = bytes，跨节点按距离放大）。
    pub fn cost_units(&self, t: &Topology) -> u64 {
        let d = t.distance(self.home, self.actual).unwrap_or(DIST_LOCAL) as u64;
        self.bytes * d / DIST_LOCAL as u64
    }
}

/// 每节点已用量（MiB）。
pub struct Allocator {
    used: [u32; MAX_NODES],
    pub spills: u64,
    pub total_bytes: u64,
    pub local_bytes: u64,
}

impl Allocator {
    pub const fn new() -> Allocator {
        Allocator { used: [0; MAX_NODES], spills: 0, total_bytes: 0, local_bytes: 0 }
    }

    pub fn used_on(&self, node: usize) -> u32 {
        if node < MAX_NODES {
            self.used[node]
        } else {
            0
        }
    }

    /// 在 `home` 节点上绑定 `mib` 内存；不足则按距离升序溢出。
    ///
    /// **溢出按距离排序**而不是按节点号：两路平台上这没区别，但在 4 CCD
    /// 的消费级芯片上，节点 3 的邻居可能是 2（近）而不是 0（远），按号
    /// 溢出会把一半流量送到最远的 die 上。
    pub fn bind(&mut self, t: &Topology, home: usize, mib: u32) -> Result<Bind, u16> {
        if t.count() == 0 {
            return Err(E_NU_EMPTY_TOPO);
        }
        if home >= t.count() {
            return Err(E_NU_NODE_OOB);
        }
        let bytes = mib as u64 * 1024 * 1024;
        self.total_bytes += bytes;
        // 本地优先
        let cap = t.node(home)?.mem_mib;
        if self.used[home] + mib <= cap {
            self.used[home] += mib;
            self.local_bytes += bytes;
            return Ok(Bind { home, actual: home, bytes, spilled: false });
        }
        // 溢出：候选按 (距离, 节点号) 排序
        let mut cand: Vec<(u16, usize)> = Vec::new();
        for i in 0..t.count() {
            if i == home {
                continue;
            }
            if self.used[i] + mib <= t.node(i)?.mem_mib {
                cand.push((t.distance(home, i)?, i));
            }
        }
        if cand.is_empty() {
            return Err(E_NU_NO_CAPACITY);
        }
        cand.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let pick = cand[0].1;
        self.used[pick] += mib;
        self.spills += 1;
        Ok(Bind { home, actual: pick, bytes, spilled: true })
    }

    /// 本地率（‰）。
    pub fn local_permille(&self) -> u32 {
        if self.total_bytes == 0 {
            return 1000;
        }
        ((self.local_bytes * 1000) / self.total_bytes) as u32
    }
}

/// 亲和调度：把任务绑到离它的内存最近的节点上跑。
pub fn affinity_node(t: &Topology, data_nodes: &[usize]) -> Result<usize, u16> {
    if data_nodes.is_empty() {
        return Ok(0);
    }
    let mut best = data_nodes[0];
    let mut best_cost: u64 = 0;
    for i in 0..t.count() {
        let mut cost = 0u64;
        for d in data_nodes {
            cost += t.distance(i, *d)? as u64;
        }
        if i == best {
            best_cost = cost;
            continue;
        }
        if cost < best_cost || best_cost == 0 {
            best = i;
            best_cost = cost;
        }
    }
    Ok(best)
}

/// 跨节点拷贝的有效带宽（MB/s）：本地 12GB/s 基准，按距离反比缩放。
pub fn cross_bw_mbps(local_mbps: u32, dist: u16) -> u32 {
    if dist == 0 {
        return 0;
    }
    (local_mbps as u64 * DIST_LOCAL as u64 / dist as u64) as u32
}

/// 单路平台零退化：`Topology::single` 下绑定与定价必须与「无 NUMA」一致。
pub fn zero_regression_baseline(mib: u32) -> u64 {
    // 无 NUMA 逻辑时的成本就是字节数本身
    mib as u64 * 1024 * 1024
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// run_cga20_checks：CGPU-F0020 判据聚合（0xCD 段）。
pub fn run_cga20_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0020 NUMA 感知");

    // 正-01：拓扑对称且对角为本地距离
    {
        let t = Topology::dual(8, 16_384);
        if t.symmetric() && t.count() == 2 {
            s.ok("F0020-正-01-拓扑对称对角本地");
        } else {
            s.fail("F0020-正-01-拓扑对称对角本地", "拓扑矩阵不合法");
        }
    }

    // 正-02：本地优先——容量足够时绝不溢出
    {
        let t = Topology::dual(8, 16_384);
        let mut a = Allocator::new();
        let b = a.bind(&t, 0, 1_024).unwrap_or(Bind { home: 0, actual: 9, bytes: 0, spilled: true });
        if b.actual == 0 && !b.spilled && a.local_permille() == 1000 {
            s.ok("F0020-正-02-本地优先");
        } else {
            s.fail("F0020-正-02-本地优先", "容量充足仍溢出");
        }
    }

    // 正-03：溢出按距离升序（4 节点环：home=0，邻居 1/3 近、2 远）
    {
        let nodes = [
            Node { id: 0, cpus: 4, mem_mib: 1 },
            Node { id: 1, cpus: 4, mem_mib: 999 },
            Node { id: 2, cpus: 4, mem_mib: 999 },
            Node { id: 3, cpus: 4, mem_mib: 999 },
        ];
        let mut d = [[0u16; MAX_NODES]; MAX_NODES];
        // 0-1 与 0-3 近(12)，0-2 远(21)
        let table = [
            [10, 12, 21, 12],
            [12, 10, 12, 21],
            [21, 12, 10, 12],
            [12, 21, 12, 10],
        ];
        for i in 0..4 {
            for j in 0..4 {
                d[i][j] = table[i][j];
            }
        }
        let t = Topology::from_parts(&nodes, &d).unwrap_or(Topology::empty());
        let mut a = Allocator::new();
        let b = a.bind(&t, 0, 10).unwrap_or(Bind { home: 0, actual: 9, bytes: 0, spilled: true });
        // 节点 1 与 3 同为距离 12，按节点号取 1
        if b.actual == 1 && b.spilled {
            s.ok("F0020-正-03-溢出按距离升序");
        } else {
            s.fail("F0020-正-03-溢出按距离升序", "溢出目标不是最近节点");
        }
    }

    // 正-04：跨节点定价——本地成本 = 字节数，远端按距离放大
    {
        let t = Topology::dual(8, 16_384);
        let local = Bind { home: 0, actual: 0, bytes: 1_048_576, spilled: false };
        let remote = Bind { home: 0, actual: 1, bytes: 1_048_576, spilled: true };
        let cl = local.cost_units(&t);
        let cr = remote.cost_units(&t);
        // 手算：1MiB × 21 / 10 = 2,202,009（整数除法）
        let want = 1_048_576u64 * 21 / 10;
        if cl == 1_048_576 && cr == want && cr > cl * 2 {
            s.ok("F0020-正-04-跨节点定价");
        } else {
            s.fail("F0020-正-04-跨节点定价", "成本折算不符");
        }
    }

    // 正-05：单路平台零退化（绑定与定价与无 NUMA 逐位一致）
    {
        let t = Topology::single(16, 32_768);
        let mut a = Allocator::new();
        let b = a.bind(&t, 0, 2_048).unwrap_or(Bind { home: 0, actual: 9, bytes: 0, spilled: true });
        let base = zero_regression_baseline(2_048);
        if b.actual == 0 && !b.spilled && b.cost_units(&t) == base && a.local_permille() == 1000 {
            s.ok("F0020-正-05-单路零退化");
        } else {
            s.fail("F0020-正-05-单路零退化", "单路平台出现退化");
        }
    }

    // 正-06：亲和调度——数据全在节点 1 时，应选节点 1 执行
    {
        let t = Topology::dual(8, 16_384);
        let n = affinity_node(&t, &[1, 1, 1, 1]).unwrap_or(9);
        if n == 1 {
            s.ok("F0020-正-06-亲和调度落点");
        } else {
            s.fail("F0020-正-06-亲和调度落点", "亲和落点错");
        }
    }

    // 正-07：跨节点带宽按距离反比缩放（本地 12000 → 远端 ≈5714）
    {
        let local = cross_bw_mbps(12_000, DIST_LOCAL);
        let remote = cross_bw_mbps(12_000, DIST_CROSS_SOCKET);
        if local == 12_000 && remote == 12_000 * 10 / 21 && remote < local / 2 + 1 {
            s.ok("F0020-正-07-跨节点带宽缩放");
        } else {
            s.fail("F0020-正-07-跨节点带宽缩放", "带宽模型不符");
        }
    }

    // 正-08：容量耗尽时全平台拒绝（不静默降级到「假装成功」）
    {
        let t = Topology::dual(4, 1_024);
        let mut a = Allocator::new();
        let _ = a.bind(&t, 0, 1_024);
        let _ = a.bind(&t, 1, 1_024);
        let r = a.bind(&t, 0, 1_024);
        if matches!(r, Err(E_NU_NO_CAPACITY)) {
            s.ok("F0020-正-08-容量耗尽显性拒");
        } else {
            s.fail("F0020-正-08-容量耗尽显性拒", "容量耗尽未拒");
        }
    }

    // 坏-01：节点越界即拒
    {
        let t = Topology::dual(4, 1_024);
        if matches!(t.node(2), Err(E_NU_NODE_OOB)) && matches!(t.distance(0, 2), Err(E_NU_NODE_OOB)) {
            s.ok("F0020-坏-01-节点越界即拒");
        } else {
            s.fail("F0020-坏-01-节点越界即拒", "越界未拒");
        }
    }

    // 坏-02：非对称距离表即拒
    {
        let nodes = [
            Node { id: 0, cpus: 4, mem_mib: 100 },
            Node { id: 1, cpus: 4, mem_mib: 100 },
        ];
        let mut d = [[0u16; MAX_NODES]; MAX_NODES];
        d[0][0] = 10;
        d[1][1] = 10;
        d[0][1] = 12;
        d[1][0] = 21; // 故意不对称
        if matches!(Topology::from_parts(&nodes, &d), Err(E_NU_ASYMMETRIC)) {
            s.ok("F0020-坏-02-非对称矩阵即拒");
        } else {
            s.fail("F0020-坏-02-非对称矩阵即拒", "非对称未拒");
        }
    }

    // 坏-03：空拓扑即拒
    {
        let nodes: [Node; 0] = [];
        let d = [[0u16; MAX_NODES]; MAX_NODES];
        if matches!(Topology::from_parts(&nodes, &d), Err(E_NU_EMPTY_TOPO)) {
            s.ok("F0020-坏-03-空拓扑即拒");
        } else {
            s.fail("F0020-坏-03-空拓扑即拒", "空拓扑未拒");
        }
    }

    // 编-01：两路整链——12 次 1GiB 分配，第 7 次起 0 号不足需溢出
    {
        let t = Topology::dual(8, 6_144); // 每节点 6GiB
        let mut a = Allocator::new();
        let mut ok = true;
        for _ in 0..12 {
            match a.bind(&t, 0, 1_024) {
                Ok(b) => {
                    if b.spilled && b.actual != 1 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        // 手算：0 号 6 次满 → 第 7..12 次共 6 次溢出到 1 号
        if ok && a.spills == 6 && a.local_permille() == 500 {
            s.ok("F0020-编-01-两路溢出账目");
        } else {
            s.fail("F0020-编-01-两路溢出账目", "溢出账目不符");
        }
    }

    s
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    450
}

/// 读屏画像。
pub fn spoken_summary() -> String {
    let mut s = String::from("NUMA 感知：距离矩阵对称校验，");
    s.push_str("绑定本地优先、溢出按距离升序，单路零退化");
    s
}
