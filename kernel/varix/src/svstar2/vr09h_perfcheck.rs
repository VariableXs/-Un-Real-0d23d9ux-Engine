//! VE-F3768 · R 域性能面终版清点
//!
//! 锚点：R 域性能面终版清点——性能清点（八组性能面清单（复述清单红线：
//! 性能指标必须有基准（基准硬门）；性能分级（P50/P95/P99 三级（复述分级）；
//! 性能对拍（八组性能声明一致（复述对拍）；性能冻结（终版冻结（复述冻结）。
//!
//! 数据结构：性能清单；三级分位；对拍记录；冻结清单。
//! 错误路径与降级矩阵：无基准→拒（硬门实测）；分级缺→补（复述）；对拍分歧→
//! 立案（复述红线）；冻结后改→回滚（复述）。
//! 跨批对接点：八组档案；F3765 对账输入；F3769 安全对端。
//!
//! 诊断码段：独占 0x9Fxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x9Fxx）
// ─────────────────────────────────────────────────────────────

/// F3768 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct N9Code(pub u16);

impl N9Code {
    /// 性能无基准——硬门。
    pub const NO_BASELINE: N9Code = N9Code(0x9F01);
    /// 分位缺失——三级。
    pub const PERCENTILE_MISS: N9Code = N9Code(0x9F02);
    /// 对拍分歧——立案。
    pub const PAIR_DIVERGE: N9Code = N9Code(0x9F03);
    /// 冻结后改——回滚。
    pub const FREEZE_VIOLATE: N9Code = N9Code(0x9F04);
    /// 清单不全——补。
    pub const LIST_INCOMPLETE: N9Code = N9Code(0x9F05);
    /// 回归超限——红线。
    pub const REGRESSION_OVER: N9Code = N9Code(0x9F06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9F01 => "性能指标无基准，硬门拒收。",
            0x9F02 => "P50/P95/P99 分位缺失。",
            0x9F03 => "性能声明对拍分歧，需立案。",
            0x9F04 => "冻结后修改，需回滚。",
            0x9F05 => "性能清单不全。",
            0x9F06 => "性能回归超限，红线不通过。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 性能分位
// ─────────────────────────────────────────────────────────────

/// 分位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Percentile {
    /// P50。
    P50,
    /// P95。
    P95,
    /// P99。
    P99,
}

impl Percentile {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            Percentile::P50 => 0,
            Percentile::P95 => 1,
            Percentile::P99 => 2,
        }
    }

    /// 全集。
    pub fn all() -> [Percentile; 3] {
        [Percentile::P50, Percentile::P95, Percentile::P99]
    }
}

/// 性能条目。
#[derive(Clone, Debug)]
pub struct PerfEntry {
    /// id。
    pub id: u32,
    /// 基准值（μs）。
    pub baseline_us: u32,
    /// P50。
    pub p50_us: u32,
    /// P95。
    pub p95_us: u32,
    /// P99。
    pub p99_us: u32,
    /// 是否有基准。
    pub has_baseline: bool,
    /// 冻结后被改。
    pub changed_after_freeze: bool,
    /// 已冻结。
    pub frozen: bool,
}

impl PerfEntry {
    /// 构造。
    pub fn new(id: u32) -> PerfEntry {
        PerfEntry {
            id,
            baseline_us: 0,
            p50_us: 0,
            p95_us: 0,
            p99_us: 0,
            has_baseline: false,
            changed_after_freeze: false,
            frozen: false,
        }
    }

    /// 基准合规。
    pub fn baseline_ok(&self) -> bool {
        self.has_baseline && self.baseline_us > 0
    }

    /// 分位齐备（P50 ≤ P95 ≤ P99）。
    pub fn percentiles_ok(&self) -> bool {
        self.p50_us > 0 && self.p50_us <= self.p95_us && self.p95_us <= self.p99_us
    }

    /// 回归合规：P99 ≤ 基准 × 1.5（容差 50%）。
    pub fn regression_ok(&self) -> bool {
        if !self.baseline_ok() {
            return false;
        }
        let limit = self.baseline_us + self.baseline_us / 2;
        self.p99_us <= limit
    }

    /// 冻结合规。
    pub fn freeze_ok(&self) -> bool {
        !self.frozen || !self.changed_after_freeze
    }
}

