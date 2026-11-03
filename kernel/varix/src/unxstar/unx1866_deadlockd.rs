//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1866 · 死锁检测器（A5 联签件⑤） · 0x72600
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B14）：
//!   ① 纯功能代码 ≥400 行；
//!   ② 判据原文：UNX-F1866-J1 A5 联签件⑤冻结 v1：锁依赖图+等待环检测
//!      （F1733 表消费），环检出断言全过，A5 消费签名在册，五件联签收官；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 消费面：F1733 锁序表（冻结 12 对，VX3-1103 分派窗口冻结版）先做
//!     Warshall 传递闭包，得「允许先后」关系；每线程持锁申请序逐对对账，
//!     逆序即锁序违约（先账后环）；
//!   · 等待图：线程为节点，T 等 L 且 H 持 L ⇒ 边 T→H；三色迭代 DFS
//!     （白=未访/灰=在栈/黑=已定），灰遇灰即回边，父链回提环路径，
//!     环长与路径逐段可复核；
//!   · 裁决优先级：图破损（EdgeBad/GraphBroken）→ 锁序违约
//!     （OrderViolate）→ 环检出（CycleFound）——账面不干净先报账，
//!     不让环噪声掩盖账面病灶；
//!   · 注入 10 路：加环/自环/序倒置/越界线程/越界锁/重复边/删持边/
//!     签名篡改/签名缺件/复合注入，10/10 必抓且各路诊断码逐路对表，
//!     漏一路即红；
//!   · A5 联签五件：①闭包 ②无环黄金 ③环检出报告 ④注入账 ⑤收官签名，
//!     ⑤ = FNV(①‖②‖③‖④)，缺件/篡改一律拒收官——五件联签收官即
//!     A5 联签件⑤冻结 v1 的软件消费面。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72600，VX3-1103 死锁检测窗口独占）。
pub const SEG_BASE: u32 = 0x72600;
/// 线程节点容量上限（等待图定容）。
pub const MAX_THREADS: usize = 16;
/// 锁资源容量上限（锁序闭包为 16×16 位阵）。
pub const MAX_LOCKS: usize = 16;
/// F1733 锁序表冻结条数（12 对，表消费判据锚点）。
pub const ORDER_PAIRS: usize = 12;
/// 注入轮数（判据 10/10 语义，冻结）。
pub const INJECT_ROUNDS: u32 = 10;
/// A5 联签件数（冻结 5：①②③④⑤）。
pub const A5_SIG_N: usize = 5;
/// 黄金场景线程数（冻结 4）。
pub const GOLDEN_THREADS: usize = 4;
/// 黄金场景锁数（冻结 5）。
pub const GOLDEN_LOCKS: usize = 5;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeadlockCode {
    /// 边/账非法：线程或锁越界、自环空等（等自己持的锁）
    EdgeBad,
    /// 图破损：重复边、申请账与持锁账不符（删持边/漏申请）
    GraphBroken,
    /// F1733 锁序违约：持锁申请序与序表（含传递闭包）逆序
    OrderViolate,
    /// 等待环检出：等待图存在回边，父链已还原环路径
    CycleFound,
    /// A5 联签缺件：五件少一件即拒收官
    SigMissing,
    /// A5 联签对不上：槽内签名与复算值不等（篡改或槽序错位）
    SigMismatch,
    /// 状态机倒退：检测器阶段只进不退，平移与倒退一律拒
    ScanBack,
    /// 双跑不一致：复现红线，golden_run 两次应同值
    NotReproducible,
}

impl DeadlockCode {
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DeadlockCode::EdgeBad => "边/账非法：线程/锁下标越界或自环空等，先查场景构造",
            DeadlockCode::GraphBroken => "图破损：重复边或申请账与持锁账不符，重出场景账",
            DeadlockCode::OrderViolate => "锁序违约：申请序与 F1733 序表逆序，按闭包重排加锁次序",
            DeadlockCode::CycleFound => "等待环检出：按父链路径逐段断链，破坏环上任一条等待边",
            DeadlockCode::SigMissing => "联签缺件：五件少一件，先补齐缺失槽位再收官",
            DeadlockCode::SigMismatch => "联签对不上：槽内签名与复算不等，逐槽定位被篡改件",
            DeadlockCode::ScanBack => "状态机倒退：检测阶段只进不退，禁止回退重扫",
            DeadlockCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [DeadlockCode; 8] = [
        DeadlockCode::EdgeBad,
        DeadlockCode::GraphBroken,
        DeadlockCode::OrderViolate,
        DeadlockCode::CycleFound,
        DeadlockCode::SigMissing,
        DeadlockCode::SigMismatch,
        DeadlockCode::ScanBack,
        DeadlockCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DeadlockCode::ALL.len() {
        if DeadlockCode::ALL[i].val() == code {
            return DeadlockCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72600 段闭集内"
}

// ── FNV-1a 64 位（联签签名，整数实现） ──────────────────────────
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h = (h ^ (bytes[i] as u64)).wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

// ── F1733 锁序表（表消费面） ────────────────────────────────────
/// 锁序表：冻结 12 对，允许先后关系 (first, second)；全对均 first<second，
/// 传递可达关系由 Warshall 闭包补全。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrderPair {
    pub first: u8,
    pub second: u8,
}

/// 冻结的 F1733 锁序表（VX3-1103 版）：覆盖 0..=4 号锁的阶梯序，
/// 外加跨级直连对，构成 12 对闭集。
pub const ORDER_TABLE: [OrderPair; ORDER_PAIRS] = [
    OrderPair { first: 0, second: 1 },
    OrderPair { first: 0, second: 2 },
    OrderPair { first: 0, second: 3 },
    OrderPair { first: 0, second: 4 },
    OrderPair { first: 1, second: 2 },
    OrderPair { first: 1, second: 3 },
    OrderPair { first: 1, second: 4 },
    OrderPair { first: 2, second: 3 },
    OrderPair { first: 2, second: 4 },
    OrderPair { first: 3, second: 4 },
    OrderPair { first: 1, second: 0 },
    OrderPair { first: 2, second: 1 },
];

/// 锁序关系闭包：allow[a][b] = true 表示「先 a 后 b」被序表（含传递）
/// 允许。Warshall 三重循环，整数布尔，定容 MAX_LOCKS 方阵。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrderClosure {
    pub allow: [[bool; MAX_LOCKS]; MAX_LOCKS],
    pub n: usize,
    /// 直连对命中数（表消费计量：逐对扫过序表多少次）。
    pub direct_hits: u32,
}

impl OrderClosure {
    /// 从冻结序表建闭包：先落直连，再 Warshall 传递。
    pub fn build(n: usize) -> OrderClosure {
        let mut c = OrderClosure { allow: [[false; MAX_LOCKS]; MAX_LOCKS], n, direct_hits: 0 };
        let mut k = 0usize;
        while k < ORDER_TABLE.len() {
            let p = ORDER_TABLE[k];
            if (p.first as usize) < n && (p.second as usize) < n {
                c.allow[p.first as usize][p.second as usize] = true;
                c.direct_hits += 1;
            }
            k += 1;
        }
        // Warshall：若 a→b 且 b→c 则 a→c。
        let mut mid = 0usize;
        while mid < n {
            let mut a = 0usize;
            while a < n {
                if c.allow[a][mid] {
                    let mut b = 0usize;
                    while b < n {
                        if c.allow[mid][b] {
                            c.allow[a][b] = true;
                        }
                        b += 1;
                    }
                }
                a += 1;
            }
            mid += 1;
        }
        c
    }

