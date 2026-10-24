//! VE-F3571 · 深空玻璃演进流程 —— 四步演进 + 历史回溯 + 灰度验证 + 悄悄改检出。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3571`
//!
//! # 职责（锚点原文拆解）
//!
//! - **四步演进**（[`EvolutionStep`]）：变更提案 / 评审 / 灰度 /
//!   全量四步——基线不是圣旨但不能悄悄改。
//! - **历史回溯**（[`BaselineHistory`]）：演进历史可回溯。
//! - **灰度验证**：灰度失败回退基线。
//! - **悄悄改检出**：无提案直接改基线 = 历史比对检出。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 悄悄改 | 历史比对检出 |
//! | 评审缺 | 阻断 |
//! | 灰度失败 | 回退基线 |
//!
//! # 复杂度
//!
//! O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6600 起）
// ---------------------------------------------------------------------------

/// 基线演进诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvoCode {
    /// 悄悄改（无提案变更）。
    SilentChange = 6600,
    /// 评审缺失。
    ReviewMissing = 6601,
    /// 灰度失败。
    GrayFailed = 6602,
}

impl EvoCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            EvoCode::SilentChange => "基线悄悄改已检出",
            EvoCode::ReviewMissing => "评审缺失已阻断",
            EvoCode::GrayFailed => "灰度失败已回退",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, EvoCode::SilentChange | EvoCode::ReviewMissing)
    }
}

// ---------------------------------------------------------------------------
// 二、四步
// ---------------------------------------------------------------------------

/// 演进四步。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvolutionStep {
    /// 提案。
    Propose,
    /// 评审。
    Review,
    /// 灰度。
    Gray,
    /// 全量。
    Full,
}

impl EvolutionStep {
    /// 步序号。
    pub const fn ordinal(self) -> u8 {
        match self {
            EvolutionStep::Propose => 0,
            EvolutionStep::Review => 1,
            EvolutionStep::Gray => 2,
            EvolutionStep::Full => 3,
        }
    }

    /// 步短名。
    pub const fn label(self) -> &'static str {
        match self {
            EvolutionStep::Propose => "提案",
            EvolutionStep::Review => "评审",
            EvolutionStep::Gray => "灰度",
            EvolutionStep::Full => "全量",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、历史库
// ---------------------------------------------------------------------------

/// 历史记录。
#[derive(Clone, Copy, Debug)]
pub struct HistoryEntry {
    /// 版本号。
    pub version: u32,
    /// 是否有合法提案。
    pub had_proposal: bool,
}

/// 基线历史库。
#[derive(Default)]
pub struct BaselineHistory {
    entries: Vec<HistoryEntry>,
}

impl BaselineHistory {
    /// 新建。
    pub fn new() -> Self {
        Self::default()
    }

    /// 记一笔。
    pub fn record(&mut self, version: u32, had_proposal: bool) {
        self.entries.push(HistoryEntry {
            version,
            had_proposal,
        });
    }

    /// 悄悄改检出：无提案的变更数。
    pub fn silent_changes(&self) -> usize {
        self.entries.iter().filter(|e| !e.had_proposal).count()
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 四、演进流
// ---------------------------------------------------------------------------

/// 基线演进流。
pub struct EvolutionFlow {
    step: EvolutionStep,
    history: BaselineHistory,
    /// 当前基线版本。
    pub current_version: u32,
}

impl EvolutionFlow {
    /// 新建（v1 起步）。
    pub fn new() -> Self {
        Self {
            step: EvolutionStep::Propose,
            history: BaselineHistory::new(),
            current_version: 1,
        }
    }

    /// 走一步（步序强制）。
    pub fn advance(&mut self, approved: bool) -> Result<EvolutionStep, EvoCode> {
        match self.step {
            EvolutionStep::Propose => {
                self.step = EvolutionStep::Review;
                Ok(self.step)
            }
            EvolutionStep::Review => {
                if !approved {
                    return Err(EvoCode::ReviewMissing);
                }
                self.step = EvolutionStep::Gray;
                Ok(self.step)
            }
            EvolutionStep::Gray => {
                if !approved {
                    // 灰度失败回退。
                    self.step = EvolutionStep::Propose;
                    return Err(EvoCode::GrayFailed);
                }
                self.step = EvolutionStep::Full;
                Ok(self.step)
            }
            EvolutionStep::Full => {
                // 全量落地：记历史。
                self.history
                    .record(self.current_version.saturating_add(1), true);
                self.current_version = self.current_version.saturating_add(1);
                self.step = EvolutionStep::Propose;
                Ok(EvolutionStep::Full)
            }
        }
    }

    /// 记一次无提案变更（悄悄改检出路径）。
    pub fn record_silent(&mut self, version: u32) {
        self.history.record(version, false);
    }

    /// 悄悄改数。
    pub fn silent_count(&self) -> usize {
        self.history.silent_changes()
    }

    /// 当前步。
    pub fn step(&self) -> EvolutionStep {
        self.step
    }
}

impl Default for EvolutionFlow {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// 基线演进判据集。
pub fn run_evo_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11k");

    // 1. 四步正序。
    let mut f = EvolutionFlow::new();
    let s1 = f.advance(true);
    let s2 = f.advance(true);
    let s3 = f.advance(true);
    let s4 = f.advance(true);
    chk.add(
        "6600-四步演进",
        s1 == Ok(EvolutionStep::Review)
            && s2 == Ok(EvolutionStep::Gray)
            && s3 == Ok(EvolutionStep::Full)
            && s4 == Ok(EvolutionStep::Full),
        "",
    );

    // 2. 评审缺失阻断。
    let mut f2 = EvolutionFlow::new();
    let _ = f2.advance(true);
    chk.add(
        "6601-评审阻断",
        f2.advance(false).err() == Some(EvoCode::ReviewMissing),
        "",
    );

    // 3. 灰度失败回退。
    let mut f3 = EvolutionFlow::new();
    let _ = f3.advance(true);
    let _ = f3.advance(true);
    chk.add(
        "6602-灰度回退",
        f3.advance(false).err() == Some(EvoCode::GrayFailed)
            && f3.step() == EvolutionStep::Propose,
        "",
    );

    // 4. 悄悄改检出。
    let mut f4 = EvolutionFlow::new();
    f4.record_silent(99);
    chk.add("6603-悄悄改检出", f4.silent_count() == 1, "");

    // 5. 版本递增。
    chk.add("6604-版本递增", f.current_version == 2, "");

    // 6. 步名可达。
    chk.add("6605-步名可达", EvolutionStep::Gray.label() == "灰度", "");

    chk
}