// ─────────────────────────────────────────────────────────────
// 性能清单
// ─────────────────────────────────────────────────────────────

/// 性能清单。
#[derive(Clone, Debug, Default)]
pub struct PerfCatalog {
    entries: Vec<PerfEntry>,
}

impl PerfCatalog {
    /// 空清单。
    pub fn new() -> PerfCatalog {
        PerfCatalog {
            entries: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, e: PerfEntry) {
        self.entries.push(e);
    }

    /// 扫描无基准。
    pub fn scan_no_baseline(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.baseline_ok() {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 扫描分位不全。
    pub fn scan_percentile_miss(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.percentiles_ok() {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 扫描回归超限。
    pub fn scan_regression(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if e.baseline_ok() && !e.regression_ok() {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 扫描冻结违例。
    pub fn scan_freeze_violate(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.freeze_ok() {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 性能对拍
// ─────────────────────────────────────────────────────────────

/// 对拍对。
#[derive(Clone, Debug)]
pub struct PerfPair {
    /// id。
    pub id: u32,
    /// 甲方 P95。
    pub p95_a: u32,
    /// 乙方 P95。
    pub p95_b: u32,
}

impl PerfPair {
    /// 构造。
    pub fn new(id: u32, p95_a: u32, p95_b: u32) -> PerfPair {
        PerfPair { id, p95_a, p95_b }
    }

    /// 一致（容差 20%）。
    pub fn match_ok(&self) -> bool {
        let a = self.p95_a as i64;
        let b = self.p95_b as i64;
        let d = if a > b { a - b } else { b - a };
        let limit = if a > b { a / 5 } else { b / 5 };
        d <= limit
    }
}

/// 对拍表。
#[derive(Clone, Debug, Default)]
pub struct PerfPairLedger {
    pairs: Vec<PerfPair>,
}

impl PerfPairLedger {
    /// 空。
    pub fn new() -> PerfPairLedger {
        PerfPairLedger { pairs: Vec::new() }
    }

    /// 登记。
    pub fn add(&mut self, p: PerfPair) {
        self.pairs.push(p);
    }

    /// 扫描分歧。
    pub fn scan_diverge(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for p in self.pairs.iter() {
            if !p.match_ok() {
                bad.push(p.id);
            }
        }
        bad
    }

    /// 对数。
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3768 判据集。
pub fn run_vr09h_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3768");

    // 0x9F01 基准硬门
    {
        let mut c = PerfCatalog::new();
        c.add(PerfEntry::new(1));
        out.add("0x9F01-无基准检出", c.scan_no_baseline().len() == 1, "");
    }

    // 0x9F02 分位
    {
        let mut e = PerfEntry::new(2);
        e.has_baseline = true;
        e.baseline_us = 1000;
        e.p50_us = 500;
        e.p95_us = 800;
        e.p99_us = 1200;
        out.add("0x9F02-分位有序过", e.percentiles_ok(), "");
        e.p50_us = 0;
        out.add("0x9F02-缺P50检出", !e.percentiles_ok(), "");
    }

    // 0x9F06 回归
    {
        let mut e = PerfEntry::new(3);
        e.has_baseline = true;
        e.baseline_us = 1000;
        e.p50_us = 500;
        e.p95_us = 800;
        e.p99_us = 1200;
        out.add("0x9F06-容差内过", e.regression_ok(), "");
        e.p99_us = 2000;
        out.add("0x9F06-超限检出", !e.regression_ok(), "");
    }

    // 0x9F03 对拍
    {
        let mut l = PerfPairLedger::new();
        l.add(PerfPair::new(1, 1000, 1100));
        out.add("0x9F03-容差内过", l.scan_diverge().is_empty(), "");
        l.add(PerfPair::new(2, 1000, 5000));
        out.add("0x9F03-超容差检出", l.scan_diverge().len() == 1, "");
    }

    // 0x9F04 冻结
    {
        let mut c = PerfCatalog::new();
        let mut e = PerfEntry::new(5);
        e.has_baseline = true;
        e.baseline_us = 100;
        e.frozen = true;
        e.changed_after_freeze = true;
        c.add(e);
        out.add("0x9F04-冻结后改检出", c.scan_freeze_violate().len() == 1, "");
    }

    out
}
