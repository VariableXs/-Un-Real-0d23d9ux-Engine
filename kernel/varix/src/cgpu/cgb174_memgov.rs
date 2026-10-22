//! CGPU-F0174 · 帧图内存治理（判据段 0x0480）
//! 任务图自身的内存治理：节点池/边池/排序缓冲的帧间复用（arena 复用
//! A 域纪律）、图构建峰值控制（万节点图的构建内存上限）、内存水位联动
//! （预算红池时限制图规模——建议降级节点数）。判据：复用命中、峰值
//! 控制、水位联动、压测归零、判据。
//!
//! 诊断码段 0x0480（本模块独占：0x0480~0x048F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0480;

/// 万节点上限（判据「峰值控制」硬界）：单帧图节点数上限。
pub const MAX_NODES: usize = 10_000;

/// 单节点平均内存账（字节）：节点槽 24 + 平均出边 4×8 + 排序缓冲 8。
pub const NODE_MEM_BYTES: usize = 24 + 4 * 8 + 8;

/// 图内存水位等级（预算红池联动）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemLevel {
    /// 绿池：全规模图。
    Green,
    /// 黄池：图规模限 75%。
    Yellow,
    /// 红池：图规模限 50%，建议降级节点数。
    Red,
}

impl MemLevel {
    /// 规模上限比例（千分比）。
    pub fn cap_permille(self) -> u32 {
        match self {
            MemLevel::Green => 1000,
            MemLevel::Yellow => 750,
            MemLevel::Red => 500,
        }
    }

    /// 从水位号解析（0/1/2）。
    pub fn from_rank(r: u8) -> Result<Self, u16> {
        match r {
            0 => Ok(MemLevel::Green),
            1 => Ok(MemLevel::Yellow),
            2 => Ok(MemLevel::Red),
            _ => Err(SEG_BASE),
        }
    }
}

/// 图构建内存账。
#[derive(Clone, Copy, Debug)]
pub struct BuildMem {
    /// 节点数。
    pub nodes: usize,
    /// 边数。
    pub edges: usize,
}

impl BuildMem {
    /// 账面校验：节点数非零且不超硬界、边数不超节点数平方量级。
    pub fn validate(&self) -> Result<(), u16> {
        if self.nodes == 0 || self.nodes > MAX_NODES {
            return Err(SEG_BASE + 1);
        }
        if self.edges > self.nodes.saturating_mul(self.nodes) {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }

    /// 构建内存估算（字节）：节点池 + 边池 + 排序缓冲。
    pub fn bytes(&self) -> usize {
        self.nodes.saturating_mul(NODE_MEM_BYTES)
            + self.edges.saturating_mul(16)
            + self.nodes.saturating_mul(8)
    }
}

/// 帧间复用池（判据「复用命中」核心面）：节点池/边池/排序缓冲
/// 三池的容量按峰值保留、跨帧复用——复用帧不再分配。
pub struct FrameArena {
    /// 三池保留容量（节点池, 边池, 排序缓冲槽）。
    pools: (usize, usize, usize),
    /// 复用命中次数。
    hits: u64,
    /// 新分配次数（池容量不足时的扩容）。
    grows: u64,
    /// 峰值账（历史最大 BuildMem.bytes）。
    peak_bytes: usize,
    /// 已封池（封池后只读）。
    sealed: bool,
}

impl FrameArena {
    /// 空池。
    pub fn new() -> Self {
        FrameArena {
            pools: (0, 0, 0),
            hits: 0,
            grows: 0,
            peak_bytes: 0,
            sealed: false,
        }
    }

