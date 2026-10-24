// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves105_perfscan
// VE-F3705 · 主题性能扫描（R06 组 · 主题质量与一致性组）
//
// 锚点：VE-F3705
// 判据原文：成本单源、热点模式、分级标注、低端预警、预估校准、判据。
//
// 难点分析：
//   1. 开销预估复用 F3322 成本模型单源——分叉即归一。
//   2. 热点模式库（大面积模糊/多层视差/高频动画）是反模式清单，命中即列改进项。
//   3. 低端预警走画像：主题在低档设备上的预估表现必须标注。

#![allow(dead_code)]

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9900 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9900;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C99Code(pub u16);

impl C99Code {
    pub const COST_OVER: Self = Self(SEG_BASE);       // 开销超标
    pub const HOTSPOT_FOUND: Self = Self(SEG_BASE + 1); // 热点模式命中
    pub const CALIBRATION_NEEDED: Self = Self(SEG_BASE + 2); // 预估失实须校准
    pub const LOWEND_WARN: Self = Self(SEG_BASE + 3); // 低端预警
    pub const PERF_GRADED: Self = Self(SEG_BASE + 4); // 性能分级完成
    pub const COST_OK: Self = Self(SEG_BASE + 5);     // 开销合规
    pub const NO_HOTSPOT: Self = Self(SEG_BASE + 6);  // 无热点

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::COST_OVER => "主题运行时开销超标",
            Self::HOTSPOT_FOUND => "高开销热点模式命中——反模式清单",
            Self::CALIBRATION_NEEDED => "预估失实——校准红线",
            Self::LOWEND_WARN => "低端设备预警——画像对齐",
            Self::PERF_GRADED => "性能分级完成",
            Self::COST_OK => "开销合规",
            Self::NO_HOTSPOT => "无热点模式",
            _ => "未知诊断",
        }
    }
}

// ── 成本预估 ──────────────────────────────────────────────────────

/// 主题成本项。
pub struct CostItem {
    /// GPU 时间（微秒）。
    pub gpu_us: u32,
    /// 内存（KB）。
    pub mem_kb: u32,
    /// 绘制调用数。
    pub draw_calls: u32,
}

impl CostItem {
    pub fn new(gpu_us: u32, mem_kb: u32, draw_calls: u32) -> Self {
        Self { gpu_us, mem_kb, draw_calls }
    }

    /// 综合开销分（基点）——公开加权：gpu*5 + mem*1 + draws*10。
    pub fn cost_bp(&self) -> u64 {
        (self.gpu_us as u64) * 5 + (self.mem_kb as u64) + (self.draw_calls as u64) * 10
    }
}

/// 成本预估器（单源复用声明：F3322 模型口径）。
pub struct CostEstimator {
    /// 开销上限（基点）。
    pub budget_bp: u64,
    /// 校准偏差。
    last_deviation: u64,
}

impl CostEstimator {
    pub fn standard() -> Self {
        Self { budget_bp: 50000, last_deviation: 0 }
    }

    /// 预估总开销。
    pub fn estimate(&self, items: &[CostItem]) -> u64 {
        items.iter().map(|i| i.cost_bp()).sum()
    }

    /// 预算检查。
    pub fn check_budget(&self, items: &[CostItem]) -> Result<(), C99Code> {
        if self.estimate(items) <= self.budget_bp {
            Ok(())
        } else {
            Err(C99Code::COST_OVER)
        }
    }

    /// 校准：预估 vs 实测偏差 >20% 即须校准。
    pub fn calibrate(&mut self, estimated: u64, actual: u64) -> Result<(), C99Code> {
        let diff = if estimated > actual { estimated - actual } else { actual - estimated };
        self.last_deviation = diff;
        if actual == 0 {
            return Ok(());
        }
        if diff * 5 > actual { // >20%
            return Err(C99Code::CALIBRATION_NEEDED);
        }
        Ok(())
    }
}

// ── 热点模式 ──────────────────────────────────────────────────────

/// 热点模式类别（反模式清单）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HotspotKind {
    /// 大面积模糊。
    LargeBlur,
    /// 多层视差。
    MultiParallax,
    /// 高频动画。
    HighFreqAnim,
}

impl HotspotKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::LargeBlur => "大面积模糊",
            Self::MultiParallax => "多层视差",
            Self::HighFreqAnim => "高频动画",
        }
    }

    pub fn all() -> [HotspotKind; 3] {
        [Self::LargeBlur, Self::MultiParallax, Self::HighFreqAnim]
    }
}

/// 热点扫描器。
pub struct HotspotScanner {
    /// 白名单（合理例外）。
    whitelist: Vec<HotspotKind>,
    hits: u32,
}

impl HotspotScanner {
    pub fn new() -> Self {
        Self { whitelist: Vec::new(), hits: 0 }
    }

    pub fn whitelist(&mut self, k: HotspotKind) {
        if !self.whitelist.contains(&k) {
            self.whitelist.push(k);
        }
    }

    /// 扫描一个主题特征。
    pub fn scan(&mut self, k: HotspotKind) -> Result<(), C99Code> {
        if self.whitelist.contains(&k) {
            return Ok(());
        }
        self.hits = self.hits.saturating_add(1);
        Err(C99Code::HOTSPOT_FOUND)
    }

    pub fn hits(&self) -> u32 {
        self.hits
    }
}

impl Default for HotspotScanner {
    fn default() -> Self {
        Self::new()
    }
}

// ── 性能分级 ──────────────────────────────────────────────────────

