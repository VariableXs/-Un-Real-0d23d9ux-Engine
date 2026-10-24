//! VE-F3588 · E 域验收矩阵 —— 十组六件 + 缺格阻断 + 日期强制 + 超龄回退。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3588`
//!
//! # 职责（锚点原文拆解）
//!
//! - **十组×六件矩阵**（[`AcceptMatrix`]）：完成度矩阵。
//! - **缺格阻断 + 日期强制 + 超龄回退**。
//!
//! # 复杂度
//!
//! O(格)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：6940 起）
// ---------------------------------------------------------------------------

/// 验收矩阵诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatrixCode {
    /// 缺格。
    CellMissing = 6940,
    /// 日期缺。
    DateMissing = 6941,
    /// 超龄。
    Overdue = 6942,
}

impl MatrixCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            MatrixCode::CellMissing => "验收矩阵缺格",
            MatrixCode::DateMissing => "结论日期缺失",
            MatrixCode::Overdue => "验收超龄回退",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, MatrixCode::CellMissing | MatrixCode::DateMissing)
    }
}

/// 超龄阈值（天）。
pub const OVERDUE_DAYS: u32 = 90;

// ---------------------------------------------------------------------------
// 二、矩阵
// ---------------------------------------------------------------------------

/// 矩阵格。
#[derive(Clone, Copy, Debug)]
pub struct MatrixCell {
    /// 组（0..=9）。
    pub group: u8,
    /// 件（0..=5）。
    pub artifact: u8,
    /// 结论日期（日序号，0 = 未填）。
    pub date: u32,
    /// 年龄（天）。
    pub age_days: u32,
}

impl MatrixCell {
    /// 格有效（日期已填 + 未超龄）。
    pub const fn valid(&self) -> bool {
        self.date != 0 && self.age_days <= OVERDUE_DAYS
    }
}

/// 验收矩阵（10×6 = 60 格）。
pub struct AcceptMatrix {
    cells: Vec<MatrixCell>,
}

impl AcceptMatrix {
    /// 新建。
    pub fn new() -> Self {
        Self { cells: Vec::new() }
    }

    /// 填格。
    pub fn fill(&mut self, c: MatrixCell) {
        self.cells.push(c);
    }

    /// 已填格数。
    pub fn filled_count(&self) -> usize {
        self.cells.len()
    }

    /// 有效格数。
    pub fn valid_count(&self) -> usize {
        self.cells.iter().filter(|c| c.valid()).count()
    }

    /// 缺格数（60 - 已填）。
    pub fn missing_count(&self) -> usize {
        60usize.saturating_sub(self.cells.len())
    }

    /// 超龄格数。
    pub fn overdue_count(&self) -> usize {
        self.cells.iter().filter(|c| c.age_days > OVERDUE_DAYS).count()
    }

    /// 验收放行（缺格 0 + 日期全有 + 无超龄）。
    pub fn can_accept(&self) -> bool {
        self.missing_count() == 0 && self.valid_count() == 60
    }
}

impl Default for AcceptMatrix {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 验收矩阵判据集。
pub fn run_matrix_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12h");

    // 1. 缺格数。
    let m = AcceptMatrix::new();
    chk.add("6940-缺格数", m.missing_count() == 60, "");

    // 2. 填格。
    let mut m2 = AcceptMatrix::new();
    m2.fill(MatrixCell {
        group: 0,
        artifact: 0,
        date: 100,
        age_days: 10,
    });
    chk.add("6941-填格", m2.filled_count() == 1 && m2.missing_count() == 59, "");

    // 3. 超龄检出。
    let mut m3 = AcceptMatrix::new();
    m3.fill(MatrixCell {
        group: 0,
        artifact: 0,
        date: 100,
        age_days: 100,
    });
    chk.add("6942-超龄检出", m3.overdue_count() == 1, "");

    // 4. 日期缺不合法。
    let no_date = MatrixCell {
        group: 0,
        artifact: 0,
        date: 0,
        age_days: 1,
    };
    chk.add("6943-日期缺", !no_date.valid(), "");

    // 5. 不放行。
    chk.add("6944-缺格不放行", !m2.can_accept(), "");

    // 6. 阈值。
    chk.add("6945-阈值", OVERDUE_DAYS == 90, "");

    chk
}
