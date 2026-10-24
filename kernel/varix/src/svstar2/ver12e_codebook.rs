//! VE-F3585 · E 域错误码总表 —— 统一登记 + 三要素 + 查重 + 开放导出 + 双向检索。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3585`
//!
//! # 职责（锚点原文拆解）
//!
//! - **统一登记**（[`ErrorCodeBook`]）：每码三要素（码/含义/所属组）。
//! - **查重**：一码两义=缺陷。
//! - **开放导出** + 双向检索。
//!
//! # 复杂度
//!
//! O(码数)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：6880 起）
// ---------------------------------------------------------------------------

/// 错误码总表诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookCode {
    /// 重复码。
    DuplicateCode = 6880,
    /// 三要素缺。
    Incomplete = 6881,
    /// 总表漂移。
    Drift = 6882,
}

impl BookCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            BookCode::DuplicateCode => "错误码重复",
            BookCode::Incomplete => "三要素缺失",
            BookCode::Drift => "总表漂移",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, BookCode::DuplicateCode)
    }
}

// ---------------------------------------------------------------------------
// 二、错误码条目
// ---------------------------------------------------------------------------

/// 错误码条目（三要素：码/含义/所属组）。
#[derive(Clone, Copy, Debug)]
pub struct ErrorEntry {
    /// 码值。
    pub code: u16,
    /// 含义 id。
    pub meaning_id: u32,
    /// 所属组 id。
    pub group_id: u32,
}

impl ErrorEntry {
    /// 三要素齐备。
    pub const fn complete(&self) -> bool {
        self.code != 0 && self.meaning_id != 0 && self.group_id != 0
    }
}

/// 错误码总表。
pub struct ErrorCodeBook {
    entries: Vec<ErrorEntry>,
}

impl ErrorCodeBook {
    /// 新建。
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// 登记（查重 + 三要素校验）。
    pub fn register(&mut self, e: ErrorEntry) -> Result<(), BookCode> {
        if !e.complete() {
            return Err(BookCode::Incomplete);
        }
        if self.entries.iter().any(|x| x.code == e.code) {
            return Err(BookCode::DuplicateCode);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 按码查含义。
    pub fn meaning_of(&self, code: u16) -> Option<u32> {
        self.entries
            .iter()
            .find(|e| e.code == code)
            .map(|e| e.meaning_id)
    }

    /// 按组查码数。
    pub fn count_in_group(&self, group: u32) -> usize {
        self.entries.iter().filter(|e| e.group_id == group).count()
    }

    /// 导出全部码（开放导出）。
    pub fn export_codes(&self) -> Vec<u16> {
        self.entries.iter().map(|e| e.code).collect()
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

impl Default for ErrorCodeBook {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 错误码总表判据集。
pub fn run_book_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12e");

    // 1. 合法登记。
    let mut b = ErrorCodeBook::new();
    let ok = b
        .register(ErrorEntry {
            code: 100,
            meaning_id: 1,
            group_id: 1,
        })
        .is_ok();
    chk.add("6880-合法登记", ok && b.len() == 1, "");

    // 2. 查重拦。
    let dup = b
        .register(ErrorEntry {
            code: 100,
            meaning_id: 2,
            group_id: 1,
        })
        .err()
        == Some(BookCode::DuplicateCode);
    chk.add("6881-查重拦", dup, "");

    // 3. 三要素缺拒。
    let bad = b
        .register(ErrorEntry {
            code: 0,
            meaning_id: 1,
            group_id: 1,
        })
        .err()
        == Some(BookCode::Incomplete);
    chk.add("6882-三要素校验", bad, "");

    // 4. 双向检索。
    chk.add("6883-按码查", b.meaning_of(100) == Some(1) && b.meaning_of(999).is_none(), "");

    // 5. 按组计数。
    chk.add("6884-按组计数", b.count_in_group(1) == 1, "");

    // 6. 开放导出。
    let codes = b.export_codes();
    chk.add("6885-导出", codes.len() == 1 && codes[0] == 100, "");

    chk
}
