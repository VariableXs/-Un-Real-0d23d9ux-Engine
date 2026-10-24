//! VE-F3829 · 高对比性能
//!
//! 高对比渲染开销治理：分项预算（映射 0.1ms / 后处理 0.2ms）。
//! 切换预算红线：全屏高对比切换 ≤200ms。常驻开销降频联动。基准复用。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xB1xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xB100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CB1Code(pub u16);

impl CB1Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const SWITCH_SLOW: Self = Self(SEG_BASE + 1);
    pub const IDLE_EXCEEDED: Self = Self(SEG_BASE + 2);
    pub const ITEM_EXCEEDED: Self = Self(SEG_BASE + 3);
    pub const BENCH_MISS: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::SWITCH_SLOW => "切换超时（红线实测）：>200ms",
            Self::IDLE_EXCEEDED => "常驻超预算，降频联动",
            Self::ITEM_EXCEEDED => "分项超预算，需定位",
            Self::BENCH_MISS => "基准不达，循环复用纪律",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 分项
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetItem {
    Mapping,
    PostProcess,
}

/// 分项预算表
#[derive(Debug, Clone)]
pub struct BudgetTable {
    pub mapping_us: u32,
    pub post_us: u32,
}

impl BudgetTable {
    pub fn standard() -> Self {
        Self { mapping_us: 100, post_us: 200 }
    }
}

/// 切换测量
#[derive(Debug, Clone, Copy)]
pub struct SwitchMeasurement {
    pub ms: u16,
}

/// 常驻开销
#[derive(Debug, Clone, Copy)]
pub struct IdleCost {
    pub us: u32,
    pub budget_us: u32,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct HcPerf {
    table: BudgetTable,
    switch_measurements: Vec<SwitchMeasurement>,
    idle: Vec<IdleCost>,
    bench_score: u16,
    bench_target: u16,
}

impl HcPerf {
    pub fn new(table: BudgetTable) -> Self {
        Self {
            table,
            switch_measurements: Vec::new(),
            idle: Vec::new(),
            bench_score: 0,
            bench_target: 100,
        }
    }

    pub fn record_switch(&mut self, ms: u16) {
        self.switch_measurements.push(SwitchMeasurement { ms });
    }

    /// 切换 200ms 断言
    pub fn assert_switch(&self) -> Result<(), CB1Code> {
        if self.switch_measurements.iter().any(|m| m.ms > 200) {
            Err(CB1Code::SWITCH_SLOW)
        } else {
            Ok(())
        }
    }

    /// 分项断言
    pub fn assert_item(&self, item: BudgetItem, actual_us: u32) -> Result<(), CB1Code> {
        let budget = match item {
            BudgetItem::Mapping => self.table.mapping_us,
            BudgetItem::PostProcess => self.table.post_us,
        };
        if actual_us > budget {
            Err(CB1Code::ITEM_EXCEEDED)
        } else {
            Ok(())
        }
    }

    pub fn record_idle(&mut self, cost: IdleCost) {
        self.idle.push(cost);
    }

    /// 常驻断言
    pub fn assert_idle(&self) -> Result<(), CB1Code> {
        if self.idle.iter().any(|c| c.us > c.budget_us) {
            Err(CB1Code::IDLE_EXCEEDED)
        } else {
            Ok(())
        }
    }

    /// 基准断言
    pub fn set_bench(&mut self, score: u16, target: u16) {
        self.bench_score = score;
        self.bench_target = target;
    }

    pub fn assert_bench(&self) -> Result<(), CB1Code> {
        if self.bench_score >= self.bench_target {
            Ok(())
        } else {
            Err(CB1Code::BENCH_MISS)
        }
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves129_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves129_hcperf");

    // 01 切换超时
    {
        let mut p = HcPerf::new(BudgetTable::standard());
        p.record_switch(300);
        chk(&mut set, "C3829-01", p.assert_switch() == Err(CB1Code::SWITCH_SLOW), "err");
    }
    // 02 切换达标
    {
        let mut p = HcPerf::new(BudgetTable::standard());
        p.record_switch(150);
        chk(&mut set, "C3829-02", p.assert_switch().is_ok(), "err");
    }
    // 03 分项超
    {
        let p = HcPerf::new(BudgetTable::standard());
        chk(&mut set, "C3829-03", p.assert_item(BudgetItem::Mapping, 150) == Err(CB1Code::ITEM_EXCEEDED), "err");
    }
    // 04 分项内
    {
        let p = HcPerf::new(BudgetTable::standard());
        chk(&mut set, "C3829-04", p.assert_item(BudgetItem::Mapping, 80).is_ok(), "err");
    }
    // 05 常驻超
    {
        let mut p = HcPerf::new(BudgetTable::standard());
        p.record_idle(IdleCost { us: 500, budget_us: 300 });
        chk(&mut set, "C3829-05", p.assert_idle() == Err(CB1Code::IDLE_EXCEEDED), "err");
    }
    // 06 常驻内
    {
        let mut p = HcPerf::new(BudgetTable::standard());
        p.record_idle(IdleCost { us: 200, budget_us: 300 });
        chk(&mut set, "C3829-06", p.assert_idle().is_ok(), "err");
    }
    // 07 基准不达
    {
        let mut p = HcPerf::new(BudgetTable::standard());
        p.set_bench(80, 100);
        chk(&mut set, "C3829-07", p.assert_bench() == Err(CB1Code::BENCH_MISS), "err");
    }
    // 08 基准达标
    {
        let mut p = HcPerf::new(BudgetTable::standard());
        p.set_bench(120, 100);
        chk(&mut set, "C3829-08", p.assert_bench().is_ok(), "err");
    }
    // 09 后处理分项
    {
        let p = HcPerf::new(BudgetTable::standard());
        chk(&mut set, "C3829-09", p.assert_item(BudgetItem::PostProcess, 200).is_ok(), "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3829-10", !CB1Code::SWITCH_SLOW.explain().is_empty(), "err");
    }

    set
}
