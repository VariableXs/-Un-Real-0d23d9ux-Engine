//! VE-F3772 · R 域预算面终版清点
//!
//! 锚点：R 域预算面终版清点——预算清点（八组预算面清单（复述清单红线：
//! 预算项必须有基线（基线硬门）；预算分级（性能/内存/存储三级（复述分级）；
//! 预算对拍（八组预算声明一致（复述对拍）；预算冻结（终版冻结（复述冻结）。
//!
//! 数据结构：预算清单；三级分类；对拍记录；冻结清单。
//! 错误路径与降级矩阵：无基线→拒（硬门实测）；分级缺→补（复述）；对拍分歧→
//! 立案（复述红线）；冻结后改→回滚（复述）。
//! 跨批对接点：八组档案；F3765 对账输入；F3773 汇总对端。
//!
//! 诊断码段：独占 0xA3xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xA3xx）
// ─────────────────────────────────────────────────────────────

/// F3772 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct P9bCode(pub u16);

impl P9bCode {
    /// 无基线——硬门。
    pub const NO_BASELINE: P9bCode = P9bCode(0xA301);
    /// 分级缺失——三级。
    pub const CLASS_MISS: P9bCode = P9bCode(0xA302);
    /// 对拍分歧——立案。
    pub const PAIR_DIVERGE: P9bCode = P9bCode(0xA303);
    /// 冻结后改——回滚。
    pub const FREEZE_VIOLATE: P9bCode = P9bCode(0xA304);
    /// 清单不全——补。
    pub const LIST_INCOMPLETE: P9bCode = P9bCode(0xA305);
    /// 超支——红线。
    pub const OVER_BUDGET: P9bCode = P9bCode(0xA306);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xA301 => "预算项无基线，硬门拒收。",
            0xA302 => "预算分级缺失（性能/内存/存储）。",
            0xA303 => "预算声明对拍分歧，需立案。",
            0xA304 => "冻结后修改，需回滚。",
            0xA305 => "预算清单不全。",
            0xA306 => "预算超支，红线不通过。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 预算三级
// ─────────────────────────────────────────────────────────────

/// 预算类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetClass {
    /// 性能预算。
    Perf,
    /// 内存预算。
    Mem,
    /// 存储预算。
    Storage,
}

impl BudgetClass {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            BudgetClass::Perf => 0,
            BudgetClass::Mem => 1,
            BudgetClass::Storage => 2,
        }
    }

    /// 全集。
    pub fn all() -> [BudgetClass; 3] {
        [BudgetClass::Perf, BudgetClass::Mem, BudgetClass::Storage]
    }
}

/// 预算条目。
#[derive(Clone, Debug)]
pub struct BudgetEntry {
    /// id。
    pub id: u32,
    /// 类别。
    pub class: BudgetClass,
    /// 基线值（单位：自定义）。
    pub baseline: u32,
    /// 实际值。
    pub actual: u32,
    /// 是否有基线。
    pub has_baseline: bool,
    /// 冻结后被改。
    pub changed_after_freeze: bool,
    /// 已冻结。
    pub frozen: bool,
}

impl BudgetEntry {
    /// 构造。
    pub fn new(id: u32, class: BudgetClass) -> BudgetEntry {
        BudgetEntry {
            id,
            class,
            baseline: 0,
            actual: 0,
            has_baseline: false,
            changed_after_freeze: false,
            frozen: false,
        }
    }

    /// 基线合规。
    pub fn baseline_ok(&self) -> bool {
        self.has_baseline && self.baseline > 0
    }

    /// 未超支。
    pub fn under_budget(&self) -> bool {
        self.baseline > 0 && self.actual <= self.baseline
    }

    /// 冻结合规。
    pub fn freeze_ok(&self) -> bool {
        !self.frozen || !self.changed_after_freeze
    }
}

// ─────────────────────────────────────────────────────────────
// 预算清单
// ─────────────────────────────────────────────────────────────

/// 预算清单。
#[derive(Clone, Debug, Default)]
pub struct BudgetCatalog {
    entries: Vec<BudgetEntry>,
}

