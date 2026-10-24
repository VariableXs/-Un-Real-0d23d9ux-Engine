//! VE-F3831 · 高对比测试
//!
//! 八类断言：映射完整/缺色/语义保持/可辨/自动门/字形/覆盖库/切换预算。
//! 54 格场景集（六域×三平台×三档）。色弱用户实测参与红线。真机红线。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xB3xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xB300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CB3Code(pub u16);

impl CB3Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const CELL_MISSING: Self = Self(SEG_BASE + 1);
    pub const GENERATOR_DRIFT: Self = Self(SEG_BASE + 2);
    pub const FAKE_PARTICIPATION: Self = Self(SEG_BASE + 3);
    pub const NO_REAL_DEVICE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::CELL_MISSING => "缺格，需补跑",
            Self::GENERATOR_DRIFT => "生成器漂移，需修复",
            Self::FAKE_PARTICIPATION => "实测形式化，需真参与（红线）",
            Self::NO_REAL_DEVICE => "真机缺失（红线）",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 八类断言
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssertKind {
    MapComplete,
    NoMissingColor,
    SemanticPreserved,
    Distinguishable,
    AutoGate,
    GlyphFidelity,
    LibraryCoverage,
    SwitchBudget,
}

impl AssertKind {
    pub const ALL: [AssertKind; 8] = [
        Self::MapComplete, Self::NoMissingColor, Self::SemanticPreserved,
        Self::Distinguishable, Self::AutoGate, Self::GlyphFidelity,
        Self::LibraryCoverage, Self::SwitchBudget,
    ];
}

/// 场景格
#[derive(Debug, Clone, Copy)]
pub struct ScenarioCell {
    pub domain: u8,
    pub platform: u8,
    pub tier: u8,
    pub passed: bool,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct HcTest {
    asserted: Vec<AssertKind>,
    cells: Vec<ScenarioCell>,
    real_participation: bool,
    real_device: bool,
}

impl HcTest {
    pub fn new() -> Self {
        Self { asserted: Vec::new(), cells: Vec::new(), real_participation: true, real_device: true }
    }

    pub fn record_assert(&mut self, kind: AssertKind) {
        self.asserted.push(kind);
    }

    /// 八类覆盖断言
    pub fn assert_eight_kinds(&self) -> Result<(), CB3Code> {
        for k in AssertKind::ALL {
            if !self.asserted.contains(&k) {
                return Err(CB3Code::CELL_MISSING);
            }
        }
        Ok(())
    }

    pub fn record_cell(&mut self, cell: ScenarioCell) {
        self.cells.push(cell);
    }

    /// 54 格覆盖
    pub fn assert_54_cells(&self, min: usize) -> Result<(), CB3Code> {
        if self.cells.len() >= min {
            Ok(())
        } else {
            Err(CB3Code::CELL_MISSING)
        }
    }

    /// 参与红线
    pub fn assert_participation(&self) -> Result<(), CB3Code> {
        if self.real_participation {
            Ok(())
        } else {
            Err(CB3Code::FAKE_PARTICIPATION)
        }
    }

    /// 真机红线
    pub fn assert_real_device(&self) -> Result<(), CB3Code> {
        if self.real_device {
            Ok(())
        } else {
            Err(CB3Code::NO_REAL_DEVICE)
        }
    }

    pub fn set_participation(&mut self, ok: bool) {
        self.real_participation = ok;
    }

    pub fn set_real_device(&mut self, ok: bool) {
        self.real_device = ok;
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
}

impl Default for HcTest {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves131_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves131_hctest");

    // 01 八类覆盖
    {
        let mut t = HcTest::new();
        for k in AssertKind::ALL {
            t.record_assert(k);
        }
        chk(&mut set, "C3831-01", t.assert_eight_kinds().is_ok(), "err");
    }
    // 02 缺类
    {
        let mut t = HcTest::new();
        t.record_assert(AssertKind::MapComplete);
        chk(&mut set, "C3831-02", t.assert_eight_kinds() == Err(CB3Code::CELL_MISSING), "err");
    }
    // 03 54 格
    {
        let mut t = HcTest::new();
        for i in 0..54 {
            t.record_cell(ScenarioCell { domain: (i % 6) as u8, platform: (i % 3) as u8, tier: (i % 3) as u8, passed: true });
        }
        chk(&mut set, "C3831-03", t.assert_54_cells(54).is_ok(), "err");
    }
    // 04 缺格
    {
        let mut t = HcTest::new();
        t.record_cell(ScenarioCell { domain: 0, platform: 0, tier: 0, passed: true });
        chk(&mut set, "C3831-04", t.assert_54_cells(54) == Err(CB3Code::CELL_MISSING), "err");
    }
    // 05 参与红线
    {
        let mut t = HcTest::new();
        t.set_participation(false);
        chk(&mut set, "C3831-05", t.assert_participation() == Err(CB3Code::FAKE_PARTICIPATION), "err");
    }
    // 06 参与正常
    {
        let t = HcTest::new();
        chk(&mut set, "C3831-06", t.assert_participation().is_ok(), "err");
    }
    // 07 真机红线
    {
        let mut t = HcTest::new();
        t.set_real_device(false);
        chk(&mut set, "C3831-07", t.assert_real_device() == Err(CB3Code::NO_REAL_DEVICE), "err");
    }
    // 08 真机正常
    {
        let t = HcTest::new();
        chk(&mut set, "C3831-08", t.assert_real_device().is_ok(), "err");
    }
    // 09 格计数
    {
        let mut t = HcTest::new();
        t.record_cell(ScenarioCell { domain: 1, platform: 1, tier: 1, passed: false });
        chk(&mut set, "C3831-09", t.cell_count() == 1, "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3831-10", !CB3Code::FAKE_PARTICIPATION.explain().is_empty(), "err");
    }

    set
}
