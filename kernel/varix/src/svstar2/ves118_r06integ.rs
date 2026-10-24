//! VE-F3718 · R06 联调
//!
//! 45 格质量组合矩阵（5 维×3 规模×3 来源）。20 分钟质量剧本。
//! 三故障注入（伪装主题/对抗样本/扫描超时）。同源双端对拍。重演确定性。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xA6xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xA600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CA6Code(pub u16);

impl CA6Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const SOURCE_DRIFT: Self = Self(SEG_BASE + 1);
    pub const DISGUISE_MISS: Self = Self(SEG_BASE + 2);
    pub const COVERAGE_LOW: Self = Self(SEG_BASE + 3);
    pub const NO_OWNER: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::SOURCE_DRIFT => "同源破（P0 红线实测）",
            Self::DISGUISE_MISS => "伪装主题漏网，防线需修正",
            Self::COVERAGE_LOW => "覆盖不达标，需补跑",
            Self::NO_OWNER => "处置无责，需兜底",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 质量维度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualDim {
    Visual,
    A11y,
    Perf,
    Consistency,
    Safety,
}

/// 资产规模
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    Small,
    Medium,
    Large,
}

/// 资产来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Official,
    ThirdParty,
    Imported,
}

/// 矩阵格
#[derive(Debug, Clone, Copy)]
pub struct MatrixCell {
    pub dim: QualDim,
    pub scale: Scale,
    pub source: Source,
    pub passed: bool,
}

/// 故障类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    DisguiseTheme,
    AdversarialSample,
    ScanTimeout,
}

/// 故障注入结果
#[derive(Debug, Clone, Copy)]
pub struct FaultResult {
    pub kind: FaultKind,
    pub detected: bool,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct R06Integration {
    cells: Vec<MatrixCell>,
    faults: Vec<FaultResult>,
    replay_deterministic: bool,
    owner_assigned: bool,
}

impl R06Integration {
    pub fn new() -> Self {
        Self { cells: Vec::new(), faults: Vec::new(), replay_deterministic: true, owner_assigned: true }
    }

    pub fn record_cell(&mut self, cell: MatrixCell) {
        self.cells.push(cell);
    }

    /// 45 格覆盖底线
    pub fn assert_coverage(&self, min_cells: usize) -> Result<(), CA6Code> {
        if self.cells.len() >= min_cells {
            Ok(())
        } else {
            Err(CA6Code::COVERAGE_LOW)
        }
    }

    pub fn record_fault(&mut self, fr: FaultResult) {
        self.faults.push(fr);
    }

    /// 三故障防线：伪装漏网
    pub fn assert_fault_defense(&self) -> Result<(), CA6Code> {
        if self.faults.iter().any(|f| !f.detected) {
            Err(CA6Code::DISGUISE_MISS)
        } else {
            Ok(())
        }
    }

    /// 同源双端对拍（同输入同输出，此处以 replay 确定性代表）
    pub fn assert_same_source(&self) -> Result<(), CA6Code> {
        if self.replay_deterministic {
            Ok(())
        } else {
            Err(CA6Code::SOURCE_DRIFT)
        }
    }

    /// 重演确定性
    pub fn set_replay(&mut self, det: bool) {
        self.replay_deterministic = det;
    }

    /// 处置兜底
    pub fn assert_owner(&self) -> Result<(), CA6Code> {
        if self.owner_assigned {
            Ok(())
        } else {
            Err(CA6Code::NO_OWNER)
        }
    }

    /// 三分流统计
    pub fn triage(&self) -> (usize, usize, usize) {
        let pass = self.cells.iter().filter(|c| c.passed).count();
        let fail = self.cells.iter().filter(|c| !c.passed).count();
        (pass, fail, self.faults.len())
    }
}

impl Default for R06Integration {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves118_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves118_r06integ");

    // 01 覆盖不达标
    {
        let r = R06Integration::new();
        chk(&mut set, "C3718-01", r.assert_coverage(45) == Err(CA6Code::COVERAGE_LOW), "err");
    }
    // 02 覆盖达标
    {
        let mut r = R06Integration::new();
        for i in 0..45 {
            r.record_cell(MatrixCell { dim: QualDim::Visual, scale: Scale::Small, source: Source::Official, passed: i % 2 == 0 });
        }
        chk(&mut set, "C3718-02", r.assert_coverage(45).is_ok(), "err");
    }
    // 03 伪装漏网
    {
        let mut r = R06Integration::new();
        r.record_fault(FaultResult { kind: FaultKind::DisguiseTheme, detected: false });
        chk(&mut set, "C3718-03", r.assert_fault_defense() == Err(CA6Code::DISGUISE_MISS), "err");
    }
    // 04 三故障全捕
    {
        let mut r = R06Integration::new();
        r.record_fault(FaultResult { kind: FaultKind::DisguiseTheme, detected: true });
        r.record_fault(FaultResult { kind: FaultKind::AdversarialSample, detected: true });
        r.record_fault(FaultResult { kind: FaultKind::ScanTimeout, detected: true });
        chk(&mut set, "C3718-04", r.assert_fault_defense().is_ok(), "err");
    }
    // 05 同源破
    {
        let mut r = R06Integration::new();
        r.set_replay(false);
        chk(&mut set, "C3718-05", r.assert_same_source() == Err(CA6Code::SOURCE_DRIFT), "err");
    }
    // 06 同源正常
    {
        let r = R06Integration::new();
        chk(&mut set, "C3718-06", r.assert_same_source().is_ok(), "err");
    }
    // 07 处置兜底
    {
        let mut r = R06Integration::new();
        r.owner_assigned = false;
        chk(&mut set, "C3718-07", r.assert_owner() == Err(CA6Code::NO_OWNER), "err");
    }
    // 08 三分流
    {
        let mut r = R06Integration::new();
        r.record_cell(MatrixCell { dim: QualDim::Visual, scale: Scale::Small, source: Source::Official, passed: true });
        r.record_cell(MatrixCell { dim: QualDim::A11y, scale: Scale::Medium, source: Source::Imported, passed: false });
        r.record_fault(FaultResult { kind: FaultKind::ScanTimeout, detected: true });
        let (p, f, fa) = r.triage();
        chk(&mut set, "C3718-08", p == 1 && f == 1 && fa == 1, "err");
    }
    // 09 重演确定
    {
        let mut r = R06Integration::new();
        r.set_replay(true);
        chk(&mut set, "C3718-09", r.assert_same_source().is_ok(), "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3718-10", !CA6Code::SOURCE_DRIFT.explain().is_empty(), "err");
    }

    set
}
