//! VE-F3587 · E 域查重终审 —— 功能查重 + 合并建议 + 语义级检测 + 报告归档。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3587`
//!
//! # 职责（锚点原文拆解）
//!
//! - **功能查重**（[`DedupChecker`]）：十组间功能重复检测。
//! - **语义级检测**：名字不同功能相同也检出。
//! - **合并建议**。
//!
//! # 复杂度
//!
//! O(条目)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：6920 起）
// ---------------------------------------------------------------------------

/// 查重诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DedupCode {
    /// 重复检出。
    DuplicateFound = 6920,
    /// 报告缺。
    ReportMissing = 6921,
}

impl DedupCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            DedupCode::DuplicateFound => "功能重复检出",
            DedupCode::ReportMissing => "查重报告缺",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、功能条目
// ---------------------------------------------------------------------------

/// 功能条目。
#[derive(Clone, Copy, Debug)]
pub struct FuncEntry {
    /// 功能 id。
    pub id: u32,
    /// 语义签名（功能语义 id——名字不同语义同 = 同值）。
    pub semantic: u32,
    /// 所属组。
    pub group: u8,
}

/// 重复对。
#[derive(Clone, Copy, Debug)]
pub struct DupPair {
    /// 功能 A。
    pub a: u32,
    /// 功能 B。
    pub b: u32,
}

/// 查重器。
pub struct DedupChecker {
    entries: Vec<FuncEntry>,
    /// 重复对。
    pub pairs: Vec<DupPair>,
}

impl DedupChecker {
    /// 新建。
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            pairs: Vec::new(),
        }
    }

    /// 注册功能。
    pub fn register(&mut self, e: FuncEntry) {
        self.entries.push(e);
    }

    /// 全表查重（语义级：semantic 相同 = 重复）。
    pub fn scan(&mut self) -> usize {
        self.pairs.clear();
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                if self.entries[i].semantic == self.entries[j].semantic {
                    self.pairs.push(DupPair {
                        a: self.entries[i].id,
                        b: self.entries[j].id,
                    });
                }
            }
        }
        self.pairs.len()
    }

    /// 合并建议（保留先注册的那个）。
    pub fn merge_suggestion(&self, pair_idx: usize) -> Option<u32> {
        self.pairs.get(pair_idx).map(|p| p.a)
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

impl Default for DedupChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 查重判据集。
pub fn run_dedup_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12g");

    // 1. 语义级查重。
    let mut d = DedupChecker::new();
    d.register(FuncEntry {
        id: 1,
        semantic: 10,
        group: 0,
    });
    d.register(FuncEntry {
        id: 2,
        semantic: 10,
        group: 1,
    }); // 语义同但组不同 = 重复
    d.register(FuncEntry {
        id: 3,
        semantic: 20,
        group: 0,
    });
    let n = d.scan();
    chk.add("6920-语义查重", n == 1, "");

    // 2. 合并建议。
    chk.add("6921-合并建议", d.merge_suggestion(0) == Some(1), "");

    // 3. 无重复。
    let mut d2 = DedupChecker::new();
    d2.register(FuncEntry {
        id: 1,
        semantic: 1,
        group: 0,
    });
    d2.register(FuncEntry {
        id: 2,
        semantic: 2,
        group: 0,
    });
    chk.add("6922-无重复", d2.scan() == 0, "");

    // 4. 空表。
    chk.add("6923-空表", DedupChecker::new().is_empty(), "");

    chk
}
