#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0xDA00）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xDA00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDA0Code(pub u16);

impl CDA0Code {
    pub const ALIGN_GAP: Self = Self(SEG_BASE);        // 对齐缺格
    pub const INSTR_DISTORT: Self = Self(SEG_BASE + 1); // 指令失真
    pub const QUALITY_FAIL: Self = Self(SEG_BASE + 2);  // 品质不达标
    pub const OVERHEAD_OVER: Self = Self(SEG_BASE + 3); // 开销反超
    pub const EQUIV_FAIL: Self = Self(SEG_BASE + 4);    // 等价失败

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::ALIGN_GAP => "对齐缺格",
            Self::INSTR_DISTORT => "指令失真",
            Self::QUALITY_FAIL => "品质不达标",
            Self::OVERHEAD_OVER => "开销反超",
            Self::EQUIV_FAIL => "等价失败",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::ALIGN_GAP, Self::INSTR_DISTORT, Self::QUALITY_FAIL,
        Self::OVERHEAD_OVER, Self::EQUIV_FAIL,
    ];
}

// ── 八类断言 ────────────────────────────────────────────────────────

/// 八类断言枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssertKind {
    /// 对齐完整。
    AlignComplete,
    /// 决策可解释。
    DecisionExplainable,
    /// 指令正确。
    InstrCorrect,
    /// 语义一致。
    SemanticConsistent,
    /// 品质达标。
    QualityMet,
    /// 开销节省。
    OverheadSaved,
    /// 协同正确。
    CollabCorrect,
    /// 预算合规。
    BudgetCompliant,
}

impl AssertKind {
    pub const ALL: [Self; 8] = [
        Self::AlignComplete, Self::DecisionExplainable, Self::InstrCorrect,
        Self::SemanticConsistent, Self::QualityMet, Self::OverheadSaved,
        Self::CollabCorrect, Self::BudgetCompliant,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::AlignComplete => "对齐完整",
            Self::DecisionExplainable => "决策可解释",
            Self::InstrCorrect => "指令正确",
            Self::SemanticConsistent => "语义一致",
            Self::QualityMet => "品质达标",
            Self::OverheadSaved => "开销节省",
            Self::CollabCorrect => "协同正确",
            Self::BudgetCompliant => "预算合规",
        }
    }
}

/// 断言结果。
#[derive(Debug, Clone)]
pub struct AssertResult {
    pub kind: AssertKind,
    pub passed: bool,
}

// ── 场景矩阵 ────────────────────────────────────────────────────────

/// 策略族。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyFamily { StaticImage, SimplifiedAnim, TextSummary }

impl StrategyFamily {
    pub const ALL: [Self; 3] = [Self::StaticImage, Self::SimplifiedAnim, Self::TextSummary];
}

/// 设备档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceTier { Low, Mid, High }

impl DeviceTier {
    pub const ALL: [Self; 3] = [Self::Low, Self::Mid, Self::High];
}

/// 场景格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioCell {
    pub motion_kind: MotionKind,
    pub strategy: StrategyFamily,
    pub device: DeviceTier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionKind { Fade, Slide, Scale, Rotate, Parallax, Particle }

impl MotionKind {
    pub const ALL: [Self; 6] = [Self::Fade, Self::Slide, Self::Scale, Self::Rotate, Self::Parallax, Self::Particle];
}

// ── 替代等价断言 ────────────────────────────────────────────────────

/// 替代信息等价。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquivAssert {
    pub original_info_units: u32,
    pub alt_info_units: u32,
}

impl EquivAssert {
    /// 等价：替代信息量 ≥80%（复用 F3145 完备）。
    pub fn equivalent(&self) -> bool {
        if self.original_info_units == 0 { return true; }
        self.alt_info_units * 5 >= self.original_info_units * 4
    }
}

// ── 测试生成器 ──────────────────────────────────────────────────────

/// 动效替代测试生成器。
#[derive(Debug, Clone)]
pub struct MotionAltTestGen {
    pub results: Vec<AssertResult>,
    pub scenario_cells: Vec<ScenarioCell>,
}

impl MotionAltTestGen {
    pub fn new() -> Self { Self { results: Vec::new(), scenario_cells: Vec::new() } }

    /// 生成场景矩阵（动效×策略×设备 = 6×3×3=54 格）。
    pub fn gen_matrix(&mut self) {
        self.scenario_cells.clear();
        for mk in MotionKind::ALL {
            for sf in StrategyFamily::ALL {
                for dt in DeviceTier::ALL {
                    self.scenario_cells.push(ScenarioCell { motion_kind: mk, strategy: sf, device: dt });
                }
            }
        }
    }