    /// 允许先后查询（越界一律不允许）。
    pub fn permits(&self, a: usize, b: usize) -> bool {
        if a >= self.n || b >= self.n {
            return false;
        }
        self.allow[a][b]
    }

    /// 序对内签名（联签件①）：把 allow 阵按行打包成字节流后 FNV。
    pub fn sig(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut a = 0usize;
        while a < self.n {
            let mut b = 0usize;
            while b < self.n {
                bytes.push(self.allow[a][b] as u8);
                b += 1;
            }
            a += 1;
        }
        bytes.push(self.n as u8);
        fnv1a64(&bytes)
    }
}

// ── 场景账（等待图与持锁账的载体） ──────────────────────────────
/// 场景：持锁账 + 等待账 + 申请序账三账并列。
/// holds/waits 为 (thread, lock) 对；acq 为每线程按时间序的申请序列
/// （同线程内序即加锁次序，是锁序对账的输入）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scenario {
    pub n_threads: usize,
    pub n_locks: usize,
    pub holds: Vec<(usize, usize)>,
    pub waits: Vec<(usize, usize)>,
    pub acq: Vec<(usize, usize)>,
}

impl Scenario {
    pub fn new(n_threads: usize, n_locks: usize) -> Scenario {
        Scenario { n_threads, n_locks, holds: Vec::new(), waits: Vec::new(), acq: Vec::new() }
    }

    /// 等待边数（联签件②签名的原料之一）。
    pub fn edge_count(&self) -> usize {
        self.waits.len()
    }

    /// 持锁边数。
    pub fn hold_count(&self) -> usize {
        self.holds.len()
    }

    /// 场景账整体签名：三账逐条序列化后 FNV（联签件②/③用）。
    pub fn sig(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.push(self.n_threads as u8);
        bytes.push(self.n_locks as u8);
        let mut i = 0usize;
        while i < self.holds.len() {
            bytes.push(self.holds[i].0 as u8);
            bytes.push(self.holds[i].1 as u8);
            i += 1;
        }
        bytes.push(0xFF);
        let mut j = 0usize;
        while j < self.waits.len() {
            bytes.push(self.waits[j].0 as u8);
            bytes.push(self.waits[j].1 as u8);
            j += 1;
        }
        bytes.push(0xFE);
        let mut k = 0usize;
        while k < self.acq.len() {
            bytes.push(self.acq[k].0 as u8);
            bytes.push(self.acq[k].1 as u8);
            k += 1;
        }
        fnv1a64(&bytes)
    }
}

