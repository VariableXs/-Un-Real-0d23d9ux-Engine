//! VE-F3813 · 无障碍渲染调试器（VE-T 域 · 无障碍渲染 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3813`
//!
//! **判据（锚点原文）**：断链扫描、策略解释、家族二十一、零常态、复用单源、判据。
//!
//! **本条是什么**：无障碍渲染调试器——五列仪表（四态当前值/策略命中/
//! 注入开销）；断链扫描器（语义-像素一键扫）；替代预览（reduce 模拟）；
//! 策略解释器（为什么这样渲染）；家族二十一成员协议集成。
//!
//! 诊断码段：独占 0xC6xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xC6xx）
// ─────────────────────────────────────────────────────────────

/// F3813 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C6Code(pub u16);

impl C6Code {
    /// 扫描器漏检——断言修正。
    pub const SCAN_MISS: C6Code = C6Code(0xC601);
    /// 解释失真——修正。
    pub const EXPLAIN_BAD: C6Code = C6Code(0xC602);
    /// 仪表分歧——立案。
    pub const GAUGE_DIFF: C6Code = C6Code(0xC603);
    /// 开销常驻——打开才采样。
    pub const ALWAYS_ON: C6Code = C6Code(0xC604);
    /// 五列缺列。
    pub const COL_MISS: C6Code = C6Code(0xC605);
    /// 协议未接。
    pub const PROTO_MISS: C6Code = C6Code(0xC606);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xC601 => "断链扫描器漏检，需修正断言。",
            0xC602 => "策略解释失真，需修正。",
            0xC603 => "仪表分歧，需立案。",
            0xC604 => "调试开销常驻违规——应打开才采样。",
            0xC605 => "五列仪表缺列。",
            0xC606 => "家族协议未接入。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 五列仪表
// ─────────────────────────────────────────────────────────────

/// 仪表列（五列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GaugeCol {
    /// 辅具态。
    Assistive,
    /// reduce 态。
    Reduce,
    /// 高对比态。
    HighContrast,
    /// 大字号态。
    LargeFont,
    /// 注入开销。
    InjectCost,
}

impl GaugeCol {
    /// 全集。
    pub const ALL: [GaugeCol; 5] = [
        GaugeCol::Assistive,
        GaugeCol::Reduce,
        GaugeCol::HighContrast,
        GaugeCol::LargeFont,
        GaugeCol::InjectCost,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            GaugeCol::Assistive => 0,
            GaugeCol::Reduce => 1,
            GaugeCol::HighContrast => 2,
            GaugeCol::LargeFont => 3,
            GaugeCol::InjectCost => 4,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            GaugeCol::Assistive => "辅具态",
            GaugeCol::Reduce => "reduce态",
            GaugeCol::HighContrast => "高对比态",
            GaugeCol::LargeFont => "大字号态",
            GaugeCol::InjectCost => "注入开销",
        }
    }
}

/// 五列仪表。
#[derive(Clone, Debug)]
pub struct Gauge {
    vals: [u16; 5],
    /// 采样开关（默认关——打开才采样）。
    pub sampling: bool,
}

impl Gauge {
    /// 新建。
    pub const fn new() -> Self {
        Gauge {
            vals: [0; 5],
            sampling: false,
        }
    }

    /// 采样一列。
    pub fn sample(&mut self, c: GaugeCol, v: u16) -> bool {
        if !self.sampling {
            return false;
        }
        self.vals[c.ordinal()] = v;
        true
    }

    /// 打开采样。
    pub fn open(&mut self) {
        self.sampling = true;
    }

    /// 关闭采样（零常态）。
    pub fn close(&mut self) {
        self.sampling = false;
    }

    /// 读一列。
    pub fn read(&self, c: GaugeCol) -> u16 {
        self.vals[c.ordinal()]
    }

    /// 分歧检测（两列状态不一致的代理：任两态列值差 >阈）。
    pub fn diverge(&self) -> bool {
        let a = self.vals[0];
        let b = self.vals[1];
        let c = self.vals[2];
        let d = self.vals[3];
        let base = a;
        b.abs_diff(base) > 100 || c.abs_diff(base) > 100 || d.abs_diff(base) > 100
    }
}

// ─────────────────────────────────────────────────────────────
// 断链扫描器 + 策略解释
// ─────────────────────────────────────────────────────────────

/// 断链扫描结果。
#[derive(Clone, Copy, Debug)]
pub struct ScanHit {
    /// 节点 id。
    pub node: u16,
    /// 断链（语义有、像素无）。
    pub broken: bool,
}

