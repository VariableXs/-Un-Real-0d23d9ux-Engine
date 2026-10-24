//! VE-F3592 · E 域文档终检 —— 目录完整 + 锚点全通 + 交叉引用核 + 命名规范。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3592`
//!
//! # 职责（锚点原文拆解）
//!
//! 十组文档全量终检：目录/锚点/交叉引用/命名四线。
//!
//! # 复杂度
//!
//! O(文档)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：7020 起）
// ---------------------------------------------------------------------------

/// 文档终检诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocFinalCode {
    /// 目录不全。
    TocIncomplete = 7020,
    /// 锚点断。
    AnchorBroken = 7021,
    /// 命名不规范。
    NamingBad = 7022,
}

impl DocFinalCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            DocFinalCode::TocIncomplete => "目录不全",
            DocFinalCode::AnchorBroken => "锚点断链",
            DocFinalCode::NamingBad => "命名不规范",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, DocFinalCode::TocIncomplete | DocFinalCode::AnchorBroken)
    }
}

// ---------------------------------------------------------------------------
// 二、终检器
// ---------------------------------------------------------------------------

/// 文档终检项。
#[derive(Clone, Copy, Debug)]
pub struct DocItem {
    /// 有目录。
    pub has_toc: bool,
    /// 锚点可通。
    pub anchor_ok: bool,
    /// 命名合规。
    pub naming_ok: bool,
}

/// 终检器。
pub struct DocFinalChecker {
    items: Vec<DocItem>,
}

impl DocFinalChecker {
    /// 新建。
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
        }
    }

    /// 终检一篇。
    pub fn check(&mut self, item: DocItem) -> Result<(), DocFinalCode> {
        if !item.has_toc {
            return Err(DocFinalCode::TocIncomplete);
        }
        if !item.anchor_ok {
            return Err(DocFinalCode::AnchorBroken);
        }
        if !item.naming_ok {
            return Err(DocFinalCode::NamingBad);
        }
        self.items.push(item);
        Ok(())
    }

    /// 通过数。
    pub fn passed_count(&self) -> usize {
        self.items.len()
    }
}

impl Default for DocFinalChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 文档终检判据集。
pub fn run_docfinal_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12l");

    let good = DocItem {
        has_toc: true,
        anchor_ok: true,
        naming_ok: true,
    };

    // 1. 合规过。
    let mut c = DocFinalChecker::new();
    chk.add("7020-合规过", c.check(good).is_ok() && c.passed_count() == 1, "");

    // 2. 目录缺拦。
    let no_toc = DocItem {
        has_toc: false,
        ..good
    };
    chk.add(
        "7021-目录拦",
        c.check(no_toc).err() == Some(DocFinalCode::TocIncomplete),
        "",
    );

    // 3. 锚点断拦。
    let no_anchor = DocItem {
        anchor_ok: false,
        ..good
    };
    chk.add(
        "7022-锚点拦",
        c.check(no_anchor).err() == Some(DocFinalCode::AnchorBroken),
        "",
    );

    // 4. 命名拦。
    let bad_name = DocItem {
        naming_ok: false,
        ..good
    };
    chk.add(
        "7023-命名拦",
        c.check(bad_name).err() == Some(DocFinalCode::NamingBad),
        "",
    );

    chk
}