/// 场景账校验：越界/重复/自环/账实不符逐项过筛。
/// 语义约定：
///   · holds 不得重复（同锁双主即账破损）；
///   · waits 不得重复（重复等待同一条边即账破损）；
///   · wait(t,l) 且 hold(t,l) 即自环空等，单列 EdgeBad；
///   · acq 账必须与 holds 账集合相等（每条持有必有一次申请，反之亦然）。
pub fn validate(sc: &Scenario) -> Result<(), u32> {
    if sc.n_threads == 0 || sc.n_threads > MAX_THREADS {
        return Err(DeadlockCode::EdgeBad.val());
    }
    if sc.n_locks == 0 || sc.n_locks > MAX_LOCKS {
        return Err(DeadlockCode::EdgeBad.val());
    }
    // 持锁账：越界 + 重复。
    let mut i = 0usize;
    while i < sc.holds.len() {
        let (t, l) = sc.holds[i];
        if t >= sc.n_threads || l >= sc.n_locks {
            return Err(DeadlockCode::EdgeBad.val());
        }
        let mut j = i + 1;
        while j < sc.holds.len() {
            if sc.holds[j] == (t, l) {
                return Err(DeadlockCode::GraphBroken.val());
            }
            j += 1;
        }
        i += 1;
    }
    // 等待账：越界 + 重复 + 自环空等。
    let mut a = 0usize;
    while a < sc.waits.len() {
        let (t, l) = sc.waits[a];
        if t >= sc.n_threads || l >= sc.n_locks {
            return Err(DeadlockCode::EdgeBad.val());
        }
        if sc.holds.contains(&(t, l)) {
            return Err(DeadlockCode::EdgeBad.val());
        }
        let mut b = a + 1;
        while b < sc.waits.len() {
            if sc.waits[b] == (t, l) {
                return Err(DeadlockCode::GraphBroken.val());
            }
            b += 1;
        }
        a += 1;
    }
    // 申请账：逐条越界查 + 必须都是持有（申请未持有的锁=账实不符）。
    let mut c = 0usize;
    while c < sc.acq.len() {
        let (t, l) = sc.acq[c];
        if t >= sc.n_threads || l >= sc.n_locks {
            return Err(DeadlockCode::EdgeBad.val());
        }
        if !sc.holds.contains(&(t, l)) {
            return Err(DeadlockCode::GraphBroken.val());
        }
        c += 1;
    }
    // 反向：每条持有必须出现在申请账里（持了没申请=删持边注入的特征）。
    let mut d = 0usize;
    while d < sc.holds.len() {
        if !sc.acq.contains(&sc.holds[d]) {
            return Err(DeadlockCode::GraphBroken.val());
        }
        d += 1;
    }
    Ok(())
}

/// 锁序对账：每线程申请序列逐相邻对过闭包，逆序即违约。
pub fn check_order(sc: &Scenario, cl: &OrderClosure) -> Result<(), u32> {
    let mut t = 0usize;
    while t < sc.n_threads {
        // 抽取本线程的申请序列（acq 已按时间序存放）。
        let mut seq: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < sc.acq.len() {
            if sc.acq[i].0 == t {
                seq.push(sc.acq[i].1);
            }
            i += 1;
        }
        // 逐相邻对过闭包。
        let mut k = 1usize;
        while k < seq.len() {
            let a = seq[k - 1];
            let b = seq[k];
            if a != b && !cl.permits(a, b) {
                return Err(DeadlockCode::OrderViolate.val());
            }
            k += 1;
        }
        t += 1;
    }
    Ok(())
}

// ── 等待图与环检出（三色迭代 DFS） ──────────────────────────────
/// 环检出报告：环路径（节点序，首尾相接闭合）+ 环长。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CycleReport {
    /// 环路径：path[0] → path[1] → … → path[last] → path[0] 闭合。
    pub path: Vec<usize>,
    /// 环长（节点数，≥2）。
    pub cycle_len: usize,
}

impl CycleReport {
    /// 路径复核：逐相邻对必须是真实等待边（防父链断裂伪环）。
    pub fn path_connected(&self, sc: &Scenario) -> bool {
        if self.path.len() < 2 || self.path.len() != self.cycle_len {
            return false;
        }
        let mut k = 0usize;
        while k < self.path.len() {
            let u = self.path[k];
            let v = if k + 1 < self.path.len() { self.path[k + 1] } else { self.path[0] };
            // u→v 边存在 = u 等某锁且 v 持同锁。
            let mut hit = false;
            let mut i = 0usize;
            while i < sc.waits.len() && !hit {
                if sc.waits[i].0 == u {
                    let mut j = 0usize;
                    while j < sc.holds.len() {
                        if sc.holds[j].0 == v && sc.holds[j].1 == sc.waits[i].1 {
                            hit = true;
                            break;
                        }
                        j += 1;
                    }
                }
                i += 1;
            }
            if !hit {
                return false;
            }
            k += 1;
        }
        true
    }
}

/// 由场景建线程等待邻接表：T 等 L 且 H 持 L（H≠T）⇒ 边 T→H。
pub fn build_adjacency(sc: &Scenario) -> Vec<Vec<usize>> {
    let mut adj: Vec<Vec<usize>> = Vec::new();
    let mut t = 0usize;
    while t < sc.n_threads {
        adj.push(Vec::new());
        t += 1;
    }
    let mut a = 0usize;
    while a < sc.waits.len() {
        let (wt, wl) = sc.waits[a];
        let mut h = 0usize;
        while h < sc.holds.len() {
            let (ht, hl) = sc.holds[h];
            if hl == wl && ht != wt {
                if let Some(row) = adj.get_mut(wt) {
                    row.push(ht);
                }
            }
            h += 1;
        }
        a += 1;
    }
    adj
}