impl BudgetCatalog {
    /// 空清单。
    pub fn new() -> BudgetCatalog {
        BudgetCatalog {
            entries: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, e: BudgetEntry) {
        self.entries.push(e);
    }

    /// 扫描无基线。
    pub fn scan_no_baseline(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.baseline_ok() {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 扫描超支。
    pub fn scan_over_budget(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if e.baseline_ok() && !e.under_budget() {
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

    /// 三级齐备。
    pub fn all_classes(&self) -> bool {
        for c in BudgetClass::all() {
            if !self.entries.iter().any(|e| e.class == c) {
                return false;
            }
        }
        true
    }

    /// 缺级。
    pub fn missing_classes(&self) -> Vec<BudgetClass> {
        let mut bad = Vec::new();
        for c in BudgetClass::all() {
            if !self.entries.iter().any(|e| e.class == c) {
                bad.push(c);
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
// 预算对拍
// ─────────────────────────────────────────────────────────────

/// 对拍对。
#[derive(Clone, Debug)]
pub struct BudgetPair {
    /// id。
    pub id: u32,
    /// 甲方基线。
    pub baseline_a: u32,
    /// 乙方基线。
    pub baseline_b: u32,
}

impl BudgetPair {
    /// 构造。
    pub fn new(id: u32, baseline_a: u32, baseline_b: u32) -> BudgetPair {
        BudgetPair {
            id,
            baseline_a,
            baseline_b,
        }
    }

    /// 一致（容差 10%）。
    pub fn match_ok(&self) -> bool {
        let a = self.baseline_a as i64;
        let b = self.baseline_b as i64;
        let d = if a > b { a - b } else { b - a };
        let limit = if a > b { a / 10 } else { b / 10 };
        d <= limit
    }
}

/// 对拍表。
#[derive(Clone, Debug, Default)]
pub struct BudgetPairLedger {
    pairs: Vec<BudgetPair>,
}

impl BudgetPairLedger {
    /// 空。
    pub fn new() -> BudgetPairLedger {
        BudgetPairLedger { pairs: Vec::new() }
    }

    /// 登记。
    pub fn add(&mut self, p: BudgetPair) {
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

/// F3772 判据集。
pub fn run_vr09l_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3772");

    // 0xA301 基线硬门
    {
        let mut c = BudgetCatalog::new();
        c.add(BudgetEntry::new(1, BudgetClass::Perf));
        out.add("0xA301-无基线检出", c.scan_no_baseline().len() == 1, "");
    }

    // 0xA302 三级
    {
        let mut c = BudgetCatalog::new();
        out.add("0xA302-全缺检出", c.missing_classes().len() == 3, "");
        for cl in BudgetClass::all() {
            let mut e = BudgetEntry::new(cl.index() as u32 + 10, cl);
            e.has_baseline = true;
            e.baseline = 1000;
            e.actual = 500;
            c.add(e);
        }
        out.add("0xA302-三级齐过", c.all_classes(), "");
    }

    // 0xA306 超支
    {
        let mut c = BudgetCatalog::new();
        let mut e = BudgetEntry::new(5, BudgetClass::Mem);
        e.has_baseline = true;
        e.baseline = 100;
        e.actual = 200;
        c.add(e);
        out.add("0xA306-超支检出", c.scan_over_budget().len() == 1, "");
    }

    // 0xA303 对拍
    {
        let mut l = BudgetPairLedger::new();
        l.add(BudgetPair::new(1, 1000, 1050));
        out.add("0xA303-容差内过", l.scan_diverge().is_empty(), "");
        l.add(BudgetPair::new(2, 1000, 5000));
        out.add("0xA303-超容差检出", l.scan_diverge().len() == 1, "");
    }

    // 0xA304 冻结
    {
        let mut c = BudgetCatalog::new();
        let mut e = BudgetEntry::new(6, BudgetClass::Perf);
        e.has_baseline = true;
        e.baseline = 100;
        e.frozen = true;
        e.changed_after_freeze = true;
        c.add(e);
        out.add("0xA304-冻结后改检出", c.scan_freeze_violate().len() == 1, "");
    }

    out
}