    /// 矩阵完备性。
    pub fn matrix_complete(&self) -> bool {
        self.scenario_cells.len() == 54
    }

    /// 八类断言全过。
    pub fn all_pass(&self) -> bool {
        self.results.len() == 8 && self.results.iter().all(|r| r.passed)
    }
}

// ── 品质评审 ────────────────────────────────────────────────────────

/// 品质评审（人工抽样）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualityReview {
    pub sample_count: u32,
    pub avg_score_pct: u16,
}

impl QualityReview {
    /// 品质达标：均分 ≥70%。
    pub fn met(&self) -> bool { self.avg_score_pct >= 70 }
}

// ── 开销对比 ────────────────────────────────────────────────────────

/// 开销对比：替代 vs 原始。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverheadCompare {
    pub original_ms: u32,
    pub alt_ms: u32,
}

impl OverheadCompare {
    /// 开销节省：替代 ≤ 原始。
    pub fn saved(&self) -> bool { self.alt_ms <= self.original_ms }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3871_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4i_motalt");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 八类齐全
    {
        chk(&mut set, "C3871-01", AssertKind::ALL.len() == 8, "err");
    }
    // 2 断言名称
    {
        chk(&mut set, "C3871-02", AssertKind::AlignComplete.name() == "对齐完整", "err");
    }
    // 3 等价断言
    {
        let e = EquivAssert { original_info_units: 100, alt_info_units: 85 };
        chk(&mut set, "C3871-03", e.equivalent(), "err");
    }
    // 4 等价不足
    {
        let e = EquivAssert { original_info_units: 100, alt_info_units: 50 };
        chk(&mut set, "C3871-04", !e.equivalent(), "err");
    }
    // 5 零信息等价
    {
        let e = EquivAssert { original_info_units: 0, alt_info_units: 0 };
        chk(&mut set, "C3871-05", e.equivalent(), "err");
    }
    // 6 矩阵生成
    {
        let mut g = MotionAltTestGen::new();
        g.gen_matrix();
        chk(&mut set, "C3871-06", g.matrix_complete(), "err");
    }
    // 7 矩阵格数
    {
        let mut g = MotionAltTestGen::new();
        g.gen_matrix();
        chk(&mut set, "C3871-07", g.scenario_cells.len() == 54, "err");
    }
    // 8 策略三族
    {
        chk(&mut set, "C3871-08", StrategyFamily::ALL.len() == 3, "err");
    }
    // 9 设备三档
    {
        chk(&mut set, "C3871-09", DeviceTier::ALL.len() == 3, "err");
    }
    // 10 动效六类
    {
        chk(&mut set, "C3871-10", MotionKind::ALL.len() == 6, "err");
    }
    // 11 品质达标
    {
        let q = QualityReview { sample_count: 10, avg_score_pct: 80 };
        chk(&mut set, "C3871-11", q.met(), "err");
    }
    // 12 品质不达标
    {
        let q = QualityReview { sample_count: 10, avg_score_pct: 50 };
        chk(&mut set, "C3871-12", !q.met(), "err");
    }
    // 13 开销节省
    {
        let o = OverheadCompare { original_ms: 100, alt_ms: 80 };
        chk(&mut set, "C3871-13", o.saved(), "err");
    }
    // 14 开销反超
    {
        let o = OverheadCompare { original_ms: 100, alt_ms: 150 };
        chk(&mut set, "C3871-14", !o.saved(), "err");
    }
    // 15 断言集全过
    {
        let mut g = MotionAltTestGen::new();
        for k in AssertKind::ALL {
            g.results.push(AssertResult { kind: k, passed: true });
        }
        chk(&mut set, "C3871-15", g.all_pass(), "err");
    }
    // 16 断言集缺项
    {
        let mut g = MotionAltTestGen::new();
        g.results.push(AssertResult { kind: AssertKind::AlignComplete, passed: true });
        chk(&mut set, "C3871-16", !g.all_pass(), "err");
    }
    // 17 等价边界（刚好 80%）
    {
        let e = EquivAssert { original_info_units: 10, alt_info_units: 8 };
        chk(&mut set, "C3871-17", e.equivalent(), "err");
    }
    // 18 等价边界（79%）
    {
        let e = EquivAssert { original_info_units: 100, alt_info_units: 79 };
        chk(&mut set, "C3871-18", !e.equivalent(), "err");
    }

    set
}