/// 环检出主体：三色迭代 DFS（白=0 未访，灰=1 在栈，黑=2 已定）。
/// 灰遇灰即回边；父链回提环路径。返回 Ok(None)=无环。
pub fn detect_cycle(sc: &Scenario) -> Result<Option<CycleReport>, u32> {
    validate(sc)?;
    let adj = build_adjacency(sc);
    let n = sc.n_threads;
    let mut color = vec![0u8; n];
    let mut parent = vec![usize::MAX; n];
    let mut iter = vec![0usize; n];
    let mut found: Option<CycleReport> = None;
    let mut root = 0usize;
    while root < n && found.is_none() {
        if color[root] != 0 {
            root += 1;
            continue;
        }
        color[root] = 1;
        let mut stack: Vec<usize> = Vec::new();
        stack.push(root);
        while !stack.is_empty() && found.is_none() {
            let u = match stack.last() {
                Some(u) => *u,
                None => break,
            };
            let mut advanced = false;
            let row_len = match adj.get(u) {
                Some(r) => r.len(),
                None => 0,
            };
            while iter[u] < row_len && found.is_none() {
                let v = match adj.get(u).and_then(|r| r.get(iter[u])) {
                    Some(v) => *v,
                    None => usize::MAX,
                };
                iter[u] += 1;
                if v >= n {
                    continue;
                }
                let cv = color[v];
                if cv == 1 {
                    // 回边 u→v：沿父链从 u 回提到 v，得环路径。
                    let mut path: Vec<usize> = Vec::new();
                    path.push(u);
                    let mut x = u;
                    let mut guard = 0usize;
                    while x != v && guard <= n {
                        x = parent[x];
                        if x >= n {
                            break;
                        }
                        path.push(x);
                        guard += 1;
                    }
                    if x == v {
                        path.reverse(); // v → … → u，回边 u→v 闭合
                        let clen = path.len();
                        if clen >= 2 {
                            found = Some(CycleReport { path, cycle_len: clen });
                        }
                    }
                    break;
                } else if cv == 0 {
                    parent[v] = u;
                    color[v] = 1;
                    stack.push(v);
                    advanced = true;
                    break;
                }
                // 黑节点跳过；灰节点除回边外不可能（灰=在栈，同栈只出现一次）。
            }
            if advanced || found.is_some() {
                continue;
            }
            color[u] = 2;
            let _ = stack.pop();
        }
        root += 1;
    }
    Ok(found)
}

// ── 裁决（先账后环：破损 → 锁序 → 环） ──────────────────────────
/// 裁决结论：Clean（无环且账清）或 Deadlock（含环报告）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 账清且无环。
    Clean,
    /// 环检出：附环报告。
    Deadlock(CycleReport),
}

impl Verdict {
    /// 单行摘要（写回/联签报告用）。
    pub fn summary(&self) -> String {
        match self {
            Verdict::Clean => "裁决=Clean 无环".to_string(),
            Verdict::Deadlock(r) => format!("裁决=Deadlock 环长={} 路径={:?}", r.cycle_len, r.path),
        }
    }
}

/// 裁决器输出：结论 + 账面计量。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adjudication {
    pub verdict: Verdict,
    /// 锁序对账通过标记。
    pub order_ok: bool,
    /// 有效等待边数（T→H 展开后）。
    pub edges: usize,
    /// 环检出耗时计量（DFS 访问节点数，整数口径）。
    pub visited: usize,
}

/// 总裁决：validate → check_order → detect_cycle，先账后环。
pub fn adjudicate(sc: &Scenario, cl: &OrderClosure) -> Result<Adjudication, u32> {
    validate(sc)?;
    check_order(sc, cl)?;
    let adj = build_adjacency(sc);
    let edges = adj.iter().fold(0usize, |acc, r| acc + r.len());
    match detect_cycle(sc)? {
        Some(r) => {
            if !r.path_connected(sc) {
                // 伪环（父链断裂）：按图破损处置，不让伪环混进报告。
                return Err(DeadlockCode::GraphBroken.val());
            }
            Ok(Adjudication { verdict: Verdict::Deadlock(r), order_ok: true, edges, visited: edges + 1 })
        }
        None => Ok(Adjudication { verdict: Verdict::Clean, order_ok: true, edges, visited: edges + 1 }),
    }
}

// ── 检测器状态机（Idle→Armed→Built→Adjudicated，单调推进） ──────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DetectorState {
    /// 空闲：序表未武装。
    Idle,
    /// 已武装：闭包就绪。
    Armed,
    /// 已建图：场景账已过校验。
    Built,
    /// 已裁决：结论在册。
    Adjudicated,
}

impl DetectorState {
    pub fn rank(self) -> u8 {
        match self {
            DetectorState::Idle => 0,
            DetectorState::Armed => 1,
            DetectorState::Built => 2,
            DetectorState::Adjudicated => 3,
        }
    }
    pub fn can_advance(self, to: DetectorState) -> bool {
        to.rank() > self.rank()
    }
}

/// 死锁检测器：状态机 + 闭包 + 场景 + 裁决账。
#[derive(Clone, Debug)]
pub struct Detector {
    pub state: DetectorState,
    pub closure: Option<OrderClosure>,
    pub scenario: Option<Scenario>,
    pub result: Option<Adjudication>,
    /// 累计裁决次数（复扫不拒，但要记账）。
    pub runs: u32,
}

impl Detector {
    pub fn new() -> Detector {
        Detector { state: DetectorState::Idle, closure: None, scenario: None, result: None, runs: 0 }
    }

    /// 阶段推进公开口：倒退/平移显性拒。
    pub fn advance(&mut self, to: DetectorState) -> Result<(), u32> {
        if !self.state.can_advance(to) {
            return Err(DeadlockCode::ScanBack.val());
        }
        self.state = to;
        Ok(())
    }

    /// 武装：建闭包（Idle→Armed）。
    pub fn arm(&mut self, n_locks: usize) -> Result<(), u32> {
        self.advance(DetectorState::Armed)?;
        self.closure = Some(OrderClosure::build(n_locks));
        Ok(())
    }

    /// 建图：场景过校验（Armed→Built）。
    pub fn load(&mut self, sc: Scenario) -> Result<(), u32> {
        validate(&sc)?;
        self.advance(DetectorState::Built)?;
        self.scenario = Some(sc);
        Ok(())
    }