    /// 帧构建请求（判据「复用命中」核心）：需求 ≤ 池容量即命中；
    /// 超出则扩容记账。封池后拒绝。
    pub fn frame_build(&mut self, need: BuildMem) -> Result<bool, u16> {
        if self.sealed {
            return Err(SEG_BASE + 3);
        }
        need.validate()?;
        let (nn, ne, nb) = self.pools;
        let need_buf = need.nodes;
        let hit = nn >= need.nodes && ne >= need.edges && nb >= need_buf;
        if hit {
            self.hits += 1;
        } else {
            self.grows += 1;
            self.pools = (
                nn.max(need.nodes),
                ne.max(need.edges),
                nb.max(need_buf),
            );
        }
        let b = need.bytes();
        if b > self.peak_bytes {
            self.peak_bytes = b;
        }
        Ok(hit)
    }

    /// 复用命中率（千分比，判据「复用命中」量化面）。
    pub fn hit_permille(&self) -> Result<u32, u16> {
        let total = self.hits + self.grows;
        if total == 0 {
            return Err(SEG_BASE + 4);
        }
        Ok(((self.hits * 1000) / total) as u32)
    }

    /// 峰值账（字节）。
    pub fn peak(&self) -> usize {
        self.peak_bytes
    }

    /// 封池（帧收尾）：封后只读，压测归零用。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(SEG_BASE + 5);
        }
        self.sealed = true;
        Ok(())
    }

    /// 压测归零（判据「压测归零」面）：池归还 + 计数清零 + 未封池
    /// 校验——归零必须发生在封池前（封池是终态）。
    pub fn reset_for_stress(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(SEG_BASE + 6);
        }
        self.pools = (0, 0, 0);
        self.hits = 0;
        self.grows = 0;
        self.peak_bytes = 0;
        Ok(())
    }

    /// 池快照。
    pub fn pools(&self) -> (usize, usize, usize) {
        self.pools
    }
}

/// 水位联动裁定（判据「水位联动」核心面）：给定当前水位与请求
/// 规模，裁定放行 / 建议降级 / 拒绝。降级建议 = 请求节点数 × 水位
/// 上限比例（向下取整）。
pub fn watermark_advice(level: MemLevel, request: BuildMem) -> Result<Result<usize, u16>, u16> {
    request.validate()?;
    let cap = (MAX_NODES as u64 * level.cap_permille() as u64 / 1000) as usize;
    if request.nodes <= cap {
        Ok(Ok(request.nodes))
    } else {
        Ok(Err(SEG_BASE + 7))
    }
}

/// 降级建议（判据「建议降级节点数」面）：红池下的推荐节点数。
pub fn downgrade_suggestion(request: BuildMem) -> Result<usize, u16> {
    request.validate()?;
    let capped = (request.nodes as u64 * MemLevel::Red.cap_permille() as u64 / 1000) as usize;
    if capped == 0 {
        return Err(SEG_BASE + 8);
    }
    Ok(capped)
}

/// 峰值控制审计（判据「峰值控制」面）：万节点图构建内存上限
/// = MAX_NODES × NODE_MEM_BYTES，超限即红。
pub fn peak_control(peak_bytes: usize) -> Result<(), u16> {
    let limit = MAX_NODES.saturating_mul(NODE_MEM_BYTES);
    if peak_bytes == 0 {
        return Err(SEG_BASE + 9);
    }
    if peak_bytes > limit {
        return Err(SEG_BASE + 10);
    }
    Ok(())
}

/// 压测台账（判据「压测归零」批量面）：多帧压测序列跑一遍 arena，
/// 校验命中率随帧数收敛（复用生效后命中率应 ≥ 800‰）。
pub struct StressLedger {
    /// 每帧命中记录。
    pub hits: Vec<bool>,
    /// 归零次数。
    pub resets: u32,
}

impl StressLedger {
    /// 空台账。
    pub fn new() -> Self {
        StressLedger {
            hits: Vec::new(),
            resets: 0,
        }
    }