/// 扫描器。
#[derive(Clone, Debug)]
pub struct ChainScanner {
    /// 扫描节点数。
    pub scanned: u32,
    /// 发现断链数。
    pub found: u32,
}

impl ChainScanner {
    /// 新建。
    pub const fn new() -> Self {
        ChainScanner {
            scanned: 0,
            found: 0,
        }
    }

    /// 扫一节点。
    pub fn scan(&mut self, hit: ScanHit) {
        self.scanned += 1;
        if hit.broken {
            self.found += 1;
        }
    }

    /// 零断链。
    pub fn zero_break(&self) -> bool {
        self.found == 0
    }
}

/// 策略解释器。
#[derive(Clone, Debug)]
pub struct PolicyExplain {
    /// 当前策略 id（0=未知）。
    pub policy: u16,
    /// 解释可信。
    pub trustworthy: bool,
}

impl PolicyExplain {
    /// 新建。
    pub const fn new() -> Self {
        PolicyExplain {
            policy: 0,
            trustworthy: true,
        }
    }

    /// 解释（返回是否给出可信解释）。
    pub fn explain(&self) -> bool {
        self.policy != 0 && self.trustworthy
    }

    /// 修正失真。
    pub fn fix(&mut self) {
        self.trustworthy = true;
    }
}

// ─────────────────────────────────────────────────────────────
// 家族二十一成员协议
// ─────────────────────────────────────────────────────────────

/// 协议表（21 成员）。
#[derive(Clone, Debug)]
pub struct FamilyProto {
    ack: [bool; 21],
}

impl FamilyProto {
    /// 新建。
    pub const fn new() -> Self {
        FamilyProto { ack: [false; 21] }
    }

    /// 接入成员（0..21）。
    pub fn enroll(&mut self, i: usize) -> bool {
        if i < 21 {
            self.ack[i] = true;
            true
        } else {
            false
        }
    }

    /// 已接数。
    pub fn count(&self) -> usize {
        self.ack.iter().filter(|a| **a).count()
    }

    /// 全接。
    pub fn complete(&self) -> bool {
        self.count() == 21
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3813 判据集。
pub fn run_f3813_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3813");

    // 0xC604/0xC605 五列仪表
    {
        let mut g = Gauge::new();
        set.add("0xC604-默认不采样", !g.sampling, "");
        set.add("0xC604-未开采样拒", !g.sample(GaugeCol::Reduce, 50), "");
        g.open();
        set.add("0xC605-开采样收", g.sample(GaugeCol::Reduce, 50), "");
        set.add("0xC605-读值", g.read(GaugeCol::Reduce) == 50, "");
        g.close();
        set.add("0xC604-关闭零常态", !g.sampling, "");
    }

    // 0xC603 分歧
    {
        let mut g = Gauge::new();
        g.open();
        g.sample(GaugeCol::Assistive, 100);
        g.sample(GaugeCol::Reduce, 100);
        set.add("0xC603-同值不分歧", !g.diverge(), "");
        g.sample(GaugeCol::Reduce, 300);
        set.add("0xC603-大差分歧", g.diverge(), "");
    }

    // 0xC601 断链扫描
    {
        let mut s = ChainScanner::new();
        s.scan(ScanHit {
            node: 1,
            broken: false,
        });
        set.add("0xC601-零断", s.zero_break(), "");
        s.scan(ScanHit { node: 2, broken: true });
        set.add("0xC601-断检出", !s.zero_break() && s.found == 1, "");
        set.add("0xC601-扫描计数", s.scanned == 2, "");
    }

    // 0xC602 策略解释
    {
        let mut p = PolicyExplain::new();
        set.add("0xC602-未知不解释", !p.explain(), "");
        p.policy = 7;
        set.add("0xC602-有策略解释", p.explain(), "");
        p.trustworthy = false;
        set.add("0xC602-失真不解释", !p.explain(), "");
        p.fix();
        set.add("0xC602-修正后解释", p.explain(), "");
    }

    // 0xC606 家族协议
    {
        let mut f = FamilyProto::new();
        set.add("0xC606-初始零接", f.count() == 0, "");
        for i in 0..21 {
            f.enroll(i);
        }
        set.add("0xC606-二十一齐", f.complete(), "");
        set.add("0xC606-越界拒", !f.enroll(21), "");
    }

    // 列序自洽
    {
        let mut ok = true;
        for (i, c) in GaugeCol::ALL.iter().enumerate() {
            if c.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xC605-列序自洽", ok, "");
    }

    set
}