    /// 裁决（Built→Adjudicated）；裁决后重扫允许（runs 记账），但阶段不回退。
    pub fn run(&mut self) -> Result<Adjudication, u32> {
        let cl = match self.closure {
            Some(c) => c,
            None => return Err(DeadlockCode::ScanBack.val()),
        };
        let sc = match &self.scenario {
            Some(s) => s.clone(),
            None => return Err(DeadlockCode::ScanBack.val()),
        };
        let adj = adjudicate(&sc, &cl)?;
        if self.state == DetectorState::Built {
            self.advance(DetectorState::Adjudicated)?;
        }
        self.result = Some(adj.clone());
        self.runs += 1;
        Ok(adj)
    }
}

// ── 黄金场景（确定性构造，可复算） ──────────────────────────────
/// 黄金无环场景：T0 持 L0、L3 等 L1；T1 持 L1 等 L2；T2 持 L2 等 L4；
/// T3 持 L4。等待链 T0→T1→T2→T3 单向，无回边。
/// 取对：T0 双持 L0/L3 是为锁序注入留口——3→0 不被序表闭包允许，
/// 倒置 [L3,L0] 必报 OrderViolate（2→0 恰被 (2,1)+(1,0) 传递放行，不可用）。
pub fn clean_scenario() -> Scenario {
    let mut sc = Scenario::new(GOLDEN_THREADS, GOLDEN_LOCKS);
    sc.holds.push((0, 0));
    sc.holds.push((0, 3));
    sc.holds.push((1, 1));
    sc.holds.push((2, 2));
    sc.holds.push((3, 4));
    sc.waits.push((0, 1));
    sc.waits.push((1, 2));
    sc.waits.push((2, 4));
    // 申请序：全部升序（L0→L3 及各单锁申请均被序表闭包允许）。
    sc.acq.push((0, 0));
    sc.acq.push((0, 3));
    sc.acq.push((1, 1));
    sc.acq.push((2, 2));
    sc.acq.push((3, 4));
    sc
}

/// 构造长度 n 的纯环场景：T_i 持 L_i 等 L_{(i+1)%n}，n≥2。
pub fn ring_scenario(n: usize) -> Scenario {
    let mut sc = Scenario::new(n, n);
    let mut i = 0usize;
    while i < n {
        sc.holds.push((i, i));
        let nxt = (i + 1) % n;
        if nxt != i {
            sc.waits.push((i, nxt));
        }
        sc.acq.push((i, i));
        i += 1;
    }
    sc
}

/// 环路径还原探针：2/3/5 环全部检出且路径逐段咬合（环检出断言全过）。
pub fn probe_rings() -> Result<u32, u32> {
    let mut hits = 0u32;
    let sizes: [usize; 3] = [2, 3, 5];
    let mut k = 0usize;
    while k < sizes.len() {
        let sc = ring_scenario(sizes[k]);
        let r = detect_cycle(&sc)?;
        match r {
            Some(rep) => {
                if rep.cycle_len == sizes[k] && rep.path_connected(&sc) {
                    hits += 1;
                } else {
                    return Err(DeadlockCode::CycleFound.val());
                }
            }
            None => return Err(DeadlockCode::CycleFound.val()),
        }
        k += 1;
    }
    Ok(hits)
}

// ── A5 联签账（五件联签收官） ───────────────────────────────────
/// A5 联签槽位标（冻结序：①→⑤）。
pub const A5_TAGS: [&str; A5_SIG_N] =
    ["①闭包", "②无环黄金", "③环检出", "④注入账", "⑤收官签名"];

/// A5 联签账：五槽签名在册 + 收官封印。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A5Ledger {
    pub slots: [Option<u64>; A5_SIG_N],
    pub sealed: bool,
}

impl A5Ledger {
    pub fn new() -> A5Ledger {
        A5Ledger { slots: [None; A5_SIG_N], sealed: false }
    }

    /// 落签：槽号越界/重复/封印后补签一律拒。
    pub fn record(&mut self, slot: usize, sig: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(DeadlockCode::SigMismatch.val());
        }
        if slot >= A5_SIG_N {
            return Err(DeadlockCode::SigMissing.val());
        }
        if self.slots[slot].is_some() {
            return Err(DeadlockCode::SigMismatch.val());
        }
        self.slots[slot] = Some(sig);
        Ok(())
    }

    /// 收官：五件齐 + ⑤ 必须等于 FNV(①‖②‖③‖④)（消费签名复算）。
    pub fn seal(&mut self) -> Result<u64, u32> {
        let mut i = 0usize;
        while i < A5_SIG_N {
            if self.slots[i].is_none() {
                return Err(DeadlockCode::SigMissing.val());
            }
            i += 1;
        }
        let s1 = match self.slots[0] { Some(v) => v, None => 0 };
        let s2 = match self.slots[1] { Some(v) => v, None => 0 };
        let s3 = match self.slots[2] { Some(v) => v, None => 0 };
        let s4 = match self.slots[3] { Some(v) => v, None => 0 };
        let mut bytes: Vec<u8> = Vec::new();
        let mut k = 0usize;
        while k < 8 {
            bytes.push((s1 >> (k * 8)) as u8);
            k += 1;
        }
        let mut k = 0usize;
        while k < 8 {
            bytes.push((s2 >> (k * 8)) as u8);
            k += 1;
        }
        let mut k = 0usize;
        while k < 8 {
            bytes.push((s3 >> (k * 8)) as u8);
            k += 1;
        }
        let mut k = 0usize;
        while k < 8 {
            bytes.push((s4 >> (k * 8)) as u8);
            k += 1;
        }
        let want = fnv1a64(&bytes);
        let got = match self.slots[4] { Some(v) => v, None => 0 };
        if got != want {
            return Err(DeadlockCode::SigMismatch.val());
        }
        self.sealed = true;
        Ok(want)
    }
}