    /// 压测判（判据「压测归零」）：至少 4 帧 + 命中率达标 + 归零
    /// 至少一次（归零是压测的收尾动作）。
    pub fn stress_green(&self) -> Result<u32, u16> {
        if self.hits.len() < 4 {
            return Err(SEG_BASE + 11);
        }
        if self.resets == 0 {
            return Err(SEG_BASE + 12);
        }
        let h = self.hits.iter().filter(|x| **x).count();
        let pm = ((h * 1000) / self.hits.len()) as u32;
        if pm < 800 {
            return Err(SEG_BASE + 13);
        }
        Ok(pm)
    }
}

/// 内存治理总闸（一站式收口）：峰值控制 + 水位裁定 + 复用命中。
pub fn mem_gov_audit(
    arena: &mut FrameArena,
    frames: &[BuildMem],
    level: MemLevel,
) -> Result<u32, u16> {
    if frames.is_empty() {
        return Err(SEG_BASE + 14);
    }
    for f in frames {
        f.validate()?;
        // 水位联动：红/黄池下超规模请求直接拒（记账为不命中）。
        match watermark_advice(level, *f)? {
            Ok(_) => {
                arena.frame_build(*f)?;
            }
            Err(_) => {
                return Err(SEG_BASE + 15);
            }
        }
    }
    peak_control(arena.peak())?;
    arena.hit_permille()
}

/// 帧间复用池深度扩展面。
impl FrameArena {
    /// 池容量审计（判据「复用命中」卫生面）：三池容量必须彼此
    /// 成比例（节点:边 约 1:4 万节点图经验值）——比例失衡说明
    /// 某池被异常峰值污染。
    pub fn pool_proportion(&self) -> Result<(), u16> {
        let (nn, ne, _) = self.pools;
        if nn == 0 {
            return Ok(());
        }
        let expect = nn.saturating_mul(4);
        if ne < expect / 2 || ne > expect * 3 / 2 {
            return Err(SEG_BASE + 16);
        }
        Ok(())
    }

    /// 峰值历史投影（判据「峰值控制」趋势面）：峰值与当前池容量
    /// 的水位比（千分比）。
    pub fn peak_watermark_permille(&self) -> Result<u32, u16> {
        if self.peak_bytes == 0 {
            return Err(SEG_BASE + 17);
        }
        let cur = self
            .pools
            .0
            .saturating_mul(NODE_MEM_BYTES)
            .saturating_add(self.pools.1.saturating_mul(16));
        if cur == 0 {
            return Err(SEG_BASE + 18);
        }
        Ok(((self.peak_bytes.min(cur) * 1000) / cur.max(1)) as u32)
    }
}

/// 多水位序列演练（判据「水位联动」序列面）：绿黄红三级水位下的
/// 允许规模单调不增——水位升高规模反而变大 = 联动断裂。
pub fn watermark_monotonic() -> Result<(), u16> {
    let mut prev = u32::MAX;
    for r in 0..=2u8 {
        let level = MemLevel::from_rank(r)?;
        let cap = level.cap_permille();
        if cap > prev {
            return Err(SEG_BASE + 19);
        }
        prev = cap;
    }
    Ok(())
}

/// 万节点压测账（判据「峰值控制」批量面）：构造满界图内存账并
/// 校验 bytes 不超上限——压测归零的静态对偶。
pub fn max_scale_bytes() -> Result<usize, u16> {
    let m = BuildMem {
        nodes: MAX_NODES,
        edges: MAX_NODES.saturating_mul(4),
    };
    m.validate()?;
    let b = m.bytes();
    peak_control(b)?;
    Ok(b)
}

/// 复用收敛演练（独立函数面）：固定需求连跑 N 帧，第 2 帧起必须
/// 全命中（首帧扩容、后续复用是 arena 的基本契约）。
pub fn reuse_contract(arena: &mut FrameArena, need: BuildMem, frames: usize) -> Result<(), u16> {
    if frames < 2 {
        return Err(SEG_BASE + 20);
    }
    need.validate()?;
    for i in 0..frames {
        let hit = arena.frame_build(need)?;
        if i >= 1 && !hit {
            return Err(SEG_BASE + 21);
        }
    }
    Ok(())
}
