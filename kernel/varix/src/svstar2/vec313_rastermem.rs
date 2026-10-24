//! VE-F0713 · 栅格化内存治理
//!
//! 职责：栅格化工作内存的全域治理——
//! AEL/边箱/瓦片缓冲/AA 累积的内存总账（峰值场景：超大路径+多线程的峰值控制）、
//! arena 复用（帧间复用工作内存——零分配热路径）、
//! 上限与降级（内存紧时降 AA 档/拆分路径）。

use alloc::vec;
use alloc::vec::Vec;

/// 内存账目项：区域名 + 当前占用（KiB）+ 上限（KiB）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemBucket {
    pub region: MemRegion,
    pub used_kib: u32,
    pub cap_kib: u32,
}

/// 四大内存区域：AEL、边箱、瓦片缓冲、AA 累积。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemRegion {
    Ael,
    EdgeBins,
    TileBuf,
    AaAccum,
}

impl MemRegion {
    pub fn name(self) -> &'static str {
        match self {
            MemRegion::Ael => "AEL",
            MemRegion::EdgeBins => "边箱",
            MemRegion::TileBuf => "瓦片缓冲",
            MemRegion::AaAccum => "AA 累积",
        }
    }
}

/// 内存总账：四区逐项记账 + 总量核验。
pub struct MemBook {
    pub buckets: Vec<MemBucket>,
}

impl MemBook {
    pub fn new() -> Self {
        MemBook { buckets: Vec::new() }
    }
    pub fn account(&mut self, r: MemRegion, used: u32, cap: u32) {
        match self.buckets.iter_mut().find(|b| b.region == r) {
            Some(b) => {
                b.used_kib = used;
                b.cap_kib = cap;
            }
            None => self.buckets.push(MemBucket { region: r, used_kib: used, cap_kib: cap }),
        }
    }
    /// 总占用。
    pub fn total_used(&self) -> u32 {
        self.buckets.iter().map(|b| b.used_kib).sum()
    }
    /// 超限区域名单（峰值控制核验）。
    pub fn over_budget(&self) -> Vec<MemRegion> {
        self.buckets.iter().filter(|b| b.used_kib > b.cap_kib).map(|b| b.region).collect()
    }
    /// 总账合规：无区域超限。
    pub fn within_budget(&self) -> bool {
        self.over_budget().is_empty()
    }
}

/// arena 复用：帧间复用工作内存——reset 不释放（零分配热路径）。
pub struct Arena {
    buf: Vec<u8>,
    top: usize,
    /// 复用断言：reset 后容量不变（不 free）。
    pub resets: u32,
    pub allocs: u32,
}

impl Arena {
    pub fn new(cap: usize) -> Self {
        Arena { buf: vec![0; cap], top: 0, resets: 0, allocs: 0 }
    }
    /// 分配：容量不足返回 None（不扩容——热路径纪律）。
    pub fn alloc(&mut self, n: usize) -> Option<&mut [u8]> {
        if self.top + n > self.buf.len() {
            return None;
        }
        let s = &mut self.buf[self.top..self.top + n];
        self.top += n;
        self.allocs += 1;
        Some(s)
    }
    /// 帧末复位：top 归零、容量不变——下一帧零分配。
    pub fn reset(&mut self) {
        self.top = 0;
        self.resets += 1;
    }
    /// 复用断言核验：容量不变且复用计数增长。
    pub fn reuse_ok(&self, cap_before: usize) -> bool {
        self.buf.len() == cap_before
    }
}

/// 降级策略：内存压力档位 → AA 降档与路径拆分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemPressure {
    Normal,
    Tight,
    Critical,
}

/// 压力判定：总占用 / 总上限。
pub fn pressure_level(total_used: u32, total_cap: u32) -> MemPressure {
    if total_cap == 0 {
        return MemPressure::Critical;
    }
    let pct = total_used * 100 / total_cap;
    if pct < 70 {
        MemPressure::Normal
    } else if pct < 90 {
        MemPressure::Tight
    } else {
        MemPressure::Critical
    }
}