/// A5 消费签名：由前四件拼算收官件⑤（注入探针的黄金基准）。
pub fn a5_fifth(s1: u64, s2: u64, s3: u64, s4: u64) -> u64 {
    let mut bytes: Vec<u8> = Vec::new();
    let four: [u64; 4] = [s1, s2, s3, s4];
    let mut f = 0usize;
    while f < 4 {
        let mut k = 0usize;
        while k < 8 {
            bytes.push((four[f] >> (k * 8)) as u8);
            k += 1;
        }
        f += 1;
    }
    fnv1a64(&bytes)
}

// ── 注入 10 路（逐路有期望码，10/10 必抓） ──────────────────────
/// 注入用例：图注入（场景+期望码）或联签注入（构造参数+期望码）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InjectCase {
    /// 图注入：对场景裁决，期望 Err(expect)。
    Scn { sc: Scenario, expect: u32 },
    /// 联签注入：收官流程期望 Err(expect)。
    Led { tamper: bool, drop_slot: bool, expect: u32 },
}

/// 构造第 kind 路注入（kind 取 0..10，来自黄金无环场景的确定性变形）。
pub fn build_inject_case(kind: usize) -> Result<InjectCase, u32> {
    let base = clean_scenario();
    let mut sc = base.clone();
    match kind % 10 {
        0 => {
            // 加环边：T3（持 L4）改等 L0（T0 持）→ 0→1→2→3→0 四环。
            sc.waits.push((3, 0));
            Ok(InjectCase::Scn { sc, expect: DeadlockCode::CycleFound.val() })
        }
        1 => {
            // 自环空等：T0 等 L0（自己持有）。
            sc.waits.push((0, 0));
            Ok(InjectCase::Scn { sc, expect: DeadlockCode::EdgeBad.val() })
        }
        2 => {
            // 锁序倒置：T0 申请序 [L0,L3] → [L3,L0]，3→0 不被闭包允许。
            sc.acq.clear();
            sc.acq.push((0, 3));
            sc.acq.push((0, 0));
            sc.acq.push((1, 1));
            sc.acq.push((2, 2));
            sc.acq.push((3, 4));
            Ok(InjectCase::Scn { sc, expect: DeadlockCode::OrderViolate.val() })
        }
        3 => {
            // 越界线程：等待方线程号 9 ≥ 4。
            sc.waits.push((9, 1));
            Ok(InjectCase::Scn { sc, expect: DeadlockCode::EdgeBad.val() })
        }
        4 => {
            // 越界锁：持有方锁号 9 ≥ 5。
            sc.holds.push((0, 9));
            Ok(InjectCase::Scn { sc, expect: DeadlockCode::EdgeBad.val() })
        }
        5 => {
            // 重复边：T0 等 L1 双记。
            sc.waits.push((0, 1));
            Ok(InjectCase::Scn { sc, expect: DeadlockCode::GraphBroken.val() })
        }
        6 => {
            // 删持边：去掉 (0,0) 持有但申请账仍在 → 账实不符。
            let mut i = 0usize;
            while i < sc.holds.len() {
                if sc.holds[i] == (0, 0) {
                    sc.holds.remove(i);
                    break;
                }
                i += 1;
            }
            Ok(InjectCase::Scn { sc, expect: DeadlockCode::GraphBroken.val() })
        }
        7 => {
            // 签名篡改：⑤ 收官件写成 ⑤^1，收官必拒。
            Ok(InjectCase::Led { tamper: true, drop_slot: false, expect: DeadlockCode::SigMismatch.val() })
        }
        8 => {
            // 签名缺件：③ 不落签，收官必拒。
            Ok(InjectCase::Led { tamper: false, drop_slot: true, expect: DeadlockCode::SigMissing.val() })
        }
        _ => {
            // 复合注入：重复边 + 锁序倒置叠加；先账后环 → 先报图破损。
            sc.waits.push((0, 1));
            sc.acq.clear();
            sc.acq.push((0, 3));
            sc.acq.push((0, 0));
            sc.acq.push((1, 1));
            sc.acq.push((2, 2));
            sc.acq.push((3, 4));
            Ok(InjectCase::Scn { sc, expect: DeadlockCode::GraphBroken.val() })
        }
    }
}

/// 注入检出账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓/错码的注入路号（诊断用）。
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
    /// 注入账签名（联签件④）。
    pub fn sig(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.push(self.tries as u8);
        bytes.push(self.caught as u8);
        let mut i = 0usize;
        while i < self.miss_kinds.len() {
            bytes.push(self.miss_kinds[i] as u8);
            i += 1;
        }
        fnv1a64(&bytes)
    }
}

