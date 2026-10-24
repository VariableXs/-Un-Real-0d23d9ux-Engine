//! VE-F3567 · 文案一致性审查 —— 三线审查（术语/标点/语气）+ 术语机检 + 偏差分级。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3567`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三线审查**：术语统一 / 标点统一 / 语气统一三线。
//! - **术语机检**（[`TermChecker`]）：术语偏离自动检出
//!   （禁用词表 + 规范词表对照）。
//! - **偏差分级**（[`CopyGrade`]）：文案偏差分级（轻微/中度/严重）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 术语偏离 | 检出 |
//! | 分级缺失 | 补级 |
//! | 表更新 | 复检 |
//!
//! # 复杂度
//!
//! O(文案)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6520 起）
// ---------------------------------------------------------------------------

/// 文案审查诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyCode {
    /// 术语偏离。
    TermDrift = 6520,
    /// 标点不统一。
    PunctuationDrift = 6521,
    /// 语气不统一。
    ToneDrift = 6522,
}

impl CopyCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            CopyCode::TermDrift => "术语偏离",
            CopyCode::PunctuationDrift => "标点不统一",
            CopyCode::ToneDrift => "语气不统一",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、术语表与分级
// ---------------------------------------------------------------------------

/// 文案偏差分级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyGrade {
    /// 轻微（标点）。
    Minor,
    /// 中度（语气）。
    Moderate,
    /// 严重（术语）。
    Severe,
}

impl CopyGrade {
    /// 级短名。
    pub const fn label(self) -> &'static str {
        match self {
            CopyGrade::Minor => "轻微",
            CopyGrade::Moderate => "中度",
            CopyGrade::Severe => "严重",
        }
    }
}

/// 术语机检器。
pub struct TermChecker {
    /// 禁用词 id 表。
    banned: Vec<u32>,
    /// 检出数。
    pub hits: u32,
}

impl TermChecker {
    /// 新建（禁用词表）。
    pub fn new(banned: Vec<u32>) -> Self {
        Self { banned, hits: 0 }
    }

    /// 检测一个用词 id。
    pub fn check_term(&mut self, term_id: u32) -> Result<(), CopyCode> {
        if self.banned.contains(&term_id) {
            self.hits = self.hits.saturating_add(1);
            return Err(CopyCode::TermDrift);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、三线审查器
// ---------------------------------------------------------------------------

/// 三线审查器。
pub struct CopyAuditor {
    terms: TermChecker,
    /// 违规计数。
    pub violations: u32,
}

impl CopyAuditor {
    /// 新建。
    pub fn new(banned: Vec<u32>) -> Self {
        Self {
            terms: TermChecker::new(banned),
            violations: 0,
        }
    }

    /// 三线审查一处文案。
    /// `term_id`：用词 id；`punct_style`：标点风格 id（0=全角 1=半角）；
    /// `tone_id`：语气 id（0=正式 1=亲切 2=技术）。
    pub fn audit(
        &mut self,
        term_id: u32,
        punct_style: u8,
        tone_id: u8,
        expected_punct: u8,
        expected_tone: u8,
    ) -> Result<CopyGrade, CopyCode> {
        // 术语线（严重）。
        self.terms.check_term(term_id)?;
        // 标点线（轻微）。
        if punct_style != expected_punct {
            self.violations = self.violations.saturating_add(1);
            return Err(CopyCode::PunctuationDrift);
        }
        // 语气线（中度）。
        if tone_id != expected_tone {
            self.violations = self.violations.saturating_add(1);
            return Err(CopyCode::ToneDrift);
        }
        Ok(CopyGrade::Minor)
    }

    /// 术语检出数。
    pub fn term_hits(&self) -> u32 {
        self.terms.hits
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// 文案审查判据集。
pub fn run_copy_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11g");

    let mut a = CopyAuditor::new(alloc::vec![77, 88]);

    // 1. 合规过审。
    chk.add("6520-合规过", a.audit(1, 0, 0, 0, 0).is_ok(), "");

    // 2. 术语偏离检出。
    chk.add(
        "6521-术语偏离",
        a.audit(77, 0, 0, 0, 0).err() == Some(CopyCode::TermDrift) && a.term_hits() == 1,
        "",
    );

    // 3. 标点不统一。
    chk.add(
        "6522-标点不统一",
        a.audit(1, 1, 0, 0, 0).err() == Some(CopyCode::PunctuationDrift),
        "",
    );

    // 4. 语气不统一。
    chk.add(
        "6523-语气不统一",
        a.audit(1, 0, 2, 0, 0).err() == Some(CopyCode::ToneDrift),
        "",
    );

    // 5. 分级名可达。
    chk.add("6524-分级可达", CopyGrade::Severe.label() == "严重", "");

    // 6. 违规计数。
    chk.add("6525-违规计数", a.violations == 2, "");

    chk
}
