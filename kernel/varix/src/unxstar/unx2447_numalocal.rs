//! UNX-F2447 · 分配 CPU 亲和与 NUMA 本地命中 · 诊断段 0x7050（独占 0x7050~0x705F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B03 每 CPU 池批 · UNX-F2447）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2447-J1 NUMA 本地命中率 ≥90%（样板判据承载），远端命中逐笔
//! 留账。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 本地命中 ≥9000‱：2 节点 × 4 核拓扑，分配亲和路由（核→
//!   本地节点池）10^4 次请求本地命中 ≥90%；命中率分节点分列。
//!   J1-b 远端命中逐笔留账：本地池空的远端回落交付，每笔记
//!   （请求核、本地节点、实际节点、事件号），逐笔可溯。
//!   J1-c 拓扑冻结：核→节点映射表常量（n0: 核 0~3，n1: 核 4~7），
//!   路由器只读该表，不猜。
//!
//! **段位复核记录**：0x7050~0x705F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x7050~0x705F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x7050;
/// 段顶。
pub const SEG_TOP: u16 = 0x705F;

/// 节点数。
pub const NODES: usize = 2;
/// 核数。
pub const CORES: usize = 8;
/// 本地命中率下限（‱）。
pub const LOCAL_HIT_FLOOR_BP: u64 = 9000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NumaError {
    /// 0x7051 核号越界。
    BadCore,
    /// 0x7052 本地命中率低于下限。
    HitRateLow,
    /// 0x7053 远端交付未留账。
    RemoteUnlogged,
    /// 0x7054 节点映射不一致。
    TopologyDrift,
}

impl NumaError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            NumaError::BadCore => format!(
                "[0x{:04X}] 核号越界：合法 0~7。改法：入参先做域校验。",
                self.code()
            ),
            NumaError::HitRateLow => format!(
                "[0x{:04X}] 本地命中率低于 {}‱。\
                 改法：加大本地池配比或修亲和路由。",
                LOCAL_HIT_FLOOR_BP, self.code()
            ),
            NumaError::RemoteUnlogged => format!(
                "[0x{:04X}] 远端交付未留账。\
                 改法：远端回落路径必须先写远端账再交付。",
                self.code()
            ),
            NumaError::TopologyDrift => format!(
                "[0x{:04X}] 核→节点映射不一致：路由与拓扑表脱钩。\
                 改法：路由只读 TOPO 常量表。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、拓扑与路由
// ===========================================================================

/// 核→节点映射表（J1-c 冻结）。
pub const TOPO: [usize; CORES] = [0, 0, 0, 0, 1, 1, 1, 1];

/// 一笔远端账。
#[derive(Clone, Copy, Debug)]
pub struct RemoteRecord {
    /// 请求核。
    pub core: usize,
    /// 本地节点。
    pub local_node: usize,
    /// 实际节点。
    pub actual_node: usize,
    /// 事件号。
    pub seq: u64,
}

/// NUMA 路由分配器。
pub struct NumaRouter {
    /// 各节点池余量。
    pub node_pools: [u64; NODES],
    /// 本地命中数（分节点）。
    pub local_hits: [u64; NODES],
    /// 总请求数（分节点）。
    pub requests: [u64; NODES],
    /// 远端账。
    pub remote_ledger: Vec<RemoteRecord>,
    seq: u64,
}

impl NumaRouter {
    /// 构造：两节点池配比。
    pub fn new(pool0: u64, pool1: u64) -> Self {
        NumaRouter {
            node_pools: [pool0, pool1],
            local_hits: [0, 0],
            requests: [0, 0],
            remote_ledger: Vec::new(),
            seq: 0,
        }
    }

    /// 请求一页（核亲和路由）：本地节点池优先，空则远端回落留账。
    pub fn alloc(&mut self, core: usize) -> result::Result<(usize, bool), NumaError> {
        if core >= CORES {
            return Err(NumaError::BadCore);
        }
        self.seq += 1;
        let local = TOPO[core];
        self.requests[local] += 1;
        if self.node_pools[local] > 0 {
            self.node_pools[local] -= 1;
            self.local_hits[local] += 1;
            return Ok((local, true));
        }
        // 远端回落。
        let remote = 1 - local;
        if self.node_pools[remote] > 0 {
            self.node_pools[remote] -= 1;
            self.remote_ledger.push(RemoteRecord {
                core,
                local_node: local,
                actual_node: remote,
                seq: self.seq,
            });
            return Ok((remote, false));
        }
        Err(NumaError::HitRateLow)
    }

    /// 命中率（全节点合计，‱）。
    pub fn hit_rate_bp(&self) -> u64 {
        let mut hits = 0u64;
        let mut reqs = 0u64;
        let mut n = 0;
        while n < NODES {
            hits += self.local_hits[n];
            reqs += self.requests[n];
            n += 1;
        }
        if reqs == 0 {
            return 0;
        }
        hits * 10_000 / reqs
    }

    /// J1-b 远端账核：每笔 actual_node ≠ local_node 且节点映射合法。
    pub fn audit_remote(&self) -> result::Result<usize, NumaError> {
        let mut i = 0;
        while i < self.remote_ledger.len() {
            let r = self.remote_ledger[i];
            if r.core >= CORES || TOPO[r.core] != r.local_node || r.actual_node == r.local_node {
                return Err(NumaError::TopologyDrift);
            }
            i += 1;
        }
        Ok(self.remote_ledger.len())
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2447_7050_9E37_79B9)
    }

    /// 下一个。
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    /// [0, bound)。
    pub fn next_below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next_u64() % bound
    }
}

/// 请求演练数。
pub const REQUESTS: u64 = 10_000;

/// J1 证据生成器：10^4 次核轮转请求，本地命中 ≥9000‱ + 远端账核。
pub fn numa_audit() -> result::Result<(u64, usize), NumaError> {
    // 配比：每节点池 4800（请求约各 5000，留 4% 空间触发远端回落）。
    let mut rt = NumaRouter::new(4700, 4700);
    let mut rng = Lcg::new(0x2447_0001);
    let mut n = 0u64;
    while n < REQUESTS {
        let core = rng.next_below(CORES as u64) as usize;
        rt.alloc(core)?;
        n += 1;
    }
    let rate = rt.hit_rate_bp();
    if rate < LOCAL_HIT_FLOOR_BP {
        return Err(NumaError::HitRateLow);
    }
    let remote_checked = rt.audit_remote()?;
    Ok((rate, remote_checked))
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 本地命中率（‱）。
    pub hit_rate: u64,
    /// 远端账核过笔数。
    pub remote_checks: usize,
    /// 拓扑表冻结生效。
    pub topo_ok: bool,
    /// 越界核被拒。
    pub bad_core_rejected: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.hit_rate >= LOCAL_HIT_FLOOR_BP && self.remote_checks > 0 && self.topo_ok && self.bad_core_rejected
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, NumaError> {
    let (rate, remote) = numa_audit()?;
    // 拓扑冻结核：TOPO 与节点半分语义一致。
    let mut topo_ok = true;
    let mut c = 0;
    while c < CORES {
        let expect = if c < 4 { 0 } else { 1 };
        if TOPO[c] != expect {
            topo_ok = false;
        }
        c += 1;
    }
    let mut rt = NumaRouter::new(1, 1);
    let bad = rt.alloc(99).is_err();
    Ok(SelfAudit {
        hit_rate: rate,
        remote_checks: remote,
        topo_ok,
        bad_core_rejected: bad,
    })
}