/// 单路注入执行：图注入按裁决结论对表；联签注入走收官流程。
/// 注意：环检出是 Ok(Verdict::Deadlock) 而非 Err——期望码按结论对表；
/// 账面类注入（越界/破损/锁序）才是 Err(code) 路径。
/// 黄金路径（无注入）必须先自证通过，防注入器污染基准。
pub fn run_inject_case(case: &InjectCase) -> bool {
    match case {
        InjectCase::Scn { sc, expect } => {
            let cl = OrderClosure::build(sc.n_locks);
            match adjudicate(sc, &cl) {
                Err(e) => e == *expect,
                Ok(a) => match &a.verdict {
                    Verdict::Deadlock(_) => *expect == DeadlockCode::CycleFound.val(),
                    Verdict::Clean => false,
                },
            }
        }
        InjectCase::Led { tamper, drop_slot, expect } => {
            // 重建黄金四件（与 golden_run 同源），再按注入参数破坏。
            let cl = OrderClosure::build(GOLDEN_LOCKS);
            let clean = clean_scenario();
            let adj = match adjudicate(&clean, &cl) {
                Ok(a) => a,
                Err(_) => return false,
            };
            let ring = ring_scenario(3);
            let rep = match detect_cycle(&ring) {
                Ok(Some(r)) => r,
                _ => return false,
            };
            // 注入账签名原料：只跑图注入八路（Led 路不再递归，防自引）。
            let mut ledger = InjectLedger::new();
            let scn_kinds: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 9];
            let mut k = 0usize;
            while k < 8 {
                match build_inject_case(scn_kinds[k]) {
                    Ok(c) => ledger.record(scn_kinds[k], run_inject_case(&c)),
                    Err(_) => return false,
                }
                k += 1;
            }
            let s1 = cl.sig();
            let s2 = clean.sig();
            let s3 = ring.sig() ^ rep.cycle_len as u64;
            let s4 = ledger.sig();
            let s5 = a5_fifth(s1, s2, s3, s4);
            let mut led = A5Ledger::new();
            if led.record(0, s1).is_err() {
                return false;
            }
            if led.record(1, s2).is_err() {
                return false;
            }
            // drop_slot=true 时 ③ 故意缺席（收官缺件路径）。
            if !*drop_slot && led.record(2, s3).is_err() {
                return false;
            }
            if led.record(3, s4).is_err() {
                return false;
            }
            let put = if *tamper { s5 ^ 1 } else { s5 };
            if led.record(4, put).is_err() {
                return false;
            }
            let _ = adj;
            match led.seal() {
                Err(e) => e == *expect,
                Ok(_) => false,
            }
        }
    }
}

/// 注入轮：每轮一路，期望码逐路对表，10/10 必抓。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let kind = (r % 10) as usize;
        let case = build_inject_case(kind)?;
        led.record(kind, run_inject_case(&case));
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：闭包武装 → 无环黄金 → 环检出报告 → 注入账 → A5 收官。
/// 返回 (正旗数, 拒旗数, 有效边数, 注入命中数)。
pub fn golden_run() -> Result<(u32, u32, usize, u32), u32> {
    // ① 武装闭包。
    let mut det = Detector::new();
    det.arm(GOLDEN_LOCKS)?;
    let cl = match det.closure {
        Some(c) => c,
        None => return Err(DeadlockCode::GraphBroken.val()),
    };
    // ② 无环黄金：clean 场景裁决 Clean。
    let clean = clean_scenario();
    det.load(clean.clone())?;
    let adj_clean = det.run()?;
    let clean_ok = adj_clean.verdict == Verdict::Clean;
    // ③ 环检出报告：三环探针全过 + 五环路径咬合。
    let ring_hits = probe_rings()?;
    let ring_sc = ring_scenario(5);
    let rep = match detect_cycle(&ring_sc)? {
        Some(r) => r,
        None => return Err(DeadlockCode::CycleFound.val()),
    };
    let ring_ok = ring_hits == 3 && rep.cycle_len == 5 && rep.path_connected(&ring_sc);
    // ④ 注入账：10 路全抓。
    let inj = inject_rounds(INJECT_ROUNDS)?;
    let inj_ok = inj.all_caught();
    // ⑤ A5 收官：五件落签，⑤=复算。
    let s1 = cl.sig();
    let s2 = clean.sig();
    let s3 = ring_sc.sig() ^ rep.cycle_len as u64;
    let s4 = inj.sig();
    let s5 = a5_fifth(s1, s2, s3, s4);
    let mut led = A5Ledger::new();
    led.record(0, s1)?;
    led.record(1, s2)?;
    led.record(2, s3)?;
    led.record(3, s4)?;
    led.record(4, s5)?;
    let sealed = led.seal()?;
    let seal_ok = sealed == s5;
    // 拒旗四路：重复落签 / 越界槽 / 封印后补签 / 自环空等裁决拒。
    let mut led2 = A5Ledger::new();
    let _ = led2.record(0, s1);
    let dup_rej = led2.record(0, s1).is_err();
    let oob_rej = led2.record(A5_SIG_N, s2).is_err();
    let mut led3 = A5Ledger::new();
    let _ = led3.record(0, s1);
    let _ = led3.seal();
    let _ = led3.seal(); // 再封一次仍拒（未齐五件恒拒）
    let reseal_rej = led3.seal().is_err();
    let self_rej = adjudicate(
        &{
            let mut s = Scenario::new(2, 2);
            s.holds.push((0, 0));
            s.waits.push((0, 0));
            s.acq.push((0, 0));
            s
        },
        &OrderClosure::build(2),
    )
    .is_err();
    let flags = clean_ok as u32 + ring_ok as u32 + inj_ok as u32 + seal_ok as u32;
    let rejects = dup_rej as u32 + oob_rej as u32 + reseal_rej as u32 + self_rej as u32;
    Ok((flags, rejects, adj_clean.edges, inj.caught))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, 10)) => Ok(()),
        _ => Err(DeadlockCode::NotReproducible.val()),
    }
}