/// 性能等级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum PerfGrade {
    /// L1 轻量。
    L1,
    /// L2 中等。
    L2,
    /// L3 偏重。
    L3,
    /// L4 重量级。
    L4,
}

impl PerfGrade {
    pub fn name(self) -> &'static str {
        match self {
            Self::L1 => "L1 轻量",
            Self::L2 => "L2 中等",
            Self::L3 => "L3 偏重",
            Self::L4 => "L4 重量",
        }
    }

    /// 从开销分映射等级。
    pub fn from_cost(cost_bp: u64) -> Self {
        if cost_bp <= 10000 {
            Self::L1
        } else if cost_bp <= 30000 {
            Self::L2
        } else if cost_bp <= 50000 {
            Self::L3
        } else {
            Self::L4
        }
    }

    /// 低端设备是否预警（L3+ 预警）。
    pub fn lowend_warn(self) -> bool {
        matches!(self, Self::L3 | Self::L4)
    }
}

// ── 性能扫描器 ────────────────────────────────────────────────────

/// 性能扫描报告。
pub struct PerfScanReport {
    pub grade: PerfGrade,
    pub cost_bp: u64,
    pub hotspot_count: u32,
    pub lowend_warned: bool,
}

impl PerfScanReport {
    pub fn is_clean(&self) -> bool {
        self.hotspot_count == 0 && !self.lowend_warned && self.grade <= PerfGrade::L2
    }
}

/// 性能扫描器。
pub struct PerfScanner {
    pub estimator: CostEstimator,
    pub hotspot: HotspotScanner,
}

impl PerfScanner {
    pub fn new() -> Self {
        Self {
            estimator: CostEstimator::standard(),
            hotspot: HotspotScanner::new(),
        }
    }

    /// 全量扫描。
    pub fn scan(&mut self, items: &[CostItem]) -> PerfScanReport {
        let cost = self.estimator.estimate(items);
        let grade = PerfGrade::from_cost(cost);
        PerfScanReport {
            grade,
            cost_bp: cost,
            hotspot_count: self.hotspot.hits(),
            lowend_warned: grade.lowend_warn(),
        }
    }
}

impl Default for PerfScanner {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves105_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves105_perfscan");
    {
        // 01 成本计算公开
        let c = CostItem::new(100, 200, 3);
        // 100*5 + 200 + 3*10 = 500+200+30 = 730
        chk(&mut set, "C399-01", c.cost_bp() == 730, "公开加权公式：gpu*5+mem*1+draw*10");
    }
    {
        // 02 开销合规
        let mut est = CostEstimator::standard();
        let items = alloc::vec![CostItem::new(100, 100, 1)];
        chk(&mut set, "C399-02", est.check_budget(&items).is_ok(), "低开销在预算内");
    }
    {
        // 03 开销超标
        let mut est = CostEstimator::standard();
        let items = alloc::vec![CostItem::new(100000, 100000, 100)];
        chk(
            &mut set,
            "C399-03",
            est.check_budget(&items).err() == Some(C99Code::COST_OVER),
            "超高开销超预算",
        );
    }
    {
        // 04 热点命中
        let mut hs = HotspotScanner::new();
        let r = hs.scan(HotspotKind::LargeBlur);
        chk(
            &mut set,
            "C399-04",
            r.err() == Some(C99Code::HOTSPOT_FOUND) && hs.hits() == 1,
            "大面积模糊反模式命中",
        );
    }
    {
        // 05 热点白名单
        let mut hs = HotspotScanner::new();
        hs.whitelist(HotspotKind::MultiParallax);
        let r = hs.scan(HotspotKind::MultiParallax);
        chk(&mut set, "C399-05", r.is_ok() && hs.hits() == 0, "白名单热点放行");
    }
    {
        // 06 分级映射 L1
        chk(&mut set, "C399-06", PerfGrade::from_cost(5000) == PerfGrade::L1, "5000→L1");
    }
    {
        // 07 分级映射 L4+低端预警
        let g = PerfGrade::from_cost(100000);
        chk(
            &mut set,
            "C399-07",
            g == PerfGrade::L4 && g.lowend_warn(),
            "100000→L4 重量级+低端预警",
        );
    }
    {
        // 08 预估校准通过
        let mut est = CostEstimator::standard();
        let r = est.calibrate(10000, 10500); // 偏差 5% ≤20%
        chk(&mut set, "C399-08", r.is_ok(), "偏差 5% 在容差内");
    }
    {
        // 09 预估失实须校准
        let mut est = CostEstimator::standard();
        let r = est.calibrate(10000, 20000); // 偏差 100%
        chk(
            &mut set,
            "C399-09",
            r.err() == Some(C99Code::CALIBRATION_NEEDED),
            "偏差 100% >20%→校准红线",
        );
    }
    {
        // 10 扫描报告干净
        let mut ps = PerfScanner::new();
        let items = alloc::vec![CostItem::new(100, 100, 1)];
        let report = ps.scan(&items);
        chk(&mut set, "C399-10", report.is_clean(), "低开销无热点报告干净");
    }
    {
        // 11 扫描报告热点计数
        let mut ps = PerfScanner::new();
        assert!(ps.hotspot.scan(HotspotKind::HighFreqAnim).is_err());
        let items = alloc::vec![CostItem::new(100, 100, 1)];
        let report = ps.scan(&items);
        chk(
            &mut set,
            "C399-11",
            report.hotspot_count == 1 && !report.is_clean(),
            "热点计数进报告",
        );
    }
    {
        // 12 三级热点全量
        chk(&mut set, "C399-12", HotspotKind::all().len() == 3, "模糊/视差/高频三热点");
    }
    set
}
