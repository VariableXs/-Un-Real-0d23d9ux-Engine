//! VE-F3714 · 质量测试
//!
//! 扫描引擎自身测试：评分与专家对拍（偏差 >10% 校准立案）、五维×扫描项覆盖矩阵、
//! 对抗测试（伪装合规主题）、扫描幂等（同主题同结果，P1）。

#![allow(dead_code)]

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xA2xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xA200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CA2Code(pub u16);

impl CA2Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const CALIBRATE: Self = Self(SEG_BASE + 1);
    pub const COVERAGE_GAP: Self = Self(SEG_BASE + 2);
    pub const ADVERSARIAL_MISS: Self = Self(SEG_BASE + 3);
    pub const IDEMPOTENCE_BROKEN: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::CALIBRATE => "对拍偏差超过 10%，需校准立案",
            Self::COVERAGE_GAP => "覆盖矩阵缺格，需补齐",
            Self::ADVERSARIAL_MISS => "对抗漏网，防线需修正",
            Self::IDEMPOTENCE_BROKEN => "幂等破坏（P1）：同主题扫描结果不一致",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 专家评审分数
#[derive(Debug, Clone, Copy)]
pub struct ExpertScore {
    pub expert_id: u32,
    pub score: u16,
}

/// 对拍记录
#[derive(Debug, Clone)]
pub struct CalibPair {
    pub engine_score: u16,
    pub expert: ExpertScore,
}

/// 覆盖矩阵（5 维 × N 项）
#[derive(Debug, Clone)]
pub struct CoverageMatrix {
    pub dims: usize,
    pub items: usize,
    covered: Vec<bool>,
}

impl CoverageMatrix {
    pub fn new(dims: usize, items: usize) -> Self {
        let total = dims * items;
        Self { dims, items, covered: alloc::vec![false; total] }
    }

    pub fn mark(&mut self, dim: usize, item: usize) {
        if dim < self.dims && item < self.items {
            self.covered[dim * self.items + item] = true;
        }
    }

    pub fn is_complete(&self) -> bool {
        self.covered.iter().all(|&c| c)
    }

    pub fn gap_count(&self) -> usize {
        self.covered.iter().filter(|&&c| !c).count()
    }
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct QualTest {
    pairs: Vec<CalibPair>,
    matrix: CoverageMatrix,
    adversarial_caught: u32,
    adversarial_total: u32,
    idempotence_broken: bool,
}

impl QualTest {
    pub fn new(dims: usize, items: usize) -> Self {
        Self {
            pairs: Vec::new(),
            matrix: CoverageMatrix::new(dims, items),
            adversarial_caught: 0,
            adversarial_total: 0,
            idempotence_broken: false,
        }
    }

    pub fn add_pair(&mut self, pair: CalibPair) {
        self.pairs.push(pair);
    }

    /// 对拍断言：任一 |engine - expert| / expert > 10% 即立案
    pub fn assert_calibration(&self) -> Result<(), CA2Code> {
        for p in &self.pairs {
            let e = p.expert.score as i32;
            let d = (p.engine_score as i32 - e).abs();
            if e > 0 && d * 10 > e {
                return Err(CA2Code::CALIBRATE);
            }
        }
        Ok(())
    }

    /// 覆盖矩阵断言
    pub fn assert_coverage(&self) -> Result<(), CA2Code> {
        if self.matrix.is_complete() {
            Ok(())
        } else {
            Err(CA2Code::COVERAGE_GAP)
        }
    }

    pub fn mark_covered(&mut self, dim: usize, item: usize) {
        self.matrix.mark(dim, item);
    }

    /// 对抗测试：记录捕获
    pub fn record_adversarial(&mut self, caught: bool) {
        self.adversarial_total += 1;
        if caught {
            self.adversarial_caught += 1;
        }
    }

    /// 对抗漏网 → 防线修正
    pub fn assert_adversarial(&self) -> Result<(), CA2Code> {
        if self.adversarial_total > 0 && self.adversarial_caught < self.adversarial_total {
            Err(CA2Code::ADVERSARIAL_MISS)
        } else {
            Ok(())
        }
    }

    /// 幂等断言 P1
    pub fn set_idempotence_broken(&mut self, broken: bool) {
        self.idempotence_broken = broken;
    }

    pub fn assert_idempotence(&self) -> Result<(), CA2Code> {
        if self.idempotence_broken {
            Err(CA2Code::IDEMPOTENCE_BROKEN)
        } else {
            Ok(())
        }
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves114_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves114_qualtest");

    // 01 对拍偏差 >10%
    {
        let mut t = QualTest::new(5, 4);
        t.add_pair(CalibPair { engine_score: 90, expert: ExpertScore { expert_id: 1, score: 80 } });
        chk(&mut set, "C3714-01", t.assert_calibration() == Err(CA2Code::CALIBRATE), "err");
    }
    // 02 对拍合格
    {
        let mut t = QualTest::new(5, 4);
        t.add_pair(CalibPair { engine_score: 82, expert: ExpertScore { expert_id: 1, score: 80 } });
        chk(&mut set, "C3714-02", t.assert_calibration().is_ok(), "err");
    }
    // 03 覆盖缺格
    {
        let t = QualTest::new(5, 4);
        chk(&mut set, "C3714-03", t.assert_coverage() == Err(CA2Code::COVERAGE_GAP), "err");
    }
    // 04 覆盖满
    {
        let mut t = QualTest::new(2, 2);
        t.mark_covered(0, 0); t.mark_covered(0, 1);
        t.mark_covered(1, 0); t.mark_covered(1, 1);
        chk(&mut set, "C3714-04", t.assert_coverage().is_ok(), "err");
    }
    // 05 对抗漏网
    {
        let mut t = QualTest::new(5, 4);
        t.record_adversarial(true);
        t.record_adversarial(false);
        chk(&mut set, "C3714-05", t.assert_adversarial() == Err(CA2Code::ADVERSARIAL_MISS), "err");
    }
    // 06 对抗全捕
    {
        let mut t = QualTest::new(5, 4);
        t.record_adversarial(true);
        t.record_adversarial(true);
        chk(&mut set, "C3714-06", t.assert_adversarial().is_ok(), "err");
    }
    // 07 幂等 P1
    {
        let mut t = QualTest::new(5, 4);
        t.set_idempotence_broken(true);
        chk(&mut set, "C3714-07", t.assert_idempotence() == Err(CA2Code::IDEMPOTENCE_BROKEN), "err");
    }
    // 08 幂等正常
    {
        let t = QualTest::new(5, 4);
        chk(&mut set, "C3714-08", t.assert_idempotence().is_ok(), "err");
    }
    // 09 gap_count
    {
        let mut t = QualTest::new(3, 3);
        t.mark_covered(0, 0);
        chk(&mut set, "C3714-09", t.matrix.gap_count() == 8, "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3714-10", !CA2Code::CALIBRATE.explain().is_empty(), "err");
    }

    set
}