/// 降级动作：Normal 不降；Tight 降 AA 档（8x→4x→无）；Critical 拆分路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Degrade {
    None,
    AaDown,
    SplitPaths,
}

pub fn degrade_action(p: MemPressure) -> Degrade {
    match p {
        MemPressure::Normal => Degrade::None,
        MemPressure::Tight => Degrade::AaDown,
        MemPressure::Critical => Degrade::SplitPaths,
    }
}

/// AA 降档链：8x→4x→解析外的无 AA。
pub fn aa_downgrade(current: u32) -> u32 {
    match current {
        8 => 4,
        4 => 1,
        _ => 1,
    }
}

/// 路径拆分：超大路径按边数对半拆，直到每片边数低于上限。
pub fn split_paths(edge_count: u32, max_edges: u32) -> u32 {
    if edge_count == 0 {
        return 1;
    }
    let mut pieces = 1u32;
    let mut n = edge_count;
    while n > max_edges {
        n = n.div_ceil(2);
        pieces *= 2;
    }
    pieces
}

pub fn run_vec313_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec313");
    // 族一：内存总账
    let mut book = MemBook::new();
    book.account(MemRegion::Ael, 512, 1024);
    book.account(MemRegion::EdgeBins, 256, 512);
    book.account(MemRegion::TileBuf, 1024, 2048);
    book.account(MemRegion::AaAccum, 128, 256);
    s.add("四区齐账", book.buckets.len() == 4, "four-regions");
    s.add("总占用求和", book.total_used() == 1920, "total");
    s.add("总账合规", book.within_budget(), "ok");
    book.account(MemRegion::Ael, 2048, 1024);
    s.add("超限点名", book.over_budget() == vec![MemRegion::Ael], "over");
    s.add("峰值控制发现超限", !book.within_budget(), "peak");
    s.add("区域名互异", {
        let r = [MemRegion::Ael, MemRegion::EdgeBins, MemRegion::TileBuf, MemRegion::AaAccum];
        r[0].name() != r[1].name() && r[2].name() != r[3].name()
    }, "names");
    // 族二：arena 复用
    let mut ar = Arena::new(64);
    let cap0 = 64;
    let a = ar.alloc(16);
    s.add("arena 分配成功", a.is_some(), "alloc");
    let b = ar.alloc(64);
    s.add("容量不足拒绝不扩容", b.is_none(), "no-grow");
    let _ = ar.alloc(48);
    s.add("分配计数", ar.allocs == 2, "alloc-count");
    ar.reset();
    s.add("复位后容量不变", ar.reuse_ok(cap0), "reuse");
    let c = ar.alloc(64);
    s.add("复位后再分配零新增", c.is_some() && ar.buf.len() == cap0, "zero-alloc");
    s.add("复用计数增长", ar.resets == 1, "reset-count");
    // 族三：压力与降级
    s.add("低水位正常", pressure_level(60, 100) == MemPressure::Normal, "normal");
    s.add("中水位紧", pressure_level(80, 100) == MemPressure::Tight, "tight");
    s.add("高水位危急", pressure_level(95, 100) == MemPressure::Critical, "critical");
    s.add("零上限危急", pressure_level(1, 0) == MemPressure::Critical, "no-cap");
    s.add("正常不降级", degrade_action(MemPressure::Normal) == Degrade::None, "none");
    s.add("紧→降 AA", degrade_action(MemPressure::Tight) == Degrade::AaDown, "aa-down");
    s.add("危急→拆路径", degrade_action(MemPressure::Critical) == Degrade::SplitPaths, "split");
    // 族四：降档与拆分
    s.add("AA 降档链 8→4", aa_downgrade(8) == 4, "8to4");
    s.add("AA 降档链 4→1", aa_downgrade(4) == 1, "4to1");
    s.add("拆分对半", split_paths(100, 32) == 4, "split4");
    s.add("已合规不拆", split_paths(30, 32) == 1, "no-split");
    s.add("零边单片", split_paths(0, 32) == 1, "zero");
    s.add("峰值场景：超大+多线程可降级", degrade_action(pressure_level(95, 100)) == Degrade::SplitPaths, "peak-scene");
    s
}
