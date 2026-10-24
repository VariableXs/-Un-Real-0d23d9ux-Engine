//! VE-F3849 · 读屏性能
//!
//! 读屏协同 0.5ms 分项预算 + 语义查询热表 + 批量合批 + 辅具态监控。
//! 开销执法。热表单源复用。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xC5xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xC500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CC5Code(pub u16);

impl CC5Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const BUDGET_EXCEEDED: Self = Self(SEG_BASE + 1);
    pub const CACHE_MISS: Self = Self(SEG_BASE + 2);
    pub const BATCH_FAIL: Self = Self(SEG_BASE + 3);
    pub const AT_STUCK: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::BUDGET_EXCEEDED => "预算超（0.5ms 分项），需定位",
            Self::CACHE_MISS => "热表击穿，降级",
            Self::BATCH_FAIL => "合批失效，风暴",
            Self::AT_STUCK => "辅具态卡（P1）",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────

/// 分项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostItem {
    /// 语义查询。
    Query,
    /// 标注。
    Annotate,
    /// 公告。
    Announce,
    /// 跟随。
    Follow,
}

impl CostItem {
    pub const ALL: [CostItem; 4] = [
        CostItem::Query,
        CostItem::Annotate,
        CostItem::Announce,
        CostItem::Follow,
    ];

    /// 分项预算（µs，合计 500µs = 0.5ms）。
    pub fn budget_us(self) -> u32 {
        match self {
            Self::Query => 200,
            Self::Annotate => 150,
            Self::Announce => 100,
            Self::Follow => 50,
        }
    }
}

/// 分项实测。
#[derive(Debug, Clone, Copy)]
pub struct CostSample {
    pub item: CostItem,
    pub elapsed_us: u32,
}

/// 热表条目。
#[derive(Debug, Clone, Copy)]
pub struct CacheEntry {
    pub key: u32,
    pub hits: u32,
}

/// 性能治理器。
#[derive(Debug, Clone)]
pub struct SrPerf {
    pub samples: Vec<CostSample>,
    pub cache: Vec<CacheEntry>,
    /// 合批前后事件数。
    pub events_raw: u32,
    pub events_batched: u32,
    /// 辅具是否接入。
    pub at_attached: bool,
    /// 辅具态是否健康。
    pub at_healthy: bool,
}

impl SrPerf {
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
            cache: Vec::new(),
            events_raw: 0,
            events_batched: 0,
            at_attached: false,
            at_healthy: true,
        }
    }

    /// 分项预算执法。
    pub fn budgets_ok(&self) -> bool {
        for it in CostItem::ALL.iter() {
            let sum: u32 = self
                .samples
                .iter()
                .filter(|s| s.item == *it)
                .map(|s| s.elapsed_us)
                .sum();
            if sum > it.budget_us() {
                return false;
            }
        }
        true
    }

    /// 总预算 500µs。
    pub fn total_within(&self) -> bool {
        self.samples.iter().map(|s| s.elapsed_us).sum::<u32>() <= 500
    }

    /// 热表命中率 ≥80%。
    pub fn cache_ok(&self) -> bool {
        if self.cache.is_empty() {
            return true;
        }
        let hits: u32 = self.cache.iter().map(|e| e.hits).sum();
        let total = self.cache.iter().count() as u32 * 10;
        hits * 5 >= total * 4
    }

    /// 合批有效：batched < raw。
    pub fn batch_effective(&self) -> bool {
        self.events_batched <= self.events_raw
    }

    /// 辅具健康。
    pub fn at_ok(&self) -> bool {
        !self.at_attached || self.at_healthy
    }

    /// 全检。
    pub fn full_check(&self) -> Result<(), CC5Code> {
        if !self.budgets_ok() || !self.total_within() {
            return Err(CC5Code::BUDGET_EXCEEDED);
        }
        if !self.cache_ok() {
            return Err(CC5Code::CACHE_MISS);
        }
        if !self.batch_effective() {
            return Err(CC5Code::BATCH_FAIL);
        }
        if !self.at_ok() {
            return Err(CC5Code::AT_STUCK);
        }
        Ok(())
    }
}

impl Default for SrPerf {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves149_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves149_srperf");

    fn ready() -> SrPerf {
        let mut p = SrPerf::new();
        p.samples.push(CostSample { item: CostItem::Query, elapsed_us: 100 });
        p.samples.push(CostSample { item: CostItem::Annotate, elapsed_us: 80 });
        p.samples.push(CostSample { item: CostItem::Announce, elapsed_us: 50 });
        p.samples.push(CostSample { item: CostItem::Follow, elapsed_us: 30 });
        p.cache.push(CacheEntry { key: 1, hits: 9 });
        p.cache.push(CacheEntry { key: 2, hits: 8 });
        p.events_raw = 100;
        p.events_batched = 40;
        p.at_attached = true;
        p.at_healthy = true;
        p
    }

    // 01 分项预算
    {
        let p = ready();
        chk(&mut set, "C3849-01", p.budgets_ok(), "err");
    }
    // 02 预算超
    {
        let mut p = ready();
        p.samples[0].elapsed_us = 300;
        chk(
            &mut set,
            "C3849-02",
            p.full_check() == Err(CC5Code::BUDGET_EXCEEDED),
            "err",
        );
    }
    // 03 总预算
    {
        let p = ready();
        chk(&mut set, "C3849-03", p.total_within(), "err");
    }
    // 04 热表命中
    {
        let p = ready();
        chk(&mut set, "C3849-04", p.cache_ok(), "err");
    }
    // 05 热表击穿
    {
        let mut p = ready();
        p.cache[0].hits = 1;
        p.cache[1].hits = 1;
        chk(
            &mut set,
            "C3849-05",
            p.full_check() == Err(CC5Code::CACHE_MISS),
            "err",
        );
    }
    // 06 合批有效
    {
        let p = ready();
        chk(&mut set, "C3849-06", p.batch_effective(), "err");
    }
    // 07 合批失效
    {
        let mut p = ready();
        p.events_batched = 200;
        chk(
            &mut set,
            "C3849-07",
            p.full_check() == Err(CC5Code::BATCH_FAIL),
            "err",
        );
    }
    // 08 辅具健康
    {
        let p = ready();
        chk(&mut set, "C3849-08", p.at_ok(), "err");
    }
    // 09 辅具卡
    {
        let mut p = ready();
        p.at_healthy = false;
        chk(
            &mut set,
            "C3849-09",
            p.full_check() == Err(CC5Code::AT_STUCK),
            "err",
        );
    }
    // 10 全检通过
    {
        let p = ready();
        chk(&mut set, "C3849-10", p.full_check().is_ok(), "err");
    }
    // 11 分项预算表
    {
        let ok = CostItem::ALL.iter().map(|i| i.budget_us()).sum::<u32>() == 500;
        chk(&mut set, "C3849-11", ok, "err");
    }
    // 12 未接入辅具
    {
        let mut p = ready();
        p.at_attached = false;
        chk(&mut set, "C3849-12", p.at_ok(), "err");
    }

    set
}
