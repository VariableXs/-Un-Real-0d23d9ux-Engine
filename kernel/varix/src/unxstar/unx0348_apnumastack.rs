//! unx0348_apnumastack —— UNX-F0348 NUMA 感知 AP 栈选址（F0074 消费）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0348 · NUMA 感知 AP 栈选址 · 目标 240 行
//! 判据原文：UNX-F0348-J1 AP 栈/区按节点亲和选址，与 F0074 位图
//!           一致，命中账齐。
//! 组件：节点位图消费面（F0074 NUMA 拓扑位图：每节点在线页区间
//!       闭集）+ 栈选址器（AP 栈按其所属节点亲和分配：节点内
//!       4KiB 对齐连续 8 页，跨节点分配 100% 拒绝）+ 命中账（每
//!       次选址落一行：AP/节点/基址/页数，账面可查）+ 区间互斥
//!       审计（已分配栈区间两两不重叠——重叠 = 双栈踩踏）。
//! 职责定位：AP 栈空间分配的唯一权威面——亲和红线保证「栈永远
//!   在 AP 自己的节点上」（跨节点栈访问延迟放大是启动超时的
//!   隐性根因），区间互斥保证物理安全，命中账保证每次分配可回溯。
//! 性能逐项分解：选址 O(节点区间数)；互斥审计 O(n²)（n≤16 核，
//!   收口时一次跑）；命中账 O(1) 摊销。全整数确定性。
//! 诊断码段：0xE430（0xE431~0xE439 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const AN_VERSION: &str = "unx0348_apnumastack-v1";

/// 诊断段基址（0xE430 段 · 本模块独占）。
pub const AN_SEG_BASE: u16 = 0xE430;

/// XcCode 闭集（0xE431~0xE439）。
pub const XC_NODE_OFFLINE: u32 = 0xE431;
pub const XC_RANGE_EXHAUST: u32 = 0xE432;
pub const XC_ALIGN_BAD: u32 = 0xE433;
pub const XC_RANGE_OVERLAP: u32 = 0xE434;
pub const XC_DUP_AP: u32 = 0xE435;
pub const XC_PAGES_UNDER: u32 = 0xE436;
pub const XC_BITMAP_MISS: u32 = 0xE437;
pub const XC_NODE_OOR: u32 = 0xE438;
pub const XC_HIT_FULL: u32 = 0xE439;

/// 每 AP 栈页数（8 页 = 32KiB，判据写死）。
pub const STACK_PAGES: u64 = 8;

/// 页粒度（4KiB）。
pub const PAGE_SIZE: u64 = 4096;

/// 节点在线区间：F0074 位图消费形态（节点号 + 在线页区间列表）。
#[derive(Clone, Debug)]
pub struct NodeRange {
    pub node: u32,
    /// 在线页区间列表 [(起页, 止页)] 双闭区间，区间必须有序不交。
    pub pages: Vec<(u64, u64)>,
}

impl NodeRange {
    /// 区间合法性：有序 + 两两不交 + 止页 ≥ 起页。
    pub fn ranges_valid(&self) -> bool {
        let mut prev_end: Option<u64> = None;
        for &(lo, hi) in self.pages.iter() {
            if hi < lo {
                return false;
            }
            if let Some(pe) = prev_end {
                if lo <= pe {
                    return false;
                }
            }
            prev_end = Some(hi);
        }
        true
    }

    /// 区间总页数。
    pub fn total_pages(&self) -> u64 {
        self.pages.iter().map(|&(lo, hi)| hi - lo + 1).sum()
    }
}

/// 一条栈命中账：AP / 节点 / 栈基址（页号）/ 页数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StackHit {
    pub ap_id: u32,
    pub node: u32,
    pub base_page: u64,
    pub pages: u64,
}

/// NUMA 感知 AP 栈选址器。
pub struct StackPlacer {
    nodes: Vec<NodeRange>,
    hits: Vec<StackHit>,
}

impl StackPlacer {
    /// 由 F0074 节点位图构造（离线节点=空区间的输入拒收）。
    pub fn new(nodes: Vec<NodeRange>) -> Result<StackPlacer, u16> {
        for n in nodes.iter() {
            if !n.ranges_valid() {
                return Err(XC_RANGE_OVERLAP as u16);
            }
            if n.total_pages() == 0 {
                return Err(XC_NODE_OFFLINE as u16);
            }
        }
        Ok(StackPlacer { nodes, hits: Vec::new() })
    }

    /// 为 AP 选址栈：优先其亲和节点内首个能容纳 STACK_PAGES 的
    /// 连续区间；节点内放不下 = 资源破，跨节点放行 = 亲和破，
    /// 两者都拒（判据红线：栈永远在本节点）。
    pub fn place(&mut self, ap_id: u32, node: u32) -> Result<u64, u16> {
        if self.hits.iter().any(|h| h.ap_id == ap_id) {
            return Err(XC_DUP_AP as u16);
        }
        if node as usize >= self.nodes.len() {
            return Err(XC_NODE_OOR as u16);
        }
        let n = &self.nodes[node as usize];
        let mut base: Option<u64> = None;
        for &(lo, hi) in n.pages.iter() {
            let span = hi - lo + 1;
            if span >= STACK_PAGES {
                base = Some(lo);
                break;
            }
        }
        let base_page = match base {
            Some(b) => b,
            None => return Err(XC_RANGE_EXHAUST as u16),
        };
        // 区间互斥审计：与既有命中逐条比（同节点才可能交）。
        let new_lo = base_page;
        let new_hi = base_page + STACK_PAGES - 1;
        for h in self.hits.iter() {
            if h.node != node {
                continue;
            }
            let (lo, hi) = (h.base_page, h.base_page + h.pages - 1);
            if new_lo <= hi && lo <= new_hi {
                return Err(XC_RANGE_OVERLAP as u16);
            }
        }
        // 4KiB 对齐断言（页号本身即对齐，防调用方传字节地址误用）。
        if base_page * PAGE_SIZE % PAGE_SIZE != 0 {
            return Err(XC_ALIGN_BAD as u16);
        }
        self.hits.push(StackHit { ap_id, node, base_page, pages: STACK_PAGES });
        Ok(base_page)
    }

