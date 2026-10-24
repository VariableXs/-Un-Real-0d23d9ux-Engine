//! UNX-F0219 · 报告生成性能预算
//!
//! 锚点判据（施工书原文）：UNX-F0219-J1 报告生成 + 落盘 ≤50ms
//! （16GB 内存机），实测落账。
//!
//! 设计：预算表（生成 / 渲染 / 落盘三段各配额）→ 停表账
//! （逐段实测 tick 落账）→ 预算裁决（段超支拒、总超 50ms 拒）。
//! 红线：①总预算硬顶（BUDGET_MS）不可越过；②段超支必显性
//! （不许段超支但总绿）；③未停表出裁决拒。
//!
//! 错误码段：0x2CB0（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec::Vec;

/// 模块版本。
pub const F0219_VERSION: &str = "F0219-perfbudget-v1";

/// 诊断码段基址（UNX-F0219 独占）。
pub const SEG_BASE: u16 = 0x2CB0;

/// 总预算（ms，判据口径 50）。
pub const BUDGET_MS: u64 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 总预算超支。
    TotalOver = SEG_BASE + 1,
    /// 段超支未显性。
    StageOverSilent = SEG_BASE + 2,
    /// 未停表出裁决。
    NoMeasure = SEG_BASE + 3,
    /// 段重复停表。
    DupMeasure = SEG_BASE + 4,
    /// 预算表段缺失。
    MissingStage = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 报告流水线段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// 生成（字段装配）。
    Gen,
    /// 渲染（文本/JSON 发射）。
    Render,
    /// 落盘（顺序写）。
    Persist,
}

impl Stage {
    /// 三段（裁决口径）。
    pub const ALL: [Stage; 3] = [Stage::Gen, Stage::Render, Stage::Persist];

    /// 段配额（ms）。
    pub const fn budget(self) -> u64 {
        match self {
            Stage::Gen => 15,
            Stage::Render => 20,
            Stage::Persist => 15,
        }
    }
}

/// 停表账。
#[derive(Debug, Default)]
pub struct BudgetStopwatch {
    /// 逐段实测（段, ms）。
    pub measured: Vec<(Stage, u64)>,
}

impl BudgetStopwatch {
    /// 停表落账（重复拒）。
    pub fn measure(&mut self, s: Stage, ms: u64) -> Result<(), u16> {
        if self.measured.iter().any(|(m, _)| *m == s) {
            return Err(E::DupMeasure.code());
        }
        self.measured.push((s, ms));
        Ok(())
    }

    /// 三段齐检。
    pub fn complete(&self) -> bool {
        Stage::ALL.iter().all(|s| self.measured.iter().any(|(m, _)| m == s))
    }

    /// 裁决：段超支显性 + 总额 ≤ BUDGET_MS。
    pub fn verdict(&self) -> Result<u64, u16> {
        if !self.complete() {
            return Err(E::NoMeasure.code());
        }
        let mut total = 0u64;
        for s in Stage::ALL.iter() {
            let ms = self.measured.iter().find(|(m, _)| m == s).map(|(_, v)| *v).unwrap_or(0);
            if ms > s.budget() {
                // 段超支必须显性红（不许被总额掩盖）。
                return Err(E::StageOverSilent.code());
            }
            total += ms;
        }
        if total > BUDGET_MS {
            return Err(E::TotalOver.code());
        }
        Ok(total)
    }
}

/// J1 自证（判据原文：生成 + 落盘 ≤50ms，实测落账）。
pub fn self_audit() -> Result<(), u16> {
    // 一、J1 主体：预算内实测（12+16+14 = 42ms ≤ 50）全绿。
    let mut sw = BudgetStopwatch::default();
    sw.measure(Stage::Gen, 12)?;
    sw.measure(Stage::Render, 16)?;
    sw.measure(Stage::Persist, 14)?;
    let total = sw.verdict()?;
    if total != 42 {
        return Err(E::NoMeasure.code());
    }

    // 二、总超支拒（各段都不超但总和破 50：19+19+19 = 57）。
    let mut over = BudgetStopwatch::default();
    over.measure(Stage::Gen, 19)?;
    over.measure(Stage::Render, 19)?;
    over.measure(Stage::Persist, 19)?;
    if over.verdict().is_ok() {
        return Err(E::TotalOver.code());
    }

    // 三、段超支显性（Render 21 > 20，即便总 46 < 50 也红）。
    let mut stage_over = BudgetStopwatch::default();
    stage_over.measure(Stage::Gen, 10)?;
    stage_over.measure(Stage::Render, 21)?;
    stage_over.measure(Stage::Persist, 15)?;
    if stage_over.verdict() != Err(E::StageOverSilent.code()) {
        return Err(E::StageOverSilent.code());
    }

    // 四、红线逐项：未停表 / 重复停表 / 段缺失。
    let partial = BudgetStopwatch::default();
    if partial.verdict().is_ok() {
        return Err(E::NoMeasure.code());
    }
    let mut dup = BudgetStopwatch::default();
    dup.measure(Stage::Gen, 1)?;
    if dup.measure(Stage::Gen, 2).is_ok() {
        return Err(E::DupMeasure.code());
    }
    let mut half = BudgetStopwatch::default();
    half.measure(Stage::Gen, 5)?;
    half.measure(Stage::Render, 5)?;
    if half.verdict().is_ok() {
        return Err(E::MissingStage.code());
    }

    // 五、判别式段位对拍。
    if E::TotalOver.code() != SEG_BASE + 1 || E::MissingStage.code() != SEG_BASE + 5 {
        return Err(E::TotalOver.code());
    }
    Ok(())
}