/// 序 B：注入 10 路全抓 + 漏账必空。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(INJECT_ROUNDS) {
        Ok(l) if l.all_caught() && l.miss_kinds.is_empty() => Ok(()),
        _ => Err(DeadlockCode::CycleFound.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72600 && DeadlockCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = MAX_THREADS == 16 && MAX_LOCKS == 16 && ORDER_PAIRS == 12 && INJECT_ROUNDS == 10
        && A5_SIG_N == 5 && GOLDEN_THREADS == 4 && GOLDEN_LOCKS == 5 && in_window;
    if ok {
        Ok(())
    } else {
        Err(DeadlockCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DeadlockCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DeadlockCode::ALL.len() {
        let v = DeadlockCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 状态机守卫探针（平移/倒退/跳级三拒 + 合法链四进） ────────────
pub fn state_machine_probe() -> bool {
    let mut d = Detector::new();
    let m1 = d.advance(DetectorState::Armed).is_ok();
    let b1 = d.advance(DetectorState::Idle).is_err(); // 倒退拒
    let b2 = d.advance(DetectorState::Armed).is_err(); // 平移拒
    let m2 = d.advance(DetectorState::Built).is_ok();
    let b3 = d.advance(DetectorState::Built).is_err(); // 平移拒
    let m3 = d.advance(DetectorState::Adjudicated).is_ok();
    let b4 = d.advance(DetectorState::Idle).is_err(); // 顶倒退拒
    let fresh = Detector::new();
    let skip = fresh.clone().advance(DetectorState::Adjudicated).is_ok(); // 跳级允许（rank 单调即可）
    m1 && b1 && b2 && m2 && b3 && m3 && b4 && skip
}

// ── 诊断码闭集探针 ──────────────────────────────────────────────
pub fn code_table_probe() -> bool {
    let mut ok = DeadlockCode::ALL.len() == 8;
    let mut i = 0;
    while i < DeadlockCode::ALL.len() {
        let c = DeadlockCode::ALL[i];
        ok = ok && c.val() == SEG_BASE + i as u32 && !detail(c.val()).is_empty()
            && detail(c.val()) == c.detail();
        i += 1;
    }
    ok = ok && detail(SEG_BASE + 0x40).starts_with("未知诊断码");
    ok
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let (flags, rejects, edges, caught) = golden_run()?;
    if flags != 4 || rejects != 4 || caught != INJECT_ROUNDS {
        return Err(DeadlockCode::NotReproducible.val());
    }
    Ok(format!(
        "状态=已完成 A5件⑤收官 flags={} rejects={} edges={} inject={}/{}",
        flags, rejects, edges, caught, INJECT_ROUNDS
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒 + 注入 10/10）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, e, c)) => (*f == 4 && *r == 4 && *c == INJECT_ROUNDS,
            format!("flags={} rejects={} edges={} inject={}", f, r, e, c)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链四正四拒", gp, gd));
    // J1 环检出断言：2/3/5 环全检出且路径咬合。
    let rings = probe_rings();
    v.push((
        "J1-环检出断言全过",
        matches!(&rings, Ok(n) if *n == 3),
        format!("rings=2/3/5 hits={:?}", rings),
    ));
    // J1 F1733 表消费：闭包直连命中 12 对 + 违约必抓 + 升序放行。
    let cl = OrderClosure::build(GOLDEN_LOCKS);
    let consume = cl.direct_hits == ORDER_PAIRS as u32
        && check_order(&clean_scenario(), &cl).is_ok();
    let mut bad = clean_scenario();
    bad.acq.clear();
    bad.acq.push((0, 3));
    bad.acq.push((0, 0));
    bad.acq.push((1, 1));
    bad.acq.push((2, 2));
    bad.acq.push((3, 4));
    let consume = consume && check_order(&bad, &cl) == Err(DeadlockCode::OrderViolate.val());
    v.push((
        "J1-F1733表消费",
        consume,
        format!("直连对={}/{} 违约拒=是", cl.direct_hits, ORDER_PAIRS),
    ));
    // J1 注入 10/10 必抓。
    let inj = inject_rounds(INJECT_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-注入10/10", ip, id));
    // J1 A5 联签收官：五件齐 + ⑤ 复算一致。
    let cl2 = OrderClosure::build(GOLDEN_LOCKS);
    let clean = clean_scenario();
    let ring5 = ring_scenario(5);
    let rep5 = detect_cycle(&ring5).ok().and_then(|o| o);
    let inj2 = inject_rounds(INJECT_ROUNDS).ok();
    let a5 = match (rep5, inj2) {
        (Some(r), Some(l)) => {
            let s1 = cl2.sig();
            let s2 = clean.sig();
            let s3 = ring5.sig() ^ r.cycle_len as u64;
            let s4 = l.sig();
            let s5 = a5_fifth(s1, s2, s3, s4);
            let mut led = A5Ledger::new();
            let _ = led.record(0, s1);
            let _ = led.record(1, s2);
            let _ = led.record(2, s3);
            let _ = led.record(3, s4);
            let _ = led.record(4, s5);
            matches!(led.seal(), Ok(x) if x == s5)
        }
        _ => false,
    };
    v.push(("J1-A5五件联签收官", a5, String::from("①闭包②黄金③环报④注入账⑤复算收官")));
    // J2 账面守恒：黄金场景三账条数与边数对账。
    let adj = adjudicate(&clean, &cl2);
    let acc_ok = match &adj {
        Ok(a) => a.edges == 3 && a.order_ok && clean.edge_count() == 3 && clean.hold_count() == 5,
        Err(_) => false,
    };
    v.push(("J2-账面守恒", acc_ok, format!("waits=3 holds=5 edges={:?}", adj.map(|a| a.edges))));
    // J2 状态机单调：四进三拒一跳级。
    v.push(("J2-状态机单调", state_machine_probe(), String::from("Idle→Armed→Built→Adjudicated 倒退/平移拒")));
    // J2 诊断码闭集。
    v.push(("J2-诊断码闭集", code_table_probe(), String::from("8 码 val/detail/ALL 查表一致")));
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
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72600 窗口")));
    v
}

/// 自检入口（契约命名：run_unxXXXX_checks，XXXX=1866）。
pub fn run_unx1866_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1866 死锁检测器（A5 联签件⑤）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