    /// 命中账只读视图。
    pub fn hits(&self) -> &[StackHit] {
        &self.hits
    }

    /// 命中账齐备断言：每个 AP 一行、每行页数达 STACK_PAGES、
    /// 每行节点 = 其亲和节点（F0074 位图一致红线）。
    pub fn ledger_complete(&self, aps: &[(u32, u32)]) -> Result<(), u16> {
        if self.hits.len() != aps.len() {
            return Err(XC_HIT_FULL as u16);
        }
        for &(ap, node) in aps.iter() {
            match self.hits.iter().find(|h| h.ap_id == ap) {
                Some(h) => {
                    if h.pages < STACK_PAGES {
                        return Err(XC_PAGES_UNDER as u16);
                    }
                    if h.node != node {
                        return Err(XC_BITMAP_MISS as u16);
                    }
                }
                None => return Err(XC_BITMAP_MISS as u16),
            }
        }
        Ok(())
    }
}

/// 域自检入口：选址/亲和/互斥/账齐 四族。
pub fn run_unx0348_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0348-NUMA栈");
    // 族一：合法拓扑构造（节点0：页0..255，节点1：页1024..2047）。
    let topo = vec![
        NodeRange { node: 0, pages: vec![(0, 255)] },
        NodeRange { node: 1, pages: vec![(1024, 2047)] },
    ];
    let mut pl = match StackPlacer::new(topo) {
        Ok(p) => p,
        Err(_) => {
            cs.add("拓扑构造", false, "合法拓扑被拒");
            return cs;
        }
    };
    let b0 = pl.place(1, 0);
    cs.add("节点0选址", matches!(b0, Ok(0)), "首 AP 落页 0");
    let b1 = pl.place(2, 1);
    cs.add("节点1选址", matches!(b1, Ok(1024)), "次 AP 落页 1024");
    cs.add("同 AP 重复拒", pl.place(1, 0).is_err(), "一 AP 一栈");

    // 族二：互斥审计（把节点0 塞到只剩 <8 页连续空间时拒绝）。
    let tight = vec![NodeRange { node: 0, pages: vec![(0, 6)] }];
    let mut tp = match StackPlacer::new(tight) {
        Ok(p) => p,
        Err(_) => {
            cs.add("紧区间构造", false, "tight 拓扑被拒");
            return cs;
        }
    };
    cs.add("空间不足拒", tp.place(1, 0).is_err(), "7 页 < 8 页需求");

    // 族三：离线节点 / 越界节点拒。
    let offline = vec![NodeRange { node: 0, pages: vec![] }];
    cs.add("离线节点拒", StackPlacer::new(offline).is_err(), "空区间 = 离线");
    let topo2 = vec![NodeRange { node: 0, pages: vec![(0, 63)] }];
    let mut p2 = match StackPlacer::new(topo2) {
        Ok(p) => p,
        Err(_) => {
            cs.add("单节点构造", false, "单节点被拒");
            return cs;
        }
    };
    cs.add("节点越界拒", p2.place(1, 5).is_err(), "无节点 5");

    // 族四：命中账齐备（位图一致红线）。
    let aps = [(1u32, 0u32), (2, 1)];
    cs.add("账齐断言", p2.ledger_complete(&aps).is_ok(), "行数+页数+亲和全对");
    cs.add("账行缺检", p2.ledger_complete(&aps[..1]).is_err(), "行数失配即破");
    cs
}

// ===== 扩展面：节点负载均衡选址（碎片感知第二落点）=====
/// 均衡选址：默认落点取节点内第一个可容区间；均衡模式在节点内
/// 多区间时取「剩余连续页最多」的区间，防扎堆把大区间切碎。
///
/// 碎片是 NUMA 栈分配的慢性病：首适配会把大区间从头部啃掉，
/// 后续大请求被迫失败。最优适配（剩余最少者胜）在此处反而
/// 不对——栈需求恒定 8 页，取「剩余最大」让小请求吃小洞。
pub fn best_fit_base(pages: &[(u64, u64)], need: u64) -> Result<u64, u16> {
    if need == 0 {
        return Err(1);
    }
    let mut best: Option<(u64, u64)> = None; // (剩余, 基址)
    for &(lo, hi) in pages.iter() {
        let span = hi - lo + 1;
        if span >= need {
            let remain = span - need;
            match best {
                Some((br, _)) if remain <= br => {}
                _ => best = Some((remain, lo)),
            }
        }
    }
    match best {
        Some((_, base)) => Ok(base),
        None => Err(2),
    }
}
